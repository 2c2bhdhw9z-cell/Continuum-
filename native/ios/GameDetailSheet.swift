// Continuum - one game's detail sheet: what it is, where its cover came from, and what can be done
// to it.
//
// A sheet rather than a pushed view, for the reason the shell's header gives: the Metal canvas is
// mounted once for the app's lifetime and a navigation push that replaced it would tear down the
// layer the engine attached to. A sheet is drawn above the existing tree and disturbs nothing.
//
// THE CHOOSER LIVES HERE AND NOWHERE ELSE. Automatic resolution takes the first cover that answers
// and says nothing at all about the others, so no card in the library hints that there was a choice to
// make. Someone who opens this sheet and goes looking gets the full set: each folder of the game's own
// system, plus the same title in other systems' lists, each one labelled with what it is and where it
// came from. The enumeration runs on that open, never during browsing.
//
// This is also where the honest detail lives. The design reference's detail view shows a
// description and a rating; this one shows the filename on disk, the real total size of a
// multi-file game, which cue tracks are missing, and which core the tap will route to, because
// those are the four things this app actually knows and every one of them has explained a failure
// at least once.

import SwiftUI

struct GameDetailSheet: View {
    let entry: LibraryEntry
    @ObservedObject var host: EngineHost
    @ObservedObject var artwork: ArtworkStore

    /// THE MAIN PLACE SAVE STATES ARE MANAGED, which is why the store is observed here rather than
    /// reached through the host: deleting a state has to take its row out of the list under the
    /// finger that deleted it, and loading one has to update the line that says when it was taken.
    @ObservedObject var saveStates: SaveStates

    /// Observed for the same reason: the cheat list below is edited in place.
    @ObservedObject var cheats: CheatStore

    /// The cheat being typed. Held by the sheet rather than the store because a half-typed code is
    /// not a cheat yet, and a store that published every keystroke would rebuild this whole sheet on
    /// each one.
    @State private var draftCode = ""
    @State private var draftLabel = ""

    /// Why the last Add was refused, or empty. A message under the field rather than an alert: the
    /// answer is always something about the code that is still on screen, and an alert would hide it.
    @State private var cheatProblem = ""

    /// The artwork section's own state, owned by this sheet and nothing else.
    ///
    /// A @StateObject here rather than properties on the store, and that is deliberate: the library
    /// shell observes the store, so a thumbnail published there would rebuild every card on screen
    /// each time one of these images arrived. It lives exactly as long as the sheet does, and the
    /// caches it fills live on the store, so reopening this card is free.
    @StateObject private var chooser = ArtworkChooserModel()

