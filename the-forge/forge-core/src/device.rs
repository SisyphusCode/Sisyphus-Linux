use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use nix::sys::inotify::{AddWatchFlags, InitFlags, Inotify};

use crate::service::forge::{ForgeState, UnitKey};
use crate::service::ghosttype_log;

pub fn spawn_watchers(state: Arc<Mutex<ForgeState>>) {
    let devices = {
        let state = state.lock();
        state.pending_device_units()
    };

    if devices.is_empty() {
        return;
    }

    std::thread::spawn(move || device_watch_loop(state, devices));
}

fn device_watch_loop(state: Arc<Mutex<ForgeState>>, devices: Vec<(UnitKey, PathBuf, String)>) {
    let inotify = match Inotify::init(InitFlags::IN_NONBLOCK) {
        Ok(i) => i,
        Err(e) => {
            ghosttype_log("DEVICE", &format!("inotify init failed: {e}"));
            return;
        }
    };

    for (key, path, _service) in &devices {
        if path.exists() {
            let mut state = state.lock();
            let _ = state.activate_device_by_key(*key);
            continue;
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
            let _ = inotify.add_watch(
                parent,
                AddWatchFlags::IN_CREATE | AddWatchFlags::IN_MOVED_TO,
            );
        }
    }

    ghosttype_log("DEVICE", "Device watcher online (inotify on /dev)");

    loop {
        let _ = inotify.read_events();
        let pending = {
            let state = state.lock();
            let p = state.pending_device_units();
            if p.is_empty() {
                ghosttype_log(
                    "DEVICE",
                    "All watched devices online — exiting watcher thread",
                );
                break;
            }
            p
        };

        for (key, path, _) in pending {
            if path.exists() {
                let mut state = state.lock();
                let _ = state.activate_device_by_key(key);
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
