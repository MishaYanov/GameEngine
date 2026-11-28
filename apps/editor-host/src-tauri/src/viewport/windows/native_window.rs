use super::*;
use windows_sys::Win32::{
	Foundation::{POINT, RECT},
	UI::WindowsAndMessaging::{
		ClipCursor, GetClipCursor, GetCursorPos, GetSystemMetrics, GetWindowRect, WindowFromPoint,
		SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
	},
};

fn window_handle(window: &Window) -> Result<HWND, String> {
	let handle = window.window_handle().map_err(|error| error.to_string())?;
	let RawWindowHandle::Win32(handle) = handle.as_raw() else {
		return Err("Native viewport window has no Win32 handle".into());
	};
	Ok(handle.hwnd.get())
}

fn show_viewport_window(window: &Window) -> Result<(), String> {
	let hwnd = window_handle(window)?;
	window.set_visible(true);
	// Win32 creates child windows below existing siblings. Wry's windowed
	// WebView2 fills the same parent, so showing the viewport is insufficient.
	// SAFETY: The worker owns the live child HWND on this thread.
	let positioned = unsafe {
		SetWindowPos(
			hwnd,
			HWND_TOP,
			0,
			0,
			0,
			0,
			SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
		)
	};
	if positioned == 0 {
		return Err(format!(
			"Could not show the Vulkan viewport above the editor WebView: {}",
			std::io::Error::last_os_error()
		));
	}
	Ok(())
}

/// A one-pixel Win32 clip belonging to the focused Game viewport child.
pub(super) struct GameCursorClip {
	hwnd: HWND,
	rect: RECT,
}

impl GameCursorClip {
	pub(super) fn acquire(parent: HWND, window: &Window) -> Result<Self, String> {
		let hwnd = window_handle(window)?;
		let mut bounds = RECT {
			left: 0,
			top: 0,
			right: 0,
			bottom: 0,
		};
		// SAFETY: The worker owns this live child HWND and the output RECT.
		if unsafe { GetWindowRect(hwnd, &mut bounds) } == 0 {
			return Err(std::io::Error::last_os_error().to_string());
		}
		if bounds.left >= bounds.right || bounds.top >= bounds.bottom {
			return Err("Game viewport has no screen area".into());
		}

		let mut point = POINT { x: 0, y: 0 };
		// SAFETY: Read the position on the input desktop into a valid POINT.
		let cursor_is_over_child =
			unsafe { GetCursorPos(&mut point) != 0 && WindowFromPoint(point) == hwnd };
		if !cursor_is_over_child {
			// The child is undecorated. Its center stays inside the rendered
			// Game content even when the aspect ratio adds letterboxing.
			point = POINT {
				x: bounds.left.midpoint(bounds.right),
				y: bounds.top.midpoint(bounds.bottom),
			};
		}

		// SAFETY: Read-only queries on the worker's DPI-aware thread.
		let desktop = unsafe {
			let left = GetSystemMetrics(SM_XVIRTUALSCREEN);
			let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
			RECT {
				left,
				top,
				right: left + GetSystemMetrics(SM_CXVIRTUALSCREEN),
				bottom: top + GetSystemMetrics(SM_CYVIRTUALSCREEN),
			}
		};
		let baseline = cursor_clip().map_err(|error| error.to_string())?;
		if !same_rect(&baseline, &desktop) {
			return Err("The cursor is already confined".into());
		}

		// Winit compares the captured HWND with GetActiveWindow(), which
		// reports a top-level HWND. The embedded WS_CHILD owns GetFocus().
		// SAFETY: The worker owns the child and checks the foreground and
		// the topmost window at the lock point before touching the OS clip.
		if unsafe {
			GetForegroundWindow() != parent || GetFocus() != hwnd || WindowFromPoint(point) != hwnd
		} || point.x < bounds.left
			|| point.x >= bounds.right
			|| point.y < bounds.top
			|| point.y >= bounds.bottom
			|| point.x < desktop.left
			|| point.x >= desktop.right
			|| point.y < desktop.top
			|| point.y >= desktop.bottom
		{
			return Err("The Game viewport does not own the cursor point".into());
		}

		let rect = RECT {
			left: point.x,
			top: point.y,
			right: point.x + 1,
			bottom: point.y + 1,
		};
		// SAFETY: The rectangle is a nonempty, validated screen-pixel area.
		if unsafe { ClipCursor(&rect) } == 0 {
			return Err(std::io::Error::last_os_error().to_string());
		}
		Ok(Self { hwnd, rect })
	}

