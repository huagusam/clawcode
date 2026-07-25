use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use runtime::ConversationRuntime;

use crate::persist::{
    DEFAULT_AGENT_MAX_ITERATIONS, DEFAULT_AGENT_TIMEOUT_SECS,
};
use crate::runtime::{build_agent_runtime_inner, ProviderRuntimeClient, SubagentToolExecutor};
use crate::types::{AgentJob, AgentProgress, AgentStatus, SharedProgress, SubagentProgressEvent};

pub struct AgentHandle {
    pub agent_id: String,
    thread_handle: Option<std::thread::JoinHandle<()>>,
    rx: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    pub progress: SharedProgress,
    finished: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TryAgain;

impl AgentHandle {
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    pub fn join(mut self) -> Result<String, String> {
        let timeout = Duration::from_secs(DEFAULT_AGENT_TIMEOUT_SECS);
        let rx = match self.rx.take() {
            Some(rx) => rx,
            None => return Ok(String::new()),
        };
        let result = match rx.recv_timeout(timeout) {
            Ok(Ok(text)) => Ok(text),
            Ok(Err(e)) => Err(e),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err("agent timed out".to_string()),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                Err("agent disconnected".to_string())
            }
        };
        self.finished.store(true, Ordering::SeqCst);
        remove_progress_entry(&self.progress, &self.agent_id);
        if result.is_ok() {
            let _ = self.thread_handle.take().map(|h| h.join());
        }
        result
    }

    pub fn try_join(&mut self) -> Result<Result<String, String>, TryAgain> {
        let rx = match self.rx.as_ref() {
            Some(rx) => rx,
            None => return Ok(Ok(String::new())),
        };
        match rx.try_recv() {
            Ok(result) => {
                self.finished.store(true, Ordering::SeqCst);
                Ok(result)
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => Err(TryAgain),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.finished.store(true, Ordering::SeqCst);
                Ok(Err("agent disconnected".to_string()))
            }
        }
    }

    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::SeqCst)
    }

    #[cfg(feature = "test-utils")]
    pub fn noop(agent_id: impl Into<String>) -> Self {
        Self {
            agent_id: agent_id.into(),
            thread_handle: None,
            rx: None,
            progress: crate::types::new_shared_progress(),
            finished: Arc::new(AtomicBool::new(true)),
        }
    }

    #[cfg(feature = "test-utils")]
    pub fn with_parts(
        agent_id: impl Into<String>,
        thread_handle: std::thread::JoinHandle<()>,
        rx: std::sync::mpsc::Receiver<Result<String, String>>,
    ) -> Self {
        Self {
            agent_id: agent_id.into(),
            thread_handle: Some(thread_handle),
            rx: Some(rx),
            progress: crate::types::new_shared_progress(),
            finished: Arc::new(AtomicBool::new(false)),
        }
    }

    #[cfg(feature = "test-utils")]
    pub fn join_with_timeout(self, timeout: Duration) -> Result<String, String> {
        let rx = match self.rx {
            Some(rx) => rx,
            None => return Ok(String::new()),
        };
        let result = match rx.recv_timeout(timeout) {
            Ok(Ok(text)) => Ok(text),
            Ok(Err(e)) => Err(e),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err("agent timed out".to_string()),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                Err("agent disconnected".to_string())
            }
        };
        let _ = self.thread_handle.map(|h| h.join());
        result
    }
}

fn remove_progress_entry(shared: &SharedProgress, agent_id: &str) {
    let mut guard = shared.agents.lock().unwrap_or_else(|e| e.into_inner());
    guard.retain(|p| p.agent_id != agent_id);
}

/// Spawn an agent task on a dedicated OS thread so that the
/// `ProviderRuntimeClient::block_on()` call inside `run_agent_job`
/// does not panic with "Cannot start a runtime from within a runtime".
pub fn spawn_agent_task(job: AgentJob) -> Result<AgentHandle, String> {
    spawn_agent_task_with_progress(job, crate::types::new_shared_progress())
}

