use crate::moonraker;
use reqwest::{Client, Response, Url, header};
use serde_json::Value;
use std::{
    io::Cursor,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    runtime::Handle,
    sync::watch,
    task::{JoinHandle, spawn_blocking},
    time::sleep,
};

mod hardware;

const MAX_FRAME: usize = 12 * 1024 * 1024;
const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Camera {
    pub id: String,
    pub name: String,
    sources: Vec<Url>,
}

pub fn cameras(result: &Value, endpoint: &str) -> Vec<Camera> {
    let Some(base) = moonraker::http_endpoint(endpoint, &[]).ok() else {
        return Vec::new();
    };
    let Some(cameras) = result["webcams"]
        .as_array()
        .or_else(|| result["result"]["webcams"].as_array())
    else {
        return Vec::new();
    };
    cameras
        .iter()
        .enumerate()
        .filter(|(_, camera)| camera["enabled"].as_bool() != Some(false))
        .filter_map(|(index, camera)| {
            let mut sources = Vec::new();
            for key in [
                "stream_url",
                "url_stream",
                "mjpg_url",
                "snapshot_url",
                "url_snapshot",
                "url",
                "webrtc_url",
            ] {
                for url in camera[key]
                    .as_str()
                    .into_iter()
                    .flat_map(|url| camera_urls(url, &base))
                {
                    if !sources.contains(&url) {
                        sources.push(url);
                    }
                }
            }
            if sources.is_empty() {
                return None;
            }
            let name = camera["name"]
                .as_str()
                .or_else(|| camera["title"].as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Webcam {}", index + 1));
            let id = camera["uid"]
                .as_str()
                .or_else(|| camera["id"].as_str())
                .or_else(|| camera["name"].as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| index.to_string());
            Some(Camera { id, name, sources })
        })
        .collect()
}

fn camera_urls(raw: &str, base: &Url) -> Vec<Url> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Vec::new();
    }
    let relative = Url::parse(raw).is_err() && !raw.starts_with("//");
    let mut directory = base.clone();
    if !directory.path().ends_with('/') {
        let path = format!("{}/", directory.path());
        directory.set_path(&path);
    }
    let Ok(mut url) = directory.join(raw) else {
        return Vec::new();
    };
    if !matches!(url.scheme(), "http" | "https") {
        return Vec::new();
    }
    if matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
        && url.set_host(base.host_str()).is_err()
    {
        return Vec::new();
    }
    if let Some(path) = url.path().strip_suffix("/webrtc") {
        let path = format!("{path}/");
        url.set_path(&path);
        url.query_pairs_mut().append_pair("action", "snapshot");
    }
    if relative && let Some(port) = base.port() {
        let mut frontend = url.clone();
        if frontend.set_port(None).is_ok() {
            return if port == 7125 {
                vec![frontend, url]
            } else {
                vec![url, frontend]
            };
        }
    }
    vec![url]
}

pub struct Feed {
    reader: JoinHandle<()>,
    decoder: JoinHandle<()>,
    frames: watch::Receiver<DecodedFrame>,
    viewport: watch::Sender<(u32, u32)>,
    presentation: Option<(Instant, u32)>,
}

enum Frame {
    Waiting,
    Image { bytes: Vec<u8>, live: bool },
    Unavailable,
}

type DecodedFrame = Option<Result<(slint::SharedPixelBuffer<slint::Rgba8Pixel>, bool), ()>>;

impl Feed {
    pub fn start(runtime: &Handle, client: Client, camera: Camera) -> Self {
        let (encoded_tx, mut encoded_rx) = watch::channel(Arc::new(Frame::Waiting));
        let (decoded_tx, frames) = watch::channel(None);
        let (viewport, viewport_rx) = watch::channel((0, 0));
        let reader = runtime.spawn(async move {
            loop {
                for source in &camera.sources {
                    if encoded_tx.is_closed() {
                        return;
                    }
                    let _ = read_source(&client, source, &encoded_tx).await;
                }
                encoded_tx.send_replace(Arc::new(Frame::Unavailable));
                sleep(Duration::from_secs(3)).await;
            }
        });
        let decoder = runtime.spawn(async move {
            let mut tried_hardware = false;
            // Both channels keep just the newest frame. Slow decoding or rendering
            // skips older frames instead of adding latency to the preview.
            while encoded_rx.changed().await.is_ok() {
                let mut frame = Arc::clone(&encoded_rx.borrow_and_update());
                if !tried_hardware && matches!(&*frame, Frame::Image { live: true, .. }) {
                    tried_hardware = true;
                    let size = *viewport_rx.borrow();
                    hardware::try_decode(&encoded_rx, &decoded_tx, frame, size).await;
                    frame = Arc::clone(&encoded_rx.borrow_and_update());
                }
                let decoded = match &*frame {
                    Frame::Waiting => continue,
                    Frame::Unavailable => {
                        tried_hardware = false;
                        Err(())
                    }
                    Frame::Image { live, .. } => {
                        if !live {
                            tried_hardware = false;
                        }
                        let size = *viewport_rx.borrow();
                        let Ok(decoded) = spawn_blocking(move || decode(&frame, size)).await else {
                            break;
                        };
                        decoded
                    }
                };
                if decoded_tx.send(Some(decoded)).is_err() {
                    break;
                }
            }
        });
        Self {
            reader,
            decoder,
            frames,
            viewport,
            presentation: None,
        }
    }

