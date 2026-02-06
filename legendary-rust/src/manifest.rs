use anyhow::{anyhow, Result};
use byteorder::{LittleEndian, ReadBytesExt};
use flate2::read::ZlibDecoder;
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::io::{Cursor, Read, Seek};

/// Reads Epic's FString format from the binary stream
/// Handles both ASCII (length > 0) and UTF-16 (length < 0) strings
fn read_fstring<R: Read>(reader: &mut R) -> Result<String> {
    let length = reader.read_i32::<LittleEndian>()?;

    if length < 0 {
        // UTF-16 encoded string
        // Length is negative and represents number of characters, not bytes
        // Each char is 2 bytes, plus 2-byte null terminator
        let byte_length = (length * -2) as usize;
        let mut buffer = vec![0u8; byte_length];
        reader.read_exact(&mut buffer)?;

        // Remove the null terminator (last 2 bytes)
        let string_bytes = &buffer[..buffer.len() - 2];

        // Convert UTF-16LE to String
        let u16_vec: Vec<u16> = string_bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();

        Ok(String::from_utf16(&u16_vec)?)
    } else if length > 0 {
        // ASCII encoded string
        let mut buffer = vec![0u8; length as usize];
        reader.read_exact(&mut buffer)?;

        // Remove null terminator (last byte)
        let string_bytes = &buffer[..buffer.len() - 1];
        Ok(String::from_utf8(string_bytes.to_vec())?)
    } else {
        // Empty string
        Ok(String::new())
    }
}

/// Returns the chunk directory name based on manifest version
pub fn get_chunk_dir(version: u32) -> &'static str {
    if version >= 15 {
        "ChunksV4"
    } else if version >= 6 {
        "ChunksV3"
    } else if version >= 3 {
        "ChunksV2"
    } else {
        "Chunks"
    }
}

/// Main Manifest structure
#[derive(Debug)]
pub struct Manifest {
    pub header_size: u32,
    pub size_compressed: u32,
    pub size_uncompressed: u32,
    pub sha_hash: Vec<u8>,
    pub stored_as: u8,
    pub version: u32,
    pub meta: ManifestMeta,
    pub chunk_data_list: CDL,
    pub file_manifest_list: FML,
    pub custom_fields: CustomFields,
}

impl Manifest {
    const HEADER_MAGIC: u32 = 0x44BEC00C;

    /// Reads and parses a complete manifest from binary data
    pub fn read_all(data: &[u8]) -> Result<Self> {
        let mut cursor = Cursor::new(data);

        // Read header
        let magic = cursor.read_u32::<LittleEndian>()?;
        if magic != Self::HEADER_MAGIC {
            return Err(anyhow!("Invalid manifest magic: {:#X}", magic));
        }

        let header_size = cursor.read_u32::<LittleEndian>()?;
        let size_uncompressed = cursor.read_u32::<LittleEndian>()?;
        let size_compressed = cursor.read_u32::<LittleEndian>()?;

        let mut sha_hash = vec![0u8; 20];
        cursor.read_exact(&mut sha_hash)?;

        let stored_as = cursor.read_u8()?;
        let version = cursor.read_u32::<LittleEndian>()?;

        // Seek to end of header if needed
        let current_pos = cursor.position();
        if current_pos < header_size as u64 {
            cursor.set_position(header_size as u64);
        }

        // Read and decompress body if needed
        let body_data = if stored_as & 0x1 != 0 {
            // Compressed
            let mut decoder = ZlibDecoder::new(&data[cursor.position() as usize..]);
            let mut decompressed = Vec::new();
            decoder.read_to_end(&mut decompressed)?;

            // Verify hash
            let mut hasher = Sha1::new();
            hasher.update(&decompressed);
            let computed_hash = hasher.finalize();

            if computed_hash.as_slice() != sha_hash.as_slice() {
                return Err(anyhow!("SHA1 hash mismatch"));
            }

            decompressed
        } else {
            // Uncompressed
            data[cursor.position() as usize..].to_vec()
        };

        // Parse body
        let mut body_cursor = Cursor::new(&body_data);

        let meta = ManifestMeta::read(&mut body_cursor)?;
        let chunk_data_list = CDL::read(&mut body_cursor, meta.feature_level)?;
        let file_manifest_list = FML::read(&mut body_cursor)?;
        let custom_fields = CustomFields::read(&mut body_cursor)?;

        Ok(Manifest {
            header_size,
            size_compressed,
            size_uncompressed,
            sha_hash,
            stored_as,
            version,
            meta,
            chunk_data_list,
            file_manifest_list,
            custom_fields,
        })
    }

    pub fn compressed(&self) -> bool {
        self.stored_as & 0x1 != 0
    }
}

