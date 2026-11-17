//! What the user attaches to a message: read, checked and made to fit before it is sent.

use std::borrow::Cow;
use std::fmt;
use std::fs::File;
use std::io::{Cursor, Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::FilterType;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageError, ImageFormat, ImageReader, Limits};
use mr_agent::{Attachment, ImageData};

/// The longest side of an image sent.
pub const MAX_SIDE: u32 = 2000;
/// The most bytes of an image sent: 5,242,880 bytes of base64.
pub const MAX_BYTES: usize = 3_932_160;
/// The largest image file read at all.
pub const READ_LIMIT: u64 = 32 * 1024 * 1024;
/// The longest side of a picture: a chip draws it small, the conversation larger.
pub const THUMB_SIDE: u32 = 320;
/// The most pixels of an image decoded at all, judged by its header.
pub const MAX_PIXELS: u64 = 64_000_000;

/// Where an attachment comes from.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Pasted(ImageData),
    Path(PathBuf),
}

/// Why an attachment cannot be sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    TooLarge,
    Unreadable,
    Name,
}

impl Problem {
    pub fn words(self) -> &'static str {
        match self {
            Problem::TooLarge => "Too large to send",
            Problem::Unreadable => "Cannot be read",
            Problem::Name => "Its name cannot be sent",
        }
    }
}

/// A small picture of an image, straight-alpha RGBA. Never printed.
#[derive(Clone, PartialEq)]
pub struct Thumb {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

impl fmt::Debug for Thumb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Thumb({}x{})", self.width, self.height)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Image,
    File,
    Folder,
}

/// An attachment ready to send, with the picture its chip shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    pub kind: Kind,
    pub attachment: Attachment,
    pub thumb: Option<Thumb>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ChipState {
    Reading,
    Ready(Prepared),
    Problem(Problem),
}

/// One attachment in the input box.
#[derive(Clone, Debug, PartialEq)]
pub struct Chip {
    pub id: u64,
    pub name: String,
    /// Where it came from. A pasted image's bytes are let go once its answer is in: the source
    /// then holds none.
    pub source: Source,
    pub state: ChipState,
}

/// The most bytes read to tell an image from any other file.
const SNIFF: usize = 32;
/// The most times an image too heavy to send is halved.
const HALVINGS: u32 = 3;
const JPEG_QUALITY: u8 = 85;
/// The quality of a photo within the limits that is encoded again only to turn it upright.
const UPRIGHT_QUALITY: u8 = 90;

/// The name a chip shows: "Pasted image", or the path's last part.
pub fn name_of(source: &Source) -> String {
    match source {
        Source::Pasted(_) => "Pasted image".to_string(),
        Source::Path(path) => match path.file_name() {
            Some(name) => name.to_string_lossy().into_owned(),
            None => path.display().to_string(),
        },
    }
}

/// Reads and checks `source`. Slow: the backend's preparing thread only.
pub fn prepare(source: &Source) -> Result<Prepared, Problem> {
    match source {
        Source::Pasted(data) => {
            let format = image_format(data.as_bytes()).ok_or(Problem::Unreadable)?;
            prepare_image(data.clone(), format)
        }
        Source::Path(path) => prepare_path(path),
    }
}

/// The picture of an image stored with a message, made as a chip's thumb is: the format known
/// by its first bytes, the same limits, a JPEG turned upright by its tag. `None` for anything
/// that is not a readable image. Slow: the backend's preparing thread only.
pub fn picture_of(image: &ImageData) -> Option<Thumb> {
    let format = image_format(image.as_bytes())?;
    let (decoded, _) = decode(image.as_bytes(), format, Limits::default()).ok()?;
    if decoded.width() == 0 || decoded.height() == 0 {
        return None;
    }
    Some(thumb(&decoded))
}

fn prepare_path(path: &Path) -> Result<Prepared, Problem> {
    match path.to_str() {
        Some(text) if !text.contains(['\n', '\r']) => {}
        _ => return Err(Problem::Name),
    }
    let metadata = std::fs::metadata(path).map_err(|_| Problem::Unreadable)?;
    let by_path = |kind| Prepared {
        kind,
        attachment: Attachment::File {
            path: path.to_path_buf(),
        },
        thumb: None,
    };
    if metadata.is_dir() {
        return Ok(by_path(Kind::Folder));
    }
    if !metadata.is_file() {
        // A pipe or a device: reading it could wait for ever.
        return Ok(by_path(Kind::File));
    }
    let mut file = File::open(path).map_err(|_| Problem::Unreadable)?;
    let mut first = Vec::with_capacity(SNIFF);
    (&mut file)
        .take(SNIFF as u64)
        .read_to_end(&mut first)
        .map_err(|_| Problem::Unreadable)?;
    let Some(format) = image_format(&first) else {
        return Ok(by_path(Kind::File));
    };
    if metadata.len() > READ_LIMIT {
        return Err(Problem::TooLarge);
    }
    file.rewind().map_err(|_| Problem::Unreadable)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(READ_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Problem::Unreadable)?;
    if bytes.len() as u64 > READ_LIMIT {
        return Err(Problem::TooLarge);
    }
    // The file changed between the two reads.
    if image_format(&bytes) != Some(format) {
        return Err(Problem::Unreadable);
    }
    prepare_image(ImageData::new(bytes), format)
}