    pub fn set_viewport(&self, size: (u32, u32)) {
        self.viewport.send_if_modified(|previous| {
            if *previous == size {
                return false;
            }
            *previous = size;
            true
        });
    }

    pub fn next(&mut self) -> Option<Result<(slint::Image, bool), ()>> {
        if !self.frames.has_changed().ok()? {
            return None;
        }
        self.frames
            .borrow_and_update()
            .clone()
            .map(|frame| frame.map(|(pixels, live)| (slint::Image::from_rgba8(pixels), live)))
    }

    pub fn preview_fps(&mut self, live: bool, presented: bool) -> Option<i32> {
        if !live {
            self.presentation = None;
            return None;
        }
        let now = Instant::now();
        let (since, frames) = self.presentation.get_or_insert((now, 0));
        *frames += u32::from(presented);
        let elapsed = now.duration_since(*since);
        if elapsed < Duration::from_secs(1) {
            return None;
        }
        let fps = (f64::from(*frames) / elapsed.as_secs_f64()).round() as i32;
        self.presentation = Some((now, 0));
        Some(fps)
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        self.reader.abort();
        self.decoder.abort();
    }
}

fn decode(
    frame: &Frame,
    (width, height): (u32, u32),
) -> Result<(slint::SharedPixelBuffer<slint::Rgba8Pixel>, bool), ()> {
    let Frame::Image { bytes, live } = frame else {
        return Err(());
    };
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| ())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(96 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|_| ())?;
    let pixels =
        if width > 0 && height > 0 && (decoded.width() > width || decoded.height() > height) {
            decoded.thumbnail(width, height)
        } else {
            decoded
        }
        .into_rgba8();
    let buffer = slint::SharedPixelBuffer::clone_from_slice(
        pixels.as_raw(),
        pixels.width(),
        pixels.height(),
    );
    Ok((buffer, *live))
}

type ReadResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

async fn read_source(
    client: &Client,
    url: &Url,
    frames: &watch::Sender<Arc<Frame>>,
) -> ReadResult<()> {
    let mut response = request(client, url).await?;
    if let Some(boundary) = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(multipart_boundary)
    {
        let mut reader = MultipartReader::new(&boundary);
        while let Some(chunk) = response.chunk().await? {
            reader.push(&chunk)?;
            while let Some(bytes) = reader.next_frame() {
                frames.send_replace(Arc::new(Frame::Image { bytes, live: true }));
            }
        }
    } else {
        loop {
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await? {
                if bytes.len() + chunk.len() > MAX_FRAME {
                    return Err("Camera image is too large".into());
                }
                bytes.extend_from_slice(&chunk);
            }
            if !is_image(&bytes) {
                return Err("Camera response was not an image".into());
            }
            frames.send_replace(Arc::new(Frame::Image { bytes, live: false }));
            sleep(SNAPSHOT_INTERVAL).await;
            response = request(client, url).await?;
        }
    }
    Ok(())
}

async fn request(client: &Client, url: &Url) -> reqwest::Result<Response> {
    client
        .get(url.clone())
        .header(header::CACHE_CONTROL, "no-cache")
        .send()
        .await?
        .error_for_status()
}

fn is_image(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\xff\xd8")
        || bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"))
}

fn multipart_boundary(content_type: &str) -> Option<String> {
    let mut fields = content_type.split(';');
    if !fields
        .next()?
        .trim()
        .eq_ignore_ascii_case("multipart/x-mixed-replace")
    {
        return None;
    }
    fields.find_map(|field| {
        let (key, value) = field.trim().split_once('=')?;
        let value = value.trim().trim_matches('"');
        (key.trim().eq_ignore_ascii_case("boundary") && !value.is_empty())
            .then(|| format!("--{value}"))
    })
}

struct MultipartReader {
    bytes: Vec<u8>,
    boundary: Vec<u8>,
    offset: usize,
    in_part: bool,
    body: Option<(usize, Option<usize>)>,
}

impl MultipartReader {
    fn new(boundary: &str) -> Self {
        Self {
            bytes: Vec::new(),
            boundary: boundary.as_bytes().to_vec(),
            offset: 0,
            in_part: false,
            body: None,
        }
    }

    fn push(&mut self, chunk: &[u8]) -> ReadResult<()> {
        if self.bytes.len() + chunk.len() > MAX_FRAME {
            return Err("Camera frame is too large".into());
        }
        self.bytes.extend_from_slice(chunk);
        Ok(())
    }

