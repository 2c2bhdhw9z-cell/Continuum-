//! The lockstep state machine for one two-player session.
//!
//! Pure: no sockets, no clock, no core. Bytes go in through [`NetplaySession::receive`], bytes
//! come out of [`NetplaySession::take_outgoing`], time arrives as an argument to
//! [`NetplaySession::poll`], and the two things only the engine can do (serialize the running
//! core, load a state into it) are handed out as [`Request`]s. That is what makes it testable
//! with two peers in one process, and what lets Android carry the same bytes.
//!
//! ## The rule
//!
//! Frame `f` runs only when BOTH players' inputs for frame `f` are known. Input sampled while
//! frame `f` is current is assigned to frame `f + delay` and sent immediately, so in the normal
//! case the other player's input for a frame has arrived before that frame is due and nobody
//! waits. If it has not, the frame does not run: the engine stalls rather than guessing, because
//! a guess that turns out wrong is a desync, and lockstep without rollback cannot repair one.
//! The first `delay` frames have no sampled input on either side; both peers treat them as
//! "nothing pressed", which is the same on both sides by construction.
//!
//! Host is player 1 (port 0), guest is player 2 (port 1), on both phones.

use std::collections::{BTreeMap, VecDeque};

use super::wire::{Decoder, Message, PeerInfo, WireInput, PROTOCOL_VERSION, STATE_CHUNK_BYTES};

/// Default frames of input delay. Two frames at 60 fps is 33 ms of headroom, enough for a home
/// Wi-Fi round trip, and barely noticeable in play.
pub const DEFAULT_INPUT_DELAY: u8 = 2;
/// Upper bound on the delay a host may ask for. Past this the game feels broken anyway.
pub const MAX_INPUT_DELAY: u8 = 15;
/// Frames between state checksums. Once a second at 60 fps.
pub const DEFAULT_CHECKSUM_INTERVAL: u32 = 60;
/// Silence from the other peer for this long ends the session.
pub const DEFAULT_TIMEOUT_MS: f64 = 10_000.0;
/// Keepalive and round-trip measurement.
const PING_INTERVAL_MS: f64 = 1_000.0;
/// A stall shorter than this is ordinary network jitter and is not reported as a stall.
const STALL_REPORT_MS: f64 = 150.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Host,
    Guest,
}

impl Role {
    pub const fn player_number(self) -> u32 {
        match self {
            Role::Host => 1,
            Role::Guest => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Host: listening. Guest: the connection is being opened.
    WaitingForPeer,
    /// Connected, the guest's Hello is being checked.
    Handshaking,
    /// The starting state is being sent, received or loaded.
    Syncing,
    Running,
    Disconnected,
}

/// What a UI shows. Derived, with the most serious condition winning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    Waiting,
    Connected,
    Syncing,
    Running,
    Stalled,
    Desynced,
    Disconnected,
}

#[derive(Debug, Clone, Copy)]
pub struct NetplayConfig {
    pub input_delay: u8,
    /// `0` turns checksums off.
    pub checksum_interval: u32,
    pub timeout_ms: f64,
}

impl Default for NetplayConfig {
    fn default() -> Self {
        Self {
            input_delay: DEFAULT_INPUT_DELAY,
            checksum_interval: DEFAULT_CHECKSUM_INTERVAL,
            timeout_ms: DEFAULT_TIMEOUT_MS,
        }
    }
}

/// Work only the engine can do, handed out by the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Host: serialize the running core, load that same state locally, then call
    /// [`NetplaySession::host_state_captured`].
    CaptureState,
    /// Guest: load this state, then call [`NetplaySession::guest_state_loaded`].
    LoadState(Vec<u8>),
}

/// What the engine may do this step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Run one frame with these inputs: `[player 1, player 2]`.
    Run([WireInput; 2]),
    /// The other player's input for the current frame has not arrived. Do not run.
    Stall,
    /// Not in the running phase. Do not run.
    Wait,
}

pub struct NetplaySession {
    role: Role,
    phase: Phase,
    config: NetplayConfig,
    local_info: PeerInfo,
    peer_name: Option<String>,

    decoder: Decoder,
    outgoing: Vec<u8>,
    requests: VecDeque<Request>,
    transport_connected: bool,

    /// Frames completed in lockstep.
    frame: u64,
    /// Next frame local input will be assigned to.
    local_next: u64,
    /// Next frame the remote's input is expected for. TCP preserves order, so anything else is
    /// a protocol error.
    remote_next: u64,
    local_inputs: BTreeMap<u64, WireInput>,
    remote_inputs: BTreeMap<u64, WireInput>,

    local_checks: BTreeMap<u64, u64>,
    remote_checks: BTreeMap<u64, u64>,
    checks_compared: u64,
    desync_frame: Option<u64>,

    stalled_since: Option<f64>,
    stall_pending: bool,
    stall_frames: u64,

