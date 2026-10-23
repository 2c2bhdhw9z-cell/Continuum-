// Continuum - the Library's extras (owner, 16 October 2026, after comparing Continuum with Manic
// on their phone). Manic tells us WHAT players expect; how each one looks and reads here is
// Continuum's own (owner-rules, "Do what Manic does, never the way Manic looks").
//
//   - Covers at each system's real box shape (`GameSystem.coverAspect`; drawn by CoverArtView).
//   - The library by console: a section per console that folds, an index down the right edge,
//     the player's own console order, and picking several games at once (`ConsoleSections`,
//     `LibraryPrefs`, `LibrarySelection`).
//   - On the game card: Clean start (no cheats, no auto-save and none of the game's own
//     settings, once), sharing the game's file, a link that opens the game, patching a game,
//     joining a game's discs, and the console's language for that game (`GameCardExtras`).
//   - Settings, SOUND: the silent switch mutes games (`SilentMode`).

import AVFoundation
import SwiftUI
import UIKit
import UniformTypeIdentifiers

// MARK: - The usual box shape of each system's covers

extension GameSystem {
    /// Width over height of this system's usual box art: the North American box, which most of
    /// libretro's covers are. Used for a cover before its picture arrives, for a game with no
    /// cover, and for the space a console's own section gives its covers. A cover that has a
    /// picture is always drawn at the picture's own shape.
    var coverAspect: CGFloat {
        switch self {
        // Landscape boxes.
        case .snes, .n64:
            return 1.38
        // Square, or nearly: the Game Boy family's boxes and the CD jewel cases.
        case .gb, .gbc, .gba, .pokemini, .ps1, .dreamcast, .pcecd, .flash:
            return 1.0
        case .ds, .n3ds:
            return 0.9
        // The tall UMD case.
        case .psp:
            return 0.58
        case .saturn, .tg16, .sgx, .wswan, .ngp:
            return 0.85
        // Portrait cartridge boxes and keep cases: NES, Mega Drive, Master System and the rest.
        default:
            return 0.72
        }
    }
}

// MARK: - Mute games with the silent switch

/// Settings, SOUND: when on, the iPhone's silent switch (or Action button) mutes games, the way
/// it mutes most games from the App Store. Off by default, so games play out loud as they always
/// did. Read by `AudioSessionPolicy`, which picks the audio category: `.ambient` follows the
/// switch, `.playback` ignores it. `.ambient` also mixes with other apps' sound, so music playing
/// in another app keeps going under the game, and the note under the switch says so.
enum SilentMode {
    static let key = "continuum.audio.respectSilentSwitch.v1"

    static var respected: Bool {
        UserDefaults.standard.bool(forKey: key)
    }
}

struct SilentModeToggle: View {
    @AppStorage(SilentMode.key) private var respected = false

    var body: some View {
        Toggle(isOn: $respected) {
            VStack(alignment: .leading, spacing: 3) {
                Text("Mute games with the silent switch")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(ShellPalette.text)
                Text("On silent, games are quiet too, and your music can keep playing.")
                    .font(.system(size: 12))
                    .foregroundStyle(ShellPalette.secondaryText)
            }
        }
        .tint(ShellPalette.accent)
        .onChange(of: respected) { _ in
            // Straight away, for a game already playing. The policy skips a category that is
            // already set, so this can never restart the sound for nothing.
            try? AudioSessionPolicy.apply(to: AVAudioSession.sharedInstance())
        }
    }
}

// MARK: - The player's console order, and which consoles are folded

@MainActor
final class LibraryPrefs: ObservableObject {
    static let shared = LibraryPrefs()

    private static let orderKey = "continuum.library.consoleOrder.v1"
    private static let foldedKey = "continuum.library.foldedConsoles.v1"

    /// The consoles in the order the player put them (Settings, LAYOUT, Console order), by id.
    /// A console not in it comes after, in the app's usual order. Empty is the usual order.
    @Published var consoleOrder: [String] {
        didSet { UserDefaults.standard.set(consoleOrder, forKey: Self.orderKey) }
    }

    /// The console sections the player folded shut in All Games, by id.
    @Published var folded: Set<String> {
        didSet { UserDefaults.standard.set(Array(folded).sorted(), forKey: Self.foldedKey) }
    }

    private init() {
        consoleOrder = UserDefaults.standard.stringArray(forKey: Self.orderKey) ?? []
        folded = Set(UserDefaults.standard.stringArray(forKey: Self.foldedKey) ?? [])
    }

    /// These systems in the player's order.
    func ordered(_ systems: [GameSystem]) -> [GameSystem] {
        let rank = Dictionary(consoleOrder.enumerated().map { ($0.element, $0.offset) },
                              uniquingKeysWith: { first, _ in first })
        let usual = GameSystem.allCases
        func position(_ system: GameSystem) -> Int {
            rank[system.rawValue] ?? (10_000 + (usual.firstIndex(of: system) ?? 0))
        }
        return systems.sorted { position($0) < position($1) }
    }

    func toggleFold(_ id: String) {
        if folded.contains(id) {
            folded.remove(id)
        } else {
            folded.insert(id)
        }
    }
}

// MARK: - Picking several games at once

@MainActor
final class LibrarySelection: ObservableObject {
    static let shared = LibrarySelection()

    /// Select mode is on: a tap ticks a game instead of starting it.
    @Published var active = false {
        didSet {
            if !active { picked = [] }
        }
    }

    /// The ticked games, by id (their path).
    @Published private(set) var picked: Set<String> = []

    /// Whether this game is ticked, or nil when select mode is off (the card then shows no tick).
    func mark(for entry: LibraryEntry) -> Bool? {
        active ? picked.contains(entry.id) : nil
    }

    func toggle(_ entry: LibraryEntry) {
        if picked.contains(entry.id) {
            picked.remove(entry.id)
        } else {
            picked.insert(entry.id)
        }
    }

    /// The ticked games among `entries` (what the page shows): a search or another console can
    /// hide ticked games, and nothing is done to a game the player cannot see.
    func picked(in entries: [LibraryEntry]) -> Set<String> {
        picked.intersection(entries.map(\.id))
    }

    func pickAll(_ entries: [LibraryEntry]) {
        picked = Set(entries.map(\.id))
    }

    func clear() {
        picked = []
    }
}

// MARK: - A link that opens a game

/// continuum://play?game=<the file name>. Copied from the game card; opened from Shortcuts
/// ("Open URLs"), a home screen bookmark or another app, it starts that game. The file name is
/// the game's identity everywhere else in the app too.
enum LaunchLink {
    static func url(for entry: LibraryEntry) -> URL? {
        var parts = URLComponents()
        parts.scheme = "continuum"
        parts.host = "play"
        parts.queryItems = [URLQueryItem(name: "game", value: entry.name)]
        return parts.url
    }

    /// The game a link names. Nil when the link has no game in it.
    static func gameName(in url: URL) -> String? {
        URLComponents(url: url, resolvingAgainstBaseURL: false)?
            .queryItems?
            .first(where: { $0.name == "game" })?
            .value
    }
}

