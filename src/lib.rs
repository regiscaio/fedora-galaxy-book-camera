use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TrySendError},
    OnceLock,
};
use std::thread;
use std::time::{Duration, Instant};

use libcamera::{
    camera::{ActiveCamera, CameraConfigurationStatus},
    camera_manager::CameraManager,
    framebuffer_allocator::{FrameBuffer as CameraFrameBuffer, FrameBufferAllocator},
    framebuffer_map::MemoryMappedFrameBuffer,
    geometry::Size,
    pixel_format::PixelFormat,
    request::{Request, ReuseFlag},
    stream::{Stream, StreamRole},
};

mod app;

pub use app::config::{CameraConfig, Preset};
pub use app::localization::{
    localized_app_name,
    localized_app_name_for_locale,
    localized_camera_word_for_locale,
};
pub use app::paths::{default_config_path, photo_library_dir, timestamp, video_library_dir};
pub use app::singleton::{setup_singleton, SingletonState};
pub(crate) use app::paths::home_dir;

pub const APP_ID: &str = "com.caioregis.GalaxyBookCamera";
pub const APP_NAME: &str = "Galaxy Book Camera";
const LOCAL_TUNING_PATH_RELATIVE: &str = ".local/share/galaxybook-camera/libcamera/simple/ov02c10.yaml";
const DEV_TUNING_PATH_RELATIVE: &str = "data/libcamera/simple/ov02c10.yaml";
const SYSTEM_TUNING_PATH: &str = "/usr/share/galaxybook-camera/libcamera/simple/ov02c10.yaml";
const LIBCAMERA_SIMPLE_TUNING_ENV: &str = "LIBCAMERA_SIMPLE_TUNING_FILE";
const COUNTDOWN_OPTIONS: [u32; 3] = [0, 3, 10];
const PREVIEW_FRAMERATE: u32 = 30;
const MAX_PREVIEW_LONG_EDGE: usize = 1280;
const STILL_CAPTURE_WARMUP_FRAMES: u32 = 6;
const DRM_RENDER_NODES: [&str; 2] = ["/dev/dri/renderD128", "/dev/dri/renderD129"];
static VIDEO_ENCODER_BACKEND: OnceLock<VideoEncoderBackend> = OnceLock::new();
static PREVIEW_RESOLUTION_OPTIONS: OnceLock<Vec<ResolutionOption>> = OnceLock::new();
static PREVIEW_ZOOM_OPTIONS: OnceLock<Vec<PreviewZoomOption>> = OnceLock::new();
const PREVIEW_ZOOM_PRESETS: [(f64, &str); 5] = [
    (1.0, "1x"),
    (2.0, "2x"),
    (3.0, "3x"),
    (5.0, "5x"),
    (10.0, "10x"),
];
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode {
    Photo,
    Video,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolutionOption {
    pub label: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreviewZoomOption {
    pub label: String,
    pub width: u32,
    pub height: u32,
    pub factor: f64,
}

fn format_resolution_label(width: u32, height: u32) -> String {
    format!("{width} x {height}")
}

fn resolution_option(width: u32, height: u32) -> ResolutionOption {
    ResolutionOption {
        label: format_resolution_label(width, height),
        width,
        height,
    }
}

fn derived_preview_zoom_options() -> Vec<PreviewZoomOption> {
    let options = preview_resolution_options();
    let Some(base_option) = options.first() else {
        return Vec::new();
    };
    let base_width = base_option.width.max(2);
    let base_height = base_option.height.max(2);

    PREVIEW_ZOOM_PRESETS
        .iter()
        .map(|(factor, label)| {
            let width = ((base_width as f64) / factor).round().max(2.0) as u32;
            let height = ((base_height as f64) / factor).round().max(2.0) as u32;

            PreviewZoomOption {
                label: (*label).to_string(),
                width: width - (width % 2),
                height: height - (height % 2),
                factor: *factor,
            }
        })
        .collect()
}

fn fallback_preview_resolution_options() -> Vec<ResolutionOption> {
    vec![resolution_option(1920, 1092), resolution_option(1280, 720)]
}

fn simple_tuning_file_candidates() -> Vec<PathBuf> {
    let mut candidates = vec![
        PathBuf::from(SYSTEM_TUNING_PATH),
        home_dir().join(LOCAL_TUNING_PATH_RELATIVE),
    ];

    if let Ok(current_dir) = env::current_dir() {
        candidates.push(current_dir.join(DEV_TUNING_PATH_RELATIVE));
    }

    if let Ok(executable) = env::current_exe() {
        if let Some(target_dir) = executable.parent().and_then(Path::parent) {
            candidates.push(target_dir.join(DEV_TUNING_PATH_RELATIVE));
        }
    }

    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join(DEV_TUNING_PATH_RELATIVE));
    candidates
}

fn simple_tuning_file_path() -> Option<PathBuf> {
    simple_tuning_file_candidates()
        .into_iter()
        .find(|candidate| candidate.is_file())
}

fn apply_simple_tuning_env() {
    if env::var_os(LIBCAMERA_SIMPLE_TUNING_ENV)
        .filter(|value| !value.is_empty())
        .is_some()
    {
        return;
    }

    if let Some(path) = simple_tuning_file_path() {
        unsafe {
            env::set_var(LIBCAMERA_SIMPLE_TUNING_ENV, path);
        }
    }
}

fn detected_preview_resolution_options() -> Result<Vec<ResolutionOption>, String> {
    apply_simple_tuning_env();
    let manager = CameraManager::new()
        .map_err(|error| format!("Falha ao inicializar libcamera para listar resoluções: {error}"))?;
    let camera_id = manager
        .cameras()
        .iter()
        .next()
        .map(|camera| camera.id().to_string())
        .ok_or_else(|| "Nenhuma câmera disponível para listar resoluções.".to_string())?;
    let camera_ref = manager
        .get(&camera_id)
        .ok_or_else(|| format!("Câmera {camera_id} não ficou acessível para listar resoluções."))?;
    let camera = camera_ref
        .acquire()
        .map_err(|error| format!("Falha ao adquirir a câmera para listar resoluções: {error}"))?;
    let configuration = camera
        .generate_configuration(&[StreamRole::ViewFinder])
        .ok_or_else(|| "Não foi possível gerar a configuração base para listar resoluções.".to_string())?;
    let stream_cfg = configuration
        .get(0)
        .ok_or_else(|| "A configuração da câmera não retornou stream para listar resoluções.".to_string())?;
    let pixel_format = PixelFormat::parse("XBGR8888")
        .ok_or_else(|| "XBGR8888 não está disponível para listar resoluções.".to_string())?;

    let mut sizes: Vec<(u32, u32)> = stream_cfg
        .formats()
        .sizes(pixel_format)
        .into_iter()
        .map(|size| (size.width, size.height))
        .collect();
    sizes.sort_unstable_by(|left, right| {
        let left_area = left.0 as u64 * left.1 as u64;
        let right_area = right.0 as u64 * right.1 as u64;
        right_area
            .cmp(&left_area)
            .then_with(|| right.0.cmp(&left.0))
            .then_with(|| right.1.cmp(&left.1))
    });
    sizes.dedup();

    if sizes.is_empty() {
        return Err("O libcamera não retornou resoluções válidas para o preview.".to_string());
    }

    Ok(sizes
        .into_iter()
        .map(|(width, height)| resolution_option(width, height))
        .collect())
}

pub fn preview_resolution_options() -> &'static [ResolutionOption] {
    PREVIEW_RESOLUTION_OPTIONS
        .get_or_init(|| {
            detected_preview_resolution_options()
                .unwrap_or_else(|_| fallback_preview_resolution_options())
        })
        .as_slice()
}

