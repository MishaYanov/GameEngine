use ash::{Device, vk};

use crate::vulkan::VulkanDevice;

pub(super) struct GpuTimer {
	_metrics: engine_logger::metrics::ResourceGuard,
	device: Device,
	pool: vk::QueryPool,
	valid_bits: u32,
	period_ns: f32,
	pending: bool,
	last_ms: Option<f64>,
}

impl GpuTimer {
	pub(super) fn new(device: &VulkanDevice) -> Result<Option<Self>, vk::Result> {
		let Some((valid_bits, period_ns)) = device.timestamp_properties() else {
			return Ok(None);
		};
		let info = vk::QueryPoolCreateInfo::default()
			.query_type(vk::QueryType::TIMESTAMP)
			.query_count(2);
		let pool = unsafe { device.raw().create_query_pool(&info, None)? };
		Ok(Some(Self {
			_metrics: crate::metrics::object(
				device.raw(),
				"query_pool",
				pool,
				"GPU timestamp queries",
				"count=2",
			),
			device: device.raw().clone(),
			pool,
			valid_bits,
			period_ns,
			pending: false,
			last_ms: None,
		}))
	}

	pub(super) fn collect(&mut self) -> Result<(), vk::Result> {
		if !self.pending {
			return Ok(());
		}
		// [timestamp, availability] per query. Never use WAIT: profiling must
		// not add synchronization to the renderer's existing frame fence.
		let mut samples = [[0_u64; 2]; 2];
		match unsafe {
			self.device.get_query_pool_results(
				self.pool,
				0,
				&mut samples,
				vk::QueryResultFlags::TYPE_64 | vk::QueryResultFlags::WITH_AVAILABILITY,
			)
		} {
			Ok(()) | Err(vk::Result::NOT_READY) => {}
			Err(error) => return Err(error),
		}
		if samples.iter().all(|sample| sample[1] != 0) {
			let mask = u64::MAX >> (64 - self.valid_bits);
			let ticks = samples[1][0].wrapping_sub(samples[0][0]) & mask;
			self.last_ms = Some(ticks as f64 * self.period_ns as f64 / 1_000_000.0);
		}
		self.pending = false;
		Ok(())
	}

	/// Start measuring commands on this device's recording command buffer.
	///
	/// # Safety
	///
	/// The command buffer must be recording outside a render pass. The previous
	/// submission using this query pool must have completed.
	pub(super) unsafe fn record_begin(&self, command_buffer: vk::CommandBuffer) {
		unsafe {
			self.device
				.cmd_reset_query_pool(command_buffer, self.pool, 0, 2);
			self.device.cmd_write_timestamp(
				command_buffer,
				vk::PipelineStageFlags::TOP_OF_PIPE,
				self.pool,
				0,
			);
		}
	}

	/// # Safety
	///
	/// The command buffer must still be recording and have called
	/// `record_begin` for this query pool.
	pub(super) unsafe fn record_end(&self, command_buffer: vk::CommandBuffer) {
		unsafe {
			self.device.cmd_write_timestamp(
				command_buffer,
				vk::PipelineStageFlags::BOTTOM_OF_PIPE,
				self.pool,
				1,
			);
		}
	}

	pub(super) fn mark_submitted(&mut self) {
		self.pending = true;
	}

	pub(super) fn last_ms(&self) -> Option<f64> {
		self.last_ms
	}
}

impl Drop for GpuTimer {
	fn drop(&mut self) {
		unsafe {
			self.device.destroy_query_pool(self.pool, None);
		}
	}
}
