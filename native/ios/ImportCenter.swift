// Every way a file gets into Continuum ends here, and then in `EngineHost.importFiles`.
//
// The Files picker, Wi-Fi transfer, the clipboard, drag and drop, Open in / Share to, WebDAV and
// SMB all produce plain file URLs, and every one of them is handed to the
// SAME import path, so a .cue with its .bin tracks, a zip of a romset or a .dsv save behave
// identically whichever way they arrived. What this file adds in front of that path:
//
//   * Kinds that are not games are routed to their own stores: .deltaskin / .manicskin to the
//     skin importer, .cht to the cheat list of the game it is named after, save files (.srm .sav
//     .dsv .mcr .eep ...) to that game's battery save through the Rust format converter, and PDF
//     manuals into Documents/Manuals.
//   * .zip and .7z are opened in Rust. Arcade, DOS and Amiga archives are copied whole (their cores
//     load the archive itself); anything else is unpacked flat, keeping filenames, so a cue and its
//     tracks land side by side.
//   * A file with a cartridge ROM extension that is plain text (README.md, LICENSE.md: .md is also
//     a Mega Drive extension) is skipped and counted, never copied, whether it came loose, out of
//     a zip or from a folder download. The engine decides (`importNotAGame`); see `NotGames`.
//   * After the copy, every file whose extension several systems share (.cue .chd .iso .bin ...)
//     is read by the Rust detector. When it is sure, `CoreCatalog.systemResolver` answers from it;
//     when it is not, the user is asked ONCE with a system picker and the answer is kept per
//     filename.

import Foundation
import SwiftUI
import UIKit
import UniformTypeIdentifiers

// MARK: - System names for the picker

/// The shared system ids (see the wave brief) and their names, for the "which system is this?"
/// picker and the status lines that name a system. The NAMES are kept here rather than read from
/// `GameSystem` because the detector reports ids nothing here runs (a GameCube disc header), and a
/// line about one should still say "GameCube". The picker itself offers only `pickable`.
struct SystemName: Hashable {
    let id: String
    let name: String
}

enum SystemNames {
    static let ordered: [SystemName] = [
        SystemName(id: "ps1", name: "PlayStation"), SystemName(id: "psp", name: "PlayStation Portable"), SystemName(id: "segacd", name: "Sega CD / Mega CD"),
        SystemName(id: "saturn", name: "Sega Saturn"), SystemName(id: "dreamcast", name: "Dreamcast"), SystemName(id: "pcecd", name: "PC Engine CD / TurboGrafx-CD"),
        SystemName(id: "genesis", name: "Mega Drive / Genesis"), SystemName(id: "sega32x", name: "Sega 32X"), SystemName(id: "sms", name: "Master System"),
        SystemName(id: "gg", name: "Game Gear"), SystemName(id: "sg1000", name: "SG-1000"), SystemName(id: "nes", name: "NES"), SystemName(id: "fds", name: "Famicom Disk System"),
        SystemName(id: "snes", name: "SNES"), SystemName(id: "gb", name: "Game Boy"), SystemName(id: "gbc", name: "Game Boy Color"), SystemName(id: "gba", name: "Game Boy Advance"),
        SystemName(id: "ds", name: "Nintendo DS"), SystemName(id: "n3ds", name: "Nintendo 3DS"), SystemName(id: "n64", name: "Nintendo 64"),
        SystemName(id: "gamecube", name: "GameCube"), SystemName(id: "wii", name: "Wii"), SystemName(id: "vb", name: "Virtual Boy"), SystemName(id: "pokemini", name: "Pokemon Mini"),
        SystemName(id: "tg16", name: "TurboGrafx-16 / PC Engine"), SystemName(id: "sgx", name: "SuperGrafx"), SystemName(id: "wswan", name: "WonderSwan"),
        SystemName(id: "ngp", name: "Neo Geo Pocket"), SystemName(id: "atari2600", name: "Atari 2600"), SystemName(id: "atari5200", name: "Atari 5200"),
        SystemName(id: "atari7800", name: "Atari 7800"), SystemName(id: "lynx", name: "Atari Lynx"), SystemName(id: "jaguar", name: "Atari Jaguar"),
        SystemName(id: "arcade", name: "Arcade"), SystemName(id: "dos", name: "DOS"), SystemName(id: "amiga", name: "Amiga"), SystemName(id: "c64", name: "Commodore 64"),
        SystemName(id: "doom", name: "Doom"), SystemName(id: "flash", name: "Flash"), SystemName(id: "j2me", name: "J2ME"), SystemName(id: "symbian", name: "Symbian"),
    ]

