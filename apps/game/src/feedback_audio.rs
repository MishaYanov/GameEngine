//! Bounded presentation cue for a newly committed Courtyard gate opening.

#[cfg(windows)]
pub(super) fn play_gate_cue() {
	use std::ffi::c_void;

	#[link(name = "winmm")]
	unsafe extern "system" {
		fn PlaySoundW(sound: *const u16, module: *mut c_void, flags: u32) -> i32;
	}

	// Original, short mono PCM tone embedded in the executable. Memory is valid
	// for the full asynchronous call and no authored path reaches WinMM.
	static CUE: &[u8] = include_bytes!("gate-open.wav");
	const SND_ASYNC: u32 = 0x0001;
	const SND_NODEFAULT: u32 = 0x0002;
	const SND_MEMORY: u32 = 0x0004;
	const SND_NOSTOP: u32 = 0x0010;
	// SAFETY: SND_MEMORY takes the address of a complete WAVE memory image;
	// WinMM does not interpret it as UTF-16 and the static allocation is retained.
	unsafe {
		PlaySoundW(
			CUE.as_ptr().cast(),
			std::ptr::null_mut(),
			SND_ASYNC | SND_NODEFAULT | SND_MEMORY | SND_NOSTOP,
		);
	}
}

#[cfg(not(windows))]
pub(super) fn play_gate_cue() {}