pub fn preview_zoom_options() -> &'static [PreviewZoomOption] {
    PREVIEW_ZOOM_OPTIONS
        .get_or_init(derived_preview_zoom_options)
        .as_slice()
}

#[derive(Clone)]
pub struct OwnedFrame {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

impl OwnedFrame {
    fn from_strided_rgba(width: usize, height: usize, stride: usize, source: &[u8]) -> Result<Self, String> {
        let row_bytes = width.saturating_mul(4);
        let required = stride
            .checked_mul(height)
            .ok_or_else(|| "Frame invalido: overflow ao calcular o tamanho do buffer.".to_string())?;
        if source.len() < required || row_bytes == 0 {
            return Err("Frame invalido: dados insuficientes para a resolucao atual.".to_string());
        }

        let mut data = if stride == row_bytes {
            source[..required].to_vec()
        } else {
            let mut packed = vec![0_u8; row_bytes * height];
            for row in 0..height {
                let src_start = row * stride;
                let src_end = src_start + row_bytes;
                let dst_start = row * row_bytes;
                let dst_end = dst_start + row_bytes;
                packed[dst_start..dst_end].copy_from_slice(&source[src_start..src_end]);
            }
            packed
        };

        for pixel in data.chunks_exact_mut(4) {
            pixel[3] = 255;
        }

        Ok(Self { width, height, data })
    }

    fn from_strided_rgba_scaled(
        source_width: usize,
        source_height: usize,
        stride: usize,
        source: &[u8],
        target_width: usize,
        target_height: usize,
    ) -> Result<Self, String> {
        if target_width == source_width && target_height == source_height {
            return Self::from_strided_rgba(source_width, source_height, stride, source);
        }

        let row_bytes = source_width.saturating_mul(4);
        let required = stride
            .checked_mul(source_height)
            .ok_or_else(|| "Frame invalido: overflow ao calcular o tamanho do buffer.".to_string())?;
        if source.len() < required || row_bytes == 0 || target_width == 0 || target_height == 0 {
            return Err("Frame invalido: dados insuficientes para a resolucao atual.".to_string());
        }

        let mut data = vec![0_u8; target_width * target_height * 4];
        for target_y in 0..target_height {
            let source_y = target_y * source_height / target_height;
            let source_row_start = source_y * stride;
            let target_row_start = target_y * target_width * 4;
            for target_x in 0..target_width {
                let source_x = target_x * source_width / target_width;
                let source_index = source_row_start + source_x * 4;
                let target_index = target_row_start + target_x * 4;
                data[target_index..(target_index + 3)].copy_from_slice(&source[source_index..(source_index + 3)]);
                data[target_index + 3] = 255;
            }
        }

        Ok(Self {
            width: target_width,
            height: target_height,
            data,
        })
    }

