use libc::{mount, MS_NODEV, MS_NOEXEC, MS_NOSUID, MS_REMOUNT, MS_STRICTATIME};
use std::ffi::CString;
use std::fs;
use std::io::Error;
use std::path::Path;
use std::ptr;

const API_FS_FLAGS: libc::c_ulong = MS_NOSUID | MS_NOEXEC | MS_NODEV;
const DEV_FS_FLAGS: libc::c_ulong = MS_NOSUID | MS_STRICTATIME;

/// Remount `/` read-write. GRUB/cmdline often passes `ro`; systemd normally flips this
/// very early — forge must do the same before service logs or unit scripts can write.
pub fn remount_root_readwrite() -> Result<(), Error> {
    let c_target = CString::new("/").unwrap();
    let c_data = CString::new("rw").unwrap();
    let result = unsafe {
        mount(
            ptr::null(),
            c_target.as_ptr(),
            ptr::null(),
            MS_REMOUNT,
            c_data.as_ptr() as *const libc::c_void,
        )
    };
    if result != 0 {
        return Err(Error::last_os_error());
    }
    crate::service::ghosttype_log("VFS", "Root filesystem remounted read-write");
    Ok(())
}

/// Mounts the essential kernel API filesystems required for a Linux environment.
pub fn mount_essential_filesystems() -> Result<(), Error> {
    if let Err(e) = remount_root_readwrite() {
        crate::service::ghosttype_log("WARN", &format!("Failed to remount / read-write: {e}"));
    }

    relabel_early_etc_files();

    // Basic mounts (4-tuple: source, target, fstype, flags)
    let basic_mounts = [
        ("proc", "/proc", "proc", API_FS_FLAGS),
        ("sysfs", "/sys", "sysfs", API_FS_FLAGS),
        ("devtmpfs", "/dev", "devtmpfs", DEV_FS_FLAGS),
        ("tmpfs", "/run", "tmpfs", MS_NOSUID | MS_NODEV),
        (
            "tmpfs",
            "/dev/shm",
            "tmpfs",
            MS_NOSUID | MS_NODEV | MS_STRICTATIME,
        ),
        ("tmpfs", "/tmp", "tmpfs", MS_NOSUID | MS_NODEV),
    ];

    for (src, target, fs_type, flags) in basic_mounts.iter() {
        let _ = fs::create_dir_all(target);

        let c_src = CString::new(*src).unwrap();
        let c_target = CString::new(*target).unwrap();
        let c_fs_type = CString::new(*fs_type).unwrap();

        let result = unsafe {
            mount(
                c_src.as_ptr(),
                c_target.as_ptr(),
                c_fs_type.as_ptr(),
                *flags,
                ptr::null(),
            )
        };

        if result != 0 {
            let err = Error::last_os_error();
            if err.raw_os_error() != Some(libc::EBUSY) {
                crate::service::ghosttype_log(
                    "WARN",
                    &format!("Failed to mount {} on {}: {}", fs_type, target, err),
                );
            }
        }
    }

    // devpts needs data options for ptmx and permissions (critical for getty, ssh, X11, Wayland, DMs)
    {
        let target = "/dev/pts";
        let _ = fs::create_dir_all(target);
        let data = CString::new("newinstance,ptmxmode=0666,mode=620,gid=5").unwrap();
        let c_src = CString::new("devpts").unwrap();
        let c_target = CString::new(target).unwrap();
        let c_fstype = CString::new("devpts").unwrap();

        let result = unsafe {
            mount(
                c_src.as_ptr(),
                c_target.as_ptr(),
                c_fstype.as_ptr(),
                MS_NOSUID | MS_NOEXEC,
                data.as_ptr() as *const libc::c_void,
            )
        };
        if result != 0 {
            let err = Error::last_os_error();
            if err.raw_os_error() != Some(libc::EBUSY) {
                crate::service::ghosttype_log(
                    "WARN",
                    &format!("Failed to mount devpts on /dev/pts: {}", err),
                );
            }
        }
        // Ensure /dev/ptmx targets devpts; sudo/login need this for PTY allocation.
        if std::path::Path::new("/dev/pts/ptmx").exists() {
            let _ = std::fs::remove_file("/dev/ptmx");
            let _ = std::os::unix::fs::symlink("/dev/pts/ptmx", "/dev/ptmx");
        }
    }

    // selinuxfs (needed for runcon before dbus on enforcing CIQ/Rocky)
    {
        let target = "/sys/fs/selinux";
        let _ = fs::create_dir_all(target);
        let c_target = CString::new(target).unwrap();
        let c_fstype = CString::new("selinuxfs").unwrap();
        let result = unsafe {
            mount(
                ptr::null(),
                c_target.as_ptr(),
                c_fstype.as_ptr(),
                0,
                ptr::null(),
            )
        };
        if result != 0 {
            let err = Error::last_os_error();
            if err.raw_os_error() != Some(libc::EBUSY) {
                crate::service::ghosttype_log("WARN", &format!("Failed to mount selinuxfs: {err}"));
            }
        }
    }

    let policy_loaded = ensure_selinux_policy();
    if policy_loaded {
        if !crate::selinux::transition_to_init_domain() {
            check_and_reexec_selinux();
        }
    }

    setup_runtime_directories();
    ensure_machine_id();
    ensure_hostname();
    load_graphics_modules();

    kill_stale_initramfs_daemons();
    ensure_console_devices();
    mount_cgroup2();
    Ok(())
}

