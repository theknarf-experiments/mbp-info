use std::ffi::CString;

use libc::c_void;

pub(crate) fn sysctl_string(name: &str) -> Result<String, String> {
    let cname = CString::new(name).map_err(|_| format!("invalid sysctl name: {name}"))?;
    let mut size: libc::size_t = 0;
    let rc = unsafe {
        libc::sysctlbyname(
            cname.as_ptr(),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return Err(format!("sysctl {name} failed"));
    }

    let mut buf = vec![0u8; size as usize];
    let rc = unsafe {
        libc::sysctlbyname(
            cname.as_ptr(),
            buf.as_mut_ptr() as *mut c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return Err(format!("sysctl {name} failed"));
    }

    let value = String::from_utf8_lossy(&buf[..size as usize]);
    Ok(value.trim_end_matches('\0').trim().to_string())
}

pub(crate) fn sysctl_u64(name: &str) -> Result<u64, String> {
    let cname = CString::new(name).map_err(|_| format!("invalid sysctl name: {name}"))?;
    let mut value: u64 = 0;
    let mut size = std::mem::size_of::<u64>();
    let rc = unsafe {
        libc::sysctlbyname(
            cname.as_ptr(),
            &mut value as *mut _ as *mut c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return Err(format!("sysctl {name} failed"));
    }

    Ok(value)
}

pub(crate) fn sysctl_timeval(name: &str) -> Result<libc::timeval, String> {
    let cname = CString::new(name).map_err(|_| format!("invalid sysctl name: {name}"))?;
    let mut value: libc::timeval = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of::<libc::timeval>();
    let rc = unsafe {
        libc::sysctlbyname(
            cname.as_ptr(),
            &mut value as *mut _ as *mut c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return Err(format!("sysctl {name} failed"));
    }

    Ok(value)
}
