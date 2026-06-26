use std::ffi::CString;
use std::fs;
use std::path::Path;

use crate::service::ghosttype_log;

pub fn mount_filesystem(
    what: &str,
    where_: &str,
    fstype: &str,
    options: &str,
) -> Result<(), String> {
    fs::create_dir_all(where_).map_err(|e| e.to_string())?;

    let c_source = CString::new(what).map_err(|e| e.to_string())?;
    let c_target = CString::new(where_).map_err(|e| e.to_string())?;
    let c_fstype = CString::new(fstype).map_err(|e| e.to_string())?;
    let c_opts = CString::new(options).map_err(|e| e.to_string())?;

    let rc = unsafe {
        libc::mount(
            c_source.as_ptr(),
            c_target.as_ptr(),
            c_fstype.as_ptr(),
            0,
            c_opts.as_ptr() as *const libc::c_void,
        )
    };

    if rc == 0 {
        ghosttype_log("MOUNT", &format!("Mounted {what} on {where_} ({fstype})"));
        Ok(())
    } else {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EBUSY) {
            ghosttype_log("MOUNT", &format!("{where_} already mounted"));
            Ok(())
        } else {
            Err(format!("mount {where_}: {err}"))
        }
    }
}

pub fn unmount(where_: &str) -> Result<(), String> {
    let c_target = CString::new(where_).map_err(|e| e.to_string())?;
    let rc = unsafe { libc::umount(c_target.as_ptr()) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

#[allow(dead_code)]
pub fn is_mount_point(path: &Path) -> bool {
    path.exists()
}
