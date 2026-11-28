"""Deliver bounded client-coordinate mouse messages to a disposable Editor HWND."""

import atexit
import ctypes as C
import json
import os
import sys
import time
from ctypes import wintypes as W

PID = int(sys.argv[1])
EXPECTED_IMAGE = os.path.normcase(os.path.realpath(sys.argv[2]))
if PID <= 0 or os.path.basename(EXPECTED_IMAGE) != "game-engine-editor.exe":
    raise ValueError("Expected the launched Game Engine Editor PID and executable")
ACTION = sys.argv[3]
ARGUMENTS = list(map(int, sys.argv[4:]))
user = C.WinDLL('user32', use_last_error=True)
kernel = C.WinDLL('kernel32', use_last_error=True)
callback_type = C.WINFUNCTYPE(W.BOOL, W.HWND, W.LPARAM)
user.EnumWindows.argtypes = (callback_type, W.LPARAM)
user.EnumChildWindows.argtypes = (W.HWND, callback_type, W.LPARAM)
user.GetForegroundWindow.restype = W.HWND
user.GetWindowTextW.argtypes = (W.HWND, W.LPWSTR, C.c_int)
user.GetWindowThreadProcessId.argtypes = (W.HWND, C.POINTER(W.DWORD))
user.GetClientRect.argtypes = (W.HWND, C.POINTER(W.RECT))
user.GetDpiForWindow.argtypes = (W.HWND,)
user.PostMessageW.argtypes = (W.HWND, W.UINT, W.WPARAM, W.LPARAM)
kernel.OpenProcess.argtypes = (W.DWORD, W.BOOL, W.DWORD)
kernel.OpenProcess.restype = W.HANDLE
kernel.CloseHandle.argtypes = (W.HANDLE,)
kernel.WaitForSingleObject.argtypes = (W.HANDLE, W.DWORD)
kernel.WaitForSingleObject.restype = W.DWORD
kernel.QueryFullProcessImageNameW.argtypes = (W.HANDLE, W.DWORD, W.LPWSTR, C.POINTER(W.DWORD))

PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
SYNCHRONIZE = 0x00100000
WAIT_TIMEOUT = 0x102
process = kernel.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, False, PID)
if not process:
    raise C.WinError(C.get_last_error())
atexit.register(kernel.CloseHandle, process)

path = C.create_unicode_buffer(32768)
path_length = W.DWORD(len(path))
if not kernel.QueryFullProcessImageNameW(process, 0, path, C.byref(path_length)):
    raise C.WinError(C.get_last_error())
if os.path.normcase(os.path.realpath(path.value)) != EXPECTED_IMAGE:
    raise RuntimeError('Refusing to deliver test input to a different process')


def belongs_to_test(hwnd):
    owner_pid = W.DWORD()
    user.GetWindowThreadProcessId(hwnd, C.byref(owner_pid))
    return owner_pid.value == PID and kernel.WaitForSingleObject(process, 0) == WAIT_TIMEOUT

def title(hwnd):
    value = C.create_unicode_buffer(256)
    user.GetWindowTextW(hwnd, value, len(value))
    return value.value

parents = []
children = []
@callback_type
def top(hwnd, _):
    if belongs_to_test(hwnd):
        parents.append(hwnd)
    return True

@callback_type
def descendant(hwnd, _):
    if belongs_to_test(hwnd) and title(hwnd) == 'Game Engine viewport':
        children.append(hwnd)
    return True

user.EnumWindows(top, 0)
for parent in parents:
    user.EnumChildWindows(parent, descendant, 0)
matches = list(dict.fromkeys(children))
if len(matches) != 1:
    raise RuntimeError(f'Expected one Vulkan child in the test process, found {len(matches)}')
target = matches[0]
client = W.RECT()
if not user.GetClientRect(target, C.byref(client)):
    raise C.WinError(C.get_last_error())
width, height = client.right, client.bottom
dpi = user.GetDpiForWindow(target)
foreground_before = user.GetForegroundWindow()

def post(message, buttons, x, y):
    if not belongs_to_test(target) or title(target) != 'Game Engine viewport':
        raise RuntimeError('The target viewport changed during the gesture')
    if not (0 <= x < min(width, 32768) and 0 <= y < min(height, 32768)):
        raise ValueError('Test coordinates are outside the native client rectangle')
    if not user.PostMessageW(target, message, buttons, (y << 16) | x):
        raise C.WinError(C.get_last_error())
    time.sleep(0.12)

if ACTION in ('click', 'drag', 'right-click', 'right-drag'):
    right = ACTION.startswith('right-')
    drag = ACTION.endswith('drag')
    expected = 4 if drag else 2
    if len(ARGUMENTS) != expected:
        raise ValueError(f'Expected {expected} client coordinates')
    x, y = ARGUMENTS[:2]
    end_x, end_y = ARGUMENTS[2:] if drag else (x, y)
    if not (0 <= end_x < min(width, 32768) and 0 <= end_y < min(height, 32768)):
        raise ValueError('Gesture would leave the native client rectangle')
    down_message, up_message, button = (0x0204, 0x0205, 2) if right else (0x0201, 0x0202, 1)
    post(0x0200, 0, x, y)
    post(down_message, button, x, y)
    try:
        if drag:
            post(0x0200, button, end_x, end_y)
    finally:
        post(up_message, 0, end_x, end_y)
elif ACTION != 'info':
    raise ValueError('Unknown action')

foreground_after = user.GetForegroundWindow()
print(json.dumps(dict(pid=PID, hwnd=target, width=width, height=height, dpi=dpi,
    foreground_unchanged=foreground_before == foreground_after)))
