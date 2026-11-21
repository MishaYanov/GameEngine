use super::*;

#[test]
fn conservative_budget_skips_sweeps_until_pressure_and_rejects_oversized_requests() {
	let mut assets = GpuBudget::default();
	assets.allocations.push(GpuAllocation {
		resource: GpuReference::Mesh(Weak::new()),
		bytes: 4,
	});
	assets.bytes = 4;
	assets.admit(4, 1).unwrap();
	assert_eq!(
		assets.allocations.len(),
		1,
		"ordinary upload must not sweep allocations"
	);
	assets.admit(GpuBudget::MAX_BYTES, 1).unwrap();
	assert!(assets.allocations.is_empty());
	assert_eq!(assets.bytes, 0);
	assert!(assets.admit(usize::MAX, 1).is_err());
	assert!(assets.admit(0, GpuBudget::MAX_RESOURCES + 1).is_err());
}
