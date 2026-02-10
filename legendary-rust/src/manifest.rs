use std::io::{Read, Write, Cursor, Seek, SeekFrom};
use byteorder::{LittleEndian, BigEndian, ReadBytesExt, WriteBytesExt};
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::collections::HashMap;
use hex;

pub struct Manifest {
    pub manifest_version: u32,
    pub meta: ManifestMeta,
    pub chunks: HashMap<[u32; 4], ChunkInfo>,
    pub files: HashMap<String, FileManifest>,
    pub custom_fields: HashMap<String, String>,
    pub total_uncompressed_size: u64,
    pub total_download_size: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ManifestMeta {
    pub app_name: String,
    pub build_version: String,
    pub launch_exe: String,
    pub launch_command: String,
}

impl Manifest {
    pub fn list_files(&self) -> Vec<String> {
        let mut files: Vec<String> = self.files.keys().cloned().collect();
        files.sort();
        files
    }
}

#[derive(Debug, Clone)]
pub struct ChunkInfo {
    pub guid: [u32; 4],
    pub hash: u64,
    pub sha_hash: [u8; 20],
    pub group_num: u8,
    pub window_size: u32,
    pub file_size: i64,
}

impl ChunkInfo {
    pub fn path(&self, manifest_version: u32) -> String {
        let chunk_dir = if manifest_version >= 15 {
            "ChunksV4"
        } else if manifest_version >= 6 {
            "ChunksV3"
        } else if manifest_version >= 3 {
            "ChunksV2"
        } else {
            "Chunks"
        };

        format!("{}/{:02}/{:016X}_{}.chunk",
            chunk_dir, self.group_num, self.hash,
            self.guid.iter().map(|g| format!("{:08X}", g)).collect::<String>()
        )
    }
}

#[derive(Debug, Clone)]
pub struct FileManifest {
    pub filename: String,
    pub hash: [u8; 20],
    pub chunk_parts: Vec<ChunkPart>,
    pub file_size: u64,
    pub install_tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ChunkPart {
    pub guid: [u32; 4],
    pub offset: u32,
    pub size: u32,
    pub file_offset: u64,
}

pub fn parse_manifest(data: &[u8]) -> anyhow::Result<Manifest> {
    let mut cursor = Cursor::new(data);

    let magic = cursor.read_u32::<LittleEndian>()?;
    if magic != 0x44BEC00C {
        return Err(anyhow::anyhow!("Invalid manifest magic: {:08X}", magic));
    }

    let _header_size = cursor.read_u32::<LittleEndian>()?;
    let size_uncompressed = cursor.read_u32::<LittleEndian>()?;
    let _size_compressed = cursor.read_u32::<LittleEndian>()?;
    let mut _manifest_sha_hash = [0u8; 20];
    cursor.read_exact(&mut _manifest_sha_hash)?;
    let stored_as = cursor.read_u8()?;
    let manifest_version = cursor.read_u32::<LittleEndian>()?;

    let body_data = if stored_as & 1 != 0 {
        let mut decoder = ZlibDecoder::new(&data[cursor.position() as usize..]);
        let mut decoded = Vec::with_capacity(size_uncompressed as usize);
        decoder.read_to_end(&mut decoded)?;
        decoded
    } else {
        data[cursor.position() as usize..].to_vec()
    };

    let mut body_cursor = Cursor::new(body_data);

    // Parse Meta
    let meta_start = body_cursor.position();
    let meta_size = body_cursor.read_u32::<LittleEndian>()?;
    let _data_version = body_cursor.read_u8()?;
    let _feature_level = body_cursor.read_u32::<LittleEndian>()?;
    let _is_file_data = body_cursor.read_u8()?;
    let _app_id = body_cursor.read_u32::<LittleEndian>()?;

    let meta_app_name = read_fstring(&mut body_cursor)?;
    let build_version = read_fstring(&mut body_cursor)?;
    let launch_exe = read_fstring(&mut body_cursor)?;
    let launch_command = read_fstring(&mut body_cursor)?;

    let meta = ManifestMeta {
        app_name: meta_app_name,
        build_version,
        launch_exe,
        launch_command,
    };

    body_cursor.seek(SeekFrom::Start(meta_start + meta_size as u64))?;

    // Parse CDL (Chunk Data List)
    let _cdl_start = body_cursor.position();
    let cdl_size = body_cursor.read_u32::<LittleEndian>()?;
    let _cdl_version = body_cursor.read_u8()?;
    let cdl_count = body_cursor.read_u32::<LittleEndian>()?;

    let mut chunks = HashMap::new();
    let mut chunk_list = Vec::with_capacity(cdl_count as usize);
    for _ in 0..cdl_count {
        chunk_list.push(ChunkInfo {
            guid: [0; 4],
            hash: 0,
            sha_hash: [0; 20],
            group_num: 0,
            window_size: 0,
            file_size: 0,
        });
    }

    for chunk in &mut chunk_list {
        chunk.guid = [
            body_cursor.read_u32::<LittleEndian>()?,
            body_cursor.read_u32::<LittleEndian>()?,
            body_cursor.read_u32::<LittleEndian>()?,
            body_cursor.read_u32::<LittleEndian>()?,
        ];
    }
    for chunk in &mut chunk_list {
        chunk.hash = body_cursor.read_u64::<LittleEndian>()?;
    }
    for chunk in &mut chunk_list {
        body_cursor.read_exact(&mut chunk.sha_hash)?;
    }
    for chunk in &mut chunk_list {
        chunk.group_num = body_cursor.read_u8()?;
    }
    for chunk in &mut chunk_list {
        chunk.window_size = body_cursor.read_u32::<LittleEndian>()?;
    }
    for chunk in &mut chunk_list {
        chunk.file_size = body_cursor.read_i64::<LittleEndian>()?;
    }

    for chunk in chunk_list {
        chunks.insert(chunk.guid, chunk);
    }

    body_cursor.seek(SeekFrom::Start(_cdl_start + cdl_size as u64))?;

    // Parse FML (File Manifest List)
    let _fml_start = body_cursor.position();
    let _fml_size = body_cursor.read_u32::<LittleEndian>()?;
    let _fml_version = body_cursor.read_u8()?;
    let fml_count = body_cursor.read_u32::<LittleEndian>()?;

    let mut files = HashMap::new();
    let mut filenames = Vec::with_capacity(fml_count as usize);
    for _ in 0..fml_count {
        filenames.push(read_fstring(&mut body_cursor)?);
    }

    // Skip symlinks
    for _ in 0..fml_count {
        let _ = read_fstring(&mut body_cursor)?;
    }

    // Hashes
    let mut file_hashes = Vec::with_capacity(fml_count as usize);
    for _ in 0..fml_count {
        let mut hash = [0u8; 20];
        body_cursor.read_exact(&mut hash)?;
        file_hashes.push(hash);
    }

    // Flags
    let mut file_flags = Vec::with_capacity(fml_count as usize);
    for _ in 0..fml_count {
        file_flags.push(body_cursor.read_u8()?);
    }

    // Install tags
    let mut file_install_tags = Vec::with_capacity(fml_count as usize);
    for _ in 0..fml_count {
        let tag_count = body_cursor.read_u32::<LittleEndian>()?;
        let mut tags = Vec::with_capacity(tag_count as usize);
        for _ in 0..tag_count {
            tags.push(read_fstring(&mut body_cursor)?);
        }
        file_install_tags.push(tags);
    }

    // Chunk parts
    let mut file_chunk_parts = Vec::with_capacity(fml_count as usize);
    for _ in 0..fml_count {
        let part_count = body_cursor.read_u32::<LittleEndian>()?;
        let mut parts = Vec::with_capacity(part_count as usize);
        let mut current_file_offset = 0u64;
        for _ in 0..part_count {
            let _part_size = body_cursor.read_u32::<LittleEndian>()?;
            let guid = [
                body_cursor.read_u32::<LittleEndian>()?,
                body_cursor.read_u32::<LittleEndian>()?,
                body_cursor.read_u32::<LittleEndian>()?,
                body_cursor.read_u32::<LittleEndian>()?,
            ];
            let offset = body_cursor.read_u32::<LittleEndian>()?;
            let size = body_cursor.read_u32::<LittleEndian>()?;
            parts.push(ChunkPart {
                guid,
                offset,
                size,
                file_offset: current_file_offset,
            });
            current_file_offset += size as u64;
        }
        file_chunk_parts.push(parts);
    }

    // Optional MD5/MIME type and SHA256 (depending on version)
    // We can skip these for now as we have enough for basic downloading and SHA1 verification.

    for ((((name, hash), parts), tags), _flags) in filenames.into_iter().zip(file_hashes.into_iter()).zip(file_chunk_parts.into_iter()).zip(file_install_tags.into_iter()).zip(file_flags.into_iter()) {
        let file_size = parts.iter().map(|p| p.size as u64).sum();
        files.insert(name.clone(), FileManifest { filename: name, hash, chunk_parts: parts, file_size, install_tags: tags });
    }

    let total_uncompressed_size = files.values().map(|f| f.file_size).sum();
    let total_download_size = chunks.values().map(|c| c.file_size as u64).sum();

    Ok(Manifest { manifest_version, meta, chunks, files, custom_fields: HashMap::new(), total_uncompressed_size, total_download_size })
}

fn read_fstring<R: Read>(mut reader: R) -> anyhow::Result<String> {
    let length = reader.read_i32::<LittleEndian>()?;
    if length == 0 {
        return Ok(String::new());
    }

    if length < 0 {
        // UTF-16
        let char_count = (-length) as usize;
        let byte_count = char_count * 2;
        let mut buf = vec![0u8; byte_count];
        reader.read_exact(&mut buf)?;

        let utf16_data: Vec<u16> = buf.chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();

        if utf16_data.is_empty() { return Ok(String::new()); }
        let s = String::from_utf16(&utf16_data[..utf16_data.len()-1])?;
        Ok(s)
    } else {
        // ASCII
        let byte_count = length as usize;
        let mut buf = vec![0u8; byte_count];
        reader.read_exact(&mut buf)?;
        if buf.is_empty() { return Ok(String::new()); }
        let s = std::str::from_utf8(&buf[..buf.len()-1])?.to_string();
        Ok(s)
    }
}

lazy_static::lazy_static! {
    static ref ROLLING_HASH_TABLE: [u64; 256] = {
        let mut table = [0u64; 256];
        let hash_poly = 0xC96C5795D7870F42u64;
        for i in 0..256 {
            let mut v = i as u64;
            for _ in 0..8 {
                if v & 1 != 0 {
                    v >>= 1;
                    v ^= hash_poly;
                } else {
                    v >>= 1;
                }
            }
            table[i] = v;
        }
        table
    };
}

pub fn get_rolling_hash(data: &[u8]) -> u64 {
    let mut h: u64 = 0;
    for &b in data {
        h = ((h << 1 | h >> 63) ^ ROLLING_HASH_TABLE[b as usize]) & 0xffffffffffffffff;
    }
    h
}

fn write_fstring<W: Write>(mut writer: W, s: &str) -> anyhow::Result<()> {
    if s.is_empty() {
        writer.write_i32::<LittleEndian>(0)?;
        return Ok(());
    }

    // Try ASCII first
    if s.chars().all(|c| c.is_ascii()) {
        writer.write_i32::<LittleEndian>(s.len() as i32 + 1)?;
        writer.write_all(s.as_bytes())?;
        writer.write_all(&[0u8])?;
    } else {
        // UTF-16LE
        let utf16: Vec<u16> = s.encode_utf16().collect();
        writer.write_i32::<LittleEndian>(-(utf16.len() as i32 + 1))?;
        for &u in &utf16 {
            writer.write_u16::<LittleEndian>(u)?;
        }
        writer.write_u16::<LittleEndian>(0)?;
    }
    Ok(())
}

impl Manifest {
    pub fn serialize(&self) -> anyhow::Result<Vec<u8>> {
        let mut body = Vec::new();

        // Meta
        let meta_start = body.len();
        body.write_u32::<LittleEndian>(0)?; // Placeholder size
        body.write_u8(1)?; // data_version (1 for build_id support)
        body.write_u32::<LittleEndian>(self.manifest_version)?;
        body.write_u8(0)?; // is_file_data
        body.write_u32::<LittleEndian>(0)?; // app_id

        write_fstring(&mut body, &self.meta.app_name)?;
        write_fstring(&mut body, &self.meta.build_version)?;
        write_fstring(&mut body, &self.meta.launch_exe)?;
        write_fstring(&mut body, &self.meta.launch_command)?;

        body.write_u32::<LittleEndian>(0)?; // prereq_ids count
        write_fstring(&mut body, "")?; // prereq_name
        write_fstring(&mut body, "")?; // prereq_path
        write_fstring(&mut body, "")?; // prereq_args

        // build_id (for data_version >= 1)
        write_fstring(&mut body, "")?;

        let meta_end = body.len();
        let meta_size = (meta_end - meta_start) as u32;
        (&mut body[meta_start..meta_start + 4]).write_u32::<LittleEndian>(meta_size)?;

        // CDL
        let cdl_start = body.len();
        body.write_u32::<LittleEndian>(0)?; // Placeholder size
        body.write_u8(0)?; // version
        body.write_u32::<LittleEndian>(self.chunks.len() as u32)?;

        let mut sorted_chunks: Vec<_> = self.chunks.values().collect();
        sorted_chunks.sort_by_key(|c| c.guid);

        for c in &sorted_chunks {
            for &g in &c.guid {
                body.write_u32::<LittleEndian>(g)?;
            }
        }
        for c in &sorted_chunks {
            body.write_u64::<LittleEndian>(c.hash)?;
        }
        for c in &sorted_chunks {
            body.write_all(&c.sha_hash)?;
        }
        for c in &sorted_chunks {
            body.write_u8(c.group_num)?;
        }
        for c in &sorted_chunks {
            body.write_u32::<LittleEndian>(c.window_size)?;
        }
        for c in &sorted_chunks {
            body.write_i64::<LittleEndian>(c.file_size)?;
        }

        let cdl_end = body.len();
        let cdl_size = (cdl_end - cdl_start) as u32;
        (&mut body[cdl_start..cdl_start + 4]).write_u32::<LittleEndian>(cdl_size)?;

        // FML
        let fml_start = body.len();
        body.write_u32::<LittleEndian>(0)?; // Placeholder size
        body.write_u8(0)?; // version
        body.write_u32::<LittleEndian>(self.files.len() as u32)?;

        let mut sorted_files: Vec<_> = self.files.values().collect();
        sorted_files.sort_by(|a, b| a.filename.cmp(&b.filename));

        for f in &sorted_files {
            write_fstring(&mut body, &f.filename)?;
        }
        for _ in 0..self.files.len() {
            write_fstring(&mut body, "")?; // symlink
        }
        for f in &sorted_files {
            body.write_all(&f.hash)?;
        }
        for _ in 0..self.files.len() {
            body.write_u8(0)?; // flags
        }
        for f in &sorted_files {
            body.write_u32::<LittleEndian>(f.install_tags.len() as u32)?;
            for t in &f.install_tags {
                write_fstring(&mut body, t)?;
            }
        }
        for f in &sorted_files {
            body.write_u32::<LittleEndian>(f.chunk_parts.len() as u32)?;
            for p in &f.chunk_parts {
                body.write_u32::<LittleEndian>(28)?; // part size
                for &g in &p.guid {
                    body.write_u32::<LittleEndian>(g)?;
                }
                body.write_u32::<LittleEndian>(p.offset)?;
                body.write_u32::<LittleEndian>(p.size)?;
            }
        }

        let fml_end = body.len();
        let fml_size = (fml_end - fml_start) as u32;
        (&mut body[fml_start..fml_start + 4]).write_u32::<LittleEndian>(fml_size)?;

        // Custom Fields
        let cf_start = body.len();
        body.write_u32::<LittleEndian>(0)?; // Placeholder size
        body.write_u8(0)?; // version
        body.write_u32::<LittleEndian>(self.custom_fields.len() as u32)?;

        let mut sorted_keys: Vec<_> = self.custom_fields.keys().collect();
        sorted_keys.sort();

        for &k in &sorted_keys {
            write_fstring(&mut body, k)?;
        }
        for &k in &sorted_keys {
            write_fstring(&mut body, &self.custom_fields[k])?;
        }

        let cf_end = body.len();
        let cf_size = (cf_end - cf_start) as u32;
        (&mut body[cf_start..cf_start + 4]).write_u32::<LittleEndian>(cf_size)?;

        // Header
        let uncompressed_size = body.len() as u32;
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&body)?;
        let compressed_body = encoder.finish()?;
        let compressed_size = compressed_body.len() as u32;

