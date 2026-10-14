// Continuum - Settings: the artwork controls, the on-screen control editor, the library layout
// controls, and the permanent home of the diagnostics.
//
// THE RULE THIS FILE IS BUILT AROUND: every control in here does something today. A switch that
// does nothing is worse than an absent one, because it teaches the user that the app lies. So the
// things that are not wired are not here as dead controls; they are listed at the bottom as a
// read-only note saying what each one needs, which is information rather than a promise.
//
// The selectors the design reference always showed for fit, filter, speed and volume are real now:
// the engine exports scale mode, filter, the pacer's multiplier, mute and a ramped volume, and
// rewind has a memory budget behind it. `EmulationSettings` owns all five, persists them and
// re-asserts them into an engine whose pacer does not survive a session.
//
// THAT LIST AT THE BOTTOM IS NOW EMPTY, and the section draws nothing rather than drawing a heading
// over an empty list. Physical controllers came off it when the engine gained a per-source input
// push, and cover art taken from the game came off it when the engine gained a framebuffer readback,
// which was the last entry. The list and its section are still here, because the next thing the
// engine cannot do yet belongs in exactly that place. See `notYetWiredSection`.
//
// Nothing was deleted from the diagnostic HUD in moving it here. The same strings, in the same
// order, are still one tap away from the status strip on both screens.

import SwiftUI
// For UISelectionFeedbackGenerator in SegmentedChoice. Imported explicitly rather than relying
// on SwiftUI to re-export UIKit, which it is not documented to do.
import UIKit

struct SettingsScreen: View {
    @ObservedObject var host: EngineHost
    @ObservedObject var artwork: ArtworkStore
    @ObservedObject var emulation: EmulationSettings

    /// Observed rather than reached through the host, so that plugging a controller in while this
    /// screen is open updates the read-out in front of you. That is not a nicety: the read-out is
    /// how a player finds out whether the app can see their pad at all, and a stale one would
    /// answer the question wrongly.
    @ObservedObject var controllers: PhysicalControllers

    /// Observed so that the storage read-out is right the moment it is looked at. It is a count and
    /// a size, and both change from the player screen, which is the screen the user was on before
    /// this one.
    @ObservedObject var saveStates: SaveStates

    /// Observed for the same reason: the cheat read-out counts what is stored across every game.
    @ObservedObject var cheats: CheatStore

    /// Two-screen layouts, the TV and trackpad mode.
    @ObservedObject var screenModes: ScreenModes

    /// Empty space at the end of the page, inside the scroll view, for the library's floating tab
    /// bar: the page scrolls under the bar and its last row still comes up clear of it.
    var bottomClearance: CGFloat = 0

    /// Whether the control layout editor is up.
    ///
    /// Presented full screen rather than as a sheet, and that is the one thing about it that is not
    /// a matter of taste: the editor's whole job is to show the controls where they will really be,
    /// and a sheet is inset from the bottom of the screen, which is exactly the edge both thumb
    /// clusters live at. A card cannot show you where a control sits relative to a screen it does
    /// not cover.
    @State private var showControlEditor = false
    @State private var confirmResetControls = false
    @State private var confirmDeleteStates = false
    @State private var confirmDeleteCheats = false
    @State private var confirmDeleteGames = false
    /// Button taps, rumble and the turbo rate. A shared object rather than a parameter, because
    /// the pad deep under the player reads the same one. See Haptics.swift.
    @ObservedObject private var feel = ControlFeel.shared

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                HStack(alignment: .firstTextBaseline) {
                    Text("Settings")
                        .font(.system(size: 26, weight: .heavy))
                        .foregroundStyle(.white)
                    Spacer(minLength: 8)
                    Button("Open all") { SettingsFolds.shared.openAll() }
                        .buttonStyle(SettingsFoldButtonStyle())
                    Button("Close all") { SettingsFolds.shared.closeAll() }
                        .buttonStyle(SettingsFoldButtonStyle())
                }
                .padding(.horizontal, 16)

                // First, so a tester finds it without scrolling. The second way in is a game's
                // ⋯ menu. See Feedback.swift.
                FeedbackSettingsSection(host: host)

                // Second from the top, and folds like the rest (closed on a fresh install). It was
                // always open and second from the top: on build 170 the core pickers sat in two
                // folding sections far down the screen (one for the PlayStation, and one for the
                // other systems with a choice, after Online Play), and once folded, or after
                // "Close all", Brett could not find any core switch at all.
                coreSwitchSection

