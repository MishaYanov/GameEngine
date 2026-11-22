use super::*;

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