    /// Whether a game answered as this system can launch: the app has a `GameSystem` for it, which
    /// is what routing, the pad and the core choice all key on. GameCube and Wii are in
    /// `ordered` with no core behind them, and a user who picked one got a game that could not
    /// start. Read from `GameSystem` rather than listed, so a system that gains a core is offered
    /// the day its case lands and not after someone remembers this list.
    static func canRun(_ id: String) -> Bool {
        guard let system = GameSystem(rawValue: id) else { return false }
        // Arcade is offered when this build has cArcade; without it, arcade games list in the
        // Library and are never offered.
        return CoreCatalog.unplayableSystems[system] == nil
    }

    /// What the "which system?" picker offers: `ordered` minus the ids nothing here can run.
    static let pickable: [SystemName] = ordered.filter { canRun($0.id) }

    static func name(_ id: String) -> String {
        ordered.first { $0.id == id }?.name ?? id
    }
}

// MARK: - Remembered answers and the detection cache (any thread)

/// The user's answers to "which system is this file?", keyed by filename, in UserDefaults.
enum SystemChoices {
    private static let key = "import.systemChoices.v1"

    static func choice(forFileName name: String) -> String? {
        let all = UserDefaults.standard.dictionary(forKey: key) as? [String: String]
        return all?[name.lowercased()]
    }

    static func remember(_ system: String, forFileName name: String) {
        var all = (UserDefaults.standard.dictionary(forKey: key) as? [String: String]) ?? [:]
        all[name.lowercased()] = system
        UserDefaults.standard.set(all, forKey: key)
        DetectionCache.shared.forget(name)
    }

    static func forget(fileName name: String) {
        var all = (UserDefaults.standard.dictionary(forKey: key) as? [String: String]) ?? [:]
        all.removeValue(forKey: name.lowercased())
        UserDefaults.standard.set(all, forKey: key)
    }
}

/// Systems the app only GUESSED for a file, keyed by filename, kept apart from `SystemChoices`
/// (the player's own answers). Today that is an archive that looks like arcade chips and is filed
/// as Arcade without a question. The resolver reads a guess last, after the player's answer and
/// after a detector that is sure, so a later version (better detection, or a question of cArcade's
/// own) can decide these files again; the player's answers are never touched by that.
enum SystemGuesses {
    private static let key = "import.systemGuesses.v1"

    static func guess(forFileName name: String) -> String? {
        let all = UserDefaults.standard.dictionary(forKey: key) as? [String: String]
        return all?[name.lowercased()]
    }

    static func remember(_ system: String, forFileName name: String) {
        var all = (UserDefaults.standard.dictionary(forKey: key) as? [String: String]) ?? [:]
        all[name.lowercased()] = system
        UserDefaults.standard.set(all, forKey: key)
    }

    /// Drops the guess for a file that left the Library. Writes only when there was one.
    static func forget(fileName name: String) {
        var all = (UserDefaults.standard.dictionary(forKey: key) as? [String: String]) ?? [:]
        guard all.removeValue(forKey: name.lowercased()) != nil else { return }
        UserDefaults.standard.set(all, forKey: key)
    }
}

/// Detection results per path, invalidated by size and modification date. The resolver may be
/// asked about the same file once per Library row redraw, and a CHD costs a hunk decompress, so
/// the answer is kept. Lock-protected: the resolver can be called from any thread.
final class DetectionCache: @unchecked Sendable {
    static let shared = DetectionCache()
    private let lock = NSLock()
    private var entries: [String: (stamp: String, detection: SystemDetection)] = [:]
    /// The engine's "this is text, not a game" answer per path, by the same stamp, so a library
    /// rescan only reads files that are new or changed.
    private var notGameEntries: [String: (stamp: String, reason: String?)] = [:]
    /// Library files already checked and found to be games: file name to stamp, kept in Caches
    /// between launches, so a cold launch doesn't open every ROM again. Loaded on first use.
    private var checkedGames: [String: String]?
    private var checkedGamesChanged = false

