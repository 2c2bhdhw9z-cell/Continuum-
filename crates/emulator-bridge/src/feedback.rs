//! Feedback from testers: the half that belongs in the engine, so Android gets the same.
//!
//! - **The activity log.** Every line the app's status text showed, with its time, kept on disk so
//!   it survives the app closing. The last session's log is kept beside it, which is what a crash
//!   report sends: on a sideloaded build there is no other record of what happened.
//! - **The session marker.** Whether the app was on screen when it last stopped. An app that was
//!   on screen and then simply was not running any more crashed, or froze and was force-closed;
//!   leaving it normally takes it off screen first. That is how the next start knows to offer a
//!   crash report.
//! - **The load guard.** A save that was being loaded when the app closed is not loaded again by
//!   itself on the next try: the auto-save resume skips it once, and a slot asks for a second tap.
//!   Without it, a save that crashes the core crashes the app on every start of that game.
//! - **The report.** The subject and text of one message, built the same way on every platform.
//!
//! Files: `activity.txt` (this session), `activity-previous.txt` (the last one), `session.txt`.

use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

/// Lines of the log kept in memory and sent with a report.
pub const LOG_KEEP: usize = 300;
const LOG_FILE: &str = "activity.txt";
const PREVIOUS_FILE: &str = "activity-previous.txt";
const MARKER_FILE: &str = "session.txt";

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

// ------------------------------------------------------------------------ the activity log

#[derive(Default)]
struct Log {
    dir: Option<PathBuf>,
    lines: VecDeque<String>,
    last: String,
    file_lines: usize,
}

static LOG: Mutex<Option<Log>> = Mutex::new(None);

/// Starts this session's log in `dir`. The previous session's log becomes `activity-previous.txt`.
pub fn log_open(dir: &Path) {
    let _ = fs::create_dir_all(dir);
    let current = dir.join(LOG_FILE);
    if current.exists() {
        let _ = fs::rename(&current, dir.join(PREVIOUS_FILE));
    }
    *lock(&LOG) = Some(Log {
        dir: Some(dir.to_path_buf()),
        ..Log::default()
    });
}

/// One line, as `stamp  text`. A repeat of the line before is dropped, and a line break inside one
/// becomes " / ", so a line is always one line.
pub fn log_add(stamp: &str, text: &str) {
    let text = text.trim().replace(['\r', '\n'], " / ");
    if text.is_empty() {
        return;
    }
    let mut guard = lock(&LOG);
    let log = guard.get_or_insert_with(Log::default);
    if log.last == text {
        return;
    }
    log.last = text.clone();
    let entry = if stamp.is_empty() {
        text
    } else {
        format!("{stamp}  {text}")
    };
    log.lines.push_back(entry.clone());
    while log.lines.len() > LOG_KEEP {
        log.lines.pop_front();
    }
    let Some(dir) = log.dir.clone() else {
        return;
    };
    let path = dir.join(LOG_FILE);
    // Appended at once and not batched: the lines that matter most are the ones right before a
    // crash. The file is rewritten from memory once it is twice the size kept.
    if log.file_lines >= 2 * LOG_KEEP {
        let all: Vec<String> = log.lines.iter().cloned().collect();
        let _ = fs::write(&path, all.join("\n") + "\n");
        log.file_lines = all.len();
    } else if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{entry}");
        log.file_lines += 1;
    }
}

/// A launch step, written BEFORE the step runs and flushed to the disk (`fsync`) before this
/// returns, so a crash inside the step leaves it as the last line of the log the next start's
/// crash report sends. Never deduplicated: the same step twice is two attempts.
pub fn launch_step(text: &str) {
    let line = format!("launch: {}", text.trim().replace(['\r', '\n'], " / "));
    {
        let mut guard = lock(&LOG);
        let log = guard.get_or_insert_with(Log::default);
        log.last.clear();
    }
    log_add("", &line);
    let dir = lock(&LOG).as_ref().and_then(|log| log.dir.clone());
    if let Some(dir) = dir {
        if let Ok(file) = OpenOptions::new().append(true).open(dir.join(LOG_FILE)) {
            let _ = file.sync_all();
        }
    }
}

