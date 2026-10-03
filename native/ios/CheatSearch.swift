// Continuum - RAM search, and the in-game cheat sheet it lives on.
//
// THE SEARCH IS THE ENGINE'S. Every snapshot, every filter and every count is `ramSearch*` on the
// engine (`cheats::search` in Rust, tested there); this screen only chooses the next filter and
// draws the answer. A search result becomes a cheat through `CheatStore.addPoke`, which stores it
// in the same per-game list as typed codes (as a `poke:` code the engine writes after every frame),
// so it is named, toggled, deleted and counted against the 128 cap like any other cheat.
//
// WHY A SHEET ON THE PLAYER. A RAM search needs the game running, between filters the player has
// to go back and play, and the game's card is only reachable from the library, with no game
// running. So the cheat list, the `.cht` import and the search are all on one sheet opened from the
// player's menu, and the card's cheat section says where to find the search.

import SwiftUI

// MARK: - The in-game cheat sheet

struct PlayerCheatsSheet: View {
    let entry: LibraryEntry
    @ObservedObject var host: EngineHost
    @ObservedObject var cheats: CheatStore
    var onDone: () -> Void

    @State private var importLine = ""

    private var gameId: String { SaveStates.gameId(for: entry) }

    var body: some View {
        NavigationView {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    listBlock
                    CheatSearchView(entry: entry, host: host, cheats: cheats)
                }
                .padding(14)
            }
            .background(Color.black.ignoresSafeArea())
            .navigationTitle("Cheats")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { onDone() }
                }
            }
        }
        .navigationViewStyle(.stack)
        .preferredColorScheme(.dark)
    }

    private var listBlock: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("THIS GAME'S CHEATS")
                .font(.system(size: 11, weight: .bold))
                .tracking(1.6)
                .foregroundStyle(ShellPalette.secondaryText)
            let list = cheats.cheats(forGameId: gameId)
            if list.isEmpty {
                SettingsNote("No cheats yet. Import a RetroArch .cht file, type one on the game's "
                             + "card, or find one with the RAM search below.")
            }
            ForEach(list) { cheat in
                CheatRow(cheat: cheat, cheats: cheats)
            }
            ChtImportButton(gameId: gameId, cheats: cheats, line: $importLine)
        }
        .padding(14)
        .background(ShellPalette.surface, in: RoundedRectangle(cornerRadius: 12))
    }
}

/// One cheat with its switch and delete. Shared by the player's sheet and nothing else; the card
/// keeps its own row.
struct CheatRow: View {
    let cheat: Cheat
    @ObservedObject var cheats: CheatStore

    var body: some View {
        HStack(spacing: 10) {
            VStack(alignment: .leading, spacing: 2) {
                Text(cheat.label.isEmpty ? "Unnamed cheat" : cheat.label)
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(cheat.enabled ? Color.white : ShellPalette.secondaryText)
                Text(CheatStore.pokeSummary(for: cheat.code) ?? cheat.code)
                    .font(.system(.caption2, design: .monospaced))
                    .foregroundStyle(ShellPalette.secondaryText)
                    .lineLimit(2)
            }
            Spacer(minLength: 6)
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
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Delete this cheat")
        }
    }
}

/// "Import a .cht file", and the line that says what the import did.
struct ChtImportButton: View {
    let gameId: String
    @ObservedObject var cheats: CheatStore
    @Binding var line: String

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            SettingsButton(title: "Import a RetroArch .cht file", role: .normal) {
                FilePicker.shared.present(onPick: { data, name in
                    line = cheats.importChtFile(data, named: name, forGameId: gameId)
                }, onFailure: { reason in
                    line = "cheat file import failed: \(reason)"
                })
            }
            if !line.isEmpty {
                Text(line)
                    .font(.system(size: 12))
                    .foregroundStyle(ShellPalette.secondaryText)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}

// MARK: - RAM search

/// Classic RAM search over the running game's memory: system RAM, or any writable region the core
/// published in its memory map (the GBA's IWRAM and EWRAM, for instance). The list of regions and
/// which addresses a hit carries both come from the engine (`ramSearchRegions`).
struct CheatSearchView: View {
    let entry: LibraryEntry
    @ObservedObject var host: EngineHost
    @ObservedObject var cheats: CheatStore