/// Manifest metadata
#[derive(Debug)]
pub struct ManifestMeta {
    pub meta_size: u32,
    pub data_version: u8,
    pub feature_level: u32,
    pub is_file_data: bool,
    pub app_id: u32,
    pub app_name: String,
    pub build_version: String,
    pub launch_exe: String,
    pub launch_command: String,
    pub prereq_ids: Vec<String>,
    pub prereq_name: String,
    pub prereq_path: String,
    pub prereq_args: String,
    pub build_id: String,
    pub uninstall_action_path: String,
    pub uninstall_action_args: String,
}

impl ManifestMeta {
    fn read<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        let meta_start = if let Ok(cursor) = reader.stream_position() {
            cursor
        } else {
            0
        };

        let meta_size = reader.read_u32::<LittleEndian>()?;
        let data_version = reader.read_u8()?;
        let feature_level = reader.read_u32::<LittleEndian>()?;
        let is_file_data = reader.read_u8()? == 1;
        let app_id = reader.read_u32::<LittleEndian>()?;
        let app_name = read_fstring(reader)?;
        let build_version = read_fstring(reader)?;
        let launch_exe = read_fstring(reader)?;
        let launch_command = read_fstring(reader)?;

        // Read prereq_ids list
        let prereq_count = reader.read_u32::<LittleEndian>()?;
        let mut prereq_ids = Vec::new();
        for _ in 0..prereq_count {
            prereq_ids.push(read_fstring(reader)?);
        }

        let prereq_name = read_fstring(reader)?;
        let prereq_path = read_fstring(reader)?;
        let prereq_args = read_fstring(reader)?;

        // Data version >= 1: build_id
        let build_id = if data_version >= 1 {
            read_fstring(reader)?
        } else {
            String::new()
        };

        // Data version >= 2: uninstall actions
        let (uninstall_action_path, uninstall_action_args) = if data_version >= 2 {
            (read_fstring(reader)?, read_fstring(reader)?)
        } else {
            (String::new(), String::new())
        };

        // Seek to end of meta if we haven't read everything
        if let Ok(current_pos) = reader.stream_position() {
            let size_read = current_pos - meta_start;
            if size_read < meta_size as u64 {
                // Skip remaining bytes
                let mut skip_buf = vec![0u8; (meta_size as u64 - size_read) as usize];
                reader.read_exact(&mut skip_buf)?;
            }
        }

        Ok(ManifestMeta {
            meta_size,
            data_version,
            feature_level,
            is_file_data,
            app_id,
            app_name,
            build_version,
            launch_exe,
            launch_command,
            prereq_ids,
            prereq_name,
            prereq_path,
            prereq_args,
            build_id,
            uninstall_action_path,
            uninstall_action_args,
        })
    }
}

/// Chunk Data List
#[derive(Debug)]
pub struct CDL {
    pub version: u8,
    pub size: u32,
    pub count: u32,
    pub elements: Vec<ChunkInfo>,
    manifest_version: u32,
}

impl CDL {
    fn read<R: Read + Seek>(reader: &mut R, manifest_version: u32) -> Result<Self> {
        let cdl_start = if let Ok(cursor) = reader.stream_position() {
            cursor
        } else {
            0
        };

        let size = reader.read_u32::<LittleEndian>()?;
        let version = reader.read_u8()?;
        let count = reader.read_u32::<LittleEndian>()?;

        // Initialize empty chunk infos
        let mut elements = Vec::new();
        for _ in 0..count {
            elements.push(ChunkInfo::new(manifest_version));
        }

        // Read column-wise data
        // GUIDs (4 u32s each)
        for chunk in elements.iter_mut() {
            let guid_0 = reader.read_u32::<LittleEndian>()?;
            let guid_1 = reader.read_u32::<LittleEndian>()?;
            let guid_2 = reader.read_u32::<LittleEndian>()?;
            let guid_3 = reader.read_u32::<LittleEndian>()?;
            chunk.guid = [guid_0, guid_1, guid_2, guid_3];
        }

        // Hashes (u64 each)
        for chunk in elements.iter_mut() {
            chunk.hash = reader.read_u64::<LittleEndian>()?;
        }

        // SHA hashes (20 bytes each)
        for chunk in elements.iter_mut() {
            let mut sha_hash = [0u8; 20];
            reader.read_exact(&mut sha_hash)?;
            chunk.sha_hash = sha_hash;
        }

        // Group numbers (u8 each)
        for chunk in elements.iter_mut() {
            chunk.group_num = reader.read_u8()?;
        }

        // Window sizes (u32 each)
        for chunk in elements.iter_mut() {
            chunk.window_size = reader.read_u32::<LittleEndian>()?;
        }

        // File sizes (i64 each)
        for chunk in elements.iter_mut() {
            chunk.file_size = reader.read_i64::<LittleEndian>()?;
        }

        // Seek to end of CDL if needed
        if let Ok(current_pos) = reader.stream_position() {
            let size_read = current_pos - cdl_start;
            if size_read < size as u64 {
                let mut skip_buf = vec![0u8; (size as u64 - size_read) as usize];
                reader.read_exact(&mut skip_buf)?;
            }
        }

        Ok(CDL {
            version,
            size,
            count,
            elements,
            manifest_version,
        })
    }

