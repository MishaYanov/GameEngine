use std::{
    error::Error,
    fmt::{Display, Formatter},
    io,
    path::Path,
};

use image::{ImageError, ImageReader};

pub struct ImageAsset {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

#[derive(Debug)]
pub enum ImageAssetError {
    Io(io::Error),
    Image(ImageError),
    InvalidDimensions {
        width: u32,
        height: u32,
    },
    SizeOverflow,
    InvalidPixelCount {
        expected: usize,
        actual: usize,
    },
}

impl Display for ImageAssetError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => {
                write!(formatter, "failed reading image asset: {error}")
            }

            Self::Image(error) => {
                write!(formatter, "failed decoding image asset: {error}")
            }

            Self::InvalidDimensions { width, height } => {
                write!(
                    formatter,
                    "invalid image dimensions: {width}x{height}"
                )
            }

            Self::SizeOverflow => {
                write!(
                    formatter,
                    "image byte count exceeds the supported size"
                )
            }

            Self::InvalidPixelCount { expected, actual } => {
                write!(
                    formatter,
                    "expected {expected} RGBA bytes, received {actual}"
                )
            }
        }
    }
}

impl Error for ImageAssetError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Image(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for ImageAssetError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<ImageError> for ImageAssetError {
    fn from(error: ImageError) -> Self {
        Self::Image(error)
    }
}

impl ImageAsset {
    /// Constructs an image from decoded RGBA8 pixels.
    ///
    /// Each pixel must contain four bytes: red, green, blue, alpha.
    /// Preserves the supplied row order.
    pub fn from_rgba8(
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    ) -> Result<Self, ImageAssetError> {
        if width == 0 || height == 0 {
            return Err(ImageAssetError::InvalidDimensions {
                width,
                height,
            });
        }

        let width_usize = usize::try_from(width)
            .map_err(|_| ImageAssetError::SizeOverflow)?;

        let height_usize = usize::try_from(height)
            .map_err(|_| ImageAssetError::SizeOverflow)?;

        let expected = width_usize
            .checked_mul(height_usize)
            .and_then(|pixel_count| pixel_count.checked_mul(4))
            .ok_or(ImageAssetError::SizeOverflow)?;

        if pixels.len() != expected {
            return Err(ImageAssetError::InvalidPixelCount {
                expected,
                actual: pixels.len(),
            });
        }

        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    /// Loads an image file using the existing procedural-mesh UV convention.
    ///
    /// Converts to RGBA8 and flips vertically before constructing the asset.
    pub fn load(
        path: impl AsRef<Path>,
    ) -> Result<Self, ImageAssetError> {
        let image = ImageReader::open(path)?
            .with_guessed_format()?
            .decode()?;

        let mut rgba = image.to_rgba8();

        // Preserve the convention used by the existing cube textures.
        image::imageops::flip_vertical_in_place(&mut rgba);

        let width = rgba.width();
        let height = rgba.height();

        Self::from_rgba8(width, height, rgba.into_raw())
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}