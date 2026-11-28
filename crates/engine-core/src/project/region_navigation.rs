//! Coarse navigation over the shared faces of authored world regions.
use super::{ProjectData, RegionBounds};
use crate::scene_catalog::SceneId;
use std::collections::{HashMap, HashSet, VecDeque};

/// Region routing for horizontal movement. Local pathing and collision stay
/// with the actor controller, which can steer toward the reported portal.
pub struct RegionNavigator {
	cells: Vec<NavigationCell>,
	by_id: HashMap<SceneId, usize>,
}

struct NavigationCell {
	id: SceneId,
	bounds: RegionBounds,
	passages: Vec<Passage>,
}

struct Passage {
	to: usize,
	portal: [f32; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct RegionPath {
	/// Includes the current and destination regions.
	pub regions: Vec<SceneId>,
	/// Optional prefetch hint independent of which step can be taken now.
	pub first_unloaded: Option<SceneId>,
	pub step: RegionNavigationStep,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RegionNavigationStep {
	Arrived,
	WaitForRegion {
		region: SceneId,
	},
	/// The next region is resident. Use local pathing to reach its shared face.
	Cross {
		region: SceneId,
		portal: [f32; 3],
	},
}

impl RegionNavigator {
	/// Half-open bounds give the precise seam to the region that starts there.
	pub fn region_at(&self, position: [f32; 3]) -> Option<&SceneId> {
		self.cells
			.iter()
			.find(|cell| {
				(0..3).all(|axis| {
					cell.bounds.min[axis] <= position[axis]
						&& position[axis] < cell.bounds.max[axis]
				})
			})
			.map(|cell| &cell.id)
	}

	/// Search the authored map and check residency only for the next step.
	/// A farther unloaded region can be prepared while the actor approaches.
	pub fn plan(
		&self,
		from: &SceneId,
		destination: &SceneId,
		resident: &[SceneId],
	) -> Result<Option<RegionPath>, String> {
		let start = self.region_index(from)?;
		let goal = self.region_index(destination)?;
		if resident.len() > self.cells.len() {
			return Err("Resident region count exceeds this navigation world".into());
		}

		let mut predecessors = vec![None; self.cells.len()];
		predecessors[start] = Some(start);
		let mut queue = VecDeque::from([start]);
		while let Some(current) = queue.pop_front() {
			if current == goal {
				break;
			}
			for passage in &self.cells[current].passages {
				if predecessors[passage.to].is_none() {
					predecessors[passage.to] = Some(current);
					queue.push_back(passage.to);
				}
			}
		}
		if predecessors[goal].is_none() {
			return Ok(None);
		}

		let mut route = vec![goal];
		while let Some(&current) = route.last().filter(|index| **index != start) {
			let previous = predecessors[current].ok_or("Navigation predecessor is missing")?;
			route.push(previous);
		}
		route.reverse();
		let regions: Vec<_> = route
			.iter()
			.map(|&index| self.cells[index].id.clone())
			.collect();
		let resident: HashSet<_> = resident.iter().collect();
		let first_unloaded = regions
			.iter()
			.find(|region| !resident.contains(region))
			.cloned();
		let step = if !resident.contains(from) {
			RegionNavigationStep::WaitForRegion {
				region: from.clone(),
			}
		} else if let Some(&next) = route.get(1) {
			let region = self.cells[next].id.clone();
			if resident.contains(&region) {
				let edge = self.cells[start]
					.passages
					.iter()
					.find(|edge| edge.to == next)
					.ok_or("Navigation portal is missing")?;
				RegionNavigationStep::Cross {
					region,
					portal: edge.portal,
				}
			} else {
				RegionNavigationStep::WaitForRegion { region }
			}
		} else {
			RegionNavigationStep::Arrived
		};

		Ok(Some(RegionPath {
			regions,
			first_unloaded,
			step,
		}))
	}

	fn region_index(&self, id: &SceneId) -> Result<usize, String> {
		self.by_id
			.get(id)
			.copied()
			.ok_or_else(|| format!("Region {:?} is not in this navigation world", id.as_str()))
	}
}

impl ProjectData {
	/// Build once when a game module prepares a world. Regions are visited in
	/// stable ID order, so equal-length paths are deterministic.
	pub fn region_navigator(&self, world: &SceneId) -> Result<RegionNavigator, String> {
		self.scene_path(world)?;
		self.validate_world_regions()?;
		let mut cells: Vec<_> = self
			.regions_for(world)
			.map(|region| NavigationCell {
				id: region.scene.clone(),
				bounds: region.bounds,
				passages: Vec::new(),
			})
			.collect();
		cells.sort_unstable_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
		for i in 0..cells.len() {
			let (left, right) = cells.split_at_mut(i + 1);
			let a = &mut left[i];
			for (offset, b) in right.iter_mut().enumerate() {
				if interiors_overlap(a.bounds, b.bounds) {
					return Err(format!(
						"Navigation region interiors overlap: {:?} and {:?}",
						a.id.as_str(),
						b.id.as_str()
					));
				}
				if let Some(portal) = shared_horizontal_face(a.bounds, b.bounds) {
					a.passages.push(Passage {
						to: i + 1 + offset,
						portal,
					});
					b.passages.push(Passage { to: i, portal });
				}
			}
		}
		let by_id = cells
			.iter()
			.enumerate()
			.map(|(index, cell)| (cell.id.clone(), index))
			.collect();
		Ok(RegionNavigator { cells, by_id })
	}
}

fn interiors_overlap(a: RegionBounds, b: RegionBounds) -> bool {
	(0..3).all(|axis| a.min[axis].max(b.min[axis]) < a.max[axis].min(b.max[axis]))
}

fn shared_horizontal_face(a: RegionBounds, b: RegionBounds) -> Option<[f32; 3]> {
	'face: for axis in [0, 2] {
		let coordinate = if a.max[axis] == b.min[axis] {
			a.max[axis]
		} else if b.max[axis] == a.min[axis] {
			b.max[axis]
		} else {
			continue;
		};
		let mut portal = [0.0; 3];
		portal[axis] = coordinate;
		for (perpendicular, value) in portal.iter_mut().enumerate() {
			if perpendicular == axis {
				continue;
			}
			let low = a.min[perpendicular].max(b.min[perpendicular]);
			let high = a.max[perpendicular].min(b.max[perpendicular]);
			if low >= high {
				continue 'face;
			}
			*value = f64::midpoint(f64::from(low), f64::from(high)) as f32;
		}
		return Some(portal);
	}
	None
}