	pub(super) fn is_current(&self) -> bool {
		let point = POINT {
			x: self.rect.left,
			y: self.rect.top,
		};
		// SAFETY: Read-only check that the clip point still lies on the child.
		let point_owned = unsafe { WindowFromPoint(point) == self.hwnd };
		point_owned && cursor_clip().is_ok_and(|current| same_rect(&current, &self.rect))
	}
}

impl Drop for GameCursorClip {
	fn drop(&mut self) {
		// The cursor clip is shared by the desktop. Leave any later
		// restriction installed by another owner alone.
		if cursor_clip().is_ok_and(|current| same_rect(&current, &self.rect)) {
			// SAFETY: Passing NULL removes this still-current restriction.
			if unsafe { ClipCursor(std::ptr::null()) } == 0 {
				let error = std::io::Error::last_os_error();
				engine_logger::warn!(target: "editor_viewport", "Could not release the Game camera cursor: {error}");
			}
		}
	}
}

fn cursor_clip() -> Result<RECT, std::io::Error> {
	let mut rect = RECT {
		left: 0,
		top: 0,
		right: 0,
		bottom: 0,
	};
	// SAFETY: The OS writes the current clip into this valid RECT.
	if unsafe { GetClipCursor(&mut rect) } == 0 {
		return Err(std::io::Error::last_os_error());
	}
	Ok(rect)
}

fn same_rect(a: &RECT, b: &RECT) -> bool {
	[a.left, a.top, a.right, a.bottom] == [b.left, b.top, b.right, b.bottom]
}

pub(super) struct KeyboardFocusHook {
	hwnd: HWND,
	_control: Arc<Control>,
}
impl KeyboardFocusHook {
	pub(super) fn new(window: &Window, control: Arc<Control>) -> Result<Self, String> {
		let hwnd = window_handle(window)?;
		// SAFETY: Installed on the child owner thread. The hook retains the
		// control pointer until removal, and is dropped before the window.
		if unsafe {
			SetWindowSubclass(
				hwnd,
				Some(keyboard_focus_proc),
				0,
				Arc::as_ptr(&control) as usize,
			)
		} == 0
		{
			return Err("Could not monitor keyboard focus for the native viewport".into());
		}
		Ok(Self {
			hwnd,
			_control: control,
		})
	}
}
impl Drop for KeyboardFocusHook {
	fn drop(&mut self) {
		// SAFETY: The worker drops its own hook on the child owner thread.
		unsafe { RemoveWindowSubclass(self.hwnd, Some(keyboard_focus_proc), 0) };
	}
}

