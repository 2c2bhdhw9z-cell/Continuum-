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
// re-asserts them into an engine whose pacer does not survive a session. That list at the bottom is
// correspondingly much shorter than it was, and everything left on it is genuinely still missing.
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

    /// Read fresh from the engine when this screen appears, never cached across appearances. The
    /// core's option list changes per core and a stale list is worse than none.
    @State private var coreOptions: [CoreOptionRecord] = []
    @State private var coreOptionsNote = "not read yet"

    /// Whether the control layout editor is up.
    ///
    /// Presented full screen rather than as a sheet, and that is the one thing about it that is not
    /// a matter of taste: the editor's whole job is to show the controls where they will really be,
    /// and a sheet is inset from the bottom of the screen, which is exactly the edge both thumb
    /// clusters live at. A card cannot show you where a control sits relative to a screen it does
    /// not cover.
    @State private var showControlEditor = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                Text("Settings")
                    .font(.system(size: 26, weight: .heavy))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 16)

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
                    // In this group rather than the second one, along the seam the note above
                    // describes: a cheat changes how the game itself behaves.
                    cheatsSection
                }
                Group {
                    artworkSection
                    layoutSection
                    diagnosticsSection
                    biosSection
                    coreOptionsSection
                    // Beside STORAGE on purpose: the two read-outs are about the same disk, and the
                    // note in STORAGE now has to explain three directories rather than two.
                    saveStatesSection
                    storageSection
                    notYetWiredSection
                }
            }
            .padding(.bottom, 24)
        }
        .onAppear {
            refreshCoreOptions()
            // Read on appearance rather than continuously: this is a sentence about the rewind
            // tape, not telemetry, and polling the engine for it every frame would take the
            // engine lock sixty times a second to redraw text that barely changes.
            emulation.refreshRewindReadout()
        }
        .fullScreenCover(isPresented: $showControlEditor) {
            TouchLayoutEditor(
                initialLayout: host.touchLayout,
                // The host's own `didSet` is what persists it. The editor is handed a closure rather
                // than the host so a drag in progress cannot republish the host, and with it every
                // view in the library shell underneath this one, on every touch move. See the note
                // on `TouchLayoutEditor`.
                onCommit: { layout in host.touchLayout = layout },
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
                         ? "On, so the library shows real box art."
                         : "Off, so every game shows its generated plate.")
                        .font(.system(size: 12))
                        .foregroundStyle(ShellPalette.secondaryText)
                }
            }
            .tint(ShellPalette.accent)

            // The disclosure, in plain words. Resolving art sends a filename to a third party, and
            // that is a real disclosure rather than an implementation detail.
            SettingsNote(
                "Looking up a cover sends that game's filename to thumbnails.libretro.com, which "
                + "is a server run by the libretro project and not by this app. Nothing else is "
                + "sent: no account, no device name, and never the game file itself. A cover is "
                + "downloaded once and kept on this device, so the library keeps showing it with "
                + "no network at all. With this off, nothing leaves the device and every game "
                + "shows the generated plate it already has."
            )

            // The second disclosure, and the reason it is here rather than in a release note: the
            // fallback search downloads the largest thing this app ever fetches, and it does it
            // without being asked.
            SettingsNote(
                "Most covers are found by name in one request. When a game's name matches nothing, "
                + "usually because it was dumped under a different naming convention, the app asks "
                + "the server for its own list of covers and matches on the title. There is one "
                + "list per system and per folder: box art first, then title screens, then in-game "
                + "shots, and a folder's list is only downloaded when the cheaper ones found "
                + "nothing, so a game whose box art is listed costs one list and no more. Only the "
                + "names are kept and not the page itself, and each list is then used for every "
                + "later search with no further download for a month. A big system is a few "
                + "megabytes a list: the NES box art list is about 4 MB and its title screen and "
                + "in-game shot lists are about 4.9 MB each, while Game Gear is about 240 KB a "
                + "list. Nothing extra is sent: the request is for the server's own public index."
            )

            SettingsNote(
                "If a game has no art anywhere under its own system, the same title is looked for "
                + "in the other systems' lists, smallest list first, and at most two extra lists "
                + "are downloaded for any one game. A cover found that way says where it came "
                + "from, for example \"box art from Game Boy\", and never pretends to be this "
                + "console's own. A title that matches two or more covers is left alone rather "
                + "than guessed at: it keeps its plate, and opening that game's card shows every "
                + "cover the server has for it so you can pick one."
            )

            SettingsReadout(label: "Stored", value: artwork.storageLine)
            SettingsReadout(label: "Cover lists", value: artwork.coverListLine)
            SettingsReadout(label: "Last", value: artwork.line)

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

            SettingsNote(
                "Clearing throws away the downloaded covers and nothing else; the plates come "
                + "straight back and a cover is fetched again next time it is on screen. A title "
                + "the server has no art for is remembered for a week rather than asked about on "
                + "every launch, and Retry forgets that so it is asked again now, for every "
                + "game in the library and not only the ones on screen. Forgetting the cover "
                + "lists is the separate one: it clears every one of them, per system and per "
                + "folder, gives back the megabytes they take, and the next game that needs the "
                + "fallback search downloads the one list it needs again."
            )
        }
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
    private var controllerSection: some View {
        SettingsSection(title: "GAME CONTROLLERS") {
            SettingsReadout(label: "Attached", value: controllers.summary)

            Toggle(isOn: $controllers.autoHidesOnScreenPad) {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Hide the on-screen pad while a controller is connected")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(.white)
                    Text(controllers.autoHidesOnScreenPad
                         ? "On, and the picture uses the space the controls were holding."
                         : "Off, so both work at once, including on the same button.")
                        .font(.system(size: 12))
                        .foregroundStyle(ShellPalette.secondaryText)
                }
            }
            .tint(ShellPalette.accent)

            SettingsNote(
                "Off is the default on purpose. A controller can be connected and not in your "
                + "hands, charging on a desk or paired from yesterday and across the room, and a "
                + "pad that vanished on its own with no way back would leave the game unplayable. "
                + "With this on, the controls come straight back the moment the controller "
                + "disconnects. Either way the buttons along the top of the player, pause, reset, "
                + "save state, the diagnostics and the way back to the library, stay where they "
                + "are: this only hides the game pad."
            )

            SettingsNote(
                "One exception, and the read-out above names it when it applies: a pad with no View "
                + "or Share button has no SELECT, so the on-screen pad stays put even with this on. "
                + "Hiding it would leave SELECT reachable from nothing at all, and that is how a "
                + "Game Boy or NES game opens its own menu."
            )

            SettingsNote(
                "Pairing is done by iOS, not here: hold the pairing button on the controller, then "
                + "open Settings, Bluetooth and tap it. It appears above as soon as it connects, "
                + "with no need to restart this app, and the light or number on the controller "
                + "shows which player it became. Xbox, DualShock, DualSense and MFi pads all "
                + "arrive the same way. Up to four are read at once, one per port, and the first "
                + "one to connect is player 1 and keeps that place when others join or leave."
            )

            SettingsNote(
                "The buttons are mapped by POSITION rather than by the letter printed on them, "
                + "which is what makes a DualSense and an Xbox pad behave identically: the bottom "
                + "button of the diamond is always B, the right one is always A, and the left and "
                + "top are Y and X. That is the arrangement these consoles used, where A sits to "
                + "the right of B. Shoulders and triggers are L, R, L2 and R2. The Menu button, "
                + "Options on a DualShock, is START, and the small button on the left, View or "
                + "Share, is SELECT. The left stick works as a D-pad as well, so a game that only "
                + "reads directions is playable without touching the D-pad, and both sticks are "
                + "passed through for the cores that read them."
            )

            SettingsNote(
                "A controller and the on-screen pad are independent, all the way down: the "
                + "emulator keeps a separate set of buttons for each and combines them when the "
                + "game reads its controls, so one cannot cancel the other out even when both are "
                + "pressing the same button in the same instant. A controller that disconnects "
                + "mid-press releases everything it was holding rather than leaving the game "
                + "stuck running in one direction."
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
            SettingsReadout(label: "Arrangement", value: host.touchLayoutLine)

            SettingsButton(title: "Move the on-screen controls", role: .normal) {
                showControlEditor = true
            }

            SettingsNote(
                "Opens the pad full screen with the two thumb groups outlined, and drags them where "
                + "you want them. Sliders for size and how faint they are, a swap for a left-handed "
                + "grip, and the room left for the picture shown as you go, because moving a group "
                + "inward takes that room away. Remembered between launches."
            )

            // Reachable from here as well as inside the editor, on the same reasoning as the
            // library's layout toggle: the editor is the place you go to fiddle, and putting the
            // way back to the shipped arrangement behind a screenful of fiddling is the wrong way
            // round when what you want is to undo it.
            SettingsButton(title: "Reset the controls to the default arrangement",
                           role: .destructive) {
                host.touchLayout = .standard
            }
            .disabled(host.touchLayout.isStandard)
            .opacity(host.touchLayout.isStandard ? 0.45 : 1)

            SettingsNote(
                "Every position is a fraction of the screen rather than a number of pixels, so one "
                + "arrangement is right on every device and in both orientations, and each one is "
                + "held inside limits that keep a control on screen and clear of the few "
                + "millimetres at each edge that iOS reserves for its own swipe gestures. The pad "
                + "checks the result it laid out for controls sitting on top of each other and says "
                + "so on the diagnostic line, so a bad arrangement is reported rather than shipped."
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

            SettingsNote(
                "These two are remembered between launches, and both are about the library. The "
                + "on-screen game controls are a separate setting with a confusingly similar name: "
                + "they are in ON-SCREEN CONTROLS above."
            )
        }
    }

    // MARK: Diagnostics

    private var diagnosticsSection: some View {
        SettingsSection(title: "DIAGNOSTICS") {
            Toggle(isOn: $host.showDiagnostics) {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Show the diagnostic block")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(.white)
                    Text("The thin status line above the tabs stays either way.")
                        .font(.system(size: 12))
                        .foregroundStyle(ShellPalette.secondaryText)
                }
            }
            .tint(ShellPalette.accent)

            // The same block both screens share, shown here in full so Settings is a real home for
            // it rather than only a switch that turns it on somewhere else.
            DiagnosticsPanel(host: host, emulation: emulation, saveStates: saveStates)

            SettingsNote(
                "This is the only debugger a sideloaded build has: no console, no crash log and no "
                + "attached Xcode. Each line answers a different question. No GPU line means the "
                + "Metal attach failed. A GPU line with zero frames means the renderer is up and "
                + "the core is not producing. A cores line naming a missing dylib means that "
                + "system's core never reached the bundle. A running line naming the wrong core "
                + "means the extension route is wrong."
            )
        }
    }

    // MARK: BIOS

    private var biosSection: some View {
        SettingsSection(title: "BIOS") {
            SettingsReadout(label: "State", value: host.bios.isEmpty
                            ? "no core in this build needs a BIOS file except PCSX ReARMed"
                            : host.bios)

            SettingsNote(
                "PCSX ReARMed is the only core here that looks for one, and it does not require "
                + "it: with no BIOS it falls back to HLE and still boots, at reduced accuracy and "
                + "compatibility. A missing BIOS is a note, not an error."
            )

            SettingsButton(title: "Install a BIOS from the Continuum folder", role: .normal) {
                host.installBiosFromDocuments()
            }

            SettingsNote(
                "The core reads its BIOS from the app's Application Support directory, which the "
                + "Files app does not show. The Continuum folder the Files app DOES show is the "
                + "Documents directory, where imports land. So put a BIOS file there, by importing "
                + "it with the plus button or by dropping it into the Continuum folder, and then "
                + "tap the button above to copy it across. Recognised names are "
                + CoreCatalog.biosNameList()
                + ". No BIOS is bundled with this app, because shipping a console BIOS is a "
                + "copyright violation."
            )
        }
    }

    // MARK: Core options

    private var coreOptionsSection: some View {
        SettingsSection(title: "CORE OPTIONS") {
            SettingsReadout(label: "Core", value: coreOptionsNote)

            if coreOptions.isEmpty {
                SettingsNote(
                    "Core options are declared by the core itself, so there is nothing to show "
                    + "unless a core is resident. Leaving a game hands the core back to the "
                    + "registry and unloads it, which is what keeps one emulator in memory instead "
                    + "of five, so this list is normally populated only while a game is running."
                )
            } else {
                ForEach(coreOptions, id: \.key) { option in
                    HStack(spacing: 10) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(option.label.isEmpty ? option.key : option.label)
                                .font(.system(size: 14, weight: .medium))
                                .foregroundStyle(.white)
                            Text(option.key)
                                .font(.system(.caption2, design: .monospaced))
                                .foregroundStyle(ShellPalette.secondaryText)
                        }
                        Spacer(minLength: 8)
                        Menu {
                            ForEach(option.values, id: \.self) { value in
                                Button(value) {
                                    host.applyCoreOption(key: option.key, value: value,
                                                         label: option.label)
                                    refreshCoreOptions()
                                }
                            }
                        } label: {
                            HStack(spacing: 4) {
                                Text(option.value)
                                    .font(.system(size: 13, weight: .semibold))
                                Image(systemName: "chevron.down")
                                    .font(.system(size: 10, weight: .bold))
                            }
                            .foregroundStyle(ShellPalette.metadata)
                            .padding(.horizontal, 10)
                            .padding(.vertical, 6)
                            .background(ShellPalette.surface, in: Capsule())
                        }
                    }
                }
            }

            SettingsButton(title: "Read the options again", role: .normal) {
                refreshCoreOptions()
            }
        }
    }

    private func refreshCoreOptions() {
        // Residency is asked of the engine, never of a cached flag. See `EngineHost.residentCore`.
        if let resident = host.residentCore() {
            coreOptions = host.coreOptionRecords()
            coreOptionsNote = "\(resident.spec.coreId) is resident (state \(resident.state)), "
                + "\(coreOptions.count) option(s) declared"
        } else {
            coreOptions = []
            coreOptionsNote = "no core is resident right now"
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
            Toggle(isOn: $saveStates.resumesAutomatically) {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Pick up where you left off")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(.white)
                    Text(saveStates.resumesAutomatically
                         ? "On, so starting a game restores its auto-save."
                         : "Off, so every game starts from the beginning.")
                        .font(.system(size: 12))
                        .foregroundStyle(ShellPalette.secondaryText)
                }
            }
            .tint(ShellPalette.accent)

            SettingsNote(
                "Leaving a game, or the app going to the background, writes that game's auto-save, "
                + "and starting the game again loads it back. There is no save every few seconds "
                + "while you play, on purpose: saving a PlayStation game's state is a megabyte of "
                + "work, and a stutter every few seconds would be a worse trade than an auto-save "
                + "that is a few minutes old. Rewind, in SPEED AND REWIND above, is the setting for "
                + "undoing the last few seconds."
            )

            SettingsReadout(label: "Stored", value: saveStates.storageLine)
            SettingsReadout(label: "Last", value: saveStates.line)

            SettingsButton(title: "Delete every save state", role: .destructive) {
                saveStates.deleteEverything()
            }

            SettingsNote(
                "States are kept outside the Continuum folder the Files app shows, which is "
                + "deliberate: a slot owns its file, and a payload renamed or moved underneath the "
                + "list that describes it would leave the app listing states it can no longer load. "
                + "They are included in a device backup, unlike the artwork, because nothing can "
                + "reproduce them. Deleting everything here cannot be undone and it includes the "
                + "auto-saves, so every game starts from the beginning afterwards. One game's states "
                + "are managed in that game's card, from the library."
            )
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
            SettingsReadout(label: "Stored", value: cheats.storageLine)
            SettingsReadout(label: "Last", value: cheats.line)

            SettingsButton(title: "Delete every stored cheat", role: .destructive) {
                cheats.deleteEverything()
            }

            SettingsNote(
                "Cheats are added in a game's card, from the library, because a code is written for "
                + "one game and one region of it. The list is pushed into the emulated console when "
                + "the game starts and again whenever it changes, so a code added mid-game takes "
                + "effect without a relaunch. Not every core takes them: the read-out above says "
                + "what the running one does, and a core that takes none is a stated fact about "
                + "that core rather than a failure."
            )
            SettingsNote(
                "Codes are never checked for shape. Each system has its own convention and cores "
                + "accept several, so a front end that insisted on one pattern would reject codes "
                + "that work. A code the core cannot parse is ignored by it, which is the same "
                + "outcome as a code for the wrong region of the same game."
            )
        }
    }

    // MARK: Storage

    private var storageSection: some View {
        SettingsSection(title: "STORAGE") {
            SettingsReadout(label: "Library", value: host.libraryStatus)
            SettingsReadout(label: "Artwork", value: artwork.storageLine)
            SettingsReadout(label: "Cover lists", value: artwork.coverListLine)
            SettingsReadout(label: "Save states", value: saveStates.storageLine)
            SettingsNote(
                "Games live in the app's own Documents directory, which the Files app shows as "
                + "Continuum, and they keep the filenames they arrived with: a cue sheet names its "
                + "tracks literally, so renaming one would break the game. Artwork is kept outside "
                + "Documents, so a folder of downloaded covers never appears there as though it "
                + "were something you imported, and the downloaded cover lists sit in their own "
                + "folder beside it so the two can be cleared separately. Deleting a game is a "
                + "swipe in All Games, or the button in its detail sheet, and it takes that game's "
                + "save states and cheats with it."
            )
            SettingsNote(
                "Save states and cheats are outside Documents too, each in its own folder, and for "
                + "a stronger reason than the artwork's: a numbered slot owns its file. Documents is "
                + "user-visible, so anything in it can be renamed or moved from the Files app, and a "
                + "payload that moved underneath the list describing it would leave a slot the app "
                + "can list and cannot load. An earlier build wrote one state per game into "
                + "Documents and had no way to read it back; that file is not used any more, and "
                + "any left over from it can be deleted from the Continuum folder."
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
                "Fit is the normal choice: as large as the picture goes while keeping the right "
                + "shape, with black bars on the short edge. Pixel perfect makes every emulated "
                + "pixel exactly the same size as its neighbours so the grid stays even, which "
                + "costs a little more of the screen and is the reason people ask for it. Fill "
                + "uses the whole screen and stretches the picture to do it: nothing is cut off, "
                + "but circles stop being round."
            )

            SettingsLabel("Scaling")
            SegmentedChoice(options: EmulationSettings.PixelFilter.allCases,
                            title: { $0.label },
                            selection: $emulation.pixelFilter)
            SettingsNote(
                "Sharp keeps hard pixel edges, the way these games were drawn, and is the "
                + "default. Smooth blends neighbouring pixels, which some people prefer on the "
                + "older systems."
            )

            SettingsNote(
                "Both apply the moment you change them, to the game running now and to every "
                + "game after it. They are remembered, so a game started tomorrow looks the way "
                + "you left this."
            )
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

            SettingsNote(
                "Volume is applied inside the emulator rather than by the system, so it is "
                + "separate from the phone's own volume and the two multiply. Muting is not the "
                + "same as sliding to zero: muting also throws away the sound already queued, so "
                + "unmuting picks up at the present moment instead of replaying the second you "
                + "missed."
            )
        }
    }

    // MARK: Speed and rewind

    private var speedSection: some View {
        SettingsSection(title: "SPEED AND REWIND") {
            SettingsLabel("Fast forward")
            SegmentedChoice(options: EmulationSettings.FastForward.allCases,
                            title: { $0.label },
                            selection: $emulation.fastForward)
            SettingsNote(
                "How fast the fast-forward button in the player runs while you hold it. Sound "
                + "keeps playing and rises in pitch, the way fast-forward has always sounded. "
                + "The list stops at 4x because that is genuinely where the emulator stops: "
                + "beyond it the extra frames are dropped rather than run, so a 8x button would "
                + "read 8x and give you 4x."
            )

            SettingsLabel("Rewind memory")
            SegmentedChoice(options: EmulationSettings.RewindBudget.allCases,
                            title: { $0.label },
                            selection: $emulation.rewindBudget)

            SettingsReadout(label: "Rewind holds", value: emulation.rewindReadout)

            SettingsNote(
                "Rewind works by quietly saving the game ten times a second and stepping back "
                + "through those saves when you hold the rewind button in the player. The setting "
                + "is memory rather than seconds because the two are not the same thing: a "
                + "PlayStation save is around a hundred times the size of an NES one, so the same "
                + "96 MB is minutes of NES and seconds of Crash Bandicoot. The line above says "
                + "what it actually bought on the game you are playing."
            )
            SettingsNote(
                "Off is the default and costs nothing. Turning it on spends that memory for as "
                + "long as a game is running, and on a phone that matters: iOS closes an app that "
                + "grows too large rather than slowing it down. Resetting a game or loading a "
                + "save clears the history, because winding back past either would take you "
                + "somewhere you never were."
            )
        }
    }

    // MARK: What is not wired

    private var notYetWiredSection: some View {
        SettingsSection(title: "NOT WIRED YET") {
            SettingsNote(
                "What is listed here is absent rather than broken. It needs something exported "
                + "from the engine that is not exported today, and each one is recorded in "
                + "FEAT-006 with what it unlocks. Physical controllers used to be on this list and "
                + "are now real, in GAME CONTROLLERS above."
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
    private static let gaps: [Gap] = [
        Gap(name: "Cover art from the game itself",
            reason: "The fourth artwork tier is a capture from a running game, which is the only "
            + "tier that works for a ROM no database has heard of. It needs a framebuffer readback "
            + "exported from the engine."),
        // "Physical controllers" was here, and it is now GAME CONTROLLERS above. The call it was
        // waiting for, an input push that names which source layer it belongs to, is exported, so
        // the entry was no longer a missing feature but a false statement about the app. An
        // out-of-date gap is worse than a missing control, because the honesty of this whole list
        // is the only thing that makes it worth reading.
    ]
}

// MARK: - The pieces a settings screen is made of

/// A titled group on a dark card.
struct SettingsSection<Content: View>: View {
    let title: String
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title)
                .font(.system(size: 11, weight: .bold))
                .tracking(1.6)
                .foregroundStyle(ShellPalette.secondaryText)
            content
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
                // The recessed track. Deliberately darker than the card it sits on, because the
                // whole illusion is a groove with something floating in it, and a track lighter
                // than its surroundings reads as a raised slab instead.
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(Color.black.opacity(0.28))
                    .overlay(
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .strokeBorder(Color.white.opacity(0.07), lineWidth: 0.5)
                    )

                // The thumb, and this is the part being asked for: glass rather than a painted
                // block. See the type's note on why it is not the accent colour.
                //
                // Drawn from the selection rather than moved by the gesture, so there is exactly
                // one source of truth for where it sits. A gesture that dragged it directly could
                // end up disagreeing with the value actually stored.
                RoundedRectangle(cornerRadius: 8, style: .continuous)
                    // `ultraThinMaterial` is what makes it glass rather than a grey rectangle: it
                    // is a real blur of what is behind it, so it picks up the card, the artwork
                    // and whatever the system is doing with materials on the running OS, instead
                    // of being a colour that has to be guessed to match.
                    .fill(.ultraThinMaterial)
                    .overlay(
                        // The specular edge. A single hairline of white at low opacity is what
                        // reads as a lit top edge on a physical control, and it is the difference
                        // between "translucent" and "glass".
                        RoundedRectangle(cornerRadius: 8, style: .continuous)
                            .strokeBorder(Color.white.opacity(0.24), lineWidth: 0.5)
                    )
                    // Lifts it out of the groove. Small and soft: a heavy shadow here looks like a
                    // floating card rather than a segment resting in a track.
                    .shadow(color: Color.black.opacity(0.32), radius: 2.5, x: 0, y: 1)
                    .padding(2)
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
