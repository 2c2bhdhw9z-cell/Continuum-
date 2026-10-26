// Continuum - the Pokémon Box: move Pokémon between the player's Pokémon games (Red, Blue,
// Yellow, Gold, Silver, Crystal, Ruby, Sapphire, Emerald, FireRed, LeafGreen), keep them in a
// box of the app's own (50 boxes of 30), or trade two in one step.
//
// A small extra, not a main screen: it is only offered on a Pokémon game's card and in its pause
// menu. All the save reading and writing is the engine's (crates/emulator-bridge/src/pokebox),
// which journals every change so one cut off halfway is finished next time, copies each game's
// save to "<save>.before-box" before changing it, and shows what changes before a move between
// generations. A game's own save is only changed while the game is closed: from the pause menu,
// the game is closed first and opened again afterwards.

import SwiftUI

/// The Box being open, and the game it was opened for.
struct PokemonBoxRequest: Identifiable {
    let id = UUID()
    let entry: LibraryEntry?
}

/// Which library games are Pokémon games the Box knows. Read from each file's header once.
/// On the main actor, as the save-state store whose game ids it uses is.
@MainActor
enum PokemonGames {
    private static var known: [String: PokeGame] = [:]
    private static var unknown: Set<String> = []
    private static let systems: Set<GameSystem> = [.gb, .gbc, .gba]

    static func game(for entry: LibraryEntry) -> PokeGame? {
        if let hit = known[entry.path] { return hit }
        if unknown.contains(entry.path) { return nil }
        guard let system = CoreCatalog.system(for: entry), systems.contains(system),
              let game = pokeboxIdentify(romPath: entry.path) else {
            unknown.insert(entry.path)
            return nil
        }
        known[entry.path] = game
        return game
    }

    static func games(in library: [LibraryEntry]) -> [LibraryEntry] {
        library.filter { game(for: $0) != nil }
    }

    /// Application Support/PokemonBox, beside BatterySaves (the engine's journal keeps its paths
    /// from the folder above, so it survives the app's folder moving on an update).
    static var folder: String {
        let support = FileManager.default.urls(for: .applicationSupportDirectory,
                                               in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSTemporaryDirectory())
        let dir = support.appendingPathComponent("PokemonBox", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir.path
    }

    static func savePath(for entry: LibraryEntry) -> String {
        SaveStateDisk.batteryURL(gameId: SaveStates.gameId(for: entry))?.path ?? ""
    }

    static func title(_ entry: LibraryEntry) -> String {
        GameMetadata.displayTitle(for: entry)
    }
}

extension EngineHost {
    /// Opens the Box for `entry`. From a running game, the game is closed first (its battery save
    /// is written on the way out) and opened again when the Box closes.
    func openPokemonBox(for entry: LibraryEntry?, fromGame: Bool) {
        pokemonBoxChanged = []
        if fromGame, let entry {
            pokemonBoxReturn = entry
            leavePlayer()
        } else {
            pokemonBoxReturn = nil
            detailEntry = nil
        }
        // A beat later: the library (which presents the Box) comes back first, and the game card's
        // sheet has gone, so two sheets never try to be on screen at once.
        Task { @MainActor in
            try? await Task.sleep(nanoseconds: 600_000_000)
            self.pokemonBox = PokemonBoxRequest(entry: entry)
        }
    }

    /// The Box closed. A game it was opened from opens again: from its title screen when the Box
    /// changed its save (the auto-save from before was set aside), else where it was.
    func finishPokemonBox() {
        guard let entry = pokemonBoxReturn else { return }
        pokemonBoxReturn = nil
        let changed = pokemonBoxChanged.contains(entry.path)
        Task { @MainActor in
            try? await Task.sleep(nanoseconds: 300_000_000)
            self.launch(entry: entry, resumingAuto: !changed)
        }
    }
}

/// Where a chosen Pokémon is.
private enum BoxPlace: Equatable {
    /// A game's PC: which side (0, or 1 for the second game when trading), box and place.
    case game(side: Int, box: Int, slot: Int)
    case storage(box: Int, slot: Int)
    /// A place in the first game's team.
    case team(slot: Int)
}

/// One Pokémon found anywhere, for the List.
private struct Found: Identifiable {
    let id: String
    let mon: PokeMon
    let place: String
}

/// A change waiting for the player's yes, with what it will do.
private struct PendingChange: Identifiable {
    let id = UUID()
    let change: PokeChange
    let plan: PokePlan
    let games: [LibraryEntry]
}

struct PokemonBoxSheet: View {
    @ObservedObject var host: EngineHost
    let request: PokemonBoxRequest