    private static var checkedGamesFile: URL? {
        FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask).first?
            .appendingPathComponent("continuum-checked-games.json")
    }

    /// Call with the lock held.
    private func loadedCheckedGames() -> [String: String] {
        if let checkedGames { return checkedGames }
        let loaded = Self.checkedGamesFile
            .flatMap { try? Data(contentsOf: $0) }
            .flatMap { try? JSONDecoder().decode([String: String].self, from: $0) } ?? [:]
        checkedGames = loaded
        return loaded
    }

    private func stamp(_ url: URL) -> String {
        let values = try? url.resourceValues(forKeys: [.fileSizeKey, .contentModificationDateKey])
        let size = values?.fileSize ?? -1
        let date = values?.contentModificationDate?.timeIntervalSince1970 ?? 0
        return "\(size)@\(date)"
    }

    func detection(for url: URL) -> SystemDetection {
        let key = url.standardizedFileURL.path
        let now = stamp(url)
        lock.lock()
        if let hit = entries[key], hit.stamp == now {
            lock.unlock()
            return hit.detection
        }
        lock.unlock()
        let fresh = importDetectSystem(path: url.path)
        lock.lock()
        entries[key] = (now, fresh)
        lock.unlock()
        return fresh
    }

    /// Why the file is not a game (a cartridge ROM extension holding plain text), or nil.
    func notAGame(for url: URL) -> String? {
        let key = url.standardizedFileURL.path
        let name = url.lastPathComponent
        let now = stamp(url)
        lock.lock()
        if let hit = notGameEntries[key], hit.stamp == now {
            lock.unlock()
            return hit.reason
        }
        if loadedCheckedGames()[name] == now {
            notGameEntries[key] = (now, nil)
            lock.unlock()
            return nil
        }
        lock.unlock()
        let fresh = importNotAGame(path: url.path)
        lock.lock()
        notGameEntries[key] = (now, fresh)
        // Loaded by the check above.
        if fresh == nil {
            checkedGames?[name] = now
            checkedGamesChanged = true
        } else if checkedGames?.removeValue(forKey: name) != nil {
            checkedGamesChanged = true
        }
        lock.unlock()
        return fresh
    }

    /// Writes the checked games to Caches when they changed, keeping only these names (the
    /// Library's files), so a deleted game drops off.
    func saveCheckedGames(keeping names: Set<String>) {
        lock.lock()
        var games = loadedCheckedGames()
        let before = games.count
        games = games.filter { names.contains($0.key) }
        let changed = checkedGamesChanged || games.count != before
        checkedGames = games
        checkedGamesChanged = false
        lock.unlock()
        guard changed, let file = Self.checkedGamesFile,
              let data = try? JSONEncoder().encode(games) else { return }
        try? data.write(to: file, options: .atomic)
    }

    func forget(_ fileName: String) {
        lock.lock()
        let lowered = fileName.lowercased()
        entries = entries.filter { ($0.key as NSString).lastPathComponent.lowercased() != lowered }
        notGameEntries = notGameEntries.filter { ($0.key as NSString).lastPathComponent.lowercased() != lowered }
        if var games = checkedGames {
            let before = games.count
            games = games.filter { $0.key.lowercased() != lowered }
            if games.count != before {
                checkedGames = games
                checkedGamesChanged = true
            }
        }
        lock.unlock()
    }
}

// MARK: - Not games

/// Files with a ROM extension that are text, not games (the engine decides; see
/// `importNotAGame`). The import skips them, and the Library moves the ones already in it to
/// Documents/Not games, which Files shows as Continuum, Not games. Not `@MainActor`: the sync
/// worker asks `wasMoved` off the main thread, and UserDefaults is thread-safe.
enum NotGames {
    /// The Files folder the Library moves them into. The Library never scans folders.
    static let folderName = "Not games"
    /// The names moved there, lowercased. A `continuum.` key, so it travels in the settings
    /// export: a copy still in the cloud folder or a backup must not come back down.
    static let movedNamesKey = "continuum.library.notGames.v1"