extension EngineHost {
    /// Opens the game a continuum:// link names. False when the URL isn't a Continuum link, so
    /// the caller treats it as a file instead.
    func openLaunchLink(_ url: URL) -> Bool {
        guard url.scheme?.lowercased() == "continuum" else { return false }
        guard let name = LaunchLink.gameName(in: url), !name.isEmpty else {
            libraryNotice = "That link doesn't name a game."
            return true
        }
        Task { @MainActor [weak self] in
            // From a cold start the Library and the game screen need a moment to come up.
            try? await Task.sleep(nanoseconds: 700_000_000)
            guard let self else { return }
            if !self.library.contains(where: { $0.name == name }) {
                self.refreshLibrary()
            }
            guard let entry = self.library.first(where: { $0.name == name }) else {
                self.libraryNotice = "\(name) isn't in your library anymore."
                return
            }
            if self.activeEntry?.path == entry.path { return }
            self.detailEntry = nil
            self.launch(entry: entry)
        }
        return true
    }

    /// Clean start (the game card): this one start has no cheats, no auto-save and none of the
    /// game's own core settings. The auto-save is held, so leaving does not write over it, and
    /// the game's own settings file is left exactly as it was (see `options::install`). Good for
    /// a game that won't start after a cheat or a setting, or a save state that crashes it.
    func launchCleanStart(entry: LibraryEntry) {
        CleanStart.pendingPath = entry.path
        launch(entry: entry, resumingAuto: false)
        CleanStart.pendingPath = nil
        // Taken back in case this start never reached the core (a missing BIOS, say). After a
        // start that did, the engine has already used it up, and this changes nothing.
        engine.skipGameSettingsNextStart(skip: false)
    }
}

/// The game being started by Clean start, read by `EngineHost.launch` while it runs.
enum CleanStart {
    @MainActor static var pendingPath: String?
}

// MARK: - Sharing a game's file

enum GameShare {
    /// Every file the game is made of: the file itself, a cue sheet's tracks, a GD-ROM sheet's
    /// tracks, a CloneCD sheet's .img and .sub, an Alcohol .mds's .mdf, a .toc's track files, and
    /// a playlist's discs with theirs. Only files that are really there, and only inside the
    /// game's own folder (a sheet that names "../something" shares nothing outside it).
    static func files(for entry: LibraryEntry) -> [URL] {
        let url = URL(fileURLWithPath: entry.path)
        var out = [url]
        if entry.ext == "m3u" {
            for disc in MultiDisc.discs(inPlaylist: url) {
                out.append(disc)
                out += parts(of: disc)
            }
        } else {
            out += parts(of: url)
        }
        let root = url.deletingLastPathComponent().standardizedFileURL.path + "/"
        var seen = Set<String>()
        return out.filter {
            let path = $0.standardizedFileURL.path
            return path.hasPrefix(root) && FileManager.default.fileExists(atPath: $0.path)
                && seen.insert(path).inserted
        }
    }

    /// The other files one disc needs, by the kind of sheet it is. Empty for a disc that is one
    /// file (a .chd, an .iso, a .pbp).
    static func parts(of disc: URL) -> [URL] {
        switch disc.pathExtension.lowercased() {
        case "cue": return cueTracks(disc)
        case "gdi": return gdiTracks(disc)
        case "ccd": return companions(of: disc, extensions: ["img", "sub"])
        case "mds": return companions(of: disc, extensions: ["mdf"])
        case "toc": return tocTracks(disc)
        default: return []
        }
    }

    /// A sheet's text, UTF-8 or else Latin-1, without a byte-order mark at the front (Notepad
    /// writes one, and it would otherwise become part of the first name in the file).
    static func text(of url: URL) -> String? {
        guard let text = (try? String(contentsOf: url, encoding: .utf8))
            ?? (try? String(contentsOf: url, encoding: .isoLatin1)) else { return nil }
        return droppingByteOrderMark(text)
    }

    /// The text with a byte-order mark taken off the front: U+FEFF as UTF-8 reads it, or its
    /// three bytes as Latin-1 reads them ("ï»¿").
    static func droppingByteOrderMark(_ text: String) -> String {
        let scalars = text.unicodeScalars
        var drop = 0
        if scalars.first?.value == 0xFEFF {
            drop = 1
        } else if scalars.prefix(3).map(\.value) == [0xEF, 0xBB, 0xBF] {
            drop = 3
        }
        guard drop > 0 else { return text }
        var rest = String.UnicodeScalarView()
        rest.append(contentsOf: scalars.dropFirst(drop))
        return String(rest)
    }

    static func cueTracks(_ cue: URL) -> [URL] {
        guard let sheet = text(of: cue) else { return [] }
        return LibraryEntry.trackURLs(inCueSheet: sheet, relativeTo: cue.deletingLastPathComponent())
    }

    /// A .gdi names one track per line after its first: number, sector, type, size, FILE, offset.
    /// The file name can be in quotes, with spaces in it ("Track 01.bin").
    static func gdiTracks(_ gdi: URL) -> [URL] {
        guard let sheet = text(of: gdi) else { return [] }
        let folder = gdi.deletingLastPathComponent()
        return sheet.split(whereSeparator: \.isNewline).compactMap { line -> URL? in
            let fields = quotedFields(String(line))
            guard fields.count >= 5 else { return nil }
            return relativeFile(fields[4], in: folder)
        }
    }

    /// A .toc (cdrdao's sheet) names its tracks' files on FILE, DATAFILE and AUDIOFILE lines. One
    /// that names none is the PlayStation kind beside a .bin of the same name.
    static func tocTracks(_ toc: URL) -> [URL] {
        let folder = toc.deletingLastPathComponent()
        var found: [URL] = []
        if let sheet = text(of: toc) {
            for line in sheet.split(whereSeparator: \.isNewline) {
                let fields = quotedFields(String(line))
                guard fields.count >= 2,
                      ["FILE", "DATAFILE", "AUDIOFILE"].contains(fields[0].uppercased()),
                      let file = relativeFile(fields[1], in: folder) else { continue }
                found.append(file)
            }
        }
        return found.isEmpty ? companions(of: toc, extensions: ["bin"]) : found
    }

    /// The files beside a sheet with its name and these extensions (a .ccd's .img and .sub),
    /// in lower or upper case, whichever is there.
    static func companions(of sheet: URL, extensions: [String]) -> [URL] {
        let folder = sheet.deletingLastPathComponent()
        let stem = sheet.deletingPathExtension().lastPathComponent
        return extensions.compactMap { ext -> URL? in
            for spelling in [ext, ext.uppercased()] {
                let file = folder.appendingPathComponent("\(stem).\(spelling)")
                if FileManager.default.fileExists(atPath: file.path) { return file }
            }
            return nil
        }
    }

    /// A line's fields, split on spaces and tabs, with a "quoted field" kept whole and its quotes
    /// taken off: `3 45000 4 2352 "Track 03.bin" 0` gives six fields, the fifth "Track 03.bin".
    static func quotedFields(_ line: String) -> [String] {
        var fields: [String] = []
        var current = ""
        var inQuotes = false
        var started = false
        for character in line {
            if character == "\"" {
                inQuotes.toggle()
                started = true
            } else if !inQuotes && (character == " " || character == "\t") {
                if started {
                    fields.append(current)
                    current = ""
                    started = false
                }
            } else {
                current.append(character)
                started = true
            }
        }
        if started {
            fields.append(current)
        }
        return fields
    }