/// A per-frame phase marker (`frame: N phase`) for a session that needs one (PSP). Only the
/// first frames and then every 120th are written, so the log is not flooded; the line is not
/// fsynced (a crash mid-frame still leaves the last one that reached the page cache, and the
/// launch steps before it are fsynced).
pub fn frame_phase(frame: u64, phase: &str) {
    if frame < 4 || frame % 120 == 0 {
        log_add("", &format!("frame: {frame} {phase}"));
    }
}

/// This session's log, oldest line first, at most `limit` lines (the newest ones).
pub fn log_text(limit: usize) -> String {
    let guard = lock(&LOG);
    let Some(log) = guard.as_ref() else {
        return String::new();
    };
    let skip = log.lines.len().saturating_sub(limit);
    log.lines.iter().skip(skip).cloned().collect::<Vec<_>>().join("\n")
}

/// The previous session's log, at most `limit` lines (the newest ones). Empty when there is none.
pub fn log_previous_text(limit: usize) -> String {
    let dir = lock(&LOG).as_ref().and_then(|log| log.dir.clone());
    let Some(dir) = dir else {
        return String::new();
    };
    let text = fs::read_to_string(dir.join(PREVIOUS_FILE)).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(limit)..].join("\n")
}

// ------------------------------------------------------------------------ the session marker

/// What the app was doing, written to disk whenever it changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionMarker {
    /// On screen and in use. False once it goes to the background or the app switcher.
    pub on_screen: bool,
    /// The app's version line, for example "Continuum 0.8.0 (127)".
    pub build: String,
    /// The game running, empty for none.
    pub game: String,
    /// The save being loaded and not yet known to be safe, empty for none.
    pub loading: String,
    /// When this session started, as the host wrote it.
    pub started: String,
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "")
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

impl SessionMarker {
    pub fn encode(&self) -> String {
        format!(
            "on_screen={}\nbuild={}\ngame={}\nloading={}\nstarted={}\n",
            self.on_screen,
            escape(&self.build),
            escape(&self.game),
            escape(&self.loading),
            escape(&self.started)
        )
    }

    /// Unknown keys and bad lines are ignored, so a damaged file reads as "nothing known".
    pub fn decode(text: &str) -> Self {
        let mut marker = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = unescape(value);
            match key {
                "on_screen" => marker.on_screen = value == "true",
                "build" => marker.build = value,
                "game" => marker.game = value,
                "loading" => marker.loading = value,
                "started" => marker.started = value,
                _ => {}
            }
        }
        marker
    }
}

/// The app stopped while it was on screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnexpectedClose {
    pub build: String,
    pub game: String,
    /// The save that was being loaded at the time, empty for none.
    pub loading: String,
    pub started: String,
}

/// Whether the session that wrote `previous` ended unexpectedly.
pub fn judge(previous: &SessionMarker) -> Option<UnexpectedClose> {
    previous.on_screen.then(|| UnexpectedClose {
        build: previous.build.clone(),
        game: previous.game.clone(),
        loading: previous.loading.clone(),
        started: previous.started.clone(),
    })
}

