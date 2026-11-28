//! Bounded CPU scheduling for additive world regions.

use crate::{
	SceneData,
	project::{ProjectData, WorldRegion},
	scene_catalog::SceneId,
	scene_prefetch::{PrefetchKey, ScenePrefetch},
};
use std::{
	collections::{HashMap, HashSet},
	path::{Path, PathBuf},
	sync::Arc,
	time::{Duration, Instant},
};

const MAX_RESIDENT: usize = 8;
const DETACH_RETRY_DELAY: Duration = Duration::from_secs(1);

pub enum RegionOperation {
	Attach {
		region: SceneId,
		document: Arc<SceneData>,
	},
	Detach {
		region: SceneId,
	},
}

pub struct WorldStreaming {
	worker: ScenePrefetch,
	root: PathBuf,
	scope: Option<(String, u64)>,
	pending: Option<PrefetchKey>,
	failed: HashSet<SceneId>,
	deferred_detaches: HashMap<SceneId, Instant>,
	arrival: Option<[f32; 3]>,
}

impl WorldStreaming {
	pub fn new(root: &Path) -> Result<Self, String> {
		Ok(Self {
			worker: ScenePrefetch::new().map_err(|error| error.to_string())?,
			root: root.canonicalize().map_err(|error| error.to_string())?,
			scope: None,
			pending: None,
			failed: HashSet::new(),
			deferred_detaches: HashMap::new(),
			arrival: None,
		})
	}

	pub fn suspend(&mut self) {
		self.worker.cancel();
		self.scope = None;
		self.pending = None;
		self.retry_failed();
		self.arrival = None;
	}

	pub fn enter_world(&mut self, run: &str, generation: u64) {
		if self
			.scope
			.as_ref()
			.is_some_and(|(id, serial)| id == run && *serial == generation)
		{
			return;
		}
		self.worker.cancel();
		self.scope = Some((run.to_owned(), generation));
		self.pending = None;
		self.retry_failed();
		self.arrival = None;
	}

	/// Request the arrival region before moving the controlled actor.
	pub fn prepare_arrival(&mut self, destination: [f32; 3]) -> Result<(), String> {
		if !destination.into_iter().all(f32::is_finite) {
			return Err("World arrival must be finite".into());
		}
		self.arrival = Some(destination);
		Ok(())
	}

	pub fn cancel_arrival(&mut self) {
		self.arrival = None;
	}

	pub fn arrival_ready(
		&self,
		project: &ProjectData,
		world: &SceneId,
		resident: &[SceneId],
	) -> bool {
		let Some(destination) = self.arrival else {
			return false;
		};
		let mut covering = project
			.regions_for(world)
			.filter(|region| region.bounds.distance_to(destination) == 0.);
		covering.next().is_some_and(|first| {
			resident.contains(&first.scene)
				&& covering.all(|region| resident.contains(&region.scene))
		})
	}

	pub fn has_scope(&self, run: &str, generation: u64) -> bool {
		self.scope
			.as_ref()
			.is_some_and(|(active, epoch)| active == run && *epoch == generation)
	}

	pub fn waiting(&self) -> bool {
		self.pending.is_some() || !self.deferred_detaches.is_empty()
	}

	pub fn retry_failed(&mut self) {
		self.failed.clear();
		self.deferred_detaches.clear();
	}

	pub fn reject(&mut self, region: SceneId) {
		self.failed.insert(region);
	}

	/// Give other region work a chance after failing to retire a resident region.
	pub fn defer_detach(&mut self, region: SceneId) {
		self.deferred_detaches
			.insert(region, Instant::now() + DETACH_RETRY_DELAY);
	}