    private enum Mode: String {
        case move, trade
    }

    /// The Box's tools, after G1R Deluxe's (the owner's pick: the box first, then these).
    private enum Tool: String, CaseIterable {
        case boxes = "Boxes"
        case team = "Team"
        case list = "List"
        case dashboard = "Dashboard"
        case arrange = "Arrange"

        var icon: String {
            switch self {
            case .boxes: return "shippingbox"
            case .team: return "person.3"
            case .list: return "list.bullet"
            case .dashboard: return "chart.bar"
            case .arrange: return "arrow.left.arrow.right"
            }
        }
    }

    @State private var tool = Tool.boxes
    @State private var team: [PokeMon] = []
    @State private var teamProblem = ""
    @State private var everyone: [Found] = []
    @State private var search = ""
    /// 0 Pokédex number, 1 level, 2 name.
    @State private var listOrder = 0

    @State private var mode = Mode.move
    @State private var games: [LibraryEntry] = []
    @State private var game: [LibraryEntry?] = [nil, nil]
    @State private var pc: [PokePc?] = [nil, nil]
    @State private var pcProblem: [String] = ["", ""]
    /// The box shown for each game; -1 until its save is read, which shows the box in use.
    @State private var gameBox: [Int] = [-1, -1]
    @State private var storage: [PokeBoxPage] = []
    @State private var storageBox = 0
    @State private var chosen: [BoxPlace] = []
    @State private var notice = ""
    @State private var pending: PendingChange?
    @State private var refusal: RefusalBox?