struct Session {
    path: PathBuf,
    marker: SessionMarker,
    /// The load that was in progress when the last session closed unexpectedly. Refused once.
    suspect_load: String,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

fn write_marker(session: &Session) {
    let _ = fs::write(&session.path, session.marker.encode());
}

/// Called once as the app starts. Reads how the last session ended, then starts this one (on
/// screen). Returns the last session when it closed unexpectedly.
pub fn session_open(dir: &Path, build: &str, started: &str) -> Option<UnexpectedClose> {
    let _ = fs::create_dir_all(dir);
    let path = dir.join(MARKER_FILE);
    let previous = fs::read_to_string(&path)
        .map(|text| SessionMarker::decode(&text))
        .unwrap_or_default();
    let verdict = judge(&previous);
    let session = Session {
        path,
        marker: SessionMarker {
            on_screen: true,
            build: build.to_owned(),
            started: started.to_owned(),
            ..SessionMarker::default()
        },
        suspect_load: verdict.as_ref().map(|v| v.loading.clone()).unwrap_or_default(),
    };
    write_marker(&session);
    *lock(&SESSION) = Some(session);
    verdict
}

fn update(change: impl FnOnce(&mut SessionMarker)) {
    let mut guard = lock(&SESSION);
    let Some(session) = guard.as_mut() else {
        return;
    };
    let before = session.marker.clone();
    change(&mut session.marker);
    if session.marker != before {
        write_marker(session);
    }
}

/// The app came on screen (true) or left it (false: the background, the app switcher, a call).
pub fn session_on_screen(on_screen: bool) {
    update(|marker| marker.on_screen = on_screen);
}

/// The game now running, empty for none. A new game ends any load in progress.
pub fn session_game(game: &str) {
    update(|marker| {
        if marker.game != game {
            marker.loading.clear();
        }
        marker.game = game.to_owned();
    });
}

/// A save is about to be loaded. Written to disk before the core sees it.
pub fn load_begin(key: &str) {
    update(|marker| marker.loading = key.to_owned());
}

/// The game ran on after the load, so it was not that save that closed the app.
pub fn load_end() {
    update(|marker| marker.loading.clear());
}

/// True once when `key` was being loaded as the last session closed unexpectedly. The second ask
/// says false, so the player can still load it on purpose.
pub fn load_blocked(key: &str) -> bool {
    let mut guard = lock(&SESSION);
    let Some(session) = guard.as_mut() else {
        return false;
    };
    if !key.is_empty() && session.suspect_load == key {
        session.suspect_load.clear();
        return true;
    }
    false
}

// ------------------------------------------------------------------------ the report

/// One report as the tester filled it in. Empty fields are left out of the message.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// "Problem", "Idea", "Game report", "Crash" and so on, as shown on the form.
    pub kind: String,
    pub message: String,
    /// The tester's name, if they gave one.
    pub tester: String,
    pub game: String,
    pub system: String,
    /// How the game runs: "Perfect", "Playable" and so on.
    pub rating: String,
    /// What is wrong with it: "Picture", "Sound" and so on.
    pub issues: Vec<String>,
    /// The app's details, one fact per line.
    pub details: Vec<String>,
}

/// The subject and the text of the message.
/// Activity lines a crash report puts in its body.
pub const CRASH_BODY_LINES: usize = 50;

