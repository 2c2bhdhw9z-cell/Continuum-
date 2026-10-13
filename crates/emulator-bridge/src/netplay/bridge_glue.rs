//! The engine half of online play: where [`crate::netplay::NetplaySession`] meets the running
//! core. A child module of `bridge.rs` so it can use the bridge's private fields.
//!
//! What this adds to the engine, and nothing more:
//!
//! - a lockstep tick that runs a frame only when the session says both inputs are known, feeds
//!   player 1 to port 0 and player 2 to port 1, and re-anchors the pacer while stalled so a
//!   stall does not turn into a catch-up sprint afterwards;
//! - the two state operations the handshake needs (the host saves AND loads its own state, so
//!   both peers start from bytes that went through the same `retro_unserialize`);
//! - a state hash every N frames for desync detection;
//! - refusals for everything that would make the two phones diverge.
//!
//! And for a link cable (a core that offers libretro's packet interface, cBoy for a Game Boy
//! game), which is not lockstep at all: the ordinary tick runs each phone's own game, and around
//! every frame the cable is pumped (what the other phone sent goes into the core before the
//! frame, the core's `poll` sends after it), the session's keepalive runs once a tick even while
//! paused, and the core's cable goes in and comes out when the session says. Every call into the
//! core's cable is made here, between frames, on the thread that runs them, as libretro asks.

use super::{EmulatorBridge, Session};
use crate::error::BridgeError;
use crate::input::{InputSnapshot, PortState, MAX_PORTS};
use crate::netplay::{
    content_fingerprint, fnv1a64, options_hash, state_hash, NetplayConfig, NetplaySession,
    PeerInfo, Request, Step, WireInput, PROTOCOL_VERSION,
};
use crate::TickReport;

impl EmulatorBridge {
    /// True while a peer could be affected by what this phone does.
    pub fn netplay_is_live(&self) -> bool {
        self.netplay.as_ref().is_some_and(NetplaySession::is_live)
    }

    /// True when online play for the running game is a link cable (each phone plays its own game)
    /// rather than one shared game: the session's own kind while there is one, otherwise what the
    /// running core offers.
    pub fn netplay_link_cable(&self) -> bool {
        match self.netplay.as_ref() {
            Some(netplay) => netplay.is_link(),
            None => self
                .session
                .as_ref()
                .is_some_and(|session| session.core.link_protocol().is_some()),
        }
    }

    /// Only a shared game takes the tick over. A link cable runs in the ordinary tick.
    pub(super) fn netplay_owns_tick(&self) -> bool {
        self.session.is_some()
            && self
                .netplay
                .as_ref()
                .is_some_and(|netplay| netplay.is_live() && !netplay.is_link())
    }

    /// Refuses `what` while a peer is playing along. For anything that changes the emulated
    /// machine on this phone alone: lockstep only carries the two players' pads, so the other
    /// phone never hears about it and the two games drift apart from that frame on.
    pub(super) fn refuse_during_netplay(&self, what: &str) -> Result<(), BridgeError> {
        match self.netplay_refusal(what) {
            Some(sentence) => Err(BridgeError::SaveState(sentence)),
            None => Ok(()),
        }
    }

    /// The same refusal for the actions that answer with a status line instead of a `Result`:
    /// the sentence to show while online play is live, `None` otherwise. One wording for both, so
    /// every refusal reads alike and says why.
    pub(super) fn netplay_refusal(&self, what: &str) -> Option<String> {
        self.netplay_is_live().then(|| {
            if self.netplay.as_ref().is_some_and(NetplaySession::is_link) {
                // libretro.h:2512 asks a frontend to keep time still while a cable is in, and one
                // phone's game jumping about mid-conversation is what would break it.
                format!("{what} is off while the link cable is on")
            } else {
                format!("{what} is off during online play: the other phone would not do the same")
            }
        })
    }

    pub fn netplay(&self) -> Option<&NetplaySession> {
        self.netplay.as_ref()
    }

    /// This phone's half of the handshake, and the cable its core offers (`None` for none, which
    /// makes the session one shared game).
    fn netplay_local_info(
        &self,
        content_path: &str,
    ) -> Result<(PeerInfo, Option<String>), BridgeError> {
        let session = self.session.as_ref().ok_or(BridgeError::NoSession)?;
        let cable = session.core.link_protocol();
        let state_size = session.core.state_size();
        // A cable carries no state, so only a shared game needs one.
        if state_size == 0 && cable.is_none() {
            return Err(BridgeError::SaveState(
                "this core cannot save states, so it cannot play online".into(),
            ));
        }
        let fingerprint = if content_path.is_empty() {
            fnv1a64(session.content_id.as_bytes())
        } else {
            content_fingerprint(content_path)
        };
        let info = PeerInfo {
            version: PROTOCOL_VERSION,
            core_id: session.core_id.clone(),
            core_version: session.core.version().unwrap_or("").to_string(),
            content_name: session.content_id.clone(),
            content_fingerprint: fingerprint,
            state_size: state_size as u64,
            // The user's typed codes, not the core entries they were split into.
            active_cheats: session.enabled_codes as u32,
            options_hash: options_hash(&session.core.core_options()),
        };
        Ok((info, cable))
    }

    /// Everything that must be true on both phones before the first lockstep frame.
    fn netplay_prepare(&mut self) {
        self.rewinding = false;
        self.speed = 1.0;
        self.pacer.set_speed(1.0);
        self.apply_audio_speed();
    }

    /// Starts hosting with the running game. The transport is opened by the platform.
    pub fn netplay_host(
        &mut self,
        content_path: &str,
        config: NetplayConfig,
    ) -> Result<(), BridgeError> {
        let (info, cable) = self.netplay_local_info(content_path)?;
        self.netplay_prepare();
        // A session this replaces may still have the cable in.
        self.service_netplay_requests();
        self.netplay = Some(NetplaySession::new_host(config, info).with_link(cable));
        Ok(())
    }

