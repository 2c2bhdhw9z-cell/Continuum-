// Continuum - the skin library screens, the game card's skin choice, and the sheets the skin
// function buttons open in the player.
//
//   - SKINS in Settings: open the library, button sounds on or off.
//   - The library: import .manicskin/.deltaskin, every skin per system, make one the default,
//     rename, delete, or set a system back to the built-in pad.
//   - The game card: this game's own skin, or the system default.
//   - The player: switching skin mid-game (the `skins` function and the ... menu), the
//     all-functions menu (`flex`), haptics, Amiibo, achievements, save slots, cheats and the
//     layout editor, each opened by `SkinRuntime.shared` when a function asks for it.
//
// Every choice is stored by `EngineHost` (the skin library section of ContinuumApp.swift) and
// takes effect on the next frame, because the player reads its skin through the same resolver.

import SwiftUI
import UIKit

// MARK: - Names

@MainActor
enum SkinNames {
    /// A system id as words. Ids with no enum case on this build still read sensibly.
    static func system(_ id: String) -> String {
        GameSystem(rawValue: id)?.displayName ?? id.uppercased()
    }

    static func choice(_ choice: SkinChoice, host: EngineHost) -> String {
        switch choice {
        case .automatic: return "Automatic"
        case .none: return "Built-in pad"
        case .skin(let id): return host.skinRecord(id: id)?.name ?? "a deleted skin"
        }
    }

    static func detail(_ record: SkinRecord) -> String {
        let format = record.format == "manic" ? "Manic" : (record.format == "legacy" ? "Older import" : "Delta")
        var parts = [format, record.systems.map(system).joined(separator: ", ")]
        if record.hasSound { parts.append("button sound") }
        return parts.joined(separator: " \u{00B7} ")
    }
}

// MARK: - Settings

/// The SKINS card in Settings.
struct SkinLibrarySettingsSection: View {
    @ObservedObject var host: EngineHost
    @ObservedObject private var runtime = SkinRuntime.shared
    @State private var showLibrary = false

    var body: some View {
        SettingsSection(title: "SKINS") {
            let _ = host.touchSkinsVersion
            Text(host.allSkinRecords.isEmpty
                 ? "No skins yet. Import Manic EMU (.manicskin) or Delta (.deltaskin) skins."
                 : "\(host.allSkinRecords.count) skin(s) in the library.")
                .font(.system(size: 13))
                .foregroundStyle(ShellPalette.secondaryText)
            SettingsButton(title: "Open the skin library", role: .normal) {
                showLibrary = true
            }
            Toggle(isOn: $runtime.buttonSoundEnabled) {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Button sounds")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(.white)
                    Text("Plays a skin's own click (sound.caf) on every press, at the game's "
                         + "volume. Silent while the app is muted or the phone is on silent.")
                        .font(.system(size: 12))
                        .foregroundStyle(ShellPalette.secondaryText)
                }
            }
            .tint(ShellPalette.accent)
        }
        .sheet(isPresented: $showLibrary) {
            SkinLibrarySheet(host: host, onDone: { showLibrary = false })
        }
    }
}

// MARK: - The library

struct SkinLibrarySheet: View {
    @ObservedObject var host: EngineHost
    let onDone: () -> Void

    @State private var picker = DeltaSkinPicker()
    /// Nil means "the console the file names".
    @State private var importFor: GameSystem?
    @State private var message: String?
    @State private var renaming: SkinRecord?
    @State private var newName = ""
    @State private var deleting: SkinRecord?