    /// Documents/Not games.
    static func folder(in documents: URL) -> URL {
        documents.appendingPathComponent(folderName, isDirectory: true)
    }

    /// Why a file arriving in an import is not a game, or nil. Not cached: an import copy is a
    /// fresh scratch file every time.
    static func reason(for url: URL) -> String? {
        importNotAGame(path: url.path)
    }

    /// Every name the Library has moved to Not games, lowercased.
    static func movedNames() -> Set<String> {
        Set((UserDefaults.standard.stringArray(forKey: movedNamesKey) ?? []).map { $0.lowercased() })
    }

    /// Whether the Library moved a file with this name, so the sync must not bring it back.
    static func wasMoved(_ name: String) -> Bool {
        movedNames().contains(name.lowercased())
    }

    /// Records names the Library just moved.
    static func remember(_ names: [String]) {
        guard !names.isEmpty else { return }
        let all = movedNames().union(names.map { $0.lowercased() })
        UserDefaults.standard.set(all.sorted(), forKey: movedNamesKey)
    }

    /// Drops names that are real games in the Library again, so the sync carries them as usual.
    /// Writes only when something changed.
    static func forget(_ names: [String]) {
        let stored = movedNames()
        guard !stored.isEmpty else { return }
        let left = stored.subtracting(names.map { $0.lowercased() })
        guard left.count != stored.count else { return }
        UserDefaults.standard.set(left.sorted(), forKey: movedNamesKey)
    }

    /// Game files for consoles this build can't run (Switch, GameCube, Wii, Wii U, Vita, PS3,
    /// Xbox and a few computers). Never called "files that aren't games": an import with one
    /// keeps the full line, which names the file type, as it always did.
    static let gameTypesThisBuildCantRun: Set<String> = [
        "nsp", "nsz", "xci", "xcz", "nca", "nro", "nso", "gcm", "gcz", "rvz", "wbfs", "wia", "ciso",
        "tgc", "dol", "wud", "wux", "wua", "rpx", "vpk", "pkg", "xex", "xbe", "cia", "col", "int",
        "vec", "chf", "tzx", "z80", "dsk", "st", "msa", "atr",
    ]

    /// The tester's line after an import: the games that went in, then the files skipped, if any.
    static func importLine(added: Int, skipped: Int) -> String {
        let addedText: String
        switch added {
        case 0: addedText = "No games added."
        case 1: addedText = "Added 1 game."
        default: addedText = "Added \(added) games."
        }
        guard skipped > 0 else { return addedText }
        let skippedText = skipped == 1
            ? "Skipped 1 file that isn\u{2019}t a game."
            : "Skipped \(skipped) files that aren\u{2019}t games."
        return "\(addedText) \(skippedText)"
    }

    /// The tester's line after the Library moved files out.
    static func movedLine(_ count: Int) -> String {
        count == 1
            ? "Moved 1 file that isn\u{2019}t a game to the Not games folder in Files."
            : "Moved \(count) files that aren\u{2019}t games to the Not games folder in Files."
    }
}

// MARK: - One question to the user

struct SystemQuestion: Identifiable, Equatable {
    let id = UUID()
    let fileName: String
    let reason: String
    /// Most likely first; the picker shows these at the top and every other system below.
    let candidates: [String]
}

// MARK: - The import center

@MainActor
final class ImportCenter: ObservableObject {
    /// The question on screen, or nil.
    @Published var question: SystemQuestion?
    /// The last thing a non-picker import method did, for the Import screen.
    @Published var line = "nothing imported this session yet"

    let wifi = WiFiTransferServer()
    let remotes = RemoteSources()

    /// What `prepare` did to the last batch (skins applied, archives unpacked), for the summary.
    private(set) var lastNote = ""
    /// The same, without the routine "unpacked 3 file(s) from X.zip" lines: what the casual
    /// import line still has to say (a skin, a save, an archive that broke).
    private(set) var lastOtherNote = ""

    private var queued: [SystemQuestion] = []
    private weak var host: EngineHost?
    /// Scratch folders made for this import (unpacked archives, clipboard copies). Removed when it
    /// finishes, because importFiles copies everything it keeps into Documents.
    private var scratch: [URL] = []