    fn scaled_nearest(&self, target_width: usize, target_height: usize) -> Self {
        if target_width == self.width && target_height == self.height {
            return self.clone();
        }

        let mut data = vec![0_u8; target_width * target_height * 4];
        for target_y in 0..target_height {
            let source_y = target_y * self.height / target_height;
            let target_row_start = target_y * target_width * 4;
            let source_row_start = source_y * self.width * 4;
            for target_x in 0..target_width {
                let source_x = target_x * self.width / target_width;
                let source_index = source_row_start + source_x * 4;
                let target_index = target_row_start + target_x * 4;
                data[target_index..(target_index + 4)]
                    .copy_from_slice(&self.data[source_index..(source_index + 4)]);
            }
        }

        Self {
            width: target_width,
            height: target_height,
            data,
        }
    }
}

fn preview_dimensions(width: usize, height: usize) -> (usize, usize) {
    let long_edge = width.max(height);
    if long_edge <= MAX_PREVIEW_LONG_EDGE {
        return (width, height);
    }

    let scale = MAX_PREVIEW_LONG_EDGE as f64 / long_edge as f64;
    let scaled_width = ((width as f64) * scale).round().max(1.0) as usize;
    let scaled_height = ((height as f64) * scale).round().max(1.0) as usize;
    (scaled_width, scaled_height)
}

#[derive(Clone)]
struct AdjustmentProfile {
    red_lut: [u8; 256],
    green_lut: [u8; 256],
    blue_lut: [u8; 256],
    has_lut_adjustments: bool,
    has_brightness_contrast_adjustments: bool,
    has_hue_saturation_adjustments: bool,
    has_extreme_luma_neutralization: bool,
    brightness_offset: f32,
    contrast: f32,
    saturation: f32,
    hue_sin: f32,
    hue_cos: f32,
    sharpen_amount: f32,
    mirror: bool,
}

impl AdjustmentProfile {
    fn new(config: &CameraConfig) -> Self {
        let has_lut_adjustments = config.exposure_value.abs() > 0.001
            || (config.gamma - 1.0).abs() > 0.001
            || config.temperature.abs() > 0.001
            || config.tint.abs() > 0.001
            || (config.red_gain - 1.0).abs() > 0.001
            || (config.green_gain - 1.0).abs() > 0.001
            || (config.blue_gain - 1.0).abs() > 0.001;

        let temperature = config.temperature as f32;
        let tint = config.tint as f32;
        let brightness_offset = config.brightness as f32;
        let contrast = (config.contrast as f32).max(0.0);
        let saturation = (config.saturation as f32).max(0.0);
        let hue_angle = (config.hue as f32) * std::f32::consts::PI;
        let exposure_scale = 2.0_f32.powf((config.exposure_value as f32) * 0.6);
        let gamma_inverse = 1.0_f32 / (config.gamma as f32).max(0.05);
        let red_scale = (config.red_gain as f32) * (1.0 + 0.25 * temperature) * (1.0 + 0.08 * tint);
        let green_scale = (config.green_gain as f32) * (1.0 - 0.20 * tint);
        let blue_scale = (config.blue_gain as f32) * (1.0 - 0.25 * temperature) * (1.0 + 0.08 * tint);

        let mut red_lut = [0_u8; 256];
        let mut green_lut = [0_u8; 256];
        let mut blue_lut = [0_u8; 256];

        for value in 0..=255 {
            red_lut[value as usize] = adjust_color_channel(value as u8, red_scale, exposure_scale, gamma_inverse);
            green_lut[value as usize] =
                adjust_color_channel(value as u8, green_scale, exposure_scale, gamma_inverse);
            blue_lut[value as usize] = adjust_color_channel(value as u8, blue_scale, exposure_scale, gamma_inverse);
        }

        Self {
            red_lut,
            green_lut,
            blue_lut,
            has_lut_adjustments,
            has_brightness_contrast_adjustments: brightness_offset.abs() > 0.001 || (contrast - 1.0).abs() > 0.001,
            has_hue_saturation_adjustments: (saturation - 1.0).abs() > 0.001 || hue_angle.abs() > 0.001,
            has_extreme_luma_neutralization: true,
            brightness_offset,
            contrast,
            saturation,
            hue_sin: hue_angle.sin(),
            hue_cos: hue_angle.cos(),
            sharpen_amount: (config.sharpness - 1.0).max(0.0) as f32,
            mirror: config.mirror,
        }
    }
}

fn adjust_color_channel(channel: u8, channel_scale: f32, exposure_scale: f32, gamma_inverse: f32) -> u8 {
    let scaled = ((channel as f32) / 255.0 * channel_scale * exposure_scale).clamp(0.0, 1.0);
    let corrected = scaled.powf(gamma_inverse);
    (corrected * 255.0).clamp(0.0, 255.0).round() as u8
}

fn apply_adjustments(frame: &mut OwnedFrame, profile: &AdjustmentProfile) {
    if profile.has_lut_adjustments
        || profile.has_brightness_contrast_adjustments
        || profile.has_hue_saturation_adjustments
        || profile.has_extreme_luma_neutralization
    {
        for pixel in frame.data.chunks_exact_mut(4) {
            let mut red = if profile.has_lut_adjustments {
                profile.red_lut[pixel[0] as usize]
            } else {
                pixel[0]
            };
            let mut green = if profile.has_lut_adjustments {
                profile.green_lut[pixel[1] as usize]
            } else {
                pixel[1]
            };
            let mut blue = if profile.has_lut_adjustments {
                profile.blue_lut[pixel[2] as usize]
            } else {
                pixel[2]
            };

            if profile.has_hue_saturation_adjustments {
                let red_f = red as f32 / 255.0;
                let green_f = green as f32 / 255.0;
                let blue_f = blue as f32 / 255.0;

                let luma = 0.299 * red_f + 0.587 * green_f + 0.114 * blue_f;
                let chroma_i = 0.596 * red_f - 0.274 * green_f - 0.322 * blue_f;
                let chroma_q = 0.211 * red_f - 0.523 * green_f + 0.312 * blue_f;

                let saturated_i = chroma_i * profile.saturation;
                let saturated_q = chroma_q * profile.saturation;
                let rotated_i = saturated_i * profile.hue_cos - saturated_q * profile.hue_sin;
                let rotated_q = saturated_i * profile.hue_sin + saturated_q * profile.hue_cos;

                red = (luma + 0.956 * rotated_i + 0.621 * rotated_q)
                    .mul_add(255.0, 0.0)
                    .clamp(0.0, 255.0)
                    .round() as u8;
                green = (luma - 0.272 * rotated_i - 0.647 * rotated_q)
                    .mul_add(255.0, 0.0)
                    .clamp(0.0, 255.0)
                    .round() as u8;
                blue = (luma - 1.106 * rotated_i + 1.703 * rotated_q)
                    .mul_add(255.0, 0.0)
                    .clamp(0.0, 255.0)
                    .round() as u8;
            }

            if profile.has_brightness_contrast_adjustments {
                red = adjust_brightness_contrast(red, profile.brightness_offset, profile.contrast);
                green = adjust_brightness_contrast(green, profile.brightness_offset, profile.contrast);
                blue = adjust_brightness_contrast(blue, profile.brightness_offset, profile.contrast);
            }

            if profile.has_extreme_luma_neutralization {
                (red, green, blue) = neutralize_extreme_luma_casts(red, green, blue);
            }

            pixel[0] = red;
            pixel[1] = green;
            pixel[2] = blue;
        }
    }

    if profile.sharpen_amount > 0.001 {
        apply_sharpen(frame, profile.sharpen_amount);
    }

    if profile.mirror {
        mirror_frame_horizontal(frame);
    }
}

fn adjust_brightness_contrast(channel: u8, brightness_offset: f32, contrast: f32) -> u8 {
    let normalized = channel as f32 / 255.0;
    let adjusted = ((normalized - 0.5) * contrast + 0.5 + brightness_offset).clamp(0.0, 1.0);
    (adjusted * 255.0).round() as u8
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    if edge0 == edge1 {
        return if value < edge0 { 0.0 } else { 1.0 };
    }

    let normalized = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    normalized * normalized * (3.0 - 2.0 * normalized)
}

fn neutralize_extreme_luma_casts(red: u8, green: u8, blue: u8) -> (u8, u8, u8) {
    let red_f = red as f32 / 255.0;
    let green_f = green as f32 / 255.0;
    let blue_f = blue as f32 / 255.0;

    let luma = 0.299 * red_f + 0.587 * green_f + 0.114 * blue_f;
    let shadow_weight = 1.0 - smoothstep(0.03, 0.24, luma);
    let highlight_weight = smoothstep(0.74, 0.98, luma);
    let neutralize_amount = shadow_weight * 0.34 + highlight_weight * 0.12;

    if neutralize_amount <= 0.001 {
        return (red, green, blue);
    }

    let blend = |channel: f32| -> u8 {
        (channel + (luma - channel) * neutralize_amount)
            .mul_add(255.0, 0.0)
            .clamp(0.0, 255.0)
            .round() as u8
    };

    (blend(red_f), blend(green_f), blend(blue_f))
}

fn apply_sharpen(frame: &mut OwnedFrame, amount: f32) {
    let width = frame.width;
    let height = frame.height;
    if width < 3 || height < 3 {
        return;
    }

    let original = frame.data.clone();
    let stride = width * 4;
    let kernel_amount = amount.min(1.0) * 0.25;

    for y in 1..(height - 1) {
        for x in 1..(width - 1) {
            let center = y * stride + x * 4;
            let left = center - 4;
            let right = center + 4;
            let up = center - stride;
            let down = center + stride;

            for channel in 0..3 {
                let center_value = original[center + channel] as f32;
                let neighbors = original[left + channel] as f32
                    + original[right + channel] as f32
                    + original[up + channel] as f32
                    + original[down + channel] as f32;
                let sharpened = center_value * (1.0 + 4.0 * kernel_amount) - neighbors * kernel_amount;
                frame.data[center + channel] = sharpened.clamp(0.0, 255.0).round() as u8;
            }
        }
    }
}

fn mirror_frame_horizontal(frame: &mut OwnedFrame) {
    if frame.width < 2 {
        return;
    }

    let row_stride = frame.width * 4;
    for row in frame.data.chunks_exact_mut(row_stride) {
        for x in 0..(frame.width / 2) {
            let left = x * 4;
            let right = (frame.width - 1 - x) * 4;
            let (before_right, right_and_after) = row.split_at_mut(right);
            before_right[left..(left + 4)].swap_with_slice(&mut right_and_after[..4]);
        }
    }
}

struct RecorderHandle {
    width: usize,
    height: usize,
    backend: VideoEncoderBackend,
    frame_sender: mpsc::SyncSender<Vec<u8>>,
}

impl RecorderHandle {
    fn try_send_frame(&self, frame: &OwnedFrame) {
        if frame.width != self.width || frame.height != self.height {
            return;
        }

        let data = frame.data.clone();
        let _ = self.frame_sender.try_send(data);
    }
}

#[derive(Clone)]
pub struct AudioSourceOption {
    pub id: String,
    pub label: String,
}

pub enum WorkerCommand {
    StartPreview,
    StopPreview,
    ApplyConfig {
        config: CameraConfig,
        restart: bool,
    },
    CapturePhoto {
        output_path: PathBuf,
    },
    StartRecording,
    StopRecording,
    Shutdown,
}

pub enum WorkerEvent {
    PreviewStarted {
        width: usize,
        height: usize,
    },
    PreviewStopped {
        reason: String,
    },
    PreviewFrame {
        frame: OwnedFrame,
        fps: f32,
    },
    Status(String),
    PhotoFinished {
        success: bool,
        output_path: PathBuf,
        stderr: String,
        resolution: Option<(usize, usize)>,
    },
    RecordingFinished {
        success: bool,
        output_path: PathBuf,
        stderr: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoEncoderBackend {
    NvidiaNvenc,
    IntelQsv,
    Vaapi,
    CpuX264,
}

impl VideoEncoderBackend {
    pub fn ui_label(self) -> &'static str {
        match self {
            Self::NvidiaNvenc => "GPU NVIDIA (NVENC)",
            Self::IntelQsv => "hardware Intel (Quick Sync)",
            Self::Vaapi => "hardware VA-API",
            Self::CpuX264 => "CPU (libx264)",
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

struct PreviewSession<'a> {
    camera: ActiveCamera<'a>,
    stream: Stream,
    request_rx: Receiver<Request>,
    width: usize,
    height: usize,
    stride: usize,
    profile: AdjustmentProfile,
    recorder: Option<RecorderHandle>,
    pending_recording_output: Option<PathBuf>,
    last_fps: f32,
    fps_window_started: Instant,
    fps_window_frames: u32,
}

impl<'a> PreviewSession<'a> {
    fn update_profile(&mut self, config: &CameraConfig) {
        self.profile = AdjustmentProfile::new(config);
    }
}

fn emit_preview_frame(event_tx: &SyncSender<WorkerEvent>, frame: OwnedFrame, fps: f32) {
    match event_tx.try_send(WorkerEvent::PreviewFrame { frame, fps }) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {}
        Err(TrySendError::Disconnected(_)) => {}
    }
}

pub fn spawn_camera_worker(
    initial_config: CameraConfig,
) -> (Sender<WorkerCommand>, Receiver<WorkerEvent>) {
    let (command_tx, command_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::sync_channel(2);
    thread::spawn(move || camera_worker(initial_config, command_rx, event_tx));
    (command_tx, event_rx)
}

pub fn set_softisp_env(mode: &str) {
    unsafe {
        std::env::set_var("LIBCAMERA_SOFTISP_MODE", mode);
    }
    apply_simple_tuning_env();
}

fn camera_worker(
    initial_config: CameraConfig,
    command_rx: Receiver<WorkerCommand>,
    event_tx: SyncSender<WorkerEvent>,
) {
    let mut config = initial_config;
    set_softisp_env(&config.softisp_mode);

    let manager = match CameraManager::new() {
        Ok(manager) => manager,
        Err(error) => {
            let _ = event_tx.send(WorkerEvent::PreviewStopped {
                reason: format!("Falha ao iniciar o libcamera: {error}"),
            });
            return;
        }
    };

    let mut session = None;

    loop {
        while let Ok(command) = command_rx.try_recv() {
            if handle_worker_command(command, &manager, &mut config, &mut session, &event_tx) {
                if let Some(mut session) = session.take() {
                    stop_preview_session(&mut session);
                }
                return;
            }
        }

        let Some(active_session) = session.as_mut() else {
            match command_rx.recv() {
                Ok(command) => {
                    if handle_worker_command(command, &manager, &mut config, &mut session, &event_tx) {
                        return;
                    }
                }
                Err(_) => return,
            }
            continue;
        };

        match active_session.request_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(mut request) => {
                if let Err(error) =
                    process_completed_request(active_session, &config, &mut request, &event_tx)
                {
                    let _ = event_tx.send(WorkerEvent::Status(format!("Erro no preview: {error}")));
                }

                request.reuse(ReuseFlag::REUSE_BUFFERS);
                if let Err((_, error)) = active_session.camera.queue_request(request) {
                    let _ = event_tx.send(WorkerEvent::PreviewStopped {
                        reason: format!("Falha ao reenfileirar o frame da camera: {error}"),
                    });
                    let mut old_session = session.take().unwrap();
                    stop_preview_session(&mut old_session);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                let _ = event_tx.send(WorkerEvent::PreviewStopped {
                    reason: "Canal interno do preview foi encerrado.".to_string(),
                });
                if let Some(mut old_session) = session.take() {
                    stop_preview_session(&mut old_session);
                }
            }
        }
    }
}

fn handle_worker_command<'a>(
    command: WorkerCommand,
    manager: &'a CameraManager,
    config: &mut CameraConfig,
    session: &mut Option<PreviewSession<'a>>,
    event_tx: &SyncSender<WorkerEvent>,
) -> bool {
    match command {
        WorkerCommand::StartPreview => {
            *session = restart_preview_session(manager, config, session.take(), event_tx);
            false
        }
        WorkerCommand::StopPreview => {
            if let Some(mut active_session) = session.take() {
                stop_preview_session(&mut active_session);
            }
            let _ = event_tx.send(WorkerEvent::PreviewStopped {
                reason: "Preview parado.".to_string(),
            });
            false
        }
        WorkerCommand::ApplyConfig {
            config: next_config,
            restart,
        } => {
            *config = next_config;
            if let Some(active_session) = session.as_mut() {
                active_session.update_profile(config);
            }
            if restart && session.is_some() {
                *session = restart_preview_session(manager, config, session.take(), event_tx);
            }
            false
        }
        WorkerCommand::CapturePhoto { output_path } => {
            if let Some(active_session) = session.as_ref() {
                if active_session.recorder.is_some() || active_session.pending_recording_output.is_some() {
                    let _ = event_tx.send(WorkerEvent::PhotoFinished {
                        success: false,
                        output_path,
                        stderr: "Pare a gravacao antes de tirar foto em resolucao maxima.".to_string(),
                        resolution: None,
                    });
                    return false;
                }
            }

            let should_restore_preview = session.is_some();
            if let Some(mut active_session) = session.take() {
                stop_preview_session(&mut active_session);
            }

            let result = capture_photo_max_resolution_with_manager(manager, config, &output_path);

            if should_restore_preview {
                *session = restart_preview_session(manager, config, session.take(), event_tx);
            }

            match result {
                Ok((width, height)) => {
                    let _ = event_tx.send(WorkerEvent::PhotoFinished {
                        success: true,
                        output_path,
                        stderr: String::new(),
                        resolution: Some((width, height)),
                    });
                }
                Err(error) => {
                    let _ = event_tx.send(WorkerEvent::PhotoFinished {
                        success: false,
                        output_path,
                        stderr: error,
                        resolution: None,
                    });
                }
            }
            false
        }
        WorkerCommand::StartRecording => {
            let Some(active_session) = session.as_mut() else {
                let _ = event_tx.send(WorkerEvent::Status(
                    "Inicie o preview antes de gravar video.".to_string(),
                ));
                return false;
            };
            if active_session.recorder.is_some() || active_session.pending_recording_output.is_some() {
                return false;
            }
            let output_path = video_library_dir().join(format!("camera-{}.mp4", timestamp()));
            active_session.pending_recording_output = Some(output_path.clone());
            let _ = event_tx.send(WorkerEvent::Status(format!(
                "Preparando gravacao em {}...",
                output_path.display()
            )));
            false
        }
        WorkerCommand::StopRecording => {
            let Some(active_session) = session.as_mut() else {
                return false;
            };
            active_session.pending_recording_output = None;
            if let Some(recorder) = active_session.recorder.take() {
                drop(recorder);
                let _ = event_tx.send(WorkerEvent::Status(
                    "Finalizando arquivo de video...".to_string(),
                ));
            }
            false
        }
        WorkerCommand::Shutdown => true,
    }
}

fn restart_preview_session<'a>(
    manager: &'a CameraManager,
    config: &CameraConfig,
    previous_session: Option<PreviewSession<'a>>,
    event_tx: &SyncSender<WorkerEvent>,
) -> Option<PreviewSession<'a>> {
    if let Some(mut session) = previous_session {
        stop_preview_session(&mut session);
    }

