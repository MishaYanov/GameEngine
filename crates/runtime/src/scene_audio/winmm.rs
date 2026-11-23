use std::{
	fmt, io,
	mem::size_of,
	os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
	ptr,
};

use windows_sys_audio::Win32::{
	Foundation::WAIT_OBJECT_0,
	Media::{
		Audio::{
			CALLBACK_EVENT, HWAVEOUT, WAVE_FORMAT_PCM, WAVE_MAPPER, WAVEFORMATEX, WAVEHDR,
			WHDR_BEGINLOOP, WHDR_DONE, WHDR_ENDLOOP, waveOutClose, waveOutOpen, waveOutPause,
			waveOutPrepareHeader, waveOutReset, waveOutRestart, waveOutSetVolume,
			waveOutUnprepareHeader, waveOutWrite,
		},
		MMSYSERR_NOERROR,
	},
	System::Threading::{CreateEventW, ResetEvent, WaitForSingleObject},
};

const HEADER_SIZE: u32 = size_of::<WAVEHDR>() as u32;
const FORMAT: WAVEFORMATEX = WAVEFORMATEX {
	wFormatTag: WAVE_FORMAT_PCM as u16,
	nChannels: 1,
	nSamplesPerSec: 16_000,
	nAvgBytesPerSec: 32_000,
	nBlockAlign: 2,
	wBitsPerSample: 16,
	cbSize: 0,
};

#[derive(Debug)]
pub struct WinmmError {
	operation: &'static str,
	status: u32,
}

impl fmt::Display for WinmmError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			formatter,
			"{} returned status {}",
			self.operation, self.status
		)
	}
}

struct PlayingBuffer {
	header: *mut WAVEHDR,
	pcm: *mut [u8],
}

impl PlayingBuffer {
	fn new(pcm: Box<[u8]>) -> Self {
		let len = pcm.len();
		let pcm = Box::into_raw(pcm);
		let header = Box::into_raw(Box::new(WAVEHDR {
			lpData: pcm.cast::<u8>(),
			dwBufferLength: len as u32,
			..WAVEHDR::default()
		}));
		Self { header, pcm }
	}
}

impl Drop for PlayingBuffer {
	fn drop(&mut self) {
		// The queue owner drops this only before successful preparation or
		// after successful unprepare. Both pointers come from Box::into_raw.
		unsafe {
			drop(Box::from_raw(self.header));
			drop(Box::from_raw(self.pcm));
		}
	}
}

struct CompletionEvent(OwnedHandle);

impl CompletionEvent {
	fn new() -> Result<Self, WinmmError> {
		// A private, initially unsignaled auto-reset event serves one queued
		// preview buffer. The output owns its handle through waveOutClose.
		let handle = unsafe { CreateEventW(ptr::null(), 0, 0, ptr::null()) };
		if handle.is_null() {
			return Err(Self::last_error("CreateEventW"));
		}
		Ok(Self(unsafe { OwnedHandle::from_raw_handle(handle) }))
	}

	fn reset(&self) -> Result<(), WinmmError> {
		// The owned handle stays valid for the synchronous reset call.
		if unsafe { ResetEvent(self.0.as_raw_handle()) } == 0 {
			return Err(Self::last_error("ResetEvent"));
		}
		Ok(())
	}

	fn signaled(&self) -> bool {
		// Zero timeout preserves the event loop's non-blocking polling.
		(unsafe { WaitForSingleObject(self.0.as_raw_handle(), 0) }) == WAIT_OBJECT_0
	}

	fn last_error(operation: &'static str) -> WinmmError {
		WinmmError {
			operation,
			status: io::Error::last_os_error()
				.raw_os_error()
				.unwrap_or_default() as u32,
		}
	}
}

pub struct WaveOutput {
	handle: HWAVEOUT,
	buffer: Option<PlayingBuffer>,
	completion: Option<CompletionEvent>,
	stereo: bool,
}

impl WaveOutput {
	pub fn open(report_completion: bool) -> Result<Self, WinmmError> {
		Self::open_format(report_completion, false)
	}

	pub fn open_stereo(report_completion: bool) -> Result<Self, WinmmError> {
		Self::open_format(report_completion, true)
	}

	fn open_format(report_completion: bool, stereo: bool) -> Result<Self, WinmmError> {
		let completion = report_completion.then(CompletionEvent::new).transpose()?;
		let (callback, flags) = completion.as_ref().map_or((0, 0), |event| {
			(event.0.as_raw_handle() as usize, CALLBACK_EVENT)
		});
		let mut handle = ptr::null_mut();
		let mut format = FORMAT;
		if stereo {
			format.nChannels = 2;
			format.nAvgBytesPerSec = FORMAT.nAvgBytesPerSec * 2;
			format.nBlockAlign = 4;
		}
		// FORMAT and the output pointer are valid for this call. WinMM can
		// signal the owned event until the output is closed.
		check("waveOutOpen", unsafe {
			waveOutOpen(&mut handle, WAVE_MAPPER, &format, callback, 0, flags)
		})?;
		Ok(Self {
			handle,
			buffer: None,
			completion,
			stereo,
		})
	}

