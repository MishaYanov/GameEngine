use std::sync::{Arc, Weak};

use renderer::vulkan::{VulkanMesh, VulkanTexture};

use super::AssetManagerError;

/// Track allocations retained by the caches or external scene and model handles.
pub(super) struct GpuBudget {
	allocations: Vec<GpuAllocation>,
	bytes: usize,
	byte_limit: usize,
	resource_limit: usize,
}

impl GpuBudget {
	pub(super) const MAX_BYTES: usize = 512 * 1024 * 1024;
	pub(super) const MAX_RESOURCES: usize = 8192;

	pub(super) fn set_limits(
		&mut self,
		bytes: usize,
		resources: usize,
	) -> Result<(), AssetManagerError> {
		if bytes > Self::MAX_BYTES || resources > Self::MAX_RESOURCES {
			return Err(AssetManagerError::ResourceBudget(
				"Configured GPU budget exceeds the engine limits".into(),
			));
		}
		self.byte_limit = bytes;
		self.resource_limit = resources;
		Ok(())
	}

	pub(super) fn live_bytes(&self) -> usize {
		self.allocations
			.iter()
			.filter(|allocation| allocation.live())
			.map(|allocation| allocation.bytes)
			.sum()
	}

	pub(super) fn live_resources(&self) -> usize {
		self.allocations
			.iter()
			.filter(|allocation| allocation.live())
			.count()
	}

	pub(super) fn track_mesh(&mut self, mesh: &Arc<VulkanMesh>, bytes: usize) {
		self.track(GpuReference::Mesh(Arc::downgrade(mesh)), bytes);
	}

	pub(super) fn track_texture(&mut self, texture: &Arc<VulkanTexture>, bytes: usize) {
		self.track(GpuReference::Texture(Arc::downgrade(texture)), bytes);
	}

	fn track(&mut self, resource: GpuReference, bytes: usize) {
		self.allocations.push(GpuAllocation { resource, bytes });
		self.bytes += bytes;
	}

	pub(super) fn sweep(&mut self) {
		self.allocations.retain(GpuAllocation::live);
		self.bytes = self
			.allocations
			.iter()
			.map(|allocation| allocation.bytes)
			.sum();
	}

	pub(super) fn admit(&mut self, bytes: usize, entries: usize) -> Result<(), AssetManagerError> {
		// Cached totals are conservative when callers have dropped their last Arc.
		// Most uploads fit without traversing every previously loaded resource.
		if self.fits(bytes, entries) {
			return Ok(());
		}
		self.sweep();
		if !self.fits(bytes, entries) {
			return Err(AssetManagerError::ResourceBudget(format!(
				"request needs {bytes} logical bytes and {entries} resources; {} bytes/{} resources remain live (limits: {} bytes and {}). Release unused scene resources and prune the cache before retrying",
				self.bytes,
				self.allocations.len(),
				self.byte_limit,
				self.resource_limit
			)));
		}
		Ok(())
	}

	fn fits(&self, bytes: usize, entries: usize) -> bool {
		self.bytes
			.checked_add(bytes)
			.is_some_and(|total| total <= self.byte_limit)
			&& self
				.allocations
				.len()
				.checked_add(entries)
				.is_some_and(|count| count <= self.resource_limit)
	}
}

impl Default for GpuBudget {
	fn default() -> Self {
		Self {
			allocations: Vec::new(),
			bytes: 0,
			byte_limit: Self::MAX_BYTES,
			resource_limit: Self::MAX_RESOURCES,
		}
	}
}

enum GpuReference {
	Mesh(Weak<VulkanMesh>),
	Texture(Weak<VulkanTexture>),
}
struct GpuAllocation {
	resource: GpuReference,
	bytes: usize,
}
impl GpuAllocation {
	fn live(&self) -> bool {
		match &self.resource {
			GpuReference::Mesh(resource) => resource.strong_count() > 0,
			GpuReference::Texture(resource) => resource.strong_count() > 0,
		}
	}
}

#[cfg(test)]
#[path = "../../../tests/unit/assets/asset_manager/gpu_budget.rs"]
mod tests;