                // Grouped, and NOT for tidiness: a SwiftUI view builder accepts at most ten
                // children, and this screen has more sections than that. Exceeding it fails with
                // an error that points at the whole block and names no section, which on a build
                // whose only compiler is CI is an expensive thing to go looking for. Two groups
                // keep every block well under the limit and leave room for the next section.
                //
                // The split is along a real seam rather than at the tenth item: these four are how
                // the game itself behaves, and the rest are the app around it.
                Group {
                    pictureSection
                    soundSection
                    speedSection
                    controllerSection
                    controlsSection
                    feelSection
                    // In this group rather than the second one, along the seam the note above
                    // describes: a cheat changes how the game itself behaves.
                    cheatsSection
                    AchievementsSettingsSection(store: host.achievements)
                    // How the game is shown and touched: the DS / 3DS screens, the TV, the mouse.
                    ScreenModesSettingsSection(modes: screenModes)
                    // The microphone, camera and Amiibo card. Its own view so it can observe
                    // `Peripherals` directly; see Peripherals.swift.
                    PeripheralsSettingsSection(peripherals: host.peripherals,
                                               report: { host.status = $0 })
                }
                Group {
                    artworkSection
                    layoutSection
                    biosSection
                    // Every core's own settings, and the TV's fit and layout. See CoreSettingsScreen.swift.
                    CoreSettingsSettingsSection(host: host)
                    // Beside STORAGE on purpose: both are about what is kept on the phone.
                    saveStatesSection
                    storageSection
                    notYetWiredSection
                }
                // A third group for the same ten-children reason as the two above.
                Group {
                    AppIconSettingsSection()
                    SkinLibrarySettingsSection(host: host)
                    CloudSyncSection(sync: host.cloudSync)
                    OnlinePlaySettingsSection(netplay: host.netplay)
                    // Controller types, button mapping profiles, motion. See InputExtras.swift.
                    InputSettingsSection(host: host, extras: host.inputExtras)
                    // Last: technical details are for reporting a problem, not for setting up.
                    diagnosticsSection
                    aboutSection
                }
            }
            .padding(.bottom, 24 + bottomClearance)
        }
        .onAppear {
            // Read on appearance rather than continuously: this is a sentence about the rewind
            // tape, not telemetry, and polling the engine for it every frame would take the
            // engine lock sixty times a second to redraw text that barely changes.
            emulation.refreshRewindReadout()
            host.refreshJit()
        }
        .fullScreenCover(isPresented: $showControlEditor) {
            TouchLayoutEditor(
                layoutFor: { system in host.touchLayout(for: system) },
                // Per-system commit: editor preview console is the key. Closure keeps drag
                // settle from republishing the whole library shell; see TouchLayoutEditor.
                onCommit: { system, layout in host.setTouchLayout(layout, for: system) },
                onSkinImported: { system, result in host.applyImportedSkin(result, for: system) },
                skinImageFor: { system in host.skinImage(for: system) },
                skinScreenFor: { system in host.skinScreenOutput(for: system) },
                skinMappingFor: { system in host.skinMapping(for: system) },
                landscapeImageFor: { system in host.skinLandscapeImage(for: system) },
                landscapeScreenFor: { system in host.skinLandscapeScreen(for: system) },
                landscapeMappingFor: { system in host.skinLandscapeMapping(for: system) },
                landscapeLayoutFor: { system in host.skinLandscapeLayout(for: system) },
                onLandscapeLayout: { system, layout in host.setLandscapeSkinLayout(layout, for: system) },
                onClearSkin: { system in host.clearSkin(for: system) },
                skinEditAccess: host.skinEditAccess,
                onClose: { showControlEditor = false }
            )
        }
    }

    // MARK: Artwork

    private var artworkSection: some View {
        SettingsSection(title: "COVER ART") {
            Toggle(isOn: $artwork.fetchEnabled) {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Look up cover art")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(.white)
                    Text(artwork.fetchEnabled
                         ? "On. Your games get real box art."
                         : "Off. Games show a title card instead.")
                        .font(.system(size: 12))
                        .foregroundStyle(ShellPalette.secondaryText)
                }
            }
            .tint(ShellPalette.accent)

            // The disclosure, in plain words. Resolving art sends a filename to a third party, and
            // that is a real disclosure rather than an implementation detail.
            SettingsNote(
                "Covers come from libretro's free cover server, looked up by each game's file "
                + "name. Nothing else is sent."
            )

            // The STORED, COVER LISTS and LAST read-outs are in TECHNICAL DETAILS
            // (`moreTechnicalReadouts`).
            SettingsNote(coverCountLine)

            HStack(spacing: 10) {
                SettingsButton(title: "Clear artwork cache", role: .destructive) {
                    artwork.clearStoredCovers()
                }
                SettingsButton(title: "Retry failed lookups", role: .normal) {
                    artwork.retryFailedLookups()
                }
            }

            SettingsButton(title: "Forget the downloaded cover lists", role: .destructive) {
                artwork.forgetCoverLists()
            }

            SettingsNote("Both clear buttons just free up space. Anything still needed downloads again.")
        }
    }

    /// The one plain line COVER ART keeps, with real plurals. The full read-outs are in
    /// TECHNICAL DETAILS.
    private var coverCountLine: String {
        let count = artwork.storedFiles
        guard count > 0 else { return "No covers saved yet" }
        return "\(count) \(count == 1 ? "cover" : "covers") saved "
            + "(\(ArtworkStore.byteText(artwork.storedBytes)))"
    }

    // MARK: Physical controllers

    /// What is attached, what its buttons do, and whether it takes the screen back.
    ///
    /// A READ-OUT AND ONE SWITCH, and that shape is the honest one for this feature. There is
    /// nothing to configure about a controller that the app should be asking: the mapping is the
    /// W3C standard layout, which is the wire format the engine documents, and iOS owns the
    /// pairing. What a player actually needs from a settings screen here is an answer to "does it
    /// see my controller", which is the read-out, and a decision about the overlay, which is the
    /// switch. Anything else would be a control invented to fill the section.
    /// What the auto-hide switch is doing at this moment, which is not always what it means.
    ///
    /// Three states rather than two. The switch can only act while a controller the app can read is
    /// attached, so "on with nothing attached" is a real and confusing condition: the switch is on,
    /// the pad is still there, and nothing has gone wrong. It was reported as the setting not
    /// working, which is the correct reading of a control that appears to do nothing, so the
    /// control now says which of the three it is in.
    private var autoHideExplanation: String {
        guard controllers.autoHidesOnScreenPad else {
            return "Off. The on-screen pad stays put."
        }
        // Named before the other two, because a switch that is on, with a controller attached, and
        // a pad still on screen is the most confusing state of all, and it is CORRECT.
        if controllers.runningSystemNeedsOverlay {
            return "On, but the pad stays up for this game's touch screen."
        }
        if controllers.playablePads > 0 {
            // The pad with no View or Share button keeps the overlay up (see
            // `portZeroPadIsComplete`). Said here, because the sentence that explained it moved
            // to TECHNICAL DETAILS with the read-out.
            return controllers.hidesOnScreenPadNow
                ? "On. The pad is hidden while your controller is connected."
                : "On, but your controller has no View or Share button, so the pad stays for "
                    + "SELECT."
        }
        return "On. The pad hides once a controller connects."
    }

    /// The one plain line GAME CONTROLLERS keeps. The full read-out is in TECHNICAL DETAILS.
    private var controllerCountLine: String {
        switch controllers.playablePads {
        case 0: return "No controller connected"
        case 1: return "1 controller connected"
        default: return "\(controllers.playablePads) controllers connected"
        }
    }

    private var controllerSection: some View {
        SettingsSection(title: "GAME CONTROLLERS") {
            SettingsNote(controllerCountLine)

            Toggle(isOn: $controllers.autoHidesOnScreenPad) {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Hide the on-screen pad while a controller is connected")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(.white)
                    // Says what it is doing RIGHT NOW rather than only what it means, because this
                    // switch cannot act without a controller attached and that was reported as it
                    // not working. Nothing was wrong, but a switch that is on and visibly changing
                    // nothing has to account for itself rather than leave the user guessing.
                    Text(autoHideExplanation)
                        .font(.system(size: 12))
                        .foregroundStyle(ShellPalette.secondaryText)
                }
            }
            .tint(ShellPalette.accent)

            SettingsNote(
                "Your controller and the on-screen pad both work, and up to four controllers can "
                + "play at once. Pair one in your iPhone's Bluetooth settings."
            )
        }
    }

    // MARK: The on-screen controls

    /// The game pad's own layout, which is a different thing from the LAYOUT section below it.
    ///
    /// Two sections rather than one, because the two settings share nothing but a word: this one is
    /// where your thumbs go during a game, and that one is how the library arranges its cards. They
    /// were confusing enough while only one of them existed.
    private var controlsSection: some View {
        SettingsSection(title: "ON-SCREEN CONTROLS") {
            SettingsButton(title: "Move the on-screen controls", role: .normal) {
                showControlEditor = true
            }

            SettingsNote(
                "Drag the buttons where you want them, or add extras like turbo and quick save. "
                + "Each console keeps its own layout."
            )

            // Reachable from here as well as inside the editor, on the same reasoning as the
            // library's layout toggle: the editor is the place you go to fiddle, and putting the
            // way back to the shipped arrangement behind a screenful of fiddling is the wrong way
            // round when what you want is to undo it.
            SettingsButton(title: "Reset every system's controls and skins",
                           role: .destructive) {
                confirmResetControls = true
            }
            .alert("Reset every system's controls and skins?", isPresented: $confirmResetControls) {
                Button("Reset and delete skins", role: .destructive) {
                    host.resetAllTouchLayouts()
                }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("Every moved button goes back to where it started, and every imported "
                     + "skin is deleted. This cannot be undone.")
            }
            .disabled(!host.hasCustomControlsOrSkins)
            .opacity(host.hasCustomControlsOrSkins ? 1 : 0.45)
        }
    }

    // MARK: Feel

    /// How the controls answer a thumb: a tap per press, a game's rumble, and the turbo rate.
    private var feelSection: some View {
        SettingsSection(title: "HAPTICS AND TURBO") {
            VStack(alignment: .leading, spacing: 8) {
                Text("Tap on a button press")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(.white)
                SegmentedChoice(options: ButtonHapticStrength.allCases,
                                title: { $0.label },
                                selection: $feel.buttonHaptics)
            }

            Toggle(isOn: $feel.rumbleEnabled) {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Rumble")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(.white)
                    Text(feel.rumbleEnabled
                         ? "On. Games that rumble shake your phone or controller."
                         : "Off. No rumble.")
                        .font(.system(size: 12))
                        .foregroundStyle(ShellPalette.secondaryText)
                }
            }
            .tint(ShellPalette.accent)

            VStack(alignment: .leading, spacing: 8) {
                Text("Turbo speed")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(.white)
                SegmentedChoice(options: TurboRate.allCases,
                                title: { $0.label },
                                selection: $feel.turboRate)
            }

            SettingsNote(
                "Turbo speed is for the turbo buttons you add in Move the on-screen controls."
            )
        }
    }

    // MARK: Layout

    private var layoutSection: some View {
        SettingsSection(title: "LAYOUT") {
            VStack(alignment: .leading, spacing: 8) {
                Text("All Games")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(.white)
                // Converted along with the others. It had the same fault and only escaped the
                // report because it is one row further down the page than the ones being tested.
                SegmentedChoice(options: LibraryLayout.allCases,
                                title: { $0.title },
                                selection: $host.libraryLayout)
            }

            Toggle(isOn: $host.showsSystemShelves) {
                VStack(alignment: .leading, spacing: 3) {
                    Text("A shelf per system on Home")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(.white)
                    Text("Off leaves Home as the featured game and Recently added.")
                        .font(.system(size: 12))
                        .foregroundStyle(ShellPalette.secondaryText)
                }
            }
            .tint(ShellPalette.accent)
        }
    }

    // MARK: Diagnostics

    /// The version a tester quotes, and the plain facts about what the app is.
    private var aboutSection: some View {
        SettingsSection(title: "ABOUT") {
            SettingsReadout(label: "Version", value: EngineHost.versionLabel)
            SettingsNote(
                "Continuum is still in beta, so expect some rough edges. Games and BIOS files "
                + "aren't included, you'll need your own. If something breaks, Send feedback at "
                + "the top of Settings goes straight to me."
            )
            OpenSourceCreditsList()
        }
    }

    private var diagnosticsSection: some View {
        SettingsSection(title: "TECHNICAL DETAILS") {
            jitRows

            AppleOverlayToggle()

            Toggle(isOn: $host.showDiagnostics) {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Show technical stuff")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(.white)
                    Text("FPS and what the app is doing behind the scenes. You don't need this "
                         + "to send feedback, it's attached for you.")
                        .font(.system(size: 12))
                        .foregroundStyle(ShellPalette.secondaryText)
                }
            }
            .tint(ShellPalette.accent)

            // The full block, only when asked for. It is the one debugger a sideloaded build has,
            // so it stays reachable, but a first-time tester should not open Settings onto it.
            if host.showDiagnostics {
                DiagnosticsPanel(host: host, emulation: emulation, saveStates: saveStates)
            }

            technicalReadouts
            moreTechnicalReadouts
        }
    }

    /// The second half of Settings' read-outs, the same way as `technicalReadouts`: word for word,
    /// under the section they came from, in screen order, in a Group of their own.
    private var moreTechnicalReadouts: some View {
        Group {
            TechnicalReadoutGroup(title: "COVER ART") {
                SettingsReadout(label: "Stored", value: artwork.storageLine)
                SettingsReadout(label: "Cover lists", value: artwork.coverListLine)
                SettingsReadout(label: "Last", value: artwork.line)
            }
            TechnicalReadoutGroup(title: "BIOS") {
                SettingsReadout(label: "State", value: host.bios.isEmpty
                                ? "no core in this build needs a BIOS file except the PlayStation cores"
                                : host.bios)
                // Every name the install button acts on, which used to end the BIOS note.
                SettingsReadout(label: "Names it looks for", value: CoreCatalog.biosNameList())
            }
            TechnicalReadoutGroup(title: "SAVE STATES") {
                SettingsReadout(label: "Stored", value: saveStates.storageLine)
                SettingsReadout(label: "Last", value: saveStates.line)
            }
            // STORAGE's Artwork, Cover lists and Save states lines were the same strings as the
            // ones under COVER ART and SAVE STATES above, so only Library is new here.
            TechnicalReadoutGroup(title: "STORAGE") {
                SettingsReadout(label: "Library", value: host.libraryStatus)
            }
            // Observes its own object, like the RETROACHIEVEMENTS lines. See CloudSync.swift.
            TechnicalReadoutGroup(title: "SYNC FOLDER") {
                CloudSyncReadouts(sync: host.cloudSync)
            }
        }
    }

    /// The read-outs that used to sit in the other sections, word for word, under the section
    /// they came from and in the same order as the screen. In a Group, for the ten-children
    /// reason in `body`; the next sections' read-outs go in a second Group.
    private var technicalReadouts: some View {
        Group {
            TechnicalReadoutGroup(title: "SPEED AND REWIND") {
                SettingsReadout(label: "Rewind holds", value: emulation.rewindReadout)
            }
            TechnicalReadoutGroup(title: "GAME CONTROLLERS") {
                SettingsReadout(label: "Attached", value: controllers.summary)
            }
            TechnicalReadoutGroup(title: "ON-SCREEN CONTROLS") {
                SettingsReadout(label: "Arrangement", value: host.touchLayoutLine)
            }
            TechnicalReadoutGroup(title: "CHEATS") {
                SettingsReadout(label: "Stored", value: cheats.storageLine)
                SettingsReadout(label: "Last", value: cheats.line)
            }
            // These two observe their own objects, which this screen does not, so their lines
            // stay live. See Achievements.swift and Peripherals.swift.
            TechnicalReadoutGroup(title: "RETROACHIEVEMENTS") {
                AchievementsReadouts(store: host.achievements)
            }
            TechnicalReadoutGroup(title: "MICROPHONE, CAMERA AND AMIIBO") {
                PeripheralsReadouts(peripherals: host.peripherals)
            }
        }
    }

    /// JIT: whether it is on, a switch to keep it off, and a shortcut to StikDebug where that is
    /// all it takes. Everything works without it (owner rule); it only makes heavy systems faster.
    @ViewBuilder
    private var jitRows: some View {
        let report = host.jitReport
        SettingsReadout(label: "JIT", value: report?.sentence ?? "Checking...")

        Toggle(isOn: $host.useJitWhenAvailable) {
            VStack(alignment: .leading, spacing: 3) {
                Text("Use JIT when it's available")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(.white)
                // "and N64 on older iPhones" until build 138, which is no longer true: the N64
                // was the last system that could not use JIT on an iPhone 13 or newer running
                // iOS 26, and it can now. Every system with a JIT build gets it on every phone
                // that can switch JIT on.
                Text("Makes PSP, 3DS, PlayStation, N64 and Dreamcast faster. If a game acts up "
                     + "with it, turn this off. Kicks in from the next game you open.")
                    .font(.system(size: 12))
                    .foregroundStyle(ShellPalette.secondaryText)
            }
        }
        .tint(ShellPalette.accent)

        if report?.canAskEnabler == true {
            SettingsButton(title: "Turn on JIT with StikDebug", role: .normal) {
                host.askStikDebugForJit()
            }
        }

        // Only on a phone that needs its code memory blessed and where that has not happened yet.
        // Normally the app does this by itself on coming back from StikDebug; this is for someone
        // who turned JIT on from StikDebug directly.
        if report?.needsSetup == true {
            SettingsButton(title: "Finish setting JIT up", role: .normal) {
                host.setUpJitNow()
            }
            SettingsNote(
                "Only works if you turned JIT on with StikDebug and picked the universal script. "
                + "If you used the button above, this is already done for you."
            )
        }
    }

    // MARK: Cores

    /// Every system with more than one core, PlayStation included, in one folding section
    /// near the top. A game's own card can override this per game (GameDetailSheet, CORE).
    private var coreSwitchSection: some View {
        SettingsSection(title: "CORES") {
            let _ = host.coreChoiceVersion
            SettingsLabel("Core for PlayStation games")
            SegmentedChoice(
                options: Ps1CoreChoice.allCases,
                title: { $0.label },
                selection: $host.ps1CoreChoice
            )
            ForEach(CoreCatalog.switchableSystems, id: \.self) { system in
                SettingsLabel("Core for \(system.displayName) games")
                SegmentedChoice(
                    options: CoreCatalog.coreOptions(for: system),
                    title: { CoreCatalog.core(id: $0)?.displayName.components(separatedBy: " (").first ?? $0 },
                    selection: Binding(
                        get: { host.coreChoice(for: system) },
                        set: { host.setCoreChoice($0, for: system) }
                    )
                )
            }
            SettingsNote(
                "Kicks in from the next game you open. For just one game, press and hold it and "
                + "pick a core on its card."
            )
            SettingsNote(
                "Not sure? Keep the first one. Save states only load on the core that made them."
            )
        }
    }

    // MARK: BIOS

    private var biosSection: some View {
        SettingsSection(title: "BIOS") {
            // The STATE read-out and the full list of file names it recognises are in TECHNICAL
            // DETAILS (`moreTechnicalReadouts`). Each row below names its own files.
            SettingsNote(
                "Put BIOS files in the Continuum folder in Files (or add them with +), then tap "
                + "the button below. Each system below says what it needs."
            )

            SettingsButton(title: "Install a BIOS from the Continuum folder", role: .normal) {
                host.installBiosFromDocuments()
            }

            // One row per system or core that uses a file from the system folder, read fresh.
            // `status` is read so the list re-renders after the install button above.
            let _ = host.status
            ForEach(Array(host.firmwareChecklist().enumerated()), id: \.offset) { _, row in
                SettingsNote("\(row.label): \(row.value)")
            }
        }
    }

    // MARK: Save states

    /// The resume switch, what the states cost, and the one button that throws them all away.
    ///
    /// Everything about an individual state is in that game's detail sheet, which is where it
    /// belongs: a list of every state on the device, across games, would be a list nobody navigates
    /// by. What is here is what is only answerable at this level, which is the total and the
    /// setting.
    private var saveStatesSection: some View {
        SettingsSection(title: "SAVE STATES") {
            Toggle(isOn: $saveStates.autoSavesEnabled) {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Auto-save")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(.white)
                    Text(saveStates.autoSavesEnabled
                         ? "On. Leaving a game saves your spot."
                         : "Off. Nothing's saved unless you save it.")
                        .font(.system(size: 12))
                        .foregroundStyle(ShellPalette.secondaryText)
                }
            }
            .tint(ShellPalette.accent)

            // Two switches rather than one, because writing the auto-save and resuming into it are
            // different questions. Keeping the auto-save written while NOT resuming into it every
            // launch is a combination someone reasonably wants, and one switch could not express it.
            Toggle(isOn: $saveStates.resumesAutomatically) {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Pick up where you left off")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(.white)
                    Text(saveStates.resumesAutomatically
                         ? "On. Games open right where you left them."
                         : "Off. Every game starts from the beginning.")
                        .font(.system(size: 12))
                        .foregroundStyle(ShellPalette.secondaryText)
                }
            }
            .tint(ShellPalette.accent)
            // Dimmed rather than hidden when auto-save is off. With nothing being written there is
            // nothing to resume into, so leaving it live would be a switch that does nothing, which
            // is the one thing this screen has a rule against. Hiding it instead would make the row
            // appear and disappear as the switch above is flipped, and a control that moves is
            // harder to find again than one that is visibly unavailable.
            .disabled(!saveStates.autoSavesEnabled)
            .opacity(saveStates.autoSavesEnabled ? 1 : 0.45)

            SettingsNote(
                "It saves when you leave a game or switch to another app. Turning it off keeps "
                + "the auto-saves you have, and your own save slots are never touched."
            )

            // The STORED and LAST read-outs are in TECHNICAL DETAILS (`moreTechnicalReadouts`).
            SettingsNote(saveStateCountLine)

            // Always shown, even with an empty library. There was never a delete-all before:
            // games went one at a time, from a card or a swipe in the list.
            SettingsButton(title: "Delete every game (\(host.library.count))", role: .destructive) {
                confirmDeleteGames = true
            }
            .alert("Delete every game?", isPresented: $confirmDeleteGames) {
                Button("Delete", role: .destructive) {
                    host.deleteEntries(at: IndexSet(host.library.indices))
                }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text(deleteGamesMessage)
            }

            SettingsButton(title: "Delete every save state", role: .destructive) {
                confirmDeleteStates = true
            }
            .alert("Delete every save state?", isPresented: $confirmDeleteStates) {
                Button("Delete", role: .destructive) { saveStates.deleteEverything() }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("This includes the auto-saves, so every game starts from the beginning. "
                     + "This cannot be undone.")
            }

            SettingsNote("Each game's own states are on its card (press and hold the game).")
        }
    }

    /// The one plain line SAVE STATES keeps, with real plurals, like CHEATS.
    private var saveStateCountLine: String {
        let count = saveStates.records.count
        guard count > 0 else { return "No save states yet" }
        let games = saveStates.gameCount
        return "\(count) \(count == 1 ? "save state" : "save states") for "
            + "\(games) \(games == 1 ? "game" : "games")"
    }

    /// What "Delete every game?" says, with real plurals.
    private var deleteGamesMessage: String {
        switch host.library.count {
        case 0:
            return "There are no games in the library right now."
        case 1:
            return "Your one game goes, with its save states and cheats. In-game saves are kept. "
                + "This cannot be undone."
        default:
            return "All \(host.library.count) games go, with their save states and cheats. "
                + "In-game saves are kept. This cannot be undone."
        }
    }

    // MARK: Cheats

    /// What is stored, whether the running core takes cheats at all, and the way to clear the lot.
    ///
    /// The per game list is in the detail sheet for the same reason the save states are: a code only
    /// means anything next to the game it was written for. This section exists because "how many
    /// cheats has this app got and is the core actually using them" cannot be answered from any one
    /// game's card, and because the core's own count is the only honest confirmation that a code
    /// was accepted.
    private var cheatsSection: some View {
        SettingsSection(title: "CHEATS") {
            // The STORED and LAST read-outs are in TECHNICAL DETAILS (`technicalReadouts`).
            SettingsNote(cheatCountLine)

            SettingsButton(title: "Delete every stored cheat", role: .destructive) {
                confirmDeleteCheats = true
            }
            .alert("Delete every stored cheat?", isPresented: $confirmDeleteCheats) {
                Button("Delete", role: .destructive) { cheats.deleteEverything() }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("Every game's cheat codes are removed. This cannot be undone.")
            }

            SettingsNote(
                "Add cheats on a game's card (press and hold the game), or from the ⋯ menu while "
                + "you play. Some emulators ignore them."
            )
        }
    }

    /// The one plain line CHEATS keeps, with real plurals.
    private var cheatCountLine: String {
        let count = cheats.cheats.count
        guard count > 0 else { return "No cheats yet" }
        let games = cheats.gameCount
        return "\(count) \(count == 1 ? "cheat" : "cheats") for "
            + "\(games) \(games == 1 ? "game" : "games")"
    }

    // MARK: Storage

    private var storageSection: some View {
        SettingsSection(title: "STORAGE") {
            // The LIBRARY read-out is in TECHNICAL DETAILS (`moreTechnicalReadouts`). The other
            // three were the same lines as COVER ART's and SAVE STATES', and are under those.
            SettingsNote(
                "Your games are in the Continuum folder in the Files app. Don't rename a disc "
                + "game's files there, or it won't load."
            )
        }
    }

    // MARK: Picture

    /// Note every explanation here is STATIC and covers all the options at once, rather than
    /// describing the selected one. That is not a style preference, it is the fix for a real bug:
    /// per-option text is different lengths, so changing the selection changed the height of the
    /// section and shifted the control out from under the finger that was still choosing. See
    /// `SegmentedChoice`. Nothing in these sections may change height as a selection changes.
    private var pictureSection: some View {
        SettingsSection(title: "PICTURE") {
            SettingsLabel("Screen fit")
            SegmentedChoice(options: EmulationSettings.ScreenFit.allCases,
                            title: { $0.label },
                            selection: $emulation.screenFit)
            SettingsNote(
                "Fit keeps the shape right. Pixel perfect keeps every pixel the same size, and "
                + "Fill stretches to the whole screen."
            )

            SettingsLabel("Scaling")
            SegmentedChoice(options: EmulationSettings.PixelFilter.allCases,
                            title: { $0.label },
                            selection: $emulation.pixelFilter)
            SettingsNote("Sharp keeps the pixels crisp. Smooth softens them a little.")
        }
    }

    // MARK: Sound

    private var soundSection: some View {
        SettingsSection(title: "SOUND") {
            Toggle(isOn: $emulation.muted) {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Mute")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(.white)
                    Text(emulation.muted
                         ? "Silent. The game still runs at full speed."
                         : "Sound on.")
                        .font(.system(size: 12))
                        .foregroundStyle(ShellPalette.secondaryText)
                }
            }
            .tint(ShellPalette.accent)

            VStack(alignment: .leading, spacing: 6) {
                Text("VOLUME \(Int((emulation.volume * 100).rounded()))%")
                    .font(.system(size: 10, weight: .bold))
                    .tracking(1.1)
                    .foregroundStyle(Color.white.opacity(0.45))
                Slider(value: $emulation.volume, in: 0...1)
                    .tint(ShellPalette.accent)
                    .disabled(emulation.muted)
            }

            SettingsNote("This works on top of your phone's own volume.")
        }
    }

    // MARK: Speed and rewind

    private var speedSection: some View {
        SettingsSection(title: "SPEED AND REWIND") {
            SettingsLabel("Fast forward")
            SegmentedChoice(options: EmulationSettings.FastForward.allCases,
                            title: { $0.label },
                            selection: $emulation.fastForward)

            SettingsLabel("Rewind")
            SegmentedChoice(options: EmulationSettings.RewindBudget.allCases,
                            title: { $0.label },
                            selection: $emulation.rewindBudget)

            // What the rewind memory bought is in TECHNICAL DETAILS (`technicalReadouts`).
            SettingsNote(
                "Hold the rewind button in a game to go back. It uses extra memory, and how far "
                + "back you can go depends on the game."
            )
        }
    }

    // MARK: What is not wired

    /// The gaps, when there are any.
    ///
    /// DRAWS NOTHING AT ALL WHEN `gaps` IS EMPTY, which is the state this build is in: the last entry
    /// was retired when in-game cover capture was wired, so the list is empty for the first time. A
    /// heading reading "NOT WIRED YET" over an explanation of a list with nothing in it is not
    /// information, it is a section asking to be read and then answering nothing, and this screen's
    /// rule against controls that do nothing is the same rule. The section, the `Gap` type and the
    /// array all stay exactly where they are, because the next thing the engine cannot do yet goes
    /// here and one line in the array brings the whole section back.
    ///
    /// `@ViewBuilder` so the empty case is genuinely nothing rather than an empty container, and this
    /// stays ONE child of `body`'s second `Group` either way, which matters: that builder is near the
    /// ten child ceiling described at the top of `body`.
    @ViewBuilder
    private var notYetWiredSection: some View {
        if !Self.gaps.isEmpty {
            SettingsSection(title: "NOT WIRED YET") {
                gapList
            }
        }
    }

    /// The contents of the section above, split out only to keep that `if` legible.
    private var gapList: some View {
        VStack(alignment: .leading, spacing: 12) {
            SettingsNote(
                "What is listed here is missing rather than broken. Each one needs work in the "
                + "emulator engine before it can be switched on."
            )
            ForEach(Self.gaps) { gap in
                VStack(alignment: .leading, spacing: 2) {
                    Text(gap.name)
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundStyle(Color.white.opacity(0.9))
                    Text(gap.reason)
                        .font(.system(size: 12))
                        .foregroundStyle(ShellPalette.secondaryText)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }

    /// One missing control and the reason it is missing.
    ///
    /// A named struct rather than a tuple because `ForEach` needs an `Identifiable` element or a
    /// key path, and a key path into a labelled tuple is not something to rely on across compiler
    /// versions.
    struct Gap: Identifiable {
        var id: String { name }
        let name: String
        let reason: String
    }

    /// The gaps, each with the reason. Written out because the design reference shows controls for
    /// all of them and a later reader would otherwise assume they were forgotten.
    ///
    /// EMPTY, FOR NOW, and the section above draws nothing while it is. Everything the design
    /// reference shows has something real behind it.
    private static let gaps: [Gap] = [
        // "Cover art from the game itself" was here, and it is now in COVER ART on a game's card and
        // behind a press on the player's save button. The call it was waiting for, a framebuffer
        // readback, is exported, so the entry had stopped being a missing feature and become a false
        // statement about the app, which is the same reason the entry below it went.
        //
        // "Physical controllers" was here, and it is now GAME CONTROLLERS above. The call it was
        // waiting for, an input push that names which source layer it belongs to, is exported, so
        // the entry was no longer a missing feature but a false statement about the app. An
        // out-of-date gap is worse than a missing control, because the honesty of this whole list
        // is the only thing that makes it worth reading.
    ]
}

// MARK: - The pieces a settings screen is made of

/// Which settings cards are folded. Every `SettingsSection` reads this, so Open all / Close all
/// and a tap on one heading stay in agreement. Missing means open: the page starts as it always
/// did, and a heading the user shuts stays shut.
final class SettingsFolds: ObservableObject {
    static let shared = SettingsFolds()

    private let closedKey = "continuum.settings.closedSections"
    private let forceKey = "continuum.settings.forceClosed"
    private let exceptKey = "continuum.settings.forceClosedExceptions"

    @Published private var closed: Set<String>
    @Published private var forceClosed: Bool
    @Published private var openedDespiteForce: Set<String>

    private init() {
        let defaults = UserDefaults.standard
        closed = Set(defaults.stringArray(forKey: closedKey) ?? [])
        // A fresh install starts with every section closed (Brett, build 171). An install that
        // has ever saved a fold state keeps it.
        let fresh = defaults.object(forKey: forceKey) == nil && defaults.object(forKey: closedKey) == nil
        forceClosed = fresh ? true : defaults.bool(forKey: forceKey)
        openedDespiteForce = Set(defaults.stringArray(forKey: exceptKey) ?? [])
    }

    func isOpen(_ title: String) -> Bool {
        if forceClosed { return openedDespiteForce.contains(title) }
        return !closed.contains(title)
    }

    func toggle(_ title: String) {
        if forceClosed {
            if openedDespiteForce.contains(title) {
                openedDespiteForce.remove(title)
            } else {
                openedDespiteForce.insert(title)
            }
        } else if closed.contains(title) {
            closed.remove(title)
        } else {
            closed.insert(title)
        }
        save()
    }

    func openAll() {
        forceClosed = false
        closed = []
        openedDespiteForce = []
        save()
    }

    func closeAll() {
        forceClosed = true
        closed = []
        openedDespiteForce = []
        save()
    }

    private func save() {
        let defaults = UserDefaults.standard
        defaults.set(Array(closed), forKey: closedKey)
        defaults.set(forceClosed, forKey: forceKey)
        defaults.set(Array(openedDespiteForce), forKey: exceptKey)
    }
}

private struct SettingsFoldButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 13, weight: .semibold))
            .foregroundStyle(ShellPalette.accent)
            .opacity(configuration.isPressed ? 0.55 : 1)
    }
}

