use engine_core::{
	Bounds3,
	asset_registry::{AssetMetadata, AssetRef, AssetRegistry, AssetRegistryLimits},
};

#[test]
fn empty_and_single_entry_batches_preserve_handles_limits_and_accounting() {
	let mut registry = AssetRegistry::new(AssetRegistryLimits {
		max_assets: 1,
		max_metadata_bytes: 1024,
		max_estimated_bytes: 10,
	})
	.unwrap();
	let reference = AssetRef::new("a.glb", None).unwrap();
	let handle = registry
		.register_batch(vec![(
			reference.clone(),
			AssetMetadata {
				estimated_bytes: 10,
				..Default::default()
			},
		)])
		.unwrap()[0];
	let before = registry.get(handle).unwrap().clone();
	let metadata_bytes = registry.metadata_bytes();
	assert!(registry.register_batch(Vec::new()).unwrap().is_empty());
	for entry in [
		(
			reference.clone(),
			AssetMetadata {
				estimated_bytes: 11,
				..Default::default()
			},
		),
		(
			AssetRef::new("b.glb", None).unwrap(),
			AssetMetadata::default(),
		),
		(
			AssetRef {
				path: "../a.glb".into(),
				mesh_index: None,
			},
			AssetMetadata::default(),
		),
	] {
		assert!(registry.register_batch(vec![entry]).is_err());
		assert_eq!(registry.get(handle), Some(&before));
		assert_eq!(registry.len(), 1);
		assert_eq!(registry.metadata_bytes(), metadata_bytes);
		assert_eq!(registry.estimated_bytes(), 10);
	}
	assert_eq!(
		registry
			.register_batch(vec![(reference.clone(), AssetMetadata::default())])
			.unwrap(),
		vec![handle]
	);
	assert_eq!(registry.estimated_bytes(), 0);
	assert_eq!(registry.metadata_bytes(), metadata_bytes);
	registry.remove(handle).unwrap();
	let replacement = registry
		.register_batch(vec![(reference, AssetMetadata::default())])
		.unwrap()[0];
	assert_eq!(replacement.index(), handle.index());
	assert_ne!(replacement.generation(), handle.generation());
	assert!(registry.get(handle).is_none());
}

#[test]
fn single_entry_batch_preserves_metadata_and_overflow_rejections() {
	let mut small = AssetRegistry::new(AssetRegistryLimits {
		max_metadata_bytes: 1,
		..Default::default()
	})
	.unwrap();
	let reference = AssetRef::new("a.glb", None).unwrap();
	assert_eq!(
		small
			.register_batch(vec![(reference.clone(), AssetMetadata::default())])
			.unwrap_err(),
		"Asset registration batch exceeds the metadata budget"
	);
	assert!(small.is_empty());
	let mut registry = AssetRegistry::default();
	let handle = registry
		.register(
			reference.clone(),
			AssetMetadata {
				estimated_bytes: 1,
				..Default::default()
			},
		)
		.unwrap();
	assert_eq!(
		registry
			.register_batch(vec![(
				AssetRef::new("b.glb", None).unwrap(),
				AssetMetadata {
					estimated_bytes: u64::MAX,
					..Default::default()
				}
			)])
			.unwrap_err(),
		"Asset byte estimate overflow"
	);
	assert_eq!(registry.len(), 1);
	assert_eq!(registry.find(&reference), Some(handle));
	assert_eq!(registry.estimated_bytes(), 1);
}

#[test]
fn batch_registration_rolls_back_existing_updates_and_new_handles_on_every_rejection() {
	let mut registry = AssetRegistry::new(AssetRegistryLimits {
		max_assets: 2,
		max_metadata_bytes: 4096,
		max_estimated_bytes: 10,
	})
	.unwrap();
	let reference = AssetRef::new("a.glb", None).unwrap();
	let handle = registry
		.register(
			reference.clone(),
			AssetMetadata {
				estimated_bytes: 3,
				..Default::default()
			},
		)
		.unwrap();
	let before = registry.get(handle).unwrap().clone();
	let bytes = registry.metadata_bytes();
	let updated = AssetMetadata {
		estimated_bytes: 1,
		..Default::default()
	};
	let other = AssetRef::new("b.glb", None).unwrap();
	for batch in [
		vec![
			(reference.clone(), updated.clone()),
			(
				other.clone(),
				AssetMetadata {
					estimated_bytes: 11,
					..Default::default()
				},
			),
		],
		vec![
			(reference.clone(), updated.clone()),
			(reference.clone(), updated.clone()),
		],
		vec![
			(reference.clone(), updated.clone()),
			(
				AssetRef {
					path: "../outside.glb".into(),
					mesh_index: None,
				},
				updated.clone(),
			),
		],
	] {
		assert!(registry.register_batch(batch).is_err());
		assert_eq!(registry.get(handle), Some(&before));
		assert_eq!(registry.find(&reference), Some(handle));
		assert!(registry.find(&other).is_none());
		assert_eq!(registry.len(), 1);
		assert_eq!(registry.estimated_bytes(), 3);
		assert_eq!(registry.metadata_bytes(), bytes);
	}
	let handles = registry
		.register_batch(vec![
			(reference.clone(), updated),
			(
				other.clone(),
				AssetMetadata {
					estimated_bytes: 9,
					..Default::default()
				},
			),
		])
		.unwrap();
	assert_eq!(handles[0], handle);
	assert_eq!(registry.find(&other), Some(handles[1]));
	assert_eq!(registry.estimated_bytes(), 10);
}

