use parking_lot::Mutex;
use rhai::{Engine, EvalAltResult, ImmutableString};
use std::path::Path;
use std::sync::Arc;

use crate::service::forge::ForgeState;
use crate::service::{boot_elapsed_ms, ghosttype_log};

pub struct ScriptEngine {
    engine: Engine,
}

impl ScriptEngine {
    pub fn new(state: Arc<Mutex<ForgeState>>) -> Self {
        let mut engine = Engine::new();

        engine.on_print(|msg| {
            ghosttype_log("SCRIPT", &msg);
        });

        let spawn_state = Arc::clone(&state);
        engine.register_fn(
            "spawn_service",
            move |name: ImmutableString, binary: ImmutableString, args: rhai::Array| {
                let string_args: Vec<String> = args.into_iter().map(|v| v.to_string()).collect();
                let mut state = spawn_state.lock();
                match state.spawn_adhoc(&name, &binary, string_args) {
                    Ok(_pid) => {}
                    Err(e) => ghosttype_log("WARN", &format!("Failed to launch '{name}': {e}")),
                }
            },
        );

        engine.register_fn("boot_ms", boot_elapsed_ms);

        engine.register_fn(
            "log_status",
            move |status: ImmutableString, detail: ImmutableString| {
                ghosttype_log(&status, &detail);
            },
        );

        let target_state = Arc::clone(&state);
        engine.register_fn("set_target", move |name: ImmutableString| {
            let mut state = target_state.lock();
            state.active_target = name.to_string();
            ghosttype_log(
                "TARGET",
                &format!("Rhai set boot target to '{}'", state.active_target),
            );
        });

        Self { engine }
    }

    pub fn execute_script<P: AsRef<Path>>(&self, path: P) -> Result<(), Box<EvalAltResult>> {
        let script_path = path.as_ref();
        if script_path.exists() {
            let _: rhai::Dynamic = self.engine.eval_file(script_path.to_path_buf())?;
        } else {
            ghosttype_log(
                "WARN",
                &format!("Configuration script missing at: {script_path:?}"),
            );
        }
        Ok(())
    }
}