    /// Prepares to join a host with the running game.
    pub fn netplay_join(&mut self, content_path: &str) -> Result<(), BridgeError> {
        let (info, cable) = self.netplay_local_info(content_path)?;
        self.netplay_prepare();
        self.service_netplay_requests();
        self.netplay = Some(NetplaySession::new_guest(info).with_link(cable));
        Ok(())
    }

    pub fn netplay_transport_connected(&mut self) {
        if let Some(netplay) = self.netplay.as_mut() {
            netplay.transport_connected();
        }
    }

    pub fn netplay_transport_lost(&mut self, reason: &str) {
        if let Some(netplay) = self.netplay.as_mut() {
            netplay.transport_lost(reason);
        }
    }

    pub fn netplay_receive(&mut self, bytes: &[u8]) {
        if let Some(netplay) = self.netplay.as_mut() {
            netplay.receive(bytes);
        }
    }

    pub fn netplay_take_outgoing(&mut self) -> Vec<u8> {
        self.netplay
            .as_mut()
            .map(NetplaySession::take_outgoing)
            .unwrap_or_default()
    }

    /// Ends the session from this side. The goodbye is left in the outgoing bytes.
    pub fn netplay_leave(&mut self, reason: &str) {
        if let Some(netplay) = self.netplay.as_mut() {
            netplay.leave(reason);
        }
    }

    /// Forgets the session entirely, returning any last bytes (the goodbye) to flush.
    pub fn netplay_clear(&mut self) -> Vec<u8> {
        // A cable the session ended with goes back out of the core before the session is gone.
        self.service_netplay_requests();
        self.netplay
            .take()
            .map(|mut netplay| netplay.take_outgoing())
            .unwrap_or_default()
    }

    /// Runs whatever the session asked of the engine: the handshake's two state operations, and
    /// the link cable going in and coming out.
    pub(super) fn service_netplay_requests(&mut self) {
        while let Some(request) = self.netplay.as_mut().and_then(NetplaySession::take_request) {
            match request {
                Request::StartLink { local, peer } => {
                    let Some(session) = self.session.as_mut() else {
                        continue;
                    };
                    let accepted = session.core.link_start(local, peer);
                    if let Some(netplay) = self.netplay.as_mut() {
                        // Whatever the core sent from inside `start` goes out with the rest.
                        for block in session.core.link_take_sent() {
                            netplay.send_link_block(&block);
                        }
                        if !accepted {
                            // Ends the session, which queues the matching unplug for this loop.
                            netplay.link_refused_by_core();
                        }
                    }
                }
                Request::StopLink { peer } => {
                    if let Some(session) = self.session.as_mut() {
                        session.core.link_stop(peer);
                        // Too late to carry anything: the session is over.
                        session.core.link_take_sent();
                    }
                }
                Request::CaptureState => {
                    let result = self
                        .save_state()
                        .and_then(|bytes| self.load_state_unchecked(&bytes).map(|()| bytes))
                        .map_err(|err| err.to_string());
                    if let Some(netplay) = self.netplay.as_mut() {
                        netplay.host_state_captured(result);
                    }
                }
                Request::LoadState(bytes) => {
                    let result = self
                        .load_state_unchecked(&bytes)
                        .map_err(|err| err.to_string());
                    if let Some(netplay) = self.netplay.as_mut() {
                        netplay.guest_state_loaded(result);
                    }
                }
            }
        }
    }

    /// Link mode, once a tick before anything runs: the cable going in or out, and the session's
    /// keepalive and silence timeout. Here rather than with the frames, so a paused game still
    /// answers pings and still notices that the other phone has gone.
    pub(super) fn tick_link_cable(&mut self, now_ms: f64) {
        if !self.netplay.as_ref().is_some_and(NetplaySession::is_link) {
            return;
        }
        self.service_netplay_requests();
        if let Some(netplay) = self.netplay.as_mut() {
            netplay.poll(now_ms);
        }
        // The poll can end the session (silence), and then the cable comes out on this tick.
        self.service_netplay_requests();
    }

    /// Link mode, just before one frame: every block the other phone sent, in its order.
    pub(super) fn link_before_frame(session: &mut Session, netplay: Option<&mut NetplaySession>) {
        let Some(netplay) = netplay.filter(|netplay| netplay.is_link()) else {
            return;
        };
        let from = netplay.peer_client_id();
        for block in netplay.take_link_blocks() {
            session.core.link_receive(&block, from);
        }
    }

    /// Link mode, just after one frame: the core's turn to send, and what it sent goes out.
    pub(super) fn link_after_frame(session: &mut Session, netplay: Option<&mut NetplaySession>) {
        let Some(netplay) = netplay.filter(|netplay| netplay.is_link() && netplay.is_live()) else {
            return;
        };
        session.core.link_poll();
        for block in session.core.link_take_sent() {
            netplay.send_link_block(&block);
        }
    }

