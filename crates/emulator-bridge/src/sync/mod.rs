//! The rules for two-way folder sync of save states, battery saves, Flash and J2ME saves, cheats,
//! manuals, Amiibo, artwork choices and settings, kept here so Android applies exactly the same
//! decisions.
//!
//! The platform does the file I/O (on iOS through a security-scoped bookmark and
//! `NSFileCoordinator`, because the folder can be iCloud Drive, Google Drive, Dropbox or anything
//! else in Files). This module only ever sees lists of `(path, size, modification time)` and
//! answers with a list of [`Action`]s, and afterwards turns what the folders look like into the
//! next [`Manifest`].
//!
//! ## The model
//!
//! Every file has a path relative to the sync root, the same on both sides
//! (`SaveStates/…`, `Battery/…`, `PlayerSaves/…`, `Cheats/index.json`, `Manuals/…`, `Amiibo/…`,
//! `Settings/…`, `Artwork/…`). The manifest remembers, per path, the size and modification time
//! each side had right after the last successful sync. A side has CHANGED a file when its current
//! size or mtime differs from that record. Then:
//!
//! | local      | cloud      | decision |
//! | ---------- | ---------- | -------- |
//! | changed    | unchanged  | upload |
//! | unchanged  | changed    | download |
//! | changed    | changed    | conflict: newest mtime wins, the loser is copied to `Conflicts/` with a date |
//! | new        | absent     | upload |
//! | absent     | new        | download |
//! | deleted    | unchanged  | the cloud copy is MOVED to `Deleted/<date>/`, never removed |
//! | unchanged  | deleted    | the local copy is moved to the cloud's `Deleted/<date>/`, then removed locally |
//! | deleted    | changed    | download (an edit beats a deletion) |
//! | changed    | deleted    | upload |
//!
//! Two rules sit on top of that table:
//!
//! - **Nothing is ever destroyed.** A deletion only propagates for file kinds where that is what
//!   a user means (a deleted save-state slot, a removed cover) and even then the last copy is
//!   moved into `Deleted/`, not removed. Everything else that goes missing on one side is simply
//!   copied back.
//! - **Record files are merged, not raced.** `SaveStates/index.json` and `Cheats/index.json`
//!   each hold every game's records. Newest-file-wins would make two phones that each saved a
//!   slot lose one of them, so these get [`ActionKind::MergeRecords`] and [`merge_records`] does
//!   a three-way merge per record against the copy kept from the last sync.

use std::collections::{BTreeMap, BTreeSet};

/// Record files: merged per record instead of whole-file newest-wins.
pub const RECORD_FILES: [&str; 2] = ["SaveStates/index.json", "Cheats/index.json"];

/// Folders inside the cloud root that the sync itself writes and never lists as data.
pub const CONFLICTS_DIR: &str = "Conflicts";
pub const DELETED_DIR: &str = "Deleted";