    /// A file a sheet or a playlist names, inside `folder`: a plain name, or a path into a folder
    /// inside it ("CD1/Game.cue"). Never an absolute path, and never one that climbs out with
    /// "..", so a list can't point anywhere else on the phone.
    static func relativeFile(_ name: String, in folder: URL) -> URL? {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty, !trimmed.hasPrefix("/") else { return nil }
        let parts = trimmed.split(separator: "/").map(String.init).filter { $0 != "." }
        guard !parts.isEmpty, !parts.contains("..") else { return nil }
        return parts.reduce(folder) { $0.appendingPathComponent($1) }
    }

    /// The share sheet, over whatever is on screen (the game card).
    @MainActor
    static func present(_ urls: [URL]) {
        guard !urls.isEmpty, let top = EngineHost.topmostViewController() else { return }
        let sheet = UIActivityViewController(activityItems: urls, applicationActivities: nil)
        // An iPad shows it as a popover, which needs somewhere to point.
        if let popover = sheet.popoverPresentationController {
            popover.sourceView = top.view
            popover.sourceRect = CGRect(x: top.view.bounds.midX, y: top.view.bounds.midY,
                                        width: 1, height: 1)
            popover.permittedArrowDirections = []
        }
        top.present(sheet, animated: true)
    }
}

// MARK: - Picking one file (a patch)

/// A Files picker for one file, handing back an app-owned copy (asCopy), or nil when cancelled.
/// The delegate is kept here while the picker is up, because the picker holds it weakly.
final class OneFilePicker: NSObject, UIDocumentPickerDelegate {
    @MainActor private static var current: OneFilePicker?
    private let done: @MainActor (URL?) -> Void
    private var delivered = false

    private init(done: @escaping @MainActor (URL?) -> Void) {
        self.done = done
        super.init()
    }

    @MainActor
    static func pick(done: @escaping @MainActor (URL?) -> Void) {
        guard let top = EngineHost.topmostViewController() else {
            done(nil)
            return
        }
        let picker = UIDocumentPickerViewController(forOpeningContentTypes: [.item], asCopy: true)
        picker.allowsMultipleSelection = false
        let delegate = OneFilePicker(done: done)
        current = delegate
        picker.delegate = delegate
        top.present(picker, animated: true)
    }

    @objc func documentPicker(_ controller: UIDocumentPickerViewController,
                              didPickDocumentsAt urls: [URL]) {
        deliver(urls.first)
    }

    @objc func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
        deliver(nil)
    }

    private func deliver(_ url: URL?) {
        guard !delivered else { return }
        delivered = true
        let done = self.done
        Task { @MainActor in
            done(url)
            OneFilePicker.current = nil
        }
    }
}

// MARK: - Joining a game's discs

enum MultiDisc {
    /// The systems whose games come on more than one disc.
    static let systems: Set<GameSystem> = [.ps1, .saturn, .segacd, .pcecd, .dreamcast]

    /// Disc image types a playlist can name.
    static let discExtensions: Set<String> = [
        "cue", "chd", "iso", "pbp", "gdi", "cdi", "ccd", "img", "mds", "toc", "cso",
    ]

    /// Whether this game is one disc of a system with multi-disc games.
    static func isDisc(_ entry: LibraryEntry) -> Bool {
        guard discExtensions.contains(entry.ext),
              let system = CoreCatalog.system(for: entry) else { return false }
        return systems.contains(system)
    }

    /// The discs a playlist names, in order: files beside it, or in a folder inside its own
    /// ("CD1/Game.cue"). Blank lines, comments, a byte-order mark at the front, and anything
    /// outside the playlist's folder (an absolute path, "..") are skipped.
    static func discs(inPlaylist url: URL) -> [URL] {
        guard let text = GameShare.text(of: url) else { return [] }
        let folder = url.deletingLastPathComponent()
        return text.split(whereSeparator: \.isNewline).compactMap { raw -> URL? in
            let line = raw.trimmingCharacters(in: .whitespaces)
            guard !line.isEmpty, !line.hasPrefix("#") else { return nil }
            // RetroArch lets a line carry a label after "|".
            let name = (line.split(separator: "|", maxSplits: 1).first.map(String.init) ?? line)
                .trimmingCharacters(in: .whitespaces)
            return GameShare.relativeFile(name, in: folder)
        }
    }

    /// Whether a disc a playlist names is beside it, where a Library row can be (the Library
    /// lists the Continuum folder itself, never a folder inside it).
    static func isBeside(_ disc: URL, playlist: URL) -> Bool {
        disc.deletingLastPathComponent().standardizedFileURL.path
            == playlist.deletingLastPathComponent().standardizedFileURL.path
    }

    /// The file names, in lower case, of the discs that the playlists among `files` name beside
    /// themselves. The Library leaves those out, so a game on several discs shows once, as its
    /// playlist. ONLY A PLAYLIST THAT CAN START hides its discs: one whose system nothing can
    /// tell (its first disc needed "Which system?", and it's not on record for the playlist)
    /// isn't listed itself, and hiding its discs too made the whole game vanish.
    static func namedByPlaylists(_ files: [URL]) -> Set<String> {
        var names = Set<String>()
        for file in files where file.pathExtension.lowercased() == "m3u"
            && CoreCatalog.isLaunchable(path: file.path) {
            for disc in discs(inPlaylist: file) where isBeside(disc, playlist: file) {
                names.insert(disc.lastPathComponent.lowercased())
            }
        }
        return names
    }

    /// A playlist nothing can place, whose first disc the player placed themselves ("Which
    /// system?"), is placed the same way, so it can start. Joining discs in build 191 wrote such
    /// playlists without recording their system (`writePlaylist` records it now), and they could
    /// never start. Only for the multi-disc systems, and never over an answer already on record.
    static func placePlaylistsByTheirFirstDisc(_ files: [URL]) {
        for file in files where file.pathExtension.lowercased() == "m3u"
            && SystemChoices.choice(forFileName: file.lastPathComponent) == nil
            && !CoreCatalog.isLaunchable(path: file.path) {
            guard let first = discs(inPlaylist: file).first, isBeside(first, playlist: file),
                  let answer = SystemChoices.choice(forFileName: first.lastPathComponent),
                  let system = GameSystem(rawValue: answer),
                  systems.contains(system) else { continue }
            SystemChoices.remember(answer, forFileName: file.lastPathComponent)
        }
    }

    /// The discs that go with these games when they're deleted: each playlist's discs beside it
    /// that are games the Library would list, unless a playlist that stays names them too. Never
    /// a file in another folder, and never a .bin track (a .cue's tracks stay, as they do when a
    /// .cue is deleted on its own).
    static func discsGoingWith(_ games: [LibraryEntry], library: [LibraryEntry]) -> [LibraryEntry] {
        let playlists = games.filter { $0.ext == "m3u" }
        guard !playlists.isEmpty else { return [] }
        let leaving = Set(games.map(\.path))
        let staying = library.filter { $0.ext == "m3u" && !leaving.contains($0.path) }
        let stillNamed = namedByPlaylists(staying.map { URL(fileURLWithPath: $0.path) })
        var seen = leaving
        var out: [LibraryEntry] = []
        for playlist in playlists {
            let url = URL(fileURLWithPath: playlist.path)
            for disc in discs(inPlaylist: url) where isBeside(disc, playlist: url) {
                guard discExtensions.contains(disc.pathExtension.lowercased()),
                      !stillNamed.contains(disc.lastPathComponent.lowercased()),
                      FileManager.default.fileExists(atPath: disc.path),
                      CoreCatalog.isLaunchable(path: disc.path),
                      seen.insert(disc.path).inserted else { continue }
                out.append(LibraryEntry(url: disc))
            }
        }
        return out
    }

