//! Read an XInput slot without taking ownership of gameplay input.

use super::PhysicalGamepadSample;
use std::mem::MaybeUninit;
use windows_sys::Win32::{
	Foundation::ERROR_SUCCESS,
	UI::Input::XboxController::{
		XINPUT_GAMEPAD_A, XINPUT_GAMEPAD_B, XINPUT_GAMEPAD_X, XINPUT_GAMEPAD_Y, XINPUT_STATE,
		XInputGetState,
	},
};

pub fn poll_gamepad(slot: u32) -> Option<PhysicalGamepadSample> {
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
	Some(PhysicalGamepadSample {
		left_x: pad.sThumbLX,
		left_y: pad.sThumbLY,
		face_buttons: [
			XINPUT_GAMEPAD_A,
			XINPUT_GAMEPAD_B,
			XINPUT_GAMEPAD_X,
			XINPUT_GAMEPAD_Y,
		]
		.map(|button| pad.wButtons & button != 0),
	})
}