    var body: some View {
        NavigationStack {
            List {
                Section {
                    Menu {
                        Button("The console the file names") { importFor = nil }
                        ForEach(GameSystem.allCases, id: \.rawValue) { system in
                            Button(system.displayName) { importFor = system }
                        }
                    } label: {
                        Label("Import for: \(importFor?.displayName ?? "the console the file names")",
                              systemImage: "gamecontroller")
                    }
                    Button {
                        importSkin()
                    } label: {
                        Label("Import skins (.manicskin, .deltaskin; pick as many as you like)",
                              systemImage: "square.and.arrow.down")
                    }
                    if let message {
                        Text(message)
                            .font(.system(size: 12))
                            .foregroundStyle(ShellPalette.secondaryText)
                    }
                } footer: {
                    Text("A skin for one system also fits its relatives: Game Boy and Game Boy "
                         + "Color; Mega Drive, Sega CD and 32X; Master System, Game Gear and "
                         + "SG-1000; NES and Famicom Disk System; DOS and DOOM.")
                }

                let _ = host.touchSkinsVersion
                ForEach(systemIDs, id: \.self) { systemID in
                    Section(SkinNames.system(systemID)) {
                        defaultRow(systemID)
                        ForEach(records(for: systemID)) { record in
                            row(record, systemID: systemID)
                        }
                    }
                }
            }
            .navigationTitle("Skins")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done", action: onDone)
                }
            }
            .alert("Rename skin", isPresented: Binding(
                get: { renaming != nil }, set: { if !$0 { renaming = nil } })) {
                TextField("Name", text: $newName)
                Button("Save") {
                    if let renaming { host.renameSkin(id: renaming.id, to: newName) }
                    renaming = nil
                }
                Button("Cancel", role: .cancel) { renaming = nil }
            }
            .confirmationDialog("Delete this skin?", isPresented: Binding(
                get: { deleting != nil }, set: { if !$0 { deleting = nil } }),
                titleVisibility: .visible) {
                Button("Delete \(deleting?.name ?? "skin")", role: .destructive) {
                    if let deleting { host.deleteSkin(id: deleting.id) }
                    deleting = nil
                }
            } message: {
                Text("Its pictures, sound and edits are removed. Games that used it go back to "
                     + "their system's default.")
            }
        }
        .preferredColorScheme(.dark)
    }

    /// Systems that have at least one skin made for them.
    private var systemIDs: [String] {
        var seen: [String] = []
        for record in host.allSkinRecords {
            for id in record.systems where !seen.contains(id) { seen.append(id) }
        }
        return seen.sorted { SkinNames.system($0) < SkinNames.system($1) }
    }

    private func records(for systemID: String) -> [SkinRecord] {
        host.allSkinRecords.filter { $0.systems.contains(systemID) }
    }

    private func defaultRow(_ systemID: String) -> some View {
        let choice = host.defaultSkinChoice(for: systemID)
        return Menu {
            Button("Automatic (newest skin)") { host.setDefaultSkin(.automatic, for: systemID) }
            Button("Built-in pad") { host.setDefaultSkin(.none, for: systemID) }
            ForEach(host.allSkinRecords.filter {
                SkinSharing.fits(skinSystems: $0.systems, on: systemID)
            }) { record in
                Button(record.name) { host.setDefaultSkin(.skin(record.id), for: systemID) }
            }
        } label: {
            HStack {
                Text("Default")
                Spacer()
                Text(SkinNames.choice(choice, host: host))
                    .foregroundStyle(ShellPalette.secondaryText)
            }
        }
    }

    private func row(_ record: SkinRecord, systemID: String) -> some View {
        let isDefault = host.defaultSkinChoice(for: systemID) == .skin(record.id)
        return HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(record.name)
                    .font(.system(size: 15, weight: .semibold))
                Text(SkinNames.detail(record))
                    .font(.system(size: 11))
                    .foregroundStyle(ShellPalette.secondaryText)
            }
            Spacer()
            if isDefault {
                Image(systemName: "checkmark.circle.fill")
                    .foregroundStyle(ShellPalette.accent)
            }
        }
        .contentShape(Rectangle())
        .contextMenu {
            Button("Make the \(SkinNames.system(systemID)) default") {
                host.setDefaultSkin(.skin(record.id), for: systemID)
            }
            Button("Rename") {
                newName = record.name
                renaming = record
            }
            Button("Delete", role: .destructive) { deleting = record }
        }
        .swipeActions {
            Button("Delete", role: .destructive) { deleting = record }
            Button("Rename") {
                newName = record.name
                renaming = record
            }
            .tint(.blue)
        }
        .onTapGesture {
            host.setDefaultSkin(.skin(record.id), for: systemID)
        }
    }

    /// Imports every skin picked in one go. "Import for" applies to all of them; with it on "the
    /// console the file names", each skin goes to its own console. One file that fails does not
    /// stop the others, and the message says which ones went in and which did not.
    private func importSkin() {
        picker.presentMany { outcomes in
            var done: [String] = []
            var problems: [String] = []
            for outcome in outcomes {
                switch outcome.result {
                case .success(let imported):
                    guard let system = importFor ?? imported.previewSystem else {
                        let named = imported.systemIDs.isEmpty
                            ? "does not say which console it is for"
                            : "is for \(imported.systemIDs.joined(separator: "/")), which this "
                                + "build cannot run yet"
                        problems.append("\(imported.skinName) \(named); pick a console under "
                                        + "Import for and import it again")
                        continue
                    }
                    host.applyImportedSkin(imported, for: system)
                    done.append("\(imported.skinName) for \(system.displayName)")
                case .failure(let error):
                    // Closing the picker is not a problem to report.
                    if case .cancelled = error { continue }
                    let reason = error.errorDescription ?? "the skin could not be imported"
                    problems.append(outcome.fileName.isEmpty ? reason
                                                             : "\(outcome.fileName): \(reason)")
                }
            }
            guard !done.isEmpty || !problems.isEmpty else { return }
            var text = done.isEmpty
                ? "No skin was imported."
                : "Imported \(done.count) skin(s): \(done.joined(separator: ", "))."
            if !problems.isEmpty {
                text += " Not imported: \(problems.joined(separator: "; "))."
            }
            message = text
            host.status = "skin import: " + text
        }
    }
}

