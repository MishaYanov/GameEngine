use super::enabled;
#[test]
fn selection_is_explicit_and_independent_of_logging_filter() {
	assert_eq!(enabled(true, None, None), Ok(true));
	assert_eq!(enabled(false, Some("debug"), None), Ok(true));
	assert_eq!(enabled(false, None, None), Ok(false));
	assert_eq!(enabled(true, Some("debug"), Some("0")), Ok(false));
	assert_eq!(enabled(false, None, Some("1")), Ok(true));
	assert!(enabled(true, None, Some("typo")).is_err());
}