    match start_preview_session(manager, config) {
        Ok(session) => {
            let _ = event_tx.send(WorkerEvent::PreviewStarted {
                width: session.width,
                height: session.height,
            });
            let _ = event_tx.send(WorkerEvent::Status(format!(
                "Preview ativo em {}x{} com libcamera direto.",
                session.width, session.height
            )));
            Some(session)
        }
        Err(error) => {
            let _ = event_tx.send(WorkerEvent::PreviewStopped {
                reason: error.clone(),
            });
            None
        }
    }
}

fn start_preview_session<'a>(
    manager: &'a CameraManager,
    config: &CameraConfig,
) -> Result<PreviewSession<'a>, String> {
    let camera_id = manager
        .cameras()
        .iter()
        .next()
        .map(|camera| camera.id().to_string())
        .ok_or_else(|| "Nenhuma camera disponivel no libcamera.".to_string())?;
    let camera_ref = manager
        .get(&camera_id)
        .ok_or_else(|| format!("Camera {camera_id} nao ficou acessivel pelo CameraManager."))?;
    let mut camera = camera_ref
        .acquire()
        .map_err(|error| format!("Falha ao adquirir a camera: {error}"))?;

    let mut configuration = camera
        .generate_configuration(&[StreamRole::ViewFinder])
        .ok_or_else(|| "Nao foi possivel gerar a configuracao padrao da camera.".to_string())?;
    let Some(mut stream_cfg) = configuration.get_mut(0) else {
        return Err("A configuracao da camera nao retornou um stream valido.".to_string());
    };

    let pixel_format = PixelFormat::parse("XBGR8888")
        .ok_or_else(|| "XBGR8888 nao esta disponivel neste host.".to_string())?;
    stream_cfg.set_pixel_format(pixel_format);
    if let (Some(width), Some(height)) = (config.width, config.height) {
        stream_cfg.set_size(Size::new(width, height));
    }

    match configuration.validate() {
        CameraConfigurationStatus::Invalid => {
            return Err("A configuracao solicitada ficou invalida depois da validacao.".to_string())
        }
        CameraConfigurationStatus::Adjusted | CameraConfigurationStatus::Valid => {}
    }

    camera
        .configure(&mut configuration)
        .map_err(|error| format!("Falha ao configurar a camera: {error}"))?;

    let stream_cfg = configuration
        .get(0)
        .ok_or_else(|| "Nao foi possivel ler o stream configurado.".to_string())?;
    let stream = stream_cfg
        .stream()
        .ok_or_else(|| "O stream configurado nao ficou disponivel depois do configure().".to_string())?;
    let size = stream_cfg.get_size();
    let width = size.width as usize;
    let height = size.height as usize;
    let stride = stream_cfg.get_stride() as usize;

    let mut allocator = FrameBufferAllocator::new(&camera);
    let buffers = allocator
        .alloc(&stream)
        .map_err(|error| format!("Falha ao alocar buffers da camera: {error}"))?;
    let buffers = buffers
        .into_iter()
        .map(|buffer| {
            MemoryMappedFrameBuffer::new(buffer)
                .map_err(|error| format!("Falha ao mapear buffer da camera na memoria: {error}"))
        })
        .collect::<Result<Vec<MemoryMappedFrameBuffer<CameraFrameBuffer>>, String>>()?;

    let mut requests = Vec::with_capacity(buffers.len());
    for (index, buffer) in buffers.into_iter().enumerate() {
        let mut request = camera
            .create_request(Some(index as u64))
            .ok_or_else(|| "Falha ao criar uma requisicao de captura.".to_string())?;
        request
            .add_buffer(&stream, buffer)
            .map_err(|error| format!("Falha ao anexar buffer a requisicao: {error}"))?;
        requests.push(request);
    }

    let request_rx = camera.subscribe_request_completed();
    camera
        .start(None)
        .map_err(|error| format!("Falha ao iniciar a captura: {error}"))?;

    for request in requests {
        camera
            .queue_request(request)
            .map_err(|(_, error)| format!("Falha ao enfileirar requisicao inicial: {error}"))?;
    }

    Ok(PreviewSession {
        camera,
        stream,
        request_rx,
        width,
        height,
        stride,
        profile: AdjustmentProfile::new(config),
        recorder: None,
        pending_recording_output: None,
        last_fps: 0.0,
        fps_window_started: Instant::now(),
        fps_window_frames: 0,
    })
}

