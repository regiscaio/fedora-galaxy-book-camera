use std::fs;
use std::path::Path;
use std::time::Duration;

use libcamera::{
    camera::CameraConfigurationStatus,
    camera_manager::CameraManager,
    framebuffer_allocator::{FrameBuffer as CameraFrameBuffer, FrameBufferAllocator},
    framebuffer_map::MemoryMappedFrameBuffer,
    pixel_format::PixelFormat,
    request::ReuseFlag,
    stream::StreamRole,
};

use crate::{
    apply_adjustments,
    set_softisp_env,
    AdjustmentProfile,
    CameraConfig,
    OwnedFrame,
    STILL_CAPTURE_WARMUP_FRAMES,
};

pub fn capture_photo_max_resolution(
    config: &CameraConfig,
    output_path: &Path,
) -> Result<(usize, usize), String> {
    set_softisp_env(&config.softisp_mode);

    let manager = CameraManager::new()
        .map_err(|error| format!("Falha ao iniciar o libcamera para foto still: {error}"))?;
    capture_photo_max_resolution_with_manager(&manager, config, output_path)
}

pub(crate) fn capture_photo_max_resolution_with_manager(
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

    let pixel_format = PixelFormat::parse("ABGR8888")
        .ok_or_else(|| "ABGR8888 nao esta disponivel neste host.".to_string())?;
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
    let stream = stream_cfg.stream().ok_or_else(|| {
        "O stream still nao ficou disponivel depois do configure().".to_string()
    })?;
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
                format!(
                    "Tempo esgotado aguardando o frame {} da foto still: {error}",
                    frame_index + 1
                )
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

pub(crate) fn write_photo_from_frame(
    frame: &OwnedFrame,
    output_path: &Path,
) -> Result<(), String> {
    if frame.width == 0 || frame.height == 0 || frame.data.is_empty() {
        return Err("Ainda nao ha frame valido para salvar como foto.".to_string());
    }

    let image = ::image::RgbaImage::from_raw(
        frame.width as u32,
        frame.height as u32,
        frame.data.clone(),
    )
    .ok_or_else(|| "Falha ao montar a imagem RGBA da foto.".to_string())?;
    let file = fs::File::create(output_path)
        .map_err(|error| format!("Falha ao criar o arquivo da foto: {error}"))?;
    let mut writer = std::io::BufWriter::new(file);
    let encoder = ::image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, 92);
    ::image::DynamicImage::ImageRgba8(image)
        .write_with_encoder(encoder)
        .map_err(|error| format!("Falha ao codificar a foto em JPEG: {error}"))
}
