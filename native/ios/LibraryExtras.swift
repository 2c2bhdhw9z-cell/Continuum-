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
    /// tracks, and a playlist's discs with their tracks. Only files that are really there.
    static func files(for entry: LibraryEntry) -> [URL] {
        let url = URL(fileURLWithPath: entry.path)
        var out = [url]
        switch entry.ext {
        case "cue":
            out += cueTracks(url)
        case "gdi":
            out += gdiTracks(url)
        case "m3u":
            for disc in MultiDisc.discs(inPlaylist: url) {
                out.append(disc)
                switch disc.pathExtension.lowercased() {
                case "cue": out += cueTracks(disc)
                case "gdi": out += gdiTracks(disc)
                default: break
                }
            }
        default:
            break
        }
        var seen = Set<String>()
        return out.filter {
            FileManager.default.fileExists(atPath: $0.path) && seen.insert($0.path).inserted
        }
    }

    static func text(of url: URL) -> String? {
        (try? String(contentsOf: url, encoding: .utf8))
            ?? (try? String(contentsOf: url, encoding: .isoLatin1))
    }

    static func cueTracks(_ cue: URL) -> [URL] {
        guard let sheet = text(of: cue) else { return [] }
        return LibraryEntry.trackURLs(inCueSheet: sheet, relativeTo: cue.deletingLastPathComponent())
    }

    /// A .gdi names one track per line: number, sector, type, size, FILE, offset.
    static func gdiTracks(_ gdi: URL) -> [URL] {
        guard let sheet = text(of: gdi) else { return [] }
        let folder = gdi.deletingLastPathComponent()
        return sheet.split(whereSeparator: \.isNewline).compactMap { line -> URL? in
            let fields = line.split(separator: " ", omittingEmptySubsequences: true)
            guard fields.count >= 5 else { return nil }
            let name = String(fields[4]).trimmingCharacters(in: CharacterSet(charactersIn: "\""))
            guard !name.isEmpty, !name.contains("/") else { return nil }
            return folder.appendingPathComponent(name)
        }
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

    /// The discs a playlist names, in order, as files next to it. Blank lines, comments and
    /// anything in another folder are skipped.
    static func discs(inPlaylist url: URL) -> [URL] {
        guard let text = GameShare.text(of: url) else { return [] }
        let folder = url.deletingLastPathComponent()
        return text.split(whereSeparator: \.isNewline).compactMap { raw -> URL? in
            let line = raw.trimmingCharacters(in: .whitespaces)
            guard !line.isEmpty, !line.hasPrefix("#") else { return nil }
            // RetroArch lets a line carry a label after "|".
            let name = (line.split(separator: "|", maxSplits: 1).first.map(String.init) ?? line)
                .trimmingCharacters(in: .whitespaces)
            guard !name.isEmpty, !name.contains("/") else { return nil }
            return folder.appendingPathComponent(name)
        }
    }

    /// The file names the playlists among `files` name. The Library leaves those out, so a
    /// game on several discs shows once, as its playlist.
    static func namedByPlaylists(_ files: [URL]) -> Set<String> {
        var names = Set<String>()
        for file in files where file.pathExtension.lowercased() == "m3u" {
            for disc in discs(inPlaylist: file) {
                names.insert(disc.lastPathComponent)
            }
        }
        return names
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
    static func writePlaylist(title: String, discs: [LibraryEntry]) -> (url: URL?, problem: String) {
        guard let first = discs.first else { return (nil, "Pick at least two discs.") }
        let folder = URL(fileURLWithPath: first.path).deletingLastPathComponent()
        let safe = title
            .replacingOccurrences(of: "/", with: "-")
            .replacingOccurrences(of: ":", with: "-")
            .trimmingCharacters(in: .whitespaces)
        let stem = safe.isEmpty ? "Game" : safe
        var target = folder.appendingPathComponent("\(stem).m3u")
        var copy = 2
        while FileManager.default.fileExists(atPath: target.path) {
            target = folder.appendingPathComponent("\(stem) (\(copy)).m3u")
            copy += 1
        }
        let text = discs.map(\.name).joined(separator: "\n") + "\n"
        do {
            try text.write(to: target, atomically: true, encoding: .utf8)
            return (target, "")
        } catch {
            return (nil, "The list of discs couldn't be saved: \(error.localizedDescription)")
        }
    }
}

/// Pick the discs of one game, and Continuum makes them one game in the Library (a playlist,
/// .m3u, next to them). Next disc in the ⋯ menu then goes through them.
struct MultiDiscSheet: View {
    let entry: LibraryEntry
    @ObservedObject var host: EngineHost
    let onDone: () -> Void

    @State private var picked: Set<String> = []
    @State private var title = ""
    @State private var problem = ""

    /// The other discs of the same system, the ones that look like this game first.
    private var candidates: [LibraryEntry] {
        let system = CoreCatalog.system(for: entry)
        let base = MultiDisc.baseTitle(entry.name).lowercased()
        let discs = host.library.filter {
            $0.ext != "m3u" && MultiDisc.discExtensions.contains($0.ext)
                && CoreCatalog.system(for: $0) == system
        }
        let alike = discs.filter { MultiDisc.baseTitle($0.name).lowercased() == base }
        let others = discs.filter { MultiDisc.baseTitle($0.name).lowercased() != base }
        return alike + others
    }

    private var chosen: [LibraryEntry] {
        candidates
            .filter { picked.contains($0.id) }
            .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
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
                    .background(ShellPalette.surface, in: RoundedRectangle(cornerRadius: 10))

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
                    }
                }
                .padding(.horizontal, 12)
                .background(ShellPalette.surface, in: RoundedRectangle(cornerRadius: 12))

                if !problem.isEmpty {
                    SettingsNote(problem)
                }

                HStack(spacing: 10) {
                    PillButton(title: "Make it one game", systemImage: "square.stack", filled: true) {
                        make()
                    }
                    PillButton(title: "Cancel", systemImage: nil, filled: false) {
                        onDone()
                    }
                }
            }
            .padding(18)
        }
        .background(ShellPalette.background.ignoresSafeArea())
        .presentationDragIndicator(.visible)
        .onAppear {
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
        host.refreshLibrary()
        host.libraryNotice = "\(playlist.deletingPathExtension().lastPathComponent) is one game "
            + "now, with \(discs.count) discs."
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
    /// Bumped after a language change, so the menu's label reads the new choice.
    @State private var languageVersion = 0

    /// Patches go on one game file. A disc's sheet, a playlist or an archive isn't one.
    private static let unpatchable: Set<String> = [
        "zip", "7z", "m3u", "cue", "gdi", "chd", "ccd", "cso", "pbp", "mds", "toc", "cdi",
    ]

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            SettingsButton(title: "Clean start", role: .normal) {
                let target = entry
                host.detailEntry = nil
                host.launchCleanStart(entry: target)
            }
            SettingsNote("No cheats, no auto-save and none of this game's own settings, just this "
                         + "once. Handy when a game won't start. Your saves stay as they are.")

            HStack(spacing: 10) {
                SettingsButton(title: "Share the game", role: .normal) {
                    GameShare.present(GameShare.files(for: entry))
                }
                SettingsButton(title: "Copy its link", role: .normal) {
                    copyLink()
                }
            }

            if !Self.unpatchable.contains(entry.ext) {
                SettingsButton(title: "Patch this game...", role: .normal) {
                    patch()
                }
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
        .background(ShellPalette.surface, in: RoundedRectangle(cornerRadius: 12))
        .sheet(isPresented: $showingDiscs) {
            MultiDiscSheet(entry: entry, host: host) { showingDiscs = false }
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
            do {
                let format = try importApplyPatch(romPath: rom.path, patchPath: picked.path,
                                                  outPath: out.path)
                FeedbackCenter.step("patched \(game.name) with \(picked.lastPathComponent) "
                                    + "(\(format)) into \(out.lastPathComponent)")
                host.refreshLibrary()
                line = "Done. \(out.deletingPathExtension().lastPathComponent) is in your library. "
                    + "The original is unchanged."
            } catch {
                line = "\(error)"
            }
            try? FileManager.default.removeItem(at: picked)
        }
    }

    // MARK: Language

    /// The console's language for this game, where its emulator has a language setting. It
    /// shows once that emulator has run a game on this phone, because that run is how Continuum
    /// learns the emulator's settings.
    @ViewBuilder
    private var languagePicker: some View {
        if let core = CoreCatalog.core(forPath: entry.path, ps1CoreId: host.ps1CoreChoice.coreId),
           let option = host.engine.coreLanguageOption(coreId: core.coreId) {
            let _ = languageVersion
            let current = host.engine.gameOptionValue(coreId: core.coreId, game: entry.name,
                                                      key: option.key) ?? ""
            SettingsLabel("Console language for this game")
            Menu {
                Button("Same as the emulator") {
                    setLanguage(nil, core: core.coreId, key: option.key)
                }
                ForEach(option.values, id: \.value) { choice in
                    Button(choice.label) {
                        setLanguage(choice.value, core: core.coreId, key: option.key)
                    }
                }
            } label: {
                HStack {
                    Text(current.isEmpty
                         ? "Same as the emulator"
                         : (option.values.first(where: { $0.value == current })?.label ?? current))
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

    private func setLanguage(_ value: String?, core: String, key: String) {
        do {
            try host.engine.setGameOptionValue(coreId: core, game: entry.name, key: key,
                                               value: value)
            line = value == nil
                ? "Back to the emulator's own language."
                : "Set. It applies the next time this game starts."
        } catch {
            line = "\(error)"
        }
        languageVersion += 1
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

    @ObservedObject private var prefs = LibraryPrefs.shared
    @ObservedObject private var selection = LibrarySelection.shared
    /// Built when the games or the order change, not on every redraw (the Home shelves' rule).
    @State private var groups: [ConsoleGroup] = []

    private static let indexRow: CGFloat = 17

    var body: some View {
        let size = host.libraryCardSize
        let showsIndex = groups.count >= 3
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 10, pinnedViews: [.sectionHeaders]) {
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
        .onAppear { rebuild() }
        .onChange(of: entries) { _ in rebuild() }
        .onChange(of: prefs.consoleOrder) { _ in rebuild() }
    }

    private func rebuild() {
        groups = Self.group(entries, order: prefs)
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

    private func header(_ group: ConsoleGroup) -> some View {
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
        .padding(.horizontal, 16)
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
