//! Read an XInput slot without taking ownership of gameplay input.

use super::GamepadSample;
use std::mem::MaybeUninit;
use windows_sys::Win32::{
	Foundation::ERROR_SUCCESS,
	UI::Input::XboxController::{XINPUT_GAMEPAD_A, XINPUT_GAMEPAD_X, XINPUT_STATE, XInputGetState},
};

pub fn poll_gamepad(slot: u32) -> Option<GamepadSample> {
	if slot > 3 {
		return None;
	}
	let mut state = MaybeUninit::<XINPUT_STATE>::uninit();
	// SAFETY: the writable, correctly aligned buffer is valid for this call.
	if unsafe { XInputGetState(slot, state.as_mut_ptr()) } != ERROR_SUCCESS {
		return None;
	}
	// SAFETY: a successful XInputGetState call initializes the state.
	let pad = unsafe { state.assume_init() }.Gamepad;
	Some(GamepadSample {
		left_x: pad.sThumbLX,
		left_y: pad.sThumbLY,
		jump: pad.wButtons & XINPUT_GAMEPAD_A != 0,
		interact: pad.wButtons & XINPUT_GAMEPAD_X != 0,
	})
}
