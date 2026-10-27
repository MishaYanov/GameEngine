"""Windows process-tree ownership for apps started by the development launcher."""
from __future__ import annotations

import ctypes
from ctypes import wintypes
import time


class _Limits(ctypes.Structure):
	_fields_ = [
		("process_time", ctypes.c_int64), ("job_time", ctypes.c_int64),
		("flags", wintypes.DWORD), ("min_working_set", ctypes.c_size_t),
		("max_working_set", ctypes.c_size_t), ("active_limit", wintypes.DWORD),
		("affinity", ctypes.c_size_t), ("priority", wintypes.DWORD),
		("scheduling", wintypes.DWORD),
	]


class _ExtendedLimits(ctypes.Structure):
	_fields_ = [
		("basic", _Limits), ("io_counters", ctypes.c_uint64 * 6),
		("process_memory", ctypes.c_size_t), ("job_memory", ctypes.c_size_t),
		("peak_process_memory", ctypes.c_size_t), ("peak_job_memory", ctypes.c_size_t),
	]


class _Accounting(ctypes.Structure):
	_fields_ = [
		("times", ctypes.c_int64 * 4), ("page_faults", wintypes.DWORD),
		("total_processes", wintypes.DWORD), ("active_processes", wintypes.DWORD),
		("terminated_processes", wintypes.DWORD),
	]


class WindowsJob:
	"""Hold native handles, never reopen arbitrary processes using a recycled PID."""

	def __init__(self) -> None:
		self.handle = None
		self.api = ctypes.WinDLL("kernel32", use_last_error=True)
		for name, arguments, result in (
			("CreateJobObjectW", [ctypes.c_void_p, wintypes.LPCWSTR], wintypes.HANDLE),
			("SetInformationJobObject", [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD], wintypes.BOOL),
			("AssignProcessToJobObject", [wintypes.HANDLE, wintypes.HANDLE], wintypes.BOOL),
			("QueryInformationJobObject", [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD, ctypes.c_void_p], wintypes.BOOL),
			("TerminateJobObject", [wintypes.HANDLE, wintypes.UINT], wintypes.BOOL),
			("CloseHandle", [wintypes.HANDLE], wintypes.BOOL),
		):
			function = getattr(self.api, name)
			function.argtypes, function.restype = arguments, result
		self.handle = self.api.CreateJobObjectW(None, None)
		if not self.handle:
			raise ctypes.WinError(ctypes.get_last_error())
		limits = _ExtendedLimits()
		limits.basic.flags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
		if not self.api.SetInformationJobObject(self.handle, 9, ctypes.byref(limits), ctypes.sizeof(limits)):
			error = ctypes.WinError(ctypes.get_last_error())
			self.close()
			raise error

	def attach(self, child) -> None:
		if not self.api.AssignProcessToJobObject(self.handle, int(child._handle)):
			raise ctypes.WinError(ctypes.get_last_error())

	def active_count(self) -> int:
		if not self.handle:
			return 0
		accounting = _Accounting()
		if not self.api.QueryInformationJobObject(self.handle, 1, ctypes.byref(accounting), ctypes.sizeof(accounting), None):
			raise ctypes.WinError(ctypes.get_last_error())
		return accounting.active_processes

	def terminate(self) -> None:
		if self.handle and not self.api.TerminateJobObject(self.handle, 1):
			raise ctypes.WinError(ctypes.get_last_error())

	def wait_empty(self, timeout: float = 3.0) -> None:
		deadline = time.monotonic() + timeout
		while self.active_count():
			if time.monotonic() >= deadline:
				raise RuntimeError("App processes did not stop within three seconds; restart was cancelled.")
			time.sleep(0.02)

	def close(self) -> None:
		if self.handle:
			self.api.CloseHandle(self.handle)
			self.handle = None

	def __del__(self) -> None:
		self.close()
