//! The Swift-facing half of RetroAchievements.
//!
//! ## The HTTP loop Swift runs
//!
//! rcheevos never touches the network. When it wants something from the server it queues a
//! request; Swift collects them with [`ContinuumEngine::achievements_take_requests`], performs each
//! with URLSession, and hands the answer back with
//! [`ContinuumEngine::achievements_complete_request`]. Completing one can queue the next (a login
//! is followed by nothing, a game load by two or three), so Swift polls again after every
//! completion. Events (logged in, game identified, unlocked) are collected the same way.
//!
//! Every method is unconditional (`#[uniffi::export]` ignores `#[cfg]` on methods) and answers
//! honestly on a build without the native core, where rcheevos is not compiled in.

use super::{ContinuumEngine, EngineError};

/// An HTTP request to make for rcheevos.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AchievementRequest {
    pub id: u64,
    pub url: String,
    /// `Some` means POST this body; `None` means GET.
    pub post_data: Option<String>,
    pub content_type: Option<String>,
}

/// Something to show or act on. See `achievements::AchievementEvent`.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum AchievementNotice {
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
    GameCompleted,
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
    ServerError {
        api: String,
        message: String,
    },
    Disconnected,
    Reconnected,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct AchievementEntry {
    pub id: u32,
    pub title: String,
    pub description: String,
    pub points: u32,
    pub unlocked: bool,
    pub bucket: String,
    pub badge_url: String,
    pub badge_locked_url: String,
    pub measured_progress: String,
    pub unlock_time: i64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct AchievementAccount {
    pub username: String,
    pub display_name: String,
    pub score: u32,
    pub score_softcore: u32,
}

/// What `achievements_status` reports.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AchievementStatus {
    /// False on a build without rcheevos compiled in.
    pub available: bool,
    pub hardcore: bool,
    pub account: Option<AchievementAccount>,
    pub game_loaded: bool,
    pub requests_in_flight: u32,
    /// rcheevos' most recent log line, for the diagnostics panel. Never contains the password.
    pub last_log: String,
}

/// Turns one engine event into its Swift shape.
#[cfg(feature = "native-core")]
fn notice(event: crate::achievements::AchievementEvent) -> AchievementNotice {
    use crate::achievements::AchievementEvent as E;
    match event {
        E::LoginSucceeded {
            username,
            display_name,
            token,
            score,
            score_softcore,
        } => AchievementNotice::LoginSucceeded {
            username,
            display_name,
            token,
            score,
            score_softcore,
        },
        E::LoginFailed { message } => AchievementNotice::LoginFailed { message },
        E::GameLoaded {
            game_id,
            title,
            hash,
            badge_url,
            total,
            unlocked,
            points_total,
            points_unlocked,
        } => AchievementNotice::GameLoaded {
            game_id,
            title,
            hash,
            badge_url,
            total,
            unlocked,
            points_total,
            points_unlocked,
        },
        E::GameLoadFailed { message } => AchievementNotice::GameLoadFailed { message },
        E::Unlocked {
            id,
            title,
            description,
            points,
            badge_url,
        } => AchievementNotice::Unlocked {
            id,
            title,
            description,
            points,
            badge_url,
        },
        E::GameCompleted => AchievementNotice::GameCompleted,
        E::Progress {
            id,
            title,
            progress,
        } => AchievementNotice::Progress {
            id,
            title,
            progress,
        },
        E::LeaderboardStarted { title } => AchievementNotice::LeaderboardStarted { title },
        E::LeaderboardFailed { title } => AchievementNotice::LeaderboardFailed { title },
        E::LeaderboardSubmitted { title, score } => {
            AchievementNotice::LeaderboardSubmitted { title, score }
        }
        E::ServerError { api, message } => AchievementNotice::ServerError { api, message },
        E::Disconnected => AchievementNotice::Disconnected,
        E::Reconnected => AchievementNotice::Reconnected,
    }
}

#[cfg(not(feature = "native-core"))]
fn unavailable() -> EngineError {
    EngineError::Achievements {
        reason: "this build has no RetroAchievements support (the native core host is off)".into(),
    }
}

#[uniffi::export]
impl ContinuumEngine {
    /// Starts a password login. The password goes into one request and is not stored by the
    /// engine. On success a `LoginSucceeded` notice carries the token to keep in the Keychain.
    pub fn achievements_login_password(
        &self,
        username: String,
        password: String,
    ) -> Result<(), EngineError> {
        #[cfg(feature = "native-core")]
        {
            let mut bridge = self.lock();
            let achievements = bridge.achievements_mut()?;
            achievements
                .login_with_password(&username, &password)
                .map_err(|reason| EngineError::Achievements { reason })
        }
        #[cfg(not(feature = "native-core"))]
        {
            let _ = (username, password);
            Err(unavailable())
        }
    }

    /// Logs in with the token a previous password login returned.
    pub fn achievements_login_token(
        &self,
        username: String,
        token: String,
    ) -> Result<(), EngineError> {
        #[cfg(feature = "native-core")]
        {
            let mut bridge = self.lock();
            let achievements = bridge.achievements_mut()?;
            achievements
                .login_with_token(&username, &token)
                .map_err(|reason| EngineError::Achievements { reason })
        }
        #[cfg(not(feature = "native-core"))]
        {
            let _ = (username, token);
            Err(unavailable())
        }
    }

