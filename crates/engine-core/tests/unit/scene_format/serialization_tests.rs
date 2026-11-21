use super::*;
use std::io::Write;

#[test]
fn counting_and_buffered_writers_reserve_the_same_newline_byte() {
	let mut count = SceneJsonWriter::new(std::io::sink());
	let mut buffered = SceneJsonWriter::new(Vec::new());
	count.bytes = MAX_SCENE_BYTES - 2;
	buffered.bytes = MAX_SCENE_BYTES - 2;
	for writer in [&mut count as &mut dyn Write, &mut buffered] {
		writer.write_all(b"a").unwrap();
		assert_eq!(
			writer.write_all(b"b").unwrap_err().to_string(),
			"Scene JSON exceeds the 32 MiB size limit"
		);
	}
	assert_eq!(count.bytes, MAX_SCENE_BYTES - 1);
	assert_eq!(buffered.bytes, count.bytes);
	assert_eq!(buffered.output, b"a");
}