    var body: some View {
        NavigationView {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    SettingsNote("Move Pokémon between your Pokémon games, keep them here, or trade "
                                 + "two in one go. Only what you saved inside a game is here, so "
                                 + "save in the game first.")
                    Menu {
                        ForEach(Tool.allCases, id: \.self) { t in
                            Button { pick(t) } label: { Label(t.rawValue, systemImage: t.icon) }
                        }
                    } label: {
                        pickerLabel("Tools: \(tool.rawValue)")
                    }
                    if tool == .boxes {
                        Picker("", selection: $mode) {
                            Text("Move").tag(Mode.move)
                            Text("Trade").tag(Mode.trade)
                        }
                        .pickerStyle(.segmented)
                        .onChange(of: mode) { _ in chosen = [] }
                    }

                    if games.isEmpty && tool != .list && tool != .dashboard && tool != .arrange {
                        SettingsNote("No Pokémon games from Red to LeafGreen are in your library "
                                     + "(English or European cartridges).")
                    } else if tool == .team {
                        gamePane(side: 0, showBoxes: false)
                        teamPane
                        storagePane
                        detailCard
                    } else if tool == .list {
                        listPane
                    } else if tool == .dashboard {
                        dashboardPane
                    } else if tool == .arrange {
                        arrangePane
                        detailCard
                    } else {
                        gamePane(side: 0)
                        if mode == .move {
                            moveButtons
                            storagePane
                        } else {
                            gamePane(side: 1)
                            SettingsButton(title: "Trade these two...", role: .normal) { askTrade() }
                                .disabled(tradePlaces == nil)
                                .opacity(tradePlaces == nil ? 0.5 : 1)
                            SettingsNote("Pick one Pokémon in each game. Kadabra, Machoke, Graveler, "
                                         + "Haunter and the ones holding the right item evolve, as "
                                         + "in a real trade.")
                        }
                        detailCard
                    }
                    if !notice.isEmpty {
                        SettingsNote(notice)
                    }
                    SettingsNote("Before a game's save is changed it is copied beside itself, and a "
                                 + "change cut off halfway is finished the next time the Box opens. "
                                 + "Loading a save state made before a change brings the old Pokémon back.")
                }
                .padding(16)
            }
            // Its own view's alert: one view can only show one.
            .alert(item: $refusal) { r in
                Alert(title: Text(r.plan.title),
                      message: Text(r.plan.changes.joined(separator: "\n")),
                      dismissButton: .default(Text("OK")))
            }
            .background(ShellPalette.background.ignoresSafeArea())
            .navigationTitle("Pokémon Box")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { host.pokemonBox = nil }
                }
            }
        }
        .navigationViewStyle(.stack)
        .onAppear(perform: start)
        .alert(item: $pending) { p in
            Alert(title: Text(p.plan.title),
                  message: Text(p.plan.changes.joined(separator: "\n")),
                  primaryButton: .default(Text(mode == .trade ? "Trade" : "Move it")) { run(p) },
                  secondaryButton: .cancel())
        }
    }

    private struct RefusalBox: Identifiable {
        let id = UUID()
        let plan: PokePlan
    }

    // MARK: Loading

    private func start() {
        if let line = pokeboxRecover(dir: PokemonGames.folder) {
            notice = line
        }
        games = PokemonGames.games(in: host.library)
        let first = request.entry.flatMap { e in games.first { $0.path == e.path } } ?? games.first
        game[0] = first
        game[1] = games.first { $0.path != first?.path }
        loadGame(0)
        loadGame(1)
        loadStorage()
        loadTeam()
    }

    private func pick(_ t: Tool) {
        tool = t
        chosen = []
        if t == .list || t == .dashboard {
            gatherEveryone()
        }
    }

    private func loadTeam() {
        guard let entry = game[0] else {
            team = []
            return
        }
        do {
            team = try pokeboxReadParty(romPath: entry.path, savePath: PokemonGames.savePath(for: entry))
            teamProblem = ""
        } catch {
            team = []
            teamProblem = "\(error)"
        }
    }

    /// Every Pokémon in the Box and in every Pokémon game's PC, for the List and the Dashboard.
    private func gatherEveryone() {
        var out: [Found] = []
        for (b, page) in storage.enumerated() {
            for (i, m) in page.slots.enumerated() {
                if let m { out.append(Found(id: "box\(b)/\(i)", mon: m, place: "Pokémon Box, box \(b + 1)")) }
            }
        }
        for entry in games {
            let title = PokemonGames.title(entry)
            if let read = try? pokeboxReadGame(romPath: entry.path, savePath: PokemonGames.savePath(for: entry)) {
                for (b, page) in read.boxes.enumerated() {
                    for (i, m) in page.slots.enumerated() {
                        if let m { out.append(Found(id: "\(entry.path)#\(b)/\(i)", mon: m, place: "\(title), \(page.name)")) }
                    }
                }
            }
            if let party = try? pokeboxReadParty(romPath: entry.path, savePath: PokemonGames.savePath(for: entry)) {
                for (i, m) in party.enumerated() {
                    out.append(Found(id: "\(entry.path)#team/\(i)", mon: m, place: "\(title), team"))
                }
            }
        }
        everyone = out
    }

    private func loadGame(_ side: Int) {
        guard let entry = game[side] else {
            pc[side] = nil
            return
        }
        do {
            let read = try pokeboxReadGame(romPath: entry.path, savePath: PokemonGames.savePath(for: entry))
            pc[side] = read
            pcProblem[side] = ""
            if gameBox[side] < 0 || gameBox[side] >= read.boxes.count { gameBox[side] = Int(read.currentBox) }
        } catch {
            pc[side] = nil
            pcProblem[side] = "\(error)"
        }
    }

    private func loadStorage() {
        do {
            storage = try pokeboxReadStorage(dir: PokemonGames.folder)
        } catch {
            storage = []
            notice = "\(error)"
        }
    }

    // MARK: The panes

    private func gamePane(side: Int, showBoxes: Bool = true) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            SettingsLabel(side == 0 ? (mode == .trade ? "First game" : "Game") : "Second game")
            Menu {
                ForEach(games.filter { side == 0 || $0.path != game[0]?.path }, id: \.id) { entry in
                    Button(PokemonGames.title(entry)) {
                        game[side] = entry
                        if side == 0, game[1]?.path == entry.path {
                            game[1] = games.first { $0.path != entry.path }
                            gameBox[1] = -1
                            loadGame(1)
                        }
                        gameBox[side] = -1
                        chosen = []
                        loadGame(side)
                        if side == 0 { loadTeam() }
                    }
                }
            } label: {
                pickerLabel(game[side].map(PokemonGames.title) ?? "Pick a game")
            }
            if let read = pc[side] {
                Text("\(read.trainer) · ID \(String(format: "%05d", Int(read.trainerId)))")
                    .font(.system(size: 12))
                    .foregroundStyle(ShellPalette.secondaryText)
            }
            if let read = pc[side], showBoxes {
                let index = min(gameBox[side], max(read.boxes.count - 1, 0))
                if read.boxes.indices.contains(index) {
                    boxNavigator(name: read.boxes[index].name, index: index, count: read.boxes.count) { gameBox[side] = $0 }
                    grid(page: read.boxes[index], columns: read.generation == 3 ? 6 : 5) { slot in
                        .game(side: side, box: index, slot: slot)
                    }
                }
            } else if !pcProblem[side].isEmpty {
                SettingsNote(pcProblem[side])
            }
        }
        .padding(12)
        .modifier(LookBlockCard(radius: 12))
    }

    private var storagePane: some View {
        VStack(alignment: .leading, spacing: 8) {
            SettingsLabel("Pokémon Box")
            if storage.indices.contains(storageBox) {
                boxNavigator(name: storage[storageBox].name, index: storageBox, count: storage.count) { storageBox = $0 }
                grid(page: storage[storageBox], columns: 6) { slot in
                    .storage(box: storageBox, slot: slot)
                }
                let used = storage.reduce(0) { total, page in total + page.slots.filter { $0 != nil }.count }
                Text("\(used) of \(storage.count * 30) places used")
                    .font(.system(size: 11))
                    .foregroundStyle(ShellPalette.tertiaryText)
            }
        }
        .padding(12)
        .modifier(LookBlockCard(radius: 12))
    }

    private func pickerLabel(_ text: String) -> some View {
        HStack {
            Text(text)
                .font(.system(size: 14, weight: .semibold))
                .foregroundStyle(ShellPalette.text)
            Spacer(minLength: 0)
            Image(systemName: "chevron.up.chevron.down")
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(ShellPalette.secondaryText)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
        .background(ShellPalette.surfaceStrong, in: RoundedRectangle(cornerRadius: 10))
    }

    private func boxNavigator(name: String, index: Int, count: Int, set: @escaping (Int) -> Void) -> some View {
        HStack {
            Button { set((index + count - 1) % max(count, 1)) } label: {
                Image(systemName: "chevron.left").frame(width: 36, height: 30)
            }
            Spacer()
            Text(name)
                .font(.system(size: 14, weight: .bold))
                .foregroundStyle(ShellPalette.text)
            Spacer()
            Button { set((index + 1) % max(count, 1)) } label: {
                Image(systemName: "chevron.right").frame(width: 36, height: 30)
            }
        }
        .foregroundStyle(ShellPalette.accent)
    }

    private func grid(page: PokeBoxPage, columns: Int, place: @escaping (Int) -> BoxPlace) -> some View {
        let cells = Int(page.capacity)
        return LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 6), count: columns), spacing: 6) {
            ForEach(0..<cells, id: \.self) { slot in
                let mon: PokeMon? = slot < page.slots.count ? page.slots[slot] : nil
                cell(mon, selected: chosen.contains(place(slot)))
                    .onTapGesture {
                        if tool == .arrange, case .storage? = chosen.first, chosen.first != place(slot) {
                            arrangeTo(place(slot))
                            return
                        }
                        guard mon != nil else { return }
                        choose(place(slot))
                    }
            }
        }
    }

    private func cell(_ mon: PokeMon?, selected: Bool) -> some View {
        VStack(spacing: 2) {
            if let mon {
                Text(mon.shiny ? "★" : " ")
                    .font(.system(size: 8))
                    .foregroundStyle(ShellPalette.gold)
                Text(mon.egg ? "EGG" : mon.speciesName)
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(ShellPalette.text)
                    .lineLimit(1)
                    .minimumScaleFactor(0.6)
                Text(mon.readable && !mon.egg ? "Lv \(mon.level)" : " ")
                    .font(.system(size: 8))
                    .foregroundStyle(ShellPalette.secondaryText)
            }
        }
        .frame(maxWidth: .infinity, minHeight: 44)
        .background(RoundedRectangle(cornerRadius: 8)
            .fill(mon == nil ? ShellPalette.surface : ShellPalette.surfaceStrong))
        .overlay(RoundedRectangle(cornerRadius: 8)
            .strokeBorder(selected ? ShellPalette.accent : Color.clear, lineWidth: 2))
        .contentShape(Rectangle())
    }

    // MARK: Choosing and moving

    private func choose(_ place: BoxPlace) {
        if let i = chosen.firstIndex(of: place) {
            chosen.remove(at: i)
            return
        }
        if mode == .move || tool != .boxes {
            chosen = [place]
        } else {
            // One per game: a new pick in a game replaces that game's pick.
            chosen.removeAll { if case .game(let s, _, _) = $0, case .game(let t, _, _) = place { return s == t }; return false }
            chosen.append(place)
        }
    }

    private func pokemon(at place: BoxPlace) -> PokeMon? {
        switch place {
        case .game(let side, let box, let slot):
            guard let read = pc[side], read.boxes.indices.contains(box),
                  read.boxes[box].slots.indices.contains(slot) else { return nil }
            return read.boxes[box].slots[slot]
        case .storage(let box, let slot):
            guard storage.indices.contains(box), storage[box].slots.indices.contains(slot) else { return nil }
            return storage[box].slots[slot]
        case .team(let slot):
            return team.indices.contains(slot) ? team[slot] : nil
        }
    }

    @ViewBuilder
    private var moveButtons: some View {
        let target = game[0].map(PokemonGames.title) ?? "the game"
        HStack(spacing: 10) {
            SettingsButton(title: "↓ Into the Box", role: .normal) {
                guard case .game(_, let box, let slot)? = chosen.first, let entry = game[0] else { return }
                ask(.toStorage(romPath: entry.path, savePath: PokemonGames.savePath(for: entry),
                               fromBox: UInt32(box), slot: UInt32(slot), toBox: UInt32(storageBox)),
                    games: [entry])
            }
            .disabled(!chosenIsGame)
            .opacity(chosenIsGame ? 1 : 0.5)
            SettingsButton(title: "↑ Into \(target)", role: .normal) {
                guard case .storage(let box, let slot)? = chosen.first, let entry = game[0] else { return }
                ask(.toGame(romPath: entry.path, savePath: PokemonGames.savePath(for: entry),
                            fromBox: UInt32(box), slot: UInt32(slot), toBox: UInt32(gameBox[0])),
                    games: [entry])
            }
            .disabled(!chosenIsStorage)
            .opacity(chosenIsStorage ? 1 : 0.5)
        }
        SettingsNote("Pick a Pokémon, then send it down into the box shown below, or up into the "
                     + "game's box shown above.")
    }

    private var chosenIsGame: Bool {
        if case .game? = chosen.first { return true }
        return false
    }

    private var chosenIsStorage: Bool {
        if case .storage? = chosen.first { return true }
        return false
    }

    private var tradePlaces: (BoxPlace, BoxPlace)? {
        var a: BoxPlace?
        var b: BoxPlace?
        for p in chosen {
            if case .game(let side, _, _) = p {
                if side == 0 { a = p } else { b = p }
            }
        }
        guard let a, let b, game[0] != nil, game[1] != nil else { return nil }
        return (a, b)
    }

    private func askTrade() {
        guard let places = tradePlaces, let ga = game[0], let gb = game[1],
              case .game(_, let aBox, let aSlot) = places.0,
              case .game(_, let bBox, let bSlot) = places.1 else { return }
        ask(.trade(aRomPath: ga.path, aSavePath: PokemonGames.savePath(for: ga),
                   aBox: UInt32(aBox), aSlot: UInt32(aSlot),
                   bRomPath: gb.path, bSavePath: PokemonGames.savePath(for: gb),
                   bBox: UInt32(bBox), bSlot: UInt32(bSlot)),
            games: [ga, gb])
    }

    private func ask(_ change: PokeChange, games touched: [LibraryEntry]) {
        let plan = pokeboxPlan(dir: PokemonGames.folder, change: change)
        if plan.allowed {
            pending = PendingChange(change: change, plan: plan, games: touched)
        } else {
            refusal = RefusalBox(plan: plan)
        }
    }

    private func run(_ p: PendingChange) {
        // The games' auto-saves hold their old memory: set aside first, or the change waits.
        for entry in p.games where !host.saveStates.setAsideAutoSaveForPokemonBox(entry: entry) {
            notice = "\(PokemonGames.title(entry))'s auto-save can't be set aside because all its "
                + "save slots are full, so nothing moved. Delete a save slot of that game first."
            return
        }
        do {
            let done = try pokeboxApply(dir: PokemonGames.folder, change: p.change)
            host.pokemonBoxChanged.formUnion(p.games.map(\.path))
            FeedbackCenter.step("pokemon box: \(done.title)")
            notice = "Done. \(done.title)"
                + (host.pokemonBoxReturn != nil ? " The game starts again from its title screen when "
                   + "you close this: pick Continue." : "")
        } catch {
            notice = "\(error)"
        }
        chosen = []
        loadGame(0)
        loadGame(1)
        loadStorage()
        loadTeam()
        if tool == .list || tool == .dashboard { gatherEveryone() }
    }

    // MARK: The other tools

    @ViewBuilder
    private var teamPane: some View {
        VStack(alignment: .leading, spacing: 8) {
            SettingsLabel("Team")
            if team.isEmpty {
                SettingsNote(teamProblem.isEmpty ? "No team in this save yet." : teamProblem)
            }
            ForEach(Array(team.enumerated()), id: \.offset) { i, m in
                Button { choose(.team(slot: i)) } label: {
                    HStack {
                        Text("\(m.name)\(m.gender.isEmpty ? "" : " \(m.gender)")\(m.shiny ? " ★" : "")")
                            .font(.system(size: 14, weight: .semibold))
                            .foregroundStyle(ShellPalette.text)
                        Spacer()
                        Text(m.egg ? "Egg" : "Lv \(m.level)")
                            .font(.system(size: 12))
                            .foregroundStyle(ShellPalette.secondaryText)
                    }
                    .padding(10)
                    .background(RoundedRectangle(cornerRadius: 8).fill(ShellPalette.surfaceStrong))
                    .overlay(RoundedRectangle(cornerRadius: 8)
                        .strokeBorder(chosen.contains(.team(slot: i)) ? ShellPalette.accent : Color.clear, lineWidth: 2))
                }
                .buttonStyle(.plain)
            }
            SettingsButton(title: "↓ Into the Box, box \(storageBox + 1)", role: .normal) {
                guard case .team(let slot)? = chosen.first, let entry = game[0] else { return }
                ask(.partyToStorage(romPath: entry.path, savePath: PokemonGames.savePath(for: entry),
                                    slot: UInt32(slot), toBox: UInt32(storageBox)),
                    games: [entry])
            }
            .disabled(!chosenIsTeam)
            .opacity(chosenIsTeam ? 1 : 0.5)
            SettingsButton(title: "↑ Into the team", role: .normal) {
                guard case .storage(let box, let slot)? = chosen.first, let entry = game[0] else { return }
                ask(.storageToParty(romPath: entry.path, savePath: PokemonGames.savePath(for: entry),
                                    fromBox: UInt32(box), slot: UInt32(slot)),
                    games: [entry])
            }
            .disabled(!chosenIsStorage || team.count >= 6)
            .opacity(chosenIsStorage && team.count < 6 ? 1 : 0.5)
            SettingsNote("Pick a team Pokémon to keep it in the Box (the last one stays: a team can't be "
                         + "empty), or a Box Pokémon below to put it in the team, with its stats worked "
                         + "out and its health full.")
        }
        .padding(12)
        .modifier(LookBlockCard(radius: 12))
    }

    private var chosenIsTeam: Bool {
        if case .team? = chosen.first { return true }
        return false
    }

    private var listed: [Found] {
        let q = search.trimmingCharacters(in: .whitespaces).lowercased()
        let hits = everyone.filter { f in
            q.isEmpty || f.mon.name.lowercased().contains(q) || f.mon.speciesName.lowercased().contains(q)
                || f.place.lowercased().contains(q) || f.mon.trainer.lowercased().contains(q)
        }
        switch listOrder {
        case 1: return hits.sorted { ($0.mon.level, $1.mon.species) > ($1.mon.level, $0.mon.species) }
        case 2: return hits.sorted { $0.mon.name < $1.mon.name }
        default: return hits.sorted { ($0.mon.species, $1.mon.level) < ($1.mon.species, $0.mon.level) }
        }
    }

    @ViewBuilder
    private var listPane: some View {
        VStack(alignment: .leading, spacing: 8) {
            TextField("Search Pokémon, games or trainers", text: $search)
                .textInputAutocapitalization(.never)
                .disableAutocorrection(true)
                .padding(10)
                .background(ShellPalette.surfaceStrong, in: RoundedRectangle(cornerRadius: 10))
            Picker("", selection: $listOrder) {
                Text("Number").tag(0)
                Text("Level").tag(1)
                Text("Name").tag(2)
            }
            .pickerStyle(.segmented)
            let rows = listed
            Text("\(rows.count) Pokémon")
                .font(.system(size: 11))
                .foregroundStyle(ShellPalette.tertiaryText)
            LazyVStack(alignment: .leading, spacing: 6) {
                ForEach(rows) { f in
                    VStack(alignment: .leading, spacing: 2) {
                        Text("\(f.mon.egg ? "Egg" : f.mon.name)\(f.mon.gender.isEmpty ? "" : " \(f.mon.gender)")\(f.mon.shiny ? " ★" : "")"
                             + (f.mon.egg || !f.mon.readable ? "" : "  Lv \(f.mon.level)"))
                            .font(.system(size: 14, weight: .semibold))
                            .foregroundStyle(ShellPalette.text)
                        Text((f.mon.name != f.mon.speciesName && !f.mon.egg ? "\(f.mon.speciesName) · " : "") + f.place)
                            .font(.system(size: 11))
                            .foregroundStyle(ShellPalette.secondaryText)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(8)
                    .background(RoundedRectangle(cornerRadius: 8).fill(ShellPalette.surfaceStrong))
                }
            }
        }
        .padding(12)
        .modifier(LookBlockCard(radius: 12))
    }

    @ViewBuilder
    private var dashboardPane: some View {
        let real = everyone.filter { $0.mon.readable && !$0.mon.egg }
        let species = Set(real.map(\.mon.species))
        VStack(alignment: .leading, spacing: 10) {
            SettingsLabel("Everything you have")
            dashRow("Pokémon", "\(real.count)")
            dashRow("Different kinds", "\(species.count) of 386")
            dashRow("Of the first 151", "\(species.filter { $0 <= 151 }.count) of 151")
            dashRow("Of the first 251", "\(species.filter { $0 <= 251 }.count) of 251")
            dashRow("Shiny", "\(real.filter(\.mon.shiny).count)")
            dashRow("Eggs", "\(everyone.filter(\.mon.egg).count)")
            dashRow("Highest level", real.map(\.mon.level).max().map { "\($0)" } ?? "-")
            SettingsLabel("Where they are")
            let inBox = everyone.filter { $0.id.hasPrefix("box") }.count
            dashRow("Pokémon Box", "\(inBox) of \(storage.count * 30)")
            ForEach(games, id: \.id) { entry in
                let n = everyone.filter { $0.id.hasPrefix(entry.path + "#") }.count
                dashRow(PokemonGames.title(entry), "\(n)")
            }
        }
        .padding(12)
        .modifier(LookBlockCard(radius: 12))
    }

    private func dashRow(_ label: String, _ value: String) -> some View {
        HStack {
            Text(label).font(.system(size: 14)).foregroundStyle(ShellPalette.text)
            Spacer()
            Text(value).font(.system(size: 14, weight: .semibold)).foregroundStyle(ShellPalette.accent)
        }
    }

    @ViewBuilder
    private var arrangePane: some View {
        storagePane
        HStack(spacing: 8) {
            SettingsButton(title: "By number", role: .normal) { sortBox(0) }
            SettingsButton(title: "By level", role: .normal) { sortBox(1) }
            SettingsButton(title: "By name", role: .normal) { sortBox(2) }
        }
        SettingsNote("Put this box in order, or tap a Pokémon and then the place it should go: it swaps "
                     + "with whatever is there. Change box with the arrows to move it to another box.")
    }

    private func sortBox(_ by: UInt8) {
        do {
            let done = try pokeboxApply(dir: PokemonGames.folder, change: .sort(boxIndex: UInt32(storageBox), by: by))
            notice = done.title
        } catch {
            notice = "\(error)"
        }
        chosen = []
        loadStorage()
    }

    private func arrangeTo(_ place: BoxPlace) {
        guard case .storage(let fb, let fs)? = chosen.first, case .storage(let tb, let ts) = place else { return }
        do {
            let done = try pokeboxApply(dir: PokemonGames.folder,
                                        change: .inStorageTo(fromBox: UInt32(fb), slot: UInt32(fs),
                                                             toBox: UInt32(tb), toSlot: UInt32(ts)))
            notice = done.title
        } catch {
            notice = "\(error)"
        }
        chosen = []
        loadStorage()
    }

    // MARK: The chosen one

    @ViewBuilder
    private var detailCard: some View {
        if let place = chosen.last, let mon = pokemon(at: place) {
            VStack(alignment: .leading, spacing: 4) {
                Text("\(mon.name)\(mon.gender.isEmpty ? "" : " \(mon.gender)")\(mon.shiny ? " ★" : "")")
                    .font(.system(size: 16, weight: .bold))
                    .foregroundStyle(ShellPalette.text)
                if mon.readable {
                    let kind = mon.egg ? "An Egg" : "\(mon.speciesName), level \(mon.level)"
                    SettingsNote("\(kind). Kept the way Pokémon \(mon.game) keeps it.")
                    SettingsNote("Trainer \(mon.trainer), ID \(String(format: "%05d", Int(mon.trainerId)))"
                                 + (mon.nature.isEmpty ? "" : ". \(mon.nature) nature")
                                 + (mon.item.isEmpty ? "" : ". Holding \(mon.item)") + ".")
                    if !mon.moves.isEmpty {
                        SettingsNote("Moves: " + mon.moves.joined(separator: ", "))
                    }
                    if mon.stats.count == 6 {
                        let st = mon.stats
                        SettingsNote("HP \(st[0]) · Attack \(st[1]) · Defense \(st[2]) · Speed \(st[3])"
                                     + (mon.generation == 1 ? " · Special \(st[4])"
                                        : " · Sp. Atk \(st[4]) · Sp. Def \(st[5])"))
                    }
                } else {
                    SettingsNote("Its data is damaged (a Bad Egg). It can only move between games of "
                                 + "the same generation.")
                }
            }
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .modifier(LookBlockCard(radius: 12))
        }
    }
}
