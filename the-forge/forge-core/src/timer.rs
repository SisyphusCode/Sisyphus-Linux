use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::service::forge::ForgeState;
use crate::service::ghosttype_log;

pub fn spawn_scheduler(state: Arc<Mutex<ForgeState>>) {
    let jobs = {
        let state = state.lock();
        state.pending_timer_jobs()
    };
    if jobs.is_empty() {
        return;
    }
    std::thread::spawn(move || timer_loop(state, jobs));
}

fn timer_loop(state: Arc<Mutex<ForgeState>>, mut jobs: Vec<(String, Instant)>) {
    ghosttype_log(
        "TIMER",
        &format!("Scheduler online — {} job(s)", jobs.len()),
    );
    loop {
        let now = Instant::now();
        let mut fired = Vec::new();
        jobs.retain(|(unit, fire_at)| {
            if *fire_at <= now {
                fired.push(unit.clone());
                false
            } else {
                true
            }
        });

        for unit in fired {
            ghosttype_log("TIMER", &format!("Firing timer for '{unit}'"));
            let mut state = state.lock();
            if let Some(timer_name) = state.timer_for_service(&unit) {
                state.mark_timer_fired(&timer_name);
            }
            if let Err(e) = state.start_service_by_name(&unit) {
                ghosttype_log("TIMER", &format!("Timer start of '{unit}' failed: {e}"));
            }
        }

        if jobs.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
