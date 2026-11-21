use super::*;

#[test]
fn upload_chunks_bound_staging_and_resource_counts_and_isolate_large_assets() {
	let limit = renderer::MAX_UPLOAD_BATCH_BYTES;
	assert_eq!(
		next_upload_batch_len([limit / 2, limit / 2, 4].into_iter()),
		2
	);
	assert_eq!(next_upload_batch_len([4, limit + 1].into_iter()), 1);
	assert_eq!(next_upload_batch_len([limit + 1, 4].into_iter()), 1);
	assert_eq!(
		next_upload_batch_len(std::iter::repeat(4)),
		renderer::MAX_UPLOAD_BATCH_RESOURCES
	);
	assert_eq!(next_upload_batch_len([].into_iter()), 0);
}
