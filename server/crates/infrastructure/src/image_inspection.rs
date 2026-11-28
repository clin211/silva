//! 按实际字节识别图片格式并完整解码，校验尺寸与像素上限。
//!
//! 解码在阻塞线程池执行，并用信号量限制同时解码的数量，避免多个大图同时解码占满内存。

use async_trait::async_trait;
use image::{
    ImageError, ImageFormat, ImageReader, Limits,
    error::{LimitError, LimitErrorKind},
};
use silva_application::ports::ImageInspector;
use silva_domain::media::{
    MAX_IMAGE_DIMENSION, MAX_IMAGE_PIXELS, MAX_UPLOAD_BYTES, RejectionReason, VerifiedImage,
    check_dimensions,
};
use silva_shared::error::{AppError, Result};
use std::{io::Cursor, sync::Arc};
use tokio::sync::Semaphore;

/// 同时解码的图片数量上限。单次解码最多分配约 336 MB，一次只解码一张。
const CONCURRENT_DECODES: usize = 1;
/// 支持的格式中每像素最多占用的字节数：16 位 RGBA PNG 为 8 字节。
const MAX_BYTES_PER_PIXEL: u64 = 8;
/// 解码器允许分配的内存上限：按最宽的像素格式计算像素缓冲，再加少量解码器工作内存，
/// 保证像素数在上限内的合法图片不会因内存上限被拒绝。
const MAX_DECODE_ALLOC: u64 = MAX_IMAGE_PIXELS * MAX_BYTES_PER_PIXEL + 16 * 1024 * 1024;
// 编译期保证：像素数达到上限的 16 位 RGBA 图片，像素缓冲也在解码内存上限之内。
const _: () = assert!(MAX_IMAGE_PIXELS * MAX_BYTES_PER_PIXEL < MAX_DECODE_ALLOC);

pub struct DecodingInspector {
    permits: Arc<Semaphore>,
}

impl Default for DecodingInspector {
    fn default() -> Self {
        Self {
            permits: Arc::new(Semaphore::new(CONCURRENT_DECODES)),
        }
    }
}

#[async_trait]
impl ImageInspector for DecodingInspector {
    #[tracing::instrument(name = "media.inspect", skip_all)]
    async fn inspect(
        &self,
        bytes: Vec<u8>,
    ) -> Result<(Vec<u8>, std::result::Result<VerifiedImage, RejectionReason>)> {
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|error| AppError::Internal(error.into()))?;
        tokio::task::spawn_blocking(move || {
            // 请求取消后阻塞解码仍会继续，许可必须由解码线程持有。
            let _permit = permit;
            let verdict = inspect_bytes(&bytes);
            (bytes, verdict)
        })
        .await
        .map_err(|error| AppError::Internal(error.into()))
    }
}

/// 识别并解码图片。最终 MIME 类型以实际格式为准，与客户端声明无关。
pub fn inspect_bytes(bytes: &[u8]) -> std::result::Result<VerifiedImage, RejectionReason> {
    if bytes.len() as u64 > MAX_UPLOAD_BYTES {
        return Err(RejectionReason::TooLarge);
    }
    let format = image::guess_format(bytes).map_err(|_| RejectionReason::UnsupportedFormat)?;
    let mime_type = match format {
        ImageFormat::Jpeg => "image/jpeg",
        ImageFormat::Png => "image/png",
        ImageFormat::WebP => "image/webp",
        _ => return Err(RejectionReason::UnsupportedFormat),
    };
    let reader = || {
        let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
        let mut limits = Limits::default();
        limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
        limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
        limits.max_alloc = Some(MAX_DECODE_ALLOC);
        reader.limits(limits);
        reader
    };
    // 先只读文件头取得尺寸，超限时不分配像素缓冲。
    let (width, height) = reader().into_dimensions().map_err(rejection)?;
    check_dimensions(width, height)?;
    reader().decode().map_err(rejection)?;
    Ok(VerifiedImage {
        mime_type,
        size_bytes: bytes.len() as u64,
        width,
        height,
    })
}

