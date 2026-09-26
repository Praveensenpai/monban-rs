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
    latest_frame: Arc<Mutex<Option<DynamicImage>>>,
    running: Arc<AtomicBool>,
    worker_handle: Option<JoinHandle<()>>,
}

impl MjpegStream {
    pub fn connect(url: &str) -> Result<Self> {
        let latest_frame = Arc::new(Mutex::new(None));
        let running = Arc::new(AtomicBool::new(true));

        let frame_clone = Arc::clone(&latest_frame);
        let run_clone = Arc::clone(&running);
        let stream_url = url.to_string();

        let handle = thread::spawn(move || {
            Self::stream_worker(&stream_url, frame_clone, run_clone);
        });

        thread::sleep(Duration::from_millis(300));

        Ok(Self {
            latest_frame,
            running,
            worker_handle: Some(handle),
        })
    }

    pub fn read_latest_frame(&self) -> Result<DynamicImage> {
        let guard = self
            .latest_frame
            .lock()
            .map_err(|e| MonbanError::Stream(format!("Lock poisoned: {e}")))?;

        guard
            .as_ref()
            .cloned()
            .ok_or_else(|| MonbanError::Stream("No frame received yet".to_string()))
    }

    fn stream_worker(
        url: &str,
        latest: Arc<Mutex<Option<DynamicImage>>>,
        running: Arc<AtomicBool>,
    ) {
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

            let mut buffer = Vec::with_capacity(65536);
            let mut chunk = [0u8; 8192];

            while running.load(Ordering::Relaxed) {
                let bytes_read = match response.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                buffer.extend_from_slice(&chunk[..bytes_read]);

                Self::extract_frames(&mut buffer, &latest);
            }
        }
    }

    fn extract_frames(buffer: &mut Vec<u8>, latest: &Arc<Mutex<Option<DynamicImage>>>) {
        while let Some(start) = buffer.windows(2).position(|w| w == [0xFF, 0xD8]) {
            if buffer.len() <= start + 2 {
                break;
            }

            let Some(offset) = buffer[start + 2..]
                .windows(2)
                .position(|w| w == [0xFF, 0xD9])
            else {
                if start > 0 {
                    buffer.drain(..start);
                }
                break;
            };

            let end = start + 2 + offset + 2;
            let jpeg_bytes = &buffer[start..end];
            Self::decode_and_store(jpeg_bytes, latest);
            buffer.drain(..end);
        }
    }

    fn decode_and_store(jpeg_bytes: &[u8], latest: &Arc<Mutex<Option<DynamicImage>>>) {
        let Ok(reader) = ImageReader::new(Cursor::new(jpeg_bytes)).with_guessed_format() else {
            return;
        };
        let Ok(img) = reader.decode() else {
            return;
        };
        if let Ok(mut guard) = latest.lock() {
            *guard = Some(img);
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