    /// Extensions importFiles accepts beyond CoreCatalog's own list: the archives, and the companion
    /// files of multi-file disc formats (a .ccd's .img and .sub, a .gdi's .raw tracks, playlists).
    nonisolated static let extraContentExtensions: Set<String> = [
        "zip", "7z", "ccd", "img", "sub", "gdi", "cdi", "raw", "m3u", "mds", "toc", "cue", "bin",
    ]

    /// Extensions several systems share: a file with one of these is read by the detector after it
    /// lands, and the user is asked when the detector is not sure.
    nonisolated static let sharedExtensions: Set<String> = [
        "cue", "chd", "iso", "bin", "img", "ccd", "mdf", "pbp", "m3u", "zip", "7z", "toc",
    ]

    nonisolated static let skinExtensions: Set<String> = ["deltaskin", "manicskin"]

    func attach(host: EngineHost) {
        self.host = host
        wifi.attach(host: host)
        remotes.attach(host: host)
        Self.installResolver()
    }

    /// Installs the detector as CoreCatalog's resolver: the remembered answer first, then the
    /// detector when it is sure, then the app's own guess (`SystemGuesses`), otherwise nil so
    /// CoreCatalog falls back to its extension table.
    nonisolated static func installResolver() {
        CoreCatalog.systemResolver = { url in
            let name = url.lastPathComponent
            if let chosen = SystemChoices.choice(forFileName: name) { return chosen }
            let ext = url.pathExtension.lowercased()
            guard ImportCenter.sharedExtensions.contains(ext) else { return nil }
            let detection = DetectionCache.shared.detection(for: url)
            if detection.sure { return detection.system }
            return SystemGuesses.guess(forFileName: name)
        }
    }

    /// The system a library entry is, by the same order the resolver uses, then the extension.
    func systemId(for entry: LibraryEntry) -> String? {
        let url = URL(fileURLWithPath: entry.path)
        if let resolved = CoreCatalog.systemResolver?(url) { return resolved }
        return CoreCatalog.system(forExtension: entry.ext)?.rawValue
    }

    // MARK: Before the copy

    /// Routes what is not a game, unpacks archives, and returns the URLs importFiles should copy.
    /// `handled` counts the files that went somewhere else (a skin, a cheat list, a save, a manual).
    func prepare(_ urls: [URL]) -> (urls: [URL], handled: Int) {
        var out: [URL] = []
        var handled = 0
        var notes: [String] = []
        var routine: [String] = []
        lastNote = ""
        lastOtherNote = ""
        for url in urls {
            let ext = url.pathExtension.lowercased()
            if Self.skinExtensions.contains(ext) {
                notes.append(importSkin(url))
                handled += 1
            } else if ext == "cht" {
                notes.append(importCheat(url))
                handled += 1
            } else if ext == "pdf" {
                notes.append(GameplayManuals.importLoose(url))
                handled += 1
            } else if ext != "zip" && ext != "bin" && saveExtensions.contains(ext) {
                notes.append(importSave(url))
                handled += 1
            } else if ext == "zip" || ext == "7z" {
                FeedbackCenter.step("import: opening archive \(url.lastPathComponent)")
                let (files, note) = expandArchive(url)
                FeedbackCenter.step("import: archive gave \(files.count) file(s)"
                    + (note.map { ": \($0)" } ?? ""))
                out.append(contentsOf: files)
                if let note {
                    notes.append(note)
                    // An archive that gave files is routine; one that gave none is worth saying.
                    if !files.isEmpty { routine.append(note) }
                }
            } else {
                out.append(url)
            }
        }
        if !notes.isEmpty {
            let text = notes.joined(separator: " | ")
            line = text
            lastNote = text
            lastOtherNote = notes.filter { !routine.contains($0) }.joined(separator: " | ")
            host?.status = text
        }
        return (out, handled)
    }

    private lazy var saveExtensions: Set<String> = Set(saveFileExtensions())

    private func newScratch(_ tag: String) -> URL {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("Continuum\(tag)", isDirectory: true)
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        scratch.append(dir)
        return dir
    }