/// Runtime dirs matching logind (systemd-logind or elogind) RuntimeDirectory= and desktop stack expectations.
fn setup_runtime_directories() {
    for d in [
        "/run/dbus",
        "/run/lock",
        "/run/user",
        "/run/gdm",
        "/run/log",
        "/run/forge",
        "/run/forge/log",
        "/run/forge/notify",
        "/run/systemd/seats",
        "/run/systemd/sessions",
        "/run/systemd/users",
        "/run/systemd/inhibit",
        "/run/systemd/ask-password",
        "/run/systemd/shutdown",
        "/run/systemd/machines",
        "/run/NetworkManager",
        "/run/udev",
        "/run/nvidia-persistenced",
        "/run/systemd/journal",
        "/run/systemd/resolve",
        "/tmp/.X11-unix",
        "/var/tmp",
        "/var/log/forge",
        "/var/lib/forge",
        "/var/lib/dbus",
        "/var/lib/NetworkManager",
        "/var/lib/gdm",
        "/var/log/gdm",
    ] {
        let _ = fs::create_dir_all(d);
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions("/run/lock", fs::Permissions::from_mode(0o1777));
        let _ = fs::set_permissions("/run/user", fs::Permissions::from_mode(0o755));
        let _ = fs::set_permissions("/var/tmp", fs::Permissions::from_mode(0o1777));
        let _ = fs::set_permissions("/tmp/.X11-unix", fs::Permissions::from_mode(0o1777));
        let _ = fs::set_permissions("/run/dbus", fs::Permissions::from_mode(0o755));
        let _ = fs::set_permissions("/run/gdm", fs::Permissions::from_mode(0o711));
    }

    crate::selinux::relabel_runtime_trees();
}

/// systemd machine-id-setup: dbus and logind read /etc/machine-id early.
fn ensure_machine_id() {
    use std::process::{Command, Stdio};
    let etc_id = Path::new("/etc/machine-id");
    let dbus_id = Path::new("/var/lib/dbus/machine-id");

    if !etc_id.exists() {
        if dbus_id.exists() {
            let _ = std::os::unix::fs::symlink("../var/lib/dbus/machine-id", etc_id);
        } else if Path::new("/usr/bin/systemd-machine-id-setup").is_file() {
            let _ = Command::new("/usr/bin/systemd-machine-id-setup")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        } else if Path::new("/usr/bin/dbus-uuidgen").is_file() {
            let _ = Command::new("/usr/bin/dbus-uuidgen")
                .arg("--ensure=/etc/machine-id")
                .status();
        }
    }

    if !dbus_id.exists() && etc_id.exists() {
        let _ = fs::copy(etc_id, dbus_id);
    }
}