fn stop_preview_session(session: &mut PreviewSession<'_>) {
    session.pending_recording_output = None;
    if let Some(recorder) = session.recorder.take() {
        drop(recorder);
    }
    let _ = session.camera.stop();
}

fn process_completed_request(
    session: &mut PreviewSession<'_>,
    config: &CameraConfig,
    request: &mut Request,
    event_tx: &SyncSender<WorkerEvent>,
) -> Result<(), String> {
    let framebuffer = request
        .buffer::<MemoryMappedFrameBuffer<CameraFrameBuffer>>(&session.stream)
        .ok_or_else(|| "O request completado nao trouxe o buffer esperado.".to_string())?;
    let planes = framebuffer.data();
    let plane = planes
        .first()
        .copied()
        .ok_or_else(|| "O request completado nao trouxe dados de imagem.".to_string())?;
    let (preview_width, preview_height) = preview_dimensions(session.width, session.height);
    let needs_full_output = session.recorder.is_some() || session.pending_recording_output.is_some();

    let preview_frame = if needs_full_output {
        let mut output_frame =
            OwnedFrame::from_strided_rgba(session.width, session.height, session.stride, plane)?;
        apply_adjustments(&mut output_frame, &session.profile);

        if let Some(output_path) = session.pending_recording_output.take() {
            fs::create_dir_all(video_library_dir())
                .map_err(|error| format!("Falha ao preparar a pasta da camera: {error}"))?;
            match spawn_video_recorder(
                output_path.clone(),
                output_frame.width,
                output_frame.height,
                config.record_audio,
                &config.audio_source,
                event_tx.clone(),
            ) {
                Ok(recorder) => {
                    recorder.try_send_frame(&output_frame);
                    let backend_label = recorder.backend.ui_label();
                    session.recorder = Some(recorder);
                    let _ = event_tx.send(WorkerEvent::Status(format!(
                        "Gravando video em {} usando {}.",
                        output_path.display(),
                        backend_label
                    )));
                }
                Err(error) => {
                    let _ = event_tx.send(WorkerEvent::Status(error));
                }
            }
        }

        if let Some(recorder) = session.recorder.as_ref() {
            recorder.try_send_frame(&output_frame);
        }

        output_frame.scaled_nearest(preview_width, preview_height)
    } else {
        let mut preview_frame = OwnedFrame::from_strided_rgba_scaled(
            session.width,
            session.height,
            session.stride,
            plane,
            preview_width,
            preview_height,
        )?;
        apply_adjustments(&mut preview_frame, &session.profile);
        preview_frame
    };

    session.fps_window_frames += 1;
    let elapsed = session.fps_window_started.elapsed();
    if elapsed >= Duration::from_millis(400) {
        session.last_fps = session.fps_window_frames as f32 / elapsed.as_secs_f32();
        session.fps_window_started = Instant::now();
        session.fps_window_frames = 0;
    }

    emit_preview_frame(event_tx, preview_frame, session.last_fps);
    Ok(())
}