    /// A disc's game title without its disc number: "Final Fantasy VII (USA) (Disc 1).cue"
    /// becomes "Final Fantasy VII (USA)".
    static func baseTitle(_ fileName: String) -> String {
        var base = (fileName as NSString).deletingPathExtension
        let patterns = [
            #"\s*[\(\[]\s*(disc|disk|cd)\s*\d+(\s*of\s*\d+)?\s*[\)\]]"#,
            #"\s*[-_]?\s*(disc|disk|cd)\s*\d+\s*$"#,
        ]
        for pattern in patterns {
            if let range = base.range(of: pattern, options: [.regularExpression, .caseInsensitive]) {
                base.removeSubrange(range)
            }
        }
        return base.trimmingCharacters(in: .whitespaces)
    }

    /// Writes "<title>.m3u" naming `discs` in order, next to them, and returns it. Never writes
    /// over a file: a name already taken gets " (2)" and so on.
    ///
    /// THE PLAYLIST IS RECORDED AS ITS DISCS' SYSTEM (`SystemChoices`, by its file name). A new
    /// playlist is otherwise placed by reading its first disc, and a disc the player had to
    /// answer "Which system?" for can't be placed that way: the playlist couldn't start, and the
    /// Library would have hidden its discs behind it.
    static func writePlaylist(title: String, discs: [LibraryEntry]) -> (url: URL?, problem: String) {
        guard let first = discs.first else { return (nil, "Pick at least two discs.") }
        let folder = URL(fileURLWithPath: first.path).deletingLastPathComponent()
        var stem = title
            .replacingOccurrences(of: "/", with: "-")
            .replacingOccurrences(of: ":", with: "-")
            .trimmingCharacters(in: .whitespaces)
        // A name starting with a dot is a hidden file, which the Library never lists
        // (".hack//Infection").
        while stem.hasPrefix(".") {
            stem.removeFirst()
        }
        stem = stem.trimmingCharacters(in: .whitespaces)
        if stem.isEmpty { stem = "Game" }
        var target = folder.appendingPathComponent("\(stem).m3u")
        var copy = 2
        while FileManager.default.fileExists(atPath: target.path) {
            target = folder.appendingPathComponent("\(stem) (\(copy)).m3u")
            copy += 1
        }
        let text = discs.map(\.name).joined(separator: "\n") + "\n"
        do {
            try text.write(to: target, atomically: true, encoding: .utf8)
        } catch {
            return (nil, "The list of discs couldn't be saved: \(error.localizedDescription)")
        }
        if let system = CoreCatalog.system(for: first) {
            SystemChoices.remember(system.rawValue, forFileName: target.lastPathComponent)
        }
        return (target, "")
    }

    // MARK: What the joined game takes from the first disc

    /// Copies what the player has for the first disc to the joined game, which goes by the
    /// playlist's file name like every game: its in-game save (a PlayStation memory card), its
    /// save states, its cheats, its star and its own settings (the core it runs on, its skin, and
    /// each emulator's settings for it). Without this the joined game started with nothing.
    ///
    /// Copies only: the first disc keeps everything it had, and nothing the joined game already
    /// has is ever written over. Returns what couldn't come along, in the player's words.
    @MainActor
    static func carryOver(from disc: LibraryEntry, to game: LibraryEntry,
                          host: EngineHost) -> [String] {
        var left: [String] = []
        let from = SaveStates.gameId(for: disc)
        let to = SaveStates.gameId(for: game)
        if !copyFile(SaveStateDisk.batteryURL(gameId: from), to: SaveStateDisk.batteryURL(gameId: to)) {
            left.append("in-game save")
        }
        if !copySaveStates(from: from, to: to, store: host.saveStates) {
            left.append("save states")
        }
        if !copyCheats(from: from, to: to, store: host.cheats) {
            left.append("cheats")
        }
        if host.favourites.contains(disc.name) {
            host.setFavourite(true, for: [game])
        }
        if !copySettings(from: disc, to: game, host: host) {
            left.append("settings")
        }
        return left
    }

    /// Copies one file. True when it's copied, or when there was nothing to copy, or when the
    /// joined game already has exactly the same file; false when it couldn't be, or when the
    /// joined game has a different one, which is kept.
    private static func copyFile(_ source: URL?, to target: URL?) -> Bool {
        let files = FileManager.default
        guard let source, files.fileExists(atPath: source.path) else { return true }
        guard let target else { return false }
        if files.fileExists(atPath: target.path) {
            return files.contentsEqual(atPath: source.path, andPath: target.path)
        }
        do {
            try files.createDirectory(at: target.deletingLastPathComponent(),
                                      withIntermediateDirectories: true)
            try files.copyItem(at: source, to: target)
            return true
        } catch {
            return false
        }
    }

    /// Every save state, in the same slot with the same name, picture and the facts the loading
    /// check needs (the emulator, its version and its settings), the auto-save included.
    ///
    /// Through the store's own files: each state's file and picture are copied to the joined
    /// game's names (`SaveStateDisk`), the list gets the copies, and the store reads the list
    /// again (`reloadFromDisk`, which is what it does after a folder sync changes it). A state
    /// whose slot the joined game already has, or whose file is gone, stays behind. If the list
    /// can't be written, the copies are taken away again, so nothing is half there.
    @MainActor
    private static func copySaveStates(from source: String, to target: String,
                                       store: SaveStates) -> Bool {
        let states = store.states(forGameId: source)
        guard !states.isEmpty else { return true }
        let taken = store.states(forGameId: target)
        let files = FileManager.default
        var copies: [SaveStateRecord] = []
        var allCopied = true
        for record in states {
            if taken.contains(where: { $0.isAuto == record.isAuto && $0.slot == record.slot }) {
                allCopied = false
                continue
            }
            guard let from = SaveStateDisk.payloadURL(gameId: source, slot: record.slot,
                                                      isAuto: record.isAuto),
                  let to = SaveStateDisk.payloadURL(gameId: target, slot: record.slot,
                                                    isAuto: record.isAuto) else {
                allCopied = false
                continue
            }
            // A state whose file is already gone has nothing to carry (its row says so).
            guard files.fileExists(atPath: from.path) else { continue }
            guard !files.fileExists(atPath: to.path) else {
                allCopied = false
                continue
            }
            do {
                try files.copyItem(at: from, to: to)
            } catch {
                allCopied = false
                continue
            }
            if let fromPicture = SaveStateDisk.thumbnailURL(gameId: source, slot: record.slot,
                                                            isAuto: record.isAuto),
               let toPicture = SaveStateDisk.thumbnailURL(gameId: target, slot: record.slot,
                                                          isAuto: record.isAuto),
               files.fileExists(atPath: fromPicture.path),
               !files.fileExists(atPath: toPicture.path) {
                try? files.copyItem(at: fromPicture, to: toPicture)
            }
            copies.append(SaveStateRecord(
                gameId: target, slot: record.slot, isAuto: record.isAuto,
                createdAt: record.createdAt, frame: record.frame, byteCount: record.byteCount,
                coreId: record.coreId, coreVersion: record.coreVersion, label: record.label,
                coreOptions: record.coreOptions))
        }
        guard !copies.isEmpty else { return allCopied }
        if SaveStateDisk.writeIndex(store.records + copies) != nil {
            for copy in copies {
                SaveStateDisk.remove(gameId: target, slot: copy.slot, isAuto: copy.isAuto)
                SaveStateDisk.removeThumbnail(gameId: target, slot: copy.slot, isAuto: copy.isAuto)
            }
            return false
        }
        store.reloadFromDisk()
        return allCopied
    }

