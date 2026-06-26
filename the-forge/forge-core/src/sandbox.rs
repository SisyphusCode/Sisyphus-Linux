use serde::Deserialize;
use std::ffi::CString;
use std::fs;
use std::path::Path;
use std::ptr;

use crate::service::ghosttype_log;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SandboxConfig {
    #[serde(default, rename = "private-tmp")]
    pub private_tmp: bool,
    #[serde(default, rename = "protect-system")]
    pub protect_system: bool,
    #[serde(default, rename = "private-devices")]
    pub private_devices: bool,
    #[serde(default, rename = "no-new-privileges")]
    pub no_new_privileges: bool,
}

impl SandboxConfig {
    pub fn from_systemd_unit(unit: &crate::service::systemd::SystemdUnit) -> Self {
        let sec = unit.section("Service");
        let flag = |key: &str| {
            sec.and_then(|s| s.get(key))
                .and_then(|v| v.first())
                .is_some_and(|v| matches!(v.as_str(), "yes" | "true" | "strict" | "full"))
        };
        let protect = sec
            .and_then(|s| s.get("ProtectSystem"))
            .and_then(|v| v.first())
            .map(|s| s == "strict" || s == "full")
            .unwrap_or(false);

        Self {
            private_tmp: flag("PrivateTmp"),
            protect_system: protect || flag("ProtectSystem"),
            private_devices: flag("PrivateDevices"),
            no_new_privileges: flag("NoNewPrivileges"),
        }
    }
}

/// Apply systemd-style sandbox flags in the child pre_exec hook.
pub fn apply_in_child(cfg: &SandboxConfig, service_name: &str) -> Result<(), String> {
    if cfg.no_new_privileges {
        let rc = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
        if rc != 0 {
            return Err(format!(
                "NoNewPrivileges for '{service_name}': {}",
                std::io::Error::last_os_error()
            ));
        }
    }

    if cfg.private_tmp {
        setup_private_tmp(service_name)?;
    }

    if cfg.protect_system {
        readonly_bind("/usr")?;
        readonly_bind("/boot")?;
    }

    if cfg.private_devices {
        minimal_devtmpfs()?;
    }

    Ok(())
}

fn setup_private_tmp(service_name: &str) -> Result<(), String> {
    let base = format!("/tmp/forge-private/{service_name}");
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let tmp = format!("{base}/tmp");
    fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;

    let c_target = CString::new("/tmp").map_err(|e| e.to_string())?;
    let c_src = CString::new(tmp.as_str()).map_err(|e| e.to_string())?;
    let rc = unsafe {
        libc::mount(
            c_src.as_ptr(),
            c_target.as_ptr(),
            ptr::null(),
            libc::MS_BIND,
            ptr::null(),
        )
    };
    if rc != 0 {
        ghosttype_log(
            "SANDBOX",
            &format!(
                "PrivateTmp bind for '{service_name}' skipped: {}",
                std::io::Error::last_os_error()
            ),
        );
    }
    Ok(())
}

fn readonly_bind(path: &str) -> Result<(), String> {
    if !Path::new(path).exists() {
        return Ok(());
    }
    let c_target = CString::new(path).map_err(|e| e.to_string())?;
    let rc = unsafe {
        libc::mount(
            c_target.as_ptr(),
            c_target.as_ptr(),
            ptr::null(),
            libc::MS_BIND | libc::MS_REMOUNT,
            CString::new("ro").unwrap().as_ptr() as *const libc::c_void,
        )
    };
    if rc != 0 {
        ghosttype_log(
            "SANDBOX",
            &format!(
                "ProtectSystem {path} skipped: {}",
                std::io::Error::last_os_error()
            ),
        );
    }
    Ok(())
}

fn minimal_devtmpfs() -> Result<(), String> {
    let c_target = CString::new("/dev").map_err(|e| e.to_string())?;
    let _ = unsafe { libc::umount2(c_target.as_ptr(), libc::MNT_DETACH) };
    let c_fstype = CString::new("devtmpfs").map_err(|e| e.to_string())?;
    let rc = unsafe {
        libc::mount(
            ptr::null(),
            c_target.as_ptr(),
            c_fstype.as_ptr(),
            libc::MS_NOSUID | libc::MS_STRICTATIME,
            ptr::null(),
        )
    };
    if rc != 0 {
        ghosttype_log(
            "SANDBOX",
            &format!(
                "PrivateDevices /dev mount skipped: {}",
                std::io::Error::last_os_error()
            ),
        );
    }
    Ok(())
}
