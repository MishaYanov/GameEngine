//! Edge-based physical controls for the Player menu.

use runtime::{
	Key,
	input::{FaceButton, PhysicalGamepadSample},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MenuIntent {
	Open,
	Back,
	Confirm,
	Up,
	Down,
	Left,
	Right,
}

impl MenuIntent {
	pub(super) fn key(self) -> Key {
		match self {
			Self::Open | Self::Back => Key::Escape,
			Self::Confirm => Key::Enter,
			Self::Up => Key::ArrowUp,
			Self::Down => Key::ArrowDown,
			Self::Left => Key::ArrowLeft,
			Self::Right => Key::ArrowRight,
		}
	}
}

#[derive(Default)]
pub(super) struct ControllerMenuInput {
	previous: Option<PhysicalGamepadSample>,
}

impl ControllerMenuInput {
	pub(super) fn reset(&mut self) {
		self.previous = None;
	}

	pub(super) fn update(
		&mut self,
		sample: PhysicalGamepadSample,
		is_open: bool,
	) -> Option<MenuIntent> {
		let Some(previous) = self.previous else {
			if sample.is_neutral() {
				self.previous = Some(sample);
			}
			return None;
		};
		self.previous = Some(sample);
		let rose = |current: bool, prior: bool| current && !prior;
		if !is_open {
			return rose(sample.menu.open, previous.menu.open).then_some(MenuIntent::Open);
		}
		let face_rose = |button: FaceButton| {
			let index = button as usize;
			rose(sample.face_buttons[index], previous.face_buttons[index])
		};
		[
			(face_rose(FaceButton::B), MenuIntent::Back),
			(face_rose(FaceButton::A), MenuIntent::Confirm),
			(
				rose(sample.menu.up, previous.menu.up) && !sample.menu.down,
				MenuIntent::Up,
			),
			(
				rose(sample.menu.down, previous.menu.down) && !sample.menu.up,
				MenuIntent::Down,
			),
			(
				rose(sample.menu.left, previous.menu.left) && !sample.menu.right,
				MenuIntent::Left,
			),
			(
				rose(sample.menu.right, previous.menu.right) && !sample.menu.left,
				MenuIntent::Right,
			),
		]
		.into_iter()
		.find_map(|(active, intent)| active.then_some(intent))
	}
}

#[cfg(test)]
#[path = "../../tests/unit/controller_menu_input.rs"]
mod tests;
