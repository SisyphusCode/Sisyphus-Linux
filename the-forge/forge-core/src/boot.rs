use nix::sys::signal::{sigprocmask, SigSet, SigmaskHow, Signal};
use nix::unistd::{getpid, Pid};

use crate::service::ghosttype_log;

pub fn setup_environment() {
    if getpid() != Pid::from_raw(1) {
        eprintln!("⚠️  [Forge-Dev]: Running in user space sandbox mode.");
        return;
    }

    // Initialize standard descriptors 0, 1, 2 to /dev/console
    unsafe {
        let fd = libc::open(
            b"/dev/console\0".as_ptr() as *const libc::c_char,
            libc::O_RDWR,
        );
        if fd >= 0 {
            libc::dup2(fd, 0);
            libc::dup2(fd, 1);
            libc::dup2(fd, 2);
            if fd > 2 {
                libc::close(fd);
            }
        }
    }

    ghosttype_log("INIT", "Asserting system identity control (PID 1)");

    unsafe {
        libc::umask(0o022);
    }

    // PID 1 inherits a minimal environment from the kernel — set PATH and D-Bus address
    // so dbus_wait, busctl, and service units can find the system bus.
    const DEFAULT_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
    if std::env::var_os("PATH").is_none_or(|p| p.is_empty()) {
        std::env::set_var("PATH", DEFAULT_PATH);
    }
    if std::env::var_os("DBUS_SYSTEM_BUS_ADDRESS").is_none() {
        std::env::set_var(
            "DBUS_SYSTEM_BUS_ADDRESS",
            "unix:path=/run/dbus/system_bus_socket",
        );
    }

    let mut sigset = SigSet::empty();
    sigset.add(Signal::SIGCHLD);
    sigset.add(Signal::SIGTERM);
    sigset.add(Signal::SIGINT);
    sigset.add(Signal::SIGUSR1);
    sigprocmask(SigmaskHow::SIG_BLOCK, Some(&sigset), None)
        .expect("Failed to block signals for signalfd reactor");
}
