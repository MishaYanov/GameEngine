//! Own a Windows job handle, without reopening arbitrary processes by PID.
use std::{io, os::windows::io::AsRawHandle, process::Child};
use windows_sys::Win32::{Foundation::CloseHandle, System::JobObjects::*};

pub struct Job(usize);
impl Job {
    pub fn attach(child: &Child) -> io::Result<Self> {
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = Self(handle as usize);
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of_val(&limits) as u32,
            )
        };
        if configured == 0 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { AssignProcessToJobObject(handle, child.as_raw_handle()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }
    pub fn terminate(&self) -> io::Result<()> {
        if unsafe { TerminateJobObject(self.0 as _, 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0 as _);
        }
    }
}