    /// Keeps an arcade, DOS or Amiga archive whole; unpacks anything else flat.
    private func expandArchive(_ url: URL) -> ([URL], String?) {
        let detection = DetectionCache.shared.detection(for: url)
        let name = url.lastPathComponent
        if detection.keepArchive {
            if detection.sure {
                SystemChoices.remember(detection.system, forFileName: name)
            }
            return ([url], "\(name) kept whole for \(SystemNames.name(detection.system)) (\(detection.reason))")
        }
        let dest = newScratch("Unpack")
        do {
            let names = try importArchiveExtract(path: url.path, destDir: dest.path)
            if names.isEmpty {
                return ([], "\(name) held no files to import")
            }
            // Only for a game: a lone README.md in a zip is skipped by the copy, and must not
            // leave a "this is a Mega Drive game" answer behind for its name.
            if detection.sure && names.count == 1,
               NotGames.reason(for: dest.appendingPathComponent(names[0])) == nil {
                SystemChoices.remember(detection.system, forFileName: names[0])
            }
            return (names.map { dest.appendingPathComponent($0) },
                    "unpacked \(names.count) file(s) from \(name)")
        } catch {
            return ([], "\(name) could not be unpacked: \(error)")
        }
    }

    // MARK: After the copy

    /// Reads every newly imported file with a shared extension, asks about the unsure ones, and
    /// clears the scratch folders. Returns the names it asked about: games waiting for that
    /// answer, which the import line counts as added.
    @discardableResult
    func finishImport(_ names: [String], documents: URL) -> [String] {
        defer { clearScratch() }
        var asked: [String] = []
        let lowered = Set(names.map { $0.lowercased() })
        // Files a sheet names (a cue's tracks, a ccd's img) are not asked about on their own.
        var named = Set<String>()
        for name in names {
            let ext = (name as NSString).pathExtension.lowercased()
            if ["cue", "m3u", "gdi", "ccd"].contains(ext) {
                for ref in importReferencedFiles(path: documents.appendingPathComponent(name).path) {
                    named.insert((ref as NSString).lastPathComponent.lowercased())
                }
            }
        }
        for name in names {
            let ext = (name as NSString).pathExtension.lowercased()
            guard Self.sharedExtensions.contains(ext), !named.contains(name.lowercased()) else { continue }
            // A .bin that arrived beside a .cue in the same batch is a track even if the cue named
            // it differently; it is never a launch target.
            if ext == "bin" && lowered.contains(where: { $0.hasSuffix(".cue") }) { continue }
            guard SystemChoices.choice(forFileName: name) == nil else { continue }
            let detection = DetectionCache.shared.detection(for: documents.appendingPathComponent(name))
            if detection.sure { continue }
            // An arcade zip lists as Arcade with no question: cArcade is the only arcade core, and
            // "Which system?" would only offer the wrong answers. Kept as the app's guess, not as
            // the player's answer, so a later version can decide again.
            if detection.system == "arcade" {
                SystemGuesses.remember(detection.system, forFileName: name)
                continue
            }
            ask(SystemQuestion(fileName: name, reason: detection.reason, candidates: detection.candidates))
            asked.append(name)
        }
        return asked
    }

    private func clearScratch() {
        for dir in scratch { try? FileManager.default.removeItem(at: dir) }
        scratch.removeAll()
    }

    private func ask(_ q: SystemQuestion) {
        if question == nil { question = q } else { queued.append(q) }
    }

    /// The picker's answer. Nil means "not now": nothing is stored, and CoreCatalog's extension
    /// table decides, which is what happened before detection existed.
    func answer(_ q: SystemQuestion, system: String?) {
        if let system {
            SystemChoices.remember(system, forFileName: q.fileName)
            let text = "\(q.fileName) will open as \(SystemNames.name(system))"
            line = text
            host?.status = text
            host?.refreshLibrary()
        } else {
            host?.status = "\(q.fileName): no system chosen, so its extension decides"
        }
        question = queued.isEmpty ? nil : queued.removeFirst()
    }

    // MARK: Non-game kinds

