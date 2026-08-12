use std::fs;
use std::path::Path;
use std::time::Duration;

use libcamera::{
    camera::CameraConfigurationStatus,
    camera_manager::CameraManager,
    framebuffer_allocator::{FrameBuffer as CameraFrameBuffer, FrameBufferAllocator},
    framebuffer_map::MemoryMappedFrameBuffer,
    pixel_format::PixelFormat,
    request::{RequestStatus, ReuseFlag},
    stream::StreamRole,
};

use crate::{
    apply_adjustments,
    preferred_camera_id,
    tr,
    trf,
    AdjustmentProfile,
    CameraConfig,
    OwnedFrame,
    STILL_CAPTURE_WARMUP_FRAMES,
};

pub fn capture_photo_max_resolution(
    config: &CameraConfig,
    output_path: &Path,
) -> Result<(usize, usize), String> {
    let manager = CameraManager::new()
        .map_err(|error| trf("Falha ao iniciar o libcamera para foto still: {error}", &[("error", error.to_string())]))?;
    capture_photo_max_resolution_with_manager(&manager, config, output_path)
}

pub(crate) fn capture_photo_max_resolution_with_manager(
    manager: &CameraManager,
    config: &CameraConfig,
    output_path: &Path,
) -> Result<(usize, usize), String> {
    let mut frame = capture_still_frame_with_manager(
        manager,
        STILL_CAPTURE_WARMUP_FRAMES,
        Duration::from_secs(5),
    )?;
    let profile = AdjustmentProfile::new(config);
    apply_adjustments(&mut frame, &profile);
    write_photo_from_frame(&frame, output_path)?;
    Ok((frame.width, frame.height))
}

pub(crate) fn capture_still_frame_with_manager(
    manager: &CameraManager,
    warmup_frames: u32,
    frame_timeout: Duration,
) -> Result<OwnedFrame, String> {
    let camera_id = preferred_camera_id(manager)
        .ok_or_else(|| tr("Nenhuma câmera disponível para capturar a foto."))?;
    let camera_ref = manager
        .get(&camera_id)
        .ok_or_else(|| trf("Câmera {camera_id} não ficou acessível pelo CameraManager.", &[("camera_id", camera_id.clone())]))?;
    let mut camera = camera_ref
        .acquire()
        .map_err(|error| trf("Falha ao adquirir a câmera para foto still: {error}", &[("error", error.to_string())]))?;

    let mut configuration = camera
        .generate_configuration(&[StreamRole::StillCapture])
        .ok_or_else(|| tr("Não foi possível gerar a configuração still da câmera."))?;
    let Some(mut stream_cfg) = configuration.get_mut(0) else {
        return Err(tr("A configuração still da câmera não retornou um stream válido."));
    };

    let pixel_format = PixelFormat::parse("ABGR8888")
        .ok_or_else(|| tr("ABGR8888 não está disponível neste host."))?;
    let formats = stream_cfg.formats();
    let range = formats.range(pixel_format);
    let mut sizes = formats.sizes(pixel_format);
    if range.max.width > 0 && range.max.height > 0 {
        sizes.push(range.max);
    }
    let max_size = sizes.into_iter().max_by_key(|size| {
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
            return Err(tr("A configuração still ficou inválida depois da validação."))
        }
        CameraConfigurationStatus::Adjusted | CameraConfigurationStatus::Valid => {}
    }

    let validated_cfg = configuration
        .get(0)
        .ok_or_else(|| tr("Não foi possível ler o stream still validado."))?;
    if validated_cfg.get_pixel_format() != pixel_format {
        return Err(trf(
            "A câmera não aceitou ABGR8888 para still capture; formato final: {pixel_format}.",
            &[("pixel_format", format!("{:?}", validated_cfg.get_pixel_format()))],
        ));
    }

    camera
        .configure(&mut configuration)
        .map_err(|error| trf("Falha ao configurar a câmera para foto still: {error}", &[("error", error.to_string())]))?;

    let stream_cfg = configuration
        .get(0)
        .ok_or_else(|| tr("Não foi possível ler o stream still configurado."))?;
    let stream = stream_cfg.stream().ok_or_else(|| {
        tr("O stream still não ficou disponível depois do configure().")
    })?;
    let size = stream_cfg.get_size();
    let width = size.width as usize;
    let height = size.height as usize;
    let stride = stream_cfg.get_stride() as usize;

    let mut allocator = FrameBufferAllocator::new(&camera);
    let buffer = allocator
        .alloc(&stream)
        .map_err(|error| trf("Falha ao alocar buffer para foto still: {error}", &[("error", error.to_string())]))?
        .into_iter()
        .next()
        .ok_or_else(|| tr("A câmera não retornou buffer para a captura still."))?;
    let buffer = MemoryMappedFrameBuffer::new(buffer)
        .map_err(|error| trf("Falha ao mapear buffer da foto still: {error}", &[("error", error.to_string())]))?;

    let mut request = camera
        .create_request(None)
        .ok_or_else(|| tr("Falha ao criar request para foto still."))?;
    request
        .add_buffer(&stream, buffer)
        .map_err(|error| trf("Falha ao anexar buffer da foto still: {error}", &[("error", error.to_string())]))?;

    let request_rx = camera.subscribe_request_completed();
    camera
        .start(None)
        .map_err(|error| trf("Falha ao iniciar a câmera para foto still: {error}", &[("error", error.to_string())]))?;
    camera
        .queue_request(request)
        .map_err(|(_, error)| trf("Falha ao enfileirar a foto still: {error}", &[("error", error.to_string())]))?;

    let capture_result = (|| {
        let mut final_request = None;
        for frame_index in 0..=warmup_frames {
            let mut request = request_rx.recv_timeout(frame_timeout).map_err(|error| {
                trf(
                    "Tempo esgotado aguardando o frame {frame_index} da foto still: {error}",
                    &[
                        ("frame_index", (frame_index + 1).to_string()),
                        ("error", error.to_string()),
                    ],
                )
            })?;
            ensure_completed_request(request.status(), frame_index + 1)?;

            if frame_index < warmup_frames {
                request.reuse(ReuseFlag::REUSE_BUFFERS);
                camera.queue_request(request).map_err(|(_, error)| {
                    trf(
                        "Falha ao reenfileirar frame de aquecimento da foto still: {error}",
                        &[("error", error.to_string())],
                    )
                })?;
                continue;
            }

            final_request = Some(request);
            break;
        }

        let request = final_request
            .ok_or_else(|| tr("A foto still não retornou um frame final válido."))?;
        let framebuffer = request
            .buffer::<MemoryMappedFrameBuffer<CameraFrameBuffer>>(&stream)
            .ok_or_else(|| tr("A foto still não retornou o buffer esperado."))?;
        let plane = framebuffer
            .data()
            .first()
            .copied()
            .ok_or_else(|| tr("A foto still não retornou dados de imagem."))?;

        OwnedFrame::from_strided_rgba(width, height, stride, plane)
    })();

    let _ = camera.stop();
    capture_result
}