    /// Every cheat, in order, on or off as it was. A code the joined game already has is left
    /// as it is.
    @MainActor
    private static func copyCheats(from source: String, to target: String,
                                   store: CheatStore) -> Bool {
        var allCopied = true
        for cheat in store.cheats(forGameId: source) {
            let code = cheat.code.lowercased()
            guard !store.cheats(forGameId: target).contains(where: { $0.code.lowercased() == code })
            else { continue }
            if store.add(code: cheat.code, label: cheat.label, forGameId: target) != nil {
                allCopied = false
                continue
            }
            // `add` switches a new cheat on.
            if !cheat.enabled,
               let copy = store.cheats(forGameId: target).last(where: { $0.code.lowercased() == code }) {
                store.setEnabled(false, for: copy)
            }
        }
        return allCopied
    }

    /// The game's own core (its card's core choice), its skin, and each emulator's settings for
    /// it, kept by the engine as CoreOptions/<emulator>/games/<the game's file name>.opt.
    @MainActor
    private static func copySettings(from disc: LibraryEntry, to game: LibraryEntry,
                                     host: EngineHost) -> Bool {
        let core = host.gameCoreChoice(for: disc)
        if !core.isEmpty, host.gameCoreChoice(for: game).isEmpty {
            host.setGameCoreChoice(core, for: game)
        }
        let skin = host.gameSkinChoice(for: disc)
        if skin != .automatic, host.gameSkinChoice(for: game) == .automatic {
            host.setGameSkin(skin, for: game)
        }
        guard let support = FileManager.default.urls(for: .applicationSupportDirectory,
                                                     in: .userDomainMask).first,
              let emulators = try? FileManager.default.contentsOfDirectory(
                  at: support.appendingPathComponent("CoreOptions", isDirectory: true),
                  includingPropertiesForKeys: nil) else { return true }
        var allCopied = true
        for emulator in emulators {
            let games = emulator.appendingPathComponent("games", isDirectory: true)
            let from = games.appendingPathComponent("\(engineFileStem(disc.name)).opt")
            let to = games.appendingPathComponent("\(engineFileStem(game.name)).opt")
            if !copyFile(from, to: to) { allCopied = false }
        }
        return allCopied
    }

    /// The engine's name for a game's settings file (`game_file_stem` in options.rs): slashes,
    /// colons and control characters become "_", the ends are trimmed, and leading dots go.
    static func engineFileStem(_ name: String) -> String {
        var scalars = String.UnicodeScalarView()
        for scalar in name.unicodeScalars {
            let swapped = scalar == "/" || scalar == "\\" || scalar == ":"
                || scalar.properties.generalCategory == .control
            scalars.append(swapped ? "_" : scalar)
        }
        var stem = String(scalars).trimmingCharacters(in: .whitespacesAndNewlines)
        while stem.hasPrefix(".") {
            stem.removeFirst()
        }
        return stem.isEmpty ? "game" : stem
    }

    /// "save states", "in-game save and cheats", "in-game save, cheats and settings".
    static func listed(_ things: [String]) -> String {
        guard let last = things.last else { return "" }
        guard things.count > 1 else { return last }
        return things.dropLast().joined(separator: ", ") + " and " + last
    }
}

/// Pick the discs of one game, and Continuum makes them one game in the Library (a playlist,
/// .m3u, next to them). Next disc in the ⋯ menu then goes through them.
struct MultiDiscSheet: View {
    let entry: LibraryEntry
    @ObservedObject var host: EngineHost
    let onDone: () -> Void

    /// The other discs of the same system, the ones that look like this game first. Worked out
    /// once, when the sheet opens: it routes every game in the Library, which is too much to do
    /// on every redraw and every tap.
    @State private var candidates: [LibraryEntry] = []
    @State private var loaded = false
    @State private var picked: Set<String> = []
    @State private var title = ""
    @State private var problem = ""
    /// The game was made, but something stayed with the first disc: the sheet stays up to say so.
    @State private var made = false

    private var chosen: [LibraryEntry] {
        candidates
            .filter { picked.contains($0.id) }
            .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }

    static func candidates(for entry: LibraryEntry, in library: [LibraryEntry]) -> [LibraryEntry] {
        let system = CoreCatalog.system(for: entry)
        let base = MultiDisc.baseTitle(entry.name).lowercased()
        let discs = library.filter {
            $0.ext != "m3u" && MultiDisc.discExtensions.contains($0.ext)
                && CoreCatalog.system(for: $0) == system
        }
        let alike = discs.filter { MultiDisc.baseTitle($0.name).lowercased() == base }
        let others = discs.filter { MultiDisc.baseTitle($0.name).lowercased() != base }
        return alike + others
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                Text("One game, several discs")
                    .font(.system(size: 22, weight: .heavy))
                    .foregroundStyle(ShellPalette.text)
                SettingsNote("Tick every disc of this game. They'll show as one game, and Next disc "
                             + "in the ⋯ menu swaps between them.")

                SettingsLabel("Name")
                TextField("Game name", text: $title)
                    .font(.system(size: 15))
                    .foregroundStyle(ShellPalette.text)
                    .padding(10)
                    .modifier(LookBlockCard(radius: 10))
                    .disabled(made)

                SettingsLabel("Discs, in order")
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(candidates) { disc in
                        Button {
                            if picked.contains(disc.id) {
                                picked.remove(disc.id)
                            } else {
                                picked.insert(disc.id)
                            }
                        } label: {
                            HStack(spacing: 10) {
                                Image(systemName: picked.contains(disc.id)
                                      ? "checkmark.circle.fill" : "circle")
                                    .foregroundStyle(picked.contains(disc.id)
                                                     ? ShellPalette.accent
                                                     : ShellPalette.secondaryText)
                                Text(disc.name)
                                    .font(.system(size: 14))
                                    .foregroundStyle(ShellPalette.text)
                                    .lineLimit(2)
                                    .multilineTextAlignment(.leading)
                                Spacer(minLength: 0)
                            }
                            .padding(.vertical, 9)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .disabled(made)
                    }
                }
                .padding(.horizontal, 12)
                .modifier(LookBlockCard(radius: 12))

                SettingsNote("The first disc's saves, states, cheats, star and settings come along.")

                if !problem.isEmpty {
                    SettingsNote(problem)
                }

                HStack(spacing: 10) {
                    if made {
                        PillButton(title: "Done", systemImage: nil, filled: true) {
                            host.detailEntry = nil
                            onDone()
                        }
                    } else {
                        PillButton(title: "Make it one game", systemImage: "square.stack",
                                   filled: true) {
                            make()
                        }
                        PillButton(title: "Cancel", systemImage: nil, filled: false) {
                            onDone()
                        }
                    }
                }
            }
            .padding(18)
        }
        .background(ShellPalette.background.ignoresSafeArea())
        .presentationDragIndicator(.visible)
        .onAppear {
            guard !loaded else { return }
            loaded = true
            candidates = Self.candidates(for: entry, in: host.library)
            title = MultiDisc.baseTitle(entry.name)
            let base = title.lowercased()
            picked = Set(candidates
                .filter { MultiDisc.baseTitle($0.name).lowercased() == base }
                .map(\.id))
        }
    }

    private func make() {
        let discs = chosen
        guard discs.count >= 2 else {
            problem = "Tick at least two discs."
            return
        }
        let written = MultiDisc.writePlaylist(title: title, discs: discs)
        guard let playlist = written.url else {
            problem = written.problem
            return
        }
        let left = MultiDisc.carryOver(from: discs[0], to: LibraryEntry(url: playlist), host: host)
        host.refreshLibrary()
        let name = playlist.deletingPathExtension().lastPathComponent
        guard left.isEmpty else {
            made = true
            problem = "\(name) is one game now. Its \(MultiDisc.listed(left)) stayed with the "
                + "first disc."
            return
        }
        host.libraryNotice = "\(name) is one game now, with \(discs.count) discs."
        host.detailEntry = nil
        onDone()
    }
}

