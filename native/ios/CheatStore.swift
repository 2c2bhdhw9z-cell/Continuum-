// Continuum - cheats: the durable per-game list this app owns, and the indexed table the core
// keeps while a game is running.
//
// ## Where a cheat actually lives
//
// Two places, and keeping them straight is the whole design of this file:
//
//   - IN THE CORE, as a numbered table pushed through `retro_cheat_set`. It is created when the
//     session starts and dies with it, so it has to be pushed again on every launch. The engine
//     already re-pushes it after a reset and after a state load, which is why nothing in this file
//     hooks either of those: doing it here as well would push the same table twice.
//   - HERE, as the list the user edits, kept in Application Support so it survives a relaunch.
//
// The list is handed to the engine WHOLE and IN ORDER, never one cheat at a time. `retro_cheat_set`
// is indexed, so removing the second of five cheats renumbers the three after it; a core given an
// incremental update ends up with a table that does not match the list the user is looking at.
// `ContinuumEngine.applyCheats` takes the codes and their enabled flags as two parallel arrays for
// exactly this reason.
//
// ## Code formats are deliberately not validated
//
// Every system has its own convention: Game Genie letter codes on the NES and SNES, Action Replay
// pairs on the GBA, raw address:value on the Mega Drive, and cores accept several of them. A front
// end that enforced one pattern would reject codes that work. So a code is stored as typed, with
// runs of whitespace collapsed, and the core is left to accept or ignore it. What IS enforced is the
// shape of the list: no blank codes, no duplicates, and a cap so that a paste accident cannot write
// a megabyte of "cheats".
//
// ## Why this is not in SaveStates.swift
//
// They look similar, and they are stored the same way for the same reasons, but a cheat is a small
// piece of text the user wrote and a save state is an opaque blob a core wrote. Nothing about the
// compatibility problem that dominates SaveStates.swift applies here: a code that means nothing to
// the running core is ignored by it, which is a wasted line and not a corrupted machine.

import Foundation

// MARK: - One cheat

/// A single code, as the user typed it.
struct Cheat: Identifiable, Codable, Hashable {

    /// The game's filename, the same id `SaveStates` uses, for the same durability reason recorded
    /// on `SaveStateRecord.gameId`.
    let gameId: String

    /// Stable across edits, so a row keeps its identity when the one above it is deleted.
    let id: String

    /// What the user called it. May be empty: a code with no description is still a working cheat,
    /// and demanding a name would be the app inventing a requirement.
    var label: String

    /// Whitespace-normalised, never otherwise interpreted.
    var code: String

    var enabled: Bool

    init(gameId: String, id: String, label: String, code: String, enabled: Bool) {
        self.gameId = gameId
        self.id = id
        self.label = label
        self.code = code
        self.enabled = enabled
    }

    private enum CodingKeys: String, CodingKey {
        case gameId, id, label, code, enabled
    }

    /// Field by field, each falling back rather than throwing, for the reason spelled out on
    /// `SaveStateRecord.init(from:)`: one renamed key must not empty out every cheat list on the
    /// device. A cheat with no code is useless and is dropped by the loader, which counts it.
    init(from decoder: Decoder) throws {
        let box = try decoder.container(keyedBy: CodingKeys.self)
        gameId = try box.decodeIfPresent(String.self, forKey: .gameId) ?? ""
        code = try box.decodeIfPresent(String.self, forKey: .code) ?? ""
        label = try box.decodeIfPresent(String.self, forKey: .label) ?? ""
        // A cheat with no id gets one derived from its own code, so it is still addressable by the
        // UI. Deriving it from the code rather than from a counter keeps it stable across launches.
        let storedId = try box.decodeIfPresent(String.self, forKey: .id)
        id = storedId ?? "\(gameId)#\(ArtworkDisk.key(forPath: code))"
        // Defaults to ON. A cheat that came back from storage switched off, when the file did not
        // say so, would look like the app had quietly disabled it.
        enabled = try box.decodeIfPresent(Bool.self, forKey: .enabled) ?? true
    }
}

// MARK: - Where the bytes live

/// The cheat list's own file, beside the save states and outside Documents for the same reason:
/// this app owns the namespace, and a list that can be renamed underneath the app by anyone
/// browsing the Continuum folder in the Files app is not a list the app can trust.
enum CheatDisk {
    static func indexURL() -> URL? {
        guard let support = FileManager.default.urls(for: .applicationSupportDirectory,
                                                     in: .userDomainMask).first else {
            return nil
        }
        var directory = support.appendingPathComponent("Cheats", isDirectory: true)
        if !FileManager.default.fileExists(atPath: directory.path) {
            do {
                try FileManager.default.createDirectory(at: directory,
                                                        withIntermediateDirectories: true)
            } catch {
                return nil
            }
            // Backed up, like the save states and unlike the artwork: these are a few hundred bytes
            // the user typed by hand, and nothing can reproduce them.
            var resourceValues = URLResourceValues()
            resourceValues.isExcludedFromBackup = false
            try? directory.setResourceValues(resourceValues)
        }
        return directory.appendingPathComponent("index.json")
    }