// MARK: - One game's skin (the game card, and the player mid-game)

/// The choice list for one game: the system default, the built-in pad, or any skin that fits.
struct GameSkinChoices: View {
    @ObservedObject var host: EngineHost
    let entry: LibraryEntry
    let system: GameSystem

    var body: some View {
        let _ = host.touchSkinsVersion
        let current = host.gameSkinChoice(for: entry)
        Button {
            host.setGameSkin(.automatic, for: entry)
        } label: {
            choiceLabel("System default (\(SkinNames.choice(host.defaultSkinChoice(for: system.rawValue), host: host)))",
                        chosen: current == .automatic)
        }
        Button {
            host.setGameSkin(.none, for: entry)
        } label: {
            choiceLabel("Built-in pad", chosen: current == .none)
        }
        ForEach(host.skinRecords(for: system)) { record in
            Button {
                host.setGameSkin(.skin(record.id), for: entry)
            } label: {
                choiceLabel(record.name, chosen: current == .skin(record.id))
            }
        }
    }

    private func choiceLabel(_ title: String, chosen: Bool) -> some View {
        Label(title, systemImage: chosen ? "checkmark.circle.fill" : "circle")
    }
}

/// The SKIN block on a game's detail card.
struct SkinCardBlock: View {
    let entry: LibraryEntry
    @ObservedObject var host: EngineHost

    var body: some View {
        if let system = CoreCatalog.system(for: entry) {
            let _ = host.touchSkinsVersion
            VStack(alignment: .leading, spacing: 8) {
                Text("SKIN")
                    .font(.system(size: 11, weight: .bold))
                    .tracking(1.6)
                    .foregroundStyle(ShellPalette.secondaryText)
                Menu {
                    GameSkinChoices(host: host, entry: entry, system: system)
                } label: {
                    HStack {
                        Text(currentWords(system))
                            .font(.system(size: 15, weight: .semibold))
                            .foregroundStyle(.white)
                        Spacer()
                        Image(systemName: "chevron.up.chevron.down")
                            .foregroundStyle(ShellPalette.secondaryText)
                    }
                    .padding(12)
                    .background(ShellPalette.surface, in: RoundedRectangle(cornerRadius: 10))
                }
                Text("This game's own skin. The system default applies when this is on default.")
                    .font(.system(size: 11))
                    .foregroundStyle(ShellPalette.secondaryText)
            }
        }
    }

    private func currentWords(_ system: GameSystem) -> String {
        switch host.gameSkinChoice(for: entry) {
        case .automatic:
            return "System default: "
                + SkinNames.choice(host.defaultSkinChoice(for: system.rawValue), host: host)
        case let other:
            return SkinNames.choice(other, host: host)
        }
    }
}

// MARK: - The player's sheets

/// Every sheet and cover a skin function can open, attached once to the player screen.
struct SkinFunctionSheets: ViewModifier {
    @ObservedObject var host: EngineHost
    @ObservedObject private var runtime = SkinRuntime.shared

    func body(content: Content) -> some View {
        content
            .sheet(item: $runtime.sheet) { sheet in
                sheetBody(sheet)
            }
            .fullScreenCover(isPresented: $runtime.showLayoutEditor) {
                ControlEditorCover(host: host) { runtime.showLayoutEditor = false }
            }
    }