// MARK: - The game card's extras

/// Clean start, Share, the game link, Patch, the discs and the language, on the game card.
struct GameCardExtras: View {
    let entry: LibraryEntry
    @ObservedObject var host: EngineHost

    @State private var line = ""
    @State private var showingDiscs = false
    /// The console's language for this game, read when the card opens and again after a change,
    /// never in the drawing: both reads open files under the engine's settings lock, and the card
    /// redraws whenever anything on the host changes.
    @State private var language: GameLanguage?
    /// A patch is being applied, away from the main thread, which a big ROM can take a moment.
    @State private var patching = false

    /// What the language menu shows: the emulator, its language setting, and this game's choice
    /// ("" when it follows the emulator).
    private struct GameLanguage {
        let coreId: String
        let option: CoreLanguageOption
        let current: String
    }

    /// Patches (.ips, .bps, .ups) are made for cartridge games: one ROM file, small enough to
    /// rebuild in memory. Never a disc or its sheet, a playlist, an archive, a big console image
    /// (a disc or 3DS image is gigabytes, and the patcher holds the whole file in memory), or a
    /// game for the Flash, J2ME or Symbian players. The formats here are refused whatever system
    /// they route to; everything else has to be a cartridge system's game as well.
    private static let unpatchable: Set<String> = [
        // Sheets, playlists and archives.
        "m3u", "cue", "gdi", "ccd", "mds", "toc", "zip", "7z", "gz", "lha", "rp9", "dosz",
        // Disc images and big console images, including the formats this app doesn't open yet.
        "chd", "cso", "pbp", "cdi", "iso", "img", "mdf", "nrg", "rvz", "wbfs", "wia", "gcm",
        "ciso", "3ds", "3dsx", "cci", "cxi", "cia", "app", "nsp", "xci", "elf", "prx",
        // The bundled players' games and Symbian's installers.
        "swf", "jar", "jad", "sis", "sisx", "n-gage",
    ]

    /// The systems whose games are cartridge ROMs (or the Disk System's small disk images), the
    /// files these patches are written for.
    private static let patchableSystems: Set<GameSystem> = [
        .nes, .fds, .snes, .gb, .gbc, .gba, .sms, .gg, .sg1000, .genesis, .sega32x, .tg16, .sgx,
        .atari2600, .atari5200, .atari7800, .lynx, .jaguar, .n64, .ds, .wswan, .ngp, .pokemini,
        .vb,
    ]

    static func canPatch(_ entry: LibraryEntry) -> Bool {
        guard !unpatchable.contains(entry.ext),
              let system = CoreCatalog.system(for: entry) else { return false }
        return patchableSystems.contains(system)
    }

