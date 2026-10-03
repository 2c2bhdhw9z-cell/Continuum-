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

use super::EmulatorBridge;
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

    pub(super) fn netplay_owns_tick(&self) -> bool {
        self.session.is_some() && self.netplay_is_live()
    }

    pub(super) fn refuse_during_netplay(&self, what: &str) -> Result<(), BridgeError> {
        if self.netplay_is_live() {
            return Err(BridgeError::SaveState(format!(
                "{what} is switched off during online play"
            )));
        }
        Ok(())
    }

    pub fn netplay(&self) -> Option<&NetplaySession> {
        self.netplay.as_ref()
    }

    fn netplay_local_info(&self, content_path: &str) -> Result<PeerInfo, BridgeError> {
        let session = self.session.as_ref().ok_or(BridgeError::NoSession)?;
        let state_size = session.core.state_size();
        if state_size == 0 {
            return Err(BridgeError::SaveState(
                "this core cannot save states, so it cannot play online".into(),
            ));
        }
        let fingerprint = if content_path.is_empty() {
            fnv1a64(session.content_id.as_bytes())
        } else {
            content_fingerprint(content_path)
        };
        Ok(PeerInfo {
            version: PROTOCOL_VERSION,
            core_id: session.core_id.clone(),
            core_version: session.core.version().unwrap_or("").to_string(),
            content_name: session.content_id.clone(),
            content_fingerprint: fingerprint,
            state_size: state_size as u64,
            active_cheats: session.cheats.iter().filter(|c| c.enabled).count() as u32,
            options_hash: options_hash(&session.core.core_options()),
        })
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
        let info = self.netplay_local_info(content_path)?;
        self.netplay_prepare();
        self.netplay = Some(NetplaySession::new_host(config, info));
        Ok(())
    }

    /// Prepares to join a host with the running game.
    pub fn netplay_join(&mut self, content_path: &str) -> Result<(), BridgeError> {
        let info = self.netplay_local_info(content_path)?;
        self.netplay_prepare();
        self.netplay = Some(NetplaySession::new_guest(info));
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
        self.netplay
            .take()
            .map(|mut netplay| netplay.take_outgoing())
            .unwrap_or_default()
    }

    /// Runs whatever the handshake asked of the engine.
    fn service_netplay_requests(&mut self) {
        while let Some(request) = self.netplay.as_mut().and_then(NetplaySession::take_request) {
            match request {
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
                        };
                        snapshot.ports[0] = inputs[0].to_port();
                        snapshot.ports[1] = inputs[1].to_port();
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
            pokes: Vec::new(),
            search: None,
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
}
