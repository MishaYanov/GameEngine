use super::*;

#[test]
fn camera_hash_enforces_its_own_byte_limit_before_consuming_a_chunk() {
	let mut hash = StateHash::new(MAX_CAMERA_REPLAY_BYTES);
	hash.write_all(&vec![0; MAX_CAMERA_REPLAY_BYTES]).unwrap();
	let accepted = hash.value;
	assert!(hash.write_all(&[1]).is_err());
	assert_eq!(hash.bytes, MAX_CAMERA_REPLAY_BYTES);
	assert_eq!(hash.value, accepted);
}