/// systemd hostname-setup: NetworkManager and GDM read /etc/hostname.
fn ensure_hostname() {
    if Path::new("/etc/hostname").exists() {
        return;
    }
    if let Ok(h) = std::fs::read_to_string("/proc/sys/kernel/hostname") {
        let host = h.trim();
        if !host.is_empty() && host != "(none)" {
            let _ = fs::write("/etc/hostname", format!("{host}\n"));
            crate::selinux::relabel_paths(&["/etc/hostname"]);
        }
    }
}

/// logind (systemd-logind/elogind) Wants=modprobe@drm.service — load DRM before seat0.
fn load_graphics_modules() {
    use std::process::{Command, Stdio};

    for module in [
        "i8042",
        "atkbd",
        "usbhid",
        "drm",
        "nvidia",
        "nvidia_drm",
        "nvidia_modeset",
    ] {
        let _ = Command::new("/usr/sbin/modprobe")
            .arg(module)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// Initramfs or partial boots may leave logind/udevd running before /run layout exists.
fn kill_stale_initramfs_daemons() {
    if std::env::var("FORGE_MOCK_BOOT").is_ok() {
        return;
    }
    use std::process::{Command, Stdio};

    for comm in [
        "dbus-daemon",
        "dbus-broker",
        "systemd-logind",
        "elogind",
        "systemd-udevd",
        "udevd",
    ] {
        let _ = Command::new("pkill")
            .args(["-9", comm])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// systemd normally loads SELinux policy very early; forge must do the same before runcon/dbus.
fn ensure_selinux_policy() -> bool {
    use std::process::{Command, Stdio};

    if !std::path::Path::new("/sys/fs/selinux").is_dir() {
        return false;
    }

    let load_policy = if std::path::Path::new("/usr/sbin/load_policy").is_file() {
        "/usr/sbin/load_policy"
    } else if std::path::Path::new("/sbin/load_policy").is_file() {
        "/sbin/load_policy"
    } else {
        return false;
    };

    let run = |args: &[&str]| {
        Command::new(load_policy)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
    };

    let mut policy_loaded = false;
    if run(&["-q"]).is_ok_and(|s| s.success()) {
        crate::service::ghosttype_log("VFS", "SELinux policy loaded");
        policy_loaded = true;
    } else if run(&["-q", "-i"]).is_ok_and(|s| s.success()) {
        crate::service::ghosttype_log("VFS", "SELinux initial policy loaded");
        policy_loaded = true;
    }

    if policy_loaded {
        let is_enforcing = crate::boot_debug::selinux_enforcing();
        if let Err(e) = std::fs::write(
            "/sys/fs/selinux/enforce",
            if is_enforcing { "1" } else { "0" },
        ) {
            crate::service::ghosttype_log(
                "WARN",
                &format!("Failed to set SELinux enforcing={is_enforcing}: {e}"),
            );
        } else {
            crate::service::ghosttype_log(
                "VFS",
                &format!(
                    "SELinux enforcement set to: {}",
                    if is_enforcing {
                        "Enforcing"
                    } else {
                        "Permissive"
                    }
                ),
            );
        }
    }

    policy_loaded
}

fn check_and_reexec_selinux() {
    if std::process::id() != 1 {
        return;
    }

    let current_context = match fs::read_to_string("/proc/self/attr/current") {
        Ok(c) => c.trim().to_string(),
        Err(_) => return,
    };

    if !current_context.contains("kernel_t") {
        crate::service::ghosttype_log(
            "VFS",
            &format!(
                "SELinux context is '{}' (already transitioned), skipping re-exec",
                current_context
            ),
        );
        return;
    }

    crate::service::ghosttype_log(
        "VFS",
        &format!("Currently running in '{}' (kernel domain). Performing re-exec to transition to init_t...", current_context),
    );

    let exe_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            crate::service::ghosttype_log(
                "WARN",
                &format!("Failed to get current exe path for re-exec: {e}"),
            );
            return;
        }
    };

    use std::os::unix::process::CommandExt;
    let args: Vec<String> = std::env::args().collect();
    let err = std::process::Command::new(&exe_path)
        .args(&args[1..])
        .exec();

    crate::service::ghosttype_log(
        "WARN",
        &format!("Failed to exec {}: {}", exe_path.display(), err),
    );
}

/// NetworkManager and initramfs can leave /etc files unlabeled before enforcing daemons start.
fn relabel_early_etc_files() {
    use std::process::{Command, Stdio};

    if std::path::Path::new("/usr/libexec/forge/restorecon-forge.sh").is_file() {
        let _ = Command::new("/usr/libexec/forge/restorecon-forge.sh")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        return;
    }

    if !std::path::Path::new("/sbin/restorecon").is_file()
        && !std::path::Path::new("/usr/sbin/restorecon").is_file()
    {
        return;
    }

    let restorecon = if std::path::Path::new("/sbin/restorecon").is_file() {
        "/sbin/restorecon"
    } else {
        "/usr/sbin/restorecon"
    };

    for path in [
        "/etc/resolv.conf",
        "/etc/hostname",
        "/etc/hosts",
        "/etc/machine-id",
    ] {
        if std::path::Path::new(path).exists() {
            let _ = Command::new(restorecon)
                .args(["-F", path])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

/// Fallback character devices when devtmpfs has not yet created VT nodes (getty exit 8).
fn ensure_console_devices() {
    let nodes: [(&str, libc::dev_t, libc::mode_t); 6] = [
        ("/dev/console", libc::makedev(5, 1), 0o622),
        ("/dev/tty0", libc::makedev(4, 0), 0o622),
        ("/dev/tty1", libc::makedev(4, 1), 0o622),
        ("/dev/tty2", libc::makedev(4, 2), 0o622),
        ("/dev/tty3", libc::makedev(4, 3), 0o622),
        ("/dev/null", libc::makedev(1, 3), 0o666),
    ];

    for (path, dev, mode) in nodes {
        if fs::metadata(path).is_ok() {
            continue;
        }
        let c_path = match CString::new(path) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let rc = unsafe { libc::mknod(c_path.as_ptr(), mode, dev) };
        if rc != 0 {
            let err = Error::last_os_error();
            if err.raw_os_error() != Some(libc::EEXIST) {
                crate::service::ghosttype_log("WARN", &format!("mknod {path}: {err}"));
            }
        }
    }
}

fn mount_cgroup2() {
    let target = "/sys/fs/cgroup";
    if fs::read_to_string(format!("{target}/cgroup.controllers")).is_ok() {
        return;
    }

    let _ = fs::create_dir_all(target);
    let c_target = CString::new(target).unwrap();
    let c_fstype = CString::new("cgroup2").unwrap();
    // systemd mount-setup.c: nsdelegate for user/session scopes.
    let data = CString::new("nsdelegate").unwrap();
    let result = unsafe {
        mount(
            c_fstype.as_ptr(),
            c_target.as_ptr(),
            c_fstype.as_ptr(),
            0,
            data.as_ptr() as *const libc::c_void,
        )
    };

    if result != 0 {
        let err = Error::last_os_error();
        if err.raw_os_error() != Some(libc::EBUSY) {
            crate::service::ghosttype_log(
                "WARN",
                &format!("Failed to mount cgroup2 on {target}: {err}"),
            );
        }
    }
}