    private var system: GameSystem? {
        CoreCatalog.system(forExtension: entry.ext)
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                header
                actions
                facts
                saveStatesBlock
                cheatsBlock
                artworkBlock
                dangerZone
            }
            .padding(18)
        }
        .background(Color.black.ignoresSafeArea())
        .presentationDragIndicator(.visible)
    }

    // MARK: Header

    private var header: some View {
        HStack(alignment: .top, spacing: 14) {
            CoverArtView(entry: entry, system: system, store: artwork,
                         generation: artwork.generation, showsCaption: true)
                .frame(width: 118, height: 157)
                .clipShape(RoundedRectangle(cornerRadius: 10))
                .overlay(
                    RoundedRectangle(cornerRadius: 10)
                        .strokeBorder(Color.white.opacity(0.12), lineWidth: 0.5)
                )

            VStack(alignment: .leading, spacing: 6) {
                HStack(alignment: .top, spacing: 6) {
                    Text(GameMetadata.displayTitle(for: entry))
                        .font(.system(size: 21, weight: .heavy))
                        .foregroundStyle(.white)
                        .fixedSize(horizontal: false, vertical: true)
                    Spacer(minLength: 0)
                    FavouriteButton(isFavourite: host.favourites.contains(entry.id), size: 17) {
                        host.toggleFavourite(entry)
                    }
                }
                Text(GameMetadata.metaLine(for: entry, system: system))
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(ShellPalette.metadata)
                    .fixedSize(horizontal: false, vertical: true)
                if let system {
                    Text(system.displayName)
                        .font(.system(size: 12))
                        .foregroundStyle(ShellPalette.secondaryText)
                }
                Spacer(minLength: 0)
            }
        }
    }

    // MARK: Actions

    private var actions: some View {
        HStack(spacing: 10) {
            PillButton(title: "Play", systemImage: "play.fill", filled: true) {
                // The one launch path, unchanged. Clearing the binding is what dismisses the sheet,
                // so the player screen is not opened underneath it.
                let target = entry
                host.detailEntry = nil
                host.launch(entry: target)
            }
            PillButton(title: "Close", systemImage: nil, filled: false) {
                host.detailEntry = nil
            }
            Spacer(minLength: 0)
        }
    }

    // MARK: The facts

    private var facts: some View {
        VStack(alignment: .leading, spacing: 10) {
            SettingsReadout(label: "On disk as", value: entry.name)
            SettingsReadout(label: "Detail", value: entry.detail)
            SettingsReadout(
                label: "Runs on",
                value: CoreCatalog.core(forExtension: entry.ext)?.displayName
                    ?? "no core is mapped to .\(entry.ext)"
            )
            if !entry.cueNotes.isEmpty {
                // The commonest import mistake by a distance: bringing the .cue and leaving its
                // .bin tracks behind. Said out loud here rather than only as part of the detail
                // line.
                SettingsNote(
                    "This cue sheet is incomplete: "
                    + entry.cueNotes.joined(separator: ", ")
                    + ". A PlayStation game is a .cue plus every .bin track it names, so import "
                    + "the missing tracks with the same names and it will boot."
                )
            }
        }
        .padding(14)
        .background(ShellPalette.surface, in: RoundedRectangle(cornerRadius: 12))
    }

    // MARK: Save states

    /// Every state this game has, newest first, with what each one is and what can be done to it.
    ///
    /// THIS IS THE MANAGEMENT SCREEN. The player has a menu on the save button, and that menu is
    /// deliberately a shortlist: it is operated with a game running underneath it and a thumb in the
    /// way. Everything a decision actually needs, when it was taken, which slot, how large it is and
    /// what frame it stopped at, is here, along with delete, which has no business being one stray
    /// press away from a game in progress.
    ///
    /// Reads no payloads. Every figure in every row comes from the metadata index, which is the
    /// whole reason the index is separate: a game with forty states would otherwise cost forty file
    /// reads and tens of megabytes to open this card. See the header of SaveStates.swift.
    private var saveStatesBlock: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("SAVE STATES")
                .font(.system(size: 11, weight: .bold))
                .tracking(1.6)
                .foregroundStyle(ShellPalette.secondaryText)

            let states = saveStates.states(for: entry)

            if states.isEmpty {
                SettingsNote(
                    "No states for this game yet. The save button at the top of the player writes "
                    + "one, and leaving the game or putting the app in the background writes a "
                    + "separate auto-save that this game picks up from next time."
                )
            } else {
                SettingsReadout(label: "Stored", value: Self.summary(of: states))
                ForEach(states) { record in
                    stateRow(record)
                }
                SettingsButton(title: "Delete every state for this game", role: .destructive) {
                    saveStates.deleteAll(forGameId: SaveStates.gameId(for: entry))
                }
            }

            SettingsNote(
                "A state can only be loaded back into the same build of the same core that wrote "
                + "it, so each one remembers which core that was, that core's version and its exact "
                + "length, and a state that fails any of those checks is refused with the reason "
                + "rather than loaded. That is not caution for its own sake: handing a core a state "
                + "from a different build does not reliably fail, it can appear to work and leave "
                + "the game quietly broken minutes later."
            )
        }
        .padding(14)
        .background(ShellPalette.surface, in: RoundedRectangle(cornerRadius: 12))
    }

    /// One state: what it is on the left, what can be done to it on the right.
    ///
    /// Both actions are 44 points tall, which is the minimum comfortable target, and they are the
    /// reason this is a row of its own rather than a line of text with a swipe: a destructive action
    /// on a list inside a sheet inside a scroll view is exactly where a swipe gesture gets eaten.
    private func stateRow(_ record: SaveStateRecord) -> some View {
        let isRunningThisGame = host.running && host.activeEntry?.id == entry.id
        let stored = saveStates.isStored(record)
        return HStack(alignment: .center, spacing: 10) {
            VStack(alignment: .leading, spacing: 2) {
                Text(record.isAuto ? "Auto-save" : record.slotLabel)
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(.white)
                Text("\(record.ageText) \u{00B7} \(record.sizeText) \u{00B7} frame \(record.frame)")
                    .font(.system(.caption2, design: .monospaced))
                    .foregroundStyle(ShellPalette.secondaryText)
                // Said in the row rather than only when it is tapped. A record whose payload has
                // gone is not dropped from the list, because the app deciding on its own that a
                // state no longer exists is worse than a row that explains itself.
                if !stored {
                    Text("its file is no longer stored")
                        .font(.system(size: 11))
                        .foregroundStyle(ShellPalette.accent)
                }
            }
            Spacer(minLength: 6)

            // "Load" into the game that is already running, "Play" when it is not: the second one
            // launches the game and loads the state into it, which is what someone tapping a state
            // from the library actually means. Without that, this button would be dead for anyone
            // who had not started the game first, and the sheet is reachable from the library.
            Button {
                if isRunningThisGame {
                    saveStates.load(record)
                } else {
                    let target = entry
                    host.detailEntry = nil
                    host.launchAndLoad(entry: target, record: record)
                }
            } label: {
                Text(isRunningThisGame ? "Load" : "Play")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(.white)
                    .frame(minWidth: 58, minHeight: 44)
                    .background(ShellPalette.surfaceStrong, in: RoundedRectangle(cornerRadius: 9))
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(!stored)
            .opacity(stored ? 1 : 0.45)

            Button {
                saveStates.delete(record)
            } label: {
                Image(systemName: "trash")
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(ShellPalette.accent)
                    .frame(width: 44, height: 44)
                    .background(ShellPalette.surfaceStrong, in: RoundedRectangle(cornerRadius: 9))
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Delete \(record.slotLabel)")
        }
    }

    /// The line above the rows: how many, how much, and whether one of them is the auto-save.
    private static func summary(of states: [SaveStateRecord]) -> String {
        let bytes = states.reduce(Int64(0)) { $0 + Int64(max(0, $1.byteCount)) }
        let autos = states.filter { $0.isAuto }.count
        var parts = ["\(states.count) state(s)", SaveStates.byteText(bytes)]
        parts.append(autos > 0 ? "including the auto-save" : "no auto-save yet")
        return parts.joined(separator: " \u{00B7} ")
    }

    // MARK: Cheats

    /// This game's cheat list, and the field that adds to it.
    ///
    /// Per game rather than global, because a code is meaningless outside the game it was written
    /// for. The list is pushed into the core as a whole and in order every time it changes, which is
    /// what `CheatStore.push` is for and why there is no per-cheat apply button here.
    private var cheatsBlock: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("CHEATS")
                .font(.system(size: 11, weight: .bold))
                .tracking(1.6)
                .foregroundStyle(ShellPalette.secondaryText)

            let list = cheats.cheats(for: entry)
            if !list.isEmpty {
                SettingsReadout(
                    label: "In the list",
                    value: "\(list.count) code(s), \(list.filter { $0.enabled }.count) enabled"
                )
                ForEach(list) { cheat in
                    cheatRow(cheat)
                }
            }

            cheatEntryField

            SettingsNote(
                "Codes are stored exactly as typed, because every system has its own convention: "
                + "Game Genie letters on the NES and SNES, Action Replay pairs on the Game Boy "
                + "Advance, plain address and value on the Mega Drive, and a core will often take "
                + "more than one of them. Checking the shape here would reject codes that work, so "
                + "the core is left to accept or ignore what it is given. A code that does nothing "
                + "is usually one meant for a different region of the same game."
            )
            SettingsNote(
                "The whole list is handed to the core each time it changes, in this order, and it "
                + "is handed over again every time the game launches, because a core's cheat table "
                + "lives only as long as the session does. Turning one off leaves it in the list "
                + "and in the core's table, switched off, which is why the order here never shifts "
                + "under you."
            )
        }
        .padding(14)
        .background(ShellPalette.surface, in: RoundedRectangle(cornerRadius: 12))
    }

    private func cheatRow(_ cheat: Cheat) -> some View {
        HStack(alignment: .center, spacing: 10) {
            VStack(alignment: .leading, spacing: 2) {
                Text(cheat.label.isEmpty ? "Unnamed cheat" : cheat.label)
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(cheat.enabled ? Color.white : ShellPalette.secondaryText)
                Text(cheat.code)
                    .font(.system(.caption2, design: .monospaced))
                    .foregroundStyle(ShellPalette.secondaryText)
                    .lineLimit(2)
            }
            Spacer(minLength: 6)

            // A plain toggle, with no label of its own: the name is the row. Tinted the accent
            // colour like every other switch in the app so that "on" means the same thing here as
            // it does in Settings.
            Toggle("", isOn: Binding(
                get: { cheat.enabled },
                set: { cheats.setEnabled($0, for: cheat) }
            ))
            .labelsHidden()
            .tint(ShellPalette.accent)

            Button {
                cheats.delete(cheat)
            } label: {
                Image(systemName: "trash")
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(ShellPalette.accent)
                    .frame(width: 44, height: 44)
                    .background(ShellPalette.surfaceStrong, in: RoundedRectangle(cornerRadius: 9))
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Delete this cheat")
        }
    }

    /// The code field, the optional name, and Add.
    ///
    /// Autocorrection and autocapitalisation are the two things that have to be got right here, and
    /// they are not a nicety: a Game Genie code is a run of letters that no dictionary has heard of,
    /// so autocorrection rewrites it into a word and the user cannot see why their cheat does
    /// nothing. Capitals are forced because codes are conventionally written in them and the
    /// duplicate check is case-insensitive either way.
    private var cheatEntryField: some View {
        VStack(alignment: .leading, spacing: 8) {
            TextField("Code, for example SXIOPO", text: $draftCode)
                .font(.system(.footnote, design: .monospaced))
                .foregroundStyle(.white)
                .autocorrectionDisabled(true)
                .textInputAutocapitalization(.characters)
                .padding(10)
                .background(ShellPalette.surfaceStrong, in: RoundedRectangle(cornerRadius: 9))

            TextField("What it does, optional", text: $draftLabel)
                .font(.system(size: 13))
                .foregroundStyle(.white)
                .padding(10)
                .background(ShellPalette.surfaceStrong, in: RoundedRectangle(cornerRadius: 9))

            SettingsButton(title: "Add this cheat", role: .normal) {
                let problem = cheats.add(code: draftCode, label: draftLabel,
                                         forGameId: SaveStates.gameId(for: entry))
                cheatProblem = problem ?? ""
                if problem == nil {
                    draftCode = ""
                    draftLabel = ""
                }
            }

            if !cheatProblem.isEmpty {
                Text(cheatProblem)
                    .font(.system(size: 12))
                    .foregroundStyle(ShellPalette.accent)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    // MARK: Artwork

    /// The artwork section, and the ONLY place in the app that enumerates every cover a game has.
    ///
    /// Nothing in the library, the hero, the shelves, the grid or the list rows says that a game has
    /// alternatives, because automatic resolution takes the first cover that answers and says nothing
    /// about the rest. That silence is the requirement, not an oversight: a badge reading "3 covers"
    /// on every card would be noise nobody asked for. Someone who opens a card and goes looking finds
    /// the whole set here, and the enumeration happens then, on that tap, and never during browsing.
    private var artworkBlock: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("COVER ART")
                .font(.system(size: 11, weight: .bold))
                .tracking(1.6)
                .foregroundStyle(ShellPalette.secondaryText)

            SettingsReadout(
                label: "Source",
                value: artwork.provenance(for: entry)
                    ?? (artwork.fetchEnabled
                        ? "no cover stored yet, so the generated plate is showing"
                        : "artwork lookups are off, so the generated plate is showing")
            )

            coverChoices

            SettingsButton(title: "Choose an image from Files", role: .normal) {
                artwork.presentArtworkPicker(for: entry)
            }
            capturedCoverControl
            if artwork.artworkChoice(for: entry) != nil {
                SettingsButton(title: "Use automatic artwork again", role: .normal) {
                    chooser.useAutomatic(entry: entry, store: artwork)
                }
            }
            SettingsButton(title: "Look it up again", role: .normal) {
                artwork.lookUpAgain(entry)
            }
            SettingsButton(title: "Remove the stored cover", role: .destructive) {
                artwork.clearCover(for: entry)
            }
            SettingsNote(
                "A cover chosen here wins over anything found automatically, including after a "
                + "relaunch, and an image from Files or a frame captured from the game wins over "
                + "everything, which is the answer for a game the thumbnail database has never heard "
                + "of. The generated plate is always underneath, so nothing is ever blank."
            )
        }
        .padding(14)
        .background(ShellPalette.surface, in: RoundedRectangle(cornerRadius: 12))
        // ONCE, and only on open. `task(id:)` keyed on the entry so a sheet reused for another game
        // loads that game, and `load` itself refuses to run twice.
        .task(id: entry.id) {
            chooser.load(entry: entry, system: system, store: artwork)
        }
        .onDisappear {
            chooser.stop()
        }
    }

    /// The row of covers on offer, which reads sensibly with one, with none, and with nine.
    /// Which offer the card is showing, asked of the store rather than of the model, so the mark is
    /// right even after an image was picked from Files or the cache was cleared from Settings.
    private var inUseOptionID: String? {
        artwork.inUseOptionID(for: entry, among: chooser.options)
    }

    @ViewBuilder
    private var coverChoices: some View {
        if !chooser.options.isEmpty {
            ScrollView(.horizontal, showsIndicators: false) {
                // Lazy for the same reason the shelves are: a game with several borrowed covers should
                // build the cells it can show, not all of them at once.
                LazyHStack(alignment: .top, spacing: 10) {
                    ForEach(chooser.options) { option in
                        ArtworkOptionCell(
                            entry: entry,
                            option: option,
                            image: chooser.thumbnails[option.id],
                            isInUse: inUseOptionID == option.id
                        ) {
                            chooser.choose(option, entry: entry, store: artwork)
                        }
                    }
                }
                .padding(.vertical, 2)
            }
        }

        if !chooser.line.isEmpty {
            Text(chooser.line)
                .font(.system(size: 12))
                .foregroundStyle(ShellPalette.secondaryText)
                .fixedSize(horizontal: false, vertical: true)
        }

        if chooser.isWorking {
            HStack(spacing: 8) {
                ProgressView()
                    .controlSize(.small)
                    .tint(ShellPalette.accent)
                Text("Looking through the server's own cover lists. Any this device does not "
                     + "already have is downloaded once and kept for a month.")
                    .font(.system(size: 12))
                    .foregroundStyle(ShellPalette.secondaryText)
            }
        }

        if !chooser.searchedOtherSystems, SystemArtwork.hasThumbnails(for: system) {
            SettingsButton(title: "Look in other systems for more covers", role: .normal) {
                chooser.searchOtherSystems(entry: entry, system: system, store: artwork)
            }
            SettingsNote(
                "The same cartridge is often the same game on four other consoles, and libretro does "
                + "not have a scanned cover for all of them. This looks in the other systems' cover "
                + "lists, smallest first, and downloads the ones this device does not already have: "
                + "all nine box art lists together are about 11 MB if none of them is here yet. A "
                + "cover found that way says which system it came from."
            )
        }
    }

    /// The cover taken from the game itself, or the reason it is not on offer.
    ///
    /// A CONTROL THAT CANNOT WORK IS NOT OFFERED, which is the rule the Settings screen is built
    /// around and it applies with force here: capturing reads the live Metal surface, so it needs a
    /// session, and a button that answered "no game is running" every time it was pressed would
    /// teach the user that this card lies. The note is not a consolation prize either, it is the
    /// actual instruction, because the place the capture lives is not obvious: it is behind a press
    /// on the player's save button.
    ///
    /// AS THINGS STAND THE NOTE IS WHAT EVERY USER SEES, and that is worth writing down rather than
    /// discovering later. This sheet is presented by the library shell, and `RootView` mounts the
    /// library shell only while `activeEntry` is nil, so a running game means the player screen is on
    /// screen and this card is not. The condition below is therefore false today for exactly the same
    /// reason `stateRow` has a "Play" variant instead of a "Load" one. It is written as a condition
    /// anyway, not hardcoded, because the day this card becomes reachable over a running game the
    /// right control appears with nothing to change here, and the wrong one would not.
    ///
    /// The button is a plain `SettingsButton` like the four artwork actions around it rather than the
    /// 44 point row the save states use: it belongs to that block visually, and the control whose
    /// mis-tap would actually cost something, the one operated with a game running and a thumb in the
    /// way, is the player's menu row, which the system sizes for a touch.
    @ViewBuilder
    private var capturedCoverControl: some View {
        if host.running, host.activeEntry?.id == entry.id {
            SettingsButton(title: "Use the current frame as the cover", role: .normal) {
                // No dismissal afterwards, deliberately. The store bumps its generation when the
                // cover lands, this sheet observes the store, so the header above redraws with the
                // new cover and the Source line above says where it came from. Closing the card would
                // hide the one piece of feedback that proves it worked.
                artwork.captureCover(for: entry)
            }
        } else {
            SettingsNote(
                "A cover can also be taken from the game itself, which is the only thing that works "
                + "for a ROM no cover database has ever heard of. It needs the game to be running: "
                + "start it, then press and hold the save button at the top of the player and choose "
                + "\"Use this frame as the cover\". Pausing first is worth doing, because the frame "
                + "you are looking at while paused is exactly the frame that gets stored."
            )
        }
    }

    // MARK: Delete

    private var dangerZone: some View {
        VStack(alignment: .leading, spacing: 10) {
            SettingsButton(title: "Delete this game", role: .destructive) {
                // Routed through the one delete implementation, by resolving this entry's id back
                // to its index in the live library array rather than passing an index the sheet
                // made up.
                let indices = host.indices(matching: [entry.id])
                host.detailEntry = nil
                host.deleteEntries(at: indices)
            }
            SettingsNote(
                "Deletes the file from the Continuum folder. If this is a cue sheet, the .bin "
                + "tracks it named stay on disk: working out which tracks belonged to it means "
                + "parsing the sheet, and a wrong guess deletes a track another game needs."
            )
        }
        .padding(14)
        .background(ShellPalette.surface, in: RoundedRectangle(cornerRadius: 12))
    }
}