	/// Consult the live world's retirement rules for out-of-range regions before
	/// selecting a detach. A pinned region still counts toward the resident limit.
	pub fn poll(
		&mut self,
		project: &ProjectData,
		world: &SceneId,
		focus: [f32; 3],
		resident: &[SceneId],
		mut can_detach: impl FnMut(&SceneId) -> bool,
	) -> Result<Option<RegionOperation>, String> {
		let Some((_, generation)) = &self.scope else {
			return Ok(None);
		};
		let generation = *generation;
		let arrival = self.arrival;
		if !self.deferred_detaches.is_empty() {
			let now = Instant::now();
			self.deferred_detaches
				.retain(|region, retry_at| *retry_at > now && resident.contains(region));
		}
		self.failed.retain(|id| {
			project.regions_for(world).any(|region| {
				&region.scene == id
					&& Self::distance(region, focus, arrival) <= region.release_distance
			})
		});
		let next = next_region(project, world, focus, self.arrival, resident, &self.failed);
		if let Some(pending) = &self.pending {
			let keep = project.regions_for(world).any(|region| {
				region.scene == pending.scene
					&& Self::distance(region, focus, arrival) <= region.release_distance
			});
			let urgent = next.is_some_and(|region| {
				region.scene != pending.scene
					&& Self::distance(region, focus, arrival) <= region.preload_distance * 0.25
			});
			if !keep || urgent {
				self.worker.cancel();
				self.pending = None;
			} else if let Some(result) = self.worker.take_confirmed(pending) {
				let region = self.pending.take().expect("matched pending region").scene;
				return match result {
					Ok(ready) => Ok(Some(RegionOperation::Attach {
						region,
						document: Arc::new(ready.document.data),
					})),
					Err(error) => {
						self.failed.insert(region.clone());
						Err(format!(
							"Cannot prepare world region {}: {error}",
							region.as_str()
						))
					}
				};
			}
		}
		if let Some(region) = project
			.regions_for(world)
			.filter(|region| {
				let distance = Self::distance(region, focus, arrival);
				resident.contains(&region.scene)
					&& distance > region.release_distance
					&& !self.deferred_detaches.contains_key(&region.scene)
					&& can_detach(&region.scene)
			})
			.max_by(|left, right| {
				Self::distance(left, focus, arrival)
					.total_cmp(&Self::distance(right, focus, arrival))
			}) {
			return Ok(Some(RegionOperation::Detach {
				region: region.scene.clone(),
			}));
		}
		if self.pending.is_none()
			&& resident.len() < MAX_RESIDENT
			&& let Some(region) = next
		{
			let key = PrefetchKey {
				asset_root: self.root.clone(),
				scene: region.scene.clone(),
				source_generation: generation,
			};
			if self.worker.confirm(
				key.clone(),
				PathBuf::from(project.scene_path(&region.scene)?),
			) {
				self.pending = Some(key);
			}
		}
		Ok(None)
	}

	/// Hold movement near an unavailable seam by one testable margin. The world
	/// author can use a gate at routes whose fastest movement needs a larger hold.
	pub fn entry_blocked(
		&self,
		project: &ProjectData,
		world: &SceneId,
		focus: [f32; 3],
		resident: &[SceneId],
	) -> bool {
		project.regions_for(world).any(|region| {
			!resident.contains(&region.scene)
				&& region.bounds.distance_to(focus) <= (region.preload_distance * 0.25).min(1.0)
		})
	}

	fn distance(region: &WorldRegion, focus: [f32; 3], arrival: Option<[f32; 3]>) -> f32 {
		let distance = region.bounds.distance_to(focus);
		arrival.map_or(distance, |arrival| {
			distance.min(region.bounds.distance_to(arrival))
		})
	}
}

fn next_region<'a>(
	project: &'a ProjectData,
	world: &SceneId,
	focus: [f32; 3],
	arrival: Option<[f32; 3]>,
	resident: &[SceneId],
	failed: &HashSet<SceneId>,
) -> Option<&'a WorldRegion> {
	project
		.regions_for(world)
		.filter(|region| !resident.contains(&region.scene) && !failed.contains(&region.scene))
		.filter_map(|region| {
			let near = region.bounds.distance_to(focus);
			let landing = arrival.map_or(f32::INFINITY, |position| {
				region.bounds.distance_to(position)
			});
			let distance = near.min(landing);
			(distance <= region.preload_distance).then_some((landing == 0., distance, region))
		})
		.min_by(|(landing_a, distance_a, a), (landing_b, distance_b, b)| {
			landing_b
				.cmp(landing_a)
				.then(distance_a.total_cmp(distance_b))
				.then_with(|| a.scene.as_str().cmp(b.scene.as_str()))
		})
		.map(|(_, _, region)| region)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::project::RegionBounds;

	#[test]
	fn chooses_nearest_region_and_explicit_arrival_takes_priority() {
		let document = SceneData::from_json(include_str!(
			"../../../apps/game/assets/scenes/camera-room.json"
		))
			.unwrap();
		let world = SceneId::new("world").unwrap();
		let mut project =
			ProjectData::for_scene_with_id("World", world.clone(), "world.json", &document)
				.unwrap();
		for (id, x) in [("near", 10.), ("far", 50.)] {
			project.world_regions.push(WorldRegion {
				world: world.clone(),
				scene: SceneId::new(id).unwrap(),
				bounds: RegionBounds {
					min: [x, -2., -2.],
					max: [x + 10., 2., 2.],
				},
				preload_distance: 12.,
				release_distance: 18.,
			});
		}
		let failed = HashSet::new();
		assert_eq!(
			next_region(&project, &world, [1., 0., 0.], None, &[], &failed)
				.unwrap()
				.scene
				.as_str(),
			"near"
		);
		assert_eq!(
			next_region(
				&project,
				&world,
				[1., 0., 0.],
				Some([55., 0., 0.]),
				&[],
				&failed
			)
				.unwrap()
				.scene
				.as_str(),
			"far"
		);
	}
}

#[cfg(test)]
#[path = "../tests/unit/world_streaming_stress.rs"]
mod stress_tests;
