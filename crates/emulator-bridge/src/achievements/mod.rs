//! RetroAchievements, through rcheevos' `rc_client`.
//!
//! ## Who does what
//!
//! - **rcheevos** (vendored C, `vendor/rcheevos`) owns the whole protocol: building API requests,
//!   parsing responses, hashing the game file, evaluating every achievement's conditions against
//!   memory each frame, and deciding when one unlocks.
//! - **This module** owns the `rc_client_t`, feeds it memory from the running core (through
//!   [`memory_map`]), calls `rc_client_do_frame` after every emulated frame (from the bridge's
//!   tick), and turns its callbacks into two queues: HTTP requests waiting to be sent, and events
//!   waiting to be shown.
//! - **The platform shell** (Swift today, Kotlin later) does the HTTP, because that is where the
//!   platform's TLS stack and network permissions live. It polls [`ServerRequest`]s, performs
//!   each one, and hands the response back by id. It also stores the login TOKEN in the platform
//!   keychain; the password is passed through once for the login request and never kept by
//!   anything here.
//!
//! ## Softcore only, on purpose
//!
//! Hardcore mode is off and stays off by default: it forbids save states, rewind, cheats and
//! slow motion, each of which is a feature of this app. rcheevos is told so at creation.

pub mod console;
pub mod memory_map;

#[cfg(feature = "native-core")]
mod client;
#[cfg(feature = "native-core")]
pub use client::Achievements;

/// An HTTP request rcheevos wants made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerRequest {
    /// Hand this back with the response.
    pub id: u64,
    pub url: String,
    /// `Some` means POST with this body; `None` means GET.
    pub post_data: Option<String>,
    pub content_type: Option<String>,
}

/// Something that happened, for the shell to show or act on.
#[derive(Debug, Clone, PartialEq)]
pub enum AchievementEvent {
    /// Store `token` in the keychain; it logs in next time without the password.
    LoginSucceeded {
        username: String,
        display_name: String,
        token: String,
        score: u32,
        score_softcore: u32,
    },
    LoginFailed {
        message: String,
    },
    GameLoaded {
        game_id: u32,
        title: String,
        hash: String,
        badge_url: String,
        total: u32,
        unlocked: u32,
        points_total: u32,
        points_unlocked: u32,
    },
    /// Includes "this game is not known to RetroAchievements", which is the common case for a
    /// hack or a bad dump and is said as such rather than as an error.
    GameLoadFailed {
        message: String,
    },
    Unlocked {
        id: u32,
        title: String,
        description: String,
        points: u32,
        badge_url: String,
    },
    /// Every achievement in the set is earned.
    GameCompleted,
    /// Progress toward a measured achievement ("12/50"), for a transient indicator.
    Progress {
        id: u32,
        title: String,
        progress: String,
    },
    LeaderboardStarted {
        title: String,
    },
    LeaderboardFailed {
        title: String,
    },
    LeaderboardSubmitted {
        title: String,
        score: String,
    },
    /// A request failed and will not be retried.
    ServerError {
        api: String,
        message: String,
    },
    /// Unlocks are queued and being retried.
    Disconnected,
    Reconnected,
}

/// One achievement of the loaded game, for the list on the game card.
#[derive(Debug, Clone, PartialEq)]
pub struct AchievementInfo {
    pub id: u32,
    pub title: String,
    pub description: String,
    pub points: u32,
    pub unlocked: bool,
    /// rc_client's progress bucket: "Locked", "Unlocked", "Recently Unlocked", "Almost There",
    /// "Active Challenges", "Unsupported", "Unsynced".
    pub bucket: String,
    pub badge_url: String,
    pub badge_locked_url: String,
    /// "12/50" for a measured achievement, empty otherwise.
    pub measured_progress: String,
    /// Seconds since 1970, 0 when locked.
    pub unlock_time: i64,
}

/// The logged-in user, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AchievementUser {
    pub username: String,
    pub display_name: String,
    pub score: u32,
    pub score_softcore: u32,
}

/// `RC_CLIENT_ACHIEVEMENT_BUCKET_*`, rc_client.h lines 537 to 545, as words.
pub fn bucket_label(bucket: u32) -> &'static str {
    match bucket {
        1 => "Locked",
        2 => "Unlocked",
        3 => "Unsupported",
        4 => "Unofficial",
        5 => "Recently Unlocked",
        6 => "Active Challenges",
        7 => "Almost There",
        8 => "Unsynced",
        _ => "Unknown",
    }
}
