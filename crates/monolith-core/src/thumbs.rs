//! Image thumbnails (doc 13 §5). Sizes are declared per bucket, so a client cannot ask for
//! arbitrary work. The source header is read first and oversized images are refused before any
//! pixel is decoded; resizing uses `fast_image_resize` (SIMD). Output is cached next to the blob.

use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;

use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer, images::Image};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{DynamicImage, ImageEncoder, ImageReader, Limits};

use crate::error::{CoreError, CoreResult};

/// Largest side of a requested thumbnail.
pub const MAX_SIDE: u32 = 4096;
/// Sources above this many pixels are refused (a 50 MP photo decodes to ~150 MB).
pub const MAX_SOURCE_PIXELS: u64 = 50_000_000;
const JPEG_QUALITY: u8 = 82;

/// `100x100` crop to fill, `100x0` / `0x100` keep the aspect ratio, `100x100f` fit inside the box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    pub w: u32,
    pub h: u32,
    pub fit: bool,
}

impl Spec {
    pub fn parse(s: &str) -> Option<Spec> {
        let (body, fit) = s.strip_suffix('f').map_or((s, false), |b| (b, true));
        let (w, h) = body.split_once('x')?;
        let digits =
            |t: &str| !t.is_empty() && t.len() <= 4 && t.bytes().all(|b| b.is_ascii_digit());
        if !digits(w) || !digits(h) {
            return None;
        }
        let (w, h): (u32, u32) = (w.parse().ok()?, h.parse().ok()?);
        let ok = w <= MAX_SIDE && h <= MAX_SIDE && (w > 0 || h > 0) && (!fit || (w > 0 && h > 0));
        // Canonical text only, so `0100x100` cannot become a second cache entry for `100x100`.
        (ok && s == format!("{w}x{h}{}", if fit { "f" } else { "" })).then_some(Spec { w, h, fit })
    }

    /// Output size for a `sw`x`sh` source.
    fn target(self, sw: u32, sh: u32) -> (u32, u32) {
        let scale = |a: u32, b: u32, c: u32| {
            ((u64::from(a) * u64::from(b) + u64::from(c) / 2) / u64::from(c)).max(1) as u32
        };
        match (self.w, self.h) {
            (w, 0) => (w, scale(sh, w, sw)),
            (0, h) => (scale(sw, h, sh), h),
            (w, h) if self.fit => {
                // Fit inside the box, never enlarging.
                if sw <= w && sh <= h {
                    (sw, sh)
                } else if u64::from(sw) * u64::from(h) > u64::from(sh) * u64::from(w) {
                    (w, scale(sh, w, sw))
                } else {
                    (scale(sw, h, sh), h)
                }
            }
            (w, h) => (w, h),
        }
    }
}

/// Source types the thumbnailer decodes.
pub fn supported(mime: &str) -> bool {
    matches!(
        mime.split(';').next().map(str::trim),
        Some("image/png" | "image/jpeg" | "image/gif" | "image/webp")
    )
}

fn bad(msg: impl Into<String>) -> CoreError {
    CoreError::BadRequest(msg.into())
}

/// Decodes `src`, resizes to `spec` and writes the result to `dst`; returns the output MIME type.
/// Blocking and CPU heavy: run on the bounded thumbnail pool.
pub fn make(src: &Path, spec: Spec, dst: &Path) -> CoreResult<&'static str> {
    let open = || -> CoreResult<_> {
        let f = File::open(src).map_err(|e| CoreError::Internal(e.to_string()))?;
        ImageReader::new(BufReader::new(f))
            .with_guessed_format()
            .map_err(|e| CoreError::Internal(e.to_string()))
    };
    let (w, h) = open()?
        .into_dimensions()
        .map_err(|e| bad(format!("cannot read the image: {e}")))?;
    if u64::from(w) * u64::from(h) > MAX_SOURCE_PIXELS {
        return Err(CoreError::TooLarge(format!(
            "image over {MAX_SOURCE_PIXELS} pixels cannot be thumbnailed"
        )));
    }
    let mut reader = open()?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(u64::from(w) * u64::from(h) * 8 + (16 << 20));
    reader.limits(limits);
    let img = reader
        .decode()
        .map_err(|e| bad(format!("cannot decode the image: {e}")))?;
    let alpha = img.color().has_alpha();
    let src_img = if alpha {
        DynamicImage::ImageRgba8(img.into_rgba8())
    } else {
        DynamicImage::ImageRgb8(img.into_rgb8())
    };
    let (tw, th) = spec.target(w, h);
    let pixel = fast_image_resize::PixelType::U8x3;
    let pixel = if alpha {
        fast_image_resize::PixelType::U8x4
    } else {
        pixel
    };
    let mut out = Image::new(tw, th, pixel);
    let mut opts = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3));
    if spec.w > 0 && spec.h > 0 && !spec.fit {
        opts = opts.fit_into_destination(Some((0.5, 0.5)));
    }
    Resizer::new()
        .resize(&src_img, &mut out, &opts)
        .map_err(|e| CoreError::Internal(e.to_string()))?;
    let mut w_out =
        BufWriter::new(File::create(dst).map_err(|e| CoreError::Internal(e.to_string()))?);
    let (color, mime) = if alpha {
        (image::ExtendedColorType::Rgba8, "image/png")
    } else {
        (image::ExtendedColorType::Rgb8, "image/jpeg")
    };
    let enc = if alpha {
        PngEncoder::new(&mut w_out).write_image(out.buffer(), tw, th, color)
    } else {
        JpegEncoder::new_with_quality(&mut w_out, JPEG_QUALITY).write_image(
            out.buffer(),
            tw,
            th,
            color,
        )
    };
    enc.map_err(|e| CoreError::Internal(e.to_string()))?;
    w_out
        .flush()
        .map_err(|e| CoreError::Internal(e.to_string()))?;
    Ok(mime)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_grammar() {
        let p = Spec::parse;
        assert_eq!(
            p("100x100"),
            Some(Spec {
                w: 100,
                h: 100,
                fit: false
            })
        );
        assert_eq!(
            p("0x300"),
            Some(Spec {
                w: 0,
                h: 300,
                fit: false
            })
        );
        assert_eq!(
            p("400x400f"),
            Some(Spec {
                w: 400,
                h: 400,
                fit: true
            })
        );
        for bad in [
            "", "0x0", "100", "x100", "100x", "0100x100", "5000x10", "100x100g", "100x0f", "-1x5",
            "1 x5",
        ] {
            assert_eq!(p(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn target_sizes() {
        let t = |s: &str, w, h| Spec::parse(s).unwrap().target(w, h);
        assert_eq!(t("100x100", 400, 200), (100, 100), "crop to fill");
        assert_eq!(t("100x0", 400, 200), (100, 50));
        assert_eq!(t("0x100", 400, 200), (200, 100));
        assert_eq!(t("100x100f", 400, 200), (100, 50), "fit inside");
        assert_eq!(t("100x100f", 200, 400), (50, 100));
        assert_eq!(
            t("500x500f", 400, 200),
            (400, 200),
            "no enlarging in fit mode"
        );
        assert_eq!(t("1x0", 10, 1000), (1, 100));
        assert_eq!(t("0x1", 1000, 10), (100, 1));
    }
}
