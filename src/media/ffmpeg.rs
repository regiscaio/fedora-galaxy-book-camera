use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, mpsc::SyncSender, OnceLock};
use std::thread;

use crate::{tr, trf, OwnedFrame, WorkerEvent, DRM_RENDER_NODES, PREVIEW_FRAMERATE};

static VIDEO_ENCODER_BACKEND: OnceLock<VideoEncoderBackend> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoEncoderBackend {
    NvidiaNvenc,
    IntelQsv,
    Vaapi,
    CpuX264,
}

impl VideoEncoderBackend {
    pub fn ui_label(self) -> String {
        match self {
            Self::NvidiaNvenc => tr("GPU NVIDIA (NVENC)"),
            Self::IntelQsv => tr("hardware Intel (Quick Sync)"),
            Self::Vaapi => tr("hardware VA-API"),
            Self::CpuX264 => tr("CPU (libx264)"),
        }
    }

    fn add_global_args(self, command: &mut Command) {
        if let Self::Vaapi = self {
            if let Some(render_node) = first_drm_render_node() {
                command.arg("-vaapi_device").arg(render_node);
            }
        }
    }

    fn add_video_output_args(self, command: &mut Command) {
        command.arg("-fps_mode:v").arg("vfr");

        match self {
            Self::NvidiaNvenc => {
                command
                    .arg("-c:v")
                    .arg("h264_nvenc")
                    .arg("-preset")
                    .arg("p5")
                    .arg("-tune")
                    .arg("hq")
                    .arg("-pix_fmt")
                    .arg("yuv420p");
            }
            Self::IntelQsv => {
                command
                    .arg("-c:v")
                    .arg("h264_qsv")
                    .arg("-preset")
                    .arg("faster");
            }
            Self::Vaapi => {
                command
                    .arg("-vf")
                    .arg("format=nv12,hwupload")
                    .arg("-c:v")
                    .arg("h264_vaapi");
            }
            Self::CpuX264 => {
                command
                    .arg("-c:v")
                    .arg("libx264")
                    .arg("-preset")
                    .arg("ultrafast")
                    .arg("-pix_fmt")
                    .arg("yuv420p");
            }
        }
    }
}

pub(crate) struct RecorderHandle {
    width: usize,
    height: usize,
    pub(crate) backend: VideoEncoderBackend,
    frame_sender: mpsc::SyncSender<Vec<u8>>,
}

impl RecorderHandle {
    pub(crate) fn try_send_frame(&self, frame: &OwnedFrame) {
        if frame.width != self.width || frame.height != self.height {
            return;
        }

        let data = frame.data.clone();
        let _ = self.frame_sender.try_send(data);
    }
}

fn ffmpeg_rawvideo_command(
    width: usize,
    height: usize,
    use_wallclock_timestamps: bool,
    backend: VideoEncoderBackend,
) -> Command {
    let mut command = Command::new("ffmpeg");
    command
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y");

    backend.add_global_args(&mut command);

    if use_wallclock_timestamps {
        command
            .arg("-use_wallclock_as_timestamps")
            .arg("1")
            .arg("-fflags")
            .arg("+genpts");
    }

    command
        .arg("-f")
        .arg("rawvideo")
        .arg("-pix_fmt")
        .arg("rgba")
        .arg("-video_size")
        .arg(format!("{width}x{height}"))
        .arg("-framerate")
        .arg(PREVIEW_FRAMERATE.to_string())
        .arg("-i")
        .arg("pipe:0");
    command
}

pub fn preferred_video_encoder_backend() -> VideoEncoderBackend {
    *VIDEO_ENCODER_BACKEND.get_or_init(detect_preferred_video_encoder_backend)
}

fn detect_preferred_video_encoder_backend() -> VideoEncoderBackend {
    let encoders = available_ffmpeg_encoders();

    if nvidia_devices_available() && ffmpeg_has_encoder(&encoders, "h264_nvenc") {
        return VideoEncoderBackend::NvidiaNvenc;
    }

    if first_drm_render_node().is_some() && ffmpeg_has_encoder(&encoders, "h264_qsv") {
        return VideoEncoderBackend::IntelQsv;
    }

    if first_drm_render_node().is_some() && ffmpeg_has_encoder(&encoders, "h264_vaapi") {
        return VideoEncoderBackend::Vaapi;
    }

    VideoEncoderBackend::CpuX264
}

fn available_ffmpeg_encoders() -> String {
    let output = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-encoders")
        .output();

    let Ok(output) = output else {
        return String::new();
    };

    let mut combined = String::from_utf8_lossy(&output.stdout).to_string();
    if !output.stderr.is_empty() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(&String::from_utf8_lossy(&output.stderr));
    }

    combined
}

fn ffmpeg_has_encoder(encoders_output: &str, encoder_name: &str) -> bool {
    encoders_output
        .lines()
        .map(str::trim)
        .any(|line| line.split_whitespace().any(|token| token == encoder_name))
}

