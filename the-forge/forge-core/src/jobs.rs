use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

static JOB_COUNTER: AtomicU32 = AtomicU32::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JobResult {
    Done,
    Failed,
    Canceled,
    Timeout,
    Dependency,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JobState {
    Waiting,
    Running,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: u32,
    pub unit: String,
    pub mode: String,
    pub state: JobState,
    pub result: Option<JobResult>,
    pub created_ms: u128,
}

impl Job {
    pub fn new(unit: impl Into<String>, mode: impl Into<String>) -> Self {
        Self {
            id: JOB_COUNTER.fetch_add(1, Ordering::Relaxed),
            unit: unit.into(),
            mode: mode.into(),
            state: JobState::Waiting,
            result: None,
            created_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0),
        }
    }

    pub fn object_path(&self) -> String {
        format!("/org/freedesktop/systemd1/job/{}", self.id)
    }

    pub fn complete(&mut self, result: JobResult) {
        self.state = JobState::Done;
        self.result = Some(result);
    }
}

#[derive(Debug, Default)]
pub struct JobQueue {
    active: VecDeque<Job>,
    history: VecDeque<Job>,
    max_history: usize,
}

impl JobQueue {
    pub fn new() -> Self {
        Self {
            active: VecDeque::new(),
            history: VecDeque::new(),
            max_history: 256,
        }
    }

    pub fn enqueue(&mut self, unit: &str, mode: &str) -> Job {
        let job = Job::new(unit, mode);
        self.active.push_back(job.clone());
        job
    }

    pub fn finish(&mut self, id: u32, result: JobResult) -> Option<Job> {
        if let Some(pos) = self.active.iter().position(|j| j.id == id) {
            let mut job = self.active.remove(pos).unwrap();
            job.complete(result);
            self.history.push_back(job.clone());
            while self.history.len() > self.max_history {
                self.history.pop_front();
            }
            return Some(job);
        }
        None
    }

    #[allow(dead_code)]
    pub fn active_jobs(&self) -> impl Iterator<Item = &Job> {
        self.active.iter()
    }

    #[allow(dead_code)]
    pub fn recent(&self, n: usize) -> Vec<&Job> {
        self.history.iter().rev().take(n).collect()
    }
}