pub fn spawn_agent_task_with_progress(
    job: AgentJob,
    progress: SharedProgress,
) -> Result<AgentHandle, String> {
    let agent_id = job.manifest.agent_id.clone();
    let name = job.manifest.name.clone();
    let subagent_type = job.manifest.subagent_type.clone().unwrap_or_default();
    let finished = Arc::new(AtomicBool::new(false));
    let finished_clone = Arc::clone(&finished);

    {
        let mut guard = progress.agents.lock().unwrap_or_else(|e| e.into_inner());
        guard.push(AgentProgress {
            agent_id: agent_id.clone(),
            name: name.clone(),
            subagent_type: subagent_type.clone(),
            status: AgentStatus::Running,
            events: vec![],
            started_at: std::time::Instant::now(),
            iteration_count: 0,
            final_event: None,
            current_activity: None,
        });
    }

    let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();

    let progress_for_job = Arc::clone(&progress);
    let agent_id_for_job = agent_id.clone();
    let thread_handle = std::thread::spawn(move || {
        let job_progress = Arc::clone(&progress_for_job);
        let job_agent_id = agent_id_for_job.clone();
        let job_with_progress = AssertUnwindSafe(AgentJobWithProgress {
            job,
            progress: progress_for_job,
            agent_id: agent_id_for_job,
        });
        let result = std::panic::catch_unwind(move || {
            run_agent_job_sync_with_progress(&job_with_progress)
        });
        clear_current_activity(&job_progress, &job_agent_id);

        let outcome = match result {
            Ok(Ok(text)) => {
                push_progress_event(
                    &job_progress,
                    &job_agent_id,
                    SubagentProgressEvent::Completed {
                        result_preview: text.clone(),
                    },
                );
                push_progress_event(
                    &job_progress,
                    &job_agent_id,
                    SubagentProgressEvent::StatusChange {
                        status: AgentStatus::Completed,
                    },
                );
                Ok(text)
            }
            Ok(Err(error)) => {
                push_progress_event(
                    &job_progress,
                    &job_agent_id,
                    SubagentProgressEvent::Failed {
                        error: error.clone(),
                    },
                );
                Err(error)
            }
            Err(panic_payload) => {
                let panic_msg = panic_message(&panic_payload);
                push_progress_event(
                    &job_progress,
                    &job_agent_id,
                    SubagentProgressEvent::Failed {
                        error: format!("panic: {panic_msg}"),
                    },
                );
                Err(format!("panic: {panic_msg}"))
            }
        };
        finished_clone.store(true, Ordering::SeqCst);
        let _ = tx.send(outcome);
    });

    Ok(AgentHandle {
        agent_id,
        thread_handle: Some(thread_handle),
        rx: Some(rx),
        progress,
        finished,
    })
}

struct AgentJobWithProgress {
    job: AgentJob,
    progress: SharedProgress,
    agent_id: String,
}

fn push_progress_event(shared: &SharedProgress, agent_id: &str, event: SubagentProgressEvent) {
    crate::types::push_progress_event(shared, agent_id, event);
}

fn clear_current_activity(shared: &SharedProgress, agent_id: &str) {
    crate::types::set_current_activity(shared, agent_id, None);
}

fn run_agent_job_sync_with_progress(job: &AgentJobWithProgress) -> Result<String, String> {
    let mut runtime: ConversationRuntime<ProviderRuntimeClient, SubagentToolExecutor> =
        build_agent_runtime_inner(
            &job.job,
            Some(Arc::clone(&job.progress)),
            Some(job.agent_id.clone()),
        )?
        .with_max_iterations(DEFAULT_AGENT_MAX_ITERATIONS);
    let summary = runtime
        .run_turn(job.job.prompt.clone(), None)
        .map_err(|error| error.to_string())?;
    Ok(final_assistant_text(&summary))
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        String::from("unknown panic payload")
    }
}

fn final_assistant_text(summary: &runtime::TurnSummary) -> String {
    summary
        .assistant_messages
        .last()
        .map(|message| {
            message
                .blocks
                .iter()
                .filter_map(|block| match block {
                    runtime::ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n\n")
        })
        .unwrap_or_default()
}
