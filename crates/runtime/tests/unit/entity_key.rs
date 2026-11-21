use super::*;

#[test]
fn constructor_rejects_empty_and_unicode_whitespace_without_trimming() {
	for value in [
		"",
		" ",
		" key",
		"key ",
		"two words",
		"tab\tkey",
		"line\nkey",
		"unicode\u{2003}space",
	] {
		assert!(EntityKey::new(value).is_err(), "accepted {value:?}");
	}
	for value in ["robot-left", "robot_0", "רובוט", "🤖"] {
		assert_eq!(EntityKey::new(value).unwrap().as_str(), value);
	}
}

#[test]
fn persistent_key_json_is_a_string_and_preserves_its_value() {
	let key = EntityKey::new("robot-left").unwrap();
	let json = serde_json::to_string(&key).unwrap();
	assert_eq!(json, "\"robot-left\"");
	assert_eq!(serde_json::from_str::<EntityKey>(&json).unwrap(), key);
}

#[test]
fn generated_keys_are_valid_distinct_uuid_v4_strings() {
	let mut keys = std::collections::HashSet::new();
	for _ in 0..32 {
		let key = EntityKey::generate();
		assert!(EntityKey::new(key.as_str()).is_ok());
		let uuid = uuid::Uuid::parse_str(key.as_str()).unwrap();
		assert_eq!(uuid.get_version_num(), 4);
		assert!(keys.insert(key));
	}
}