    /// Clean start leaves out the cheats, the auto-save and the game's own emulator settings,
    /// which only a game on an emulator has. A Flash or J2ME game opens in its own player before
    /// any of that is looked at, and a game this version can't play opens nothing, so neither is
    /// offered it.
    static func offersCleanStart(_ entry: LibraryEntry) -> Bool {
        guard CoreCatalog.player(forPath: entry.path) == nil else { return false }
        return CoreCatalog.system(for: entry).flatMap { CoreCatalog.unplayableSystems[$0] } == nil
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if Self.offersCleanStart(entry) {
                SettingsButton(title: "Clean start", role: .normal) {
                    let target = entry
                    host.detailEntry = nil
                    host.launchCleanStart(entry: target)
                }
                SettingsNote("No cheats, no auto-save and none of this game's own settings, just "
                             + "this once. Handy when a game won't start. Your saves stay as they "
                             + "are.")
            }

            HStack(spacing: 10) {
                SettingsButton(title: "Share the game", role: .normal) {
                    GameShare.present(GameShare.files(for: entry))
                }
                SettingsButton(title: "Copy its link", role: .normal) {
                    copyLink()
                }
            }

            if Self.canPatch(entry) {
                SettingsButton(title: "Patch this game...", role: .normal) {
                    guard !patching else { return }
                    patch()
                }
                .disabled(patching)
            }
            if MultiDisc.isDisc(entry) {
                SettingsButton(title: "Join it with its other discs...", role: .normal) {
                    showingDiscs = true
                }
            }

            languagePicker

            if !line.isEmpty {
                SettingsNote(line)
            }
        }
        .padding(14)
        .modifier(LookBlockCard(radius: 12))
        .sheet(isPresented: $showingDiscs) {
            MultiDiscSheet(entry: entry, host: host) { showingDiscs = false }
        }
        // Once per card, and again when this game's core changes (its card's core choice, or the
        // PlayStation's in Settings), because the language belongs to the emulator.
        .task(id: "\(entry.id)#\(host.ps1CoreChoice.coreId)#\(host.coreChoiceVersion)") {
            loadLanguage()
        }
    }

    private var title: String { GameMetadata.displayTitle(for: entry) }

    private func copyLink() {
        guard let url = LaunchLink.url(for: entry) else { return }
        UIPasteboard.general.string = url.absoluteString
        line = "Link copied. In Shortcuts, add Open URLs and paste it, and \(title) starts with "
            + "one tap from your home screen."
    }

    /// Pick a patch in Files; the patched game is a new copy next to the original, which is
    /// never changed: "<name> [<patch name>].<ext>", so its cover is found under the same title.
    /// Its card says which patch it has (`PatchedGames`), and a system the player picked for the
    /// original ("Which system?") is picked for the copy too, so it opens the same way.
    ///
    /// The patch runs away from the main thread: the patcher reads the whole game, and a big
    /// cartridge (a DS game is up to 512 MB) would freeze the screen while it works.
    private func patch() {
        let game = entry
        OneFilePicker.pick { picked in
            guard let picked else { return }
            let rom = URL(fileURLWithPath: game.path)
            let folder = rom.deletingLastPathComponent()
            let base = (game.name as NSString).deletingPathExtension
            let patchName = (picked.lastPathComponent as NSString).deletingPathExtension
            var out = folder.appendingPathComponent("\(base) [\(patchName)].\(game.ext)")
            var copy = 2
            while FileManager.default.fileExists(atPath: out.path) {
                out = folder.appendingPathComponent("\(base) [\(patchName)] (\(copy)).\(game.ext)")
                copy += 1
            }
            let target = out
            // A copy of a copy names both patches.
            let label = PatchedGames.patchName(forFileName: game.name)
                .map { "\($0) + \(patchName)" } ?? patchName
            patching = true
            line = "Patching..."
            Task { @MainActor in
                let romPath = rom.path
                let patchPath = picked.path
                let outPath = target.path
                let outcome = await Task.detached(priority: .userInitiated) { () -> Result<String, Error> in
                    do {
                        return .success(try importApplyPatch(romPath: romPath,
                                                             patchPath: patchPath,
                                                             outPath: outPath))
                    } catch {
                        return .failure(error)
                    }
                }.value
                patching = false
                try? FileManager.default.removeItem(at: picked)
                switch outcome {
                case .success(let format):
                    FeedbackCenter.step("patched \(game.name) with \(picked.lastPathComponent) "
                                        + "(\(format)) into \(target.lastPathComponent)")
                    PatchedGames.remember(label, forFileName: target.lastPathComponent)
                    if let chosen = SystemChoices.choice(forFileName: game.name) {
                        SystemChoices.remember(chosen, forFileName: target.lastPathComponent)
                    }
                    host.refreshLibrary()
                    line = "Done. \(GameMetadata.displayTitle(forFileName: target.lastPathComponent)) "
                        + "is in your library. The original is unchanged."
                case .failure(let error):
                    line = "\(error)"
                }
            }
        }
    }

    // MARK: Language

    /// The console's language for this game, where its emulator has a language setting. It
    /// shows once that emulator has run a game on this phone, because that run is how Continuum
    /// learns the emulator's settings.
    @ViewBuilder
    private var languagePicker: some View {
        if let language {
            SettingsLabel("Console language for this game")
            Menu {
                Button("Same as the emulator") {
                    setLanguage(nil, in: language)
                }
                ForEach(language.option.values, id: \.value) { choice in
                    Button(choice.label) {
                        setLanguage(choice.value, in: language)
                    }
                }
            } label: {
                HStack {
                    Text(choiceTitle(language))
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
        }
    }

    /// What the menu's label reads: the choice's own name, or the emulator's when there is none.
    private func choiceTitle(_ language: GameLanguage) -> String {
        guard !language.current.isEmpty else { return "Same as the emulator" }
        return language.option.values.first(where: { $0.value == language.current })?.label
            ?? language.current
    }

    /// Reads the language setting and this game's choice from the engine, once.
    private func loadLanguage() {
        guard let core = CoreCatalog.core(forPath: entry.path,
                                          ps1CoreId: host.ps1CoreChoice.coreId),
              let option = host.engine.coreLanguageOption(coreId: core.coreId) else {
            language = nil
            return
        }
        let current = host.engine.gameOptionValue(coreId: core.coreId, game: entry.name,
                                                  key: option.key) ?? ""
        language = GameLanguage(coreId: core.coreId, option: option, current: current)
    }

    private func setLanguage(_ value: String?, in language: GameLanguage) {
        do {
            try host.engine.setGameOptionValue(coreId: language.coreId, game: entry.name,
                                               key: language.option.key, value: value)
            line = value == nil
                ? "Back to the emulator's own language."
                : "Set. It applies the next time this game starts."
        } catch {
            line = "\(error)"
        }
        // Read back, so the menu shows what the engine kept.
        loadLanguage()
    }
}

// MARK: - The library by console

/// One console's section in All Games.
struct ConsoleGroup: Identifiable, Equatable {
    let id: String
    let title: String
    let badge: String
    let system: GameSystem?
    let entries: [LibraryEntry]
}

/// All Games by console: a section per console with its name and game count that folds shut
/// with a tap, its covers at that console's box shape, and an index of the consoles down the
/// right edge to jump straight to one. The order is the player's (Settings, LAYOUT, Console
/// order).
struct ConsoleSections: View {
    let entries: [LibraryEntry]
    @ObservedObject var host: EngineHost
    let artwork: ArtworkStore
    let generation: Int
    let bottomClearance: CGFloat
    /// False when the Library's console tray has one console picked: its games show straight
    /// away, with no header to fold them away under (build 211 showed only a folded bar there).
    var showsHeaders = true

    @ObservedObject private var prefs = LibraryPrefs.shared
    @ObservedObject private var selection = LibrarySelection.shared
    /// Built when the games or the order change, not on every redraw (the Home shelves' rule).
    @State private var groups: [ConsoleGroup] = []
    private var isNew: Bool { LookStore.isNewLook }

    private static let indexRow: CGFloat = 17

    var body: some View {
        let size = host.libraryCardSize
        let showsIndex = showsHeaders && groups.count >= 3
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 10, pinnedViews: [.sectionHeaders]) {
                    if !showsHeaders {
                        ForEach(groups) { group in
                            grid(group, size: size)
                                .padding(.bottom, 8)
                        }
                    } else {
                    ForEach(groups) { group in
                        Section {
                            if !prefs.folded.contains(group.id) {
                                grid(group, size: size)
                                    .padding(.bottom, 8)
                            }
                        } header: {
                            // The index jumps to the header. On the header, not the Section, so
                            // the Section stays a plain section and its header still pins.
                            header(group)
                                .id(group.id)
                        }
                    }
                    }
                }
                .padding(.trailing, showsIndex ? 30 : 0)
                .padding(.bottom, bottomClearance)
            }
            .overlay(alignment: .topTrailing) {
                if showsIndex {
                    index(proxy)
                        .padding(.trailing, 4)
                        .padding(.top, 6)
                }
            }
        }
        .onAppear { rebuild(entries) }
        // From the value handed in, NOT from `entries`: on iOS 16 this closure can still hold
        // the view from before the change, so it grouped the previous games, and the Library
        // showed the console picked before the one just picked (build 211).
        .onChange(of: entries) { latest in rebuild(latest) }
        .onChange(of: prefs.consoleOrder) { _ in rebuild(entries) }
    }

    private func rebuild(_ latest: [LibraryEntry]) {
        groups = Self.group(latest, order: prefs)
    }

    @MainActor
    static func group(_ entries: [LibraryEntry], order prefs: LibraryPrefs) -> [ConsoleGroup] {
        var bySystem: [GameSystem: [LibraryEntry]] = [:]
        var unrouted: [LibraryEntry] = []
        for entry in entries {
            if let system = CoreCatalog.system(for: entry) {
                bySystem[system, default: []].append(entry)
            } else {
                unrouted.append(entry)
            }
        }
        var out = prefs.ordered(Array(bySystem.keys)).map { system in
            ConsoleGroup(id: system.rawValue, title: system.displayName, badge: system.badge,
                         system: system, entries: bySystem[system] ?? [])
        }
        if !unrouted.isEmpty {
            out.append(ConsoleGroup(id: "other", title: "Other", badge: "...", system: nil,
                                    entries: unrouted))
        }
        return out
    }

    @ViewBuilder
    private func header(_ group: ConsoleGroup) -> some View {
        if isNew { newHeader(group) } else { currentHeader(group) }
    }

    /// The New look's console header: a dark rounded tray bar with the console's photo, its
    /// name, the count and the fold chevron, as in the mockup's SYSTEMS tray.
    private func newHeader(_ group: ConsoleGroup) -> some View {
        let folded = prefs.folded.contains(group.id)
        let shape = RoundedRectangle(cornerRadius: 16, style: .continuous)
        return Button {
            withAnimation(.easeOut(duration: 0.2)) {
                prefs.toggleFold(group.id)
            }
        } label: {
            HStack(spacing: 10) {
                Group {
                    if let system = group.system {
                        Image("console-\(system.rawValue)")
                            .resizable()
                            .interpolation(.high)
                            .scaledToFit()
                    } else {
                        Image(systemName: "questionmark.square.dashed")
                            .font(.system(size: 18, weight: .semibold))
                            .foregroundStyle(ShellPalette.gold)
                    }
                }
                .frame(width: 34, height: 30)
                Text(group.title)
                    .font(.system(size: 17, weight: .bold))
                    .foregroundStyle(.white)
                    .lineLimit(1)
                    .minimumScaleFactor(0.8)
                Text("\(group.entries.count)")
                    .font(.system(size: 11, weight: .bold))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 7)
                    .frame(minHeight: 20)
                    .background(Color(white: 0.30), in: Capsule())
                Spacer(minLength: 0)
                Image(systemName: "chevron.down")
                    .font(.system(size: 12, weight: .bold))
                    .foregroundStyle(ShellPalette.gold)
                    .rotationEffect(.degrees(folded ? -90 : 0))
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(
                ZStack {
                    shape.fill(ShellPalette.charcoal.opacity(0.96))
                    shape.fill(Color.white.opacity(0.06))
                }
            )
            .overlay(shape.strokeBorder(Color.white.opacity(0.08), lineWidth: 0.5))
            .contentShape(shape)
            .padding(.horizontal, 12)
            .padding(.vertical, 4)
        }
        .buttonStyle(NewLookPressStyle(scale: 0.98))
        .accessibilityLabel("\(group.title), \(group.entries.count == 1 ? "1 game" : "\(group.entries.count) games")")
        .accessibilityHint(folded ? "Shows this console's games" : "Hides this console's games")
    }

    private func currentHeader(_ group: ConsoleGroup) -> some View {
        let folded = prefs.folded.contains(group.id)
        return Button {
            withAnimation(.easeOut(duration: 0.2)) {
                prefs.toggleFold(group.id)
            }
        } label: {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Image(systemName: "chevron.down")
                    .font(.system(size: 12, weight: .bold))
                    .foregroundStyle(ShellPalette.secondaryText)
                    .rotationEffect(.degrees(folded ? -90 : 0))
                Text(group.title)
                    .font(.system(size: 18, weight: .bold))
                    .foregroundStyle(ShellPalette.text)
                    .lineLimit(1)
                Text(group.entries.count == 1 ? "1 game" : "\(group.entries.count) games")
                    .font(.system(size: 12))
                    .foregroundStyle(ShellPalette.secondaryText)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 9)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(ShellPalette.background.opacity(0.94))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityHint(folded ? "Shows this console's games" : "Hides this console's games")
    }

    private func grid(_ group: ConsoleGroup, size: LibraryCardSize) -> some View {
        LazyVGrid(
            columns: [GridItem(.adaptive(minimum: size.gridMinimum, maximum: size.gridMaximum),
                               spacing: size.gridSpacing)],
            alignment: .leading,
            spacing: size.gridRowSpacing
        ) {
            ForEach(group.entries) { entry in
                if isNew {
                    NewLookGlowCard(
                        entry: entry,
                        system: group.system,
                        store: artwork,
                        generation: generation,
                        isFavourite: host.favourites.contains(entry.name),
                        width: size.gridCardWidth,
                        cardSize: size,
                        host: host,
                        line: GameMetadata.cardLine(for: entry, system: group.system),
                        frameAspect: group.system?.coverAspect ?? 0.8,
                        selected: selection.mark(for: entry)
                    )
                } else {
                CoverCard(
                    entry: entry,
                    system: group.system,
                    store: artwork,
                    generation: generation,
                    isFavourite: host.favourites.contains(entry.name),
                    width: size.gridCardWidth,
                    host: host,
                    showsLabel: true,
                    cardSize: size,
                    frameAspect: group.system?.coverAspect,
                    selected: selection.mark(for: entry)
                )
                }
            }
        }
        .padding(.horizontal, 16)
        .padding(.top, isNew ? 6 : 0)
    }

    /// The consoles' short names down the right edge. A tap, or a finger sliding down it, jumps
    /// to that console.
    private func index(_ proxy: ScrollViewProxy) -> some View {
        VStack(spacing: 0) {
            ForEach(groups) { group in
                Text(group.badge)
                    .font(.system(size: 8.5, weight: .bold, design: .rounded))
                    .foregroundStyle(ShellPalette.accent)
                    .lineLimit(1)
                    .minimumScaleFactor(0.6)
                    .frame(width: 30, height: Self.indexRow)
            }
        }
        .padding(.vertical, 4)
        .background(ShellPalette.surface, in: Capsule())
        .contentShape(Rectangle())
        .gesture(
            DragGesture(minimumDistance: 0)
                .onChanged { value in
                    let row = Int((value.location.y - 4) / Self.indexRow)
                    guard groups.indices.contains(row) else { return }
                    proxy.scrollTo(groups[row].id, anchor: .top)
                }
        )
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Jump to a console")
    }
}