pub fn capture_photo_max_resolution(
    config: &CameraConfig,
    output_path: &Path,
) -> Result<(usize, usize), String> {
    set_softisp_env(&config.softisp_mode);

    let manager = CameraManager::new()
        .map_err(|error| format!("Falha ao iniciar o libcamera para foto still: {error}"))?;
    capture_photo_max_resolution_with_manager(&manager, config, output_path)
}

fn capture_photo_max_resolution_with_manager(
    manager: &CameraManager,
    config: &CameraConfig,
    output_path: &Path,
) -> Result<(usize, usize), String> {
    let camera_id = manager
        .cameras()
        .iter()
        .next()
        .map(|camera| camera.id().to_string())
        .ok_or_else(|| "Nenhuma camera disponivel para capturar a foto.".to_string())?;
    let camera_ref = manager
        .get(&camera_id)
        .ok_or_else(|| format!("Camera {camera_id} nao ficou acessivel pelo CameraManager."))?;
    let mut camera = camera_ref
        .acquire()
        .map_err(|error| format!("Falha ao adquirir a camera para foto still: {error}"))?;

    let mut configuration = camera
        .generate_configuration(&[StreamRole::StillCapture])
        .ok_or_else(|| "Nao foi possivel gerar a configuracao still da camera.".to_string())?;
    let Some(mut stream_cfg) = configuration.get_mut(0) else {
        return Err("A configuracao still da camera nao retornou um stream valido.".to_string());
    };

    let pixel_format =
        PixelFormat::parse("ABGR8888").ok_or_else(|| "ABGR8888 nao esta disponivel neste host.".to_string())?;
    let max_size = stream_cfg
        .formats()
        .sizes(pixel_format)
        .into_iter()
        .max_by_key(|size| {
            (
                u64::from(size.width) * u64::from(size.height),
                size.width,
                size.height,
            )
        });
    stream_cfg.set_pixel_format(pixel_format);
    if let Some(max_size) = max_size {
        stream_cfg.set_size(max_size);
    }

    match configuration.validate() {
        CameraConfigurationStatus::Invalid => {
            return Err("A configuracao still ficou invalida depois da validacao.".to_string())
        }
        CameraConfigurationStatus::Adjusted | CameraConfigurationStatus::Valid => {}
    }

    let validated_cfg = configuration
        .get(0)
        .ok_or_else(|| "Nao foi possivel ler o stream still validado.".to_string())?;
    if validated_cfg.get_pixel_format() != pixel_format {
        return Err(format!(
            "A camera nao aceitou ABGR8888 para still capture; formato final: {:?}.",
            validated_cfg.get_pixel_format()
        ));
    }

    camera
        .configure(&mut configuration)
        .map_err(|error| format!("Falha ao configurar a camera para foto still: {error}"))?;

    let stream_cfg = configuration
        .get(0)
        .ok_or_else(|| "Nao foi possivel ler o stream still configurado.".to_string())?;
    let stream = stream_cfg
        .stream()
        .ok_or_else(|| "O stream still nao ficou disponivel depois do configure().".to_string())?;
    let size = stream_cfg.get_size();
    let width = size.width as usize;
    let height = size.height as usize;
    let stride = stream_cfg.get_stride() as usize;

    let mut allocator = FrameBufferAllocator::new(&camera);
    let buffer = allocator
        .alloc(&stream)
        .map_err(|error| format!("Falha ao alocar buffer para foto still: {error}"))?
        .into_iter()
        .next()
        .ok_or_else(|| "A camera nao retornou buffer para a captura still.".to_string())?;
    let buffer = MemoryMappedFrameBuffer::new(buffer)
        .map_err(|error| format!("Falha ao mapear buffer da foto still: {error}"))?;

    let mut request = camera
        .create_request(None)
        .ok_or_else(|| "Falha ao criar request para foto still.".to_string())?;
    request
        .add_buffer(&stream, buffer)
        .map_err(|error| format!("Falha ao anexar buffer da foto still: {error}"))?;

    let request_rx = camera.subscribe_request_completed();
    camera
        .start(None)
        .map_err(|error| format!("Falha ao iniciar a camera para foto still: {error}"))?;
    camera
        .queue_request(request)
        .map_err(|(_, error)| format!("Falha ao enfileirar a foto still: {error}"))?;

    let capture_result = (|| {
        let mut final_request = None;
        for frame_index in 0..=STILL_CAPTURE_WARMUP_FRAMES {
            let mut request = request_rx.recv_timeout(Duration::from_secs(5)).map_err(|error| {
                format!("Tempo esgotado aguardando o frame {} da foto still: {error}", frame_index + 1)
            })?;

            if frame_index < STILL_CAPTURE_WARMUP_FRAMES {
                request.reuse(ReuseFlag::REUSE_BUFFERS);
                camera.queue_request(request).map_err(|(_, error)| {
                    format!("Falha ao reenfileirar frame de aquecimento da foto still: {error}")
                })?;
                continue;
            }

            final_request = Some(request);
            break;
        }

        let request = final_request
            .ok_or_else(|| "A foto still nao retornou um frame final valido.".to_string())?;
        let framebuffer = request
            .buffer::<MemoryMappedFrameBuffer<CameraFrameBuffer>>(&stream)
            .ok_or_else(|| "A foto still nao retornou o buffer esperado.".to_string())?;
        let plane = framebuffer
            .data()
            .first()
            .copied()
            .ok_or_else(|| "A foto still nao retornou dados de imagem.".to_string())?;

        let mut frame = OwnedFrame::from_strided_rgba(width, height, stride, plane)?;
        let profile = AdjustmentProfile::new(config);
        apply_adjustments(&mut frame, &profile);
        write_photo_from_frame(&frame, output_path)?;
        Ok((width, height))
    })();

    let _ = camera.stop();
    capture_result
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

pub fn detect_audio_sources() -> Vec<AudioSourceOption> {
    let output = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-sources")
        .arg("pulse")
        .output();

    let Ok(output) = output else {
        return default_audio_sources();
    };

    if !output.status.success() && output.stdout.is_empty() && output.stderr.is_empty() {
        return default_audio_sources();
    }

    let mut combined = String::from_utf8_lossy(&output.stdout).to_string();
    if !output.stderr.is_empty() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(&String::from_utf8_lossy(&output.stderr));
    }

    parse_audio_sources(&combined)
}