fn nvidia_devices_available() -> bool {
    Path::new("/dev/nvidia0").exists() && Path::new("/dev/nvidiactl").exists()
}

fn first_drm_render_node() -> Option<&'static str> {
    DRM_RENDER_NODES
        .into_iter()
        .find(|path| Path::new(path).exists())
}

pub(crate) fn normalize_ffmpeg_stderr(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with("libva info:"))
        .collect::<Vec<_>>()
        .join("; ")
}

pub(crate) fn spawn_video_recorder(
    output_path: PathBuf,
    width: usize,
    height: usize,
    record_audio: bool,
    audio_source: &str,
    event_tx: SyncSender<WorkerEvent>,
) -> Result<RecorderHandle, String> {
    let (frame_sender, frame_receiver) = mpsc::sync_channel::<Vec<u8>>(4);
    let backend = preferred_video_encoder_backend();
    let mut child = ffmpeg_rawvideo_command(width, height, true, backend);
    if record_audio {
        child
            .arg("-thread_queue_size")
            .arg("512")
            .arg("-f")
            .arg("pulse")
            .arg("-i")
            .arg(if audio_source.trim().is_empty() {
                "default"
            } else {
                audio_source
            });
    } else {
        child.arg("-an");
    }

    backend.add_video_output_args(&mut child);

    if record_audio {
        child
            .arg("-af")
            .arg("aresample=async=1:first_pts=0")
            .arg("-c:a")
            .arg("aac")
            .arg("-b:a")
            .arg("160k")
            .arg("-shortest");
    }

    child
        .arg(&output_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let mut child = child
        .spawn()
        .map_err(|error| trf("Falha ao iniciar o ffmpeg para vídeo: {error}", &[("error", error.to_string())]))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| tr("O ffmpeg não abriu stdin para a gravação."))?;

    let finished_output = output_path.clone();
    thread::spawn(move || {
        let mut write_error = String::new();
        for frame in frame_receiver {
            if let Err(error) = stdin.write_all(&frame) {
                write_error = trf("Falha ao enviar frame para o ffmpeg: {error}", &[("error", error.to_string())]);
                break;
            }
        }
        drop(stdin);

        let result = child.wait_with_output();
        let (success, stderr) = match result {
            Ok(output) => {
                let ffmpeg_stderr = normalize_ffmpeg_stderr(&output.stderr);
                let had_write_error = !write_error.is_empty();
                let stderr = if !output.status.success() && !had_write_error {
                    ffmpeg_stderr
                } else if !output.status.success() && ffmpeg_stderr.is_empty() {
                    write_error
                } else if !output.status.success() {
                    format!("{write_error}; {ffmpeg_stderr}")
                } else if !had_write_error {
                    String::new()
                } else if ffmpeg_stderr.is_empty() {
                    write_error
                } else {
                    format!("{write_error}; {ffmpeg_stderr}")
                };
                (output.status.success() && !had_write_error, stderr)
            }
            Err(error) => (
                false,
                if write_error.is_empty() {
                    trf("Falha ao finalizar o ffmpeg: {error}", &[("error", error.to_string())])
                } else {
                    trf(
                        "{write_error}; falha ao finalizar o ffmpeg: {error}",
                        &[
                            ("write_error", write_error),
                            ("error", error.to_string()),
                        ],
                    )
                },
            ),
        };

        let _ = event_tx.send(WorkerEvent::RecordingFinished {
            success,
            output_path: finished_output,
            stderr,
        });
    });

    Ok(RecorderHandle {
        width,
        height,
        backend,
        frame_sender,
    })
}

pub(crate) fn write_smoke_video(frame: &OwnedFrame, output_path: &Path) -> Result<(), String> {
    let backend = VideoEncoderBackend::CpuX264;
    let mut child = ffmpeg_rawvideo_command(frame.width, frame.height, false, backend);
    child.arg("-an");
    backend.add_video_output_args(&mut child);
    child
        .arg(output_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let mut child = child
        .spawn()
        .map_err(|error| trf("Falha ao iniciar ffmpeg no smoke test: {error}", &[("error", error.to_string())]))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| tr("O ffmpeg não abriu stdin no smoke test."))?;

    for _ in 0..30 {
        stdin
            .write_all(&frame.data)
            .map_err(|error| trf("Falha ao alimentar o ffmpeg no smoke test: {error}", &[("error", error.to_string())]))?;
    }
    drop(stdin);

    let output = child
        .wait_with_output()
        .map_err(|error| trf("Falha ao finalizar o ffmpeg no smoke test: {error}", &[("error", error.to_string())]))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(command_stderr_or(
            &output.stderr,
            &tr("ffmpeg falhou no smoke test de vídeo."),
        ))
    }
}

fn command_stderr_or(output: &[u8], fallback: &str) -> String {
    let stderr = String::from_utf8_lossy(output).trim().to_string();
    if stderr.is_empty() {
        fallback.to_string()
    } else {
        stderr
    }
}
