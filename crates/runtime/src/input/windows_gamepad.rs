//! Read an XInput slot without taking ownership of gameplay input.

use super::{MenuButtons, PhysicalGamepadSample};
use std::mem::MaybeUninit;
use windows_sys::Win32::{
	Foundation::ERROR_SUCCESS,
	UI::Input::XboxController::{
		XINPUT_GAMEPAD_A, XINPUT_GAMEPAD_B, XINPUT_GAMEPAD_DPAD_DOWN, XINPUT_GAMEPAD_DPAD_LEFT,
		XINPUT_GAMEPAD_DPAD_RIGHT, XINPUT_GAMEPAD_DPAD_UP, XINPUT_GAMEPAD_START, XINPUT_GAMEPAD_X,
		XINPUT_GAMEPAD_Y, XINPUT_STATE, XInputGetState,
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
	let pressed = |button| pad.wButtons & button != 0;
	Some(PhysicalGamepadSample {
		left_x: pad.sThumbLX,
		left_y: pad.sThumbLY,
		right_x: pad.sThumbRX,
		right_y: pad.sThumbRY,
		face_buttons: [
			XINPUT_GAMEPAD_A,
			XINPUT_GAMEPAD_B,
			XINPUT_GAMEPAD_X,
			XINPUT_GAMEPAD_Y,
		]
		.map(pressed),
		menu: MenuButtons {
			open: pressed(XINPUT_GAMEPAD_START),
			up: pressed(XINPUT_GAMEPAD_DPAD_UP),
			down: pressed(XINPUT_GAMEPAD_DPAD_DOWN),
			left: pressed(XINPUT_GAMEPAD_DPAD_LEFT),
			right: pressed(XINPUT_GAMEPAD_DPAD_RIGHT),
		},
	})
}
