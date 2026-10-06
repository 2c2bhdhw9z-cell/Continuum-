// Continuum - the player screen, and the diagnostics panel both screens share.
//
// Before this file the app had one screen: the Metal canvas full bleed, with the debug HUD and the
// Library stacked on top of it. That was correct for bring-up and wrong as a product. There was
// also no way OUT of a running game, which matters more than it sounds: leaving has to call
// `stopSession()` so the core is handed back to the registry and unloaded under the Drop retention
// policy, and nothing was calling it.
//
// Two layout decisions in here are deliberate and are the reason the bug in docs/mobile-player.png
// is not reproduced:
//
//  1. SESSION controls (pause, reset, save state, diagnostics) live in the TOP bar. GAMEPLAY
//     controls own the bottom. In the browser layout those two sets shared one band, and the
//     screenshot shows the result: the D-pad sitting on the aspect selector, B sitting on START,
//     and SCALE disappearing under the D-pad. Separating them by region means they cannot collide
//     no matter what either one is sized at.
//  2. The status line sits at the TOP, under the telemetry strip, and not along the bottom, where
//     it would land on SELECT and START.

import SwiftUI

// MARK: - Where the picture goes

/// The rect the emulated picture is asked to occupy, inside the region the controls leave free.
///
/// THE BUG THIS FIXES, from the device screenshot: the picture sat against the left edge of the
/// area above the controls with a wide band of black to its right, rather than centred and filling
/// the space. `TouchControlsView` correctly reports the whole free region, and handing the canvas
/// the whole of it means handing the renderer a surface whose shape has nothing to do with the
/// game's: a 240x160 Game Boy Advance frame inside a tall portrait band is mostly empty surface,
/// and where the unused part of a surface ends up is a property of the layer, not of this app.
///
/// So the rect asked for is the largest one with the GAME's aspect ratio that fits the free region,
/// centred in it. The surface then has the same shape as the content, the renderer's own aspect fit
/// becomes a no-op inside it, and the picture fills the rect edge to edge with nothing left over to
/// be pushed into a corner. Centring is then simply where the rect is.
///
/// NOTHING ABOUT THE ATTACH OR THE RESIZE CONTRACT CHANGES. `MetalCanvas` still forwards its own
/// bounds through `resizeSurface` exactly as it did for rotation; only the bounds it is given are
/// different. The renderer stays the authority on scaling: the aspect used here is the one the core
/// DECLARED, and a core that reports different geometry at runtime is still letterboxed correctly
/// inside this rect rather than overflowing it.
enum PictureFit {
    /// The largest rect of the given aspect ratio that fits `region`, centred in it.
    ///
    /// A degenerate region or a nonsensical aspect returns the region untouched, because a surface
    /// of nothing reads on a device as a black screen with no explanation, and the old behaviour is
    /// a better failure than that.
    static func rect(aspect: CGFloat, in region: CGRect) -> CGRect {
        guard region.width >= 1, region.height >= 1 else { return region }
        guard aspect.isFinite, aspect > 0 else { return region }

        let regionAspect = region.width / region.height
        var width = region.width
        var height = region.height
        if aspect > regionAspect {
            // Wider than the space: full width, and the height follows.
            height = (region.width / aspect).rounded(.down)
        } else {
            width = (region.height * aspect).rounded(.down)
        }
        // Whole points, and never larger than the region or smaller than a pixel. A fractional
        // drawable size would be rounded by Metal anyway, and rounding it here is what keeps the
        // centring symmetric.
        width = max(1, min(width.rounded(.down), region.width))
        height = max(1, min(height.rounded(.down), region.height))

        return CGRect(
            x: region.minX + ((region.width - width) / 2).rounded(),
            y: region.minY + ((region.height - height) / 2).rounded(),
            width: width,
            height: height
        )
    }
}

// MARK: - The player

/// A running game: the picture, the telemetry, the touch controls and the way back out.
struct PlayerScreen: View {
    @ObservedObject var host: EngineHost

    /// Watched so the rewind and fast-forward buttons redraw while held, and so the rewind
    /// button appears and disappears with the setting that enables it.
    @ObservedObject var emulation: EmulationSettings

    /// Watched because a controller connecting can take the on-screen pad off the screen, so this
    /// object decides whether the touch controls below are mounted at all.
    @ObservedObject var controllers: PhysicalControllers

