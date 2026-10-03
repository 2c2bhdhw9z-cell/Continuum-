//! The Swift-facing half of online play and cloud sync. A child module of `uniffi_api` (so it
//! can use `ContinuumEngine::lock`), kept in its own file so those two features do not churn the
//! main facade. Same rule as the parent: convert, lock, delegate, nothing else.

use super::{ContinuumEngine, EngineError};
use crate::netplay::{self, NetplayConfig, Role, StatusKind};
use crate::sync;

// ------------------------------------------------------------------ netplay

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum NetplayStatusKind {
    /// No online session at all.
    Idle,
    Waiting,
    Connected,
    Syncing,
    Running,
    Stalled,
    Desynced,
    Disconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum NetplayRole {
    Host,
    Guest,
}

/// Everything the HUD needs about the online session, in one read.
#[derive(Debug, Clone, uniffi::Record)]
pub struct NetplayStatus {
    pub kind: NetplayStatusKind,
    pub role: Option<NetplayRole>,
    /// True while rewind, fast forward, reset, cheats and state loads are refused.
    pub live: bool,
    /// One plain line for this state, ready to show.
    pub line: String,
    pub frame: u64,
    pub input_delay: u32,
    /// Round trip in milliseconds, `-1` until the first pong.
    pub ping_ms: f64,
    pub stall_ms: f64,
    pub desync_frame: Option<u64>,
    pub checks_compared: u64,
}

/// TCP port a host listens on unless it is taken. Shared with Android through this function.
#[uniffi::export]
pub fn netplay_default_port() -> u16 {
    netplay::DEFAULT_PORT
}

/// Bonjour service type both platforms advertise and browse.
#[uniffi::export]
pub fn netplay_bonjour_type() -> String {
    netplay::BONJOUR_SERVICE_TYPE.to_string()
}

#[uniffi::export]
impl ContinuumEngine {
    /// Starts hosting the running game. `content_path` is the ROM file, fingerprinted so a
    /// guest with a different game is refused by name. `input_delay` is in frames (2 is the
    /// default and 0 is allowed); `checksum_interval` is frames between desync checks, 0 off.
    pub fn netplay_host(
        &self,
        content_path: String,
        input_delay: u32,
        checksum_interval: u32,
    ) -> Result<(), EngineError> {
        let config = NetplayConfig {
            input_delay: input_delay.min(u32::from(netplay::MAX_INPUT_DELAY)) as u8,
            checksum_interval,
            ..NetplayConfig::default()
        };
        Ok(self.lock().netplay_host(&content_path, config)?)
    }

    /// Prepares to join a host with the running game. Open the connection afterwards and call
    /// `netplayTransportConnected` when it is ready.
    pub fn netplay_join(&self, content_path: String) -> Result<(), EngineError> {
        Ok(self.lock().netplay_join(&content_path)?)
    }

    pub fn netplay_transport_connected(&self) {
        self.lock().netplay_transport_connected();
    }

    pub fn netplay_transport_lost(&self, reason: String) {
        self.lock().netplay_transport_lost(&reason);
    }

    /// Bytes that arrived from the other phone, exactly as the socket delivered them.
    pub fn netplay_receive(&self, data: Vec<u8>) {
        self.lock().netplay_receive(&data);
    }

    /// Bytes to send to the other phone now. Call after every tick and every receive.
    pub fn netplay_take_outgoing(&self) -> Vec<u8> {
        self.lock().netplay_take_outgoing()
    }

    /// Ends the session from this side; the goodbye is in the next `netplayTakeOutgoing`.
    pub fn netplay_leave(&self, reason: String) {
        self.lock().netplay_leave(&reason);
    }

    /// Forgets the session, returning the last bytes to flush before closing the socket.
    pub fn netplay_stop(&self) -> Vec<u8> {
        self.lock().netplay_clear()
    }

    pub fn netplay_is_live(&self) -> bool {
        self.lock().netplay_is_live()
    }

    pub fn netplay_status(&self) -> NetplayStatus {
        let guard = self.lock();
        let Some(np) = guard.netplay() else {
            return NetplayStatus {
                kind: NetplayStatusKind::Idle,
                role: None,
                live: false,
                line: "online: off".to_string(),
                frame: 0,
                input_delay: 0,
                ping_ms: -1.0,
                stall_ms: 0.0,
                desync_frame: None,
                checks_compared: 0,
            };
        };
        NetplayStatus {
            kind: match np.status_kind() {
                StatusKind::Waiting => NetplayStatusKind::Waiting,
                StatusKind::Connected => NetplayStatusKind::Connected,
                StatusKind::Syncing => NetplayStatusKind::Syncing,
                StatusKind::Running => NetplayStatusKind::Running,
                StatusKind::Stalled => NetplayStatusKind::Stalled,
                StatusKind::Desynced => NetplayStatusKind::Desynced,
                StatusKind::Disconnected => NetplayStatusKind::Disconnected,
            },
            role: Some(match np.role() {
                Role::Host => NetplayRole::Host,
                Role::Guest => NetplayRole::Guest,
            }),
            live: np.is_live(),
            line: np.status_line(),
            frame: np.frame(),
            input_delay: u32::from(np.input_delay()),
            ping_ms: np.rtt_ms().unwrap_or(-1.0),
            stall_ms: np.stall_ms(),
            desync_frame: np.desync_frame(),
            checks_compared: np.checks_compared(),
        }
    }
}

// ------------------------------------------------------------------ sync

#[derive(Debug, Clone, uniffi::Record)]
pub struct SyncFileStat {
    /// Relative to the sync root, `/`-separated, e.g. `SaveStates/index.json`.
    pub path: String,
    pub size: u64,
    /// Modification time in milliseconds since 1970.
    pub mtime_ms: i64,
}

impl From<SyncFileStat> for sync::FileStat {
    fn from(s: SyncFileStat) -> Self {
        Self {
            path: s.path,
            size: s.size,
            mtime_ms: s.mtime_ms,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SyncActionKind {
    Upload,
    Download,
    ConflictKeepLocal,
    ConflictKeepRemote,
    MergeRecords,
    ArchiveLocal,
    ArchiveRemote,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SyncAction {
    pub kind: SyncActionKind,
    pub path: String,
    /// Cloud-relative destination of the copy kept aside (`Conflicts/...` or `Deleted/...`),
    /// empty when the action keeps nothing aside.
    pub aside: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SyncRecord {
    pub key: String,
    pub stamp: f64,
    pub body: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SyncReport {
    pub uploaded: u32,
    pub downloaded: u32,
    pub conflicts: u32,
    pub merged: u32,
    pub archived: u32,
    pub errors: Vec<String>,
}

fn stats(list: Vec<SyncFileStat>) -> Vec<sync::FileStat> {
    list.into_iter().map(Into::into).collect()
}

/// A sync plan, or the reason none should be carried out.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SyncPlan {
    pub actions: Vec<SyncAction>,
    /// Set when the listing looks like an unreachable folder. Do nothing and show this.
    pub refusal: Option<String>,
}

/// Everything to do this sync, in order. See `crate::sync` for the rules.
#[uniffi::export]
pub fn sync_plan(
    local: Vec<SyncFileStat>,
    remote: Vec<SyncFileStat>,
    manifest_text: String,
    now_ms: i64,
) -> SyncPlan {
    let manifest = sync::Manifest::parse(&manifest_text);
    let (local, remote) = (stats(local), stats(remote));
    let actions = sync::plan(&local, &remote, &manifest, now_ms);
    let refusal = sync::refusal(&actions, local.len(), remote.len(), &manifest);
    let actions = actions
        .into_iter()
        .map(|a| SyncAction {
            kind: match a.kind {
                sync::ActionKind::Upload => SyncActionKind::Upload,
                sync::ActionKind::Download => SyncActionKind::Download,
                sync::ActionKind::ConflictKeepLocal => SyncActionKind::ConflictKeepLocal,
                sync::ActionKind::ConflictKeepRemote => SyncActionKind::ConflictKeepRemote,
                sync::ActionKind::MergeRecords => SyncActionKind::MergeRecords,
                sync::ActionKind::ArchiveLocal => SyncActionKind::ArchiveLocal,
                sync::ActionKind::ArchiveRemote => SyncActionKind::ArchiveRemote,
            },
            path: a.path,
            aside: a.aside,
        })
        .collect();
    SyncPlan { actions, refusal }
}

/// The manifest text to store after a sync. `*_before` are the listings the plan was made from,
/// `*_after` the listings once the actions ran; `touched_paths` are the paths an action wrote, and
/// `failed_paths` keep their previous record. See `crate::sync::commit`.
#[uniffi::export]
#[allow(clippy::too_many_arguments)]
pub fn sync_commit(
    manifest_text: String,
    local_before: Vec<SyncFileStat>,
    remote_before: Vec<SyncFileStat>,
    local_after: Vec<SyncFileStat>,
    remote_after: Vec<SyncFileStat>,
    touched_paths: Vec<String>,
    failed_paths: Vec<String>,
    now_ms: i64,
) -> String {
    let previous = sync::Manifest::parse(&manifest_text);
    sync::commit(
        &previous,
        &stats(local_before),
        &stats(remote_before),
        &stats(local_after),
        &stats(remote_after),
        &touched_paths,
        &failed_paths,
        now_ms,
    )
    .to_text()
}

/// Where to keep a file that would otherwise be replaced, named the way the plan names a conflict
/// loser. For the platform's "the cloud file changed since it was listed" guard.
#[uniffi::export]
pub fn sync_conflict_path(path: String, now_ms: i64, loser_is_local: bool) -> String {
    sync::conflict_path(&path, now_ms, loser_is_local)
}

/// When the manifest says the last sync finished, `0` for never.
#[uniffi::export]
pub fn sync_last_synced_ms(manifest_text: String) -> i64 {
    sync::Manifest::parse(&manifest_text).last_sync_ms
}

#[uniffi::export]
pub fn sync_merge_records(
    base: Vec<SyncRecord>,
    local: Vec<SyncRecord>,
    remote: Vec<SyncRecord>,
) -> Vec<SyncRecord> {
    let conv = |list: Vec<SyncRecord>| -> Vec<sync::Record> {
        list.into_iter()
            .map(|r| sync::Record {
                key: r.key,
                stamp: r.stamp,
                body: r.body,
            })
            .collect()
    };
    sync::merge_records(&conv(base), &conv(local), &conv(remote))
        .into_iter()
        .map(|r| SyncRecord {
            key: r.key,
            stamp: r.stamp,
            body: r.body,
        })
        .collect()
}

#[uniffi::export]
pub fn sync_files_identical(path_a: String, path_b: String) -> bool {
    sync::files_identical(&path_a, &path_b)
}

#[uniffi::export]
pub fn sync_is_valid_path(path: String) -> bool {
    sync::is_valid_path(&path)
}

#[uniffi::export]
pub fn sync_report_line(report: SyncReport) -> String {
    sync::Report {
        uploaded: report.uploaded,
        downloaded: report.downloaded,
        conflicts: report.conflicts,
        merged: report.merged,
        archived: report.archived,
        errors: report.errors,
    }
    .line()
}
