use crate::assets::ImageAsset;

pub struct MaterialData {
	pub base_color_image: Option<ImageAsset>,
	pub base_color_factor: [f32; 4],
	pub double_sided: bool,
}
