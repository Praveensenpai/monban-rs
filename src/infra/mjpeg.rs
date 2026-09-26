use crate::error::{MonbanError, Result};
use image::{DynamicImage, ImageReader};
use std::io::{Cursor, Read};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub struct MjpegStream {
    latest_jpeg: Arc<Mutex<Option<Vec<u8>>>>,
    running: Arc<AtomicBool>,
    worker_handle: Option<JoinHandle<()>>,
}

impl MjpegStream {
    pub fn connect(url: &str) -> Result<Self> {
        let latest_jpeg = Arc::new(Mutex::new(None));
        let running = Arc::new(AtomicBool::new(true));

        let jpeg_clone = Arc::clone(&latest_jpeg);
        let run_clone = Arc::clone(&running);
        let stream_url = url.to_string();

        let handle = thread::spawn(move || {
            Self::stream_worker(&stream_url, jpeg_clone, run_clone);
        });

        thread::sleep(Duration::from_millis(300));

        Ok(Self {
            latest_jpeg,
            running,
            worker_handle: Some(handle),
        })
    }

    pub fn read_latest_frame(&self) -> Result<DynamicImage> {
        let raw_opt = {
            let guard = self
                .latest_jpeg
                .lock()
                .map_err(|e| MonbanError::Stream(format!("Lock poisoned: {e}")))?;
            guard.clone()
        };

        let raw =
            raw_opt.ok_or_else(|| MonbanError::Stream("No frame received yet".to_string()))?;
        let reader = ImageReader::new(Cursor::new(raw)).with_guessed_format()?;
        let img = reader.decode()?;
        Ok(img)
    }

    fn stream_worker(url: &str, latest: Arc<Mutex<Option<Vec<u8>>>>, running: Arc<AtomicBool>) {
        let client = match reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
        {
            Ok(c) => c,
            Err(_) => return,
        };

        while running.load(Ordering::Relaxed) {
            let mut response = match client.get(url).send() {
                Ok(res) => res,
                Err(_) => {
                    thread::sleep(Duration::from_secs(1));
                    continue;
                }
            };

            let mut buffer = Vec::with_capacity(131072);
            let mut chunk = [0u8; 16384];

            while running.load(Ordering::Relaxed) {
                let bytes_read = match response.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                buffer.extend_from_slice(&chunk[..bytes_read]);

                Self::extract_latest_frame(&mut buffer, &latest);
            }
        }
    }

    fn extract_latest_frame(buffer: &mut Vec<u8>, latest: &Arc<Mutex<Option<Vec<u8>>>>) {
        let mut last_complete = None;
        let mut search_idx = 0;

        while let Some(start_offset) = buffer[search_idx..]
            .windows(2)
            .position(|w| w == [0xFF, 0xD8])
        {
            let start = search_idx + start_offset;
            if buffer.len() <= start + 2 {
                break;
            }

            let Some(end_offset) = buffer[start + 2..]
                .windows(2)
                .position(|w| w == [0xFF, 0xD9])
            else {
                if start > 0 && last_complete.is_none() {
                    buffer.drain(..start);
                }
                break;
            };

            let end = start + 2 + end_offset + 2;
            last_complete = Some((start, end));
            search_idx = end;
        }

        if let Some((start, end)) = last_complete {
            let latest_bytes = buffer[start..end].to_vec();
            if let Ok(mut guard) = latest.lock() {
                *guard = Some(latest_bytes);
            }
            buffer.drain(..end);
        }
    }
}

impl Drop for MjpegStream {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
    }
}
