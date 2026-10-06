//! The Swift-facing half of `feedback`: free functions, nothing of the engine lock. Every one
//! converts a type and delegates, so Android finds the same behaviour in `feedback.rs`.

use std::path::Path;

/// The app stopped while it was on screen last time.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FeedbackUnexpectedClose {
    pub build: String,
    /// The game that was running, empty for none.
    pub game: String,
    /// The save being loaded at the time, empty for none.
    pub loading: String,
    pub started: String,
}

/// One report as the tester filled it in.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FeedbackReportRecord {
    pub kind: String,
    pub message: String,
    pub tester: String,
    pub game: String,
    pub system: String,
    pub rating: String,
    pub issues: Vec<String>,
    pub details: Vec<String>,
}

/// The message to send.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FeedbackMessage {
    pub subject: String,
    pub body: String,
}

/// Starts this session's activity log in `dir`; the last session's becomes the previous one.
#[uniffi::export]
pub fn feedback_log_open(dir: String) {
    crate::feedback::log_open(Path::new(&dir));
}

/// One line of the status text, with the time it showed.
#[uniffi::export]
pub fn feedback_log_add(stamp: String, line: String) {
    crate::feedback::log_add(&stamp, &line);
}

/// This session's log, the newest `limit` lines.
#[uniffi::export]
pub fn feedback_log_text(limit: u32) -> String {
    crate::feedback::log_text(limit as usize)
}

/// The last session's log, the newest `limit` lines.
#[uniffi::export]
pub fn feedback_log_previous_text(limit: u32) -> String {
    crate::feedback::log_previous_text(limit as usize)
}

/// Called once at launch. Returns how the last session ended, when it ended unexpectedly.
#[uniffi::export]
pub fn feedback_session_open(
    dir: String,
    build: String,
    started: String,
) -> Option<FeedbackUnexpectedClose> {
    crate::feedback::session_open(Path::new(&dir), &build, &started).map(|v| {
        FeedbackUnexpectedClose {
            build: v.build,
            game: v.game,
            loading: v.loading,
            started: v.started,
        }
    })
}

#[uniffi::export]
pub fn feedback_session_on_screen(on_screen: bool) {
    crate::feedback::session_on_screen(on_screen);
}

/// The game now running, empty for none.
#[uniffi::export]
pub fn feedback_session_game(game: String) {
    crate::feedback::session_game(&game);
}

/// A save is about to be loaded (`auto:<game>` or `slot:<game>#<n>`).
#[uniffi::export]
pub fn feedback_load_begin(key: String) {
    crate::feedback::load_begin(&key);
}

/// The game ran on after the load.
#[uniffi::export]
pub fn feedback_load_end() {
    crate::feedback::load_end();
}

/// True once when this save was being loaded as the app closed last time.
#[uniffi::export]
pub fn feedback_load_blocked(key: String) -> bool {
    crate::feedback::load_blocked(&key)
}

/// The subject and text of one report.
#[uniffi::export]
pub fn feedback_compose(report: FeedbackReportRecord) -> FeedbackMessage {
    let (subject, body) = crate::feedback::compose(&crate::feedback::Report {
        kind: report.kind,
        message: report.message,
        tester: report.tester,
        game: report.game,
        system: report.system,
        rating: report.rating,
        issues: report.issues,
        details: report.details,
    });
    FeedbackMessage { subject, body }
}