    fn boundary_position(&mut self) -> Option<usize> {
        let position = self.bytes[self.offset..]
            .windows(self.boundary.len())
            .position(|candidate| candidate == self.boundary)
            .map(|index| self.offset + index);
        if position.is_none() {
            self.offset = self.bytes.len().saturating_sub(self.boundary.len() - 1);
        }
        position
    }

    fn consume(&mut self, bytes: usize) {
        self.bytes.drain(..bytes);
        self.offset = 0;
    }

    fn next_frame(&mut self) -> Option<Vec<u8>> {
        loop {
            if !self.in_part {
                let boundary = self.boundary_position()?;
                self.consume(boundary + self.boundary.len());
                self.in_part = true;
            }

            let (start, length) = match self.body {
                Some(body) => body,
                None => {
                    let header_end = self
                        .bytes
                        .windows(4)
                        .position(|window| window == b"\r\n\r\n")?;
                    let start = header_end + 4;
                    let length = std::str::from_utf8(&self.bytes[..header_end])
                        .ok()
                        .and_then(|headers| {
                            headers.lines().find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("Content-Length")
                                    .then(|| value.trim().parse::<usize>().ok())
                                    .flatten()
                                    .filter(|length| *length <= MAX_FRAME.saturating_sub(start))
                            })
                        });
                    self.body = Some((start, length));
                    self.offset = start;
                    (start, length)
                }
            };

            let (end, next) = if let Some(length) = length {
                let end = start + length;
                if self.bytes.len() < end {
                    return None;
                }
                // A complete part can be shown before the next boundary arrives.
                (end, end)
            } else {
                let boundary = self.boundary_position()?;
                let end = if self.bytes[..boundary].ends_with(b"\r\n") {
                    boundary - 2
                } else {
                    boundary
                };
                (end, boundary)
            };

            let frame = self
                .bytes
                .get(start..end)
                .filter(|content| is_image(content))
                .map(<[u8]>::to_vec);
            self.consume(next);
            self.in_part = false;
            self.body = None;
            if frame.is_some() {
                return frame;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolves_relative_camera_paths_against_the_printer_web_frontend_first() {
        let cameras = cameras(
            &json!({"result": {"webcams": [
                {"uid": "main", "stream_url": "/webcam/stream", "snapshot_url": "/webcam/snapshot"},
                {"enabled": false, "stream_url": "/unused"}
            ]}}),
            "printer.local:7125",
        );
        assert_eq!(cameras.len(), 1);
        assert_eq!(cameras[0].id, "main");
        let urls: Vec<_> = cameras[0].sources.iter().map(Url::as_str).collect();
        assert_eq!(
            urls,
            [
                "http://printer.local/webcam/stream",
                "http://printer.local:7125/webcam/stream",
                "http://printer.local/webcam/snapshot",
                "http://printer.local:7125/webcam/snapshot",
            ]
        );

        let base = moonraker::http_endpoint("https://example.test/printer", &[]).unwrap();
        assert_eq!(
            camera_urls("webcam/snapshot", &base)[0].as_str(),
            "https://example.test/printer/webcam/snapshot"
        );
        assert_eq!(
            camera_urls("http://127.0.0.1:8080/stream", &base)[0].as_str(),
            "http://example.test:8080/stream"
        );
    }

    #[test]
    fn extracts_mjpeg_frames_across_chunk_boundaries() {
        let boundary =
            multipart_boundary("multipart/x-mixed-replace; boundary=\"camera\"").unwrap();
        let mut reader = MultipartReader::new(&boundary);
        reader.push(b"--ca").unwrap();
        assert!(reader.next_frame().is_none());
        reader
            .push(b"mera\r\nContent-Type: image/jpeg\r\n\r\n\xff\xd8one\xff\xd9\r\n--cam")
            .unwrap();
        assert!(reader.next_frame().is_none());
        reader
            .push(b"era\r\nContent-Type: image/jpeg\r\n\r\n\xff\xd8two\xff\xd9\r\n--camera--\r\n")
            .unwrap();
        assert_eq!(reader.next_frame().unwrap(), b"\xff\xd8one\xff\xd9");
        assert_eq!(reader.next_frame().unwrap(), b"\xff\xd8two\xff\xd9");
        assert!(reader.next_frame().is_none());
    }
    #[test]
    fn emits_a_complete_length_delimited_frame_before_the_next_boundary() {
        let mut reader = MultipartReader::new("--camera");
        reader
            .push(b"--camera\r\nContent-Type: image/jpeg\r\nContent-Length: 7\r\n\r\n\xff\xd8one")
            .unwrap();
        assert!(reader.next_frame().is_none());
        reader.push(b"\xff\xd9").unwrap();
        assert_eq!(reader.next_frame().unwrap(), b"\xff\xd8one\xff\xd9");
    }
}