pub fn compose(report: &Report) -> (String, String) {
    let kind = if report.kind.trim().is_empty() {
        "Feedback"
    } else {
        report.kind.trim()
    };
    let mut subject = format!("Continuum: {kind}");
    if !report.game.trim().is_empty() {
        subject.push_str(" - ");
        subject.push_str(report.game.trim());
    }

    let mut body: Vec<String> = Vec::new();
    let message = report.message.trim();
    body.push(if message.is_empty() {
        "(no message typed)".to_owned()
    } else {
        message.to_owned()
    });

    let mut facts: Vec<String> = Vec::new();
    if !report.game.trim().is_empty() {
        let mut line = format!("Game: {}", report.game.trim());
        if !report.system.trim().is_empty() {
            line.push_str(&format!(" ({})", report.system.trim()));
        }
        facts.push(line);
    }
    if !report.rating.trim().is_empty() {
        facts.push(format!("How it runs: {}", report.rating.trim()));
    }
    let issues: Vec<&str> = report
        .issues
        .iter()
        .map(|i| i.trim())
        .filter(|i| !i.is_empty())
        .collect();
    if !issues.is_empty() {
        facts.push(format!("What is wrong: {}", issues.join(", ")));
    }
    if !report.tester.trim().is_empty() {
        facts.push(format!("From: {}", report.tester.trim()));
    }
    if !facts.is_empty() {
        body.push(facts.join("\n"));
    }

    let details: Vec<&str> = report
        .details
        .iter()
        .map(|d| d.trim())
        .filter(|d| !d.is_empty())
        .collect();
    if !details.is_empty() {
        body.push(format!("--\n{}", details.join("\n")));
    }
    // A crash report carries the end of the log that ended in the crash in its body too, not
    // only in the attached file: the last `launch:` and `frame:` lines say where it died, and
    // a mail client that drops the attachment must not lose them.
    if kind.to_ascii_lowercase().contains("crash") {
        let tail = log_previous_text(CRASH_BODY_LINES);
        if !tail.trim().is_empty() {
            body.push(format!("-- last {CRASH_BODY_LINES} activity lines --\n{tail}"));
        }
    }
    // A subject is one line: a game file name with a line break in it must not split it.
    let subject = subject.replace(['\r', '\n'], " ");
    (subject, body.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("continuum-feedback-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_crash_is_the_app_stopping_while_on_screen() {
        let _g = lock(&TEST_LOCK);
        let dir = temp_dir("marker");
        // First start ever: nothing to report.
        assert_eq!(session_open(&dir, "Continuum 0.8.0 (127)", "10:00"), None);
        session_game("Mario Kart 7.3ds");
        load_begin("auto:Mario Kart 7.3ds");
        // The app dies here, still on screen. The next start says so, and names the load.
        let verdict = session_open(&dir, "Continuum 0.8.0 (127)", "10:05").unwrap();
        assert_eq!(verdict.game, "Mario Kart 7.3ds");
        assert_eq!(verdict.loading, "auto:Mario Kart 7.3ds");
        assert_eq!(verdict.started, "10:00");
        // That load is refused once, then allowed.
        assert!(load_blocked("auto:Mario Kart 7.3ds"));
        assert!(!load_blocked("auto:Mario Kart 7.3ds"));

        // A normal exit goes off screen first: nothing to report.
        session_on_screen(false);
        assert_eq!(session_open(&dir, "b", "10:10"), None);
        // A load that finished does not count against the save.
        session_game("Zelda.gba");
        load_begin("slot:Zelda.gba#1");
        load_end();
        let verdict = session_open(&dir, "b", "10:15").unwrap();
        assert_eq!(verdict.loading, "");
        assert!(!load_blocked("slot:Zelda.gba#1"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_marker_survives_awkward_text() {
        let marker = SessionMarker {
            on_screen: true,
            build: "Continuum 0.8.0 (127)".into(),
            game: "A=B\\C\nnew line.gba".into(),
            loading: "slot:A=B#3".into(),
            started: "Oct 6, 10:00".into(),
        };
        assert_eq!(SessionMarker::decode(&marker.encode()), marker);
        assert_eq!(SessionMarker::decode("rubbish\n=\non_screen=maybe"), SessionMarker::default());
    }

    #[test]
    fn the_log_keeps_the_newest_lines_and_the_last_session() {
        let _g = lock(&TEST_LOCK);
        let dir = temp_dir("log");
        log_open(&dir);
        log_add("10:00:01", "opening Mario Kart 7");
        log_add("10:00:02", "opening Mario Kart 7");
        log_add("10:00:03", "two\nlines");
        log_add("10:00:04", "   ");
        assert_eq!(log_text(10), "10:00:01  opening Mario Kart 7\n10:00:03  two / lines");
        for i in 0..(3 * LOG_KEEP) {
            log_add("t", &format!("line {i}"));
        }
        let text = log_text(LOG_KEEP + 50);
        assert_eq!(text.lines().count(), LOG_KEEP);
        assert!(text.ends_with(&format!("line {}", 3 * LOG_KEEP - 1)));
        assert_eq!(log_text(2).lines().count(), 2);
        // The next start: this session becomes the previous one, newest lines intact.
        log_open(&dir);
        let previous = log_previous_text(5);
        assert_eq!(previous.lines().count(), 5);
        assert!(previous.ends_with(&format!("line {}", 3 * LOG_KEEP - 1)));
        assert_eq!(log_text(10), "");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_report_reads_as_a_message() {
        let report = Report {
            kind: "Game report".into(),
            message: "  Stutters after each race.  ".into(),
            tester: "Sam".into(),
            game: "Mario Kart 7.3ds".into(),
            system: "Nintendo 3DS".into(),
            rating: "Playable".into(),
            issues: vec!["Speed".into(), " ".into(), "Sound".into()],
            details: vec!["Continuum 0.8.0 (127)".into(), "iPhone: iPhone18,2".into()],
        };
        let (subject, body) = compose(&report);
        assert_eq!(subject, "Continuum: Game report - Mario Kart 7.3ds");
        assert_eq!(
            body,
            "Stutters after each race.\n\nGame: Mario Kart 7.3ds (Nintendo 3DS)\nHow it runs: \
             Playable\nWhat is wrong: Speed, Sound\nFrom: Sam\n\n--\nContinuum 0.8.0 (127)\n\
             iPhone: iPhone18,2"
        );
        let (subject, body) = compose(&Report::default());
        assert_eq!(subject, "Continuum: Feedback");
        assert_eq!(body, "(no message typed)");
        let (subject, _) = compose(&Report {
            game: "Two\nlines.gba".into(),
            ..Report::default()
        });
        assert_eq!(subject, "Continuum: Feedback - Two lines.gba");
    }
}
