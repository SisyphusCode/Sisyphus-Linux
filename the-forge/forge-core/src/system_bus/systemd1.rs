use parking_lot::Mutex;
use std::sync::Arc;

use zbus::blocking::Connection;
use zbus::interface;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, Value};

use crate::jobs::JobResult;
use crate::service::forge::{lock_state, ForgeState};
use crate::user_sessions;

pub fn register(conn: &Connection, state: Arc<Mutex<ForgeState>>) -> zbus::Result<()> {
    conn.object_server().at(
        "/org/freedesktop/systemd1",
        Manager {
            state: state.clone(),
        },
    )?;
    conn.object_server()
        .at("/org/freedesktop/systemd1/unit/_forge_scope", Scope)?;
    Ok(())
}

struct Manager {
    state: Arc<Mutex<ForgeState>>,
}

#[interface(name = "org.freedesktop.systemd1.Manager")]
impl Manager {
    async fn start_unit(&self, name: &str, mode: &str) -> zbus::fdo::Result<OwnedObjectPath> {
        let job = {
            let mut locked = lock_state(&self.state);
            let job = locked.enqueue_job(name, mode);
            if name.starts_with("user@") && name.ends_with(".service") {
                let _ = locked.start_user_manager_stub(name);
            } else if name == "user-sessions.service" || name == "systemd-user-sessions.service" {
                let _ = user_sessions::allow_logins();
            }
            job
        };

        let result = {
            let mut locked = lock_state(&self.state);
            let unit = name.trim_end_matches(".service");
            match locked.start_service_by_name(unit) {
                Ok(_) => JobResult::Done,
                Err(_) => JobResult::Failed,
            }
        };

        {
            let mut locked = lock_state(&self.state);
            locked.finish_job(job.id, result);
        }

        Ok(object_path(&job.object_path())?)
    }

    async fn start_transient_unit(
        &self,
        name: &str,
        mode: &str,
        properties: Vec<(String, Value<'_>)>,
        _aux: Vec<(String, Vec<(String, Value<'_>)>)>,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        if name.starts_with("session-") && name.ends_with(".scope") {
            let leader = leader_from_properties(&properties);
            let job = {
                let mut locked = lock_state(&self.state);
                locked.attach_session_scope(name, leader);
                locked.enqueue_job(name, mode)
            };
            {
                let mut locked = lock_state(&self.state);
                locked.finish_job(job.id, JobResult::Done);
            }
            return Ok(object_path(&job.object_path())?);
        }
        self.start_unit(name, mode).await
    }

    async fn stop_unit(&self, name: &str, _mode: &str) -> zbus::fdo::Result<OwnedObjectPath> {
        let job = {
            let mut locked = lock_state(&self.state);
            locked.enqueue_job(name, "replace")
        };
        let result = {
            let mut locked = lock_state(&self.state);
            let unit = name.trim_end_matches(".service");
            match locked.stop_service_by_name(unit) {
                Ok(()) => JobResult::Done,
                Err(_) => JobResult::Failed,
            }
        };
        {
            let mut locked = lock_state(&self.state);
            locked.finish_job(job.id, result);
        }
        Ok(object_path(&job.object_path())?)
    }

    async fn get_unit_by_pid(&self, _pid: u32) -> zbus::fdo::Result<OwnedObjectPath> {
        Ok(object_path("/org/freedesktop/systemd1/unit/_forge_scope")?)
    }

    async fn subscribe(&self) -> zbus::fdo::Result<()> {
        Ok(())
    }

    async fn list_units(
        &self,
    ) -> zbus::fdo::Result<
        Vec<(
            String,
            String,
            String,
            String,
            u32,
            String,
            String,
            u32,
            bool,
            bool,
        )>,
    > {
        let locked = lock_state(&self.state);
        Ok(locked.list_units_dbus())
    }

    #[zbus(property)]
    async fn environment(&self) -> zbus::fdo::Result<Vec<String>> {
        Ok(read_greeter_environment())
    }

    async fn set_environment(&self, _variables: Vec<String>) -> zbus::fdo::Result<()> {
        Ok(())
    }

    async fn unset_environment(&self, _variable: &str) -> zbus::fdo::Result<()> {
        Ok(())
    }

    async fn reset_environment(&self) -> zbus::fdo::Result<()> {
        Ok(())
    }
}

struct Scope;

#[interface(name = "org.freedesktop.systemd1.Scope")]
impl Scope {
    async fn abandon(&self) -> zbus::fdo::Result<()> {
        Ok(())
    }

    #[zbus(property)]
    async fn controller(&self) -> zbus::fdo::Result<String> {
        Ok(String::new())
    }

    #[zbus(property)]
    async fn pids(&self) -> zbus::fdo::Result<Vec<u32>> {
        Ok(Vec::new())
    }
}

fn value_to_u32(value: &Value<'_>) -> Option<u32> {
    match value {
        Value::U32(v) => Some(*v),
        Value::U64(v) => u32::try_from(*v).ok(),
        Value::I32(v) if *v >= 0 => Some(*v as u32),
        _ => None,
    }
}

fn leader_from_properties(properties: &[(String, Value<'_>)]) -> Option<u32> {
    let mut leader = None;
    let mut pids = Vec::new();
    for (key, value) in properties {
        match key.as_str() {
            "Leader" => leader = value_to_u32(value),
            "PIDs" => {
                if let Value::Array(arr) = value {
                    for item in arr.iter() {
                        if let Some(pid) = value_to_u32(item) {
                            pids.push(pid);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    leader.or_else(|| pids.first().copied())
}

fn object_path(s: &str) -> zbus::fdo::Result<OwnedObjectPath> {
    ObjectPath::try_from(s)
        .map(OwnedObjectPath::from)
        .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
}

pub fn read_greeter_environment() -> Vec<String> {
    let path = "/run/gdm/forge-greeter-environment";
    let mut entries = Vec::new();
    if let Ok(raw) = std::fs::read_to_string(path) {
        for line in raw.lines() {
            let line = line.trim();
            if !line.is_empty() && line.contains('=') && !line.starts_with('#') {
                entries.push(line.to_string());
            }
        }
    }
    if entries.is_empty() {
        entries = vec![
            "WLR_NO_HARDWARE_CURSORS=1".into(),
            "__GLX_VENDOR_LIBRARY_NAME=nvidia".into(),
            "GBM_BACKEND=nvidia-drm".into(),
        ];
    }
    entries
}