    pub fn achievements_logout(&self) {
        #[cfg(feature = "native-core")]
        if let Ok(achievements) = self.lock().achievements_mut() {
            achievements.logout();
        }
    }

    /// Identifies the running game (`system` is the app's system id, `path` the file) and loads
    /// its set. The answer arrives as a `GameLoaded` or `GameLoadFailed` notice.
    pub fn achievements_load_game(&self, system: String, path: String) -> Result<(), EngineError> {
        #[cfg(feature = "native-core")]
        {
            Ok(self.lock().achievements_load_game(&system, &path)?)
        }
        #[cfg(not(feature = "native-core"))]
        {
            let _ = (system, path);
            Err(unavailable())
        }
    }

    /// Requests to send. Each is handed out once.
    pub fn achievements_take_requests(&self) -> Vec<AchievementRequest> {
        #[cfg(feature = "native-core")]
        {
            let mut bridge = self.lock();
            let Ok(achievements) = bridge.achievements_mut() else {
                return Vec::new();
            };
            achievements
                .take_requests()
                .into_iter()
                .map(|r| AchievementRequest {
                    id: r.id,
                    url: r.url,
                    post_data: r.post_data,
                    content_type: r.content_type,
                })
                .collect()
        }
        #[cfg(not(feature = "native-core"))]
        {
            Vec::new()
        }
    }

    /// Hands a response back. `http_status` is the HTTP status, or -2 when there was no
    /// response at all (offline, timed out), which rcheevos retries.
    pub fn achievements_complete_request(
        &self,
        id: u64,
        http_status: i32,
        body: Vec<u8>,
    ) -> Result<(), EngineError> {
        #[cfg(feature = "native-core")]
        {
            let mut bridge = self.lock();
            let achievements = bridge.achievements_mut()?;
            achievements
                .complete_request(id, http_status, &body)
                .map_err(|reason| EngineError::Achievements { reason })
        }
        #[cfg(not(feature = "native-core"))]
        {
            let _ = (id, http_status, body);
            Err(unavailable())
        }
    }

    /// Notices since the last call.
    pub fn achievements_take_notices(&self) -> Vec<AchievementNotice> {
        #[cfg(feature = "native-core")]
        {
            let mut bridge = self.lock();
            let Ok(achievements) = bridge.achievements_mut() else {
                return Vec::new();
            };
            achievements.take_events().into_iter().map(notice).collect()
        }
        #[cfg(not(feature = "native-core"))]
        {
            Vec::new()
        }
    }

    /// Every achievement of the loaded game. Empty when none is loaded.
    pub fn achievements_list(&self) -> Vec<AchievementEntry> {
        #[cfg(feature = "native-core")]
        {
            let bridge = self.lock();
            let Some(achievements) = bridge.achievements() else {
                return Vec::new();
            };
            achievements
                .achievements()
                .into_iter()
                .map(|a| AchievementEntry {
                    id: a.id,
                    title: a.title,
                    description: a.description,
                    points: a.points,
                    unlocked: a.unlocked,
                    bucket: a.bucket,
                    badge_url: a.badge_url,
                    badge_locked_url: a.badge_locked_url,
                    measured_progress: a.measured_progress,
                    unlock_time: a.unlock_time,
                })
                .collect()
        }
        #[cfg(not(feature = "native-core"))]
        {
            Vec::new()
        }
    }

    pub fn achievements_status(&self) -> AchievementStatus {
        #[cfg(feature = "native-core")]
        {
            let bridge = self.lock();
            match bridge.achievements() {
                Some(a) => AchievementStatus {
                    available: true,
                    hardcore: a.hardcore(),
                    account: a.user().map(|u| AchievementAccount {
                        username: u.username,
                        display_name: u.display_name,
                        score: u.score,
                        score_softcore: u.score_softcore,
                    }),
                    game_loaded: a.is_game_loaded(),
                    requests_in_flight: a.requests_in_flight() as u32,
                    last_log: a.last_log(),
                },
                None => AchievementStatus {
                    available: true,
                    hardcore: false,
                    account: None,
                    game_loaded: false,
                    requests_in_flight: 0,
                    last_log: String::new(),
                },
            }
        }
        #[cfg(not(feature = "native-core"))]
        {
            AchievementStatus {
                available: false,
                hardcore: false,
                account: None,
                game_loaded: false,
                requests_in_flight: 0,
                last_log: String::new(),
            }
        }
    }

    /// "rcheevos/12.5" or similar, to append to the User-Agent header.
    pub fn achievements_user_agent_clause(&self) -> String {
        #[cfg(feature = "native-core")]
        {
            let mut bridge = self.lock();
            bridge
                .achievements_mut()
                .map(|a| a.user_agent_clause())
                .unwrap_or_default()
        }
        #[cfg(not(feature = "native-core"))]
        {
            String::new()
        }
    }
}