    pub fn get_chunk_by_guid(&self, guid: &str) -> Option<&ChunkInfo> {
        let guid_lower = guid.to_lowercase();
        self.elements
            .iter()
            .find(|chunk| chunk.guid_str() == guid_lower)
    }
}

/// Chunk information
#[derive(Debug)]
pub struct ChunkInfo {
    pub guid: [u32; 4],
    pub hash: u64,
    pub sha_hash: [u8; 20],
    pub group_num: u8,
    pub window_size: u32,
    pub file_size: i64,
    manifest_version: u32,
}

impl ChunkInfo {
    fn new(manifest_version: u32) -> Self {
        ChunkInfo {
            guid: [0; 4],
            hash: 0,
            sha_hash: [0; 20],
            group_num: 0,
            window_size: 0,
            file_size: 0,
            manifest_version,
        }
    }

    pub fn guid_str(&self) -> String {
        format!(
            "{:08x}-{:08x}-{:08x}-{:08x}",
            self.guid[0], self.guid[1], self.guid[2], self.guid[3]
        )
    }

    pub fn guid_num(&self) -> u128 {
        ((self.guid[0] as u128) << 96)
            | ((self.guid[1] as u128) << 64)
            | ((self.guid[2] as u128) << 32)
            | (self.guid[3] as u128)
    }

    pub fn path(&self) -> String {
        format!(
            "{}/{:02}/{:016X}_{}.chunk",
            get_chunk_dir(self.manifest_version),
            self.group_num,
            self.hash,
            format!(
                "{:08X}{:08X}{:08X}{:08X}",
                self.guid[0], self.guid[1], self.guid[2], self.guid[3]
            )
        )
    }
}

/// File Manifest List
#[derive(Debug)]
pub struct FML {
    pub version: u8,
    pub size: u32,
    pub count: u32,
    pub elements: Vec<FileManifest>,
}

impl FML {
    fn read<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        let fml_start = if let Ok(cursor) = reader.stream_position() {
            cursor
        } else {
            0
        };

        let size = reader.read_u32::<LittleEndian>()?;
        let version = reader.read_u8()?;
        let count = reader.read_u32::<LittleEndian>()?;

        // Initialize empty file manifests
        let mut elements = Vec::new();
        for _ in 0..count {
            elements.push(FileManifest::new());
        }

        // Read filenames
        for fm in elements.iter_mut() {
            fm.filename = read_fstring(reader)?;
        }

        // Read symlink targets
        for fm in elements.iter_mut() {
            fm.symlink_target = read_fstring(reader)?;
        }

        // Read SHA1 hashes
        for fm in elements.iter_mut() {
            let mut hash = [0u8; 20];
            reader.read_exact(&mut hash)?;
            fm.hash = hash;
        }

        // Read flags
        for fm in elements.iter_mut() {
            fm.flags = reader.read_u8()?;
        }

        // Read install tags
        for fm in elements.iter_mut() {
            let tag_count = reader.read_u32::<LittleEndian>()?;
            for _ in 0..tag_count {
                fm.install_tags.push(read_fstring(reader)?);
            }
        }

        // Read chunk parts
        for fm in elements.iter_mut() {
            let chunk_part_count = reader.read_u32::<LittleEndian>()?;
            let mut file_offset = 0u32;

            for _ in 0..chunk_part_count {
                let cp_size = reader.read_u32::<LittleEndian>()?;
                let cp_start = reader.stream_position().unwrap_or(0);

                let guid_0 = reader.read_u32::<LittleEndian>()?;
                let guid_1 = reader.read_u32::<LittleEndian>()?;
                let guid_2 = reader.read_u32::<LittleEndian>()?;
                let guid_3 = reader.read_u32::<LittleEndian>()?;
                let offset = reader.read_u32::<LittleEndian>()?;
                let size = reader.read_u32::<LittleEndian>()?;

                let chunk_part = ChunkPart {
                    guid: [guid_0, guid_1, guid_2, guid_3],
                    offset,
                    size,
                    file_offset,
                };

                fm.chunk_parts.push(chunk_part);
                file_offset += size;

                // Skip any remaining bytes in this chunk part
                if let Ok(current_pos) = reader.stream_position() {
                    let bytes_read = current_pos - cp_start;
                    if bytes_read < cp_size as u64 {
                        let mut skip_buf = vec![0u8; (cp_size as u64 - bytes_read) as usize];
                        reader.read_exact(&mut skip_buf)?;
                    }
                }
            }
        }

