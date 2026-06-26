use nix::sys::epoll::{Epoll, EpollCreateFlags, EpollEvent, EpollFlags, EpollTimeout};
use nix::sys::signal::{SigSet, Signal};
use nix::sys::signalfd::{SfdFlags, SignalFd};
use parking_lot::Mutex;
use std::os::unix::io::{AsRawFd, BorrowedFd};
use std::os::unix::net::UnixListener;
use std::sync::Arc;

use crate::ipc;
use crate::service::forge::{lock_state, ForgeState};
use crate::service::ghosttype_log;

const SIGNAL_TOKEN: u64 = 1;
const IPC_LISTEN_TOKEN: u64 = 2;

pub struct Reactor {
    epoll: Epoll,
    signal_fd: SignalFd,
    ipc_listener: UnixListener,
}

impl Reactor {
    pub fn new(ipc_listener: UnixListener) -> Self {
        let mut sigset = SigSet::empty();
        sigset.add(Signal::SIGCHLD);
        sigset.add(Signal::SIGTERM);
        sigset.add(Signal::SIGINT);
        sigset.add(Signal::SIGUSR1);

        let signal_fd = SignalFd::with_flags(&sigset, SfdFlags::SFD_NONBLOCK)
            .expect("Failed to create signalfd (fatal at startup)");
        let epoll = Epoll::new(EpollCreateFlags::EPOLL_CLOEXEC)
            .expect("Failed to create epoll (fatal at startup)");

        epoll
            .add(
                unsafe { BorrowedFd::borrow_raw(signal_fd.as_raw_fd()) },
                EpollEvent::new(EpollFlags::EPOLLIN, SIGNAL_TOKEN),
            )
            .expect("Failed to register signalfd (fatal at startup)");

        epoll
            .add(
                unsafe { BorrowedFd::borrow_raw(ipc_listener.as_raw_fd()) },
                EpollEvent::new(EpollFlags::EPOLLIN, IPC_LISTEN_TOKEN),
            )
            .expect("Failed to register IPC listener (fatal at startup)");

        Self {
            epoll,
            signal_fd,
            ipc_listener,
        }
    }

    pub fn run(self, state: Arc<Mutex<ForgeState>>) -> ! {
        let mut events = [EpollEvent::empty(); 32];
        ghosttype_log(
            "STEADY",
            &format!(
                "Epoll reactor online — control socket {}",
                ipc::control_socket_path()
            ),
        );

        loop {
            if lock_state(&state).is_shutting_down() {
                crate::recovery::finish_system_shutdown(crate::recovery::shutdown_action_from_env());
            }

            let n = match self.epoll.wait(&mut events, EpollTimeout::from(1000_u16)) {
                Ok(n) => n,
                Err(e) => {
                    ghosttype_log(
                        "WARN",
                        &format!("epoll_wait failed: {e} — continuing reactor loop"),
                    );
                    // Brief pause to avoid tight error loop under extreme conditions
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                }
            };

            for event in &events[..n] {
                match event.data() {
                    SIGNAL_TOKEN => self.handle_signals(&state),
                    IPC_LISTEN_TOKEN => self.handle_ipc(&state),
                    _ => {}
                }
            }
        }
    }

    fn handle_signals(&self, state: &Arc<Mutex<ForgeState>>) {
        while let Ok(Some(sig)) = self.signal_fd.read_signal() {
            let signo = sig.ssi_signo as i32;
            if signo == Signal::SIGCHLD as i32 {
                let mut locked = lock_state(state);
                locked.reap_all_pending();
                locked.process_pending_restarts();
            } else if signo == Signal::SIGTERM as i32 || signo == Signal::SIGINT as i32 {
                ghosttype_log("SIGNAL", "Received shutdown signal");
                let mut locked = lock_state(state);
                let _ = locked.shutdown();
            } else if signo == Signal::SIGUSR1 as i32 {
                let locked = lock_state(state);
                let profile = locked.boot_profile();
                ghosttype_log(
                    "PROFILE",
                    &format!(
                        "Boot {} ms / {} waves / target {}",
                        profile.total_boot_ms,
                        profile.waves.len(),
                        profile.active_target
                    ),
                );
            }
        }
    }

    fn handle_ipc(&self, state: &Arc<Mutex<ForgeState>>) {
        match self.ipc_listener.accept() {
            Ok((stream, _addr)) => ipc::handle_client(state, stream),
            Err(e) => ghosttype_log("IPC", &format!("accept failed: {e}")),
        }
    }
}

pub fn run(state: Arc<Mutex<ForgeState>>) -> ! {
    let listener = match ipc::bind_control_socket() {
        Ok(l) => l,
        Err(e) => {
            // Fatal only at the point we enter steady state; log and exit hard (init can't continue without control)
            eprintln!("FATAL: failed to bind control socket: {e}");
            std::process::exit(1);
        }
    };
    Reactor::new(listener).run(state);
}
