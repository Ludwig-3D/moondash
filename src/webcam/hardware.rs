use super::{DecodedFrame, Frame, ReadResult};
use std::{fs, io::Cursor, path::Path, process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::watch,
    time::timeout,
};

const FRAME_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Copy)]
enum Backend {
    Vaapi,
    Nvdec,
    Rockchip,
    V4l2,
}

impl Backend {
    fn name(self) -> &'static str {
        match self {
            Self::Vaapi => "VA-API",
            Self::Nvdec => "NVIDIA NVDEC",
            Self::Rockchip => "Rockchip MPP",
            Self::V4l2 => "V4L2",
        }
    }

    fn command(self, width: u32, height: u32) -> Command {
        if matches!(self, Self::V4l2) {
            let mut command = Command::new("gst-launch-1.0");
            command.args([
                "-q",
                "fdsrc",
                "fd=0",
                "!",
                "multipart/x-mixed-replace,boundary=camera",
                "!",
                "multipartdemux",
                "!",
                "jpegparse",
                "!",
                "v4l2jpegdec",
                "!",
                "videoconvert",
                "!",
                "videoscale",
                "!",
                &format!("video/x-raw,format=RGBA,width={width},height={height}"),
                "!",
                "fdsink",
                "fd=1",
                "sync=false",
            ]);
            return command;
        }

        let mut command = Command::new("ffmpeg");
        command.args(["-hide_banner", "-loglevel", "error"]);
        let filter = match self {
            Self::Vaapi => {
                command.args(["-hwaccel", "vaapi", "-hwaccel_output_format", "vaapi"]);
                format!(
                    "scale_vaapi=w={width}:h={height}:format=nv12,hwdownload,format=nv12,format=rgba"
                )
            }
            Self::Nvdec => {
                command.args(["-c:v", "mjpeg_cuvid"]);
                format!("scale={width}:{height},format=rgba")
            }
            Self::Rockchip => {
                command.args([
                    "-hwaccel",
                    "rkmpp",
                    "-hwaccel_output_format",
                    "drm_prime",
                    "-c:v",
                    "mjpeg_rkmpp",
                ]);
                format!(
                    "scale_rkrga=w={width}:h={height}:format=nv12,hwdownload,format=nv12,format=rgba"
                )
            }
            Self::V4l2 => unreachable!(),
        };
        command.args([
            "-probesize",
            "32",
            "-analyzeduration",
            "1",
            "-f",
            "mpjpeg",
            "-i",
            "pipe:0",
            "-an",
            "-vf",
            &filter,
            "-pix_fmt",
            "rgba",
            "-fps_mode",
            "passthrough",
            "-f",
            "rawvideo",
            "pipe:1",
        ]);
        command
    }
}

fn available_backends() -> Vec<Backend> {
    let exists = |path| Path::new(path).exists();
    let mut backends = Vec::new();
    if exists("/dev/nvidiactl") {
        backends.push(Backend::Nvdec);
    }
    if ["/dev/mpp_service", "/dev/mpp-service", "/dev/vpu_service"]
        .into_iter()
        .any(exists)
    {
        backends.push(Backend::Rockchip);
    }
    if fs::read_dir("/dev/dri").is_ok_and(|items| {
        items
            .flatten()
            .any(|item| item.file_name().to_string_lossy().starts_with("renderD"))
    }) {
        backends.push(Backend::Vaapi);
    }
    if cfg!(any(target_arch = "arm", target_arch = "aarch64")) && exists("/sys/class/video4linux") {
        backends.push(Backend::V4l2);
    }
    backends
}

pub(super) async fn try_decode(
    frames: &watch::Receiver<Arc<Frame>>,
    decoded: &watch::Sender<DecodedFrame>,
    first: Arc<Frame>,
    viewport: (u32, u32),
) {
    let Some(size) = jpeg_size(&first) else {
        return;
    };
    let (width, height) = preview_size(size, viewport);
    for backend in available_backends() {
        // Once a backend worked, leave recovery to the Rust pipeline if it later stops.
        let mut started = false;
        let _ = run(
            backend,
            frames.clone(),
            decoded,
            Arc::clone(&first),
            (width, height),
            &mut started,
        )
        .await;
        if started || jpeg_size(&frames.borrow()).is_none() {
            break;
        }
    }
}

fn jpeg_size(frame: &Frame) -> Option<(u32, u32)> {
    let Frame::Image { bytes, live: true } = frame else {
        return None;
    };
    if !bytes.starts_with(b"\xff\xd8") {
        return None;
    }
    let (width, height) =
        image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Jpeg)
            .into_dimensions()
            .ok()?;
    (width > 0 && height > 0 && width <= 4096 && height <= 4096).then_some((width, height))
}

fn preview_size((width, height): (u32, u32), (max_width, max_height): (u32, u32)) -> (u32, u32) {
    let scale = (f64::from(max_width.max(2)) / f64::from(width))
        .min(f64::from(max_height.max(2)) / f64::from(height))
        .min(1.);
    let even = |size| ((f64::from(size) * scale) as u32 & !1).max(2);
    if max_width == 0 || max_height == 0 {
        (width.div_ceil(2) * 2, height.div_ceil(2) * 2)
    } else {
        (even(width), even(height))
    }
}

async fn run(
    backend: Backend,
    mut frames: watch::Receiver<Arc<Frame>>,
    decoded: &watch::Sender<DecodedFrame>,
    mut frame: Arc<Frame>,
    (width, height): (u32, u32),
    started: &mut bool,
) -> ReadResult<()> {
    let mut child = backend
        .command(width, height)
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut input = child.stdin.take().ok_or("Missing decoder input")?;
    let mut output = child.stdout.take().ok_or("Missing decoder output")?;
    let writer = async {
        loop {
            if jpeg_size(&frame).is_none() {
                return Ok(());
            }
            let Frame::Image { bytes, .. } = &*frame else {
                return Ok(());
            };
            let header = format!(
                "--camera\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
                bytes.len()
            );
            input.write_all(header.as_bytes()).await?;
            input.write_all(bytes).await?;
            input.write_all(b"\r\n").await?;
            frames.changed().await?;
            frame = Arc::clone(&frames.borrow_and_update());
        }
    };
    let reader = async {
        loop {
            let mut pixels = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(width, height);
            timeout(FRAME_TIMEOUT, output.read_exact(pixels.make_mut_bytes())).await??;
            if !*started {
                log::info!("using {} MJPEG decoding", backend.name());
                *started = true;
            }
            if decoded.send(Some(Ok((pixels, true)))).is_err() {
                return Ok(());
            }
        }
    };
    let result = tokio::select! { result = writer => result, result = reader => result };
    let _ = child.kill().await;
    result
}

#[cfg(test)]
mod tests {
    use super::preview_size;

    #[test]
    fn fits_the_viewport_with_even_hardware_dimensions() {
        assert_eq!(preview_size((1280, 720), (900, 472)), (838, 472));
        assert_eq!(preview_size((640, 480), (900, 480)), (640, 480));
        assert_eq!(preview_size((1280, 720), (0, 0)), (1280, 720));
    }
}
