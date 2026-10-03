//! Screenshot encoding to the requested image format.

use std::path::PathBuf;

use anyhow::{ensure, Context};
use niri_config::Config;
use niri_ipc::ScreenshotFormat;

use super::write_png_rgba8;

#[derive(Debug, PartialEq)]
pub enum ExportRequest {
    ClipboardOnly,
    Save {
        path: Option<PathBuf>,
        format: Option<ScreenshotFormat>,
        quality: Option<f32>,
    },
}

pub struct ExportPlan {
    file: Option<FileExport>,
}

pub struct FileExport {
    destination: Destination,
    encoding: Encoding,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Encoding {
    Png,
    WebpLossless,
    WebpLossy(f32),
    Avif(f32),
}

// Keep libwebp's allocation until the write completes, without copying its bytes.
enum EncodedImage {
    Rust(Vec<u8>),
    Webp(webp::WebPMemory),
}

impl AsRef<[u8]> for EncodedImage {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Rust(bytes) => bytes,
            Self::Webp(bytes) => bytes,
        }
    }
}

enum Destination {
    Explicit(PathBuf),
    Configured(PathBuf),
}

impl ExportPlan {
    pub fn resolve(request: ExportRequest, config: &Config) -> anyhow::Result<Self> {
        let ExportRequest::Save {
            path,
            format,
            quality,
        } = request
        else {
            return Ok(Self { file: None });
        };
        let format = format.unwrap_or(config.screenshot_format);
        if let Some(quality) = quality {
            ensure!(
                (0.0..=100.0).contains(&quality),
                "quality must be between 0 and 100"
            );
            ensure!(
                format != ScreenshotFormat::Avif || quality >= 1.,
                "AVIF quality must be between 1 and 100"
            );
        }
        let destination = if let Some(path) = path {
            if let Some(extension) = path.extension() {
                ensure!(
                    extension
                        .to_str()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case(format.extension())),
                    "screenshot path extension must match {} format",
                    format.extension(),
                );
            }
            Some(Destination::Explicit(path))
        } else {
            super::make_screenshot_path(config, format)?.map(Destination::Configured)
        };
        let encoding = match format {
            ScreenshotFormat::Png => Encoding::Png,
            ScreenshotFormat::Webp => match quality {
                Some(quality) => Encoding::WebpLossy(quality),
                None => Encoding::WebpLossless,
            },
            ScreenshotFormat::Avif => Encoding::Avif(quality.unwrap_or(90.)),
        };
        Ok(Self {
            file: destination.map(|destination| FileExport {
                destination,
                encoding,
            }),
        })
    }

    pub fn into_file(self) -> Option<FileExport> {
        self.file
    }
}

impl FileExport {
    /// Write on the export worker; reuse the already encoded clipboard PNG.
    pub fn write(
        self,
        width: u32,
        height: u32,
        pixels: &[u8],
        png: &[u8],
    ) -> anyhow::Result<PathBuf> {
        let path = match self.destination {
            Destination::Explicit(path) => path,
            Destination::Configured(path) => {
                if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                    std::fs::create_dir_all(parent)
                        .context("error creating screenshot directory")?;
                }
                path
            }
        };
        let encoded;
        let bytes = if self.encoding == Encoding::Png {
            png
        } else {
            encoded = encode_screenshot(self.encoding, width, height, pixels)?;
            encoded.as_ref()
        };
        std::fs::write(&path, bytes).context("error writing screenshot file")?;
        Ok(path)
    }
}

// Keep AVIF encoding off the compositor loop.
const AVIF_SPEED: u8 = 8;