    @State private var width: RamSearchWidth = .bits8
    @State private var aligned = false
    @State private var valueText = ""
    @State private var count: UInt64?
    @State private var hits: [RamSearchHit] = []
    @State private var line = ""
    /// The hit being turned into a cheat, and the value it will pin.
    @State private var making: RamSearchHit?
    /// Memory the search can run over, from the engine, and the one chosen ("system" or "map:N").
    @State private var regions: [RamSearchRegion] = []
    @State private var regionKey = "system"
    @State private var pinText = ""
    @State private var pinLabel = ""

    /// Rows drawn. The count says how many there really are.
    private static let resultLimit: UInt32 = 200

    private var isRunningThisGame: Bool { host.running && host.activeEntry?.id == entry.id }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("RAM SEARCH")
                .font(.system(size: 11, weight: .bold))
                .tracking(1.6)
                .foregroundStyle(ShellPalette.secondaryText)

            if !isRunningThisGame {
                SettingsNote("The search reads the running game's memory, so start the game first.")
            } else {
                setup
                if count != nil {
                    filters
                    results
                }
                if !line.isEmpty {
                    Text(line)
                        .font(.system(size: 12))
                        .foregroundStyle(ShellPalette.secondaryText)
                        .fixedSize(horizontal: false, vertical: true)
                }
                SettingsNote(
                    "Start a search, go back to the game and change the thing you are after (lose "
                    + "a life, spend some money), then come back and filter: less than before, "
                    + "unchanged, equal to a number. A few rounds usually leave one address. "
                    + "\"Changed\" and \"unchanged\" compare with the moment the search started; the "
                    + "others compare with the last filter. Values are read little endian."
                )
            }
        }
        .padding(14)
        .background(ShellPalette.surface, in: RoundedRectangle(cornerRadius: 12))
        .onAppear(perform: refresh)
        .alert("Make a cheat from this address", isPresented: Binding(
            get: { making != nil }, set: { if !$0 { making = nil } }
        )) {
            TextField("Value to keep it at", text: $pinText)
                .keyboardType(.numberPad)
            TextField("What it does", text: $pinLabel)
            Button("Add cheat") { addCheat() }
            Button("Cancel", role: .cancel) { making = nil }
        } message: {
            if let hit = making {
                Text("Writes this value at \(Self.hex(hit.address)) after every frame.")
            }
        }
    }

    private var setup: some View {
        VStack(alignment: .leading, spacing: 8) {
            if regions.count > 1 {
                Picker("Memory", selection: $regionKey) {
                    ForEach(regions, id: \.key) { region in
                        Text(Self.describe(region)).tag(region.key)
                    }
                }
                .pickerStyle(.menu)
                .font(.system(size: 13))
            }
            Picker("Size", selection: $width) {
                Text("8-bit").tag(RamSearchWidth.bits8)
                Text("16-bit").tag(RamSearchWidth.bits16)
                Text("32-bit").tag(RamSearchWidth.bits32)
            }
            .pickerStyle(.segmented)
            Toggle("Aligned addresses only", isOn: $aligned)
                .font(.system(size: 13))
                .tint(ShellPalette.accent)
            SettingsButton(title: count == nil ? "Start a search" : "Start over", role: .normal) {
                do {
                    let key = regions.contains(where: { $0.key == regionKey }) ? regionKey : "system"
                    let total = try host.engine.ramSearchStartIn(region: key, width: width,
                                                                 aligned: aligned)
                    let name = regions.first(where: { $0.key == key }).map(Self.describe)
                        ?? "system RAM"
                    line = "search started over \(total) address(es) of \(name)"
                } catch {
                    line = "the search could not start: \(error)"
                }
                refresh()
            }
        }
    }

    private var filters: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                filterButton("Less", .lessThanPrevious)
                filterButton("Greater", .greaterThanPrevious)
                filterButton("Equal", .equalToPrevious)
                filterButton("Not equal", .notEqualToPrevious)
            }
            HStack(spacing: 8) {
                filterButton("Changed", .changed)
                filterButton("Unchanged", .unchanged)
            }
            HStack(spacing: 8) {
                TextField("Value", text: $valueText)
                    .keyboardType(.numberPad)
                    .font(.system(.footnote, design: .monospaced))
                    .padding(8)
                    .background(ShellPalette.surfaceStrong, in: RoundedRectangle(cornerRadius: 8))
                    .frame(maxWidth: 110)
                valueButton("=") { .equalTo(value: $0) }
                valueButton("\u{2260}") { .notEqualTo(value: $0) }
                valueButton(">") { .greaterThan(value: $0) }
                valueButton("<") { .lessThan(value: $0) }
            }
            HStack(spacing: 8) {
                valueButton("Up by") { .increasedBy(value: $0) }
                valueButton("Down by") { .decreasedBy(value: $0) }
            }
        }
    }

    private var results: some View {
        VStack(alignment: .leading, spacing: 6) {
            if let count {
                SettingsReadout(
                    label: "Candidates",
                    value: count > UInt64(hits.count)
                        ? "\(count), showing the first \(hits.count)"
                        : "\(count)"
                )
            }
            ForEach(hits, id: \.address) { hit in
                HStack {
                    Text(Self.hex(hit.address))
                        .font(.system(.caption, design: .monospaced))
                        .foregroundStyle(.white)
                    Spacer()
                    Text("now \(hit.current), was \(hit.previous)")
                        .font(.system(.caption2, design: .monospaced))
                        .foregroundStyle(ShellPalette.secondaryText)
                    Button("Make cheat") {
                        pinText = String(hit.current)
                        pinLabel = ""
                        making = hit
                    }
                    .font(.system(size: 12, weight: .semibold))
                    .buttonStyle(.bordered)
                }
            }
        }
    }

    private func filterButton(_ title: String, _ filter: RamSearchFilter) -> some View {
        Button(title) { apply(filter) }
            .font(.system(size: 12, weight: .semibold))
            .buttonStyle(.bordered)
            .tint(.white)
    }

    private func valueButton(_ title: String,
                             _ make: @escaping (UInt32) -> RamSearchFilter) -> some View {
        Button(title) {
            guard let value = UInt32(valueText.trimmingCharacters(in: .whitespaces)) else {
                line = "type a whole number in the value box first"
                return
            }
            apply(make(value))
        }
        .font(.system(size: 12, weight: .semibold))
        .buttonStyle(.bordered)
        .tint(.white)
    }

    private func apply(_ filter: RamSearchFilter) {
        do {
            let left = try host.engine.ramSearchFilter(filter: filter)
            line = left == 0
                ? "nothing matched, so the search is empty; start over"
                : "\(left) address(es) left"
        } catch {
            line = "that filter was not applied: \(error)"
        }
        refresh()
    }

    private func refresh() {
        regions = host.engine.ramSearchRegions()
        if let running = host.engine.ramSearchRegion() {
            regionKey = running
        } else if !regions.contains(where: { $0.key == regionKey }), let first = regions.first {
            regionKey = first.key
        }
        count = host.engine.ramSearchCount()
        hits = count == nil ? [] : host.engine.ramSearchResults(limit: Self.resultLimit)
        if let running = host.engine.ramSearchWidth() {
            width = running
        }
    }

    private func addCheat() {
        guard let hit = making else { return }
        making = nil
        let bytes: UInt8
        switch width {
        case .bits8: bytes = 1
        case .bits16: bytes = 2
        case .bits32: bytes = 4
        }
        guard let value = UInt32(pinText.trimmingCharacters(in: .whitespaces)) else {
            line = "the value has to be a whole number, so no cheat was added"
            return
        }
        let label = pinLabel.isEmpty ? "RAM \(Self.hex(hit.address))" : pinLabel
        // The engine makes the code, because only it knows whether the hit is a system RAM offset
        // or a console address from a mapped region (a `:bus` poke).
        let code: String
        do {
            code = try host.engine.ramSearchPokeCode(address: hit.address, value: value,
                                                     bytes: bytes)
        } catch {
            line = "no cheat was added: \(error)"
            return
        }
        if let problem = cheats.add(code: code, label: label,
                                    forGameId: SaveStates.gameId(for: entry)) {
            line = "no cheat was added: \(problem)"
        } else {
            line = "added \"\(label)\": \(Self.hex(hit.address)) stays at \(value)"
        }
    }

    /// "IWRAM at $03000000, 32 KB".
    static func describe(_ region: RamSearchRegion) -> String {
        let size = region.size >= 1024 ? "\(region.size / 1024) KB" : "\(region.size) bytes"
        if region.key == "system" {
            return "\(region.name), \(size)"
        }
        let start = String(region.start, radix: 16, uppercase: true)
        return "\(region.name) at $\(start), \(size)"
    }

    static func hex(_ address: UInt32) -> String {
        let digits = String(address, radix: 16, uppercase: true)
        return "$" + String(repeating: "0", count: max(0, 4 - digits.count)) + digits
    }
}
