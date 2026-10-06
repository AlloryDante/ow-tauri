//! The quit sequence and the crash-restart rule (CONTRACT A.6), as pure
//! state machines. The host turns their [`QuitAction`]s into host messages,
//! window closes and the final exit.
//!
//! ```
//! use tauri_plugin_overwolf::lifecycle::{QuitAction, QuitSequence};
//! let mut ids = 0_u64;
//! let mut q = QuitSequence::default();
//! let QuitAction::Lifecycle { request_id, .. } = q.begin(0, 0, &mut ids).unwrap() else { panic!() };
//! // No windows open: before-quit, then will-quit, then finish.
//! let a = q.answer_lifecycle(request_id, false, 10, &[], &mut ids);
//! let QuitAction::Lifecycle { event: "will-quit", request_id } = a[0] else { panic!() };
//! assert_eq!(q.answer_lifecycle(request_id, false, 20, &[], &mut ids), vec![QuitAction::Finish { exit_code: 0 }]);
//! ```

use std::collections::BTreeMap;

/// How long each quit step waits for an answer.
pub const STEP_TIMEOUT_MS: u64 = 5000;
/// How long a window close request waits for `window_close_reply`.
pub const CLOSE_TIMEOUT_MS: u64 = 5000;
/// The crash window of `main.crashRestartLimit`.
pub const CRASH_WINDOW_MS: u64 = 60_000;

/// What the host must do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuitAction {
    /// Send `lifecycle { event, requestId }` to `ow-main`.
    Lifecycle {
        /// `before-quit` or `will-quit`.
        event: &'static str,
        /// Answer id.
        request_id: u64,
    },
    /// Send `window { id, event: 'close', requestId }` for each pair.
    CloseWindows(Vec<(u32, u64)>),
    /// Destroy window `id` (its close was not prevented, or timed out).
    DestroyWindow(u32),
    /// The quit was cancelled by a `preventDefault()`.
    Cancelled,
    /// Steps 1 to 4 are done: drain analytics, send `quit`, install a
    /// pending update, exit with this code.
    Finish {
        /// Exit code.
        exit_code: i32,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum Step {
    #[default]
    Idle,
    BeforeQuit {
        request_id: u64,
        deadline: u64,
    },
    Closing {
        pending: BTreeMap<u64, u32>,
        deadline: u64,
    },
    WillQuit {
        request_id: u64,
        deadline: u64,
    },
    Done,
}

/// The A.6 quit sequence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuitSequence {
    step: Step,
    exit_code: i32,
}

fn alloc(ids: &mut u64) -> u64 {
    *ids += 1;
    *ids
}