    static func write(_ cheats: [Cheat]) -> String? {
        guard let url = indexURL() else {
            return "no Application Support directory, so the list could not be kept"
        }
        do {
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.sortedKeys]
            try encoder.encode(cheats).write(to: url, options: .atomic)
            return nil
        } catch {
            return "the list could not be written: \(error.localizedDescription)"
        }
    }

    static func read() -> (cheats: [Cheat], failure: String?) {
        guard let url = indexURL() else { return ([], "no Application Support directory") }
        guard let data = try? Data(contentsOf: url), !data.isEmpty else { return ([], nil) }
        do {
            return (try JSONDecoder().decode([Cheat].self, from: data), nil)
        } catch {
            return ([], "the stored cheats could not be read: \(error.localizedDescription)")
        }
    }
}

// MARK: - The store

/// Every cheat on the device, and the one path that pushes a game's list into a core.
@MainActor
final class CheatStore: ObservableObject {

    /// Plenty for any real game, low enough that a runaway paste is caught.
    private static let maxPerGame = 128
    private static let maxCodeLength = 2048
    private static let maxLabelLength = 120

    /// Every cheat, in the order they were added. Order is meaningful: it is the order the core is
    /// given them in, and therefore the index each one has in the core's own table.
    @Published private(set) var cheats: [Cheat] = []

    /// This store's read-out, for Settings. Never empty.
    @Published private(set) var line = "cheats: none stored"

    private let engine: ContinuumEngine
    private weak var host: EngineHost?

    init(engine: ContinuumEngine) {
        self.engine = engine
        let (stored, failure) = CheatDisk.read()
        // A cheat with no code cannot be pushed to anything, so it is the one thing dropped on the
        // way in. Counted rather than ignored.
        let usable = stored.filter { !$0.code.isEmpty && !$0.gameId.isEmpty }
        cheats = usable
        if let failure {
            line = "cheats: \(failure)"
        } else {
            let dropped = stored.count - usable.count
            line = "cheats: \(usable.count) stored for \(gameCount) game(s)"
            if dropped > 0 {
                line += ", \(dropped) unreadable entr(y/ies) skipped"
            }
        }
    }

    func attach(host: EngineHost) {
        self.host = host
    }

    // MARK: Reading

    var gameCount: Int { Set(cheats.map { $0.gameId }).count }

    func cheats(forGameId gameId: String) -> [Cheat] {
        cheats.filter { $0.gameId == gameId }
    }

    func cheats(for entry: LibraryEntry) -> [Cheat] {
        cheats(forGameId: SaveStates.gameId(for: entry))
    }

    func enabledCount(forGameId gameId: String) -> Int {
        cheats(forGameId: gameId).filter { $0.enabled }.count
    }

    /// Whether the running core takes cheats at all. Asked of the engine every time rather than
    /// cached, because it is a property of the resident core and the resident core changes.
    var supportedNow: Bool { engine.cheatsSupported() }

    /// How many the core says it currently has, which is the only honest answer to "did that
    /// work". It is the core's count, not this app's.
    var activeInCoreNow: Int { Int(engine.activeCheatCount()) }

    // MARK: Editing

    /// Adds a cheat to a game's list, and pushes the list if that game is running.
    ///
    /// Returns nil on success, or the sentence to show. A thrown error would be the wrong shape
    /// here: every failure in this function is a thing the user typed, which is a message next to
    /// the field and not an error condition.
    @discardableResult
    func add(code rawCode: String, label rawLabel: String, forGameId gameId: String) -> String? {
        let code = Self.normalise(rawCode)
        guard !code.isEmpty else { return "a cheat needs a code." }
        guard code.count <= Self.maxCodeLength else {
            return "that code is \(code.count) characters long, and \(Self.maxCodeLength) is the "
                + "limit."
        }
        let existing = cheats(forGameId: gameId)
        guard existing.count < Self.maxPerGame else {
            return "\(Self.maxPerGame) cheats for one game is the limit."
        }
        guard !existing.contains(where: { $0.code.lowercased() == code.lowercased() }) else {
            return "that code is already in the list."
        }

        let label = String(rawLabel.trimmingCharacters(in: .whitespacesAndNewlines)
            .prefix(Self.maxLabelLength))
        // The id includes the code's hash and the current list length, so it is stable for this
        // cheat and cannot collide with another in the same game.
        let cheat = Cheat(gameId: gameId,
                          id: "\(gameId)#\(ArtworkDisk.key(forPath: code))#\(existing.count)",
                          label: label,
                          code: code,
                          enabled: true)
        cheats.append(cheat)
        persist(describing: "added a cheat for \(gameId), \(existing.count + 1) in the list")
        pushIfRunning(gameId: gameId)
        return nil
    }

