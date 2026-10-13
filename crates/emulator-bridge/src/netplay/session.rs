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
//!
//! ## The link cable
//!
//! A session is one of two [`Mode`]s, and the running core decides which, on each phone: a core
//! that offers libretro's two-player packet interface (cBoy does, for every Game Boy game) gets a
//! [`Mode::Link`] session, everything else [`Mode::Lockstep`]. A link is two consoles and a cable,
//! not one shared game: each phone keeps its own game, save and pace, nothing above runs, and the
//! only thing that crosses (besides the keepalive) is what the core puts on the cable, as
//! [`Message::Link`] blocks the session never looks inside. So the two games can differ (Pokemon
//! Red trades with Blue), and the handshake checks only that both phones speak the same cable.
//! The engine is told when to plug the core's cable in and when to pull it out with
//! [`Request::StartLink`] and [`Request::StopLink`].

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

/// libretro's client numbers for the two ends of a link: 0 is the host and any number above it a
/// player who joined (libretro.h:6275). There is only ever one of those here, and it is 1.
pub const HOST_CLIENT_ID: u16 = 0;
pub const GUEST_CLIENT_ID: u16 = 1;

/// Link blocks held for the next frame before newer ones are dropped. A game puts a few bytes a
/// frame on the cable at most, and the queue is emptied every frame, so this is only reached while
/// this phone's game is paused or in the background, when every block is stale anyway; the cap is
/// what keeps a peer that never stops sending from filling the phone's memory.
const LINK_QUEUE_MAX: usize = 1024;

/// The refusal both phones read when only one of them has a game that can use the link cable. It
/// is sent to the other phone as well, so it names neither side.
pub const LINK_NEEDS_A_CABLE_GAME: &str =
    "the link cable needs a Game Boy game running on cBoy on both phones";
/// The refusal for two phones whose cBoy speaks different versions of its cable.
pub const LINK_DIFFERENT_CABLES: &str =
    "the two phones have different versions of cBoy, so put the same Continuum build on both";