impl QuitSequence {
    /// Whether a quit is in progress (or finished).
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.step != Step::Idle
    }

    /// Whether `request_id` is a window close request of this sequence.
    #[must_use]
    pub fn owns_close(&self, request_id: u64) -> bool {
        matches!(&self.step, Step::Closing { pending, .. } if pending.contains_key(&request_id))
    }

    /// Step 1: starts the sequence. `None` when one is already running.
    pub fn begin(&mut self, exit_code: i32, now: u64, ids: &mut u64) -> Option<QuitAction> {
        if self.is_running() {
            return None;
        }
        let request_id = alloc(ids);
        self.exit_code = exit_code;
        self.step = Step::BeforeQuit {
            request_id,
            deadline: now + STEP_TIMEOUT_MS,
        };
        Some(QuitAction::Lifecycle {
            event: "before-quit",
            request_id,
        })
    }

    fn after_before_quit(&mut self, now: u64, windows: &[u32], ids: &mut u64) -> Vec<QuitAction> {
        if windows.is_empty() {
            return vec![self.enter_will_quit(now, ids)];
        }
        let pending: BTreeMap<u64, u32> = windows.iter().map(|w| (alloc(ids), *w)).collect();
        let list = pending.iter().map(|(r, w)| (*w, *r)).collect();
        self.step = Step::Closing {
            pending,
            deadline: now + CLOSE_TIMEOUT_MS,
        };
        vec![QuitAction::CloseWindows(list)]
    }

    fn enter_will_quit(&mut self, now: u64, ids: &mut u64) -> QuitAction {
        let request_id = alloc(ids);
        self.step = Step::WillQuit {
            request_id,
            deadline: now + STEP_TIMEOUT_MS,
        };
        QuitAction::Lifecycle {
            event: "will-quit",
            request_id,
        }
    }

    fn finish(&mut self) -> QuitAction {
        self.step = Step::Done;
        QuitAction::Finish {
            exit_code: self.exit_code,
        }
    }

    /// `app_quit_reply`. Returns no action for an unknown `request_id`.
    pub fn answer_lifecycle(
        &mut self,
        request_id: u64,
        prevent: bool,
        now: u64,
        windows: &[u32],
        ids: &mut u64,
    ) -> Vec<QuitAction> {
        match self.step {
            Step::BeforeQuit { request_id: r, .. } if r == request_id => {
                if prevent {
                    self.step = Step::Idle;
                    vec![QuitAction::Cancelled]
                } else {
                    self.after_before_quit(now, windows, ids)
                }
            }
            Step::WillQuit { request_id: r, .. } if r == request_id => {
                if prevent {
                    self.step = Step::Idle;
                    vec![QuitAction::Cancelled]
                } else {
                    vec![self.finish()]
                }
            }
            _ => Vec::new(),
        }
    }

    /// Whether `request_id` is a pending lifecycle request.
    #[must_use]
    pub fn owns_lifecycle(&self, request_id: u64) -> bool {
        matches!(self.step, Step::BeforeQuit { request_id: r, .. } | Step::WillQuit { request_id: r, .. } if r == request_id)
    }

    /// `window_close_reply` for a close this sequence requested.
    pub fn answer_close(
        &mut self,
        request_id: u64,
        prevent: bool,
        now: u64,
        ids: &mut u64,
    ) -> Vec<QuitAction> {
        let Step::Closing { pending, .. } = &mut self.step else {
            return Vec::new();
        };
        let Some(window) = pending.remove(&request_id) else {
            return Vec::new();
        };
        if prevent {
            self.step = Step::Idle;
            return vec![QuitAction::Cancelled];
        }
        let mut out = vec![QuitAction::DestroyWindow(window)];
        if pending.is_empty() {
            out.push(self.enter_will_quit(now, ids));
        }
        out
    }

    /// A window went away during step 3 (closed by other means).
    pub fn window_gone(&mut self, window: u32, now: u64, ids: &mut u64) -> Vec<QuitAction> {
        let Step::Closing { pending, .. } = &mut self.step else {
            return Vec::new();
        };
        pending.retain(|_, w| *w != window);
        if pending.is_empty() {
            vec![self.enter_will_quit(now, ids)]
        } else {
            Vec::new()
        }
    }

    /// Timeouts: a step without an answer proceeds as not prevented.
    pub fn tick(&mut self, now: u64, windows: &[u32], ids: &mut u64) -> Vec<QuitAction> {
        match &self.step {
            Step::BeforeQuit { deadline, .. } if now >= *deadline => {
                self.after_before_quit(now, windows, ids)
            }
            Step::Closing { pending, deadline } if now >= *deadline => {
                let mut out: Vec<QuitAction> = pending
                    .values()
                    .map(|w| QuitAction::DestroyWindow(*w))
                    .collect();
                out.push(self.enter_will_quit(now, ids));
                out
            }
            Step::WillQuit { deadline, .. } if now >= *deadline => vec![self.finish()],
            _ => Vec::new(),
        }
    }
}

/// What to do after an `ow-main` crash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashDecision {
    /// Relaunch with the original arguments.
    Relaunch,
    /// Exit with code 1: the limit was reached.
    Exit,
}