fn default_audio_sources() -> Vec<AudioSourceOption> {
    vec![AudioSourceOption {
        id: "default".to_string(),
        label: "Padrao do sistema".to_string(),
    }]
}

fn parse_audio_sources(raw: &str) -> Vec<AudioSourceOption> {
    let mut sources = default_audio_sources();

    for line in raw.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with("Auto-detected sources for ") {
            continue;
        }

        let is_default = trimmed.starts_with('*');
        let entry = trimmed.trim_start_matches('*').trim();
        let Some((id, rest)) = entry.split_once(' ') else {
            continue;
        };
        if id.ends_with(".monitor") {
            continue;
        }

        let label = if let (Some(start), Some(end)) = (rest.find('['), rest.rfind(']')) {
            let text = rest[(start + 1)..end].trim();
            if is_default {
                format!("{text} (padrao atual)")
            } else {
                text.to_string()
            }
        } else if is_default {
            format!("{id} (padrao atual)")
        } else {
            id.to_string()
        };

        if !sources.iter().any(|source| source.id == id) {
            sources.push(AudioSourceOption {
                id: id.to_string(),
                label,
            });
        }
    }

    sources
}

pub fn selected_audio_source_label(options: &[AudioSourceOption], selected_id: &str) -> String {
    options
        .iter()
        .find(|option| option.id == selected_id)
        .map(|option| option.label.clone())
        .unwrap_or_else(|| selected_id.to_string())
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

fn write_photo_from_frame(frame: &OwnedFrame, output_path: &Path) -> Result<(), String> {
    if frame.width == 0 || frame.height == 0 || frame.data.is_empty() {
        return Err("Ainda nao ha frame valido para salvar como foto.".to_string());
    }

    let image = image::RgbaImage::from_raw(frame.width as u32, frame.height as u32, frame.data.clone())
        .ok_or_else(|| "Falha ao montar a imagem RGBA da foto.".to_string())?;
    let file = fs::File::create(output_path)
        .map_err(|error| format!("Falha ao criar o arquivo da foto: {error}"))?;
    let mut writer = std::io::BufWriter::new(file);
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, 92);
    image::DynamicImage::ImageRgba8(image)
        .write_with_encoder(encoder)
        .map_err(|error| format!("Falha ao codificar a foto em JPEG: {error}"))
}

fn normalize_ffmpeg_stderr(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with("libva info:"))
        .collect::<Vec<_>>()
        .join("; ")
}

