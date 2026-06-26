use std::ffi::CString;

#[allow(dead_code)]
pub fn user_exists(user: &str) -> bool {
    let Ok(cname) = CString::new(user) else {
        return false;
    };
    !unsafe { libc::getpwnam(cname.as_ptr()) }.is_null()
}

pub fn drop_to_user(user: &str) -> Result<(), String> {
    let cname = CString::new(user).map_err(|e| e.to_string())?;
    let pwd = unsafe { libc::getpwnam(cname.as_ptr()) };
    if pwd.is_null() {
        return Err(format!("user '{user}' not found"));
    }

    let (uid, gid, name) = unsafe { ((*pwd).pw_uid, (*pwd).pw_gid, (*pwd).pw_name) };

    if unsafe { libc::setgroups(0, std::ptr::null()) } != 0 {
        return Err(format!("setgroups(0): {}", std::io::Error::last_os_error()));
    }
    if unsafe { libc::initgroups(name, gid) } != 0 {
        return Err(format!(
            "initgroups({user}): {}",
            std::io::Error::last_os_error()
        ));
    }
    if unsafe { libc::setgid(gid) } != 0 {
        return Err(format!(
            "setgid({gid}): {}",
            std::io::Error::last_os_error()
        ));
    }
    if unsafe { libc::setuid(uid) } != 0 {
        return Err(format!(
            "setuid({uid}): {}",
            std::io::Error::last_os_error()
        ));
    }

    Ok(())
}