    incoming_state: Vec<u8>,
    expected_state_len: u64,
    expected_state_hash: u64,
    outgoing_state_len: u64,

    now_ms: f64,
    /// First `poll` time, for the guest's connect deadline.
    started_ms: Option<f64>,
    last_rx_ms: f64,
    rx_since_poll: bool,
    last_ping_ms: f64,
    rtt_ms: Option<f64>,
    disconnect_reason: Option<String>,
}

impl NetplaySession {
    fn new(role: Role, config: NetplayConfig, local_info: PeerInfo) -> Self {
        let config = NetplayConfig {
            input_delay: config.input_delay.min(MAX_INPUT_DELAY),
            ..config
        };
        Self {
            role,
            phase: Phase::WaitingForPeer,
            config,
            local_info,
            peer_name: None,
            decoder: Decoder::new(),
            outgoing: Vec::new(),
            requests: VecDeque::new(),
            transport_connected: false,
            frame: 0,
            local_next: 0,
            remote_next: 0,
            local_inputs: BTreeMap::new(),
            remote_inputs: BTreeMap::new(),
            local_checks: BTreeMap::new(),
            remote_checks: BTreeMap::new(),
            checks_compared: 0,
            desync_frame: None,
            stalled_since: None,
            stall_pending: false,
            stall_frames: 0,
            incoming_state: Vec::new(),
            expected_state_len: 0,
            expected_state_hash: 0,
            outgoing_state_len: 0,
            now_ms: 0.0,
            started_ms: None,
            last_rx_ms: 0.0,
            rx_since_poll: false,
            last_ping_ms: f64::NEG_INFINITY,
            rtt_ms: None,
            disconnect_reason: None,
        }
    }

    /// The host decides the delay and checksum interval; the guest is told them.
    pub fn new_host(config: NetplayConfig, local_info: PeerInfo) -> Self {
        Self::new(Role::Host, config, local_info)
    }

    pub fn new_guest(local_info: PeerInfo) -> Self {
        Self::new(Role::Guest, NetplayConfig::default(), local_info)
    }

    // ------------------------------------------------------------ accessors

    pub fn role(&self) -> Role {
        self.role
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn frame(&self) -> u64 {
        self.frame
    }
    pub fn input_delay(&self) -> u8 {
        self.config.input_delay
    }
    pub fn checksum_interval(&self) -> u32 {
        self.config.checksum_interval
    }
    pub fn desync_frame(&self) -> Option<u64> {
        self.desync_frame
    }
    pub fn rtt_ms(&self) -> Option<f64> {
        self.rtt_ms
    }
    pub fn checks_compared(&self) -> u64 {
        self.checks_compared
    }
    pub fn stall_frames(&self) -> u64 {
        self.stall_frames
    }
    pub fn disconnect_reason(&self) -> Option<&str> {
        self.disconnect_reason.as_deref()
    }
    pub fn peer_name(&self) -> Option<&str> {
        self.peer_name.as_deref()
    }
    /// How long the current stall has lasted, `0` when not stalled.
    pub fn stall_ms(&self) -> f64 {
        self.stalled_since
            .map_or(0.0, |since| (self.now_ms - since).max(0.0))
    }

    /// True from the moment a peer could be affected by local actions until the session ends:
    /// what gates rewind, fast forward and state loading.
    pub fn is_live(&self) -> bool {
        self.phase != Phase::Disconnected
    }

    pub fn status_kind(&self) -> StatusKind {
        match self.phase {
            Phase::Disconnected => StatusKind::Disconnected,
            _ if self.desync_frame.is_some() => StatusKind::Desynced,
            Phase::WaitingForPeer => StatusKind::Waiting,
            Phase::Handshaking => StatusKind::Connected,
            Phase::Syncing => StatusKind::Syncing,
            Phase::Running if self.stall_ms() >= STALL_REPORT_MS => StatusKind::Stalled,
            Phase::Running => StatusKind::Running,
        }
    }

    /// One plain line for the HUD. Every state has its own wording.
    pub fn status_line(&self) -> String {
        let other = match self.role {
            Role::Host => "player 2",
            Role::Guest => "the host",
        };
        match self.status_kind() {
            StatusKind::Disconnected => format!(
                "online: disconnected: {}",
                self.disconnect_reason.as_deref().unwrap_or("the session ended")
            ),
            StatusKind::Desynced => format!(
                "online: DESYNC at frame {}: the two games no longer match. Leave online play and host again",
                self.desync_frame.unwrap_or(0)
            ),
            StatusKind::Waiting => match self.role {
                Role::Host => "online: waiting for player 2 to join".to_string(),
                Role::Guest => "online: connecting to the host".to_string(),
            },
            StatusKind::Connected => match self.role {
                Role::Host => "online: player 2 connected, checking they have the same game".to_string(),
                Role::Guest => "online: connected, waiting for the host to accept".to_string(),
            },
            StatusKind::Syncing => match self.role {
                Role::Host if self.requests.contains(&Request::CaptureState) => {
                    "online: syncing: saving the starting state".to_string()
                }
                Role::Host => format!(
                    "online: syncing: sent the starting state ({} KB), waiting for player 2 to load it",
                    self.outgoing_state_len.div_ceil(1024)
                ),
                Role::Guest => format!(
                    "online: syncing: receiving the starting state ({} of {} KB)",
                    (self.incoming_state.len() as u64).div_ceil(1024),
                    self.expected_state_len.div_ceil(1024)
                ),
            },
            StatusKind::Stalled => format!(
                "online: stalled {:.1} s waiting for {other}'s input (frame {})",
                self.stall_ms() / 1000.0,
                self.frame
            ),
            StatusKind::Running => {
                let ping = self
                    .rtt_ms
                    .map_or_else(|| "ping --".to_string(), |rtt| format!("ping {rtt:.0} ms"));
                format!(
                    "online: player {}, delay {} frames, {ping}, frame {}",
                    self.role.player_number(),
                    self.config.input_delay,
                    self.frame
                )
            }
        }
    }

    // ------------------------------------------------------------ transport

    /// The socket is open. The guest introduces itself; the host waits to be introduced to.
    pub fn transport_connected(&mut self) {
        if self.phase == Phase::Disconnected {
            return;
        }
        self.transport_connected = true;
        self.rx_since_poll = true;
        if self.phase == Phase::WaitingForPeer {
            self.phase = Phase::Handshaking;
            if self.role == Role::Guest {
                let mut hello = self.local_info.clone();
                hello.version = PROTOCOL_VERSION;
                self.send(&Message::Hello(hello));
            }
        }
    }

    /// The socket failed or closed underneath us.
    pub fn transport_lost(&mut self, reason: &str) {
        self.transport_connected = false;
        self.end(&format!("the connection was lost ({reason})"), false);
    }

    /// Ends the session from this side, telling the peer why.
    pub fn leave(&mut self, reason: &str) {
        self.end(reason, true);
    }

    fn end(&mut self, reason: &str, tell_peer: bool) {
        if self.phase == Phase::Disconnected {
            return;
        }
        if tell_peer && self.transport_connected {
            self.send(&Message::Bye {
                reason: reason.to_string(),
            });
        }
        self.phase = Phase::Disconnected;
        self.disconnect_reason = Some(reason.to_string());
        self.requests.clear();
        self.stalled_since = None;
    }

    pub fn take_outgoing(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.outgoing)
    }