    /// Observed rather than reached through the host, because the save button's menu LISTS this
    /// game's states: saving has to make the new slot appear in the menu without leaving the player,
    /// and deleting one from the detail sheet has to take it out of the menu.
    @ObservedObject var saveStates: SaveStates

    /// Which pad to draw. Nil when the launched file's extension has no system mapped, which the
    /// launch path should already have refused, so it is reported rather than silently ignored.
    let system: GameSystem?

    /// The 50-slot manager and the cheat sheet, opened from the menu.
    @State private var showingSlots = false
    @State private var showingCheats = false
    /// The online play sheet, opened from the save-state menu.
    @State private var showNetplay = false
    /// The feedback form, opened from the menu with this game attached.
    @State private var showFeedback = false
    /// Core settings, filters and the disc list, opened by the in-game actions.
    @ObservedObject private var coreActions = CoreActionsModel.shared

    /// The player's own choice to tuck the top bar away: the game's name and every button in the
    /// bar except Back and the button that brings them back.
    ///
    /// ASKED FOR FROM A SCREENSHOT. With a skin that puts the game in its own screen holes, the bar
    /// sits on top of the picture (a 3DS skin sideways was the case), and nothing could move it.
    /// Back stays because leaving the game must never be hidden, and the eye stays because a
    /// hidden bar with no way back would be a trap. Remembered across games and launches, since a
    /// player who wants a clean screen wants it every time.
    @AppStorage("continuum.player.topBarHidden.v1") private var topBarHidden = false

    var body: some View {
        ZStack(alignment: .top) {
            // Controls first, so the chrome's buttons sit above them in the z-order. They only
            // claim the touches that land on an actual control (see
            // `TouchControlsView.point(inside:with:)`), so the picture and the chrome stay live.
            //
            // Not mounted at all while a controller has taken over, rather than mounted and hidden.
            // A hidden overlay would still be laid out, would still claim the region it reserves
            // for itself, and `TouchControlsView` would still be the thing reporting the picture
            // area, so the game would keep the letterbox it needed for controls nobody can see.
            // Unmounting also drops any held button on the way out, through
            // `TouchControlsHost.dismantleUIView`, so a finger down at the moment a pad connects
            // cannot leave a press behind on the touch layer.
            if let system, !controllers.hidesOnScreenPadNow {
                // touchLayoutsVersion / touchSkinsVersion: refresh pad when Settings edits land.
                let _ = host.touchLayoutsVersion
                let _ = host.touchSkinsVersion
                TouchControlsHost(
                    system: system,
                    layout: host.touchLayout(for: system),
                    // The same number `RootView` positions the canvas with, so the pad can work out
                    // where the picture actually is and put the DS touch screen exactly on it.
                    pictureAspect: host.activePictureAspect,
                    input: host.padInput,
                    onDiagnostic: { line in host.noteControlLayout(line) },
                    onPictureArea: { rect in host.updatePictureArea(rect) },
                    skinArtwork: host.skinImage(for: system),
                    skinScreenNormalized: host.skinScreenOutput(for: system),
                    skinMapping: host.skinMapping(for: system),
                    landscapeArtwork: host.skinLandscapeImage(for: system),
                    landscapeScreen: host.skinLandscapeScreen(for: system),
                    landscapeMapping: host.skinLandscapeMapping(for: system),
                    landscapeLayout: host.skinLandscapeLayout(for: system),
                    portraitFace: host.skinPadFace(for: system, landscape: false),
                    landscapeFace: host.skinPadFace(for: system, landscape: true),
                    onSkinHoles: { screens in host.applySkinHoles(screens) },
                    // Where the touch screen is comes from the engine, which drew it: a layout,
                    // a swap or a TV can all move it.
                    touchMapper: host.touchMapper(for: system),
                    screenLayoutVersion: host.screenLayoutVersion,
                    trackpadEnabled: host.activeTrackpad
                )
                .ignoresSafeArea()
            }

            VStack(alignment: .leading, spacing: 6) {
                topBar
                // Outside the skin-hole condition on purpose: a stalled or desynced online game has
                // to say so even when the picture fills a skin hole.
                NetplayHUDLine(netplay: host.netplay)
                // The technical read-outs only when the (i) button asks for them: fps, frames and
                // the whole block. The strip is left out over a skin hole, where it would sit on
                // the picture.
                if host.showDiagnostics {
                    if !host.skinHolesActive {
                        telemetryStrip
                    }
                    DiagnosticsPanel(host: host, emulation: emulation, saveStates: saveStates)
                } else {
                    // Otherwise each new status line shows for a few seconds and fades, so what the
                    // app did (a save, a refusal, a restart) is still said without sitting on the
                    // game. Every line is also kept in the activity log a feedback report sends.
                    StatusFlash(text: host.status)
                }
                // Claims the rest of the height without claiming any touches, so everything below
                // reaches the controls.
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 12)
            .padding(.top, 6)

            // Unlock banners. Takes no touches; see `AchievementToastOverlay`.
            AchievementToastOverlay(store: host.achievements)

            // Hardware key capture, the on-screen keyboard and the Controllers sheet.
            // See InputExtras.swift.
            InputPlayerLayer(host: host, extras: host.inputExtras, system: system)
        }
        .sheet(isPresented: $showingSlots) {
            if let entry = host.activeEntry {
                SaveSlotsSheet(entry: entry, host: host, saveStates: saveStates,
                               onDone: { showingSlots = false })
            }
        }
        .sheet(isPresented: $showingCheats) {
            if let entry = host.activeEntry {
                PlayerCheatsSheet(entry: entry, host: host, cheats: host.cheats,
                                  onDone: { showingCheats = false })
            }
        }
        // The overlay is the thing that reports how much room the picture may have, so when it goes
        // away the last rect it reported is a lie: the picture would keep a letterbox reserved for
        // controls that are no longer there. Cleared rather than recomputed here, because nil
        // already means "the whole window" to `RootView`, and a re-mounted overlay publishes its own
        // rect on its first layout pass, so the way back needs nothing extra.
        .onChange(of: controllers.hidesOnScreenPadNow) { _ in
            host.pictureArea = nil
            host.applySkinHoles([])
        }
        .sheet(isPresented: $host.webPlayerSettingsOpen) {
            if let player = host.webPlayer {
                WebPlayerSettingsSheet(host: host, session: player)
            }
        }
        .sheet(item: $coreActions.sheet) { sheet in
            CoreActionSheet(host: host, sheet: sheet)
        }
        // Sheets the skin function buttons open (SkinFunctions.swift, SkinLibraryScreen.swift).
        .modifier(SkinFunctionSheets(host: host))
        .sheet(isPresented: $showNetplay) {
            NetplaySheet(netplay: host.netplay,
                         gameName: host.activeEntry?.name ?? "no game") {
                showNetplay = false
            }
        }
        .sheet(isPresented: $showFeedback) {
            FeedbackSheet(host: host, entry: host.activeEntry) {
                showFeedback = false
            }
        }
    }

