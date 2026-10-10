use crate::assets::ImageAsset;

pub struct MaterialData {
	pub(crate) base_color_image: Option<ImageAsset>,
	pub(crate) base_color_factor: [f32; 4],
	pub(crate) double_sided: bool,
}