/// Encodes raw RGBA8 screenshot pixels into the requested image format.
///
/// `pixels` must be packed 8-bit RGBA, exactly `width * height * 4` bytes.
///
/// Quality is resolved from this export request before starting the export worker.
fn encode_screenshot(
    encoding: Encoding,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> anyhow::Result<EncodedImage> {
    ensure!(
        width > 0 && height > 0,
        "screenshot dimensions must be non-zero"
    );
    let expected_len = usize::try_from(width)
        .ok()
        .zip(usize::try_from(height).ok())
        .and_then(|(w, h)| w.checked_mul(h))
        .and_then(|px| px.checked_mul(4))
        .context("screenshot dimensions are too large")?;
    ensure!(
        pixels.len() == expected_len,
        "expected {expected_len} pixel bytes, got {}",
        pixels.len()
    );

    let encoded = match encoding {
        Encoding::Png => {
            let mut buf = Vec::new();
            write_png_rgba8(&mut buf, width, height, pixels).context("error encoding PNG")?;
            EncodedImage::Rust(buf)
        }
        Encoding::WebpLossless => {
            let mut buf = Vec::new();
            image_webp::WebPEncoder::new(&mut buf)
                .encode(pixels, width, height, image_webp::ColorType::Rgba8)
                .context("error encoding lossless WebP")?;
            EncodedImage::Rust(buf)
        }
        Encoding::WebpLossy(quality) => {
            let bytes = webp::Encoder::from_rgba(pixels, width, height)
                .encode_simple(false, quality)
                .map_err(|error| anyhow::anyhow!("error encoding lossy WebP: {error:?}"))?;
            EncodedImage::Webp(bytes)
        }
        Encoding::Avif(quality) => {
            let img = ravif::Img::new(
                bytemuck::cast_slice::<u8, rgb::RGBA8>(pixels),
                width as usize,
                height as usize,
            );
            let encoded = ravif::Encoder::new()
                .with_quality(quality)
                .with_alpha_quality(quality)
                .with_speed(AVIF_SPEED)
                .with_bit_depth(ravif::BitDepth::Eight)
                .encode_rgba(img)
                .context("error encoding AVIF")?
                .avif_file;
            EncodedImage::Rust(encoded)
        }
    };

    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_resolution_preserves_override_and_disabled_path_precedence() {
        let config = Config::parse_mem("screenshot-format \"webp\"\nscreenshot-path null").unwrap();
        let request = |path: &str, format| ExportRequest::Save {
            path: Some(path.into()),
            format,
            quality: None,
        };
        let override_file =
            ExportPlan::resolve(request("shot.avif", Some(ScreenshotFormat::Avif)), &config)
                .unwrap()
                .into_file()
                .unwrap();
        assert_eq!(override_file.encoding, Encoding::Avif(90.));
        let next_file = ExportPlan::resolve(request("shot.webp", None), &config)
            .unwrap()
            .into_file()
            .unwrap();
        assert_eq!(next_file.encoding, Encoding::WebpLossless);
        assert!(
            matches!(next_file.destination, Destination::Explicit(path) if path == std::path::Path::new("shot.webp"))
        );
        assert!(ExportPlan::resolve(
            ExportRequest::Save {
                path: None,
                format: None,
                quality: None,
            },
            &config
        )
        .unwrap()
        .into_file()
        .is_none());
        assert!(ExportPlan::resolve(ExportRequest::ClipboardOnly, &config)
            .unwrap()
            .into_file()
            .is_none());
        assert!(ExportPlan::resolve(request("shot.png", None), &config).is_err());
        assert!(ExportPlan::resolve(request("shot", None), &config).is_ok());
    }

    /// Small deterministic RGBA8 image with sharp edges and non-opaque alpha.
    fn test_image(width: usize, height: usize) -> Vec<u8> {
        let mut pixels = vec![0u8; width * height * 4];
        for y in 0..height {
            for x in 0..width {
                let i = (y * width + x) * 4;
                pixels[i] = ((x * 37 + y * 11) % 251) as u8;
                pixels[i + 1] = ((x * 13 + y * 53) % 241) as u8;
                pixels[i + 2] = (((x * 91) ^ (y * 7)) % 233) as u8;
                // Mix fully opaque, semi- and fully transparent pixels.
                pixels[i + 3] = match (x + y) % 4 {
                    0 => 255,
                    1 => 128,
                    2 => 0,
                    _ => 200,
                };
            }
        }
        pixels
    }

    #[test]
    fn png_roundtrip() {
        let (w, h) = (17, 13);
        let pixels = test_image(w, h);
        let encoded = encode_screenshot(Encoding::Png, w as u32, h as u32, &pixels).unwrap();

        let mut decoder = png::Decoder::new(std::io::Cursor::new(encoded.as_ref()))
            .read_info()
            .unwrap();
        let mut decoded = vec![0; decoder.output_buffer_size().unwrap()];
        let info = decoder.next_frame(&mut decoded).unwrap();
        assert_eq!(info.width, w as u32);
        assert_eq!(info.height, h as u32);
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn webp_lossless_roundtrip() {
        let (w, h) = (17, 13);
        let pixels = test_image(w, h);
        let encoded =
            encode_screenshot(Encoding::WebpLossless, w as u32, h as u32, &pixels).unwrap();

        assert_eq!(&encoded.as_ref()[0..4], b"RIFF");
        assert_eq!(&encoded.as_ref()[8..12], b"WEBP");

        let mut decoder =
            image_webp::WebPDecoder::new(std::io::Cursor::new(encoded.as_ref())).unwrap();
        assert_eq!(decoder.dimensions(), (w as u32, h as u32));
        assert!(!decoder.is_lossy());
        let mut decoded = vec![0; decoder.output_buffer_size().unwrap()];
        decoder.read_image(&mut decoded).unwrap();
        assert_eq!(decoded, pixels);
    }

    #[test]
    fn lossy_webp_preserves_alpha_and_uses_lossy_bitstream() {
        let pixels = test_image(17, 13);
        for quality in [0., 75., 100.] {
            let encoded = encode_screenshot(Encoding::WebpLossy(quality), 17, 13, &pixels).unwrap();
            let mut decoder =
                image_webp::WebPDecoder::new(std::io::Cursor::new(encoded.as_ref())).unwrap();
            assert!(decoder.is_lossy());
            assert_eq!(decoder.dimensions(), (17, 13));
            let mut decoded = vec![0; decoder.output_buffer_size().unwrap()];
            decoder.read_image(&mut decoded).unwrap();
            for (original, result) in pixels.chunks_exact(4).zip(decoded.chunks_exact(4)) {
                assert_eq!(original[3], result[3]);
            }
        }
    }

    #[test]
    fn rejects_bad_input() {
        let pixels = test_image(4, 4);

        assert!(encode_screenshot(Encoding::Png, 0, 4, &pixels).is_err());
        assert!(encode_screenshot(Encoding::Png, 4, 4, &pixels[..8]).is_err());
        assert!(encode_screenshot(Encoding::WebpLossless, 4, 4, &pixels[..8]).is_err());
        assert!(encode_screenshot(Encoding::Avif(90.), 4, 4, &pixels[..8]).is_err());
    }
}