    pub fn has_outgoing(&self) -> bool {
        !self.outgoing.is_empty()
    }

    pub fn take_request(&mut self) -> Option<Request> {
        self.requests.pop_front()
    }

    fn send(&mut self, message: &Message) {
        message.encode_into(&mut self.outgoing);
    }

    /// Bytes from the other peer, in whatever pieces the transport delivered them.
    pub fn receive(&mut self, bytes: &[u8]) {
        if self.phase == Phase::Disconnected {
            return;
        }
        self.rx_since_poll = true;
        self.decoder.push(bytes);
        loop {
            match self.decoder.next_message() {
                Ok(Some(message)) => {
                    self.handle(message);
                    if self.phase == Phase::Disconnected {
                        return;
                    }
                }
                Ok(None) => return,
                Err(err) => {
                    self.end(
                        &format!("the other phone sent something unreadable ({err})"),
                        true,
                    );
                    return;
                }
            }
        }
    }

    /// Keepalive, round trip and timeout. Call every tick.
    pub fn poll(&mut self, now_ms: f64) {
        self.now_ms = now_ms;
        if self.rx_since_poll {
            self.rx_since_poll = false;
            self.last_rx_ms = now_ms;
        }
        if self.stall_pending {
            self.stall_pending = false;
            if self.stalled_since.is_none() {
                self.stalled_since = Some(now_ms);
            }
        }
        let started = *self.started_ms.get_or_insert(now_ms);
        if self.role == Role::Guest
            && self.phase == Phase::WaitingForPeer
            && now_ms - started > self.config.timeout_ms
        {
            let seconds = (self.config.timeout_ms / 1000.0).round();
            self.end(
                &format!("could not reach the host within {seconds} s; check the address and that both phones are on the same network"),
                false,
            );
            return;
        }
        if self.phase == Phase::Disconnected || !self.transport_connected {
            return;
        }
        if now_ms - self.last_ping_ms >= PING_INTERVAL_MS {
            self.last_ping_ms = now_ms;
            self.send(&Message::Ping {
                token: now_ms.to_bits(),
            });
        }
        if now_ms - self.last_rx_ms > self.config.timeout_ms {
            let seconds = (self.config.timeout_ms / 1000.0).round();
            self.end(
                &format!("nothing heard from the other phone for {seconds} s"),
                true,
            );
        }
    }