fn rejection(error: ImageError) -> RejectionReason {
    match error {
        ImageError::Limits(limit) => limit_rejection(&limit),
        ImageError::Unsupported(_) => RejectionReason::UnsupportedFormat,
        _ => RejectionReason::Undecodable,
    }
}

/// 只有尺寸超限才报告为 `dimensions_exceeded`。内存上限已按最宽像素格式留足，
/// 仍超出说明解码器需要异常多的工作内存，按无法完整解码处理。
fn limit_rejection(error: &LimitError) -> RejectionReason {
    match error.kind() {
        LimitErrorKind::DimensionError => RejectionReason::DimensionsExceeded,
        _ => RejectionReason::Undecodable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, RgbImage};

    fn encode(width: u32, height: u32, format: ImageFormat) -> Vec<u8> {
        let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(
            width,
            height,
            image::Rgb([200, 80, 40]),
        ));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, format).unwrap();
        bytes.into_inner()
    }

    #[test]
    fn accepts_jpeg_png_and_webp_by_actual_bytes() {
        for (format, mime) in [
            (ImageFormat::Jpeg, "image/jpeg"),
            (ImageFormat::Png, "image/png"),
            (ImageFormat::WebP, "image/webp"),
        ] {
            let bytes = encode(3, 2, format);
            let image = inspect_bytes(&bytes).unwrap();
            assert_eq!(image.mime_type, mime);
            assert_eq!((image.width, image.height), (3, 2));
            assert_eq!(image.size_bytes, bytes.len() as u64);
        }
    }

    #[test]
    fn rejects_other_formats_and_non_images() {
        assert_eq!(
            inspect_bytes(b"GIF89a\x01\x00\x01\x00\x00\x00\x00;"),
            Err(RejectionReason::UnsupportedFormat)
        );
        assert_eq!(
            inspect_bytes(b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>"),
            Err(RejectionReason::UnsupportedFormat)
        );
        assert_eq!(inspect_bytes(b""), Err(RejectionReason::UnsupportedFormat));
    }

    #[test]
    fn rejects_truncated_images() {
        let bytes = encode(64, 64, ImageFormat::Png);
        assert_eq!(
            inspect_bytes(&bytes[..bytes.len() / 2]),
            Err(RejectionReason::Undecodable)
        );
        let jpeg = encode(64, 64, ImageFormat::Jpeg);
        assert!(inspect_bytes(&jpeg[..20]).is_err());
    }

    #[test]
    fn rejects_oversized_dimensions_before_decoding() {
        let bytes = encode(MAX_IMAGE_DIMENSION + 1, 1, ImageFormat::Png);
        assert_eq!(
            inspect_bytes(&bytes),
            Err(RejectionReason::DimensionsExceeded)
        );
    }

    #[test]
    fn accepts_sixteen_bit_png_and_sizes_alloc_for_widest_pixels() {
        let image = DynamicImage::ImageRgba16(image::ImageBuffer::from_pixel(
            4,
            3,
            image::Rgba([65535, 0, 32768, 65535]),
        ));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, ImageFormat::Png).unwrap();
        let verified = inspect_bytes(&bytes.into_inner()).unwrap();
        assert_eq!((verified.width, verified.height), (4, 3));
    }

    #[test]
    fn only_dimension_limits_report_dimensions_exceeded() {
        assert_eq!(
            limit_rejection(&LimitError::from_kind(LimitErrorKind::DimensionError)),
            RejectionReason::DimensionsExceeded
        );
        assert_eq!(
            limit_rejection(&LimitError::from_kind(LimitErrorKind::InsufficientMemory)),
            RejectionReason::Undecodable
        );
    }

    #[tokio::test]
    async fn inspector_returns_bytes_with_verdict() {
        let bytes = encode(2, 2, ImageFormat::Png);
        let (returned, verdict) = DecodingInspector::default()
            .inspect(bytes.clone())
            .await
            .unwrap();
        assert_eq!(returned, bytes);
        assert_eq!(verdict.unwrap().width, 2);
    }
}