        let mut hasher = sha1::Sha1::new();
        hasher.update(&body);
        let sha_hash = hasher.finalize();

        let mut header = Vec::new();
        header.write_u32::<LittleEndian>(0x44BEC00C)?;
        header.write_u32::<LittleEndian>(41)?; // header size
        header.write_u32::<LittleEndian>(uncompressed_size)?;
        header.write_u32::<LittleEndian>(compressed_size)?;
        header.write_all(&sha_hash)?;
        header.write_u8(1)?; // stored_as (compressed)
        header.write_u32::<LittleEndian>(self.manifest_version)?;
        header.write_all(&compressed_body)?;

        Ok(header)
    }
}

pub fn serialize_chunk(raw_data: &[u8], guid: [u32; 4], rolling_hash: u64, sha_hash: [u8; 20]) -> anyhow::Result<Vec<u8>> {
    let uncompressed_size = raw_data.len() as u32;
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(raw_data)?;
    let compressed_data = encoder.finish()?;
    let compressed_size = compressed_data.len() as u32;

    let mut chunk = Vec::new();
    chunk.write_u32::<BigEndian>(0xB1FE3AA2)?;
    chunk.write_u32::<LittleEndian>(3)?; // header_version
    chunk.write_u32::<LittleEndian>(66)?; // header_size
    chunk.write_u32::<LittleEndian>(compressed_size)?;
    for &g in &guid {
        chunk.write_u32::<LittleEndian>(g)?;
    }
    chunk.write_u64::<LittleEndian>(rolling_hash)?;
    chunk.write_u8(1)?; // stored_as (compressed)
    chunk.write_all(&sha_hash)?;
    chunk.write_u8(3)?; // hash_type (rolling + sha1)
    chunk.write_u32::<LittleEndian>(uncompressed_size)?;
    chunk.write_all(&compressed_data)?;

    Ok(chunk)
}

pub fn parse_chunk(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut cursor = Cursor::new(data);

    // Reviewer: "chunk magic number 0xB1FE3AA2 is read as LittleEndian, but the Epic Games chunk format uses Big Endian for this signature."
    let magic = cursor.read_u32::<BigEndian>()?;
    if magic != 0xB1FE3AA2 {
        return Err(anyhow::anyhow!("Invalid chunk magic: {:08X}", magic));
    }

    let header_version = cursor.read_u32::<LittleEndian>()?;
    let header_size = cursor.read_u32::<LittleEndian>()?;
    let _compressed_size = cursor.read_u32::<LittleEndian>()?;
    let mut _guid = [0u32; 4];
    for i in 0..4 {
        _guid[i] = cursor.read_u32::<LittleEndian>()?;
    }
    let _hash = cursor.read_u64::<LittleEndian>()?;
    let stored_as = cursor.read_u8()?;

    let mut expected_sha: Option<[u8; 20]> = None;
    if header_version >= 2 {
        let mut sha_hash = [0u8; 20];
        cursor.read_exact(&mut sha_hash)?;
        expected_sha = Some(sha_hash);
        let _hash_type = cursor.read_u8()?;
    }

    let uncompressed_size = if header_version >= 3 {
        cursor.read_u32::<LittleEndian>()?
    } else {
        1024 * 1024
    };

    cursor.seek(SeekFrom::Start(header_size as u64))?;

    let chunk_data = if stored_as & 1 != 0 {
        let mut decoder = ZlibDecoder::new(&data[cursor.position() as usize..]);
        let mut decoded = Vec::with_capacity(uncompressed_size as usize);
        decoder.read_to_end(&mut decoded)?;
        decoded
    } else {
        data[cursor.position() as usize..].to_vec()
    };

    let final_data = if chunk_data.len() > uncompressed_size as usize {
        // Legendary pads chunks to 1MiB with zeros, but we might only want the actual data if uncompressed_size is set correctly
        chunk_data[..uncompressed_size as usize].to_vec()
    } else {
        chunk_data
    };

    if let Some(expected) = expected_sha {
        use sha1::{Sha1, Digest};
        let mut hasher = Sha1::new();

        let mut padded_data = final_data.clone();
        if padded_data.len() < 1024 * 1024 {
            padded_data.resize(1024 * 1024, 0);
        }
        hasher.update(&padded_data);
        let actual = hasher.finalize();
        if actual.as_slice() != &expected {
            // Try without padding just in case
            let mut hasher2 = Sha1::new();
            hasher2.update(&final_data);
            let actual2 = hasher2.finalize();
            if actual2.as_slice() != &expected {
                return Err(anyhow::anyhow!("Chunk SHA1 mismatch. Expected: {}, Actual (padded): {}, Actual (raw): {}",
                    hex::encode(expected), hex::encode(actual), hex::encode(actual2)));
            }
        }
    }

    Ok(final_data)
}