    private func importSkin(_ url: URL) -> String {
        guard let host else { return "skin import failed: the app is not ready" }
        do {
            let imported = try DeltaSkinImporter.importPackage(at: url)
            guard let system = imported.previewSystem else {
                return "\(url.lastPathComponent): the skin doesn't say which console it's for. "
                    + "Import it in Settings, Skins, Open the skin library, and pick the console"
            }
            let resolved = imported.applying(system: system)
            host.setTouchLayout(resolved.layout.sanitised, for: system)
            host.applyImportedSkin(imported, for: system)
            return "skin \(url.lastPathComponent) applied to \(system.displayName): \(imported.summary)"
        } catch {
            return "skin \(url.lastPathComponent) was not imported: \(error.localizedDescription)"
        }
    }

    /// The library game a loose file belongs to, by matching filename stems. A running game wins
    /// when its stem matches too.
    private func game(matching url: URL) -> LibraryEntry? {
        guard let host else { return nil }
        let stem = url.deletingPathExtension().lastPathComponent.lowercased()
        if let running = host.activeEntry,
           (running.name as NSString).deletingPathExtension.lowercased() == stem {
            return running
        }
        return host.library.first { ($0.name as NSString).deletingPathExtension.lowercased() == stem }
    }

    private func importCheat(_ url: URL) -> String {
        guard let host else { return "cheat import failed: the app is not ready" }
        guard let data = try? Data(contentsOf: url) else {
            return "\(url.lastPathComponent) could not be read"
        }
        guard let entry = game(matching: url) ?? host.activeEntry else {
            return "\(url.lastPathComponent): no game in the library has that name. Open the game's card "
                + "and import the .cht from its cheats section"
        }
        return host.cheats.importChtFile(data, named: url.lastPathComponent,
                                         forGameId: SaveStates.gameId(for: entry))
    }

    private func importSave(_ url: URL) -> String {
        guard let host else { return "save import failed: the app is not ready" }
        guard let data = try? Data(contentsOf: url) else {
            return "\(url.lastPathComponent) could not be read"
        }
        guard let entry = game(matching: url) else {
            return "\(url.lastPathComponent): no game in the library has that name. Open the "
                + "game's card and use Save slots, export and import"
        }
        guard let system = systemId(for: entry) else {
            return "\(url.lastPathComponent): \(entry.name) has no system yet"
        }
        return host.saveStates.importSaveFile(data, named: url.lastPathComponent, for: entry,
                                              system: system)
    }

    // MARK: Clipboard, drag and drop, Open in

    /// Imports whatever files are on the clipboard (Handoff from a Mac puts them there too).
    func importClipboard() {
        let providers = UIPasteboard.general.itemProviders
        guard !providers.isEmpty else {
            report("the clipboard is empty; copy a file in Files or on a Mac first")
            return
        }
        importProviders(providers, source: "the clipboard")
    }

    /// Loads the file representation of each provider into scratch, then imports them together so
    /// a .cue and its tracks arrive as one batch.
    func importProviders(_ providers: [NSItemProvider], source: String) {
        let dest = newScratch("Incoming")
        let group = DispatchGroup()
        let box = IncomingBox()
        for provider in providers {
            guard let type = Self.fileType(of: provider) else {
                box.fail(provider.suggestedName ?? "an item that is not a file")
                continue
            }
            group.enter()
            let suggested = provider.suggestedName
            provider.loadFileRepresentation(forTypeIdentifier: type) { url, error in
                defer { group.leave() }
                guard let url else {
                    box.fail("\(suggested ?? "item"): \(error?.localizedDescription ?? "no file")")
                    return
                }
                var name = suggested ?? url.lastPathComponent
                if (name as NSString).pathExtension.isEmpty {
                    let ext = url.pathExtension.isEmpty
                        ? (UTType(type)?.preferredFilenameExtension ?? "")
                        : url.pathExtension
                    if !ext.isEmpty { name += ".\(ext)" }
                }
                let target = dest.appendingPathComponent(wifiSafeFileName(name: name) ?? url.lastPathComponent)
                do {
                    try? FileManager.default.removeItem(at: target)
                    try FileManager.default.copyItem(at: url, to: target)
                    box.add(target)
                } catch {
                    box.fail("\(name): \(error.localizedDescription)")
                }
            }
        }
        group.notify(queue: .main) {
            Task { @MainActor [weak self] in
                self?.finishIncoming(box.urls(), failures: box.failures(), source: source)
            }
        }
    }