/// The format of an image this module sends, from its first bytes.
fn image_format(bytes: &[u8]) -> Option<ImageFormat> {
    match image::guess_format(bytes) {
        Ok(
            format @ (ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::WebP),
        ) => Some(format),
        _ => None,
    }
}

fn prepare_image(data: ImageData, format: ImageFormat) -> Result<Prepared, Problem> {
    let (image, turned) = decode(data.as_bytes(), format, Limits::default())?;
    let (width, height) = (image.width(), image.height());
    if width == 0 || height == 0 {
        return Err(Problem::Unreadable);
    }
    let fits = width <= MAX_SIDE && height <= MAX_SIDE && data.len() <= MAX_BYTES;
    let attachment = if fits && !turned {
        Attachment::Image {
            media_type: format.to_mime_type().to_string(),
            data,
        }
    } else {
        // A photo turned by its tag is sent as its chip shows it: turned, and encoded again.
        let upright = fits
            .then(|| jpeg(&image, UPRIGHT_QUALITY))
            .flatten()
            .filter(|bytes| bytes.len() <= MAX_BYTES);
        let (media_type, bytes) = match upright {
            Some(bytes) => ("image/jpeg", bytes),
            None => shrink(&image, MAX_BYTES)?,
        };
        Attachment::Image {
            media_type: media_type.to_string(),
            data: ImageData::new(bytes),
        }
    };
    Ok(Prepared {
        kind: Kind::Image,
        attachment,
        thumb: Some(thumb(&image)),
    })
}

/// `bytes` decoded as `format` within `limits` and turned upright, and whether its tag turned
/// it. An image of more than `MAX_PIXELS` is not decoded.
fn decode(
    bytes: &[u8],
    format: ImageFormat,
    mut limits: Limits,
) -> Result<(DynamicImage, bool), Problem> {
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits.clone());
    let mut decoder = reader.into_decoder().map_err(problem_of)?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(Problem::TooLarge);
    }
    // `into_decoder` gave the decoder the whole budget. As `ImageReader::decode` does, the
    // output buffer is reserved from it and the decoder is held to what is left.
    limits.reserve(decoder.total_bytes()).map_err(problem_of)?;
    decoder.set_limits(limits).map_err(problem_of)?;
    // A tag that cannot be read leaves the image as it is stored.
    let orientation = if reads_orientation(format) {
        decoder.orientation().unwrap_or(Orientation::NoTransforms)
    } else {
        Orientation::NoTransforms
    };
    let mut image = DynamicImage::from_decoder(decoder).map_err(problem_of)?;
    image.apply_orientation(orientation);
    Ok((image, orientation != Orientation::NoTransforms))
}

/// Whether the orientation tag of an image of `format` is read: a JPEG's only. Any other image is
/// taken as upright, so no other decoder is asked for its metadata.
fn reads_orientation(format: ImageFormat) -> bool {
    format == ImageFormat::Jpeg
}

/// The decoder refuses an image too large for its limits; anything else is unreadable.
fn problem_of(error: ImageError) -> Problem {
    match error {
        ImageError::Limits(_) => Problem::TooLarge,
        _ => Problem::Unreadable,
    }
}

/// `image` scaled to fit `MAX_SIDE` and encoded, halved while it is over `max_bytes`, at most
/// `HALVINGS` times. `TooLarge` when it never fits. An opaque image is first tried as a PNG at
/// the fitted size, which keeps a screenshot lossless; only when that is too heavy does it go
/// as a JPEG, and is halved as one.
fn shrink(image: &DynamicImage, max_bytes: usize) -> Result<(&'static str, Vec<u8>), Problem> {
    let transparent = has_transparency(image);
    let (width, height) = fit(image.width(), image.height(), MAX_SIDE);
    let mut scaled = if (width, height) == (image.width(), image.height()) {
        Cow::Borrowed(image)
    } else {
        Cow::Owned(image.resize_exact(width, height, FilterType::Triangle))
    };
    if !transparent {
        let lossless = encode(&scaled, true).ok_or(Problem::Unreadable)?;
        if lossless.1.len() <= max_bytes {
            return Ok(lossless);
        }
    }
    let mut halvings = 0;
    loop {
        let encoded = encode(&scaled, transparent).ok_or(Problem::Unreadable)?;
        if encoded.1.len() <= max_bytes {
            return Ok(encoded);
        }
        if halvings == HALVINGS {
            return Err(Problem::TooLarge);
        }
        halvings += 1;
        let (width, height) = ((scaled.width() / 2).max(1), (scaled.height() / 2).max(1));
        scaled = Cow::Owned(scaled.resize_exact(width, height, FilterType::Triangle));
    }
}

/// PNG with `lossless`, else JPEG.
fn encode(image: &DynamicImage, lossless: bool) -> Option<(&'static str, Vec<u8>)> {
    if !lossless {
        return Some(("image/jpeg", jpeg(image, JPEG_QUALITY)?));
    }
    let mut bytes = Cursor::new(Vec::new());
    let encoder =
        PngEncoder::new_with_quality(&mut bytes, CompressionType::Default, PngFilter::Adaptive);
    DynamicImage::ImageRgba8(image.to_rgba8())
        .write_with_encoder(encoder)
        .ok()?;
    Some(("image/png", bytes.into_inner()))
}

