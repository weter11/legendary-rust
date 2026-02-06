use std::io::{Read, Cursor, Seek, SeekFrom};
use byteorder::{LittleEndian, ReadBytesExt};
use flate2::read::ZlibDecoder;
use std::collections::HashMap;

pub struct Manifest {
    pub files: HashMap<String, FileInfo>,
}

#[derive(Debug, Clone)]
pub struct FileInfo {
    pub filename: String,
    pub hash: Vec<u8>,
}

pub fn parse_manifest(data: &[u8]) -> anyhow::Result<Manifest> {
    let mut cursor = Cursor::new(data);

    let magic = cursor.read_u32::<LittleEndian>()?;
    if magic != 0x44BEC00C {
        return Err(anyhow::anyhow!("Invalid manifest magic: {:x}", magic));
    }

    let _header_size = cursor.read_u32::<LittleEndian>()?;
    let size_uncompressed = cursor.read_u32::<LittleEndian>()?;
    let _size_compressed = cursor.read_u32::<LittleEndian>()?;
    let mut sha_hash = [0u8; 20];
    cursor.read_exact(&mut sha_hash)?;
    let stored_as = cursor.read_u8()?;
    let _version = cursor.read_u32::<LittleEndian>()?;

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
    let cdl_size = body_cursor.read_u32::<LittleEndian>()?;
    body_cursor.seek(SeekFrom::Current(cdl_size as i64 - 4))?;

    // Parse FML (File Manifest List)
    let _fml_start = body_cursor.position();
    let _fml_size = body_cursor.read_u32::<LittleEndian>()?;
    let _fml_version = body_cursor.read_u8()?;
    let count = body_cursor.read_u32::<LittleEndian>()?;

    let mut files = HashMap::new();
    let mut filenames = Vec::new();
    for _ in 0..count {
        filenames.push(read_fstring(&mut body_cursor)?);
    }

    // Skip symlinks
    for _ in 0..count {
        let _ = read_fstring(&mut body_cursor)?;
    }

    // Hashes
    let mut hashes = Vec::new();
    for _ in 0..count {
        let mut hash = vec![0u8; 20];
        body_cursor.read_exact(&mut hash)?;
        hashes.push(hash);
    }

    for (name, hash) in filenames.into_iter().zip(hashes.into_iter()) {
        files.insert(name.clone(), FileInfo { filename: name, hash });
    }

    Ok(Manifest { files })
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

        let s = String::from_utf16(&utf16_data[..utf16_data.len()-1])?;
        Ok(s)
    } else {
        // ASCII
        let byte_count = length as usize;
        let mut buf = vec![0u8; byte_count];
        reader.read_exact(&mut buf)?;
        let s = std::str::from_utf8(&buf[..buf.len()-1])?.to_string();
        Ok(s)
    }
}
