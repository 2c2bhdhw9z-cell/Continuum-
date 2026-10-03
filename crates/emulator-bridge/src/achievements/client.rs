//! The `rc_client_t`, behind a safe Rust face. Native builds only (rcheevos is compiled by
//! `build.rs` under the `native-core` feature).
//!
//! ## Re-entrancy, which is the whole difficulty
//!
//! rcheevos calls back into us from inside the calls we make: a login call queues an HTTP request
//! through the server hook before it returns, completing that request fires the login callback,
//! and `rc_client_do_frame` reads memory and raises unlock events. Every hook therefore lands in
//! [`Shared`], behind a `RefCell` in a `Box` whose address is what rcheevos was given as its
//! context, and THE RULE IS: no method here holds a borrow of `Shared` while it calls into C.
//! Hooks use `try_borrow` and drop the work on the floor if that rule is ever broken, because a
//! panic inside an `extern "C"` function aborts the process, and an app that dies on an
//! achievement is worse than one that misses it.
//!
//! The login and load callbacks only RECORD that they finished. The user and game details are
//! read after the outer call has returned, so nothing here asks rc_client a question from inside
//! one of its own callbacks.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void, CStr, CString};

use super::memory_map::{self, ConsoleRegion, MappedBlock};
use super::{bucket_label, AchievementEvent, AchievementInfo, AchievementUser, ServerRequest};

#[repr(C)]
struct RcClient {
    _private: [u8; 0],
}

type ReadHook = unsafe extern "C" fn(*mut c_void, u32, *mut u8, u32) -> u32;
type ServerHook = unsafe extern "C" fn(
    *mut c_void,
    *const c_char,
    *const c_char,
    *const c_char,
    *mut c_void,
    *mut c_void,
);
type AsyncHook = unsafe extern "C" fn(*mut c_void, c_int, c_int, *const c_char);
type EventHook = unsafe extern "C" fn(
    *mut c_void,
    u32,
    u32,
    *const c_char,
    *const c_char,
    u32,
    *const c_char,
    *const c_char,
);
type LogHook = unsafe extern "C" fn(*mut c_void, *const c_char);
type Visitor = unsafe extern "C" fn(
    *mut c_void,
    u32,
    *const c_char,
    *const c_char,
    u32,
    c_int,
    u32,
    *const c_char,
    *const c_char,
    *const c_char,
    i64,
);

// The shim, `src/achievements/rc_shim.c`. Every signature here is mirrored from that file.
extern "C" {
    fn continuum_rc_create(
        rust: *mut c_void,
        read: ReadHook,
        server: ServerHook,
        done: AsyncHook,
        event: EventHook,
        log: LogHook,
    ) -> *mut RcClient;
    fn continuum_rc_destroy(client: *mut RcClient);
    fn continuum_rc_complete(
        callback: *mut c_void,
        callback_data: *mut c_void,
        body: *const c_char,
        body_length: usize,
        http_status: c_int,
    );
    fn continuum_rc_login_password(
        client: *mut RcClient,
        user: *const c_char,
        password: *const c_char,
    );
    fn continuum_rc_login_token(client: *mut RcClient, user: *const c_char, token: *const c_char);
    fn continuum_rc_logout(client: *mut RcClient);
    fn continuum_rc_user(
        client: *mut RcClient,
        username: *mut c_char,
        username_size: usize,
        display_name: *mut c_char,
        display_size: usize,
        token: *mut c_char,
        token_size: usize,
        score: *mut u32,
        score_softcore: *mut u32,
    ) -> c_int;
    fn continuum_rc_load_game(
        client: *mut RcClient,
        console_id: u32,
        path: *const c_char,
        data: *const u8,
        size: usize,
    );
    fn continuum_rc_unload_game(client: *mut RcClient);
    fn continuum_rc_game(
        client: *mut RcClient,
        id: *mut u32,
        title: *mut c_char,
        title_size: usize,
        hash: *mut c_char,
        hash_size: usize,
        badge_url: *mut c_char,
        badge_size: usize,
        core_count: *mut u32,
        unlocked_count: *mut u32,
        points_core: *mut u32,
        points_unlocked: *mut u32,
    ) -> c_int;
    fn continuum_rc_list_achievements(client: *mut RcClient, ctx: *mut c_void, visit: Visitor);
    fn continuum_rc_do_frame(client: *mut RcClient);
    fn continuum_rc_idle(client: *mut RcClient);
    fn continuum_rc_reset(client: *mut RcClient);
    fn continuum_rc_is_game_loaded(client: *mut RcClient) -> c_int;
    fn continuum_rc_get_hardcore(client: *mut RcClient) -> c_int;
    fn continuum_rc_console_region(
        console_id: u32,
        index: u32,
        start: *mut u32,
        end: *mut u32,
        kind: *mut u8,
    ) -> c_int;
    fn continuum_rc_error_str(result: c_int) -> *const c_char;
    fn continuum_rc_user_agent_clause(
        client: *mut RcClient,
        buffer: *mut c_char,
        size: usize,
    ) -> usize;
}

