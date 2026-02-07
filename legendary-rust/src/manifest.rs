use std::io::{Read, Cursor, Seek, SeekFrom};
use byteorder::{LittleEndian, ReadBytesExt};
use flate2::read::ZlibDecoder;
use std::collections::HashMap;

pub struct Manifest {
    pub manifest_version: u32,
    pub chunks: HashMap<[u32; 4], ChunkInfo>,
    pub files: HashMap<String, FileManifest>,
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
    let meta_size = body_cursor.read_u32::<LittleEndian>()?;
    body_cursor.seek(SeekFrom::Current(meta_size as i64 - 4))?;

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

    Ok(Manifest { manifest_version, chunks, files })
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