// MARK: - Settings: the console order

/// Settings, LAYOUT, Console order: drag the consoles into the order All Games and Home's
/// shelves show them in.
struct ConsoleOrderSheet: View {
    @ObservedObject var host: EngineHost
    let onDone: () -> Void
    @ObservedObject private var prefs = LibraryPrefs.shared
    @State private var systems: [GameSystem] = []

    var body: some View {
        NavigationView {
            List {
                Section {
                    ForEach(systems, id: \.rawValue) { system in
                        Text(system.displayName)
                            .foregroundStyle(ShellPalette.text)
                    }
                    .onMove { from, to in
                        systems.move(fromOffsets: from, toOffset: to)
                        save()
                    }
                } footer: {
                    Text("Drag a console by its handle. Only consoles with games are listed.")
                }
                Section {
                    Button("Back to the usual order") {
                        prefs.consoleOrder = []
                        load()
                    }
                    .foregroundStyle(ShellPalette.accent)
                }
            }
            .newLookList()
            .environment(\.editMode, .constant(.active))
            .navigationTitle("Console order")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { onDone() }
                }
            }
        }
        .onAppear { load() }
    }

    private func load() {
        var present: [GameSystem] = []
        var seen = Set<String>()
        for entry in host.library {
            if let system = CoreCatalog.system(for: entry), seen.insert(system.rawValue).inserted {
                present.append(system)
            }
        }
        systems = prefs.ordered(present)
    }

    private func save() {
        let shown = systems.map(\.rawValue)
        let rest = prefs.consoleOrder.filter { !shown.contains($0) }
        prefs.consoleOrder = shown + rest
    }
}

/// The Settings row that opens the console order.
struct ConsoleOrderRow: View {
    @ObservedObject var host: EngineHost
    @State private var showing = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            SettingsButton(title: "Console order...", role: .normal) { showing = true }
            SettingsNote("The order of the consoles in All Games and on Home.")
        }
        .sheet(isPresented: $showing) {
            ConsoleOrderSheet(host: host) { showing = false }
        }
    }
}
