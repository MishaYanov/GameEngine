use super::*;
#[test]
fn validation_severities_are_mapped_and_collected() {
    use engine_logger::Level;
    assert_eq!(
        validation_log_level(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR),
        Level::Error
    );
    assert_eq!(
        validation_log_level(vk::DebugUtilsMessageSeverityFlagsEXT::WARNING),
        Level::Warning
    );
    assert_eq!(
        validation_log_level(vk::DebugUtilsMessageSeverityFlagsEXT::INFO),
        Level::Info
    );
    assert_eq!(
        validation_log_level(vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE),
        Level::Trace
    );
    let mut config = engine_logger::Config::new("vulkan-validation-test");
    config.console = false;
    config.error_backtraces = false;
    let _logging = engine_logger::init(config).unwrap();
    let data = vk::DebugUtilsMessengerCallbackDataEXT {
        p_message: c"fixture validation failure".as_ptr(),
        ..Default::default()
    };
    let result = unsafe {
        vulkan_debug_callback(
            vk::DebugUtilsMessageSeverityFlagsEXT::ERROR,
            vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION,
            &data,
            std::ptr::null_mut(),
        )
    };
    assert_eq!(result, vk::FALSE);
    let record = engine_logger::snapshot(Default::default())
        .entries
        .remove(0);
    assert_eq!(record.level, Level::Error);
    assert_eq!(record.component, "renderer::vulkan::validation");
    assert!(record.message.contains("fixture validation failure"));
    assert!(record.message.contains("VALIDATION"));
    assert_eq!(
        unsafe {
            vulkan_debug_callback(
                vk::DebugUtilsMessageSeverityFlagsEXT::ERROR,
                vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION,
                std::ptr::null(),
                std::ptr::null_mut(),
            )
        },
        vk::FALSE
    );
}
