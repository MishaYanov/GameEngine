use super::*;

#[test]
fn rgba_input_rejects_zero_dimensions_and_incomplete_pixels() {
    for (width, height) in [(0, 1), (1, 0)] {
        assert!(matches!(
            ImageAsset::from_rgba8(width, height, vec![]),
            Err(ImageAssetError::InvalidDimensions { .. })
        ));
    }
    assert!(matches!(
        ImageAsset::from_rgba8(2, 1, vec![0; 7]),
        Err(ImageAssetError::InvalidPixelCount {
            expected: 8,
            actual: 7
        })
    ));
    assert!(matches!(
        ImageAsset::from_rgba8(u32::MAX, u32::MAX, vec![]),
        Err(ImageAssetError::SizeOverflow)
    ));
}

#[test]
fn rgba_input_preserves_row_order_and_alpha() {
    let pixels = vec![255, 0, 0, 0, 0, 255, 0, 128];
    let image = ImageAsset::from_rgba8(1, 2, pixels.clone()).unwrap();
    assert_eq!((image.width(), image.height()), (1, 2));
    assert_eq!(image.pixels(), pixels);
}