    fn handle(&mut self, message: Message) {
        match message {
            Message::Ping { token } => self.send(&Message::Pong { token }),
            Message::Pong { token } => {
                let sent = f64::from_bits(token);
                if sent.is_finite() && self.now_ms >= sent {
                    self.rtt_ms = Some(self.now_ms - sent);
                }
            }
            Message::Bye { reason } => {
                self.transport_connected = false;
                self.end(&format!("the other player left ({reason})"), false);
            }
            Message::Reject { reason } => {
                self.transport_connected = false;
                self.end(&format!("refused: {reason}"), false);
            }
            Message::Hello(info) => self.on_hello(info),
            Message::Welcome {
                input_delay,
                checksum_interval,
                state_len,
                state_hash,
            } => self.on_welcome(input_delay, checksum_interval, state_len, state_hash),
            Message::StateChunk { offset, bytes } => self.on_chunk(offset, &bytes),
            Message::Ready => {
                if self.role == Role::Host
                    && self.phase == Phase::Syncing
                    && !self.requests.contains(&Request::CaptureState)
                    && self.outgoing_state_len > 0
                {
                    self.start_running();
                } else {
                    self.protocol_error("an unexpected Ready");
                }
            }
            Message::Input { frame, input } => self.on_input(frame, input),
            Message::Checksum { frame, hash } => {
                if self.phase != Phase::Running {
                    self.protocol_error("a checksum before the game started");
                    return;
                }
                self.remote_checks.insert(frame, hash);
                self.compare_checks();
            }
        }
    }

    fn protocol_error(&mut self, what: &str) {
        self.end(
            &format!("protocol error: the other phone sent {what}"),
            true,
        );
    }

    // ------------------------------------------------------------ handshake

    /// Why `guest` cannot play with `host`, or `None` when they match.
    pub fn incompatibility(host: &PeerInfo, guest: &PeerInfo) -> Option<String> {
        if guest.version != PROTOCOL_VERSION {
            return Some(format!(
                "the two apps speak different online versions ({} and {}); update both",
                PROTOCOL_VERSION, guest.version
            ));
        }
        if guest.core_id != host.core_id {
            return Some(format!(
                "different emulator cores ({} and {}); pick the same core in Settings on both",
                host.core_id, guest.core_id
            ));
        }
        if guest.core_version != host.core_version {
            return Some(format!(
                "different builds of the {} core ({} and {}); install the same Continuum build on both",
                host.core_id, host.core_version, guest.core_version
            ));
        }
        if guest.content_fingerprint != host.content_fingerprint {
            return Some(format!(
                "different games: the host has {} and player 2 has {}; load the same ROM file on both",
                host.content_name, guest.content_name
            ));
        }
        if guest.state_size != host.state_size {
            return Some(format!(
                "the two cores report different state sizes ({} and {} bytes)",
                host.state_size, guest.state_size
            ));
        }
        if guest.options_hash != host.options_hash {
            return Some(
                "the core settings differ between the two phones; reset the core's options in \
                 Settings on both, or set them the same"
                    .to_string(),
            );
        }
        if host.active_cheats > 0 || guest.active_cheats > 0 {
            return Some("cheats are on; turn every cheat off on both phones first".to_string());
        }
        None
    }

    fn on_hello(&mut self, info: PeerInfo) {
        if self.role != Role::Host || self.phase != Phase::Handshaking {
            self.protocol_error("a second Hello");
            return;
        }
        self.peer_name = Some(info.content_name.clone());
        if let Some(reason) = Self::incompatibility(&self.local_info, &info) {
            self.send(&Message::Reject {
                reason: reason.clone(),
            });
            self.transport_connected = false;
            self.end(&format!("refused player 2: {reason}"), false);
            return;
        }
        self.phase = Phase::Syncing;
        self.requests.push_back(Request::CaptureState);
    }

    /// Host: the engine serialized the core and loaded that same state locally.
    pub fn host_state_captured(&mut self, state: Result<Vec<u8>, String>) {
        if self.role != Role::Host || self.phase != Phase::Syncing {
            return;
        }
        let state = match state {
            Ok(state) if !state.is_empty() => state,
            Ok(_) => {
                self.leave("this core cannot save a state, so online play cannot start");
                return;
            }
            Err(err) => {
                self.leave(&format!("the starting state could not be saved ({err})"));
                return;
            }
        };
        self.outgoing_state_len = state.len() as u64;
        self.send(&Message::Welcome {
            input_delay: self.config.input_delay,
            checksum_interval: self.config.checksum_interval,
            state_len: state.len() as u64,
            state_hash: super::state_hash(&state),
        });
        for (index, chunk) in state.chunks(STATE_CHUNK_BYTES).enumerate() {
            self.send(&Message::StateChunk {
                offset: (index * STATE_CHUNK_BYTES) as u64,
                bytes: chunk.to_vec(),
            });
        }
    }