fn jpeg(image: &DynamicImage, quality: u8) -> Option<Vec<u8>> {
    let mut bytes = Cursor::new(Vec::new());
    let encoder = JpegEncoder::new_with_quality(&mut bytes, quality);
    DynamicImage::ImageRgb8(image.to_rgb8())
        .write_with_encoder(encoder)
        .ok()?;
    Some(bytes.into_inner())
}

/// Whether some pixel of `image` is not fully opaque. An alpha channel alone does not count: a
/// pasted image always has one. The alpha samples are read in place.
fn has_transparency(image: &DynamicImage) -> bool {
    match image {
        DynamicImage::ImageRgba8(pixels) => pixels.pixels().any(|pixel| pixel.0[3] < u8::MAX),
        DynamicImage::ImageLumaA8(pixels) => pixels.pixels().any(|pixel| pixel.0[1] < u8::MAX),
        DynamicImage::ImageRgba16(pixels) => pixels.pixels().any(|pixel| pixel.0[3] < u16::MAX),
        DynamicImage::ImageLumaA16(pixels) => pixels.pixels().any(|pixel| pixel.0[1] < u16::MAX),
        DynamicImage::ImageRgba32F(pixels) => pixels.pixels().any(|pixel| pixel.0[3] < 1.0),
        // A kind a later `image` adds.
        other if other.color().has_alpha() => {
            other.to_rgba8().pixels().any(|pixel| pixel.0[3] < u8::MAX)
        }
        _ => false,
    }
}

fn thumb(image: &DynamicImage) -> Thumb {
    let (width, height) = fit(image.width(), image.height(), THUMB_SIDE);
    let small = image.thumbnail_exact(width, height).to_rgba8();
    Thumb {
        width,
        height,
        rgba: small.into_raw().into(),
    }
}