/// A titled group on a dark card. The heading opens and closes the card.
struct SettingsSection<Content: View>: View {
    let title: String
    /// False for a card that is a form rather than a Settings group (the feedback and crash
    /// report sheets): always open, no chevron, untouched by Settings' "start closed" and
    /// Open all / Close all, which used to leave those forms showing only their Send button.
    let foldable: Bool
    @ViewBuilder var content: Content
    @ObservedObject private var folds = SettingsFolds.shared

    init(title: String, foldable: Bool = true, @ViewBuilder content: () -> Content) {
        self.title = title
        self.foldable = foldable
        self.content = content()
    }

    private var heading: some View {
        Text(title)
            .font(.system(size: 11, weight: .bold))
            .tracking(1.6)
            .foregroundStyle(ShellPalette.secondaryText)
    }

    var body: some View {
        let open = !foldable || folds.isOpen(title)
        VStack(alignment: .leading, spacing: open ? 12 : 0) {
            if foldable {
                Button {
                    withAnimation(.easeInOut(duration: 0.2)) {
                        folds.toggle(title)
                    }
                } label: {
                    HStack(spacing: 8) {
                        heading
                        Spacer(minLength: 8)
                        Image(systemName: "chevron.down")
                            .font(.system(size: 11, weight: .bold))
                            .foregroundStyle(ShellPalette.secondaryText)
                            .rotationEffect(.degrees(open ? 0 : -90))
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(title)
                .accessibilityValue(open ? "open" : "closed")
            } else {
                heading
                    .accessibilityAddTraits(.isHeader)
            }

            if open {
                content
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(16)
        .background(ShellPalette.surface, in: RoundedRectangle(cornerRadius: 14))
        .padding(.horizontal, 16)
    }
}

/// The name of the control underneath it.
///
/// Needed because `SegmentedChoice` draws no label of its own, the same as the stock segmented
/// control it replaced: there is nowhere sensible inside a row of equal segments to put one. With
/// three selectors now stacked in one card, an unlabelled row of four sizes and an unlabelled row
/// of four speeds would be guesswork.
struct SettingsLabel: View {
    let text: String

    init(_ text: String) {
        self.text = text
    }

    var body: some View {
        Text(text.uppercased())
            .font(.system(size: 10, weight: .bold))
            .tracking(1.1)
            .foregroundStyle(Color.white.opacity(0.45))
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// A sentence of explanation. Plain language, because every note in here is either a disclosure or
/// the reason something is missing.
struct SettingsNote: View {
    let text: String

    init(_ text: String) {
        self.text = text
    }

    var body: some View {
        Text(text)
            .font(.system(size: 12))
            .foregroundStyle(ShellPalette.secondaryText)
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// A label and a value, in the monospace the diagnostics use, because most of these values are
/// diagnostic strings and must not be reflowed into something prettier and less exact.
struct SettingsReadout: View {
    let label: String
    let value: String

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(label.uppercased())
                .font(.system(size: 10, weight: .bold))
                .tracking(1.1)
                .foregroundStyle(Color.white.opacity(0.45))
            Text(value)
                .font(.system(.caption2, design: .monospaced))
                .foregroundStyle(Color.white.opacity(0.85))
                .fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// One section's read-outs in TECHNICAL DETAILS, under that section's name as it appears on
/// screen. The read-outs moved there from the sections themselves, so a player reads plain lines
/// and a tester can still quote the exact strings.
struct TechnicalReadoutGroup<Content: View>: View {
    let title: String
    @ViewBuilder var content: Content

    init(title: String, @ViewBuilder content: () -> Content) {
        self.title = title
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title)
                .font(.system(size: 14, weight: .semibold))
                .foregroundStyle(Color.white.opacity(0.9))
            content
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// A full-width action.
struct SettingsButton: View {
    enum Role {
        case normal
        case destructive
    }

    let title: String
    let role: Role
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(title)
                .font(.system(size: 14, weight: .semibold))
                .foregroundStyle(role == .destructive ? ShellPalette.accent : .white)
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .frame(maxWidth: .infinity)
                .background(
                    RoundedRectangle(cornerRadius: 10)
                        .fill(ShellPalette.surfaceStrong)
                        .overlay(
                            RoundedRectangle(cornerRadius: 10)
                                .strokeBorder(role == .destructive
                                              ? ShellPalette.accent.opacity(0.55)
                                              : Color.clear,
                                              lineWidth: 1)
                        )
                )
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}


/// A segmented selector that reliably follows a dragging finger, and looks like the system's.
///
/// ## Why it is glass and not a block of accent colour
///
/// THIS APPEARANCE IS A DELIBERATE MATCH TO THE NATIVE iOS SEGMENTED CONTROL AND SHOULD NOT BE
/// "TIDIED UP" INTO A FILLED RECTANGLE. The first version of this control drew the selection as
/// a solid accent-red rectangle, which was flagged on sight: a saturated filled block is
/// Material Design's language, not the recessed-groove-with-a-floating-thumb that iOS uses, and
/// on a phone running a version of iOS whose whole design language is translucency it looked
/// like an Android app. So the track is a dark groove and the thumb is `ultraThinMaterial` with
/// a hairline specular edge and a small soft shadow.
///
/// The material matters more than the colour. It is a real blur of what sits behind it, so it
/// inherits whatever the running OS does with materials rather than being a grey that had to be
/// guessed; that is why the thumb is not simply `Color.white.opacity(0.14)`.
///
/// The accent colour is used for SELECTION ELSEWHERE in this app, on the tab bar and the Play
/// pill, and that is not an inconsistency: those are single emphasised actions, where iOS also
/// uses colour. A segmented control indicates its selection by POSITION and ELEVATION instead,
/// and borrowing the accent for it was what made it look foreign.
///
/// When the Android build arrives, the flat filled style is the correct one THERE, and this note
/// exists so that it is understood as a per-platform decision rather than a mistake to be
/// unified. The engine is shared; the look of a selection control is not.
///
/// ## Why this exists instead of `Picker` with `.pickerStyle(.segmented)`
///
/// The stock segmented control is the right INTERACTION: put a thumb on it, slide, and the
/// selection follows. It is also close to unusable inside a `ScrollView`, and the two reasons
/// compound into a control that answers about one drag in ten.
///
/// The first reason is gesture arbitration. A vertical `ScrollView` and a horizontal drag are
/// not as separable as they sound: a thumb sliding sideways across a phone always travels a
/// little vertically too, and the scroll view is entitled to claim the sequence the moment it
/// sees that. When it does, the segmented control never sees the rest of the drag and the
/// selection springs back to where it started, which reads as the control ignoring you.
///
/// The second reason was mine, and it is worth recording because it is invisible in a
/// screenshot. Each of these selectors used to be followed by an explanatory sentence that
/// CHANGED WITH THE SELECTION, and those sentences are different lengths. So sliding across
/// the control rewrote the paragraph underneath it, the section grew or shrank by a line or
/// two, and everything below moved. Including, sometimes, the control itself: a target that
/// slides out from under the finger mid-drag defeats any amount of correct gesture handling.
/// The fix for that half is in the sections above, where the notes are now static text that
/// covers every option, so choosing can no longer change the height of anything.
///
/// This control fixes the first half. `DragGesture(minimumDistance: 0)` begins on touch-down
/// rather than after a threshold, and `highPriorityGesture` gives it to this view ahead of the
/// scroll view, so once a finger lands here the whole gesture belongs to the selector. The
/// deliberate cost: a drag that STARTS on a selector will not scroll the page. That is the
/// right trade, because a finger placed on a selector was placed there to choose something.
///
/// Two smaller things that matter more than they look:
///
///   - The track is 44 points tall, which is Apple's minimum comfortable target and noticeably
///     taller than the stock control. Most of "selecting is hard" is a target too small to
///     land on while holding a phone one-handed.
///   - It ticks. `UISelectionFeedbackGenerator` fires on every change, so a selection that
///     landed is felt as well as seen. That is not decoration: the complaint that started this
///     rewrite was not knowing whether a choice had registered, and on a control you operate
///     with the thumb that is covering it, touch is the sense that is actually free.
struct SegmentedChoice<Option: Hashable>: View {
    let options: [Option]
    let title: (Option) -> String
    @Binding var selection: Option

    /// Apple's minimum comfortable hit target. See the note above on why this is the fix for
    /// most of "selecting is hard".
    private static var trackHeight: CGFloat { 44 }

    /// Held as state rather than made per event, because a feedback generator is meant to be
    /// kept alive by whatever uses it; one constructed and thrown away on each change is doing
    /// the expensive half of the work and skipping the cheap half.
    @State private var feedback = UISelectionFeedbackGenerator()

    var body: some View {
        GeometryReader { proxy in
            let segment = options.isEmpty ? proxy.size.width
                : proxy.size.width / CGFloat(options.count)
            let index = options.firstIndex(of: selection) ?? 0

            ZStack(alignment: .leading) {
                // A CAPSULE, NOT A ROUNDED SQUARE, and this is the shape the whole control was
                // rebuilt for. An earlier pass changed the thumb's material and left both of these
                // at a 10 and 8 point radius, which on a 44 point track is visually square, and the
                // difference was called out immediately on a device: the corner radius was doing
                // more to make it look like an Android control than the colour ever was. A capsule
                // takes its radius from its own height, so this cannot drift back to looking boxy
                // if the track height changes.
                Capsule(style: .continuous)
                    .fill(Color.black.opacity(0.28))
                    .overlay(
                        Capsule(style: .continuous)
                            .strokeBorder(Color.white.opacity(0.07), lineWidth: 0.5)
                    )

                // The thumb, and this is the part being asked for: glass rather than a painted
                // block. See the type's note on why it is not the accent colour.
                //
                // Drawn from the selection rather than moved by the gesture, so there is exactly
                // one source of truth for where it sits. A gesture that dragged it directly could
                // end up disagreeing with the value actually stored.
                Capsule(style: .continuous)
                    // `ultraThinMaterial` is what makes it glass rather than a grey rectangle: it
                    // is a real blur of what is behind it, so it picks up the card, the artwork
                    // and whatever the system is doing with materials on the running OS, instead
                    // of being a colour that has to be guessed to match.
                    .fill(.ultraThinMaterial)
                    .overlay(
                        // The specular edge. A single hairline of white at low opacity is what
                        // reads as a lit top edge on a physical control, and it is the difference
                        // between "translucent" and "glass".
                        Capsule(style: .continuous)
                            .strokeBorder(Color.white.opacity(0.24), lineWidth: 0.5)
                    )
                    // Lifts it out of the groove. Small and soft: a heavy shadow here looks like a
                    // floating card rather than a segment resting in a track.
                    .shadow(color: Color.black.opacity(0.32), radius: 2.5, x: 0, y: 1)
                    // Three points, not two, so the groove stays visible all the way around a
                    // capsule. At two the thumb's curve meets the track's curve and the gap
                    // disappears at the ends, which is what makes a pill read as a filled box.
                    .padding(3)
                    .frame(width: segment)
                    .offset(x: segment * CGFloat(index))
                    // A spring rather than a fixed curve, so the thumb settles the way the
                    // system's own does instead of arriving on a timer.
                    .animation(.interactiveSpring(response: 0.24, dampingFraction: 0.86),
                               value: index)

                HStack(spacing: 0) {
                    ForEach(options, id: \.self) { option in
                        Text(title(option))
                            // The selected label carries the weight, since the thumb behind it is
                            // now subtle rather than a block of colour. Without this the selection
                            // is legible on a bright screen and nearly invisible in sunlight.
                            .font(.system(size: 14,
                                          weight: option == selection ? .semibold : .medium))
                            .foregroundStyle(option == selection
                                             ? Color.white
                                             : ShellPalette.secondaryText)
                            .lineLimit(1)
                            // Shrinks rather than truncating, because a label clipped to "Pixel
                            // perf..." is a worse outcome than one a point smaller.
                            .minimumScaleFactor(0.75)
                            .frame(width: segment, height: Self.trackHeight)
                    }
                }
            }
            // The whole track takes touches, including the gaps between labels. Without this the
            // responsive area is the text, which is the small-target problem again.
            .contentShape(Rectangle())
            .highPriorityGesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { value in choose(atX: value.location.x, segment: segment) }
                    // Also on the release, so a straight tap with no movement still selects. A
                    // tap does deliver one `onChanged`, but relying on that alone would make the
                    // simplest interaction depend on the subtlest behaviour.
                    .onEnded { value in choose(atX: value.location.x, segment: segment) }
            )
        }
        .frame(height: Self.trackHeight)
    }

    /// Selects whichever segment the finger is over.
    ///
    /// Clamped rather than ignored outside the track, so a thumb that slides off the end holds
    /// the last option instead of dropping the drag. Sliding past the edge is how someone
    /// reaches for the final choice in a hurry, and it should land there.
    private func choose(atX x: CGFloat, segment: CGFloat) {
        guard segment > 0, !options.isEmpty else { return }
        let raw = Int((x / segment).rounded(.down))
        let option = options[min(max(raw, 0), options.count - 1)]
        guard option != selection else { return }
        feedback.selectionChanged()
        selection = option
    }
}


// MARK: - Open-source credits

/// The emulators and libraries shipped inside the app, with each one's license and where its
/// source code lives. GPL cores require that users can get the source; the links do that.
/// Keep this in step with the dylibs in the .ipa when a core is added or removed.
struct OpenSourceCreditsList: View {
    struct Credit: Identifiable {
        let name: String
        let systems: String
        let license: String
        let source: String
        var id: String { name }
    }

    static let credits: [Credit] = [
        .init(name: "bsnes", systems: "SNES", license: "GPL v3", source: "https://github.com/libretro/bsnes-libretro"),
        .init(name: "Mesen 2", systems: "NES", license: "GPL v3", source: "https://github.com/libretro/MesenCE"),
        .init(name: "FCEUmm", systems: "NES, Famicom Disk System", license: "GPL v2", source: "https://github.com/libretro/libretro-fceumm"),
        .init(name: "Gearsystem", systems: "Master System, Game Gear, SG-1000", license: "GPL v3", source: "https://github.com/drhelius/Gearsystem"),
        .init(name: "BlastEm", systems: "Mega Drive, Sega CD, 32X", license: "GPL v3", source: "https://github.com/libretro/blastem"),
        .init(name: "mGBA", systems: "Game Boy, Game Boy Color, Game Boy Advance", license: "MPL 2.0", source: "https://github.com/libretro/mgba"),
        .init(name: "melonDS", systems: "Nintendo DS", license: "GPL v3", source: "https://github.com/libretro/melonds"),
        .init(name: "Azahar", systems: "Nintendo 3DS", license: "GPL v3", source: "https://github.com/azahar-emu/azahar"),
        .init(name: "ParaLLEl N64", systems: "Nintendo 64", license: "GPL v2", source: "https://github.com/libretro/parallel-n64"),
        .init(name: "PCSX ReARMed", systems: "PlayStation", license: "GPL v2", source: "https://github.com/libretro/pcsx_rearmed"),
        .init(name: "Beetle PSX HW", systems: "PlayStation", license: "GPL v2", source: "https://github.com/libretro/beetle-psx-libretro"),
        .init(name: "PPSSPP", systems: "PSP", license: "GPL v2 or later", source: "https://github.com/libretro/ppsspp"),
        .init(name: "Flycast", systems: "Dreamcast", license: "GPL v2", source: "https://github.com/libretro/flycast"),
        .init(name: "Yabause", systems: "Saturn", license: "GPL v2", source: "https://github.com/libretro/yabause"),
        .init(name: "Beetle Saturn", systems: "Saturn", license: "GPL v2", source: "https://github.com/libretro/beetle-saturn-libretro"),
        .init(name: "Beetle PCE, PCE Fast, SuperGrafx", systems: "PC Engine / TurboGrafx-16", license: "GPL v2", source: "https://github.com/libretro/beetle-pce-libretro"),
        .init(name: "Beetle VB", systems: "Virtual Boy", license: "GPL v2", source: "https://github.com/libretro/beetle-vb-libretro"),
        .init(name: "Beetle WonderSwan", systems: "WonderSwan", license: "GPL v2", source: "https://github.com/libretro/beetle-wswan-libretro"),
        .init(name: "Beetle NeoPop", systems: "Neo Geo Pocket", license: "GPL v2", source: "https://github.com/libretro/beetle-ngp-libretro"),
        .init(name: "Stella", systems: "Atari 2600", license: "GPL v2", source: "https://github.com/libretro/stella2023"),
        .init(name: "a5200", systems: "Atari 5200", license: "GPL v2", source: "https://github.com/libretro/a5200"),
        .init(name: "ProSystem", systems: "Atari 7800", license: "GPL v2", source: "https://github.com/libretro/prosystem-libretro"),
        .init(name: "Handy", systems: "Atari Lynx", license: "zlib license", source: "https://github.com/libretro/libretro-handy"),
        .init(name: "PokeMini", systems: "Pokemon Mini", license: "GPL v3", source: "https://github.com/libretro/PokeMini"),
        .init(name: "PUAE", systems: "Amiga", license: "GPL v2", source: "https://github.com/libretro/libretro-uae"),
        .init(name: "VICE x64sc", systems: "Commodore 64", license: "GPL v2", source: "https://github.com/libretro/vice-libretro"),
        .init(name: "Virtual Jaguar", systems: "Atari Jaguar", license: "GPL v3", source: "https://github.com/libretro/virtualjaguar-libretro"),
        .init(name: "DOSBox Pure", systems: "DOS", license: "GPL v2", source: "https://github.com/libretro/dosbox-pure"),
        .init(name: "PrBoom", systems: "Doom", license: "GPL v2", source: "https://github.com/libretro/libretro-prboom"),
        .init(name: "Ruffle", systems: "Flash", license: "MIT or Apache 2.0", source: "https://github.com/ruffle-rs/ruffle"),
        .init(name: "J2meJS", systems: "J2ME", license: "GPL v2", source: "https://github.com/Rosabis/J2meJS"),
        .init(name: "Continuum Symbian (built on EKA2L1)", systems: "Symbian / N-Gage", license: "GPL v3", source: "https://github.com/EKA2L1/EKA2L1"),
        .init(name: "rcheevos", systems: "Achievements", license: "MIT", source: "https://github.com/RetroAchievements/rcheevos"),
        .init(name: "libsmb2", systems: "SMB file sharing", license: "LGPL v2.1", source: "https://github.com/sahlberg/libsmb2"),
        .init(name: "MoltenVK", systems: "Vulkan graphics", license: "Apache 2.0", source: "https://github.com/KhronosGroup/MoltenVK"),
        .init(name: "Continuum", systems: "This app", license: "GPL v3", source: "https://github.com/2c2bhdhw9z-cell/Continuum-")
    ]

    @State private var open = false

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            SettingsButton(title: open ? "Hide open-source credits" : "Open-source credits",
                           role: .normal) { open.toggle() }
            if open {
                SettingsNote(
                    "Continuum is built on these free emulators and libraries. Each one is the "
                    + "work of its own authors and keeps its own license. Tap a name to see its "
                    + "source code."
                )
                ForEach(Self.credits) { credit in
                    VStack(alignment: .leading, spacing: 2) {
                        if let url = URL(string: credit.source) {
                            Link(credit.name, destination: url)
                                .font(.system(size: 14, weight: .semibold))
                        } else {
                            Text(credit.name).font(.system(size: 14, weight: .semibold))
                        }
                        Text("\(credit.systems) · \(credit.license)")
                            .font(.system(size: 12))
                            .foregroundStyle(ShellPalette.secondaryText)
                    }
                }
            }
        }
    }
}