/// `CONTINUUM_RC_ASYNC_*` in the shim.
const ASYNC_LOGIN: c_int = 1;
const ASYNC_LOAD_GAME: c_int = 2;

/// `RC_CLIENT_EVENT_*`, rc_client.h lines 809 to 828.
const EVENT_ACHIEVEMENT_TRIGGERED: u32 = 1;
const EVENT_LEADERBOARD_STARTED: u32 = 2;
const EVENT_LEADERBOARD_FAILED: u32 = 3;
const EVENT_LEADERBOARD_SUBMITTED: u32 = 4;
const EVENT_PROGRESS_SHOW: u32 = 7;
const EVENT_PROGRESS_UPDATE: u32 = 9;
const EVENT_GAME_COMPLETED: u32 = 15;
const EVENT_SERVER_ERROR: u32 = 16;
const EVENT_DISCONNECTED: u32 = 17;
const EVENT_RECONNECTED: u32 = 18;

/// `RC_API_SERVER_RESPONSE_RETRYABLE_CLIENT_ERROR`, rc_api_request.h line 72: what the shell
/// reports when the request never got an HTTP answer (offline, timed out). rcheevos retries it.
pub const RETRYABLE_CLIENT_ERROR: i32 = -2;

/// Everything the hooks write. See the module note on borrowing.
#[derive(Default)]
struct Shared {
    next_id: u64,
    queued: Vec<ServerRequest>,
    /// Request id to rcheevos' `(callback, callback_data)`, as addresses.
    in_flight: HashMap<u64, (usize, usize)>,
    events: Vec<AchievementEvent>,
    finished: Vec<(c_int, c_int, String)>,
    blocks: Vec<MappedBlock>,
    last_log: String,
}

fn text(pointer: *const c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned()
}

fn optional_text(pointer: *const c_char) -> Option<String> {
    let value = text(pointer);
    (!value.is_empty()).then_some(value)
}

fn shared_of<'a>(ctx: *mut c_void) -> Option<&'a RefCell<Shared>> {
    (!ctx.is_null()).then(|| unsafe { &*(ctx as *const RefCell<Shared>) })
}

unsafe extern "C" fn read_hook(ctx: *mut c_void, address: u32, buffer: *mut u8, n: u32) -> u32 {
    let Some(shared) = shared_of(ctx) else {
        return 0;
    };
    let Ok(shared) = shared.try_borrow() else {
        return 0;
    };
    if buffer.is_null() || n == 0 {
        return 0;
    }
    let out = unsafe { std::slice::from_raw_parts_mut(buffer, n as usize) };
    unsafe { memory_map::read(&shared.blocks, address, out) }
}

unsafe extern "C" fn server_hook(
    ctx: *mut c_void,
    url: *const c_char,
    post_data: *const c_char,
    content_type: *const c_char,
    callback: *mut c_void,
    callback_data: *mut c_void,
) {
    let Some(shared) = shared_of(ctx) else {
        return;
    };
    let Ok(mut shared) = shared.try_borrow_mut() else {
        // Breaking the borrow rule would lose this request, and rcheevos would wait on it
        // forever. Answer it as a retryable failure instead, outside any borrow.
        unsafe {
            continuum_rc_complete(callback, callback_data, std::ptr::null(), 0, RETRYABLE_CLIENT_ERROR)
        };
        return;
    };
    shared.next_id += 1;
    let id = shared.next_id;
    shared.queued.push(ServerRequest {
        id,
        url: text(url),
        post_data: optional_text(post_data),
        content_type: optional_text(content_type),
    });
    shared
        .in_flight
        .insert(id, (callback as usize, callback_data as usize));
}