/// `width` by `height` scaled to fit `side` on both sides, keeping the shape, never enlarged.
fn fit(width: u32, height: u32, side: u32) -> (u32, u32) {
    if width <= side && height <= side {
        return (width, height);
    }
    let scale = f64::from(side) / f64::from(width.max(height));
    let scaled = |length: u32| ((f64::from(length) * scale).round() as u32).clamp(1, side);
    (scaled(width), scaled(height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageFormat, RgbImage, RgbaImage};
    use std::fs;
    use std::io::Cursor;
    use std::path::Path;

    /// `image` encoded as `format`.
    fn encoded(image: &DynamicImage, format: ImageFormat) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, format).unwrap();
        bytes.into_inner()
    }

    /// A file named `name` in `folder` holding `bytes`.
    fn file(folder: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = folder.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    /// An opaque image of `width` by `height`, a gradient.
    fn opaque(width: u32, height: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        }))
    }

    /// An image of `width` by `height` with an alpha channel, a gradient.
    fn with_alpha(width: u32, height: u32) -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([(x % 256) as u8, (y % 256) as u8, 128, 200])
        }))
    }

    /// An image of `width` by `height` with an alpha channel, every byte noise from a fixed seed.
    fn noise(width: u32, height: u32) -> DynamicImage {
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut bytes = vec![0u8; width as usize * height as usize * 4];
        for byte in &mut bytes {
            // xorshift64
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = (state >> 32) as u8;
        }
        DynamicImage::ImageRgba8(RgbaImage::from_raw(width, height, bytes).unwrap())
    }

    /// `noise`, with every pixel fully opaque: an alpha channel and no transparency, as a pasted
    /// photo comes.
    fn opaque_noise(width: u32, height: u32) -> DynamicImage {
        let mut pixels = noise(width, height).into_rgba8();
        for pixel in pixels.pixels_mut() {
            pixel.0[3] = u8::MAX;
        }
        DynamicImage::ImageRgba8(pixels)
    }

    fn prepared(path: &Path) -> Result<Prepared, Problem> {
        prepare(&Source::Path(path.to_path_buf()))
    }

    /// The media type and bytes of a prepared image.
    fn image_of(prepared: &Prepared) -> (&str, &[u8]) {
        assert_eq!(prepared.kind, Kind::Image);
        match &prepared.attachment {
            Attachment::Image { media_type, data } => (media_type, data.as_bytes()),
            Attachment::File { path } => panic!("an image went by path: {path:?}"),
        }
    }

    fn assert_thumb_fits(prepared: &Prepared) {
        let thumb = prepared.thumb.as_ref().expect("an image has a thumb");
        assert!(
            thumb.width <= THUMB_SIDE && thumb.height <= THUMB_SIDE,
            "{thumb:?}"
        );
        assert!(thumb.width >= 1 && thumb.height >= 1, "{thumb:?}");
        assert_eq!(
            thumb.rgba.len(),
            thumb.width as usize * thumb.height as usize * 4
        );
    }

    #[test]
    fn a_small_image_goes_out_byte_for_byte() {
        let folder = tempfile::tempdir().unwrap();
        for (format, name, media) in [
            (ImageFormat::Png, "shot.png", "image/png"),
            (ImageFormat::Jpeg, "photo.jpg", "image/jpeg"),
        ] {
            let bytes = encoded(&opaque(THUMB_SIDE * 3, THUMB_SIDE * 3 / 2), format);
            let path = file(folder.path(), name, &bytes);
            let prepared = prepared(&path).unwrap();
            let (media_type, sent) = image_of(&prepared);
            assert_eq!(media_type, media);
            assert!(sent == bytes.as_slice(), "the bytes of {name} changed");
            assert_thumb_fits(&prepared);
            let thumb = prepared.thumb.unwrap();
            assert_eq!((thumb.width, thumb.height), (THUMB_SIDE, THUMB_SIDE / 2));
        }
    }

    #[test]
    fn a_tiny_image_keeps_its_size_in_the_thumb() {
        let folder = tempfile::tempdir().unwrap();
        let path = file(
            folder.path(),
            "dot.png",
            &encoded(&opaque(3, 2), ImageFormat::Png),
        );
        let thumb = prepared(&path).unwrap().thumb.unwrap();
        assert_eq!((thumb.width, thumb.height), (3, 2));
    }

    #[test]
    fn an_image_is_known_by_its_bytes_not_its_name() {
        let folder = tempfile::tempdir().unwrap();
        let png = encoded(&opaque(4, 4), ImageFormat::Png);
        let notes = file(folder.path(), "notes.txt", &png);
        let prepared_notes = prepared(&notes).unwrap();
        assert_eq!(image_of(&prepared_notes).0, "image/png");

        let shot = file(folder.path(), "shot.png", b"not a picture at all");
        let prepared_shot = prepared(&shot).unwrap();
        assert_eq!(prepared_shot.kind, Kind::File);
        assert_eq!(prepared_shot.attachment, Attachment::File { path: shot });
        assert_eq!(prepared_shot.thumb, None);
    }

    #[test]
    fn a_wide_image_is_scaled_to_fit() {
        let folder = tempfile::tempdir().unwrap();
        let path = file(
            folder.path(),
            "wide.png",
            &encoded(&opaque(3000, 1000), ImageFormat::Png),
        );
        let prepared = prepared(&path).unwrap();
        let (_, sent) = image_of(&prepared);
        let out = image::load_from_memory(sent).unwrap();
        assert_eq!(out.width(), 2000);
        assert!(matches!(out.height(), 666 | 667), "{}", out.height());
        assert_thumb_fits(&prepared);
    }

    #[test]
    fn a_heavy_image_is_halved_until_it_fits() {
        let folder = tempfile::tempdir().unwrap();
        // Written without compression: the test's own encoding is not what it measures.
        let mut bytes = Vec::new();
        let encoder = PngEncoder::new_with_quality(
            &mut bytes,
            CompressionType::Uncompressed,
            PngFilter::NoFilter,
        );
        noise(2000, 2000).write_with_encoder(encoder).unwrap();
        assert!(
            bytes.len() > MAX_BYTES,
            "the test image is not heavy enough"
        );
        let path = file(folder.path(), "noise.png", &bytes);
        let prepared = prepared(&path).unwrap();
        let (media_type, sent) = image_of(&prepared);
        assert_eq!(media_type, "image/png");
        assert!(sent.len() <= MAX_BYTES, "{} bytes", sent.len());
        assert!(sent.len() < bytes.len());
        let out = image::load_from_memory(sent).unwrap();
        assert!(out.width() < 2000 && out.height() < 2000);
        assert_eq!(out.width(), out.height());
    }

    #[test]
    fn an_image_without_alpha_too_heavy_as_a_png_is_encoded_as_jpeg() {
        let folder = tempfile::tempdir().unwrap();
        let photo = DynamicImage::ImageRgb8(opaque_noise(1200, 2400).to_rgb8());
        let path = file(
            folder.path(),
            "tall.png",
            &encoded(&photo, ImageFormat::Png),
        );
        let prepared = prepared(&path).unwrap();
        let (media_type, sent) = image_of(&prepared);
        assert_eq!(media_type, "image/jpeg");
        assert_eq!(image::guess_format(sent).unwrap(), ImageFormat::Jpeg);
        let out = image::load_from_memory(sent).unwrap();
        assert_eq!(
            (out.width(), out.height()),
            (1000, 2000),
            "fitted, not halved"
        );
    }

    #[test]
    fn an_opaque_image_with_an_alpha_channel_is_encoded_as_jpeg() {
        let folder = tempfile::tempdir().unwrap();
        let image = opaque_noise(2400, 1200);
        assert!(image.color().has_alpha());
        let path = file(
            folder.path(),
            "pasted.png",
            &encoded(&image, ImageFormat::Png),
        );
        let prepared = prepared(&path).unwrap();
        let (media_type, sent) = image_of(&prepared);
        assert_eq!(media_type, "image/jpeg");
        assert!(sent.len() <= MAX_BYTES, "{} bytes", sent.len());
        let out = image::load_from_memory_with_format(sent, ImageFormat::Jpeg).unwrap();
        assert_eq!(
            (out.width(), out.height()),
            (2000, 1000),
            "fitted, not halved"
        );
    }

    #[test]
    fn a_large_flat_screenshot_stays_a_png() {
        let folder = tempfile::tempdir().unwrap();
        // A window's worth of flat panels and lines, opaque, with an alpha channel as a paste has.
        let image = DynamicImage::ImageRgba8(RgbaImage::from_fn(2400, 1600, |x, y| {
            if y % 40 == 0 || x % 300 == 0 {
                image::Rgba([60, 60, 70, 255])
            } else if x < 500 {
                image::Rgba([30, 30, 36, 255])
            } else if y < 80 {
                image::Rgba([240, 240, 245, 255])
            } else {
                image::Rgba([250, 250, 250, 255])
            }
        }));
        let path = file(
            folder.path(),
            "screen.png",
            &encoded(&image, ImageFormat::Png),
        );
        let prepared = prepared(&path).unwrap();
        let (media_type, sent) = image_of(&prepared);
        assert_eq!(media_type, "image/png");
        assert!(sent.len() <= MAX_BYTES, "{} bytes", sent.len());
        let out = image::load_from_memory_with_format(sent, ImageFormat::Png).unwrap();
        assert_eq!((out.width(), out.height()), (2000, 1333));
    }

    #[test]
    fn transparency_is_read_from_the_pixels_of_every_kind_with_alpha() {
        use image::{ImageBuffer, LumaA, Rgba};
        let rgba16 = |alpha: u16| {
            DynamicImage::ImageRgba16(ImageBuffer::from_fn(3, 2, |x, y| {
                Rgba([9, 9, 9, if (x, y) == (2, 1) { alpha } else { u16::MAX }])
            }))
        };
        let grey8 = |alpha: u8| {
            DynamicImage::ImageLumaA8(ImageBuffer::from_fn(3, 2, |x, y| {
                LumaA([9, if (x, y) == (2, 1) { alpha } else { u8::MAX }])
            }))
        };
        let grey16 = |alpha: u16| {
            DynamicImage::ImageLumaA16(ImageBuffer::from_fn(3, 2, |x, y| {
                LumaA([9, if (x, y) == (2, 1) { alpha } else { u16::MAX }])
            }))
        };
        for (image, kind) in [
            (rgba16(u16::MAX - 1), "16-bit colour"),
            (grey8(0), "grey"),
            (grey16(u16::MAX - 1), "16-bit grey"),
        ] {
            assert!(has_transparency(&image), "{kind}: one pixel shows through");
        }
        for (image, kind) in [
            (rgba16(u16::MAX), "16-bit colour"),
            (grey8(u8::MAX), "grey"),
            (grey16(u16::MAX), "16-bit grey"),
        ] {
            assert!(!has_transparency(&image), "{kind}: opaque");
        }
    }

    #[test]
    fn one_with_alpha_stays_png() {
        let folder = tempfile::tempdir().unwrap();
        let path = file(
            folder.path(),
            "tall.png",
            &encoded(&with_alpha(10, 2400), ImageFormat::Png),
        );
        let prepared = prepared(&path).unwrap();
        let (media_type, sent) = image_of(&prepared);
        assert_eq!(media_type, "image/png");
        let out = image::load_from_memory_with_format(sent, ImageFormat::Png).unwrap();
        assert_eq!(out.height(), 2000);
        assert!(out.color().has_alpha());
    }

    #[test]
    fn an_image_still_too_heavy_after_three_halvings_is_too_large() {
        let image = noise(64, 64);
        let mut halved = image.clone();
        for _ in 0..3 {
            let side = halved.width() / 2;
            halved = halved.resize_exact(side, side, FilterType::Triangle);
        }
        assert_eq!(halved.width(), 8);
        let (_, last) = encode(&halved, true).unwrap();
        let (media_type, bytes) = shrink(&image, last.len()).unwrap();
        assert_eq!(media_type, "image/png");
        assert!(bytes == last, "the third halving was not what was sent");
        assert!(
            matches!(shrink(&image, last.len() - 1), Err(Problem::TooLarge)),
            "a fourth halving was made"
        );
    }

    #[test]
    fn an_image_over_the_read_limit_is_too_large() {
        let folder = tempfile::tempdir().unwrap();
        let png = encoded(&opaque(2, 2), ImageFormat::Png);
        let path = file(folder.path(), "huge.png", &png);
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(33 * 1024 * 1024)
            .unwrap();
        assert_eq!(prepared(&path), Err(Problem::TooLarge));
    }

    #[test]
    fn a_broken_image_cannot_be_read() {
        let folder = tempfile::tempdir().unwrap();
        let png = encoded(&opaque(64, 64), ImageFormat::Png);
        let path = file(folder.path(), "cut.png", &png[..png.len() / 2]);
        assert_eq!(prepared(&path), Err(Problem::Unreadable));
    }

    #[test]
    fn a_folder_an_empty_file_and_a_pdf_go_by_path() {
        let folder = tempfile::tempdir().unwrap();
        let inner = folder.path().join("src");
        fs::create_dir(&inner).unwrap();
        let empty = file(folder.path(), "empty.png", b"");
        let pdf = file(folder.path(), "paper.pdf", b"%PDF-1.7\n%alex\n");
        for (path, kind) in [
            (inner, Kind::Folder),
            (empty, Kind::File),
            (pdf, Kind::File),
        ] {
            let prepared = prepared(&path).unwrap();
            assert_eq!(prepared.kind, kind, "{path:?}");
            assert_eq!(prepared.attachment, Attachment::File { path });
            assert_eq!(prepared.thumb, None);
        }
    }

    #[test]
    fn a_file_that_is_gone_cannot_be_read() {
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(
            prepared(&folder.path().join("gone.png")),
            Err(Problem::Unreadable)
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_file_without_permission_cannot_be_read() {
        use std::os::unix::fs::PermissionsExt;
        let folder = tempfile::tempdir().unwrap();
        let path = file(
            folder.path(),
            "locked.png",
            &encoded(&opaque(4, 4), ImageFormat::Png),
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::File::open(&path).is_ok() {
            // Root reads it anyway.
            return;
        }
        assert_eq!(prepared(&path), Err(Problem::Unreadable));
    }

    #[test]
    fn a_name_with_a_line_break_cannot_be_sent() {
        let folder = tempfile::tempdir().unwrap();
        for name in ["two\nlines.txt", "carriage\rreturn.txt"] {
            let path = file(folder.path(), name, b"alex");
            assert_eq!(prepared(&path), Err(Problem::Name), "{name:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_name_that_is_not_utf8_cannot_be_sent() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join(OsStr::from_bytes(b"shot\xff.png"));
        assert_eq!(prepared(&path), Err(Problem::Name));
    }

    #[test]
    fn a_pasted_image_is_prepared_as_a_png() {
        let data = ImageData::new(encoded(&with_alpha(40, 20), ImageFormat::Png));
        let prepared = prepare(&Source::Pasted(data.clone())).unwrap();
        let Attachment::Image {
            media_type,
            data: sent,
        } = &prepared.attachment
        else {
            panic!("a pasted image went by path");
        };
        assert_eq!(prepared.kind, Kind::Image);
        assert_eq!(media_type, "image/png");
        // The same bytes, shared, not copied.
        assert_eq!(sent.as_bytes().as_ptr(), data.as_bytes().as_ptr());
        let thumb = prepared.thumb.unwrap();
        assert_eq!((thumb.width, thumb.height), (40, 20));
    }

    #[test]
    fn pasted_bytes_that_are_no_image_cannot_be_read() {
        let pasted = Source::Pasted(ImageData::new(b"alex".to_vec()));
        assert_eq!(prepare(&pasted), Err(Problem::Unreadable));
    }

    #[test]
    fn the_name_of_a_source() {
        let pasted = Source::Pasted(ImageData::new(vec![1u8, 2, 3]));
        assert_eq!(name_of(&pasted), "Pasted image");
        let shot = Source::Path(PathBuf::from("/home/alex/shot.png"));
        assert_eq!(name_of(&shot), "shot.png");
        let folder = Source::Path(PathBuf::from("/home/alex/src/"));
        assert_eq!(name_of(&folder), "src");
        assert_eq!(name_of(&Source::Path(PathBuf::from("/"))), "/");
    }

    #[test]
    fn the_words_of_a_problem() {
        assert_eq!(Problem::TooLarge.words(), "Too large to send");
        assert_eq!(Problem::Unreadable.words(), "Cannot be read");
        assert_eq!(Problem::Name.words(), "Its name cannot be sent");
    }

    /// The CRC of a PNG chunk.
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &byte in bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    /// A PNG whose header says `width` by `height` pixels of RGB, with a few bytes of body.
    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut chunk = |kind: &[u8], body: &[u8]| {
            bytes.extend_from_slice(&(body.len() as u32).to_be_bytes());
            let mut named = kind.to_vec();
            named.extend_from_slice(body);
            bytes.extend_from_slice(&named);
            bytes.extend_from_slice(&crc32(&named).to_be_bytes());
        };
        let mut header = Vec::new();
        header.extend_from_slice(&width.to_be_bytes());
        header.extend_from_slice(&height.to_be_bytes());
        // 8 bits, RGB, deflate, no filter method, no interlace.
        header.extend_from_slice(&[8, 2, 0, 0, 0]);
        chunk(b"IHDR", &header);
        chunk(b"IDAT", &[0x78, 0x9C]);
        chunk(b"IEND", &[]);
        bytes
    }

    /// `jpeg` with an Exif segment saying `orientation`, put right after its start marker.
    fn with_orientation(jpeg: &[u8], orientation: u16) -> Vec<u8> {
        let mut exif = b"Exif\0\0MM\0\x2a\0\0\0\x08".to_vec();
        // One entry: tag 0x0112 (orientation), type SHORT, count 1, the value padded to four.
        exif.extend_from_slice(&[0, 1, 0x01, 0x12, 0, 3, 0, 0, 0, 1]);
        exif.extend_from_slice(&orientation.to_be_bytes());
        exif.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        assert!(jpeg.starts_with(b"\xff\xd8"), "not a JPEG");
        let mut bytes = jpeg[..2].to_vec();
        bytes.extend_from_slice(b"\xff\xe1");
        bytes.extend_from_slice(&(exif.len() as u16 + 2).to_be_bytes());
        bytes.extend_from_slice(&exif);
        bytes.extend_from_slice(&jpeg[2..]);
        bytes
    }

    /// An opaque image of `width` by `height`, blue, with its top left `corner` by `corner` red.
    fn marked(width: u32, height: u32, corner: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_fn(width, height, |x, y| {
            if x < corner && y < corner {
                image::Rgb([255, 0, 0])
            } else {
                image::Rgb([0, 0, 255])
            }
        }))
    }

    fn is_red(image: &DynamicImage, x: u32, y: u32) -> bool {
        let [red, _, blue] = image.to_rgb8().get_pixel(x, y).0;
        red > 200 && blue < 60
    }

    fn is_blue(image: &DynamicImage, x: u32, y: u32) -> bool {
        let [red, _, blue] = image.to_rgb8().get_pixel(x, y).0;
        red < 60 && blue > 200
    }

    #[test]
    fn a_turned_photo_has_an_upright_thumb() {
        let folder = tempfile::tempdir().unwrap();
        // Stored 80 wide and 40 high; orientation 6 turns it a quarter clockwise.
        let bytes = with_orientation(&encoded(&marked(80, 40, 20), ImageFormat::Jpeg), 6);
        let path = file(folder.path(), "photo.jpg", &bytes);
        let prepared = prepared(&path).unwrap();
        let (media_type, sent) = image_of(&prepared);
        assert_eq!(media_type, "image/jpeg");
        // Within the limits, but sent as the chip shows it: turned, and encoded again.
        assert!(
            sent != bytes.as_slice(),
            "a turned photo went out as stored"
        );
        let out = image::load_from_memory_with_format(sent, ImageFormat::Jpeg).unwrap();
        assert_eq!((out.width(), out.height()), (40, 80), "portrait");
        assert!(is_red(&out, 35, 5), "the top right is not red");
        assert!(is_blue(&out, 5, 5), "the top left is not blue");
        let thumb = prepared.thumb.unwrap();
        assert_eq!((thumb.width, thumb.height), (40, 80));
        // The stored top left is now the top right.
        let pixel = |x: u32, y: u32| {
            let at = (y * thumb.width + x) as usize * 4;
            [thumb.rgba[at], thumb.rgba[at + 2]]
        };
        let [red, blue] = pixel(35, 5);
        assert!(red > 200 && blue < 60, "the top right is not red");
        let [red, blue] = pixel(5, 5);
        assert!(red < 60 && blue > 200, "the top left is not blue");
    }

    #[test]
    fn a_photo_tagged_upright_goes_out_byte_for_byte() {
        let folder = tempfile::tempdir().unwrap();
        let bytes = with_orientation(&encoded(&marked(80, 40, 20), ImageFormat::Jpeg), 1);
        let path = file(folder.path(), "photo.jpg", &bytes);
        let prepared = prepared(&path).unwrap();
        let (media_type, sent) = image_of(&prepared);
        assert_eq!(media_type, "image/jpeg");
        assert!(
            sent == bytes.as_slice(),
            "the bytes of an upright photo changed"
        );
    }

    #[test]
    fn a_turned_photo_is_sent_upright_when_it_is_made_smaller() {
        let folder = tempfile::tempdir().unwrap();
        // Stored 2400 wide and 200 high: over the limits, and landscape.
        let bytes = with_orientation(&encoded(&marked(2400, 200, 100), ImageFormat::Jpeg), 6);
        let path = file(folder.path(), "photo.jpg", &bytes);
        let prepared = prepared(&path).unwrap();
        let (media_type, sent) = image_of(&prepared);
        // Two flat colours: lossless fits, so it goes as a PNG.
        assert_eq!(media_type, "image/png");
        let out = image::load_from_memory_with_format(sent, ImageFormat::Png).unwrap();
        // Portrait: 200 by 2400 scaled to fit 2000 high.
        assert_eq!((out.width(), out.height()), (167, 2000));
        // The stored top left corner is now the top right one.
        assert!(is_red(&out, 150, 40), "the top right is not red");
        assert!(is_blue(&out, 40, 40), "the top left is not blue");
        assert!(is_blue(&out, 150, 1000), "the middle right is not blue");
        let thumb = prepared.thumb.unwrap();
        assert!(thumb.height > thumb.width, "{thumb:?}");
    }

    #[test]
    fn only_a_jpeg_is_asked_for_its_orientation() {
        assert!(reads_orientation(ImageFormat::Jpeg));
        for format in [ImageFormat::WebP, ImageFormat::Png, ImageFormat::Gif] {
            assert!(!reads_orientation(format), "{format:?}");
        }
    }

    #[test]
    fn an_image_the_decoder_refuses_for_its_size_is_too_large() {
        let png = encoded(&opaque(64, 64), ImageFormat::Png);
        let mut limits = Limits::default();
        limits.max_alloc = Some(1024);
        assert!(matches!(
            decode(&png, ImageFormat::Png, limits),
            Err(Problem::TooLarge)
        ));
        assert!(decode(&png, ImageFormat::Png, Limits::default()).is_ok());
    }

    #[test]
    fn an_image_of_too_many_pixels_is_too_large_and_is_not_decoded() {
        let folder = tempfile::tempdir().unwrap();
        // The same header and body, small: decoded, and found to have no pixels.
        let small = file(folder.path(), "small.png", &png_header(100, 100));
        assert_eq!(prepared(&small), Err(Problem::Unreadable));
        // 9,000 by 9,000 is over `MAX_PIXELS`: refused before the body is looked at.
        let huge = file(folder.path(), "huge.png", &png_header(9000, 9000));
        assert_eq!(prepared(&huge), Err(Problem::TooLarge));
    }

    #[test]
    fn a_gif_and_a_webp_within_the_limits_keep_their_type_and_bytes() {
        let folder = tempfile::tempdir().unwrap();
        for (format, name, media) in [
            (ImageFormat::Gif, "loop.gif", "image/gif"),
            (ImageFormat::WebP, "shot.webp", "image/webp"),
        ] {
            let bytes = encoded(&with_alpha(30, 20), format);
            let path = file(folder.path(), name, &bytes);
            let prepared = prepared(&path).unwrap();
            let (media_type, sent) = image_of(&prepared);
            assert_eq!(media_type, media);
            assert!(sent == bytes.as_slice(), "the bytes of {name} changed");
            assert_thumb_fits(&prepared);
        }
    }

    #[test]
    fn a_very_thin_image_keeps_a_side_of_one() {
        let folder = tempfile::tempdir().unwrap();
        let path = file(
            folder.path(),
            "line.png",
            &encoded(&opaque(10_000, 1), ImageFormat::Png),
        );
        let prepared = prepared(&path).unwrap();
        let (_, sent) = image_of(&prepared);
        let out = image::load_from_memory(sent).unwrap();
        assert_eq!((out.width(), out.height()), (2000, 1));
        let thumb = prepared.thumb.as_ref().unwrap();
        assert_eq!((thumb.width, thumb.height), (THUMB_SIDE, 1));
    }

    #[test]
    fn a_chips_thumb_fits_the_side_of_a_picture() {
        assert_eq!(THUMB_SIDE, 320);
        let folder = tempfile::tempdir().unwrap();
        let path = file(
            folder.path(),
            "tall.png",
            &encoded(&opaque(THUMB_SIDE, THUMB_SIDE * 4), ImageFormat::Png),
        );
        let prepared = prepared(&path).unwrap();
        assert_thumb_fits(&prepared);
        let thumb = prepared.thumb.unwrap();
        assert_eq!((thumb.width, thumb.height), (THUMB_SIDE / 4, THUMB_SIDE));
    }

    #[test]
    fn a_picture_is_made_of_stored_bytes() {
        let stored = ImageData::new(encoded(
            &opaque(THUMB_SIDE * 2, THUMB_SIDE),
            ImageFormat::Png,
        ));
        let picture = picture_of(&stored).expect("a picture");
        assert_eq!(
            (picture.width, picture.height),
            (THUMB_SIDE, THUMB_SIDE / 2)
        );
        assert_eq!(
            picture.rgba.len(),
            picture.width as usize * picture.height as usize * 4
        );
        // A small one keeps its size.
        let small = ImageData::new(encoded(&with_alpha(30, 20), ImageFormat::WebP));
        let picture = picture_of(&small).expect("a picture");
        assert_eq!((picture.width, picture.height), (30, 20));
        // A photo is turned upright by its tag, as a chip's thumb is.
        let turned = with_orientation(&encoded(&marked(80, 40, 20), ImageFormat::Jpeg), 6);
        let picture = picture_of(&ImageData::new(turned)).expect("a picture");
        assert_eq!((picture.width, picture.height), (40, 80));
        let at = (5 * picture.width + 35) as usize * 4;
        let (red, blue) = (picture.rgba[at], picture.rgba[at + 2]);
        assert!(red > 200 && blue < 60, "the top right is not red");
    }

    #[test]
    fn what_is_not_an_image_gives_no_picture() {
        let text = ImageData::new(b"alex wrote this".to_vec());
        assert!(picture_of(&text).is_none(), "text");
        let png = encoded(&opaque(64, 64), ImageFormat::Png);
        let cut = ImageData::new(png[..png.len() / 2].to_vec());
        assert!(picture_of(&cut).is_none(), "a cut PNG");
        // Over `MAX_PIXELS` by its header: refused before the body is looked at.
        let huge = ImageData::new(png_header(9000, 9000));
        assert!(picture_of(&huge).is_none(), "too many pixels");
        assert!(picture_of(&ImageData::new(Vec::new())).is_none(), "nothing");
    }

    #[test]
    fn a_thumb_is_never_printed() {
        let thumb = Thumb {
            width: 96,
            height: 48,
            rgba: vec![7u8; 96 * 48 * 4].into(),
        };
        assert_eq!(format!("{thumb:?}"), "Thumb(96x48)");
        let prepared = Prepared {
            kind: Kind::Image,
            attachment: Attachment::Image {
                media_type: "image/png".into(),
                data: ImageData::new(vec![7u8; 1000]),
            },
            thumb: Some(thumb),
        };
        let printed = format!("{prepared:?}");
        assert!(!printed.contains("7, 7"), "{printed}");
    }
}