/// One shared game or two games and a cable. See the module header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// One game, two pads: both phones run the same machine frame by frame.
    Lockstep,
    /// Two games and a cable: each phone runs its own machine, and only what its core puts on the
    /// cable crosses.
    Link,
}

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
    /// Link mode, once both phones agree: plug the core's cable in. `local` is this phone's
    /// libretro client number and `peer` the other phone's. If the core turns the other phone
    /// away, say so with [`NetplaySession::link_refused_by_core`].
    StartLink { local: u16, peer: u16 },
    /// Link mode, when the session ends after a [`Request::StartLink`] was handed out: pull the
    /// cable out. Exactly one follows each start.
    StopLink { peer: u16 },
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

    /// The cable this phone's core offers (its protocol name), `None` for no cable. `Some` is
    /// what makes this a [`Mode::Link`] session.
    link_protocol: Option<String>,
    /// Host: the guest's `Hello`, held until the `LinkOffer` that follows it says which kind of
    /// session the guest is asking for.
    peer_hello: Option<PeerInfo>,
    /// Blocks from the other phone's core, waiting for this phone's next frame.
    link_in: VecDeque<Vec<u8>>,
    /// A [`Request::StartLink`] was handed out and its [`Request::StopLink`] has not been.
    link_plugged: bool,
    link_blocks_in: u64,
    link_blocks_out: u64,
    link_blocks_dropped: u64,
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
            link_protocol: None,
            peer_hello: None,
            link_in: VecDeque::new(),
            link_plugged: false,
            link_blocks_in: 0,
            link_blocks_out: 0,
            link_blocks_dropped: 0,
        }
    }

    /// The host decides the delay and checksum interval; the guest is told them.
    pub fn new_host(config: NetplayConfig, local_info: PeerInfo) -> Self {
        Self::new(Role::Host, config, local_info)
    }

    pub fn new_guest(local_info: PeerInfo) -> Self {
        Self::new(Role::Guest, NetplayConfig::default(), local_info)
    }

    /// Makes this a link cable session when this phone's core offers a cable (`protocol` is the
    /// name it gave it), and leaves it one shared game when it offers none. Before the transport
    /// opens.
    pub fn with_link(mut self, protocol: Option<String>) -> Self {
        self.link_protocol = protocol.filter(|p| !p.is_empty());
        self
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
    pub fn mode(&self) -> Mode {
        if self.link_protocol.is_some() {
            Mode::Link
        } else {
            Mode::Lockstep
        }
    }
    pub fn is_link(&self) -> bool {
        self.mode() == Mode::Link
    }
    /// Link mode: blocks carried in from the other phone, blocks sent to it, and blocks dropped
    /// because this phone was not running its game to take them.
    pub fn link_counts(&self) -> (u64, u64, u64) {
        (self.link_blocks_in, self.link_blocks_out, self.link_blocks_dropped)
    }
    /// The libretro client number of the other phone (see [`HOST_CLIENT_ID`]).
    pub fn peer_client_id(&self) -> u16 {
        match self.role {
            Role::Host => GUEST_CLIENT_ID,
            Role::Guest => HOST_CLIENT_ID,
        }
    }
    fn local_client_id(&self) -> u16 {
        match self.role {
            Role::Host => HOST_CLIENT_ID,
            Role::Guest => GUEST_CLIENT_ID,
        }
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
        if self.is_link() {
            return self.link_status_line();
        }
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

    /// The same, for a link cable. A link has no starting state, no frames in step and no
    /// desyncs, so it only ever has four things to say.
    fn link_status_line(&self) -> String {
        match (self.status_kind(), self.role) {
            (StatusKind::Disconnected, _) => format!(
                "Link cable unplugged: {}",
                self.disconnect_reason.as_deref().unwrap_or("the link ended")
            ),
            (StatusKind::Waiting, Role::Host) => {
                "Link cable: waiting for the other phone to join".to_string()
            }
            (StatusKind::Waiting, Role::Guest) => "Link cable: reaching the other phone".to_string(),
            (StatusKind::Connected, Role::Host) => {
                "Link cable: the other phone is here, checking its game".to_string()
            }
            (StatusKind::Connected, Role::Guest) => {
                "Link cable: connected, waiting for the other phone to say yes".to_string()
            }
            _ => match self.rtt_ms {
                Some(rtt) => format!("Link cable plugged in (ping {rtt:.0} ms)"),
                None => "Link cable plugged in".to_string(),
            },
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
                // Always, even with no cable to offer: the host waits for it to know which kind
                // of session this guest is asking for.
                self.send(&Message::LinkOffer {
                    protocol: self.link_protocol.clone().unwrap_or_default(),
                });
            }
        }
    }

    /// The socket failed or closed underneath us.
    pub fn transport_lost(&mut self, reason: &str) {
        self.transport_connected = false;
        if self.is_link() {
            // The network's own words ("Connection reset by peer") are for the log, not the
            // player.
            log::info!("link cable: the connection was lost ({reason})");
            self.end("the connection to the other phone dropped", false);
        } else {
            self.end(&format!("the connection was lost ({reason})"), false);
        }
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
        // A cable that was never plugged in (its start still waiting here) needs no pulling out.
        let start_pending = self
            .requests
            .iter()
            .any(|request| matches!(request, Request::StartLink { .. }));
        self.requests.clear();
        if self.link_plugged && !start_pending {
            self.requests.push_back(Request::StopLink {
                peer: self.peer_client_id(),
            });
        }
        self.link_plugged = false;
        self.link_in.clear();
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
            let reason = if self.is_link() {
                format!("couldn't reach the other phone in {seconds} seconds. Check the address, and that both phones are on the same Wi-Fi")
            } else {
                format!("could not reach the host within {seconds} s; check the address and that both phones are on the same network")
            };
            self.end(&reason, false);
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
                if self.is_link() {
                    // The other phone's own words ("the other player went back to the library"),
                    // which already say who left.
                    self.end(&reason, false);
                } else {
                    self.end(&format!("the other player left ({reason})"), false);
                }
            }
            Message::Reject { reason } => {
                self.transport_connected = false;
                if self.is_link() {
                    self.end(&reason, false);
                } else {
                    self.end(&format!("refused: {reason}"), false);
                }
            }
            Message::Hello(info) => self.on_hello(info),
            Message::LinkOffer { protocol } => self.on_link_offer(protocol),
            Message::LinkWelcome => {
                if self.role == Role::Guest && self.phase == Phase::Handshaking && self.is_link() {
                    self.start_link();
                } else {
                    self.protocol_error("an unexpected link cable welcome");
                }
            }
            Message::Link { block } => self.on_link_block(block),
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
                if self.phase != Phase::Running || self.is_link() {
                    self.protocol_error("a checksum outside a shared game");
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

    /// Why a guest whose game offers the cable `theirs` (empty for none) cannot join a host whose
    /// game offers `ours`, as far as the cable decides it, or `None` when it does not stand in the
    /// way. Two phones with no cable at all are a shared game, judged by [`Self::incompatibility`].
    pub fn cable_mismatch(ours: Option<&str>, theirs: &str) -> Option<&'static str> {
        match ours {
            None if theirs.is_empty() => None,
            Some(ours) if ours == theirs => None,
            // Both have a cable, and they are not the same one.
            Some(_) if !theirs.is_empty() => Some(LINK_DIFFERENT_CABLES),
            // One phone has a cable and the other does not.
            _ => Some(LINK_NEEDS_A_CABLE_GAME),
        }
    }

    fn on_hello(&mut self, info: PeerInfo) {
        if self.role != Role::Host || self.phase != Phase::Handshaking || self.peer_hello.is_some()
        {
            self.protocol_error("a second Hello");
            return;
        }
        self.peer_name = Some(info.content_name.clone());
        // An app from before the link cable sends no offer after its Hello, so it is refused here,
        // on the Hello alone, with the reason it can read.
        if info.version != PROTOCOL_VERSION {
            let reason = Self::incompatibility(&self.local_info, &info).unwrap_or_default();
            self.refuse(&reason);
            return;
        }
        self.peer_hello = Some(info);
    }

    /// Host: the guest's offer, which says what kind of session it wants. Decided here.
    fn on_link_offer(&mut self, protocol: String) {
        if self.role != Role::Host || self.phase != Phase::Handshaking {
            self.protocol_error("an unexpected link cable offer");
            return;
        }
        let Some(info) = self.peer_hello.take() else {
            self.protocol_error("a link cable offer before its Hello");
            return;
        };
        if let Some(reason) = Self::cable_mismatch(self.link_protocol.as_deref(), &protocol) {
            self.refuse(reason);
            return;
        }
        if self.is_link() {
            // Two consoles and a cable: the games are their own business, and may differ.
            self.send(&Message::LinkWelcome);
            self.start_link();
            return;
        }
        if let Some(reason) = Self::incompatibility(&self.local_info, &info) {
            self.refuse(&reason);
            return;
        }
        self.phase = Phase::Syncing;
        self.requests.push_back(Request::CaptureState);
    }

    /// Host: turns the guest away, telling it why.
    fn refuse(&mut self, reason: &str) {
        self.send(&Message::Reject {
            reason: reason.to_string(),
        });
        self.transport_connected = false;
        if self.is_link() {
            self.end(reason, false);
        } else {
            self.end(&format!("refused player 2: {reason}"), false);
        }
    }

    /// Both phones, link mode: the two have agreed, so the cable goes in.
    fn start_link(&mut self) {
        self.phase = Phase::Running;
        self.link_plugged = true;
        self.requests.push_back(Request::StartLink {
            local: self.local_client_id(),
            peer: self.peer_client_id(),
        });
    }

    /// The engine plugged the cable in and the core turned the other phone away (libretro's
    /// `connected` said no). The link ends, and the other phone is told.
    pub fn link_refused_by_core(&mut self) {
        if !self.is_link() || self.phase != Phase::Running {
            return;
        }
        // Each phone reads it from its own side.
        if self.transport_connected {
            self.send(&Message::Bye {
                reason: "the game on the other phone turned this phone away".into(),
            });
        }
        self.end("this game turned the other phone away", false);
    }

    fn on_link_block(&mut self, block: Vec<u8>) {
        if !self.is_link() || self.phase != Phase::Running {
            self.protocol_error("a link cable block outside a link");
            return;
        }
        if block.is_empty() {
            return;
        }
        self.link_blocks_in += 1;
        if self.link_in.len() >= LINK_QUEUE_MAX {
            self.link_blocks_dropped += 1;
            return;
        }
        self.link_in.push_back(block);
    }

    /// Link mode: every block the other phone's core sent since the last call, in the order it
    /// sent them, for this phone's core before its next frame.
    pub fn take_link_blocks(&mut self) -> VecDeque<Vec<u8>> {
        std::mem::take(&mut self.link_in)
    }

    /// Link mode: one block this phone's core put on the cable, for the other phone. Dropped when
    /// there is no cable to put it on, which is what a core sending after the session ended gets.
    pub fn send_link_block(&mut self, block: &[u8]) {
        if !self.is_link() || self.phase != Phase::Running || !self.link_plugged {
            return;
        }
        if block.is_empty() || block.len() > super::wire::MAX_LINK_BLOCK_BYTES {
            log::warn!("link cable: a {} byte block was not sent", block.len());
            return;
        }
        self.link_blocks_out += 1;
        self.send(&Message::Link {
            block: block.to_vec(),
        });
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
        if self.role != Role::Guest || self.phase != Phase::Handshaking || self.is_link() {
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
        // A link cable never runs frames in step: each phone plays its own game.
        if self.phase != Phase::Running || self.is_link() {
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
        if self.phase != Phase::Running || self.is_link() {
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
        if self.phase != Phase::Running || self.is_link() {
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
        if self.phase != Phase::Running || self.is_link() {
            self.protocol_error("input outside a shared game");
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
            core_id: "bsnes".into(),
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
                    Request::StartLink { .. } | Request::StopLink { .. } => {
                        panic!("a shared game asked for a link cable: {request:?}")
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

    // ------------------------------------------------------------ the link cable

    const CABLE: &str = "cboy-link-1";

    fn cable_info(content: &str) -> PeerInfo {
        PeerInfo {
            core_id: "continuum_gb".into(),
            ..info(content)
        }
    }

    fn link_host(content: &str, cable: Option<&str>) -> NetplaySession {
        NetplaySession::new_host(NetplayConfig::default(), cable_info(content))
            .with_link(cable.map(str::to_string))
    }

    fn link_guest(content: &str, cable: Option<&str>) -> NetplaySession {
        NetplaySession::new_guest(cable_info(content)).with_link(cable.map(str::to_string))
    }

    /// Opens both transports and passes everything back and forth until nothing is left to say.
    fn connect(host: &mut NetplaySession, guest: &mut NetplaySession) {
        host.transport_connected();
        guest.transport_connected();
        for _ in 0..4 {
            host.receive(&guest.take_outgoing());
            guest.receive(&host.take_outgoing());
        }
    }

    fn requests(session: &mut NetplaySession) -> Vec<Request> {
        std::iter::from_fn(|| session.take_request()).collect()
    }

    /// Red trades with Blue: two different games join, each phone keeps its own (no state is
    /// asked for or sent), both are told to plug their cable in with the right numbers, and blocks
    /// cross both ways, in order, untouched.
    #[test]
    fn a_link_cable_joins_two_different_games_and_carries_blocks_both_ways() {
        let mut host = link_host("Pokemon Red.gb", Some(CABLE));
        let mut guest = link_guest("Pokemon Blue.gb", Some(CABLE));
        assert_eq!(host.mode(), Mode::Link);
        connect(&mut host, &mut guest);
        for side in [&host, &guest] {
            assert_eq!(side.phase(), Phase::Running);
            assert_eq!(side.status_kind(), StatusKind::Running);
            assert!(side.is_live());
            assert_eq!(side.status_line(), "Link cable plugged in");
        }
        assert_eq!(requests(&mut host), vec![Request::StartLink { local: 0, peer: 1 }]);
        assert_eq!(requests(&mut guest), vec![Request::StartLink { local: 1, peer: 0 }]);

        for byte in [0x11u8, 0x12, 0x13] {
            host.send_link_block(&[b'C', byte - 0x11, byte]);
        }
        guest.send_link_block(&[b'A', 0, 0x22]);
        guest.receive(&host.take_outgoing());
        host.receive(&guest.take_outgoing());
        assert_eq!(
            guest.take_link_blocks(),
            vec![vec![b'C', 0, 0x11], vec![b'C', 1, 0x12], vec![b'C', 2, 0x13]]
        );
        assert_eq!(host.take_link_blocks(), vec![vec![b'A', 0, 0x22]]);
        assert!(guest.take_link_blocks().is_empty(), "taken once");
        assert_eq!(host.link_counts(), (1, 3, 0));
        assert_eq!(guest.link_counts(), (3, 1, 0));
        // A link never runs frames in step: each phone paces its own game.
        assert_eq!(host.begin_frame(WireInput::default()), Step::Wait);
        assert!(!host.end_frame());
        assert_eq!(host.phase(), Phase::Running);
    }

    /// The other phone is not playing on cBoy. Both phones read the same plain sentence, which
    /// names neither, and nobody plugs anything in.
    #[test]
    fn a_phone_without_the_cable_is_refused_in_plain_words_on_both_sides() {
        // This phone links; the other one is on another emulator.
        let mut host = link_host("Tetris.gb", Some(CABLE));
        let mut guest = link_guest("Tetris.gb", None);
        connect(&mut host, &mut guest);
        assert_eq!(host.phase(), Phase::Disconnected);
        assert_eq!(guest.phase(), Phase::Disconnected);
        assert_eq!(
            host.status_line(),
            format!("Link cable unplugged: {LINK_NEEDS_A_CABLE_GAME}")
        );
        assert!(guest.status_line().contains(LINK_NEEDS_A_CABLE_GAME), "{}", guest.status_line());
        assert!(requests(&mut host).is_empty() && requests(&mut guest).is_empty());

        // The other way round: this phone joins with the cable, the host shares one game.
        let mut host = link_host("Tetris.gb", None);
        let mut guest = link_guest("Tetris.gb", Some(CABLE));
        connect(&mut host, &mut guest);
        assert_eq!(host.phase(), Phase::Disconnected);
        assert!(host.status_line().contains(LINK_NEEDS_A_CABLE_GAME));
        assert_eq!(
            guest.status_line(),
            format!("Link cable unplugged: {LINK_NEEDS_A_CABLE_GAME}")
        );
        assert!(requests(&mut host).is_empty() && requests(&mut guest).is_empty());
    }

    #[test]
    fn two_versions_of_the_cable_are_refused() {
        let mut host = link_host("a.gb", Some(CABLE));
        let mut guest = link_guest("a.gb", Some("cboy-link-2"));
        connect(&mut host, &mut guest);
        assert_eq!(
            guest.status_line(),
            format!("Link cable unplugged: {LINK_DIFFERENT_CABLES}")
        );
        assert_eq!(host.phase(), Phase::Disconnected);
    }

    /// An app from before the cable sends a version-1 Hello and no offer after it. It is refused on
    /// the Hello alone, with the version in words, instead of being left waiting for an offer.
    #[test]
    fn an_app_from_before_the_cable_is_refused_on_its_version() {
        for mut host in [link_host("a.gb", Some(CABLE)), link_host("a.gb", None)] {
            host.transport_connected();
            let old = PeerInfo {
                version: 1,
                ..cable_info("a.gb")
            };
            host.receive(&Message::Hello(old).encode());
            assert_eq!(host.phase(), Phase::Disconnected);
            let mut decoder = super::super::wire::Decoder::new();
            decoder.push(&host.take_outgoing());
            match decoder.next_message() {
                Ok(Some(Message::Reject { reason })) => {
                    assert!(reason.contains("different online versions"), "{reason}")
                }
                other => panic!("expected a refusal, got {other:?}"),
            }
        }
    }

    /// Both cables plugged in, then the connection goes. Exactly one unplug each, and the line
    /// is a sentence, not the network's error.
    #[test]
    fn a_dropped_connection_unplugs_the_cable_once_and_says_so_plainly() {
        let mut host = link_host("a.gb", Some(CABLE));
        let mut guest = link_guest("b.gb", Some(CABLE));
        connect(&mut host, &mut guest);
        requests(&mut host);
        requests(&mut guest);
        host.transport_lost("POSIXErrorCode(rawValue: 54): Connection reset by peer");
        assert_eq!(host.phase(), Phase::Disconnected);
        assert!(!host.is_live());
        assert_eq!(requests(&mut host), vec![Request::StopLink { peer: 1 }]);
        assert_eq!(
            host.status_line(),
            "Link cable unplugged: the connection to the other phone dropped"
        );
        host.transport_lost("again");
        host.leave("and again");
        assert!(requests(&mut host).is_empty(), "one unplug for one plug");
        // Blocks that turn up after the end go nowhere.
        host.send_link_block(&[1, 2, 3]);
        assert!(host.take_outgoing().is_empty());

        // The guest hears nothing more and gives up on its own clock.
        guest.poll(0.0);
        guest.poll(DEFAULT_TIMEOUT_MS + 1_000.0);
        assert_eq!(guest.phase(), Phase::Disconnected);
        assert_eq!(requests(&mut guest), vec![Request::StopLink { peer: 0 }]);
        assert!(guest.status_line().contains("nothing heard from the other phone"));
    }

    /// The other player leaving reads as their own words, and pulls this phone's cable out too.
    #[test]
    fn the_other_player_leaving_unplugs_this_phone() {
        let mut host = link_host("a.gb", Some(CABLE));
        let mut guest = link_guest("b.gb", Some(CABLE));
        connect(&mut host, &mut guest);
        requests(&mut host);
        requests(&mut guest);
        guest.leave("the other player went back to the library");
        assert_eq!(requests(&mut guest), vec![Request::StopLink { peer: 0 }]);
        host.receive(&guest.take_outgoing());
        assert_eq!(
            host.status_line(),
            "Link cable unplugged: the other player went back to the library"
        );
        assert_eq!(requests(&mut host), vec![Request::StopLink { peer: 1 }]);
    }

    /// A session that ends before the engine ever plugged the cable in leaves nothing to pull out.
    #[test]
    fn a_cable_that_never_went_in_is_not_pulled_out() {
        let mut host = link_host("a.gb", Some(CABLE));
        let mut guest = link_guest("b.gb", Some(CABLE));
        connect(&mut host, &mut guest);
        host.leave("changed my mind");
        assert!(requests(&mut host).is_empty());
    }

    #[test]
    fn the_core_turning_the_other_phone_away_ends_the_link() {
        let mut host = link_host("a.gb", Some(CABLE));
        let mut guest = link_guest("b.gb", Some(CABLE));
        connect(&mut host, &mut guest);
        requests(&mut host);
        host.link_refused_by_core();
        assert_eq!(host.phase(), Phase::Disconnected);
        assert_eq!(requests(&mut host), vec![Request::StopLink { peer: 1 }]);
        guest.receive(&host.take_outgoing());
        assert!(guest.status_line().contains("turned this phone away"), "{}", guest.status_line());
    }

    /// Shared-game messages on a cable, and cable blocks in a shared game, are a broken peer.
    #[test]
    fn the_two_kinds_of_session_do_not_take_each_others_messages() {
        let mut host = link_host("a.gb", Some(CABLE));
        let mut guest = link_guest("b.gb", Some(CABLE));
        connect(&mut host, &mut guest);
        host.receive(
            &Message::Input {
                frame: 0,
                input: WireInput::default(),
            }
            .encode(),
        );
        assert_eq!(host.phase(), Phase::Disconnected);
        assert!(host.status_line().contains("protocol error"));
        guest.receive(&Message::Checksum { frame: 60, hash: 1 }.encode());
        assert_eq!(guest.phase(), Phase::Disconnected);

        let (mut lock_host, mut lock_guest) = pair(2);
        handshake(&mut lock_host, &mut lock_guest);
        lock_host.session.receive(&Message::Link { block: vec![1, 2, 3] }.encode());
        assert_eq!(lock_host.session.phase(), Phase::Disconnected);
        assert!(lock_host.session.status_line().contains("protocol error"));
        // And a guest offering no cable is sent the shared game as always.
        assert_eq!(lock_guest.core.state, 1234);
    }

    /// A peer that keeps sending while this phone's game is not running to take the blocks is
    /// held to a bounded queue.
    #[test]
    fn blocks_waiting_for_a_paused_game_are_capped() {
        let mut host = link_host("a.gb", Some(CABLE));
        let mut guest = link_guest("b.gb", Some(CABLE));
        connect(&mut host, &mut guest);
        for n in 0..LINK_QUEUE_MAX + 10 {
            guest.send_link_block(&[b'C', n as u8, 0]);
        }
        host.receive(&guest.take_outgoing());
        assert_eq!(host.take_link_blocks().len(), LINK_QUEUE_MAX);
        assert_eq!(host.link_counts(), (LINK_QUEUE_MAX as u64 + 10, 0, 10));
        assert_eq!(host.phase(), Phase::Running, "not a protocol error");
    }

    #[test]
    fn every_link_state_has_its_own_plain_line() {
        let host = link_host("a.gb", Some(CABLE));
        assert_eq!(host.status_line(), "Link cable: waiting for the other phone to join");
        let mut guest = link_guest("a.gb", Some(CABLE));
        assert_eq!(guest.status_line(), "Link cable: reaching the other phone");
        guest.transport_connected();
        assert_eq!(
            guest.status_line(),
            "Link cable: connected, waiting for the other phone to say yes"
        );
        let mut host = host;
        host.transport_connected();
        assert_eq!(
            host.status_line(),
            "Link cable: the other phone is here, checking its game"
        );
        host.receive(&guest.take_outgoing());
        guest.receive(&host.take_outgoing());
        assert_eq!(guest.status_line(), "Link cable plugged in");
        host.poll(1000.0);
        guest.receive(&host.take_outgoing());
        guest.poll(1000.0);
        host.poll(1012.0);
        host.receive(&guest.take_outgoing());
        assert_eq!(host.status_line(), "Link cable plugged in (ping 12 ms)");

        let mut lost = link_guest("a.gb", Some(CABLE));
        lost.poll(0.0);
        lost.poll(DEFAULT_TIMEOUT_MS + 500.0);
        assert_eq!(
            lost.status_line(),
            "Link cable unplugged: couldn't reach the other phone in 10 seconds. Check the \
             address, and that both phones are on the same Wi-Fi"
        );
        for line in [host.status_line(), lost.status_line()] {
            assert!(!line.contains("online:"), "{line}");
        }
    }

    #[test]
    fn the_cable_check_covers_every_pairing() {
        assert_eq!(NetplaySession::cable_mismatch(None, ""), None);
        assert_eq!(NetplaySession::cable_mismatch(Some(CABLE), CABLE), None);
        assert_eq!(
            NetplaySession::cable_mismatch(Some(CABLE), ""),
            Some(LINK_NEEDS_A_CABLE_GAME)
        );
        assert_eq!(
            NetplaySession::cable_mismatch(None, CABLE),
            Some(LINK_NEEDS_A_CABLE_GAME)
        );
        assert_eq!(
            NetplaySession::cable_mismatch(Some(CABLE), "cboy-link-2"),
            Some(LINK_DIFFERENT_CABLES)
        );
        // An empty name is no cable at all, not a cable called "".
        assert_eq!(link_host("a", Some("")).mode(), Mode::Lockstep);
    }
}