fn spawn_video_recorder(
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
        .map_err(|error| format!("Falha ao iniciar o ffmpeg para video: {error}"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "O ffmpeg nao abriu stdin para a gravacao.".to_string())?;

    let finished_output = output_path.clone();
    thread::spawn(move || {
        let mut write_error = String::new();
        for frame in frame_receiver {
            if let Err(error) = stdin.write_all(&frame) {
                write_error = format!("Falha ao enviar frame para o ffmpeg: {error}");
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
                    format!("Falha ao finalizar o ffmpeg: {error}")
                } else {
                    format!("{write_error}; falha ao finalizar o ffmpeg: {error}")
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

pub fn normalize_countdown_seconds(value: u32) -> u32 {
    if COUNTDOWN_OPTIONS.contains(&value) {
        value
    } else {
        0
    }
}

pub fn countdown_options() -> &'static [u32] {
    &COUNTDOWN_OPTIONS
}

fn synthetic_smoke_frame() -> OwnedFrame {
    let width = 320;
    let height = 180;
    let mut data = vec![0_u8; width * height * 4];

    for y in 0..height {
        for x in 0..width {
            let offset = (y * width + x) * 4;
            data[offset] = ((x * 255) / width) as u8;
            data[offset + 1] = ((y * 255) / height) as u8;
            data[offset + 2] = 180;
            data[offset + 3] = 255;
        }
    }

    OwnedFrame { width, height, data }
}

fn write_smoke_video(frame: &OwnedFrame, output_path: &Path) -> Result<(), String> {
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
        .map_err(|error| format!("Falha ao iniciar ffmpeg no smoke test: {error}"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "O ffmpeg nao abriu stdin no smoke test.".to_string())?;

    for _ in 0..30 {
        stdin
            .write_all(&frame.data)
            .map_err(|error| format!("Falha ao alimentar o ffmpeg no smoke test: {error}"))?;
    }
    drop(stdin);

    let output = child
        .wait_with_output()
        .map_err(|error| format!("Falha ao finalizar o ffmpeg no smoke test: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(command_stderr_or(
            &output.stderr,
            "ffmpeg falhou no smoke test de video.",
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

pub fn run_smoke_test(config_path: &Path) -> Result<(), String> {
    let config = CameraConfig::load(config_path);
    set_softisp_env(&config.softisp_mode);
    let cameras_count = CameraManager::new()
        .map_err(|error| format!("Falha ao inicializar libcamera no smoke test: {error}"))?
        .cameras()
        .iter()
        .count();
    let photo_path = Path::new("/tmp/galaxybook-camera-smoke-test.jpg");
    let video_path = Path::new("/tmp/galaxybook-camera-smoke-test.mp4");
    let mut frame = synthetic_smoke_frame();
    let profile = AdjustmentProfile::new(&config);
    apply_adjustments(&mut frame, &profile);

    println!("config={}", config_path.display());
    println!("cameras={cameras_count}");
    println!("resolution={}", config.resolution_text());
    println!("photo={}", photo_path.display());
    println!("video={}", video_path.display());

    write_photo_from_frame(&frame, photo_path)?;
    write_smoke_video(&frame, video_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn neutral_config() -> CameraConfig {
        CameraConfig {
            brightness: 0.0,
            exposure_value: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            hue: 0.0,
            temperature: 0.0,
            tint: 0.0,
            red_gain: 1.0,
            green_gain: 1.0,
            blue_gain: 1.0,
            gamma: 1.0,
            sharpness: 1.0,
            mirror: false,
            ..CameraConfig::default()
        }
    }

    fn frame_from_pixels(width: usize, height: usize, pixels: &[(u8, u8, u8)]) -> OwnedFrame {
        let mut data = Vec::with_capacity(width * height * 4);
        for (red, green, blue) in pixels {
            data.extend_from_slice(&[*red, *green, *blue, 255]);
        }

        OwnedFrame { width, height, data }
    }

    fn rgb_at(frame: &OwnedFrame, x: usize, y: usize) -> (u8, u8, u8) {
        let index = (y * frame.width + x) * 4;
        (frame.data[index], frame.data[index + 1], frame.data[index + 2])
    }

    #[test]
    fn brightness_adjustment_changes_pixel_values() {
        let mut config = neutral_config();
        config.brightness = 0.20;

        let mut frame = frame_from_pixels(1, 1, &[(90, 100, 110)]);
        let profile = AdjustmentProfile::new(&config);
        apply_adjustments(&mut frame, &profile);

        let (red, green, blue) = rgb_at(&frame, 0, 0);
        assert!(red > 90);
        assert!(green > 100);
        assert!(blue > 110);
    }

    #[test]
    fn contrast_adjustment_expands_shadow_and_highlight() {
        let mut config = neutral_config();
        config.contrast = 1.40;

        let mut frame = frame_from_pixels(2, 1, &[(64, 64, 64), (192, 192, 192)]);
        let profile = AdjustmentProfile::new(&config);
        apply_adjustments(&mut frame, &profile);

        let dark = rgb_at(&frame, 0, 0).0;
        let bright = rgb_at(&frame, 1, 0).0;
        assert!(dark < 64);
        assert!(bright > 192);
    }

    #[test]
    fn saturation_adjustment_changes_channel_separation() {
        let mut config = neutral_config();
        config.saturation = 0.40;

        let original = (210_u8, 120_u8, 40_u8);
        let mut frame = frame_from_pixels(1, 1, &[original]);
        let profile = AdjustmentProfile::new(&config);
        apply_adjustments(&mut frame, &profile);

        let adjusted = rgb_at(&frame, 0, 0);
        let original_span = original.0.max(original.1).max(original.2) - original.0.min(original.1).min(original.2);
        let adjusted_span = adjusted.0.max(adjusted.1).max(adjusted.2) - adjusted.0.min(adjusted.1).min(adjusted.2);
        assert!(adjusted_span < original_span);
    }

    #[test]
    fn deep_shadow_casts_are_neutralized() {
        let config = neutral_config();

        let original = (18_u8, 36_u8, 24_u8);
        let mut frame = frame_from_pixels(1, 1, &[original]);
        let profile = AdjustmentProfile::new(&config);
        apply_adjustments(&mut frame, &profile);

        let adjusted = rgb_at(&frame, 0, 0);
        let original_span =
            original.0.max(original.1).max(original.2) - original.0.min(original.1).min(original.2);
        let adjusted_span =
            adjusted.0.max(adjusted.1).max(adjusted.2) - adjusted.0.min(adjusted.1).min(adjusted.2);

        assert!(adjusted_span < original_span);
    }

    #[test]
    fn sharpness_adjustment_changes_center_pixel() {
        let mut config = neutral_config();
        config.sharpness = 2.0;

        let mut frame = frame_from_pixels(
            3,
            3,
            &[
                (100, 100, 100),
                (100, 100, 100),
                (100, 100, 100),
                (100, 100, 100),
                (140, 140, 140),
                (100, 100, 100),
                (100, 100, 100),
                (100, 100, 100),
                (100, 100, 100),
            ],
        );
        let profile = AdjustmentProfile::new(&config);
        apply_adjustments(&mut frame, &profile);

        let center = rgb_at(&frame, 1, 1).0;
        assert!(center > 140);
    }

    #[test]
    fn normalize_ffmpeg_stderr_ignores_libva_info() {
        let normalized = normalize_ffmpeg_stderr(
            b"libva info: VA-API version 1.23.0\nreal warning\nlibva info: driver loaded\n",
        );
        assert_eq!(normalized, "real warning");
    }

    #[test]
    fn preview_dimensions_downscale_large_stream_preserving_aspect_ratio() {
        let (width, height) = preview_dimensions(1920, 1092);
        assert_eq!(width, 1280);
        assert_eq!(height, 728);
    }

    #[test]
    fn scaled_preview_samples_expected_pixels() {
        let frame = frame_from_pixels(
            4,
            2,
            &[
                (10, 0, 0),
                (20, 0, 0),
                (30, 0, 0),
                (40, 0, 0),
                (50, 0, 0),
                (60, 0, 0),
                (70, 0, 0),
                (80, 0, 0),
            ],
        );

        let scaled = frame.scaled_nearest(2, 1);
        assert_eq!(scaled.width, 2);
        assert_eq!(scaled.height, 1);
        assert_eq!(rgb_at(&scaled, 0, 0), (10, 0, 0));
        assert_eq!(rgb_at(&scaled, 1, 0), (30, 0, 0));
    }

    #[test]
    fn localized_app_name_uses_portuguese_camera_word() {
        assert_eq!(
            localized_app_name_for_locale("pt_BR.UTF-8"),
            "Galaxy Book Câmera"
        );
    }

    #[test]
    fn localized_app_name_uses_first_language_candidate() {
        assert_eq!(
            app::localization::first_locale_candidate("C:pt_BR.UTF-8:en_US.UTF-8"),
            Some("pt_BR.UTF-8")
        );
    }

    #[test]
    fn localized_camera_word_supports_traditional_chinese() {
        assert_eq!(localized_camera_word_for_locale("zh_TW.UTF-8"), "相機");
    }

    #[test]
    fn normalize_countdown_seconds_accepts_snapshot_values() {
        assert_eq!(normalize_countdown_seconds(0), 0);
        assert_eq!(normalize_countdown_seconds(3), 3);
        assert_eq!(normalize_countdown_seconds(10), 10);
        assert_eq!(normalize_countdown_seconds(5), 0);
    }

    #[test]
    fn natural_preset_returns_to_neutral_baseline() {
        let mut config = CameraConfig::default();
        config.width = Some(1280);
        config.height = Some(720);
        config.brightness = 0.22;
        config.exposure_value = 0.42;
        config.contrast = 1.30;
        config.saturation = 1.45;
        config.temperature = -0.15;
        config.tint = 0.09;
        config.gamma = 1.08;
        config.sharpness = 1.25;

        config.apply_preset(Preset::Natural);

        assert_eq!(config.brightness, 0.0);
        assert_eq!(config.exposure_value, -0.04);
        assert_eq!(config.contrast, 1.04);
        assert_eq!(config.saturation, 1.05);
        assert_eq!(config.temperature, 0.04);
        assert_eq!(config.tint, 0.0);
        assert_eq!(config.gamma, 1.0);
        assert_eq!(config.sharpness, 1.0);
        assert_eq!(config.width, Some(1280));
        assert_eq!(config.height, Some(720));
    }

    #[test]
    fn preview_zoom_options_start_with_one_x() {
        let zoom_options = preview_zoom_options();
        assert_eq!(zoom_options.len(), 5);
        assert_eq!(zoom_options[0].label, "1x");
        assert_eq!(zoom_options[1].label, "2x");
        assert_eq!(zoom_options[2].label, "3x");
        assert_eq!(zoom_options[3].label, "5x");
        assert_eq!(zoom_options[4].label, "10x");
        assert!(zoom_options[0].factor >= 1.0);
    }

    #[test]
    fn ov02c10_simple_tuning_file_is_shipped_with_project() {
        let tuning_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(DEV_TUNING_PATH_RELATIVE);
        assert!(tuning_path.is_file(), "missing tuning file: {}", tuning_path.display());
    }
}