    func setEnabled(_ enabled: Bool, for cheat: Cheat) {
        guard let index = cheats.firstIndex(where: { $0.id == cheat.id }) else { return }
        guard cheats[index].enabled != enabled else { return }
        cheats[index].enabled = enabled
        persist(describing: "\(enabled ? "enabled" : "disabled") a cheat for \(cheat.gameId)")
        pushIfRunning(gameId: cheat.gameId)
    }

    func delete(_ cheat: Cheat) {
        cheats = cheats.filter { $0.id != cheat.id }
        persist(describing: "deleted a cheat for \(cheat.gameId)")
        pushIfRunning(gameId: cheat.gameId)
    }

    func deleteAll(forGameId gameId: String) {
        let count = cheats(forGameId: gameId).count
        guard count > 0 else { return }
        cheats = cheats.filter { $0.gameId != gameId }
        persist(describing: "deleted \(count) cheat(s) for \(gameId)")
        pushIfRunning(gameId: gameId)
    }

    /// Deletes every cheat for every game, and clears the core's table if one is resident.
    func deleteEverything() {
        let count = cheats.count
        cheats = []
        persist(describing: "deleted every stored cheat, \(count) in total")
        // The list is empty now, so pushing it is the same as clearing, but `clearCheats` is the
        // call that says so and it is the one the engine documents for this.
        if host?.running == true {
            do {
                try engine.clearCheats()
            } catch {
                report("cheats were deleted, but the running game kept them: \(error)")
            }
        }
    }

    // MARK: Pushing

    /// Pushes a game's list into the resident core. Called on launch, and after every edit to the
    /// running game's list.
    ///
    /// A core that does not support cheats is not an error and is not reported as one: it is a
    /// stated fact about that core, shown in Settings, and pushing to it anyway would produce a
    /// failure line for something the user did not do wrong.
    func push(for entry: LibraryEntry) {
        let gameId = SaveStates.gameId(for: entry)
        let list = cheats(forGameId: gameId)
        guard !list.isEmpty else { return }
        guard engine.cheatsSupported() else {
            report("\(list.count) cheat(s) stored for \(entry.name), but "
                   + "\(engine.currentCoreId() ?? "this core") does not take cheats")
            return
        }
        do {
            // The WHOLE list, in order, including the disabled ones. A core's table is indexed, so
            // leaving the disabled entries out would renumber everything after them; they are
            // pushed with their flag set to false instead, which is what the flags array is for.
            let applied = try engine.applyCheats(codes: list.map { $0.code },
                                                 enabled: list.map { $0.enabled })
            let on = list.filter { $0.enabled }.count
            report("pushed \(list.count) cheat(s) for \(entry.name), \(on) enabled, the core "
                   + "accepted \(applied)")
        } catch {
            report("cheats were not applied to \(entry.name): \(error)")
        }
    }

    private func pushIfRunning(gameId: String) {
        guard let entry = host?.activeEntry, host?.running == true,
              SaveStates.gameId(for: entry) == gameId else { return }
        push(for: entry)
    }

    // MARK: Read-outs

    /// What is stored and what the running core makes of it.
    var storageLine: String {
        var parts: [String] = []
        if cheats.isEmpty {
            parts.append("no cheats stored")
        } else {
            parts.append("\(cheats.count) cheat(s) for \(gameCount) game(s)")
            parts.append("\(cheats.filter { $0.enabled }.count) enabled")
        }
        if host?.running == true {
            parts.append(engine.cheatsSupported()
                         ? "\(activeInCoreNow) active in \(engine.currentCoreId() ?? "the core")"
                         : "\(engine.currentCoreId() ?? "the running core") takes no cheats")
        } else {
            parts.append("no core resident, so nothing is active")
        }
        return parts.joined(separator: " \u{00B7} ")
    }

    // MARK: Plumbing

    /// Collapses runs of whitespace so that "ABCD EFGH" and "ABCD  EFGH" are one cheat rather than
    /// two that look identical in a list.
    private static func normalise(_ code: String) -> String {
        code.split(whereSeparator: { $0.isWhitespace }).joined(separator: " ")
    }

    private func persist(describing text: String) {
        if let failure = CheatDisk.write(cheats) {
            report("\(text), but \(failure)")
        } else {
            report(text)
        }
    }

    private func report(_ text: String) {
        line = "cheats: \(text)"
        host?.status = line
    }
}
