// Continuum - Settings: the artwork controls, the layout controls, and the permanent home of the
// diagnostics.
//
// THE RULE THIS FILE IS BUILT AROUND: every control in here does something today. The design
// reference shows selectors for aspect, filter, speed, scale and volume, and not one of them can be
// wired, because `set_scale_mode`, filter selection, the pacer's speed multiplier, mute and
// `drain_audio` are all absent from the engine's exported surface. A switch that does nothing is
// worse than an absent one: it teaches the user that the app lies. So those are not here as dead
// controls, they are listed at the bottom as a read-only note saying what each one needs, which is
// information rather than a promise.
//
// Nothing was deleted from the diagnostic HUD in moving it here. The same strings, in the same
// order, are still one tap away from the status strip on both screens.

import SwiftUI

struct SettingsScreen: View {
    @ObservedObject var host: EngineHost
    @ObservedObject var artwork: ArtworkStore

    /// Read fresh from the engine when this screen appears, never cached across appearances. The
    /// core's option list changes per core and a stale list is worse than none.
    @State private var coreOptions: [CoreOptionRecord] = []
    @State private var coreOptionsNote = "not read yet"

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                Text("Settings")
                    .font(.system(size: 26, weight: .heavy))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 16)

                artworkSection
                layoutSection
                diagnosticsSection
                biosSection
                coreOptionsSection
                storageSection
                notYetWiredSection
            }
            .padding(.bottom, 24)
        }
        .onAppear(perform: refreshCoreOptions)
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

    // MARK: Layout

    private var layoutSection: some View {
        SettingsSection(title: "LAYOUT") {
            VStack(alignment: .leading, spacing: 8) {
                Text("All Games")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(.white)
                Picker("All Games layout", selection: $host.libraryLayout) {
                    ForEach(LibraryLayout.allCases) { layout in
                        Text(layout.title).tag(layout)
                    }
                }
                .pickerStyle(.segmented)
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
                "These two are remembered between launches. Moving and resizing the on-screen game "
                + "controls is a bigger piece of work and is not here yet: the control layout is "
                + "already a value the player screen takes, with limits that keep a button on "
                + "screen and clear of the system edge gestures, so the editor is a screen that "
                + "writes six numbers rather than a rewrite. It is recorded as FEAT-006."
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
            DiagnosticsPanel(host: host)

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

    // MARK: Storage

    private var storageSection: some View {
        SettingsSection(title: "STORAGE") {
            SettingsReadout(label: "Library", value: host.libraryStatus)
            SettingsReadout(label: "Artwork", value: artwork.storageLine)
            SettingsReadout(label: "Cover lists", value: artwork.coverListLine)
            SettingsNote(
                "Games live in the app's own Documents directory, which the Files app shows as "
                + "Continuum, and they keep the filenames they arrived with: a cue sheet names its "
                + "tracks literally, so renaming one would break the game. Artwork is kept outside "
                + "Documents, so a folder of downloaded covers never appears there as though it "
                + "were something you imported, and the downloaded cover lists sit in their own "
                + "folder beside it so the two can be cleared separately. Deleting a game is a "
                + "swipe in All Games, or the button in its detail sheet."
            )
        }
    }

    // MARK: What is not wired

    private var notYetWiredSection: some View {
        SettingsSection(title: "NOT WIRED YET") {
            SettingsNote(
                "These are absent rather than broken. Each one needs something exported from the "
                + "engine that is not exported today, and they are recorded in FEAT-006 with what "
                + "each one unlocks."
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
        Gap(name: "Aspect and integer scale",
            reason: "The renderer already has the modes and a setter for them. It is not exported "
            + "through the engine's Swift interface, so nothing here can call it."),
        Gap(name: "Filter, nearest or linear",
            reason: "The renderer distinguishes them already. Same reason: not exported."),
        Gap(name: "Speed and fast forward",
            reason: "The frame pacer supports a speed multiplier. Not exported. Rewind is a "
            + "different matter, because a save state runs from roughly 13 KB to 1 MB and a rewind "
            + "ring needs a memory budget."),
        Gap(name: "Volume",
            reason: "There is no audio output path on this build at all: the engine queues audio "
            + "and the drain is not exported, so a volume control would have nothing to control. "
            + "Audio output comes first and volume falls out of it. The queued figure in the "
            + "diagnostics is real, it is measuring a ring nothing is draining."),
        Gap(name: "Moving the on-screen controls",
            reason: "The layout is already a value with limits and clamping, so this is an editor "
            + "screen rather than a rewrite. It is the next piece of UI work, not an engine gap."),
        Gap(name: "Cover art from the game itself",
            reason: "The fourth artwork tier is a capture from a running game, which is the only "
            + "tier that works for a ROM no database has heard of. It needs a framebuffer readback "
            + "exported from the engine."),
        Gap(name: "Physical controllers",
            reason: "The engine merges input per source already, so a real pad and the on-screen "
            + "pad could be used together, but the only exported input call replaces the whole "
            + "gamepad layer, so the two would fight over it."),
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