    fn on_welcome(&mut self, delay: u8, interval: u32, len: u64, hash: u64) {
        if self.role != Role::Guest || self.phase != Phase::Handshaking {
            self.protocol_error("an unexpected Welcome");
            return;
        }
        // The sizes were compared at the handshake, so a state far larger than this core's own
        // is a broken or hostile host, and is refused before any memory is reserved for it.
        let ceiling = self.local_info.state_size.saturating_mul(2) + 1024 * 1024;
        if len == 0 || len > ceiling {
            self.protocol_error("a starting state of an impossible size");
            return;
        }
        self.config.input_delay = delay.min(MAX_INPUT_DELAY);
        self.config.checksum_interval = interval;
        self.expected_state_len = len;
        self.expected_state_hash = hash;
        self.incoming_state = Vec::new();
        self.phase = Phase::Syncing;
    }

    fn on_chunk(&mut self, offset: u64, bytes: &[u8]) {
        if self.role != Role::Guest || self.phase != Phase::Syncing {
            self.protocol_error("an unexpected piece of state");
            return;
        }
        if offset != self.incoming_state.len() as u64
            || self.incoming_state.len() as u64 + bytes.len() as u64 > self.expected_state_len
        {
            self.protocol_error("the starting state out of order");
            return;
        }
        self.incoming_state.extend_from_slice(bytes);
        if self.incoming_state.len() as u64 == self.expected_state_len {
            let state = std::mem::take(&mut self.incoming_state);
            if super::state_hash(&state) != self.expected_state_hash {
                self.leave("the starting state arrived damaged");
                return;
            }
            self.requests.push_back(Request::LoadState(state));
        }
    }

    /// Guest: the engine loaded the host's state (or could not).
    pub fn guest_state_loaded(&mut self, result: Result<(), String>) {
        if self.role != Role::Guest || self.phase != Phase::Syncing {
            return;
        }
        match result {
            Ok(()) => {
                self.send(&Message::Ready);
                self.start_running();
            }
            Err(err) => self.leave(&format!("the host's state would not load here ({err})")),
        }
    }

    fn start_running(&mut self) {
        self.phase = Phase::Running;
        self.frame = 0;
        let delay = u64::from(self.config.input_delay);
        self.local_next = delay;
        self.remote_next = delay;
        self.local_inputs.clear();
        self.remote_inputs.clear();
        self.stalled_since = None;
    }

    // ------------------------------------------------------------ lockstep

    fn input_for(map: &BTreeMap<u64, WireInput>, frame: u64, delay: u64) -> Option<WireInput> {
        if frame < delay {
            // Before the first sampled frame: nothing pressed, identically on both peers.
            Some(WireInput::default())
        } else {
            map.get(&frame).copied()
        }
    }

    /// Offers this peer's current input and asks whether the current frame may run.
    ///
    /// Call once per frame the pacer wants. The input is only consumed when a new frame slot is
    /// open, so calling it again while stalled does not resample the pad into a later frame.
    pub fn begin_frame(&mut self, local: WireInput) -> Step {
        if self.phase != Phase::Running {
            return Step::Wait;
        }
        let delay = u64::from(self.config.input_delay);
        if self.local_next <= self.frame + delay {
            let frame = self.local_next;
            self.local_inputs.insert(frame, local);
            self.local_next += 1;
            self.send(&Message::Input {
                frame,
                input: local,
            });
        }
        let mine = Self::input_for(&self.local_inputs, self.frame, delay);
        let theirs = Self::input_for(&self.remote_inputs, self.frame, delay);
        match (mine, theirs) {
            (Some(mine), Some(theirs)) => {
                self.stall_pending = false;
                self.stalled_since = None;
                Step::Run(match self.role {
                    Role::Host => [mine, theirs],
                    Role::Guest => [theirs, mine],
                })
            }
            _ => {
                self.stall_pending = true;
                self.stall_frames += 1;
                Step::Stall
            }
        }
    }

    /// The frame from the last `Step::Run` was executed. Returns true when the engine should
    /// hash its state now and pass it to [`Self::record_checksum`].
    pub fn end_frame(&mut self) -> bool {
        if self.phase != Phase::Running {
            return false;
        }
        let ran = self.frame;
        self.frame += 1;
        self.local_inputs.remove(&ran);
        self.remote_inputs.remove(&ran);
        let interval = u64::from(self.config.checksum_interval);
        interval > 0 && self.frame % interval == 0
    }

    pub fn record_checksum(&mut self, hash: u64) {
        if self.phase != Phase::Running {
            return;
        }
        let frame = self.frame;
        self.local_checks.insert(frame, hash);
        self.send(&Message::Checksum { frame, hash });
        self.compare_checks();
    }