/// Records a crash at `now` (Unix ms) into `history` (kept to the last 60 s)
/// and decides (A.6): the `limit`-th crash within 60 s exits.
///
/// ```
/// use tauri_plugin_overwolf::lifecycle::{record_crash, CrashDecision};
/// let mut h = Vec::new();
/// assert_eq!(record_crash(&mut h, 1_000, 3), CrashDecision::Relaunch);
/// assert_eq!(record_crash(&mut h, 2_000, 3), CrashDecision::Relaunch);
/// assert_eq!(record_crash(&mut h, 3_000, 3), CrashDecision::Exit);
/// let mut later = vec![1_000, 2_000];
/// assert_eq!(record_crash(&mut later, 70_000, 3), CrashDecision::Relaunch);
/// ```
pub fn record_crash(history: &mut Vec<u64>, now: u64, limit: u32) -> CrashDecision {
    history.retain(|t| *t <= now && now - *t < CRASH_WINDOW_MS);
    history.push(now);
    if history.len() >= limit.max(1) as usize {
        CrashDecision::Exit
    } else {
        CrashDecision::Relaunch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_sequence_with_windows() {
        let mut ids = 100;
        let mut q = QuitSequence::default();
        let QuitAction::Lifecycle { request_id, .. } = q.begin(3, 0, &mut ids).unwrap() else {
            panic!()
        };
        assert!(q.begin(3, 0, &mut ids).is_none(), "one sequence at a time");
        let actions = q.answer_lifecycle(request_id, false, 0, &[1, 2], &mut ids);
        let QuitAction::CloseWindows(list) = &actions[0] else {
            panic!()
        };
        assert_eq!(list.len(), 2);
        assert!(q.owns_close(list[0].1));
        assert_eq!(
            q.answer_close(list[0].1, false, 0, &mut ids),
            vec![QuitAction::DestroyWindow(list[0].0)]
        );
        let a = q.answer_close(list[1].1, false, 0, &mut ids);
        assert_eq!(a[0], QuitAction::DestroyWindow(list[1].0));
        let QuitAction::Lifecycle { event, request_id } = a[1] else {
            panic!()
        };
        assert_eq!(event, "will-quit");
        assert_eq!(
            q.answer_lifecycle(request_id, false, 0, &[], &mut ids),
            vec![QuitAction::Finish { exit_code: 3 }]
        );
    }

    #[test]
    fn prevent_cancels_at_each_step() {
        let mut ids = 0;
        let mut q = QuitSequence::default();
        let QuitAction::Lifecycle { request_id, .. } = q.begin(0, 0, &mut ids).unwrap() else {
            panic!()
        };
        assert_eq!(
            q.answer_lifecycle(request_id, true, 0, &[], &mut ids),
            vec![QuitAction::Cancelled]
        );
        assert!(!q.is_running());

        let QuitAction::Lifecycle { request_id, .. } = q.begin(0, 0, &mut ids).unwrap() else {
            panic!()
        };
        let a = q.answer_lifecycle(request_id, false, 0, &[7], &mut ids);
        let QuitAction::CloseWindows(list) = &a[0] else {
            panic!()
        };
        assert_eq!(
            q.answer_close(list[0].1, true, 0, &mut ids),
            vec![QuitAction::Cancelled]
        );
        assert!(!q.is_running());

        let QuitAction::Lifecycle { request_id, .. } = q.begin(0, 0, &mut ids).unwrap() else {
            panic!()
        };
        let a = q.answer_lifecycle(request_id, false, 0, &[], &mut ids);
        let QuitAction::Lifecycle { request_id, .. } = a[0] else {
            panic!()
        };
        assert_eq!(
            q.answer_lifecycle(request_id, true, 0, &[], &mut ids),
            vec![QuitAction::Cancelled]
        );
        // Unknown ids do nothing.
        assert!(q.answer_lifecycle(999, false, 0, &[], &mut ids).is_empty());
    }

    #[test]
    fn timeouts_proceed() {
        let mut ids = 0;
        let mut q = QuitSequence::default();
        q.begin(0, 0, &mut ids).unwrap();
        assert!(q.tick(STEP_TIMEOUT_MS - 1, &[1], &mut ids).is_empty());
        let a = q.tick(STEP_TIMEOUT_MS, &[1], &mut ids);
        assert!(matches!(a[0], QuitAction::CloseWindows(_)));
        let a = q.tick(STEP_TIMEOUT_MS + CLOSE_TIMEOUT_MS, &[1], &mut ids);
        assert_eq!(a[0], QuitAction::DestroyWindow(1));
        assert!(matches!(
            a[1],
            QuitAction::Lifecycle {
                event: "will-quit",
                ..
            }
        ));
        let a = q.tick(3 * STEP_TIMEOUT_MS, &[], &mut ids);
        assert_eq!(a, vec![QuitAction::Finish { exit_code: 0 }]);
    }

    #[test]
    fn window_gone_during_close() {
        let mut ids = 0;
        let mut q = QuitSequence::default();
        let QuitAction::Lifecycle { request_id, .. } = q.begin(0, 0, &mut ids).unwrap() else {
            panic!()
        };
        q.answer_lifecycle(request_id, false, 0, &[4], &mut ids);
        let a = q.window_gone(4, 0, &mut ids);
        assert!(matches!(
            a[0],
            QuitAction::Lifecycle {
                event: "will-quit",
                ..
            }
        ));
    }

    #[test]
    fn crash_limit() {
        let mut h = vec![0, 59_999];
        assert_eq!(
            record_crash(&mut h, 60_000, 3),
            CrashDecision::Relaunch,
            "the crash at 0 is outside 60 s"
        );
        assert_eq!(record_crash(&mut Vec::new(), 5, 1), CrashDecision::Exit);
        assert_eq!(record_crash(&mut Vec::new(), 5, 0), CrashDecision::Exit);
    }
}