    /// The lockstep tick.
    pub(super) fn tick_netplay(&mut self, now_ms: f64) -> Result<TickReport, BridgeError> {
        self.service_netplay_requests();

        let Self {
            session,
            renderer,
            sink,
            pacer,
            gamepads,
            netplay,
            state_scratch,
            ..
        } = self;
        let (Some(session), Some(netplay)) = (session.as_mut(), netplay.as_mut()) else {
            return Ok(TickReport::default());
        };

        let mut steps = 0u32;
        let mut dropped = 0u32;
        let mut stalled = false;
        if !session.paused {
            let plan = pacer.plan(now_ms);
            dropped = plan.dropped;
            // Quantised once per tick, and the SAME quantised value is what this phone's core
            // reads, so both cores see identical bits.
            let local = WireInput::from_port(&gamepads.snapshot().port(0));
            for _ in 0..plan.steps {
                match netplay.begin_frame(local) {
                    Step::Run(inputs) => {
                        let mut snapshot = InputSnapshot {
                            ports: [PortState::default(); MAX_PORTS],
                            keys: Default::default(),
                        };
                        snapshot.ports[0] = inputs[0].to_port();
                        snapshot.ports[1] = inputs[1].to_port();
                        // After the exchange, so both phones swap the same untranslated bits.
                        crate::input::core_layout::translate_for(&session.core_id, &mut snapshot);
                        session.core.run_frame(&snapshot)?;
                        session.core.drain_audio(sink.as_mut());
                        steps += 1;
                        if netplay.end_frame() {
                            let size = session.core.state_size();
                            state_scratch.clear();
                            state_scratch.resize(size, 0);
                            match session.core.save_state(state_scratch) {
                                Ok(written) => {
                                    netplay.record_checksum(state_hash(&state_scratch[..written]))
                                }
                                Err(err) => log::debug!("netplay checksum skipped: {err}"),
                            }
                        }
                    }
                    Step::Stall | Step::Wait => {
                        stalled = true;
                        break;
                    }
                }
            }
            if stalled {
                // The frames not run are not owed. Without this the pacer would try to make
                // the stall up in a burst the moment input arrives.
                pacer.resync(now_ms);
            }
        }
        netplay.poll(now_ms);

        let mut presented = false;
        if let Some(renderer) = renderer.as_mut() {
            if steps > 0 {
                if let Err(err) = crate::gfx::vulkan_hw::apply_pending_to_renderer(renderer) {
                    log::debug!("vulkan HW adopt skipped: {err}");
                }
            }
            let frame = if steps > 0 {
                session.core.video()
            } else {
                None
            };
            renderer.present(frame)?;
            presented = true;
        }

        Ok(TickReport {
            steps,
            dropped,
            presented,
            resynced: stalled,
            display_fps: pacer.display_fps(),
            frame_count: session.core.frame_count(),
            audio: sink.stats(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cores::{ContentHint, CoreDescriptor, EmulatorCore};
    use crate::frame::{FrameGeometry, FrameView, PixelFormat};

    /// A core whose whole state is a counter folded with both ports' buttons each frame.
    struct LockstepCore {
        descriptor: CoreDescriptor,
        state: u64,
        frames: u64,
    }

    impl LockstepCore {
        fn new(seed: u64) -> Self {
            Self {
                descriptor: CoreDescriptor {
                    id: "lock".into(),
                    display_name: "Lockstep test core".into(),
                    systems: vec!["test".into()],
                    geometry: FrameGeometry::new(256, 240, 4.0 / 3.0),
                    target_fps: 60.0,
                    audio_sample_rate: 48_000,
                    pixel_format: PixelFormat::Xrgb8888,
                    module_url: String::new(),
                    priority: 0,
                },
                state: seed,
                frames: 0,
            }
        }
    }

    impl EmulatorCore for LockstepCore {
        fn descriptor(&self) -> &CoreDescriptor {
            &self.descriptor
        }
        fn load_content(&mut self, _: &[u8], _: &ContentHint) -> Result<(), BridgeError> {
            Ok(())
        }
        fn run_frame(&mut self, input: &InputSnapshot) -> Result<(), BridgeError> {
            let mut bytes = self.state.to_le_bytes().to_vec();
            bytes.extend_from_slice(&input.ports[0].buttons.to_le_bytes());
            bytes.extend_from_slice(&input.ports[1].buttons.to_le_bytes());
            self.state = fnv1a64(&bytes);
            self.frames += 1;
            Ok(())
        }
        fn video(&self) -> Option<FrameView<'_>> {
            None
        }
        fn drain_audio(&mut self, _: &mut dyn crate::audio::AudioSink) {}
        fn reset(&mut self) -> Result<(), BridgeError> {
            Ok(())
        }
        fn state_size(&self) -> usize {
            8
        }
        fn save_state(&self, dst: &mut [u8]) -> Result<usize, BridgeError> {
            dst[..8].copy_from_slice(&self.state.to_le_bytes());
            Ok(8)
        }
        fn load_state(&mut self, src: &[u8]) -> Result<(), BridgeError> {
            let mut a = [0u8; 8];
            a.copy_from_slice(&src[..8]);
            self.state = u64::from_le_bytes(a);
            Ok(())
        }
        fn frame_count(&self) -> u64 {
            self.frames
        }
    }

    /// A bridge with a running session on the test core, without a renderer: the session is
    /// installed directly because `launch` insists on a GPU.
    fn bridge_with(seed: u64) -> EmulatorBridge {
        let mut bridge = EmulatorBridge::new();
        bridge.session = Some(super::super::Session {
            core_id: "lock".into(),
            content_id: "game.sfc".into(),
            core: Box::new(LockstepCore::new(seed)),
            paused: false,
            cheats: Vec::new(),
            enabled_codes: 0,
            pokes: Vec::new(),
            search: None,
            search_region: Default::default(),
            content_name: "game.sfc".into(),
            content_ext: "sfc".into(),
            battery_layout: Default::default(),
        });
        bridge
    }

    fn core_state(bridge: &EmulatorBridge) -> u64 {
        let mut buf = [0u8; 8];
        bridge
            .session
            .as_ref()
            .unwrap()
            .core
            .save_state(&mut buf)
            .unwrap();
        u64::from_le_bytes(buf)
    }

    #[test]
    fn two_engines_play_in_lockstep_and_refuse_divergent_actions() {
        let mut host = bridge_with(7);
        let mut guest = bridge_with(99);
        host.netplay_host("", NetplayConfig::default()).unwrap();
        guest.netplay_join("").unwrap();
        host.netplay_transport_connected();
        guest.netplay_transport_connected();

        // Local player 2 presses B on THEIR port 0; it must reach port 1 on both cores.
        guest.gamepads.set_button(
            0,
            crate::input::PadSource::Keyboard,
            crate::input::Button::B,
            true,
        );

        let mut now = 0.0;
        for _ in 0..300 {
            host.tick(now).unwrap();
            guest.tick(now).unwrap();
            let h = host.netplay_take_outgoing();
            let g = guest.netplay_take_outgoing();
            guest.netplay_receive(&h);
            host.netplay_receive(&g);
            now += 1000.0 / 60.0;
        }
        let np = host.netplay().unwrap();
        assert_eq!(np.phase(), crate::netplay::Phase::Running);
        assert!(np.frame() > 200, "frame {}", np.frame());
        assert!(np.checks_compared() > 0);
        assert_eq!(np.desync_frame(), None);
        assert_eq!(guest.netplay().unwrap().desync_frame(), None);
        let host_p2 = host.session.as_ref().map(|s| s.core.frame_count()).unwrap();
        assert!(host_p2 > 200);

        // Refusals while live.
        assert!(host.load_state(&[0; 8]).is_err());
        assert!(host.reset().is_err());
        assert!(host.rewind_step().is_err());
        host.set_rewinding(true);
        assert!(!host.is_rewinding());
        host.set_speed(4.0);
        assert_eq!(host.speed(), 1.0);

        // Leaving tells the guest, and both drop back to ordinary play.
        host.netplay_leave("test over");
        let bye = host.netplay_take_outgoing();
        guest.netplay_receive(&bye);
        assert!(!guest.netplay_is_live());
        assert!(guest.netplay().unwrap().status_line().contains("test over"));
        assert!(guest.load_state(&[0; 8]).is_ok());
        assert_eq!(core_state(&guest), 0);
    }

    #[test]
    fn hosting_needs_a_running_game() {
        let mut bridge = EmulatorBridge::new();
        assert!(bridge.netplay_host("", NetplayConfig::default()).is_err());
    }

    type Witnessed = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

    /// The lockstep core, plus a note of every change the engine makes to it from outside a frame.
    /// So a test can show a refusal left the machine untouched, not only that an error came back,
    /// and that the same calls do reach it once online play is over.
    struct WitnessCore {
        inner: LockstepCore,
        ram: Vec<u8>,
        log: Witnessed,
    }

    impl WitnessCore {
        fn note(&self, what: String) {
            self.log.lock().unwrap().push(what);
        }
    }

    impl EmulatorCore for WitnessCore {
        fn descriptor(&self) -> &CoreDescriptor {
            self.inner.descriptor()
        }
        fn load_content(&mut self, rom: &[u8], hint: &ContentHint) -> Result<(), BridgeError> {
            self.inner.load_content(rom, hint)
        }
        fn run_frame(&mut self, input: &InputSnapshot) -> Result<(), BridgeError> {
            self.inner.run_frame(input)
        }
        fn video(&self) -> Option<FrameView<'_>> {
            None
        }
        fn drain_audio(&mut self, _: &mut dyn crate::audio::AudioSink) {}
        fn reset(&mut self) -> Result<(), BridgeError> {
            self.note("reset".into());
            Ok(())
        }
        fn state_size(&self) -> usize {
            self.inner.state_size()
        }
        fn save_state(&self, dst: &mut [u8]) -> Result<usize, BridgeError> {
            self.inner.save_state(dst)
        }
        fn load_state(&mut self, src: &[u8]) -> Result<(), BridgeError> {
            self.inner.load_state(src)
        }
        fn supports_cheats(&self) -> bool {
            true
        }
        fn reset_cheats(&mut self) -> Result<(), BridgeError> {
            self.note("cheats cleared".into());
            Ok(())
        }
        fn set_cheat(&mut self, index: u32, _: bool, code: &str) -> Result<(), BridgeError> {
            self.note(format!("cheat {index} {code}"));
            Ok(())
        }
        fn set_core_option(&mut self, key: &str, value: &str) -> Result<(), BridgeError> {
            self.note(format!("option {key}={value}"));
            Ok(())
        }
        fn memory_region(&self, id: u32) -> Option<&[u8]> {
            (id == crate::memory::MEMORY_SAVE_RAM || id == crate::memory::MEMORY_SYSTEM_RAM)
                .then_some(self.ram.as_slice())
        }
        fn memory_region_mut(&mut self, id: u32) -> Option<&mut [u8]> {
            if id != crate::memory::MEMORY_SAVE_RAM && id != crate::memory::MEMORY_SYSTEM_RAM {
                return None;
            }
            self.note(format!("memory {id} written"));
            Some(self.ram.as_mut_slice())
        }
        fn set_controller_port_device(&mut self, port: u32, device: u32) -> Result<(), BridgeError> {
            self.note(format!("port {port} device {device}"));
            Ok(())
        }
        fn controller_types(&self, _: u32) -> Vec<(String, u32)> {
            vec![
                ("RetroPad".into(), crate::input::RETRO_DEVICE_JOYPAD),
                ("Mouse".into(), crate::input::RETRO_DEVICE_MOUSE),
                ("DualShock".into(), crate::input::RETRO_DEVICE_ANALOG),
            ]
        }
        fn disk_status(&self) -> Option<crate::cores::disk::DiskStatus> {
            Some(crate::cores::disk::DiskStatus {
                count: 2,
                index: 0,
                ejected: false,
                labels: Vec::new(),
                extended: false,
            })
        }
        fn disk_insert(&mut self, index: u32) -> Result<String, String> {
            self.note(format!("disc {index}"));
            Ok(format!("disc {}", index + 1))
        }
        fn frame_count(&self) -> u64 {
            self.inner.frame_count()
        }
    }

    fn witness_bridge(log: Witnessed) -> EmulatorBridge {
        let mut bridge = bridge_with(7);
        if let Some(session) = bridge.session.as_mut() {
            session.core = Box::new(WitnessCore {
                inner: LockstepCore::new(7),
                ram: vec![0; 64],
                log,
            });
        }
        bridge
    }

    const REFUSED: &str = "is off during online play: the other phone would not do the same";

    fn refused<T>(result: Result<T, BridgeError>) -> bool {
        result.err().is_some_and(|err| err.to_string().contains(REFUSED))
    }

    #[test]
    fn every_change_to_one_phones_machine_is_refused_while_online() {
        let _options = crate::cores::options::test_guard();
        crate::cores::options::reset_for_tests();
        let log = Witnessed::default();
        let mut bridge = witness_bridge(log.clone());
        bridge.input_session_started("snes", "game");
        bridge.netplay_host("", NetplayConfig::default()).unwrap();
        assert!(bridge.netplay_is_live());

        // The ones that answer with a Result.
        assert!(refused(bridge.clear_cheats()));
        assert!(refused(bridge.write_memory(crate::memory::MEMORY_SYSTEM_RAM, 0, &[1])));
        assert!(refused(bridge.restore_battery_save(&[1, 2])));
        assert!(refused(bridge.load_battery_save_file("/does/not/matter.srm")));
        assert!(refused(bridge.set_core_option("k", "v")));
        assert!(refused(bridge.set_controller_port_device(0, 2)));
        assert!(refused(bridge.reset()));

        // The settings screen, for the running core.
        for result in [
            bridge.set_core_option_value("lock", "k", "v", false),
            bridge.reset_core_options("lock"),
            bridge.reset_game_options("lock"),
        ] {
            assert!(result.unwrap_err().contains(REFUSED));
        }
        // A core that is not running only has its saved settings changed, which neither phone's
        // game sees, so that is still allowed.
        assert!(bridge.set_core_option_value("another", "k", "v", false).is_ok());

        // The ones that answer with a status line.
        for line in [
            bridge.swap_disc(),
            bridge.insert_disc(1),
            bridge.cycle_palette("gb"),
            bridge.cycle_resolution(),
            bridge.set_mouse_mode(0, true),
            bridge.toggle_analog_mode(),
            bridge.shake(),
            bridge.set_controller_type("snes", 0, "Mouse"),
        ] {
            assert!(line.contains(REFUSED), "{line}");
        }

        // The Atari switches and the FDS flip are presses the engine queues itself.
        let rename = |bridge: &mut EmulatorBridge, core_id: &str| {
            if let Some(session) = bridge.session.as_mut() {
                session.core_id = core_id.into();
            }
        };
        rename(&mut bridge, "stella2023");
        assert!(bridge.toggle_tv_type().contains(REFUSED));
        assert!(bridge.toggle_difficulty(true).contains(REFUSED));
        assert!(bridge.tv_type_is_color(), "a refused switch must not flip what the UI shows");
        rename(&mut bridge, "fceumm");
        bridge.actions.new_session("game.fds");
        assert!(bridge.is_fds_session());
        assert!(bridge.swap_disc().contains(REFUSED));
        assert!(bridge.insert_disc(0).contains(REFUSED));
        assert_eq!(bridge.actions.pulses_pending(), 0, "no press may be left to fire later");
        rename(&mut bridge, "lock");

        assert!(log.lock().unwrap().is_empty(), "the core was changed: {:?}", log.lock().unwrap());

        // Once online play is over, the same calls reach the core. Without this the silence above
        // could just mean the test core never listens.
        bridge.netplay_leave("done");
        assert!(!bridge.netplay_is_live());
        bridge.clear_cheats().unwrap();
        bridge.write_memory(crate::memory::MEMORY_SYSTEM_RAM, 0, &[1]).unwrap();
        bridge.set_core_option("k", "v").unwrap();
        bridge.set_controller_port_device(0, 2).unwrap();
        assert_eq!(bridge.insert_disc(1), "disc 2");
        assert_eq!(
            *log.lock().unwrap(),
            vec![
                "cheats cleared".to_string(),
                format!("memory {} written", crate::memory::MEMORY_SYSTEM_RAM),
                "option k=v".into(),
                "port 0 device 2".into(),
                "disc 1".into(),
            ]
        );
        crate::cores::options::reset_for_tests();
    }

    // ------------------------------------------------------------ the link cable

    #[derive(Default)]
    struct CableLog {
        /// Every time the engine plugged the cable in or pulled it out, in order.
        calls: Vec<String>,
        /// Every block that reached this core, in order.
        received: Vec<Vec<u8>>,
    }
    type CableWitness = std::sync::Arc<std::sync::Mutex<CableLog>>;

    /// The lockstep core with a cable on it: while plugged in, every frame its game puts one
    /// numbered block on the cable (`[name, n]`), which goes out when the engine polls.
    struct CableCore {
        inner: LockstepCore,
        name: u8,
        plugged: bool,
        next: u8,
        on_the_cable: Vec<Vec<u8>>,
        sent: Vec<Vec<u8>>,
        log: CableWitness,
    }

    impl EmulatorCore for CableCore {
        fn descriptor(&self) -> &CoreDescriptor {
            self.inner.descriptor()
        }
        fn load_content(&mut self, rom: &[u8], hint: &ContentHint) -> Result<(), BridgeError> {
            self.inner.load_content(rom, hint)
        }
        fn run_frame(&mut self, input: &InputSnapshot) -> Result<(), BridgeError> {
            self.inner.run_frame(input)?;
            if self.plugged {
                self.on_the_cable.push(vec![self.name, self.next]);
                self.next = self.next.wrapping_add(1);
            }
            Ok(())
        }
        fn video(&self) -> Option<FrameView<'_>> {
            None
        }
        fn drain_audio(&mut self, _: &mut dyn crate::audio::AudioSink) {}
        fn reset(&mut self) -> Result<(), BridgeError> {
            Ok(())
        }
        fn state_size(&self) -> usize {
            self.inner.state_size()
        }
        fn save_state(&self, dst: &mut [u8]) -> Result<usize, BridgeError> {
            self.inner.save_state(dst)
        }
        fn load_state(&mut self, src: &[u8]) -> Result<(), BridgeError> {
            self.inner.load_state(src)
        }
        fn frame_count(&self) -> u64 {
            self.inner.frame_count()
        }
        fn link_protocol(&self) -> Option<String> {
            Some("test-cable-1".into())
        }
        fn link_start(&mut self, local: u16, peer: u16) -> bool {
            self.log.lock().unwrap().calls.push(format!("start {local} {peer}"));
            self.plugged = true;
            true
        }
        fn link_receive(&mut self, block: &[u8], _from: u16) {
            assert!(self.plugged, "a block reached a core with no cable in");
            self.log.lock().unwrap().received.push(block.to_vec());
        }
        fn link_poll(&mut self) {
            self.sent.append(&mut self.on_the_cable);
        }
        fn link_stop(&mut self, peer: u16) {
            self.log.lock().unwrap().calls.push(format!("stop {peer}"));
            self.plugged = false;
        }
        fn link_take_sent(&mut self) -> Vec<Vec<u8>> {
            std::mem::take(&mut self.sent)
        }
    }

    fn cable_bridge(seed: u64, name: u8, log: CableWitness) -> EmulatorBridge {
        let mut bridge = bridge_with(seed);
        if let Some(session) = bridge.session.as_mut() {
            session.core = Box::new(CableCore {
                inner: LockstepCore::new(seed),
                name,
                plugged: false,
                next: 0,
                on_the_cable: Vec::new(),
                sent: Vec::new(),
                log,
            });
        }
        bridge
    }

    /// One tick on each phone, then the connection carries each one's bytes to the other.
    fn link_round(a: &mut EmulatorBridge, b: &mut EmulatorBridge, now: f64) {
        a.tick(now).unwrap();
        b.tick(now).unwrap();
        let from_a = a.netplay_take_outgoing();
        let from_b = b.netplay_take_outgoing();
        b.netplay_receive(&from_a);
        a.netplay_receive(&from_b);
    }

    const FRAME_MS: f64 = 1000.0 / 60.0;

    /// Two engines on a cable: each runs its own game (no state crosses, and the two machines stay
    /// different), the cable goes in once on each with the right numbers, every block crosses in
    /// order, a paused phone keeps the connection alive and gets its blocks when it resumes, time
    /// is held still while linked, and leaving pulls both cables out.
    #[test]
    fn two_engines_on_a_link_cable_each_play_their_own_game() {
        let (host_log, guest_log) = (CableWitness::default(), CableWitness::default());
        let mut host = cable_bridge(7, b'H', host_log.clone());
        let mut guest = cable_bridge(99, b'G', guest_log.clone());
        assert!(host.netplay_link_cable(), "the core offers a cable");
        host.netplay_host("", NetplayConfig::default()).unwrap();
        guest.netplay_join("").unwrap();
        host.netplay_transport_connected();
        guest.netplay_transport_connected();

        let mut now = 0.0;
        for _ in 0..120 {
            link_round(&mut host, &mut guest, now);
            now += FRAME_MS;
        }
        for bridge in [&host, &guest] {
            let np = bridge.netplay().unwrap();
            assert!(np.is_link());
            assert_eq!(np.phase(), crate::netplay::Phase::Running);
            assert!(bridge.netplay_is_live());
            assert!(bridge.netplay_link_cable());
        }
        assert_eq!(host_log.lock().unwrap().calls, ["start 0 1"]);
        assert_eq!(guest_log.lock().unwrap().calls, ["start 1 0"]);
        // Each phone's blocks reached the other, whole, in order, with none missing.
        for (log, name) in [(&host_log, b'G'), (&guest_log, b'H')] {
            let received = &log.lock().unwrap().received;
            assert!(received.len() > 100, "{} blocks", received.len());
            for (n, block) in received.iter().enumerate() {
                assert_eq!(block, &vec![name, n as u8]);
            }
        }
        // Two games, not one: nothing replaced the guest's machine with the host's.
        assert_ne!(core_state(&host), core_state(&guest));

        // Time stands still while linked.
        let refused = "is off while the link cable is on";
        assert!(host.load_state(&[0; 8]).unwrap_err().to_string().contains(refused));
        assert!(guest.rewind_step().unwrap_err().to_string().contains(refused));
        host.set_rewinding(true);
        assert!(!host.is_rewinding());
        guest.set_speed(4.0);
        assert_eq!(guest.speed(), 1.0);

        // The host pauses for longer than the silence timeout: its tick still answers the other
        // phone, so neither side gives up, and nothing reaches the paused game.
        let before = host_log.lock().unwrap().received.len();
        host.pause();
        for _ in 0..900 {
            link_round(&mut host, &mut guest, now);
            now += FRAME_MS;
        }
        assert!(host.netplay_is_live() && guest.netplay_is_live(), "15 s paused is not silence");
        assert_eq!(host_log.lock().unwrap().received.len(), before, "a paused game takes nothing");
        host.resume(now);
        now += FRAME_MS;
        link_round(&mut host, &mut guest, now);
        let after = host_log.lock().unwrap().received.len();
        assert!(after >= before + 900, "the blocks held while paused arrive: {before} -> {after}");

        // Leaving: this phone's cable comes out when the session is forgotten, the other phone's
        // when its next tick hears the goodbye.
        guest.netplay_leave("the other player went back to the library");
        let bye = guest.netplay_clear();
        assert_eq!(guest_log.lock().unwrap().calls, ["start 1 0", "stop 0"]);
        host.netplay_receive(&bye);
        host.tick(now).unwrap();
        assert_eq!(host_log.lock().unwrap().calls, ["start 0 1", "stop 1"]);
        assert!(!host.netplay_is_live());
        assert_eq!(
            host.netplay().unwrap().status_line(),
            "Link cable unplugged: the other player went back to the library"
        );
        // And time moves again.
        assert!(host.load_state(&[0; 8]).is_ok());
    }

    /// A connection that drops is noticed on the network's own thread, but the core's cable only
    /// ever comes out on the engine's tick (libretro asks for its calls on the frame's thread).
    /// And closing the game pulls the cable before the core goes.
    #[test]
    fn the_cable_comes_out_on_the_tick_and_before_the_game_closes() {
        let (host_log, guest_log) = (CableWitness::default(), CableWitness::default());
        let mut host = cable_bridge(7, b'H', host_log.clone());
        let mut guest = cable_bridge(99, b'G', guest_log.clone());
        host.netplay_host("", NetplayConfig::default()).unwrap();
        guest.netplay_join("").unwrap();
        host.netplay_transport_connected();
        guest.netplay_transport_connected();
        let mut now = 0.0;
        for _ in 0..10 {
            link_round(&mut host, &mut guest, now);
            now += FRAME_MS;
        }
        guest.netplay_transport_lost("POSIXErrorCode(rawValue: 54): Connection reset by peer");
        assert_eq!(guest_log.lock().unwrap().calls, ["start 1 0"], "not from the network thread");
        guest.tick(now).unwrap();
        assert_eq!(guest_log.lock().unwrap().calls, ["start 1 0", "stop 0"]);
        assert_eq!(
            guest.netplay().unwrap().status_line(),
            "Link cable unplugged: the connection to the other phone dropped"
        );

        host.stop();
        assert_eq!(host_log.lock().unwrap().calls, ["start 0 1", "stop 1"]);
    }

    /// The real thing: two engines, each with its own copy of a host build of cBoy loaded through
    /// the real libretro entry points, joined by the session's cable. Phone A drives the cable and
    /// phone B is driven, then the other way round, then phone A drives into a cable that has gone
    /// quiet and must give up rather than freeze; and when the session times out, cBoy's cable is
    /// really unplugged. Opt-in like `tests/cboy_frontend.rs`: CBOY_SO=path/to/libcontinuum_gb.so.
    #[cfg(feature = "native-core")]
    #[test]
    fn two_phones_link_two_cboys_through_the_real_entry_points() {
        let Ok(so) = std::env::var("CBOY_SO") else {
            eprintln!("CBOY_SO not set: the two-cBoy link test was skipped");
            return;
        };
        let _options = crate::cores::options::test_guard();
        crate::cores::options::reset_for_tests();
        let dir = std::env::temp_dir().join(format!("continuum-link-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // Phone A: wait about ten frames, drive $11 (result to $C000); be driven offering $33
        // ($C001); wait for the test's go at $C010, then drive $55 ($C002) and mark $C003; wait for
        // a second go at $C011, then drive $77 ($C004).
        let mut a = init_wram();
        a.extend(delay_frames());
        a.extend(transfer(0x11, 0x81, 0xC000));
        a.extend(transfer(0x33, 0x80, 0xC001));
        a.extend(wait_for(0xC010));
        a.extend(transfer(0x55, 0x81, 0xC002));
        a.extend(store(0xC003, 1));
        a.extend(wait_for(0xC011));
        a.extend(transfer(0x77, 0x81, 0xC004));
        a.extend([0x18, 0xFE]); // JR -2: done
        // Phone B: be driven offering $22 ($C000); wait about ten frames, drive $44 ($C001).
        let mut b = init_wram();
        b.extend(transfer(0x22, 0x80, 0xC000));
        b.extend(delay_frames());
        b.extend(transfer(0x44, 0x81, 0xC001));
        b.extend([0x18, 0xFE]);

        let load = |name: &str, code: &[u8]| -> EmulatorBridge {
            // Its own copy of the library, so the two cBoys share nothing, like two phones.
            let library = dir.join(format!("libcboy_{name}.so"));
            std::fs::copy(&so, &library).unwrap();
            let rom = dir.join(format!("{name}.gb"));
            std::fs::write(&rom, gb_rom(code)).unwrap();
            let descriptor = CoreDescriptor {
                id: "continuum_gb".into(),
                display_name: "cBoy".into(),
                systems: vec!["gb".into()],
                geometry: FrameGeometry::new(160, 144, 10.0 / 9.0),
                target_fps: 59.7275,
                audio_sample_rate: 48_000,
                pixel_format: PixelFormat::Xrgb8888,
                module_url: String::new(),
                priority: 0,
            };
            let mut core = unsafe {
                crate::cores::native_core::NativeLibretroCore::load(
                    descriptor,
                    &library,
                    dir.to_str(),
                    dir.to_str(),
                )
            }
            .unwrap();
            let mut hint = ContentHint::from_filename(rom.to_str().unwrap());
            hint.full_path = rom.to_str().map(str::to_string);
            core.load_content(&[], &hint).unwrap();
            assert_eq!(core.link_protocol().as_deref(), Some("cboy-link-1"));
            let mut bridge = bridge_with(0);
            bridge.session.as_mut().unwrap().core = Box::new(core);
            bridge.session.as_mut().unwrap().core_id = "continuum_gb".into();
            bridge
        };
        let mut phone_a = load("a", &a);
        let mut phone_b = load("b", &b);
        let wram = |bridge: &EmulatorBridge, at: u16| -> u8 {
            bridge.session.as_ref().unwrap().core.memory_region(crate::memory::MEMORY_SYSTEM_RAM)
                .unwrap()[usize::from(at - 0xC000)]
        };
        let poke = |bridge: &mut EmulatorBridge, at: u16| {
            bridge.session.as_mut().unwrap().core.memory_region_mut(crate::memory::MEMORY_SYSTEM_RAM)
                .unwrap()[usize::from(at - 0xC000)] = 1;
        };

        assert!(phone_a.netplay_link_cable() && phone_b.netplay_link_cable());
        phone_a.netplay_host("", NetplayConfig::default()).unwrap();
        phone_b.netplay_join("").unwrap();
        phone_a.netplay_transport_connected();
        phone_b.netplay_transport_connected();

        // Both directions, both clocks: A drives then is driven, B the reverse.
        let mut now = 0.0;
        let mut rounds = 0;
        while (wram(&phone_a, 0xC001) == 0 || wram(&phone_b, 0xC001) == 0) && rounds < 600 {
            link_round(&mut phone_a, &mut phone_b, now);
            now += FRAME_MS;
            rounds += 1;
        }
        assert!(phone_a.netplay().unwrap().is_link());
        assert_eq!(wram(&phone_a, 0xC000), 0x22, "A drove the cable and read B's byte");
        assert_eq!(wram(&phone_b, 0xC000), 0x11, "B was driven and read A's byte");
        assert_eq!(wram(&phone_b, 0xC001), 0x33, "B drove the cable and read A's byte");
        assert_eq!(wram(&phone_a, 0xC001), 0x44, "A was driven and read B's byte");
        let (a_in, a_out, _) = phone_a.netplay().unwrap().link_counts();
        let (b_in, b_out, _) = phone_b.netplay().unwrap().link_counts();
        assert_eq!((a_in, a_out), (b_out, b_in), "every block crossed");
        assert_eq!((a_in, a_out), (2, 2), "a clock and an answer each way");
        eprintln!("both directions done after {rounds} rounds ({:.0} ms of play)", now);

        // The connection goes quiet. A drives into it: it must wait, not be answered at once, and
        // then give up and carry on rather than freeze.
        poke(&mut phone_a, 0xC010);
        for frame in 0..5 {
            phone_a.tick(now).unwrap();
            phone_b.tick(now).unwrap();
            phone_a.netplay_take_outgoing();
            phone_b.netplay_take_outgoing();
            now += FRAME_MS;
            assert_eq!(wram(&phone_a, 0xC002), 0, "still waiting at frame {frame}");
        }
        let mut waited = 5;
        while wram(&phone_a, 0xC003) == 0 && waited < 60 {
            phone_a.tick(now).unwrap();
            phone_b.tick(now).unwrap();
            phone_a.netplay_take_outgoing();
            phone_b.netplay_take_outgoing();
            now += FRAME_MS;
            waited += 1;
        }
        assert_eq!(wram(&phone_a, 0xC002), 0xFF, "a pulled-out cable's all 1s");
        assert_eq!(wram(&phone_a, 0xC003), 1, "and the game played on");
        assert!(waited < 30, "gave up after {waited} frames");
        eprintln!("the quiet cable was given up on after {waited} frames");

        // Ten seconds of nothing ends the session on both phones, and cBoy's cable is unplugged:
        // a byte driven now finishes at once, the way it does on a console with nothing attached.
        for _ in 0..700 {
            phone_a.tick(now).unwrap();
            phone_b.tick(now).unwrap();
            phone_a.netplay_take_outgoing();
            phone_b.netplay_take_outgoing();
            now += FRAME_MS;
        }
        for phone in [&phone_a, &phone_b] {
            assert!(!phone.netplay_is_live());
            let line = phone.netplay().unwrap().status_line();
            assert!(line.starts_with("Link cable unplugged: nothing heard"), "{line}");
        }
        poke(&mut phone_a, 0xC011);
        for _ in 0..2 {
            phone_a.tick(now).unwrap();
            now += FRAME_MS;
        }
        assert_eq!(wram(&phone_a, 0xC004), 0xFF, "unplugged: finished at once, all 1s");

        drop(phone_a);
        drop(phone_b);
        std::fs::remove_dir_all(&dir).ok();
        crate::cores::options::reset_for_tests();
    }

    /// Clears the result bytes and the two go flags.
    fn init_wram() -> Vec<u8> {
        let mut code = vec![0xAF]; // XOR A
        for at in [0xC000u16, 0xC001, 0xC002, 0xC003, 0xC004, 0xC010, 0xC011] {
            code.extend([0xEA, at as u8, (at >> 8) as u8]); // LD (at), A
        }
        code
    }

    /// About ten frames of nothing: 24576 turns of a seven-cycle loop.
    fn delay_frames() -> Vec<u8> {
        vec![
            0x01, 0x00, 0x60, // LD BC, $6000
            0x0B, // DEC BC          <- loop
            0x78, // LD A, B
            0xB1, // OR C
            0x20, 0xFB, // JR NZ, loop
        ]
    }

    /// Puts `byte` in SB, starts a transfer with `sc` ($81 drives the cable, $80 waits to be
    /// driven), waits for it to finish and stores what came back at `at`.
    fn transfer(byte: u8, sc: u8, at: u16) -> Vec<u8> {
        vec![
            0x3E, byte, // LD A, byte
            0xE0, 0x01, // LDH (SB), A
            0x3E, sc, // LD A, sc
            0xE0, 0x02, // LDH (SC), A
            0xF0, 0x02, // LDH A, (SC)     <- wait
            0xCB, 0x7F, // BIT 7, A
            0x20, 0xFA, // JR NZ, wait
            0xF0, 0x01, // LDH A, (SB)
            0xEA, at as u8, (at >> 8) as u8, // LD (at), A
        ]
    }

    /// Spins until the byte at `at` is no longer 0.
    fn wait_for(at: u16) -> Vec<u8> {
        vec![
            0xFA, at as u8, (at >> 8) as u8, // LD A, (at)     <- spin
            0xA7, // AND A
            0x28, 0xFA, // JR Z, spin
        ]
    }

    fn store(at: u16, value: u8) -> Vec<u8> {
        vec![0x3E, value, 0xEA, at as u8, (at >> 8) as u8]
    }

    /// A 32 KB cartridge with no mapper and no save RAM, starting `code` at $0150.
    fn gb_rom(code: &[u8]) -> Vec<u8> {
        let mut rom = vec![0u8; 0x8000];
        rom[0x100..0x104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]); // NOP; JP $0150
        rom[0x134..0x13D].copy_from_slice(b"LINKTEST\0");
        rom[0x147] = 0x00;
        rom[0x148] = 0x00;
        rom[0x149] = 0x00;
        rom[0x14D] = rom[0x134..0x14D]
            .iter()
            .fold(0u8, |x, b| x.wrapping_sub(*b).wrapping_sub(1));
        rom[0x150..0x150 + code.len()].copy_from_slice(code);
        rom
    }

    #[test]
    fn purely_local_display_stays_allowed_while_online() {
        let mut bridge = bridge_with(7);
        bridge.netplay_host("", NetplayConfig::default()).unwrap();
        // Rotation, the look, volume and the TV's fit change what this phone shows or plays, not
        // the machine, so they are not refused.
        assert!(!bridge.rotate_screen().contains(REFUSED));
        bridge.set_volume(0.5);
        assert_eq!(bridge.volume(), 0.5);
        bridge.set_tv_scale_mode(Some(crate::gfx::ScaleMode::Stretch));
        assert_eq!(bridge.tv_scale_mode(), Some(crate::gfx::ScaleMode::Stretch));
    }
}
