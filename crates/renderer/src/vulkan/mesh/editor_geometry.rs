//! Lazily uploaded topology for static and skinned editor meshes.

use std::sync::{
	Arc, Mutex, OnceLock,
	atomic::{AtomicUsize, Ordering},
};

use ash::Instance;

use crate::vulkan::{VulkanBuffer, VulkanDevice};

const MAX_TOPOLOGY_BYTES: usize = 128 * 1024 * 1024;
const MAX_PICKING_BYTES: usize = 96 * 1024 * 1024;
const MAX_MESH_INDICES: usize = 4 * 1024 * 1024;

#[derive(Default)]
pub(crate) struct EditorTopologyBudget {
	topology_bytes: Arc<AtomicUsize>,
	picking_bytes: Arc<AtomicUsize>,
}

impl EditorTopologyBudget {
	fn reserve(&self, bytes: usize) -> Option<TopologyReservation> {
		TopologyReservation::reserve(&self.topology_bytes, bytes, MAX_TOPOLOGY_BYTES)
	}

	pub(super) fn reserve_picking(&self, bytes: usize) -> Option<TopologyReservation> {
		TopologyReservation::reserve(&self.picking_bytes, bytes, MAX_PICKING_BYTES)
	}
}

#[derive(Debug)]
pub(super) struct TopologyReservation {
	budget: Arc<AtomicUsize>,
	bytes: usize,
}

impl TopologyReservation {
	fn reserve(budget: &Arc<AtomicUsize>, bytes: usize, limit: usize) -> Option<Self> {
		budget
			.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
				used.checked_add(bytes).filter(|total| *total <= limit)
			})
			.ok()?;
		Some(Self {
			budget: Arc::clone(budget),
			bytes,
		})
	}
}

impl Drop for TopologyReservation {
	fn drop(&mut self) {
		self.budget.fetch_sub(self.bytes, Ordering::Relaxed);
	}
}

pub(super) struct MeshTopology {
	indices: Mutex<Option<Vec<u32>>>,
	edges: OnceLock<Option<(VulkanBuffer, u32)>>,
	_reservation: TopologyReservation,
}

impl MeshTopology {
	pub(super) fn retain(budget: &EditorTopologyBudget, indices: &[u32]) -> Option<Self> {
		if indices.len() < 3 {
			return None;
		}
		// Bound persistent source indices plus up to three distinct edges per triangle.
		let reservation = (indices.len() <= MAX_MESH_INDICES)
			.then(|| budget.reserve(size_of_val(indices) * 3))
			.flatten();
		let mut copy = Vec::new();
		let Some(reservation) =
			reservation.filter(|_| copy.try_reserve_exact(indices.len()).is_ok())
		else {
			engine_logger::warn!(target: "editor_viewport", "Mesh wireframe topology exceeds the editor resource budget; using a filled surface");
			return None;
		};
		copy.extend_from_slice(indices);
		Some(Self {
			indices: Mutex::new(Some(copy)),
			edges: OnceLock::new(),
			_reservation: reservation,
		})
	}

	pub(super) fn prepare(&self, instance: &Instance, device: &VulkanDevice) {
		self.edges.get_or_init(|| {
			let indices = self.indices.lock().ok()?.take()?;
			let Some(edges) = edge_indices(&indices) else {
				engine_logger::warn!(target: "editor_viewport", "Could not allocate wireframe topology; using a filled surface");
				return None;
			};
			if edges.is_empty() {
				return None;
			}
			match VulkanBuffer::index(instance, device, &edges) {
				Ok(buffer) => Some((buffer, (edges.len() * 2) as u32)),
				Err(error) => {
					engine_logger::warn!(target: "editor_viewport", "Could not upload wireframe topology; using a filled surface: {error}");
					None
				}
			}
		});
	}

	pub(super) fn edges(&self) -> Option<(&VulkanBuffer, u32)> {
		self.edges
			.get()?
			.as_ref()
			.map(|(buffer, count)| (buffer, *count))
	}
}

fn edge_indices(indices: &[u32]) -> Option<Vec<[u32; 2]>> {
	let mut edges = Vec::new();
	edges.try_reserve_exact(indices.len()).ok()?;
	for face in indices.chunks_exact(3) {
		for (start, end) in [(face[0], face[1]), (face[1], face[2]), (face[2], face[0])] {
			if start != end {
				edges.push([start.min(end), start.max(end)]);
			}
		}
	}
	edges.sort_unstable();
	edges.dedup();
	Some(edges)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn wireframe_edges_are_unique_in_either_direction_and_ignore_degenerate_pairs() {
		let edges = edge_indices(&[0, 1, 2, 2, 1, 3, 5, 5, 5]).unwrap();
		assert_eq!(edges, [[0, 1], [0, 2], [1, 2], [1, 3], [2, 3]]);
	}
}