	pub fn submit(&mut self, samples: &[u8], gain: u8, repeat: bool) -> Result<(), WinmmError> {
		self.stop()?;
		let mono = scaled_pcm(samples, gain);
		let pcm = if self.stereo {
			mono.chunks_exact(2)
				.flat_map(|frame| [frame[0], frame[1], frame[0], frame[1]])
				.collect()
		} else {
			mono
		};
		let buffer = PlayingBuffer::new(pcm);
		let header = buffer.header;
		// Both allocations use raw ownership until stop() has reset and
		// unprepared the driver buffer. WinMM updates WAVEHDR flags.
		check("waveOutPrepareHeader", unsafe {
			waveOutPrepareHeader(self.handle, header, HEADER_SIZE)
		})?;
		if repeat {
			// WinMM permits setting loop fields after preparation and before
			// queueing, when playback cannot write the header.
			unsafe {
				(*header).dwFlags |= WHDR_BEGINLOOP | WHDR_ENDLOOP;
				(*header).dwLoops = u32::MAX;
			}
		}
		self.buffer = Some(buffer);
		if let Some(event) = &self.completion {
			event.reset()?;
		}
		check("waveOutWrite", unsafe {
			waveOutWrite(self.handle, header, HEADER_SIZE)
		})?;
		self.resume()
	}

	pub fn finished(&self) -> bool {
		self.buffer.as_ref().is_some_and(|buffer| {
			if !self
				.completion
				.as_ref()
				.is_some_and(CompletionEvent::signaled)
			{
				return false;
			}
			// WinMM signaled a state change. Check the current playback
			// header as documented for CALLBACK_EVENT.
			let flags = unsafe { ptr::addr_of!((*buffer.header).dwFlags).read_volatile() };
			flags & WHDR_DONE != 0
		})
	}

	/// Device handles change only this output instance.
	pub fn set_gain(&mut self, [left, right]: [f32; 2]) -> Result<(), WinmmError> {
		let level = |value: f32| (value.clamp(0.0, 1.0) * f32::from(u16::MAX)).round() as u16;
		let volume = u32::from(level(left)) | (u32::from(level(right)) << 16);
		check("waveOutSetVolume", unsafe {
			waveOutSetVolume(self.handle, volume)
		})
	}

	pub fn pause(&mut self) -> Result<(), WinmmError> {
		// The live handle belongs exclusively to this output lane.
		check("waveOutPause", unsafe { waveOutPause(self.handle) })
	}

	pub fn resume(&mut self) -> Result<(), WinmmError> {
		// Restart is a no-op on an output that is already running.
		check("waveOutRestart", unsafe { waveOutRestart(self.handle) })
	}

	pub fn stop(&mut self) -> Result<(), WinmmError> {
		let Some(buffer) = &self.buffer else {
			return Ok(());
		};
		// A successful reset returns every queued buffer for this handle.
		// Free neither allocation until unprepare has relinquished the header.
		check("waveOutReset", unsafe { waveOutReset(self.handle) })?;
		check("waveOutUnprepareHeader", unsafe {
			waveOutUnprepareHeader(self.handle, buffer.header, HEADER_SIZE)
		})?;
		self.buffer = None;
		Ok(())
	}
}

impl Drop for WaveOutput {
	fn drop(&mut self) {
		let unprepared = self.stop().is_ok();
		// The handle remains valid until close returns. If cleanup failed
		// and the driver could still own the buffer, leave that allocation
		// live for the process lifetime. The failing lane is never reused.
		let closed = unsafe { waveOutClose(self.handle) } == MMSYSERR_NOERROR;
		if !closed && let Some(event) = self.completion.take() {
			std::mem::forget(event);
		}
		if !unprepared && let Some(buffer) = self.buffer.take() {
			std::mem::forget(buffer);
		}
	}
}

pub fn scaled_pcm(samples: &[u8], gain: u8) -> Box<[u8]> {
	if gain == 100 {
		return samples.into();
	}
	let scale = i32::from(gain.min(100));
	samples
		.chunks_exact(2)
		.flat_map(|bytes| {
			let sample = i32::from(i16::from_le_bytes([bytes[0], bytes[1]]));
			((sample * scale / 100) as i16).to_le_bytes()
		})
		.collect()
}

fn check(operation: &'static str, status: u32) -> Result<(), WinmmError> {
	if status == MMSYSERR_NOERROR {
		Ok(())
	} else {
		Err(WinmmError { operation, status })
	}
}
