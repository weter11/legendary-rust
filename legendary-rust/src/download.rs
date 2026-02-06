use std::io::{Read, Write, Cursor, Seek, SeekFrom};
use byteorder::{LittleEndian, ReadBytesExt};
use flate2::read::ZlibDecoder;
use anyhow::Result;
use crate::manifest::Manifest;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use crate::app::WorkerResponse;

pub struct Downloader {
    client: reqwest::blocking::Client,
    base_url: String,
    tx: Sender<WorkerResponse>,
    cancel: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
}

impl Downloader {
    pub fn new(base_url: String, tx: Sender<WorkerResponse>, cancel: Arc<AtomicBool>, pause: Arc<AtomicBool>) -> Self {
        let client = reqwest::blocking::Client::builder()
            .user_agent("UELauncher/11.0.1-14907503+++Portal+Release-Live Windows/10.0.19041.1.256.64bit")
            .build()
            .unwrap();

        Self {
            client,
            base_url,
            tx,
            cancel,
            pause,
        }
    }

    fn check_status(&self) -> bool {
        while self.pause.load(Ordering::SeqCst) {
            if self.cancel.load(Ordering::SeqCst) { return true; }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        self.cancel.load(Ordering::SeqCst)
    }

    pub fn download_game(&self, manifest: &Manifest, install_path: &Path, selected_tags: Option<std::collections::HashSet<String>>) -> Result<()> {
        // Group files by chunks to avoid redundant downloads
        let mut chunk_to_files: std::collections::HashMap<[u32; 4], Vec<String>> = std::collections::HashMap::new();
        for (filename, file_manifest) in &manifest.files {
            if let Some(tags) = &selected_tags {
                if !file_manifest.install_tags.is_empty() && !file_manifest.install_tags.iter().any(|t| tags.contains(t)) {
                    continue;
                }
            }
            for part in &file_manifest.chunk_parts {
                chunk_to_files.entry(part.guid).or_default().push(filename.clone());
            }
        }

        let total_chunks = manifest.chunks.len();
        let mut processed_chunks = 0;

        for (guid, chunk_info) in &manifest.chunks {
            if self.check_status() {
                return Err(anyhow::anyhow!("Download cancelled"));
            }

            let chunk_url = format!("{}/{}", self.base_url, chunk_info.path(manifest.manifest_version));

            let mut response = self.client.get(&chunk_url).send()?;
            if !response.status().is_success() {
                return Err(anyhow::anyhow!("Failed to download chunk {}: {}", chunk_url, response.status()));
            }

            let mut buffer = Vec::new();
            response.read_to_end(&mut buffer)?;

            let decompressed_data = self.decompress_chunk(&buffer)?;

            // Write chunk parts to corresponding files
            if let Some(filenames) = chunk_to_files.get(guid) {
                for filename in filenames {
                    let file_manifest = manifest.files.get(filename).unwrap();
                    for part in &file_manifest.chunk_parts {
                        if part.guid == *guid {
                            let target_file_path = install_path.join(filename);
                            if let Some(parent) = target_file_path.parent() {
                                std::fs::create_dir_all(parent)?;
                            }

                            let mut file = std::fs::OpenOptions::new()
                                .write(true)
                                .create(true)
                                .open(&target_file_path)?;

                            file.seek(SeekFrom::Start(part.file_offset))?;

                            let start = part.offset as usize;
                            let end = (part.offset + part.size) as usize;
                            file.write_all(&decompressed_data[start..end])?;
                        }
                    }
                }
            }

            processed_chunks += 1;
            let progress = processed_chunks as f32 / total_chunks as f32;
            let _ = self.tx.send(WorkerResponse::TaskProgress {
                task_name: format!("Downloading chunks ({}/{})", processed_chunks, total_chunks),
                progress,
                is_paused: self.pause.load(Ordering::SeqCst),
            });
        }

        Ok(())
    }

    fn decompress_chunk(&self, buffer: &[u8]) -> Result<Vec<u8>> {
        let mut cursor = Cursor::new(buffer);
        let magic = cursor.read_u32::<LittleEndian>()?;
        if magic != 0xB1FE3AA2 {
            return Err(anyhow::anyhow!("Invalid chunk magic: {:08X}", magic));
        }

        let header_version = cursor.read_u32::<LittleEndian>()?;
        let _header_size = cursor.read_u32::<LittleEndian>()?;
        let compressed_size = cursor.read_u32::<LittleEndian>()?;
        let mut _guid = [0u32; 4];
        for i in 0..4 { _guid[i] = cursor.read_u32::<LittleEndian>()?; }
        let mut _hash = cursor.read_u64::<LittleEndian>()?;
        let stored_as = cursor.read_u8()?;

        if header_version >= 2 {
            let mut _sha_hash = [0u8; 20];
            cursor.read_exact(&mut _sha_hash)?;
            let _hash_type = cursor.read_u8()?;
        }

        let mut uncompressed_size = 0u32;
        if header_version >= 3 {
            uncompressed_size = cursor.read_u32::<LittleEndian>()?;
        }

        let data_start = cursor.position() as usize;
        let data = &buffer[data_start..data_start + compressed_size as usize];

        if stored_as & 1 != 0 {
            let mut decoder = ZlibDecoder::new(data);
            let mut decoded = Vec::with_capacity(uncompressed_size as usize);
            decoder.read_to_end(&mut decoded)?;
            Ok(decoded)
        } else {
            Ok(data.to_vec())
        }
    }
}