    // MARK: Top bar

    private var topBar: some View {
        HStack(spacing: 10) {
            Button {
                host.leavePlayer()
            } label: {
                Image(systemName: "chevron.left")
                    .font(.system(size: 17, weight: .semibold))
                    .frame(width: 40, height: 40)
                    .background(Color.white.opacity(0.12), in: Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Back to the library")

            // Hides or shows everything to its right. See `topBarHidden`.
            sessionButton(topBarHidden ? "eye" : "eye.slash",
                          label: topBarHidden ? "Show the player buttons"
                                              : "Hide the player buttons") {
                topBarHidden.toggle()
            }

            if topBarHidden {
                Spacer(minLength: 0)
            } else {
                topBarItems
            }
        }
        .foregroundStyle(.white)
    }

    /// Everything in the top bar after Back and the hide button: the game's name and the session
    /// buttons. Its own builder so the eye button can take all of it away in one place.
    @ViewBuilder
    private var topBarItems: some View {
        Text(host.activeEntry?.name ?? "no game")
            .font(.system(.footnote, design: .monospaced))
            .lineLimit(1)
            .truncationMode(.middle)
            .foregroundStyle(Color.white.opacity(0.9))

        Spacer(minLength: 4)

        // Rewind, fast forward and reset are hidden during online play: on one phone and not
        // the other they would split the two games apart. The engine refuses them as well.
        if emulation.rewindEnabled && !host.netplayLive && host.webPlayer == nil {
            holdButton("backward.fill",
                       label: "Rewind",
                       active: emulation.isRewinding,
                       onPress: { emulation.beginRewind() },
                       onRelease: { emulation.endRewind() })
        }
        if !host.netplayLive && host.webPlayer == nil {
            holdButton("forward.fill",
                       label: "Fast forward",
                       active: emulation.isFastForwarding,
                       onPress: { emulation.beginFastForward() },
                       onRelease: { emulation.endFastForward() })
        }
        // Pause too: a paused phone stops sending input, which leaves the other one stalled.
        if !host.netplayLive {
            sessionButton(host.paused ? "play.fill" : "pause.fill",
                          label: host.paused ? "Resume" : "Pause") {
                host.togglePause()
            }
            sessionButton("arrow.clockwise", label: "Reset") {
                host.resetGame()
            }
        }
        // DS and 3DS only: which screen is the big one. One tap, and the touch screen
        // follows the picture because the engine maps touches through the same layout.
        if host.activeSystemHasTwoScreens {
            sessionButton("rectangle.2.swap", label: "Swap screens") {
                host.swapScreens()
            }
        }
        if host.webPlayer != nil {
            // Flash and J2ME: their settings (keys, phone, save file) instead of save states,
            // which these players cannot make.
            sessionButton("slider.horizontal.3", label: "Player settings") {
                host.showWebPlayerSettings()
            }
        } else {
            saveStateControl
        }
        sessionButton(host.showDiagnostics ? "info.circle.fill" : "info.circle",
                      label: "Diagnostics") {
            host.showDiagnostics.toggle()
        }
    }

    /// Everything durable this screen can do to a game: save, load, and take the cover from it.
    ///
    /// A PLAIN TAP OPENS THIS, and it used to take a press. That was `Menu(content:label:
    /// primaryAction:)`, where the tap saved to a new slot and only a long press revealed the menu,
    /// which read well on paper and failed in the one way that matters: the cover capture was
    /// reported as having "no button to even try". A control whose only affordance is a gesture
    /// nobody was told about is an absent control, and the press was not discoverable on a screen
    /// with seven other things that respond to a tap. So the save moved into the menu with the rest,
    /// and the glyph changed from the save arrow to an ellipsis, because a button that now opens a
    /// list of three things should not go on claiming to be the save button.
    ///
    /// The cost is honest and small: saving is two taps instead of one. Worth it for load and
    /// capture becoming findable, and a save state is not something anyone needs to reach for in a
    /// fraction of a second.
    ///
    /// ONE CONTROL RATHER THAN THREE, which is the bar it sits in rather than a preference. The row
    /// already holds back, a title, rewind, fast-forward, pause, reset and diagnostics, and on a
    /// phone in portrait there is no room for more circles without shrinking all of them, which is
    /// the opposite of what a touch target needs. Menu rows cost no width at all and the system
    /// sizes each one for a finger.
    ///
    /// 44 POINTS, not the 38 the other session buttons use, because this is the one that opens
    /// something rather than doing something, and a menu that fails to open reads as a dead button.
    private var saveStateControl: some View {
        // Isolated in its own Equatable view. PlayerScreen observes the whole EngineHost, which
        // publishes frameCount and displayFps on every telemetry tick, so an inline Menu had its
        // content rebuilt (engine queries included) every frame while it was open, which froze
        // the open menu, worst in landscape. The menu now re-evaluates only when something it
        // shows changes: the game, online play, the pause state, the status line (every action
        // in it reports there) or the save list.
        PlayerActionsMenu(host: host,
                          saveStates: saveStates,
                          system: system,
                          entryID: host.activeEntry?.id,
                          netplayLive: host.netplayLive,
                          paused: host.paused,
                          status: host.status,
                          showingSlots: $showingSlots,
                          showingCheats: $showingCheats,
                          showNetplay: $showNetplay,
                          showFeedback: $showFeedback)
            .equatable()
    }

    private func sessionButton(_ symbol: String, label: String,
                               action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(.system(size: 15, weight: .semibold))
                .frame(width: 38, height: 38)
                .background(Color.white.opacity(0.12), in: Circle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
    }

    /// A button that acts while it is held rather than when it is tapped.
    ///
    /// Built on `DragGesture(minimumDistance: 0)` rather than on `Button`, because a `Button`
    /// only reports the completed tap and gives no press and release either side of it, and
    /// `onLongPressGesture` waits out a recognition delay before firing. Fast-forward and
    /// rewind have to start the instant the finger lands and stop the instant it lifts, and a
    /// half-second delay on a rewind button feels like the button is broken.
    ///
    /// A drag that wanders off the button still ends the hold, because `onEnded` fires wherever
    /// the finger lifts. That is the behaviour to want: the alternative, a hold that survives
    /// the finger sliding away, is how you end up stuck at 4x.
    private func holdButton(_ symbol: String, label: String, active: Bool,
                            onPress: @escaping () -> Void,
                            onRelease: @escaping () -> Void) -> some View {
        Image(systemName: symbol)
            .font(.system(size: 15, weight: .semibold))
            .frame(width: 38, height: 38)
            .background(active ? ShellPalette.accent.opacity(0.85) : Color.white.opacity(0.12),
                        in: Circle())
            .contentShape(Circle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { _ in
                        // `onChanged` fires repeatedly for one press, so this must be
                        // idempotent. Both `begin` calls guard on their own state.
                        onPress()
                    }
                    .onEnded { _ in onRelease() }
            )
            .accessibilityLabel(label)
            .accessibilityAddTraits(active ? [.isSelected] : [])
    }

    // MARK: Telemetry

    /// The thin strip from the design reference: fps, frames, audio, core.
    ///
    /// Two figures from that screenshot are deliberately not here, because inventing them would be
    /// worse than leaving them out:
    ///
    ///   - core memory. There is no memory query anywhere in the engine, so the core id takes that
    ///     slot instead, which is at least load-bearing: it is how a routing mistake is spotted.
    ///
    /// The audio figure was the other one, and it is real now: `drain_audio` is exported, the
    /// AVAudioEngine graph plays what it returns, and the strip reports both ring depths and the
    /// device's own underrun count rather than the depth of a ring nobody was draining.
    private var telemetryStrip: some View {
        Text(host.telemetryLine)
            .font(.system(.caption2, design: .monospaced))
            .foregroundStyle(Color.white.opacity(0.75))
            .lineLimit(1)
            .minimumScaleFactor(0.7)
            // See `DiagnosticsPanel`: a read-out must never eat a touch, and this one spans the
            // full width directly above the picture.
            .allowsHitTesting(false)
    }

    /// Always on screen, never behind the toggle.
    ///
    /// On a sideloaded build with no debugger this line is the only thing that explains a failure,
    /// so the player screen keeps it even when the full panel is hidden.
    private var statusLine: some View {
        Text(host.status)
            .font(.system(.caption2, design: .monospaced))
            .foregroundStyle(Color.white.opacity(0.6))
            .lineLimit(2)
            .fixedSize(horizontal: false, vertical: true)
            // Two lines of it, full width, and it is always on screen rather than behind the
            // toggle. See `DiagnosticsPanel`.
            .allowsHitTesting(false)
    }
}

// MARK: - The in-game actions menu

/// The ellipsis menu on the player's top bar. See `PlayerScreen.saveStateControl` for why this
/// is its own Equatable view holding the host as a plain reference rather than observing it.
struct PlayerActionsMenu: View, Equatable {
    let host: EngineHost
    @ObservedObject var saveStates: SaveStates
    let system: GameSystem?
    let entryID: LibraryEntry.ID?
    let netplayLive: Bool
    let paused: Bool
    let status: String
    let showingSlots: Binding<Bool>
    let showingCheats: Binding<Bool>
    let showNetplay: Binding<Bool>
    let showFeedback: Binding<Bool>

    // saveStates is left out: it is observed above, so a change to the list redraws the menu on
    // its own. Only `let` values of Sendable types are read, so this can be nonisolated as
    // Equatable requires.
    nonisolated static func == (lhs: PlayerActionsMenu, rhs: PlayerActionsMenu) -> Bool {
        lhs.host === rhs.host
            && lhs.system == rhs.system
            && lhs.entryID == rhs.entryID
            && lhs.netplayLive == rhs.netplayLive
            && lhs.paused == rhs.paused
            && lhs.status == rhs.status
    }

    var body: some View {
        Menu {
            Button {
                host.saveStateToSlot()
            } label: {
                Label("Save to the next free slot", systemImage: "square.and.arrow.down")
            }
            Button {
                showingSlots.wrappedValue = true
            } label: {
                Label("Save slots...", systemImage: "square.grid.2x2")
            }
            Button {
                showingCheats.wrappedValue = true
            } label: {
                Label("Cheats and RAM search...", systemImage: "wand.and.stars")
            }
            // Switching skin mid-game, the same sheet the `skins` skin button opens.
            Button {
                SkinRuntime.shared.sheet = .skins
            } label: {
                Label("Skin...", systemImage: "paintpalette")
            }
            Button {
                SkinRuntime.shared.sheet = .functions
            } label: {
                Label("All functions...", systemImage: "square.grid.3x3")
            }

            let states = host.activeEntry.map { saveStates.states(for: $0) } ?? []
            if netplayLive {
                Button("Loading a state is off during online play") {}
                    .disabled(true)
            } else if states.isEmpty {
                // Disabled and explicit rather than absent. An empty menu reads as a broken
                // control, and "there is nothing to load yet" is the answer to the question the
                // user just asked by opening it.
                Button("No saved states for this game yet") {}
                    .disabled(true)
            } else {
                // Newest first, which is the order the store keeps. Capped, because this is a menu
                // over a running game and not the management screen: the detail sheet lists every
                // state with its size and frame, and a menu long enough to scroll would be a worse
                // version of that list. The cap is a hard limit rather than a page, so the wording
                // below has to stay honest about it.
                Section("Load a state") {
                    ForEach(states.prefix(Self.menuStateLimit)) { record in
                        Button {
                            saveStates.load(record)
                        } label: {
                            Label(record.summaryLine, systemImage: record.isAuto
                                  ? "clock.arrow.circlepath"
                                  : "tray.and.arrow.up")
                        }
                    }
                }
            }

            // Its own section, because it is not a save state and a row loose among them would read
            // as one. Offered only with a game on screen: the player is never mounted without one,
            // so this is the optional being unwrapped rather than a condition anybody can hit, and
            // `captureCover` guards the session itself for the paths that can.
            //
            // THE ANSWER ARRIVES ON THE STATUS LINE under the telemetry strip, which this screen
            // already draws, so there is no alert and nothing to dismiss. Capturing while PAUSED
            // works and is the better way to use it: the engine refreshes from the core before it
            // reads the surface, so a game paused on its title screen gives up exactly that frame.
            // Core settings, filters, discs, palettes, speeds, rotation and the TV.
            CoreActionsMenuSection(host: host, system: system)

            Section("Controls") {
                Button {
                    host.showControllers()
                } label: {
                    Label("Controllers and button mapping...", systemImage: "gamecontroller")
                }
                Button {
                    host.toggleOnScreenKeyboard()
                } label: {
                    Label("Keyboard", systemImage: "keyboard")
                }
                if system == .ps1 {
                    Button {
                        host.toggleAnalogMode()
                    } label: {
                        Label(host.isAnalogMode() ? "Analog pad: on" : "Analog pad: off",
                              systemImage: "l.joystick")
                    }
                }
                if system == .ds {
                    Button {
                        host.toggleDSLid()
                    } label: {
                        Label("Close or open the lid", systemImage: "laptopcomputer")
                    }
                }
                if system == .n3ds {
                    Button {
                        host.pressHomeButton()
                    } label: {
                        Label("HOME button", systemImage: "house")
                    }
                }
                Button {
                    host.shake()
                } label: {
                    Label("Shake", systemImage: "iphone.radiowaves.left.and.right")
                }
            }
            Section("Online play") {
                Button {
                    showNetplay.wrappedValue = true
                } label: {
                    Label(netplayLive ? "Online play: status and leave"
                                           : "Play online with a second phone",
                          systemImage: "person.2")
                }
            }

            if let entry = host.activeEntry {
                Section("Cover art") {
                    Button {
                        host.artwork.captureCover(for: entry)
                    } label: {
                        Label("Use this frame as the cover", systemImage: "camera.viewfinder")
                    }
                }
            }

            // The Amiibo picker, for the 3DS only: the only system here with an NFC reader. The
            // answer to a tap arrives on the status line, like the cover capture above.
            // Grouped with the feedback row so this menu has no more top-level children than it did
            // before that row: a view builder's child limit is not something to find out on CI.
            Group {
                if system == .n3ds {
                    AmiiboMenuSection(peripherals: host.peripherals,
                                      report: { host.status = $0 })
                }

                // Last, and from inside a game on purpose: that is where a tester notices
                // something, and the form attaches this game, its system and a picture of it.
                Section("Feedback") {
                    Button {
                        showFeedback.wrappedValue = true
                    } label: {
                        Label("Send feedback about this game...", systemImage: "envelope")
                    }
                }
            }
        } label: {
            // `ellipsis` rather than the save glyph, because this control no longer does one thing.
            Image(systemName: "ellipsis.circle")
                .font(.system(size: 17, weight: .semibold))
                .frame(width: 44, height: 44)
                .background(Color.white.opacity(0.12), in: Circle())
                .contentShape(Circle())
        }
        // One literal rather than a concatenation, like every other label in this file: the
        // concatenated form resolves to a different overload of this modifier than a plain string
        // does, and on a build whose only compiler is CI that is not a thing to find out remotely.
        .accessibilityLabel("Save states and cover art")
    }

    /// How many states the in-game menu offers. Six is about what fits without scrolling on the
    /// shortest screen this app supports, and the detail sheet is the place that shows them all.
    private static let menuStateLimit = 6
}

// MARK: - The diagnostics, in one place both screens use

/// The full diagnostic block: every line the HUD has ever carried.
///
/// NOTHING HERE MAY BE DELETED, AND THAT IS NOT CAUTION. On a sideloaded build there is no
/// debugger, no console and no crash log, so each of these lines is the only way to tell one
/// failure from another:
///
///   - no GPU line          -> attachMetal failed; the reason is in `status`
///   - GPU but 0 frames     -> renderer up, core or gate not producing
///   - frames but no colour -> compositor or pixel-format problem
///   - frozen counter       -> the frame gate is not releasing
///   - a core line naming a missing dylib -> that system's core never reached the bundle
///   - a running line naming the wrong core -> the extension route is wrong
///   - a control layout line -> two controls were laid out on top of each other
///   - an artwork line naming a network failure -> the covers are missing because of the network,
///     not because the games have none, which are two conditions that look identical on a shelf
///
/// It moved out of the always-on HUD into a toggle, and moved nowhere else: the same strings, in
/// the same order, reachable from the library and from the player.
struct DiagnosticsPanel: View {
    @ObservedObject var host: EngineHost

    /// Optional so the panel can still be shown from a context that has no settings object to
    /// hand. When absent the emulation line is simply not drawn, rather than drawn empty.
    var emulation: EmulationSettings?

    /// Optional for the same reason, and defaulted so that every existing caller still compiles.
    /// The line it draws answers "did the auto-save happen", which on a sideloaded build with no
    /// console is otherwise unanswerable.
    var saveStates: SaveStates? = nil

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            // `buildLine` already starts with "Continuum 0.8.0 (N)"; prefixing it again printed
            // the name twice.
            Text(host.buildLine)
                .font(.system(.caption2, design: .monospaced)).bold()
            Text(host.status)
                .fixedSize(horizontal: false, vertical: true)
            if !host.cores.isEmpty {
                Text(host.cores)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if !host.bios.isEmpty {
                Text(host.bios)
            }
            if !host.gpu.isEmpty {
                Text(host.gpu)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Text(host.frameLine)
            // Wrapped, because the audio line now carries the real rate, both ring depths, both
            // underrun counters and the status sentence behind them, and a truncated diagnostic
            // is not a diagnostic.
            Text(host.audioLine)
                .fixedSize(horizontal: false, vertical: true)
            // Wrapped, because this line now carries controller names as well as the on-screen
            // pad's state, and a pad whose name is cut off is exactly the information the line was
            // read for.
            Text(host.inputLine)
                .fixedSize(horizontal: false, vertical: true)
            // Fit, filter, volume and the rewind depth, plus a marker while either held control
            // is active. Worth a line because all five are now things a user can change, so
            // "why does this game look/sound like that" became a question the HUD should answer.
            if let emulation {
                Text(emulation.diagnosticLine)
                    .fixedSize(horizontal: false, vertical: true)
            }
            // GROUPED, and not for tidiness: a view builder takes at most ten children, this block
            // had reached twelve, and the failure names the whole block rather than the line that
            // broke it. Nothing was removed to make room, which the note at the top of this type
            // forbids; the last four lines simply share one child. See `SettingsScreen.body`, which
            // is split the same way and for the same reason.
            Group {
                // Inside the Group so the VStack stays at ten children. Skipped when status
                // already is the "Last N64: …" line (home strip after reopen).
                if !host.lastN64Crumb.isEmpty && !host.status.hasPrefix("Last N64:") {
                    Text("Last N64: \(host.lastN64Crumb)")
                        .fixedSize(horizontal: false, vertical: true)
                }
                if !host.controlNote.isEmpty {
                    Text(host.controlNote)
                        .fixedSize(horizontal: false, vertical: true)
                }
                if !host.libraryStatus.isEmpty {
                    Text(host.libraryStatus)
                        .fixedSize(horizontal: false, vertical: true)
                }
                if !host.artworkLine.isEmpty {
                    Text(host.artworkLine)
                        .fixedSize(horizontal: false, vertical: true)
                }
                // How many states exist, whether resuming is on, and whether the running game has
                // an auto-save to resume FROM. That last one is the line worth having: a game that
                // started from the beginning did so either because resuming is off, because there
                // was nothing stored, or because the gate refused it, and those are three different
                // bugs.
                if let saveStates {
                    Text(saveStates.diagnosticLine)
                        .fixedSize(horizontal: false, vertical: true)
                }
                // Whether this app can write instructions and run them, asked once at startup.
                // Here rather than in Settings because it is a fact about the installed build, and
                // the diagnostics panel is where facts about the build already live. It decides
                // whether N64 is possible at all: an N64 interpreter is far too slow to be
                // playable, so N64 needs a recompiler, and a recompiler needs this to say working.
                if !host.jitLine.isEmpty {
                    Text(host.jitLine)
                        .fixedSize(horizontal: false, vertical: true)
                }
                // Whether MoltenVK is in the process. The gate on every system that renders through
                // a GPU rather than on the CPU, which is Dreamcast, PSP, the 3DS and a playable
                // N64. Here rather than in Settings for the same reason the JIT line is: it is a
                // fact about the installed build, and this panel is where those live.
                if !host.vulkanLine.isEmpty {
                    Text(host.vulkanLine)
                        .fixedSize(horizontal: false, vertical: true)
                }
                // The TV: connected, disconnected, or why the picture could not move to it.
                if !host.displayLine.isEmpty {
                    Text(host.displayLine)
                        .fixedSize(horizontal: false, vertical: true)
                }
                // Whether the running core asked for the microphone or the camera, and what iOS
                // said when they were started. "The game cannot hear me" has five different
                // causes, and this line names which one it is.
                Text(host.peripherals.statusLine)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .font(.system(.caption2, design: .monospaced))
        .foregroundStyle(.white)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(10)
        .background(.black.opacity(0.62), in: RoundedRectangle(cornerRadius: 8))
        // A READ-OUT MUST NEVER EAT A TOUCH. This panel is full width, has a filled background,
        // grows to fourteen-odd lines, and sits above the control overlay in the player's z-order,
        // so wherever it overhangs it was absorbing touches with no feedback of any kind. On the DS
        // that is a dead band across the top of the touch screen; in landscape, where the panel is
        // full width and the controls sit high, it can reach a shoulder button too. There is
        // nothing interactive in here to lose: every child is a `Text`.
        .allowsHitTesting(false)
    }
}

// MARK: - The status line, briefly, over a skin

/// The status line for a skin that puts the game in its own screen holes, where the full line
/// would sit on the picture for good: each new line shows for a few seconds and then fades.
///
/// Shown again whenever the text changes and when the player first appears (a restart lands here
/// with its "restarted from the beginning" line already set), and takes no touches, like every
/// read-out on this screen.
struct StatusFlash: View {
    let text: String

    @State private var visible = false
    /// Which showing the pending fade belongs to, so an older timer cannot hide a newer line.
    @State private var showing = 0

    /// Long enough to read two lines of small text, short enough not to sit on the game. A problem
    /// stays twice as long, because it is the line a tester needs to read or screenshot.
    private var seconds: Double {
        StatusTint.tint(for: text) == ShellPalette.metadata ? 5 : 10
    }

    var body: some View {
        Text(text)
            .font(.system(.caption2, design: .monospaced))
            .foregroundStyle(Color.white.opacity(0.9))
            .lineLimit(3)
            .fixedSize(horizontal: false, vertical: true)
            .padding(.horizontal, 8)
            .padding(.vertical, 5)
            .background(Color.black.opacity(0.72), in: RoundedRectangle(cornerRadius: 7))
            .opacity(visible && !text.isEmpty ? 1 : 0)
            .animation(.easeOut(duration: 0.35), value: visible)
            .allowsHitTesting(false)
            .onAppear { show() }
            .onChange(of: text) { _ in show() }
    }

    private func show() {
        showing += 1
        let mine = showing
        visible = true
        DispatchQueue.main.asyncAfter(deadline: .now() + seconds) {
            if showing == mine {
                visible = false
            }
        }
    }
}