#[test]
fn registry_deduplicates_logical_references_updates_metadata_and_generations_reject_stale_handles()
{
	let mut registry = AssetRegistry::default();
	let reference = AssetRef::new("models/robot.glb", Some(1)).unwrap();
	let first = registry
		.register(
			reference.clone(),
			AssetMetadata {
				estimated_bytes: 10,
				..Default::default()
			},
		)
		.unwrap();
	let metadata = AssetMetadata {
		estimated_bytes: 20,
		bounds: Some(Bounds3::new([-1.0; 3], [1.0; 3]).unwrap()),
	};
	assert_eq!(
		registry
			.register(reference.clone(), metadata.clone())
			.unwrap(),
		first
	);
	assert_eq!(registry.get(first).unwrap().metadata, metadata);
	assert_eq!(registry.len(), 1);
	assert_eq!(registry.estimated_bytes(), 20);
	assert_eq!(registry.find(&reference), Some(first));
	assert_eq!(registry.remove(first).unwrap().reference, reference);
	assert!(registry.get(first).is_none());
	assert_eq!(registry.estimated_bytes(), 0);
	assert_eq!(registry.metadata_bytes(), 0);
	let next = registry
		.register(reference, AssetMetadata::default())
		.unwrap();
	assert_eq!(next.index(), first.index());
	assert_ne!(next.generation(), first.generation());
	assert!(registry.remove(first).is_none());
	assert!(registry.get(next).is_some());
	registry.clear();
	assert!(registry.is_empty());
	assert!(registry.get(next).is_none());
	assert_eq!(registry.iter().count(), 0);
}

#[test]
fn asset_count_estimate_and_metadata_budgets_reject_before_mutation_and_removal_reclaims_capacity()
{
	let limits = AssetRegistryLimits {
		max_assets: 1,
		max_metadata_bytes: 1024,
		max_estimated_bytes: 10,
	};
	let mut registry = AssetRegistry::new(limits).unwrap();
	let reference = AssetRef::new("a.glb", None).unwrap();
	let handle = registry
		.register(
			reference.clone(),
			AssetMetadata {
				estimated_bytes: 10,
				..Default::default()
			},
		)
		.unwrap();
	let before = registry.get(handle).unwrap().clone();
	let metadata_bytes = registry.metadata_bytes();
	assert!(
		registry
			.register(
				reference,
				AssetMetadata {
					estimated_bytes: 11,
					..Default::default()
				}
			)
			.is_err()
	);
	assert_eq!(registry.get(handle), Some(&before));
	assert!(
		registry
			.register(
				AssetRef::new("b.glb", None).unwrap(),
				AssetMetadata::default()
			)
			.is_err()
	);
	assert_eq!(registry.len(), 1);
	assert_eq!(registry.metadata_bytes(), metadata_bytes);
	assert_eq!(registry.estimated_bytes(), 10);
	registry.remove(handle).unwrap();
	assert!(
		registry
			.register(
				AssetRef::new("b.glb", None).unwrap(),
				AssetMetadata::default()
			)
			.is_ok()
	);
	let mut small = AssetRegistry::new(AssetRegistryLimits {
		max_assets: 10,
		max_metadata_bytes: 1,
		max_estimated_bytes: 10,
	})
	.unwrap();
	assert!(
		small
			.register(
				AssetRef::new("a.glb", None).unwrap(),
				AssetMetadata::default()
			)
			.is_err()
	);
	assert!(small.is_empty());
	assert_eq!(small.metadata_bytes(), 0);
}

#[test]
fn asset_reference_validation_rejects_nonportable_aliases_and_distinguishes_meshes() {
	for path in [
		"../robot.glb",
		"models/./robot.glb",
		"models\\robot.glb",
		"C:/robot.glb",
		"models//robot.glb",
		"",
	] {
		assert!(AssetRef::new(path, None).is_err(), "accepted {path:?}");
	}
	assert!(AssetRef::new("robot.glb", Some(1_000_001)).is_err());
	let mut registry = AssetRegistry::default();
	let a = registry
		.register(
			AssetRef::new("robot.glb", Some(0)).unwrap(),
			AssetMetadata::default(),
		)
		.unwrap();
	let b = registry
		.register(
			AssetRef::new("robot.glb", Some(1)).unwrap(),
			AssetMetadata::default(),
		)
		.unwrap();
	assert_ne!(a, b);
	assert_eq!(registry.len(), 2);
}

#[test]
fn registry_limits_are_bounded_and_default_capacity_requires_no_payload_allocations() {
	for limits in [
		AssetRegistryLimits {
			max_assets: 0,
			..Default::default()
		},
		AssetRegistryLimits {
			max_assets: 100_001,
			..Default::default()
		},
		AssetRegistryLimits {
			max_metadata_bytes: 0,
			..Default::default()
		},
		AssetRegistryLimits {
			max_estimated_bytes: 0,
			..Default::default()
		},
	] {
		assert!(AssetRegistry::new(limits).is_err());
	}
	let registry = AssetRegistry::default();
	assert_eq!(registry.len(), 0);
	assert_eq!(registry.estimated_bytes(), 0);
	assert_eq!(registry.metadata_bytes(), 0);
}

#[test]
fn handles_from_another_registry_cannot_resolve_or_remove_this_registry_assets() {
	let mut first = AssetRegistry::default();
	let mut second = AssetRegistry::default();
	let foreign = first
		.register(
			AssetRef::new("first.glb", None).unwrap(),
			AssetMetadata::default(),
		)
		.unwrap();
	let local = second
		.register(
			AssetRef::new("second.glb", None).unwrap(),
			AssetMetadata::default(),
		)
		.unwrap();
	assert!(
		second.get(foreign).is_none(),
		"asset handles must be scoped to the registry that issued them"
	);
	assert!(second.remove(foreign).is_none());
	assert!(second.get(local).is_some());
	assert_eq!(second.len(), 1);
}