        // Version 1+: MD5 hash and MIME type
        if version >= 1 {
            for fm in elements.iter_mut() {
                let has_md5 = reader.read_u32::<LittleEndian>()?;
                if has_md5 != 0 {
                    let mut hash_md5 = [0u8; 16];
                    reader.read_exact(&mut hash_md5)?;
                    fm.hash_md5 = Some(hash_md5);
                }
            }

            for fm in elements.iter_mut() {
                fm.mime_type = read_fstring(reader)?;
            }
        }

        // Version 2+: SHA256 hash
        if version >= 2 {
            for fm in elements.iter_mut() {
                let mut hash_sha256 = [0u8; 32];
                reader.read_exact(&mut hash_sha256)?;
                fm.hash_sha256 = Some(hash_sha256);
            }
        }

        // Calculate file sizes
        for fm in elements.iter_mut() {
            fm.file_size = fm.chunk_parts.iter().map(|cp| cp.size as u64).sum();
        }

        // Seek to end of FML if needed
        if let Ok(current_pos) = reader.stream_position() {
            let size_read = current_pos - fml_start;
            if size_read < size as u64 {
                let mut skip_buf = vec![0u8; (size as u64 - size_read) as usize];
                reader.read_exact(&mut skip_buf)?;
            }
        }

        Ok(FML {
            version,
            size,
            count,
            elements,
        })
    }

    pub fn get_file_by_path(&self, path: &str) -> Option<&FileManifest> {
        self.elements.iter().find(|fm| fm.filename == path)
    }
}

/// File manifest entry
#[derive(Debug)]
pub struct FileManifest {
    pub filename: String,
    pub symlink_target: String,
    pub hash: [u8; 20],
    pub flags: u8,
    pub install_tags: Vec<String>,
    pub chunk_parts: Vec<ChunkPart>,
    pub file_size: u64,
    pub hash_md5: Option<[u8; 16]>,
    pub mime_type: String,
    pub hash_sha256: Option<[u8; 32]>,
}

impl FileManifest {
    fn new() -> Self {
        FileManifest {
            filename: String::new(),
            symlink_target: String::new(),
            hash: [0; 20],
            flags: 0,
            install_tags: Vec::new(),
            chunk_parts: Vec::new(),
            file_size: 0,
            hash_md5: None,
            mime_type: String::new(),
            hash_sha256: None,
        }
    }

    pub fn read_only(&self) -> bool {
        self.flags & 0x1 != 0
    }

    pub fn compressed(&self) -> bool {
        self.flags & 0x2 != 0
    }

    pub fn executable(&self) -> bool {
        self.flags & 0x4 != 0
    }
}

/// Chunk part within a file
#[derive(Debug)]
pub struct ChunkPart {
    pub guid: [u32; 4],
    pub offset: u32,
    pub size: u32,
    pub file_offset: u32,
}

impl ChunkPart {
    pub fn guid_str(&self) -> String {
        format!(
            "{:08x}-{:08x}-{:08x}-{:08x}",
            self.guid[0], self.guid[1], self.guid[2], self.guid[3]
        )
    }

    pub fn guid_num(&self) -> u128 {
        ((self.guid[0] as u128) << 96)
            | ((self.guid[1] as u128) << 64)
            | ((self.guid[2] as u128) << 32)
            | (self.guid[3] as u128)
    }
}

/// Custom fields (key-value pairs)
#[derive(Debug)]
pub struct CustomFields {
    pub version: u8,
    pub size: u32,
    pub count: u32,
    pub fields: HashMap<String, String>,
}

impl CustomFields {
    fn read<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        let cf_start = if let Ok(cursor) = reader.stream_position() {
            cursor
        } else {
            0
        };

        let size = reader.read_u32::<LittleEndian>()?;
        let version = reader.read_u8()?;
        let count = reader.read_u32::<LittleEndian>()?;

        // Read keys
        let mut keys = Vec::new();
        for _ in 0..count {
            keys.push(read_fstring(reader)?);
        }

        // Read values
        let mut values = Vec::new();
        for _ in 0..count {
            values.push(read_fstring(reader)?);
        }

        // Combine into HashMap
        let fields: HashMap<String, String> = keys.into_iter().zip(values.into_iter()).collect();

        // Seek to end of custom fields if needed
        if let Ok(current_pos) = reader.stream_position() {
            let size_read = current_pos - cf_start;
            if size_read < size as u64 {
                let mut skip_buf = vec![0u8; (size as u64 - size_read) as usize];
                reader.read_exact(&mut skip_buf)?;
            }
        }

        Ok(CustomFields {
            version,
            size,
            count,
            fields,
        })
    }

    pub fn get(&self, key: &str) -> Option<&String> {
        self.fields.get(key)
    }
}