unsafe extern "C" fn async_hook(ctx: *mut c_void, kind: c_int, result: c_int, message: *const c_char) {
    if let Some(Ok(mut shared)) = shared_of(ctx).map(RefCell::try_borrow_mut) {
        shared.finished.push((kind, result, text(message)));
    }
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn event_hook(
    ctx: *mut c_void,
    kind: u32,
    id: u32,
    title: *const c_char,
    description: *const c_char,
    points: u32,
    badge_url: *const c_char,
    detail: *const c_char,
) {
    let event = match kind {
        EVENT_ACHIEVEMENT_TRIGGERED => AchievementEvent::Unlocked {
            id,
            title: text(title),
            description: text(description),
            points,
            badge_url: text(badge_url),
        },
        EVENT_LEADERBOARD_STARTED => AchievementEvent::LeaderboardStarted { title: text(title) },
        EVENT_LEADERBOARD_FAILED => AchievementEvent::LeaderboardFailed { title: text(title) },
        EVENT_LEADERBOARD_SUBMITTED => AchievementEvent::LeaderboardSubmitted {
            title: text(title),
            score: text(detail),
        },
        EVENT_PROGRESS_SHOW | EVENT_PROGRESS_UPDATE => AchievementEvent::Progress {
            id,
            title: text(title),
            progress: text(detail),
        },
        EVENT_GAME_COMPLETED => AchievementEvent::GameCompleted,
        EVENT_SERVER_ERROR => AchievementEvent::ServerError {
            api: text(title),
            message: text(description),
        },
        EVENT_DISCONNECTED => AchievementEvent::Disconnected,
        EVENT_RECONNECTED => AchievementEvent::Reconnected,
        // Challenge indicators, leaderboard trackers and the hardcore-only reset have nothing to
        // show in this app yet.
        _ => return,
    };
    if let Some(Ok(mut shared)) = shared_of(ctx).map(RefCell::try_borrow_mut) {
        shared.events.push(event);
    }
}

unsafe extern "C" fn log_hook(ctx: *mut c_void, message: *const c_char) {
    let line = text(message);
    log::info!("rcheevos: {line}");
    if let Some(Ok(mut shared)) = shared_of(ctx).map(RefCell::try_borrow_mut) {
        shared.last_log = line;
    }
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn visit_achievement(
    ctx: *mut c_void,
    id: u32,
    title: *const c_char,
    description: *const c_char,
    points: u32,
    unlocked: c_int,
    bucket: u32,
    badge_url: *const c_char,
    badge_locked_url: *const c_char,
    measured_progress: *const c_char,
    unlock_time: i64,
) {
    if ctx.is_null() {
        return;
    }
    let list = unsafe { &mut *(ctx as *mut Vec<AchievementInfo>) };
    list.push(AchievementInfo {
        id,
        title: text(title),
        description: text(description),
        points,
        unlocked: unlocked != 0,
        bucket: bucket_label(bucket).to_string(),
        badge_url: text(badge_url),
        badge_locked_url: text(badge_locked_url),
        measured_progress: text(measured_progress),
        unlock_time,
    });
}

fn error_text(result: c_int) -> String {
    text(unsafe { continuum_rc_error_str(result) })
}

/// The console's achievement address map, read once per game from rcheevos.
pub fn console_regions(console_id: u32) -> Vec<ConsoleRegion> {
    let mut regions = Vec::new();
    for index in 0..64 {
        let (mut start, mut end, mut kind) = (0u32, 0u32, 0u8);
        if unsafe { continuum_rc_console_region(console_id, index, &mut start, &mut end, &mut kind) }
            == 0
        {
            break;
        }
        regions.push(ConsoleRegion { start, end, kind });
    }
    regions
}

/// One `rc_client_t` and everything it has said.
pub struct Achievements {
    client: *mut RcClient,
    /// Boxed so its address, which rcheevos holds, never moves.
    shared: Box<RefCell<Shared>>,
    console: Vec<ConsoleRegion>,
}

// SAFETY: `rc_client_t` has no thread affinity (it guards itself with its own mutex), and this
// value is only ever reached through the engine's `Mutex<EmulatorBridge>`, so the `RefCell` and the
// raw block pointers are never touched from two threads at once.
unsafe impl Send for Achievements {}

impl Achievements {
    pub fn new() -> Result<Self, String> {
        let shared = Box::new(RefCell::new(Shared::default()));
        let ctx = &*shared as *const RefCell<Shared> as *mut c_void;
        let client = unsafe {
            continuum_rc_create(ctx, read_hook, server_hook, async_hook, event_hook, log_hook)
        };
        if client.is_null() {
            return Err("rcheevos could not create its client (out of memory)".into());
        }
        Ok(Self {
            client,
            shared,
            console: Vec::new(),
        })
    }

    fn cstring(value: &str, what: &str) -> Result<CString, String> {
        CString::new(value).map_err(|_| format!("the {what} contains a NUL character"))
    }

    /// Turns every finished login or load into an event. Runs after the outer call returned.
    fn settle(&mut self) {
        let finished = std::mem::take(&mut self.shared.borrow_mut().finished);
        for (kind, result, message) in finished {
            let event = match (kind, result) {
                (ASYNC_LOGIN, 0) => match self.user_with_token() {
                    Some((user, token)) => AchievementEvent::LoginSucceeded {
                        username: user.username,
                        display_name: user.display_name,
                        token,
                        score: user.score,
                        score_softcore: user.score_softcore,
                    },
                    None => AchievementEvent::LoginFailed {
                        message: "the server accepted the login but sent no user".into(),
                    },
                },
                (ASYNC_LOGIN, _) => AchievementEvent::LoginFailed {
                    message: if message.is_empty() { error_text(result) } else { message },
                },
                (ASYNC_LOAD_GAME, 0) => match self.game_event() {
                    Some(event) => event,
                    None => AchievementEvent::GameLoadFailed {
                        message: "the game loaded and then reported no details".into(),
                    },
                },
                (ASYNC_LOAD_GAME, _) => AchievementEvent::GameLoadFailed {
                    message: if message.is_empty() { error_text(result) } else { message },
                },
                _ => continue,
            };
            self.shared.borrow_mut().events.push(event);
        }
    }

    /// Starts a login with a password. The password is handed to rcheevos for this one request
    /// and is not kept anywhere in Rust.
    pub fn login_with_password(&mut self, username: &str, password: &str) -> Result<(), String> {
        let user = Self::cstring(username, "username")?;
        let pass = Self::cstring(password, "password")?;
        unsafe { continuum_rc_login_password(self.client, user.as_ptr(), pass.as_ptr()) };
        self.settle();
        Ok(())
    }

    /// Starts a login with the token a previous password login returned.
    pub fn login_with_token(&mut self, username: &str, token: &str) -> Result<(), String> {
        let user = Self::cstring(username, "username")?;
        let token = Self::cstring(token, "token")?;
        unsafe { continuum_rc_login_token(self.client, user.as_ptr(), token.as_ptr()) };
        self.settle();
        Ok(())
    }

    pub fn logout(&mut self) {
        unsafe { continuum_rc_logout(self.client) };
        self.settle();
    }

    fn user_with_token(&self) -> Option<(AchievementUser, String)> {
        let mut username = [0 as c_char; 256];
        let mut display = [0 as c_char; 256];
        let mut token = [0 as c_char; 256];
        let (mut score, mut softcore) = (0u32, 0u32);
        let found = unsafe {
            continuum_rc_user(
                self.client,
                username.as_mut_ptr(),
                username.len(),
                display.as_mut_ptr(),
                display.len(),
                token.as_mut_ptr(),
                token.len(),
                &mut score,
                &mut softcore,
            )
        };
        (found != 0).then(|| {
            (
                AchievementUser {
                    username: text(username.as_ptr()),
                    display_name: text(display.as_ptr()),
                    score,
                    score_softcore: softcore,
                },
                text(token.as_ptr()),
            )
        })
    }

    /// The logged-in user, without the token.
    pub fn user(&self) -> Option<AchievementUser> {
        self.user_with_token().map(|(user, _)| user)
    }

    fn game_event(&self) -> Option<AchievementEvent> {
        let mut title = [0 as c_char; 512];
        let mut hash = [0 as c_char; 64];
        let mut badge = [0 as c_char; 512];
        let (mut id, mut total, mut unlocked, mut points, mut points_unlocked) = (0, 0, 0, 0, 0);
        let found = unsafe {
            continuum_rc_game(
                self.client,
                &mut id,
                title.as_mut_ptr(),
                title.len(),
                hash.as_mut_ptr(),
                hash.len(),
                badge.as_mut_ptr(),
                badge.len(),
                &mut total,
                &mut unlocked,
                &mut points,
                &mut points_unlocked,
            )
        };
        (found != 0).then(|| AchievementEvent::GameLoaded {
            game_id: id,
            title: text(title.as_ptr()),
            hash: text(hash.as_ptr()),
            badge_url: text(badge.as_ptr()),
            total,
            unlocked,
            points_total: points,
            points_unlocked,
        })
    }

    /// The loaded game's summary, the same shape as the `GameLoaded` event.
    pub fn game(&self) -> Option<AchievementEvent> {
        self.game_event()
    }

    /// Identifies the game by hashing it and loads its achievement set.
    ///
    /// `system` is Continuum's system id; `path` is the file (rcheevos opens it itself, which is
    /// what disc images need); `data` may be empty.
    pub fn load_game(&mut self, system: &str, path: &str, data: &[u8]) -> Result<(), String> {
        let console = super::console::console_id(system)
            .ok_or_else(|| format!("RetroAchievements has no console for the system '{system}'"))?;
        let path = Self::cstring(path, "game path")?;
        self.console = console_regions(console);
        let (pointer, len) = if data.is_empty() {
            (std::ptr::null(), 0)
        } else {
            (data.as_ptr(), data.len())
        };
        unsafe { continuum_rc_load_game(self.client, console, path.as_ptr(), pointer, len) };
        self.settle();
        Ok(())
    }

    pub fn unload_game(&mut self) {
        unsafe { continuum_rc_unload_game(self.client) };
        self.shared.borrow_mut().blocks.clear();
        self.settle();
    }

    pub fn is_game_loaded(&self) -> bool {
        unsafe { continuum_rc_is_game_loaded(self.client) != 0 }
    }

    pub fn hardcore(&self) -> bool {
        unsafe { continuum_rc_get_hardcore(self.client) != 0 }
    }

    /// Every achievement of the loaded game, in rc_client's progress grouping.
    pub fn achievements(&self) -> Vec<AchievementInfo> {
        let mut list: Vec<AchievementInfo> = Vec::new();
        unsafe {
            continuum_rc_list_achievements(
                self.client,
                &mut list as *mut Vec<AchievementInfo> as *mut c_void,
                visit_achievement,
            )
        };
        list
    }

    /// Requests waiting to be sent. Each is handed out once.
    pub fn take_requests(&mut self) -> Vec<ServerRequest> {
        std::mem::take(&mut self.shared.borrow_mut().queued)
    }

    /// Hands a response back to rcheevos. `http_status` is the HTTP status code, or
    /// [`RETRYABLE_CLIENT_ERROR`] when there was no response at all.
    pub fn complete_request(&mut self, id: u64, http_status: i32, body: &[u8]) -> Result<(), String> {
        let entry = self.shared.borrow_mut().in_flight.remove(&id);
        let Some((callback, data)) = entry else {
            return Err(format!("no request {id} is waiting for an answer"));
        };
        // The borrow above is gone: completing can queue the next request, which borrows again.
        unsafe {
            continuum_rc_complete(
                callback as *mut c_void,
                data as *mut c_void,
                body.as_ptr().cast::<c_char>(),
                body.len(),
                http_status,
            )
        };
        self.settle();
        Ok(())
    }

    /// Requests handed out and not yet answered.
    pub fn requests_in_flight(&self) -> usize {
        self.shared.borrow().in_flight.len()
    }

    pub fn take_events(&mut self) -> Vec<AchievementEvent> {
        std::mem::take(&mut self.shared.borrow_mut().events)
    }

    /// Points the memory reads at the core's regions as they are right now.
    pub fn set_memory(&mut self, core_region: impl Fn(u32) -> Option<(*const u8, usize)>) {
        let blocks = memory_map::build(&self.console, core_region);
        self.shared.borrow_mut().blocks = blocks;
    }

    /// Drops every memory pointer, for when the core is about to go away.
    pub fn clear_memory(&mut self) {
        self.shared.borrow_mut().blocks.clear();
    }

    /// One emulated frame: evaluate every active achievement.
    pub fn do_frame(&mut self) {
        unsafe { continuum_rc_do_frame(self.client) };
        self.settle();
    }

    /// Keeps the session alive while paused (rich presence pings, retries).
    pub fn idle(&mut self) {
        unsafe { continuum_rc_idle(self.client) };
        self.settle();
    }

    /// The emulated machine was reset or jumped in time (state load, rewind).
    pub fn reset(&mut self) {
        unsafe { continuum_rc_reset(self.client) };
        self.settle();
    }

    /// "rcheevos/12.5" or similar, for the User-Agent header.
    pub fn user_agent_clause(&self) -> String {
        let mut buffer = [0 as c_char; 64];
        unsafe { continuum_rc_user_agent_clause(self.client, buffer.as_mut_ptr(), buffer.len()) };
        text(buffer.as_ptr())
    }

    pub fn last_log(&self) -> String {
        self.shared.borrow().last_log.clone()
    }
}

impl Drop for Achievements {
    fn drop(&mut self) {
        // Outstanding requests are abandoned: rc_client_destroy frees their state, and the map of
        // callbacks dies with `shared` right after, so a late response has nothing to call.
        unsafe { continuum_rc_destroy(self.client) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn login(achievements: &mut Achievements) -> ServerRequest {
        achievements.login_with_password("bob", "hunter2").unwrap();
        let mut requests = achievements.take_requests();
        assert_eq!(requests.len(), 1, "a login is one request");
        requests.remove(0)
    }

    #[test]
    fn creates_softcore_with_no_user_and_no_game() {
        let achievements = Achievements::new().unwrap();
        assert!(!achievements.hardcore(), "hardcore must be off by default");
        assert!(achievements.user().is_none());
        assert!(!achievements.is_game_loaded());
        assert!(achievements.achievements().is_empty());
        assert!(achievements.user_agent_clause().starts_with("rcheevos/"));
    }

    #[test]
    fn a_login_is_a_queued_request_and_a_good_answer_yields_a_token() {
        let mut achievements = Achievements::new().unwrap();
        let request = login(&mut achievements);
        assert!(request.url.contains("retroachievements.org"), "{}", request.url);
        let post = request.post_data.clone().unwrap_or_default();
        assert!(post.contains("r=login"), "{post}");
        assert!(post.contains("u=bob"), "{post}");
        assert_eq!(achievements.requests_in_flight(), 1);
        // Handed out once.
        assert!(achievements.take_requests().is_empty());

        let body = br#"{"Success":true,"User":"bob","DisplayName":"Bob","Token":"tok123","Score":120,"SoftcoreScore":30,"Messages":0}"#;
        achievements.complete_request(request.id, 200, body).unwrap();
        assert_eq!(achievements.requests_in_flight(), 0);
        let events = achievements.take_events();
        match &events[..] {
            [AchievementEvent::LoginSucceeded {
                username,
                token,
                score,
                score_softcore,
                ..
            }] => {
                assert_eq!(username, "bob");
                assert_eq!(token, "tok123");
                assert_eq!((*score, *score_softcore), (120, 30));
            }
            other => panic!("expected one login success, got {other:?}"),
        }
        assert_eq!(achievements.user().unwrap().username, "bob");
    }

    #[test]
    fn a_rejected_login_says_why() {
        let mut achievements = Achievements::new().unwrap();
        let request = login(&mut achievements);
        let body = br#"{"Success":false,"Error":"Invalid User/Password combination. Please try again","Code":"invalid_credentials"}"#;
        achievements.complete_request(request.id, 401, body).unwrap();
        match &achievements.take_events()[..] {
            [AchievementEvent::LoginFailed { message }] => {
                assert!(message.contains("Invalid"), "{message}");
            }
            other => panic!("expected one login failure, got {other:?}"),
        }
        assert!(achievements.user().is_none());
    }

    #[test]
    fn answering_an_unknown_request_is_refused() {
        let mut achievements = Achievements::new().unwrap();
        assert!(achievements.complete_request(99, 200, b"{}").is_err());
    }

    #[test]
    fn token_login_posts_the_token_not_a_password() {
        let mut achievements = Achievements::new().unwrap();
        achievements.login_with_token("bob", "tok123").unwrap();
        let request = achievements.take_requests().remove(0);
        let post = request.post_data.unwrap_or_default();
        assert!(post.contains("t=tok123"), "{post}");
        assert!(!post.contains("p="), "{post}");
    }

    #[test]
    fn loading_a_game_needs_a_known_system() {
        let mut achievements = Achievements::new().unwrap();
        assert!(achievements.load_game("switch", "/nope", &[]).is_err());
    }

    #[test]
    fn loading_a_game_hashes_it_and_asks_the_server() {
        let mut achievements = Achievements::new().unwrap();
        let request = login(&mut achievements);
        let body = br#"{"Success":true,"User":"bob","Token":"tok","Score":0,"SoftcoreScore":0,"Messages":0}"#;
        achievements.complete_request(request.id, 200, body).unwrap();
        achievements.take_events();

        // Sixteen bytes of iNES header and a little PRG, handed over in memory.
        let mut rom = b"NES\x1a\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00".to_vec();
        rom.extend(std::iter::repeat_n(0xEA, 16 * 1024));
        achievements.load_game("nes", "game.nes", &rom).unwrap();
        let requests = achievements.take_requests();
        assert!(!requests.is_empty(), "identifying a game asks the server");
        // The server does not know it: the load fails with a reason, not silently.
        for request in requests {
            let body = br#"{"Success":false,"Error":"Unknown game","Code":"not_found"}"#;
            achievements.complete_request(request.id, 404, body).unwrap();
        }
        // Drain any follow-up requests the same way.
        for _ in 0..4 {
            for request in achievements.take_requests() {
                achievements
                    .complete_request(request.id, 404, br#"{"Success":false,"Error":"Unknown game"}"#)
                    .unwrap();
            }
        }
        let events = achievements.take_events();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, AchievementEvent::GameLoadFailed { .. })),
            "{events:?}"
        );
        assert!(!achievements.is_game_loaded());
    }

    #[test]
    fn memory_reads_go_through_the_console_map() {
        let mut achievements = Achievements::new().unwrap();
        // NES (console 7): rcheevos maps $0000-$07FF to system RAM.
        achievements.console = console_regions(7);
        assert!(!achievements.console.is_empty());
        assert_eq!(achievements.console[0].start, 0);
        let ram: Vec<u8> = (0..2048u32).map(|i| (i % 251) as u8).collect();
        achievements.set_memory(|id| {
            (id == crate::memory::MEMORY_SYSTEM_RAM).then_some((ram.as_ptr(), ram.len()))
        });
        let ctx = &*achievements.shared as *const RefCell<Shared> as *mut c_void;
        let mut out = [0u8; 2];
        let read = unsafe { read_hook(ctx, 0x0100, out.as_mut_ptr(), 2) };
        assert_eq!(read, 2);
        assert_eq!(out, [(0x100 % 251) as u8, (0x101 % 251) as u8]);
        achievements.clear_memory();
        assert_eq!(unsafe { read_hook(ctx, 0x0100, out.as_mut_ptr(), 2) }, 0);
    }

    #[test]
    fn console_tables_come_from_rcheevos() {
        // The SNES table starts with work RAM at zero and has a save RAM region after it.
        let snes = console_regions(3);
        assert!(snes.len() >= 2, "{snes:?}");
        assert_eq!(snes[0].start, 0);
        assert_eq!(snes[0].kind, memory_map::RC_MEMORY_TYPE_SYSTEM_RAM);
        assert!(snes
            .iter()
            .any(|r| r.kind == memory_map::RC_MEMORY_TYPE_SAVE_RAM));
        assert!(console_regions(9999).is_empty());
    }
}
