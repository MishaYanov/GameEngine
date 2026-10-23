use super::*;

#[test]
fn extent_uses_surface_size_or_clamps_requested_dimensions() {
    let mut capabilities = vk::SurfaceCapabilitiesKHR {
        current_extent: vk::Extent2D {
            width: 640,
            height: 480,
        },
        min_image_extent: vk::Extent2D {
            width: 100,
            height: 100,
        },
        max_image_extent: vk::Extent2D {
            width: 1000,
            height: 800,
        },
        ..Default::default()
    };
    assert_eq!(
        choose_extent(&capabilities, 1, 9000),
        capabilities.current_extent
    );
    capabilities.current_extent = vk::Extent2D {
        width: u32::MAX,
        height: u32::MAX,
    };
    assert_eq!(
        choose_extent(&capabilities, 1, 9000),
        vk::Extent2D {
            width: 100,
            height: 800
        }
    );
}

#[test]
fn image_count_respects_bounded_and_unbounded_surface_limits() {
    let mut capabilities = vk::SurfaceCapabilitiesKHR {
        min_image_count: 2,
        max_image_count: 2,
        ..Default::default()
    };
    assert_eq!(choose_image_count(&capabilities), 2);
    capabilities.max_image_count = 0;
    assert_eq!(choose_image_count(&capabilities), 3);
}

#[test]
fn presentation_preferences_keep_supported_fallbacks() {
    assert_eq!(
        choose_present_mode(&[vk::PresentModeKHR::FIFO]),
        vk::PresentModeKHR::FIFO
    );
    assert_eq!(
        choose_present_mode(&[vk::PresentModeKHR::FIFO, vk::PresentModeKHR::MAILBOX]),
        vk::PresentModeKHR::FIFO
    );
    assert_eq!(
        choose_composite_alpha(vk::CompositeAlphaFlagsKHR::empty()),
        None
    );
    assert_eq!(
        choose_composite_alpha(vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED),
        Some(vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED)
    );
}
