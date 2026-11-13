use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Cursor;
use std::sync::Arc;

/// Encoded image formats the app carries around as bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Webp,
    Gif,
    Svg,
    Bmp,
    Tiff,
    Ico,
    Pnm,
}

impl ImageFormat {
    pub const ALL: [ImageFormat; 9] = [
        ImageFormat::Png,
        ImageFormat::Jpeg,
        ImageFormat::Webp,
        ImageFormat::Gif,
        ImageFormat::Svg,
        ImageFormat::Bmp,
        ImageFormat::Tiff,
        ImageFormat::Ico,
        ImageFormat::Pnm,
    ];

    /// Same strings as GPUI's `ImageFormat::mime_type`, which stored drafts were written with.
    pub const fn mime_type(self) -> &'static str {
        match self {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
            ImageFormat::Webp => "image/webp",
            ImageFormat::Gif => "image/gif",
            ImageFormat::Svg => "image/svg+xml",
            ImageFormat::Bmp => "image/bmp",
            ImageFormat::Tiff => "image/tiff",
            ImageFormat::Ico => "image/ico",
            ImageFormat::Pnm => "image/x-portable-anymap",
        }
    }
}

/// Encoded image bytes with a content id, cheap to clone. Views turn it into toolkit images.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageData {
    pub format: ImageFormat,
    pub bytes: Arc<[u8]>,
    /// Hash of `bytes`, so views can cache by it without hashing again.
    pub id: u64,
}

impl ImageData {
    pub fn from_bytes(format: ImageFormat, bytes: impl Into<Arc<[u8]>>) -> Self {
        let bytes = bytes.into();
        let mut hasher = DefaultHasher::new();
        bytes.hash(&mut hasher);
        ImageData {
            format,
            bytes,
            id: hasher.finish(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl InlineFormat {
    pub fn image_format(self) -> ImageFormat {
        match self {
            InlineFormat::Png => ImageFormat::Png,
            InlineFormat::Jpeg => ImageFormat::Jpeg,
            InlineFormat::Gif => ImageFormat::Gif,
            InlineFormat::Webp => ImageFormat::Webp,
        }
    }

    pub fn decoder_format(self) -> Option<image::ImageFormat> {
        match self {
            InlineFormat::Png => Some(image::ImageFormat::Png),
            InlineFormat::Jpeg => Some(image::ImageFormat::Jpeg),
            InlineFormat::Webp => Some(image::ImageFormat::WebP),
            InlineFormat::Gif => None,
        }
    }
}

pub fn sniff_inline_format(bytes: &[u8]) -> Option<InlineFormat> {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => Some(InlineFormat::Png),
        [0xFF, 0xD8, 0xFF, ..] => Some(InlineFormat::Jpeg),
        [b'G', b'I', b'F', b'8', ..] => Some(InlineFormat::Gif),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => Some(InlineFormat::Webp),
        _ => None,
    }
}

pub fn image_dimensions(format: InlineFormat, bytes: &[u8]) -> Option<(u32, u32)> {
    match format.decoder_format() {
        Some(decoder) => image::ImageReader::with_format(Cursor::new(bytes), decoder)
            .into_dimensions()
            .ok(),
        None => {
            let header = bytes.get(6..10)?;
            Some((
                u32::from(u16::from_le_bytes([header[0], header[1]])),
                u32::from(u16::from_le_bytes([header[2], header[3]])),
            ))
        }
    }
}

pub fn dimensions_of(bytes: &[u8]) -> Option<(u32, u32)> {
    image_dimensions(sniff_inline_format(bytes)?, bytes)
}