fn ensure_completed_request(status: RequestStatus, frame_number: u32) -> Result<(), String> {
    if status == RequestStatus::Complete {
        return Ok(());
    }

    Err(trf(
        "O frame {frame_number} da foto still terminou com estado inesperado: {status}.",
        &[
            ("frame_number", frame_number.to_string()),
            ("status", format!("{status:?}")),
        ],
    ))
}

pub(crate) fn write_photo_from_frame(
    frame: &OwnedFrame,
    output_path: &Path,
) -> Result<(), String> {
    if frame.width == 0 || frame.height == 0 || frame.data.is_empty() {
        return Err(tr("Ainda não há frame válido para salvar como foto."));
    }

    let image = ::image::RgbaImage::from_raw(
        frame.width as u32,
        frame.height as u32,
        frame.data.clone(),
    )
    .ok_or_else(|| tr("Falha ao montar a imagem RGBA da foto."))?;
    let file = fs::File::create(output_path)
        .map_err(|error| trf("Falha ao criar o arquivo da foto: {error}", &[("error", error.to_string())]))?;
    let mut writer = std::io::BufWriter::new(file);
    let encoder = ::image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, 92);
    ::image::DynamicImage::ImageRgba8(image)
        .write_with_encoder(encoder)
        .map_err(|error| trf("Falha ao codificar a foto em JPEG: {error}", &[("error", error.to_string())]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn still_capture_accepts_only_completed_requests() {
        assert!(ensure_completed_request(RequestStatus::Complete, 1).is_ok());

        let pending = ensure_completed_request(RequestStatus::Pending, 2)
            .expect_err("pending request must be rejected");
        assert!(pending.contains('2'));
        assert!(pending.contains("Pending"));

        let cancelled = ensure_completed_request(RequestStatus::Cancelled, 3)
            .expect_err("cancelled request must be rejected");
        assert!(cancelled.contains('3'));
        assert!(cancelled.contains("Cancelled"));
    }
}
