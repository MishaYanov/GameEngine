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
}

impl Display for ImageAssetError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => {
                write!(formatter, "failed reading image asset: {error}",)
            }

            Self::Image(error) => {
                write!(formatter, "failed decoding image asset: {error}",)
            }
        }
    }
}

impl Error for ImageAssetError {}

impl From<io::Error> for ImageAssetError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ImageError> for ImageAssetError {
    fn from(value: ImageError) -> Self {
        Self::Image(value)
    }
}

impl ImageAsset {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ImageAssetError> {
        let image = ImageReader::open(path)?.with_guessed_format()?.decode()?;

        /*
         * Convert whatever the source format was
         * into exactly what VulkanTexture expects.
         */
        let mut rgba = image.to_rgba8();

        /*
         * Our cube UV convention treats V=0 as
         * the bottom of a face.
         *
         * Typical image files begin with their
         * top row, so flip once during asset
         * loading rather than inside Vulkan.
         */
        image::imageops::flip_vertical_in_place(&mut rgba);

        let width = rgba.width();

        let height = rgba.height();

        let pixels = rgba.into_raw();

        Ok(Self {
            width,
            height,
            pixels,
        })
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