    @ViewBuilder
    private func sheetBody(_ sheet: SkinFunctionSheet) -> some View {
        let close = { runtime.sheet = nil }
        switch sheet {
        case .functions:
            SkinFunctionMenu(host: host, onDone: close)
        case .saveSlots:
            if let entry = host.activeEntry {
                SaveSlotsSheet(entry: entry, host: host, saveStates: host.saveStates, onDone: close)
            }
        case .cheats:
            if let entry = host.activeEntry {
                PlayerCheatsSheet(entry: entry, host: host, cheats: host.cheats, onDone: close)
            }
        case .skins:
            PlayerSkinSheet(host: host, onDone: close)
        case .haptics:
            HapticsQuickSheet(onDone: close)
        case .amiibo:
            SimpleSheet(title: "Amiibo", onDone: close) {
                Menu {
                    AmiiboMenuSection(peripherals: host.peripherals, report: { host.status = $0 })
                } label: {
                    Label("Choose an Amiibo to tap", systemImage: "wave.3.right")
                }
            }
        case .achievements:
            SimpleSheet(title: "Achievements", onDone: close) {
                if let entry = host.activeEntry {
                    AchievementsCardBlock(entry: entry, host: host, store: host.achievements)
                }
            }
        }
    }
}

/// A plain titled sheet with a Done button.
struct SimpleSheet<Content: View>: View {
    let title: String
    let onDone: () -> Void
    @ViewBuilder var content: Content

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) { content }
                    .padding(18)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .navigationTitle(title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button("Done", action: onDone) }
            }
        }
        .preferredColorScheme(.dark)
    }
}

/// Switching skin mid-game. The choice is this game's, so it is still there next time.
struct PlayerSkinSheet: View {
    @ObservedObject var host: EngineHost
    let onDone: () -> Void
    @State private var showLibrary = false

    var body: some View {
        NavigationStack {
            List {
                if let entry = host.activeEntry, let system = host.activeSystem {
                    Section {
                        GameSkinChoices(host: host, entry: entry, system: system)
                    } footer: {
                        Text("Kept for \(entry.name). Change the default for every "
                             + "\(system.displayName) game in the skin library.")
                    }
                } else {
                    Text("No game is running.")
                }
                Section {
                    Button("Open the skin library") { showLibrary = true }
                }
            }
            .navigationTitle("Skin")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button("Done", action: onDone) }
            }
            .sheet(isPresented: $showLibrary) {
                SkinLibrarySheet(host: host, onDone: { showLibrary = false })
            }
        }
        .preferredColorScheme(.dark)
    }
}

/// Manic's `flex` button: every function in one list. Held functions are offered in their
/// tap form (fast forward on/off), because a list row cannot be held.
struct SkinFunctionMenu: View {
    @ObservedObject var host: EngineHost
    let onDone: () -> Void

    var body: some View {
        NavigationStack {
            List {
                Section {
                    Button {
                        run { host.status = host.cycleFastForward() }
                    } label: {
                        Label("Cycle fast-forward speed", systemImage: "forward.fill")
                    }
                }
                Section("Functions") {
                    ForEach(SkinFunction.allCases.filter { !$0.isHold && $0 != .flex }, id: \.rawValue) { function in
                        Button {
                            run { host.performSkinFunction(function, pressed: true) }
                        } label: {
                            Text(function.title)
                        }
                    }
                }
            }
            .navigationTitle("Functions")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button("Done", action: onDone) }
            }
        }
        .preferredColorScheme(.dark)
    }

    /// Closes this sheet first, because several functions open a sheet of their own and only one
    /// can be up at a time.
    private func run(_ action: @escaping () -> Void) {
        onDone()
        Task { @MainActor in
            try? await Task.sleep(nanoseconds: 400_000_000)
            action()
        }
    }
}

/// The `haptics` function: button taps, rumble and turbo speed, without leaving the game.
struct HapticsQuickSheet: View {
    let onDone: () -> Void
    @ObservedObject private var feel = ControlFeel.shared

    var body: some View {
        NavigationStack {
            Form {
                Picker("Button taps", selection: $feel.buttonHaptics) {
                    ForEach(ButtonHapticStrength.allCases) { strength in
                        Text(strength.label).tag(strength)
                    }
                }
                Toggle("Game rumble", isOn: $feel.rumbleEnabled)
                Picker("Turbo speed", selection: $feel.turboRate) {
                    ForEach(TurboRate.allCases) { rate in
                        Text(rate.label).tag(rate)
                    }
                }
            }
            .navigationTitle("Haptics")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button("Done", action: onDone) }
            }
        }
        .preferredColorScheme(.dark)
    }
}

/// The on-screen layout editor, the same one Settings opens, for the `functionLayout` function.
struct ControlEditorCover: View {
    @ObservedObject var host: EngineHost
    let onClose: () -> Void

    var body: some View {
        TouchLayoutEditor(
            layoutFor: { system in host.touchLayout(for: system) },
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
            onClose: onClose
        )
    }
}