    fn compare_checks(&mut self) {
        let common: Vec<u64> = self
            .local_checks
            .keys()
            .filter(|frame| self.remote_checks.contains_key(frame))
            .copied()
            .collect();
        for frame in common {
            let mine = self.local_checks.remove(&frame);
            let theirs = self.remote_checks.remove(&frame);
            self.checks_compared += 1;
            if mine != theirs && self.desync_frame.is_none() {
                log::warn!("netplay desync detected at frame {frame}");
                self.desync_frame = Some(frame);
            }
        }
        // A peer that stops sending checksums must not grow these maps forever.
        while self.local_checks.len() > 64 {
            self.local_checks.pop_first();
        }
        while self.remote_checks.len() > 64 {
            self.remote_checks.pop_first();
        }
    }

    fn on_input(&mut self, frame: u64, input: WireInput) {
        if self.phase != Phase::Running {
            self.protocol_error("input before the game started");
            return;
        }
        if frame != self.remote_next {
            self.protocol_error("input for the wrong frame");
            return;
        }
        // A peer can only be `delay` frames ahead of a frame we have not run; anything far
        // beyond is a broken peer, and buffering it would be unbounded memory.
        if frame > self.frame + 2 * u64::from(MAX_INPUT_DELAY) + 600 {
            self.protocol_error("input too far ahead");
            return;
        }
        self.remote_inputs.insert(frame, input);
        self.remote_next += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(content: &str) -> PeerInfo {
        PeerInfo {
            version: PROTOCOL_VERSION,
            core_id: "snes9x".into(),
            core_version: "1.0".into(),
            content_name: content.into(),
            content_fingerprint: super::super::fnv1a64(content.as_bytes()),
            state_size: 16,
            active_cheats: 0,
            options_hash: 1,
        }
    }

    /// A deterministic stand-in core: its whole state is one number, and every frame folds both
    /// players' inputs into it.
    #[derive(Clone, Default)]
    struct FakeCore {
        state: u64,
        /// Every input set the core was run with, for routing assertions.
        history: Vec<[WireInput; 2]>,
    }

    impl FakeCore {
        fn run(&mut self, inputs: [WireInput; 2]) {
            let mut bytes = self.state.to_le_bytes().to_vec();
            bytes.extend_from_slice(&inputs[0].buttons.to_le_bytes());
            bytes.extend_from_slice(&inputs[1].buttons.to_le_bytes());
            self.state = super::super::fnv1a64(&bytes);
            self.history.push(inputs);
        }
        fn save(&self) -> Vec<u8> {
            self.state.to_le_bytes().to_vec()
        }
        fn load(&mut self, bytes: &[u8]) {
            let mut a = [0u8; 8];
            a.copy_from_slice(&bytes[..8]);
            self.state = u64::from_le_bytes(a);
        }
    }

    struct Peer {
        session: NetplaySession,
        core: FakeCore,
        pad: u16,
    }

    impl Peer {
        /// What the engine's tick does, minus the pacer: service requests, try `steps` frames.
        fn tick(&mut self, now: f64, steps: u32) -> u32 {
            while let Some(request) = self.session.take_request() {
                match request {
                    Request::CaptureState => {
                        let state = self.core.save();
                        self.core.load(&state);
                        self.session.host_state_captured(Ok(state));
                    }
                    Request::LoadState(bytes) => {
                        self.core.load(&bytes);
                        self.session.guest_state_loaded(Ok(()));
                    }
                }
            }
            let mut ran = 0;
            for _ in 0..steps {
                let local = WireInput {
                    buttons: self.pad,
                    ..Default::default()
                };
                match self.session.begin_frame(local) {
                    Step::Run(inputs) => {
                        self.core.run(inputs);
                        if self.session.end_frame() {
                            let hash = super::super::fnv1a64(&self.core.save());
                            self.session.record_checksum(hash);
                        }
                        ran += 1;
                    }
                    Step::Stall | Step::Wait => break,
                }
            }
            self.session.poll(now);
            ran
        }
    }

    fn pair(delay: u8) -> (Peer, Peer) {
        let config = NetplayConfig {
            input_delay: delay,
            checksum_interval: 10,
            timeout_ms: DEFAULT_TIMEOUT_MS,
        };
        let mut host = Peer {
            session: NetplaySession::new_host(config, info("game.sfc")),
            core: FakeCore {
                state: 1234,
                ..Default::default()
            },
            pad: 0,
        };
        let mut guest = Peer {
            session: NetplaySession::new_guest(info("game.sfc")),
            // Different on purpose: the handshake must overwrite it with the host's.
            core: FakeCore {
                state: 999,
                ..Default::default()
            },
            pad: 0,
        };
        host.session.transport_connected();
        guest.session.transport_connected();
        (host, guest)
    }

    fn deliver(from: &mut Peer, to: &mut Peer) {
        let bytes = from.session.take_outgoing();
        to.session.receive(&bytes);
    }

    fn exchange(a: &mut Peer, b: &mut Peer) {
        deliver(a, b);
        deliver(b, a);
    }

    /// Ticks both peers and delivers everything, `rounds` times.
    fn run_connected(host: &mut Peer, guest: &mut Peer, rounds: usize, start_ms: f64) -> f64 {
        let mut now = start_ms;
        for _ in 0..rounds {
            host.tick(now, 1);
            guest.tick(now, 1);
            exchange(host, guest);
            now += 16.0;
        }
        now
    }

    fn handshake(host: &mut Peer, guest: &mut Peer) -> f64 {
        let mut now = 0.0;
        for _ in 0..6 {
            host.tick(now, 0);
            guest.tick(now, 0);
            exchange(host, guest);
            now += 16.0;
        }
        assert_eq!(host.session.phase(), Phase::Running);
        assert_eq!(guest.session.phase(), Phase::Running);
        now
    }

    #[test]
    fn handshake_hands_the_guest_the_hosts_state() {
        let (mut host, mut guest) = pair(2);
        handshake(&mut host, &mut guest);
        assert_eq!(guest.core.state, host.core.state);
        assert_eq!(guest.core.state, 1234);
        assert_eq!(host.session.status_kind(), StatusKind::Running);
    }

    #[test]
    fn both_peers_stay_identical_and_route_players_to_their_ports() {
        let (mut host, mut guest) = pair(2);
        let mut now = handshake(&mut host, &mut guest);
        for round in 0..200u16 {
            host.pad = round % 7;
            guest.pad = 0x100 | (round % 5);
            host.tick(now, 1);
            guest.tick(now, 1);
            exchange(&mut host, &mut guest);
            now += 16.0;
        }
        assert!(host.session.frame() > 150);
        let common = host.core.history.len().min(guest.core.history.len());
        assert_eq!(host.core.history[..common], guest.core.history[..common]);
        // Player 2's pad always carries the 0x100 bit once sampling began, and lands on port 1.
        for inputs in &host.core.history[2..common] {
            assert_eq!(inputs[1].buttons & 0x100, 0x100);
            assert_eq!(inputs[0].buttons & 0x100, 0);
        }
        assert!(host.session.checks_compared() > 0);
        assert_eq!(host.session.desync_frame(), None);
        assert_eq!(guest.session.desync_frame(), None);
    }

    #[test]
    fn input_takes_effect_exactly_delay_frames_later() {
        let (mut host, mut guest) = pair(3);
        let now = handshake(&mut host, &mut guest);
        let now = run_connected(&mut host, &mut guest, 10, now);
        let pressed_at = host.session.frame();
        host.pad = 0x8;
        run_connected(&mut host, &mut guest, 10, now);
        let first = host
            .core
            .history
            .iter()
            .position(|inputs| inputs[0].buttons == 0x8)
            .unwrap() as u64;
        assert_eq!(first, pressed_at + 3);
        assert_eq!(guest.core.history[first as usize][0].buttons, 0x8);
    }

    #[test]
    fn a_missing_input_stalls_instead_of_guessing() {
        let (mut host, mut guest) = pair(2);
        let now = handshake(&mut host, &mut guest);
        let now = run_connected(&mut host, &mut guest, 5, now);
        let frame = host.session.frame();
        // The guest goes quiet: host can only run the frames it already has input for.
        let mut t = now;
        let mut held = Vec::new();
        for _ in 0..20 {
            host.tick(t, 1);
            held.extend(host.session.take_outgoing());
            t += 16.0;
        }
        assert!(host.session.frame() <= frame + 2, "ran ahead of the guest");
        assert_eq!(host.session.status_kind(), StatusKind::Stalled);
        assert!(host.session.status_line().contains("stalled"));
        // Recovery: the guest catches up and both continue in step.
        guest.session.receive(&held);
        run_connected(&mut host, &mut guest, 40, t);
        assert_eq!(host.session.status_kind(), StatusKind::Running);
        let common = host.core.history.len().min(guest.core.history.len());
        assert_eq!(host.core.history[..common], guest.core.history[..common]);
        assert_eq!(host.session.desync_frame(), None);
    }

    #[test]
    fn zero_delay_still_runs_in_lockstep() {
        let (mut host, mut guest) = pair(0);
        let mut now = handshake(&mut host, &mut guest);
        for _ in 0..50 {
            host.tick(now, 1);
            guest.tick(now, 1);
            exchange(&mut host, &mut guest);
            // With no delay each frame needs a round trip, so a second pass runs it.
            host.tick(now, 1);
            guest.tick(now, 1);
            exchange(&mut host, &mut guest);
            now += 16.0;
        }
        assert!(host.session.frame() >= 40);
        let common = host.core.history.len().min(guest.core.history.len());
        assert_eq!(host.core.history[..common], guest.core.history[..common]);
        assert_eq!(host.session.desync_frame(), None);
    }

    #[test]
    fn a_diverged_state_is_reported_as_a_desync() {
        let (mut host, mut guest) = pair(2);
        let now = handshake(&mut host, &mut guest);
        let now = run_connected(&mut host, &mut guest, 5, now);
        guest.core.state ^= 1;
        run_connected(&mut host, &mut guest, 30, now);
        assert!(host.session.desync_frame().is_some());
        assert!(guest.session.desync_frame().is_some());
        assert_eq!(host.session.status_kind(), StatusKind::Desynced);
        assert!(host.session.status_line().contains("DESYNC"));
    }

    #[test]
    fn a_different_game_is_refused_with_both_names() {
        let config = NetplayConfig::default();
        let mut host = NetplaySession::new_host(config, info("a.sfc"));
        let mut guest = NetplaySession::new_guest(info("b.sfc"));
        host.transport_connected();
        guest.transport_connected();
        host.receive(&guest.take_outgoing());
        guest.receive(&host.take_outgoing());
        assert_eq!(host.phase(), Phase::Disconnected);
        assert_eq!(guest.phase(), Phase::Disconnected);
        let line = guest.status_line();
        assert!(line.contains("a.sfc") && line.contains("b.sfc"), "{line}");
    }

    #[test]
    fn cheats_on_either_side_are_refused() {
        let mut with_cheats = info("a.sfc");
        with_cheats.active_cheats = 1;
        assert!(
            NetplaySession::incompatibility(&info("a.sfc"), &with_cheats)
                .unwrap()
                .contains("cheat")
        );
        assert!(NetplaySession::incompatibility(&info("a.sfc"), &info("a.sfc")).is_none());
    }

    #[test]
    fn different_core_settings_are_refused() {
        let mut other = info("a.sfc");
        other.options_hash = 99;
        assert!(NetplaySession::incompatibility(&info("a.sfc"), &other)
            .unwrap()
            .contains("core settings"));
    }

    #[test]
    fn an_oversized_starting_state_is_refused_before_allocating() {
        let mut guest = NetplaySession::new_guest(info("a.sfc"));
        guest.transport_connected();
        let _ = guest.take_outgoing();
        guest.receive(
            &Message::Welcome {
                input_delay: 2,
                checksum_interval: 60,
                state_len: 400 * 1024 * 1024,
                state_hash: 0,
            }
            .encode(),
        );
        assert_eq!(guest.phase(), Phase::Disconnected);
        assert!(guest.status_line().contains("impossible size"));
    }

    #[test]
    fn a_host_that_never_answers_times_out_for_the_guest() {
        let mut guest = NetplaySession::new_guest(info("a.sfc"));
        guest.poll(0.0);
        guest.poll(5_000.0);
        assert_eq!(guest.phase(), Phase::WaitingForPeer);
        guest.poll(10_500.0);
        assert_eq!(guest.phase(), Phase::Disconnected);
        assert!(guest.status_line().contains("could not reach the host"));
    }

    #[test]
    fn leaving_tells_the_other_side_why() {
        let (mut host, mut guest) = pair(2);
        handshake(&mut host, &mut guest);
        guest.session.leave("player 2 went back to the library");
        deliver(&mut guest, &mut host);
        assert_eq!(host.session.phase(), Phase::Disconnected);
        assert!(host
            .session
            .status_line()
            .contains("player 2 went back to the library"));
    }

    #[test]
    fn silence_times_out() {
        let (mut host, mut guest) = pair(2);
        let now = handshake(&mut host, &mut guest);
        host.tick(now, 1);
        host.tick(now + 11_000.0, 1);
        assert_eq!(host.session.phase(), Phase::Disconnected);
        assert!(host.session.status_line().contains("nothing heard"));
    }

    #[test]
    fn garbage_ends_the_session_readably() {
        let (mut host, _guest) = pair(2);
        host.session.receive(&[5, 0, 0, 0, 200, 1, 2, 3, 4]);
        assert_eq!(host.session.phase(), Phase::Disconnected);
        assert!(host.session.status_line().contains("unreadable"));
    }

    #[test]
    fn ping_measures_a_round_trip() {
        let (mut host, mut guest) = pair(2);
        host.session.poll(1000.0);
        deliver(&mut host, &mut guest);
        guest.session.poll(1000.0);
        let pong = guest.session.take_outgoing();
        host.session.poll(1040.0);
        host.session.receive(&pong);
        assert_eq!(host.session.rtt_ms(), Some(40.0));
    }

    #[test]
    fn every_status_has_its_own_line() {
        let host = NetplaySession::new_host(NetplayConfig::default(), info("a"));
        assert!(host.status_line().contains("waiting"));
        let guest = NetplaySession::new_guest(info("a"));
        assert!(guest.status_line().contains("connecting"));
        let (mut h, mut g) = pair(2);
        assert!(h.session.status_line().contains("checking"));
        h.tick(0.0, 0);
        g.tick(0.0, 0);
        deliver(&mut g, &mut h);
        assert_eq!(h.session.status_kind(), StatusKind::Syncing);
        assert!(h.session.status_line().contains("syncing"));
    }
}