/// Two mtimes this close are the same moment. Some cloud providers store whole seconds.
const MTIME_SLACK_MS: i64 = 2_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStat {
    pub path: String,
    pub size: u64,
    pub mtime_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seen {
    pub size: u64,
    pub mtime_ms: i64,
}

impl Seen {
    fn of(stat: &FileStat) -> Self {
        Self {
            size: stat.size,
            mtime_ms: stat.mtime_ms,
        }
    }

    fn matches(self, stat: &FileStat) -> bool {
        self.size == stat.size && (self.mtime_ms - stat.mtime_ms).abs() <= MTIME_SLACK_MS
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BaseEntry {
    pub local: Option<Seen>,
    pub remote: Option<Seen>,
}

/// What both folders looked like after the last sync. Stored on the device, not in the cloud:
/// each install has its own view of "last time".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Manifest {
    pub entries: BTreeMap<String, BaseEntry>,
    pub last_sync_ms: i64,
}

const MANIFEST_HEADER: &str = "continuum-sync-manifest 1";

fn escape(path: &str) -> String {
    path.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn seen_field(size: &str, mtime: &str) -> Option<Seen> {
    Some(Seen {
        size: size.parse().ok()?,
        mtime_ms: mtime.parse().ok()?,
    })
}

impl Manifest {
    /// Parses a stored manifest. Tolerant on purpose: a damaged line costs that one file a
    /// conflict check (which keeps both copies), never data.
    pub fn parse(text: &str) -> Self {
        let mut manifest = Manifest::default();
        let mut lines = text.lines();
        if lines.next() != Some(MANIFEST_HEADER) {
            return manifest;
        }
        for line in lines {
            let fields: Vec<&str> = line.split('\t').collect();
            if fields.first() == Some(&"#last") {
                manifest.last_sync_ms = fields.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);
                continue;
            }
            if fields.len() != 5 {
                continue;
            }
            let entry = BaseEntry {
                local: seen_field(fields[1], fields[2]),
                remote: seen_field(fields[3], fields[4]),
            };
            manifest.entries.insert(unescape(fields[0]), entry);
        }
        manifest
    }

    pub fn to_text(&self) -> String {
        let mut out = String::from(MANIFEST_HEADER);
        out.push('\n');
        out.push_str(&format!("#last\t{}\n", self.last_sync_ms));
        let field = |seen: Option<Seen>| match seen {
            Some(s) => format!("{}\t{}", s.size, s.mtime_ms),
            None => "-\t-".to_string(),
        };
        for (path, entry) in &self.entries {
            out.push_str(&format!(
                "{}\t{}\t{}\n",
                escape(path),
                field(entry.local),
                field(entry.remote)
            ));
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    /// Copy local over the cloud.
    Upload,
    /// Copy the cloud over local.
    Download,
    /// Both changed and local is newer: copy the CLOUD file to `aside` (in the cloud), then
    /// upload. The platform should first check the two files for identical bytes
    /// ([`files_identical`]) and do nothing if they are.
    ConflictKeepLocal,
    /// Both changed and the cloud is newer: copy the LOCAL file to `aside` (in the cloud), then
    /// download. Same identical-bytes check first.
    ConflictKeepRemote,
    /// A record file: three-way merge with [`merge_records`], write the result to both sides.
    MergeRecords,
    /// Deleted in the cloud: copy the local file to `aside` in the cloud, then remove it locally.
    ArchiveLocal,
    /// Deleted locally: move the cloud file to `aside` in the cloud.
    ArchiveRemote,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub kind: ActionKind,
    pub path: String,
    /// Destination, relative to the cloud root, for the copy kept aside. Empty when unused.
    pub aside: String,
}

pub fn is_record_file(path: &str) -> bool {
    RECORD_FILES.contains(&path)
}

/// Whether deleting this kind of file on one phone should delete it on the other (into
/// `Deleted/`, never for good). True only where a deletion is a deliberate user act. Everything
/// else (battery and player saves, cheats, settings, manuals, Amiibo) is copied back when it goes
/// missing on one side, because a game's only save vanishing by accident costs far more than
/// having to remove a file from the cloud folder as well.
pub fn propagates_deletion(path: &str) -> bool {
    (path.starts_with("SaveStates/") && (path.ends_with(".state") || path.ends_with(".png")))
        || path.starts_with("Artwork/covers/")
}

/// Whether a path is a sync data path at all. Rejects the sync's own folders, absolute or
/// escaping paths and empty components, so a hostile or damaged folder listing cannot make the
/// platform write outside its roots.
pub fn is_valid_path(path: &str) -> bool {
    if path.is_empty() || path.starts_with('/') || path.contains('\\') {
        return false;
    }
    let mut parts = path.split('/');
    if let Some(first) = parts.clone().next() {
        if first == CONFLICTS_DIR || first == DELETED_DIR {
            return false;
        }
    }
    parts.all(|part| !part.is_empty() && part != "." && part != ".." && !part.starts_with('.'))
}

// ------------------------------------------------------------------ dates

/// `(year, month, day, hour, minute, second)` in UTC. Howard Hinnant's civil-from-days.
fn civil(ms: i64) -> (i64, u32, u32, u32, u32, u32) {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (
        year,
        month,
        day,
        (rem / 3600) as u32,
        ((rem % 3600) / 60) as u32,
        (rem % 60) as u32,
    )
}

/// `2025-03-04 1530 07` style stamp, UTC, safe in a filename on every provider.
pub fn date_stamp(ms: i64) -> String {
    let (y, mo, d, h, mi, s) = civil(ms);
    format!("{y:04}-{mo:02}-{d:02} {h:02}{mi:02}{s:02} UTC")
}

/// Where the losing copy of a conflict goes: `Conflicts/<dir>/<stem> (conflict <date> from
/// <side>).<ext>`.
pub fn conflict_path(path: &str, now_ms: i64, loser_is_local: bool) -> String {
    let (dir, name) = match path.rfind('/') {
        Some(i) => (&path[..=i], &path[i + 1..]),
        None => ("", path),
    };
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    let side = if loser_is_local {
        "this device"
    } else {
        "cloud"
    };
    format!(
        "{CONFLICTS_DIR}/{dir}{stem} (conflict {} from {side}){ext}",
        date_stamp(now_ms)
    )
}

pub fn archive_path(path: &str, now_ms: i64) -> String {
    format!("{DELETED_DIR}/{}/{path}", date_stamp(now_ms))
}

// ------------------------------------------------------------------ planning

/// Decides what to do with every path.
///
/// `local` and `remote` are what the two folders hold right now. Invalid paths are ignored.
/// Actions come back sorted by path, with record files last, so payloads land before the
/// index that refers to them.
pub fn plan(local: &[FileStat], remote: &[FileStat], base: &Manifest, now_ms: i64) -> Vec<Action> {
    let local: BTreeMap<&str, &FileStat> = local
        .iter()
        .filter(|s| is_valid_path(&s.path))
        .map(|s| (s.path.as_str(), s))
        .collect();
    let remote: BTreeMap<&str, &FileStat> = remote
        .iter()
        .filter(|s| is_valid_path(&s.path))
        .map(|s| (s.path.as_str(), s))
        .collect();
    let paths: BTreeSet<&str> = local
        .keys()
        .chain(remote.keys())
        .copied()
        .chain(
            base.entries
                .keys()
                .map(String::as_str)
                .filter(|p| is_valid_path(p)),
        )
        .collect();

    let mut actions = Vec::new();
    for path in paths {
        let l = local.get(path).copied();
        let r = remote.get(path).copied();
        let b = base.entries.get(path).copied().unwrap_or_default();
        if let Some(action) = decide(path, l, r, b, now_ms) {
            actions.push(action);
        }
    }
    actions.sort_by_key(|a| (is_record_file(&a.path), a.path.clone()));
    actions
}

fn action(kind: ActionKind, path: &str, aside: String) -> Option<Action> {
    Some(Action {
        kind,
        path: path.to_string(),
        aside,
    })
}

fn decide(
    path: &str,
    l: Option<&FileStat>,
    r: Option<&FileStat>,
    b: BaseEntry,
    now_ms: i64,
) -> Option<Action> {
    let changed = |stat: &FileStat, seen: Option<Seen>| seen.is_none_or(|s| !s.matches(stat));
    match (l, r) {
        (None, None) => None,
        (Some(l), Some(r)) => {
            let lc = changed(l, b.local);
            let rc = changed(r, b.remote);
            match (lc, rc) {
                (false, false) => None,
                (true, false) => action(ActionKind::Upload, path, String::new()),
                (false, true) => action(ActionKind::Download, path, String::new()),
                (true, true) => {
                    if is_record_file(path) {
                        return action(ActionKind::MergeRecords, path, String::new());
                    }
                    // Newest wins. Ties go to the larger file, then to this device, so the
                    // decision is the same however many times it is made.
                    let local_wins = (l.mtime_ms, l.size) >= (r.mtime_ms, r.size);
                    if local_wins {
                        action(
                            ActionKind::ConflictKeepLocal,
                            path,
                            conflict_path(path, now_ms, false),
                        )
                    } else {
                        action(
                            ActionKind::ConflictKeepRemote,
                            path,
                            conflict_path(path, now_ms, true),
                        )
                    }
                }
            }
        }
        (Some(l), None) => {
            // Never in the cloud as far as we know, or edited here since: send it up.
            if b.remote.is_none() || changed(l, b.local) || !propagates_deletion(path) {
                action(ActionKind::Upload, path, String::new())
            } else {
                action(ActionKind::ArchiveLocal, path, archive_path(path, now_ms))
            }
        }
        (None, Some(r)) => {
            if b.local.is_none() || changed(r, b.remote) || !propagates_deletion(path) {
                action(ActionKind::Download, path, String::new())
            } else {
                action(ActionKind::ArchiveRemote, path, archive_path(path, now_ms))
            }
        }
    }
}

/// Refuses a plan that looks like an unreachable folder rather than a real change.
///
/// A cloud folder that is signed out, unmounted or still materialising can list as EMPTY. Read
/// literally that means "every file was deleted in the cloud", and the plan would archive this
/// device's saves. So: a cloud listing with nothing in it, when the manifest says it held files
/// last time, or a plan that would archive more than half of this device's files at once (and
/// more than three), is refused with a readable reason and nothing is touched.
pub fn refusal(
    actions: &[Action],
    local_count: usize,
    remote_count: usize,
    base: &Manifest,
) -> Option<String> {
    let remote_known = base.entries.values().filter(|e| e.remote.is_some()).count();
    if remote_count == 0 && remote_known > 0 {
        return Some(format!(
            "the cloud folder looks empty but held {remote_known} files last time; it may be signed out or not downloaded. Nothing was changed. Open it in Files, or choose it again"
        ));
    }
    let archiving = actions
        .iter()
        .filter(|a| a.kind == ActionKind::ArchiveLocal)
        .count();
    if archiving > 3 && archiving * 2 > local_count {
        return Some(format!(
            "the cloud folder is missing {archiving} of this device's {local_count} files at once, which looks like a folder that is not reachable. Nothing was changed"
        ));
    }
    None
}

/// The manifest after a sync.
///
/// A path an action TOUCHED is recorded as it is now (`*_after`). Every other path is recorded as
/// it was when the plan was made (`*_before`), not as it is now: a file that changed while the
/// sync ran (a core writing its battery save, a slot saved) must still look changed next time,
/// or its change would be silently marked as synced and later overwritten. A path whose action
/// FAILED keeps its previous record, so the same decision is made again.
#[allow(clippy::too_many_arguments)]
pub fn commit(
    previous: &Manifest,
    local_before: &[FileStat],
    remote_before: &[FileStat],
    local_after: &[FileStat],
    remote_after: &[FileStat],
    touched: &[String],
    failed: &[String],
    now_ms: i64,
) -> Manifest {
    let touched: BTreeSet<&str> = touched.iter().map(String::as_str).collect();
    let failed: BTreeSet<&str> = failed.iter().map(String::as_str).collect();
    let pick = |before: &[FileStat], after: &[FileStat]| -> Vec<FileStat> {
        before
            .iter()
            .filter(|s| !touched.contains(s.path.as_str()))
            .chain(after.iter().filter(|s| touched.contains(s.path.as_str())))
            .filter(|s| is_valid_path(&s.path))
            .cloned()
            .collect()
    };
    let mut entries: BTreeMap<String, BaseEntry> = BTreeMap::new();
    for stat in pick(local_before, local_after) {
        entries.entry(stat.path.clone()).or_default().local = Some(Seen::of(&stat));
    }
    for stat in pick(remote_before, remote_after) {
        entries.entry(stat.path.clone()).or_default().remote = Some(Seen::of(&stat));
    }
    for path in &failed {
        match previous.entries.get(*path) {
            Some(old) => {
                entries.insert((*path).to_string(), *old);
            }
            None => {
                entries.remove(*path);
            }
        }
    }
    Manifest {
        entries,
        last_sync_ms: now_ms,
    }
}

// ------------------------------------------------------------------ record merge

/// One record of a record file, as the platform extracted it: a stable key, a timestamp for
/// breaking ties, and the record's canonical text (sorted-key JSON on iOS).
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub key: String,
    pub stamp: f64,
    pub body: String,
}

/// Three-way merge of a record file. `base` is the copy kept from the last sync (empty on the
/// first one).
///
/// Per key: a side that left a record as it was in `base` defers to the side that changed or
/// removed it. When both changed it, the newer `stamp` wins (ties go local). When one side
/// deleted a record and the other edited it, the edit is kept: losing an edit is worse than
/// resurrecting a record. Order follows the local file, then records only the cloud has, in the
/// cloud's order.
pub fn merge_records(base: &[Record], local: &[Record], remote: &[Record]) -> Vec<Record> {
    let by_key = |list: &[Record]| -> BTreeMap<String, Record> {
        list.iter().map(|r| (r.key.clone(), r.clone())).collect()
    };
    let b = by_key(base);
    let l = by_key(local);
    let r = by_key(remote);
    let mut order: Vec<String> = Vec::new();
    let mut seen = BTreeSet::new();
    for key in local.iter().chain(remote.iter()).map(|r| &r.key) {
        if seen.insert(key.clone()) {
            order.push(key.clone());
        }
    }
    let same = |a: Option<&Record>, b: Option<&Record>| match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.body == b.body,
        _ => false,
    };
    let mut out = Vec::new();
    for key in order {
        let (bv, lv, rv) = (b.get(&key), l.get(&key), r.get(&key));
        let chosen = if same(lv, rv) {
            lv
        } else if same(lv, bv) {
            rv
        } else if same(rv, bv) {
            lv
        } else {
            match (lv, rv) {
                (Some(lv), Some(rv)) => Some(if rv.stamp > lv.stamp { rv } else { lv }),
                (Some(lv), None) => Some(lv),
                (None, Some(rv)) => Some(rv),
                (None, None) => None,
            }
        };
        if let Some(record) = chosen {
            out.push(record.clone());
        }
    }
    out
}

// ------------------------------------------------------------------ helpers

/// Byte-for-byte comparison of two files, used before acting on a conflict so that two phones
/// that wrote the same bytes (a first sync, a provider that touched an mtime) do not produce a
/// pointless conflict copy. Any read failure is "not identical", which keeps both.
pub fn files_identical(a: &str, b: &str) -> bool {
    use std::io::Read;
    let (Ok(mut fa), Ok(mut fb)) = (std::fs::File::open(a), std::fs::File::open(b)) else {
        return false;
    };
    match (fa.metadata(), fb.metadata()) {
        (Ok(ma), Ok(mb)) if ma.len() == mb.len() => {}
        _ => return false,
    }
    let mut ba = vec![0u8; 64 * 1024];
    let mut bb = vec![0u8; 64 * 1024];
    loop {
        let Ok(na) = fa.read(&mut ba) else {
            return false;
        };
        if na == 0 {
            return true;
        }
        if fb.read_exact(&mut bb[..na]).is_err() || ba[..na] != bb[..na] {
            return false;
        }
    }
}

/// Totals of one sync run, for the status line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub uploaded: u32,
    pub downloaded: u32,
    pub conflicts: u32,
    pub merged: u32,
    pub archived: u32,
    pub errors: Vec<String>,
}

impl Report {
    /// One plain line: `3 up, 2 down, 1 conflict (both kept), 0 errors`.
    pub fn line(&self) -> String {
        let plural =
            |n: u32, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        let mut parts = vec![
            format!("{} up", self.uploaded),
            format!("{} down", self.downloaded),
        ];
        if self.conflicts > 0 {
            parts.push(format!(
                "{} (both kept in Conflicts)",
                plural(self.conflicts, "conflict", "conflicts")
            ));
        }
        if self.merged > 0 {
            parts.push(format!("{} merged", plural(self.merged, "list", "lists")));
        }
        if self.archived > 0 {
            parts.push(format!(
                "{} moved to Deleted",
                plural(self.archived, "file", "files")
            ));
        }
        match self.errors.first() {
            None => parts.push("no errors".to_string()),
            Some(first) => parts.push(format!(
                "{} (first: {first})",
                plural(self.errors.len() as u32, "error", "errors")
            )),
        }
        parts.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(path: &str, size: u64, mtime: i64) -> FileStat {
        FileStat {
            path: path.into(),
            size,
            mtime_ms: mtime,
        }
    }

    fn base_of(path: &str, local: Option<(u64, i64)>, remote: Option<(u64, i64)>) -> Manifest {
        let mut m = Manifest::default();
        let seen = |v: Option<(u64, i64)>| v.map(|(size, mtime_ms)| Seen { size, mtime_ms });
        m.entries.insert(
            path.into(),
            BaseEntry {
                local: seen(local),
                remote: seen(remote),
            },
        );
        m
    }

    const NOW: i64 = 1_700_000_000_000;
    const P: &str = "SaveStates/0123456789abcdef-1.state";

    fn one(local: &[FileStat], remote: &[FileStat], base: &Manifest) -> Option<Action> {
        let mut actions = plan(local, remote, base, NOW);
        assert!(actions.len() <= 1, "{actions:?}");
        actions.pop()
    }

    #[test]
    fn unchanged_files_do_nothing() {
        let base = base_of(P, Some((10, 1000)), Some((10, 5000)));
        assert_eq!(one(&[st(P, 10, 1000)], &[st(P, 10, 5000)], &base), None);
    }

    #[test]
    fn a_one_sided_change_is_copied_the_right_way() {
        let base = base_of(P, Some((10, 1000)), Some((10, 5000)));
        assert_eq!(
            one(&[st(P, 11, 9000)], &[st(P, 10, 5000)], &base)
                .unwrap()
                .kind,
            ActionKind::Upload
        );
        assert_eq!(
            one(&[st(P, 10, 1000)], &[st(P, 12, 9000)], &base)
                .unwrap()
                .kind,
            ActionKind::Download
        );
    }

    #[test]
    fn an_mtime_within_slack_is_not_a_change() {
        let base = base_of(P, Some((10, 1000)), Some((10, 5000)));
        assert_eq!(one(&[st(P, 10, 2500)], &[st(P, 10, 5000)], &base), None);
    }

    #[test]
    fn both_changed_newest_wins_and_the_loser_is_kept_with_a_date() {
        let base = base_of(P, Some((10, 1000)), Some((10, 5000)));
        let a = one(&[st(P, 11, 20_000)], &[st(P, 12, 9_000)], &base).unwrap();
        assert_eq!(a.kind, ActionKind::ConflictKeepLocal);
        assert!(a
            .aside
            .starts_with("Conflicts/SaveStates/0123456789abcdef-1 (conflict 2023-11-14"));
        assert!(a.aside.ends_with("from cloud).state"), "{}", a.aside);
        let b = one(&[st(P, 11, 9_000)], &[st(P, 12, 20_000)], &base).unwrap();
        assert_eq!(b.kind, ActionKind::ConflictKeepRemote);
        assert!(b.aside.ends_with("from this device).state"));
    }

    #[test]
    fn a_first_sync_with_both_copies_is_a_conflict_check_not_an_overwrite() {
        let a = one(&[st(P, 10, 1000)], &[st(P, 10, 1000)], &Manifest::default()).unwrap();
        assert!(matches!(
            a.kind,
            ActionKind::ConflictKeepLocal | ActionKind::ConflictKeepRemote
        ));
    }

    #[test]
    fn new_files_flow_to_the_side_that_lacks_them() {
        let m = Manifest::default();
        assert_eq!(
            one(&[st(P, 1, 1)], &[], &m).unwrap().kind,
            ActionKind::Upload
        );
        assert_eq!(
            one(&[], &[st(P, 1, 1)], &m).unwrap().kind,
            ActionKind::Download
        );
    }

    #[test]
    fn a_deleted_slot_is_archived_never_destroyed() {
        let base = base_of(P, Some((10, 1000)), Some((10, 5000)));
        let a = one(&[], &[st(P, 10, 5000)], &base).unwrap();
        assert_eq!(a.kind, ActionKind::ArchiveRemote);
        assert!(a.aside.starts_with("Deleted/2023-11-14 "));
        assert!(a.aside.ends_with(P));
        let b = one(&[st(P, 10, 1000)], &[], &base).unwrap();
        assert_eq!(b.kind, ActionKind::ArchiveLocal);
    }

    #[test]
    fn an_edit_beats_a_deletion() {
        let base = base_of(P, Some((10, 1000)), Some((10, 5000)));
        assert_eq!(
            one(&[], &[st(P, 99, 9000)], &base).unwrap().kind,
            ActionKind::Download
        );
        assert_eq!(
            one(&[st(P, 99, 9000)], &[], &base).unwrap().kind,
            ActionKind::Upload
        );
    }

    #[test]
    fn battery_saves_and_settings_are_restored_rather_than_deleted() {
        for path in [
            "Battery/Game.srm",
            "Settings/defaults.plist",
            "Cheats/index.json",
        ] {
            let base = base_of(path, Some((10, 1000)), Some((10, 5000)));
            assert_eq!(
                one(&[], &[st(path, 10, 5000)], &base).unwrap().kind,
                ActionKind::Download,
                "{path}"
            );
            assert_eq!(
                one(&[st(path, 10, 1000)], &[], &base).unwrap().kind,
                ActionKind::Upload,
                "{path}"
            );
        }
    }

    #[test]
    fn player_saves_manuals_and_amiibo_are_restored_rather_than_deleted() {
        for path in [
            "PlayerSaves/Bloons.json",
            "PlayerSaves/Snake.J2meJS.srm",
            "Manuals/Super Metroid (USA).pdf",
            "Amiibo/Mario.bin",
        ] {
            assert!(is_valid_path(path), "{path}");
            assert!(!propagates_deletion(path), "{path}");
            let base = base_of(path, Some((10, 1000)), Some((10, 5000)));
            assert_eq!(
                one(&[], &[st(path, 10, 5000)], &base).unwrap().kind,
                ActionKind::Download,
                "{path}"
            );
            assert_eq!(
                one(&[st(path, 10, 1000)], &[], &base).unwrap().kind,
                ActionKind::Upload,
                "{path}"
            );
        }
    }

    #[test]
    fn a_player_save_changed_on_both_phones_keeps_both() {
        let path = "PlayerSaves/Snake.J2meJS.srm";
        let base = base_of(path, Some((10, 1000)), Some((10, 5000)));
        let a = one(&[st(path, 11, 20_000)], &[st(path, 12, 9_000)], &base).unwrap();
        assert_eq!(a.kind, ActionKind::ConflictKeepLocal);
        assert_eq!(
            a.aside,
            "Conflicts/PlayerSaves/Snake.J2meJS (conflict 2023-11-14 221320 UTC from cloud).srm"
        );
    }

    #[test]
    fn record_files_merge_and_come_last() {
        let idx = "SaveStates/index.json";
        let mut base = base_of(idx, Some((10, 1000)), Some((10, 5000)));
        base.entries.insert(
            P.into(),
            BaseEntry {
                local: Some(Seen {
                    size: 1,
                    mtime_ms: 1,
                }),
                remote: Some(Seen {
                    size: 1,
                    mtime_ms: 1,
                }),
            },
        );
        let actions = plan(
            &[st(idx, 11, 9000), st(P, 2, 9000)],
            &[st(idx, 12, 9000), st(P, 1, 1)],
            &base,
            NOW,
        );
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].kind, ActionKind::Upload);
        assert_eq!(actions[1].kind, ActionKind::MergeRecords);
        assert_eq!(actions[1].path, idx);
    }

    #[test]
    fn hostile_paths_are_ignored() {
        let bad = [
            st("../escape", 1, 1),
            st("/abs", 1, 1),
            st("Conflicts/x", 1, 1),
            st("Deleted/x", 1, 1),
            st("SaveStates/.hidden", 1, 1),
            st("a//b", 1, 1),
        ];
        assert!(plan(&[], &bad, &Manifest::default(), NOW).is_empty());
    }

    #[test]
    fn manifest_round_trips_including_odd_names() {
        let mut m = Manifest {
            last_sync_ms: 42,
            ..Default::default()
        };
        m.entries.insert(
            "Battery/We\tird\\name.srm".into(),
            BaseEntry {
                local: Some(Seen {
                    size: 3,
                    mtime_ms: -5,
                }),
                remote: None,
            },
        );
        assert_eq!(Manifest::parse(&m.to_text()), m);
        assert_eq!(Manifest::parse("garbage"), Manifest::default());
    }

    #[test]
    fn commit_keeps_the_old_record_for_failures() {
        let previous = base_of(P, Some((1, 1)), Some((1, 1)));
        let local = [st(P, 5, 5), st("Cheats/index.json", 2, 2)];
        let remote = [st(P, 9, 9), st("Cheats/index.json", 2, 3)];
        let touched = [P.to_string(), "Cheats/index.json".to_string()];
        let after = commit(
            &previous,
            &local,
            &remote,
            &local,
            &remote,
            &touched,
            &[P.to_string()],
            NOW,
        );
        assert_eq!(after.entries[P], previous.entries[P]);
        assert_eq!(
            after.entries["Cheats/index.json"].remote,
            Some(Seen {
                size: 2,
                mtime_ms: 3
            })
        );
        assert_eq!(after.last_sync_ms, NOW);
        // After a clean commit the same listing plans nothing.
        let (l, r) = ([st(P, 5, 5)], [st(P, 9, 9)]);
        let clean = commit(
            &Manifest::default(),
            &l,
            &r,
            &l,
            &r,
            &[P.to_string()],
            &[],
            NOW,
        );
        assert!(plan(&l, &r, &clean, NOW).is_empty());
    }

    #[test]
    fn a_file_changed_during_the_sync_still_looks_changed_next_time() {
        let base = base_of(P, Some((1, 1)), Some((1, 1)));
        let before = [st(P, 1, 1)];
        // Nothing to do at plan time; then a core rewrites the file while the sync runs.
        assert!(plan(&before, &before, &base, NOW).is_empty());
        let during = [st(P, 2, 50_000)];
        let next = commit(&base, &before, &before, &during, &before, &[], &[], NOW);
        let again = plan(&during, &before, &next, NOW);
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].kind, ActionKind::Upload);
    }

    fn rec(key: &str, stamp: f64, body: &str) -> Record {
        Record {
            key: key.into(),
            stamp,
            body: body.into(),
        }
    }

    #[test]
    fn records_added_on_both_phones_are_both_kept() {
        let base = [rec("g#1", 1.0, "a")];
        let local = [rec("g#1", 1.0, "a"), rec("g#2", 2.0, "local slot 2")];
        let remote = [rec("g#1", 1.0, "a"), rec("g#3", 3.0, "cloud slot 3")];
        let merged = merge_records(&base, &local, &remote);
        let keys: Vec<&str> = merged.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, ["g#1", "g#2", "g#3"]);
    }

    #[test]
    fn a_record_deleted_on_one_side_and_untouched_on_the_other_is_deleted() {
        let base = [rec("g#1", 1.0, "a"), rec("g#2", 1.0, "b")];
        let local = [rec("g#1", 1.0, "a")];
        let remote = [rec("g#1", 1.0, "a"), rec("g#2", 1.0, "b")];
        assert_eq!(
            merge_records(&base, &local, &remote),
            vec![rec("g#1", 1.0, "a")]
        );
    }

    #[test]
    fn both_edited_the_newer_stamp_wins_and_edit_beats_delete() {
        let base = [rec("g#1", 1.0, "a"), rec("g#2", 1.0, "b")];
        let local = [rec("g#1", 5.0, "local")];
        let remote = [rec("g#1", 9.0, "cloud"), rec("g#2", 7.0, "b edited")];
        let merged = merge_records(&base, &local, &remote);
        assert_eq!(
            merged,
            vec![rec("g#1", 9.0, "cloud"), rec("g#2", 7.0, "b edited")]
        );
    }

    #[test]
    fn dates_are_utc_civil_dates() {
        assert_eq!(date_stamp(0), "1970-01-01 000000 UTC");
        assert_eq!(date_stamp(951_782_400_000), "2000-02-29 000000 UTC");
        assert_eq!(date_stamp(NOW), "2023-11-14 221320 UTC");
    }

    #[test]
    fn conflict_names_keep_the_extension_and_folder() {
        assert_eq!(
            conflict_path("Battery/Zelda.srm", 0, true),
            "Conflicts/Battery/Zelda (conflict 1970-01-01 000000 UTC from this device).srm"
        );
        assert_eq!(
            conflict_path("noext", 0, false),
            "Conflicts/noext (conflict 1970-01-01 000000 UTC from cloud)"
        );
    }

    #[test]
    fn identical_files_compare_equal_and_different_ones_do_not() {
        let dir = std::env::temp_dir().join(format!("continuum-sync-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a");
        let b = dir.join("b");
        let c = dir.join("c");
        std::fs::write(&a, vec![7u8; 200_000]).unwrap();
        std::fs::write(&b, vec![7u8; 200_000]).unwrap();
        let mut other = vec![7u8; 200_000];
        other[150_000] = 8;
        std::fs::write(&c, other).unwrap();
        let s = |p: &std::path::Path| p.to_str().unwrap().to_string();
        assert!(files_identical(&s(&a), &s(&b)));
        assert!(!files_identical(&s(&a), &s(&c)));
        assert!(!files_identical(&s(&a), &s(&dir.join("missing"))));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_empty_cloud_listing_is_refused_not_obeyed() {
        let mut base = Manifest::default();
        for i in 0..6 {
            base.entries.insert(
                format!("SaveStates/{i}.state"),
                BaseEntry {
                    local: Some(Seen {
                        size: 1,
                        mtime_ms: 1,
                    }),
                    remote: Some(Seen {
                        size: 1,
                        mtime_ms: 1,
                    }),
                },
            );
        }
        let local: Vec<FileStat> = (0..6)
            .map(|i| st(&format!("SaveStates/{i}.state"), 1, 1))
            .collect();
        let actions = plan(&local, &[], &base, NOW);
        assert!(refusal(&actions, local.len(), 0, &base)
            .unwrap()
            .contains("looks empty"));
        // One file really removed in the cloud is an ordinary archive.
        let remote: Vec<FileStat> = local[1..].to_vec();
        let actions = plan(&local, &remote, &base, NOW);
        assert_eq!(actions.len(), 1);
        assert!(refusal(&actions, local.len(), remote.len(), &base).is_none());
        // Most of them gone at once is refused even with a non-empty listing.
        let remote: Vec<FileStat> = local[5..].to_vec();
        let actions = plan(&local, &remote, &base, NOW);
        assert!(refusal(&actions, local.len(), remote.len(), &base)
            .unwrap()
            .contains("not reachable"));
    }

    #[test]
    fn the_report_line_is_plain() {
        let report = Report {
            uploaded: 3,
            downloaded: 2,
            conflicts: 1,
            errors: vec!["Battery/x.srm: no space".into()],
            ..Default::default()
        };
        assert_eq!(
            report.line(),
            "3 up, 2 down, 1 conflict (both kept in Conflicts), 1 error (first: Battery/x.srm: no space)"
        );
        assert_eq!(Report::default().line(), "0 up, 0 down, no errors");
    }
}
