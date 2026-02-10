use std::io::{Read, Write, Cursor, Seek, SeekFrom};
use byteorder::{LittleEndian, ReadBytesExt};
use flate2::read::ZlibDecoder;
use anyhow::Result;
use crate::manifest::Manifest;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering, AtomicU64};
use std::sync::mpsc::{Sender, channel};
use crate::worker::WorkerResponse;
use std::time::{Instant, Duration};

pub struct Downloader {
    client: reqwest::blocking::Client,
    base_url: String,
    tx: Sender<WorkerResponse>,
    cancel: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
}

impl Downloader {
    pub fn new(base_url: String, user_agent: String, tx: Sender<WorkerResponse>, cancel: Arc<AtomicBool>, pause: Arc<AtomicBool>) -> Self {
        let client = reqwest::blocking::Client::builder()
            .user_agent(user_agent)
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
        let mut files_to_download = std::collections::HashSet::new();
        for (filename, file_manifest) in &manifest.files {
            if let Some(tags) = &selected_tags {
                if !file_manifest.install_tags.is_empty() && !file_manifest.install_tags.iter().any(|t| tags.contains(t)) {
                    continue;
                }
            }

            let target_file_path = install_path.join(filename);
            if target_file_path.exists() {
                // Delta patching: skip files that already match the hash
                if let Ok(actual_hash) = crate::utils::hash_file(&target_file_path) {
                    if actual_hash == hex::encode(&file_manifest.hash) {
                        log::info!("Skipping {}, hash matches", filename);
                        continue;
                    }
                }
            }
            files_to_download.insert(filename.clone());
        }

        if files_to_download.is_empty() {
            log::info!("All files are up to date.");
            return Ok(());
        }

        let chunk_to_files: Arc<std::collections::HashMap<[u32; 4], Vec<String>>> = Arc::new({
            let mut map: std::collections::HashMap<[u32; 4], Vec<String>> = std::collections::HashMap::new();
            for filename in &files_to_download {
                if let Some(file_manifest) = manifest.files.get(filename) {
                    for part in &file_manifest.chunk_parts {
                        map.entry(part.guid).or_default().push(filename.clone());
                    }
                }
            }
            map
        });

        let chunks_to_download: Vec<([u32; 4], crate::manifest::ChunkInfo)> = manifest.chunks.iter()
            .filter(|(guid, _)| chunk_to_files.contains_key(*guid))
            .map(|(guid, info)| (*guid, info.clone()))
            .collect();

        let total_chunks = chunks_to_download.len();
        if total_chunks == 0 { return Ok(()); }

        let processed_chunks = Arc::new(AtomicU64::new(0));
        let total_downloaded = Arc::new(AtomicU64::new(0));
        let file_mutex = Arc::new(Mutex::new(()));
        let start_time = Instant::now();

        let (work_tx, work_rx) = channel();
        for item in chunks_to_download {
            work_tx.send(item).unwrap();
        }
        drop(work_tx);

        let work_rx = Arc::new(Mutex::new(work_rx));
        let manifest_version = manifest.manifest_version;
        let manifest_files = Arc::new(manifest.files.clone());
        let install_path = Arc::new(install_path.to_path_buf());

        let num_threads = 4;
        let mut threads = Vec::new();

        for _ in 0..num_threads {
            let rx = Arc::clone(&work_rx);
            let processed = Arc::clone(&processed_chunks);
            let downloaded = Arc::clone(&total_downloaded);
            let f_mutex = Arc::clone(&file_mutex);
            let chunk_files = Arc::clone(&chunk_to_files);
            let m_files = Arc::clone(&manifest_files);
            let i_path = Arc::clone(&install_path);
            let base_url = self.base_url.clone();
            let cancel = Arc::clone(&self.cancel);
            let pause = Arc::clone(&self.pause);
            let client = self.client.clone();

            threads.push(std::thread::spawn(move || {
                loop {
                    if cancel.load(Ordering::SeqCst) { break; }
                    while pause.load(Ordering::SeqCst) {
                        if cancel.load(Ordering::SeqCst) { return; }
                        std::thread::sleep(Duration::from_millis(100));
                    }

                    let item = {
                        let lock = rx.lock().unwrap();
                        lock.recv().ok()
                    };

                    let (guid, chunk_info) = match item {
                        Some(val) => val,
                        None => break,
                    };

                    let chunk_url = format!("{}/{}", base_url, chunk_info.path(manifest_version));
                    if let Ok(mut response) = client.get(&chunk_url).send() {
                        if response.status().is_success() {
                            let mut buffer = Vec::new();
                            if response.read_to_end(&mut buffer).is_ok() {
                                downloaded.fetch_add(buffer.len() as u64, Ordering::SeqCst);
                                if let Ok(decompressed_data) = decompress_chunk_static(&buffer) {
                                    if let Some(filenames) = chunk_files.get(&guid) {
                                        for filename in filenames {
                                            if let Some(file_manifest) = m_files.get(filename) {
                                                for part in &file_manifest.chunk_parts {
                                                    if part.guid == guid {
                                                        let target_file_path = i_path.join(filename);
                                                        if let Some(parent) = target_file_path.parent() {
                                                            let _ = std::fs::create_dir_all(parent);
                                                        }

                                                        let _lock = f_mutex.lock().unwrap();
                                                        if let Ok(mut file) = std::fs::OpenOptions::new()
                                                            .write(true)
                                                            .create(true)
                                                            .open(&target_file_path) {
                                                            let _ = file.seek(SeekFrom::Start(part.file_offset));
                                                            let start = part.offset as usize;
                                                            let end = (part.offset + part.size) as usize;
                                                            if end <= decompressed_data.len() {
                                                                let _ = file.write_all(&decompressed_data[start..end]);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    processed.fetch_add(1, Ordering::SeqCst);
                }
            }));
        }

        let mut last_update = Instant::now();
        loop {
            let processed = processed_chunks.load(Ordering::SeqCst);
            let downloaded = total_downloaded.load(Ordering::SeqCst);

            if last_update.elapsed() >= Duration::from_millis(500) {
                let elapsed = start_time.elapsed().as_secs_f64();
                let speed_bps = if elapsed > 0.0 { downloaded as f64 / elapsed } else { 0.0 };
                let speed_str = format_speed(speed_bps);

                let progress = processed as f32 / total_chunks as f32;
                let eta_str = if speed_bps > 0.0 && progress > 0.0 {
                    let total_size_estimate = (downloaded as f64 / progress as f64) as u64;
                    let remaining_bytes = total_size_estimate.saturating_sub(downloaded);
                    let eta_secs = remaining_bytes as f64 / speed_bps;
                    if eta_secs.is_finite() {
                        format_duration_eta(Duration::from_secs_f64(eta_secs))
                    } else {
                        "Unknown".to_string()
                    }
                } else {
                    "Unknown".to_string()
                };

                let _ = self.tx.send(WorkerResponse::TaskProgress {
                    task_name: format!("Downloading chunks ({}/{})", processed, total_chunks),
                    progress,
                    is_paused: self.pause.load(Ordering::SeqCst),
                    speed: Some(speed_str),
                    eta: Some(eta_str),
                });
                last_update = Instant::now();
            }

            if processed as usize >= total_chunks || self.cancel.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        for t in threads {
            let _ = t.join();
        }

        if self.cancel.load(Ordering::SeqCst) {
            return Err(anyhow::anyhow!("Download cancelled"));
        }

        Ok(())
    }

    fn decompress_chunk(&self, buffer: &[u8]) -> Result<Vec<u8>> {
        decompress_chunk_static(buffer)
    }
}

fn decompress_chunk_static(buffer: &[u8]) -> Result<Vec<u8>> {
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
    if data_start + compressed_size as usize > buffer.len() {
         return Err(anyhow::anyhow!("Compressed size exceeds buffer"));
    }
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

fn format_speed(bps: f64) -> String {
    if bps < 1024.0 {
        format!("{:.2} B/s", bps)
    } else if bps < 1024.0 * 1024.0 {
        format!("{:.2} KB/s", bps / 1024.0)
    } else {
        format!("{:.2} MB/s", bps / (1024.0 * 1024.0))
    }
}

fn format_duration_eta(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}