unsafe extern "system" fn keyboard_focus_proc(
	hwnd: HWND,
	message: u32,
	wparam: WPARAM,
	lparam: LPARAM,
	id: usize,
	context: usize,
) -> LRESULT {
	if message == WM_KILLFOCUS {
		// SAFETY: The installed hook owns the Arc backing this reference.
		let control = unsafe { &*(context as *const Control) };
		control.keyboard_blurred.store(true, Ordering::Release);
		control.wake();
	} else if message == WM_NCDESTROY {
		// SAFETY: A destroying HWND can detach its own subclass procedure.
		unsafe { RemoveWindowSubclass(hwnd, Some(keyboard_focus_proc), id) };
	}
	// SAFETY: Forward every message through the existing subclass chain.
	unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

impl Worker {
	pub(super) fn update_window_visibility(&mut self) {
		if let Some(window) = self.window.as_ref() {
			if self.active() && !self.failed {
				if self.focus_hook.is_none() {
					match KeyboardFocusHook::new(window, Arc::clone(&self.control)) {
						Ok(hook) => self.focus_hook = Some(hook),
						Err(error) => {
							self.fail("window_keyboard_focus", error);
							return;
						}
					}
				}
				if let Err(error) = show_viewport_window(window) {
					self.fail("window_z_order", error);
				}
			} else {
				window.set_visible(false);
			}
		}
	}
	pub(super) fn native_keyboard_focused(&self) -> bool {
		// SAFETY: Read-only queries for the worker's own input queue.
		self.focus_hook.as_ref().is_some_and(|hook| unsafe {
			GetForegroundWindow() == self.hwnd && GetFocus() == hook.hwnd
		})
	}
	pub(super) fn note_keyboard_focus(&mut self, focused: bool) {
		if !focused {
			self.release_game_camera();
			self.modifiers = ModifiersState::empty();
			self.pending_motion = None;
			self.pending_fly_motion = [0.0; 2];
			if let Some(navigation) = &mut self.prop_navigation {
				self.dirty |= navigation.captured();
				navigation.cancel();
				self.update_capture();
			}
		}
		if self.interaction.keyboard_focused() != focused {
			let event = if focused { "focused" } else { "blurred" };
			self.process_input(event, None, None, false, None);
			self.emit_input(event, None, None, false, None);
		}
	}
	pub(super) fn drain_native_blur(&mut self) {
		if self.control.keyboard_blurred.swap(false, Ordering::AcqRel) {
			self.note_keyboard_focus(false);
		}
	}
	pub(super) fn focus_keyboard(&mut self) {
		let Some(hook) = self.focus_hook.as_ref() else {
			return;
		};
		// SAFETY: SetFocus targets our child on its owning thread, and only
		// while the editor is the foreground window.
		if self.active() && !self.failed && unsafe { GetForegroundWindow() == self.hwnd } {
			unsafe { SetFocus(hook.hwnd) };
			self.note_keyboard_focus(self.native_keyboard_focused());
		}
	}
	pub(super) fn release_keyboard_focus(&mut self) {
		let was_focused = self.interaction.keyboard_focused();
		self.note_keyboard_focus(false);
		let Some(hook) = self.focus_hook.as_ref() else {
			return;
		};
		// SAFETY: Clear focus only when our own child still owns it.
		if unsafe { GetFocus() } != hook.hwnd {
			return;
		}
		unsafe { SetFocus(0) };
		if !was_focused {
			// Escape already updated interaction state before releasing the HWND.
			self.emit_input("blurred", None, None, false, None);
		}
		let parent = self.parent.clone();
		let hwnd = self.hwnd;
		let _ = self.parent.run_on_main_thread(move || {
			// An intervening click or dialog owns its own focus. Wry's
			// programmatic WebView focus restores the prior HTML element.
			let (foreground, focused) = unsafe { (GetForegroundWindow(), GetFocus()) };
			if foreground == hwnd && (focused == 0 || focused == hwnd) {
				let _ = parent.as_ref().set_focus();
			}
		});
	}
	pub(super) fn refresh_game_keyboard_focus(&mut self) {
		let game_active = self.interaction.game_keyboard_active();
		if game_active != self.game_keyboard_active {
			self.game_keyboard_active = game_active;
			self.game_focus_pending = game_active;
			if !game_active {
				self.release_keyboard_focus();
			}
		}
		if self.game_focus_pending && self.active() && !self.failed && self.focus_hook.is_some() {
			// This is a one-shot request on entering Play/Game. Escape, blur,
			// and restoring a hidden surface must not reacquire it.
			self.game_focus_pending = false;
			self.focus_keyboard();
		}
	}
}