    private func finishIncoming(_ urls: [URL], failures: [String], source: String) {
        if urls.isEmpty {
            report("nothing from \(source) could be imported"
                + (failures.isEmpty ? "" : ": " + failures.joined(separator: ", ")))
            return
        }
        host?.importFiles(urls.sorted { $0.lastPathComponent < $1.lastPathComponent })
        if !failures.isEmpty {
            host?.status += " | not imported from \(source): " + failures.joined(separator: ", ")
        }
        line = "from \(source): \(host?.status ?? "")"
    }

    /// The type to load a provider as: its first type that is file data rather than text, a link
    /// or a picture preview.
    private static func fileType(of provider: NSItemProvider) -> String? {
        let refused: [UTType] = [.plainText, .utf8PlainText, .html, .url, .rtf]
        for id in provider.registeredTypeIdentifiers {
            guard let type = UTType(id) else { continue }
            if refused.contains(where: { type == $0 }) { continue }
            if type.conforms(to: .data) || type.conforms(to: .fileURL) || type.conforms(to: .item) {
                return id
            }
        }
        return nil
    }

    /// Open in / Share to Continuum. The URL may be a security-scoped original (opened in place)
    /// or a copy in Documents/Inbox; either way it is copied to scratch and imported.
    func open(url: URL) {
        guard url.isFileURL else {
            report("Continuum was opened with \(url.absoluteString), which is not a file")
            return
        }
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        let dest = newScratch("Opened")
        let target = dest.appendingPathComponent(url.lastPathComponent)
        do {
            try FileManager.default.copyItem(at: url, to: target)
        } catch {
            report("\(url.lastPathComponent) could not be opened: \(error.localizedDescription)")
            return
        }
        // A "Copy to" lands in Documents/Inbox; the copy above is the one kept.
        if url.path.contains("/Documents/Inbox/") {
            try? FileManager.default.removeItem(at: url)
        }
        host?.importFiles([target])
        line = "opened \(url.lastPathComponent): \(host?.status ?? "")"
    }

    private func report(_ text: String) {
        line = text
        host?.status = text
    }
}

/// What the item-provider callbacks collect, behind a lock: they run on arbitrary queues.
final class IncomingBox: @unchecked Sendable {
    private let lock = NSLock()
    private var collected: [URL] = []
    private var failed: [String] = []

    func add(_ url: URL) { lock.lock(); collected.append(url); lock.unlock() }
    func fail(_ text: String) { lock.lock(); failed.append(text); lock.unlock() }
    func urls() -> [URL] { lock.lock(); defer { lock.unlock() }; return collected }
    func failures() -> [String] { lock.lock(); defer { lock.unlock() }; return failed }
}

// MARK: - The picker

/// "Which system is this?" Asked once per file when detection is unsure.
struct SystemQuestionSheet: View {
    let question: SystemQuestion
    let onAnswer: (String?) -> Void

    /// The detector's guesses that can launch here. It can guess a system with no core (a disc
    /// that might be GameCube), and offering that at the top would import a game that cannot start.
    private var likely: [String] {
        question.candidates.filter { SystemNames.canRun($0) }
    }

    var body: some View {
        NavigationView {
            List {
                Section {
                    Text(question.fileName).font(.headline)
                    Text("Continuum couldn't tell which system this is (\(question.reason)). "
                         + "Pick one and it'll remember.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
                if !likely.isEmpty {
                    Section("Most likely") {
                        ForEach(likely, id: \.self) { id in
                            Button(SystemNames.name(id)) { onAnswer(id) }
                        }
                    }
                }
                Section("Every system") {
                    ForEach(SystemNames.pickable.filter { !likely.contains($0.id) }, id: \.id) { item in
                        Button(item.name) { onAnswer(item.id) }
                    }
                }
            }
            .navigationTitle("Which system?")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Not now") { onAnswer(nil) }
                }
            }
        }
        .preferredColorScheme(.dark)
    }
}

/// Mounted once under RootView: presents the system question and handles Open in.
struct ImportCenterPresenter: View {
    @ObservedObject var center: ImportCenter

    var body: some View {
        Color.clear
            .allowsHitTesting(false)
            .sheet(item: $center.question) { q in
                SystemQuestionSheet(question: q) { answer in center.answer(q, system: answer) }
                    .interactiveDismissDisabled(true)
            }
    }
}
