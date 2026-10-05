// Gameplay manuals: one PDF per game, read with PDFKit from the game card and from the player.
//
// Manuals live in Documents/Manuals, so they are visible in the Files app and can be dropped in
// by hand. A game finds its manual two ways: one attached from its card (remembered by game
// filename), or one whose filename matches the game's, compared without extension, case, region
// tags or punctuation, so "Super Metroid (USA).sfc" finds "Super Metroid.pdf".

import Foundation
import PDFKit
import SwiftUI
import UIKit

enum GameplayManuals {
    private static let attachedKey = "manuals.attached.v1"
    /// The PDFs this app WROTE for an attachment: file name to the game it was written for.
    ///
    /// Its own key beside the attachment map rather than folded into it, because that map syncs
    /// between phones as a plain [game: file] dictionary and other builds read it in that shape.
    /// This one is what lets `forget` tell a copy the app made from a PDF the user put in the folder
    /// themselves, and a phone without it (another phone, or an attachment made by an older build)
    /// simply keeps the file, which is the safe way to be unsure.
    private static let writtenKey = "manuals.written.v1"

    /// Documents/Manuals, created on first use.
    static func folder() -> URL? {
        guard let docs = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first else {
            return nil
        }
        let dir = docs.appendingPathComponent("Manuals", isDirectory: true)
        if !FileManager.default.fileExists(atPath: dir.path) {
            try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        }
        return dir
    }

    /// "Super Metroid (USA) [!]" -> "supermetroid".
    static func matchKey(_ name: String) -> String {
        var stem = (name as NSString).deletingPathExtension
        for (open, close) in [("(", ")"), ("[", "]")] {
            while let start = stem.range(of: open),
                  let end = stem.range(of: close, range: start.upperBound..<stem.endIndex) {
                stem.removeSubrange(start.lowerBound..<end.upperBound)
            }
        }
        return stem.lowercased().filter { $0.isLetter || $0.isNumber }
    }

    private static func attached() -> [String: String] {
        (UserDefaults.standard.dictionary(forKey: attachedKey) as? [String: String]) ?? [:]
    }

    private static func writtenFiles() -> [String: String] {
        (UserDefaults.standard.dictionary(forKey: writtenKey) as? [String: String]) ?? [:]
    }

    /// Whether the file at `url` holds exactly `data`. The size is compared first, so a different
    /// PDF is told apart without reading it.
    private static func holds(_ data: Data, at url: URL) -> Bool {
        guard let size = (try? url.resourceValues(forKeys: [.fileSizeKey]))?.fileSize,
              size == data.count,
              let existing = try? Data(contentsOf: url, options: .mappedIfSafe) else {
            return false
        }
        return existing == data
    }

    /// The first of `preferred`, "<stem> - <suffix>.pdf", "<stem> - <suffix> 2.pdf", " 3" and so on
    /// that `usable` accepts, or nil if a hundred of them are all taken.
    ///
    /// THE SUFFIX IS A WORD, NOT A BARE NUMBER, because a manual is found by its name with the
    /// punctuation taken out: a second "Mega Man.pdf" kept as "Mega Man 2.pdf" would open for
    /// Mega Man 2, and one kept as "Mega Man (2).pdf" would tie with the first for Mega Man itself,
    /// since bracketed tags are ignored. A name that ends in a word is found by neither.
    private static func firstUsableName(preferred: String, stem: String, suffix: String,
                                        usable: (String) -> Bool) -> String? {
        if usable(preferred) { return preferred }
        var base = stem.isEmpty ? "manual" : stem
        // Room for the suffix inside the 255-byte limit on a file name, cut by whole characters.
        while base.utf8.count > 200 {
            base.removeLast()
        }
        for number in 1...100 {
            let name = number == 1
                ? "\(base) - \(suffix).pdf"
                : "\(base) - \(suffix) \(number).pdf"
            if usable(name) { return name }
        }
        return nil
    }

    /// The manual for a game, or nil.
    static func manual(for entry: LibraryEntry) -> URL? {
        guard let dir = folder() else { return nil }
        if let file = attached()[entry.name] {
            let url = dir.appendingPathComponent(file)
            if FileManager.default.fileExists(atPath: url.path) { return url }
        }
        let key = matchKey(entry.name)
        guard !key.isEmpty,
              let files = try? FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil)
        else { return nil }
        let pdfs = files.filter { $0.pathExtension.lowercased() == "pdf" }
        let gameStem = (entry.name as NSString).deletingPathExtension.lowercased()
        return pdfs.first { $0.deletingPathExtension().lastPathComponent.lowercased() == gameStem }
            ?? pdfs.first { matchKey($0.lastPathComponent) == key }
    }

    /// Copies a PDF into the folder and attaches it to `entry`. Returns the status line.
    static func attach(_ data: Data, named name: String, to entry: LibraryEntry) -> String {
        guard data.starts(with: Array("%PDF".utf8)) else {
            return "\(name) is not a PDF"
        }
        guard let dir = folder() else { return "manual not saved: no Documents folder" }
        let preferred = wifiSafeFileName(name: name) ?? "\(UUID().uuidString).pdf"
        var map = attached()
        // NEVER OVER ANOTHER FILE. Manuals are called "manual.pdf" more often than not, and writing
        // the second game's over the first silently took the first game's manual away. A name is
        // only reused when it already holds these exact bytes and no other game is attached to it;
        // any other clash is kept beside it as "<game> - manual.pdf". Compared without case, because
        // whether the disk tells "Manual.pdf" from "manual.pdf" is not something to bet a manual on.
        let otherGames = Set(map.filter { $0.key != entry.name }.map { $0.value.lowercased() })
        let stem = (entry.name as NSString).deletingPathExtension
        let file = firstUsableName(preferred: preferred, stem: stem, suffix: "manual") { candidate in
            guard !otherGames.contains(candidate.lowercased()) else { return false }
            let url = dir.appendingPathComponent(candidate)
            return !FileManager.default.fileExists(atPath: url.path) || holds(data, at: url)
        } ?? "\(UUID().uuidString).pdf"
        let target = dir.appendingPathComponent(file)
        if !FileManager.default.fileExists(atPath: target.path) {
            do {
                try data.write(to: target, options: .atomic)
            } catch {
                return "manual not saved: \(error.localizedDescription)"
            }
            // Recorded only when this app made the file, so `forget` can tell it from a PDF that
            // was already in the folder and merely turned out to be the same one.
            var made = writtenFiles()
            made[file] = entry.name
            UserDefaults.standard.set(made, forKey: writtenKey)
        }
        map[entry.name] = file
        UserDefaults.standard.set(map, forKey: attachedKey)
        if file == preferred {
            return "manual \(file) attached to \(entry.name)"
        }
        return "manual \(name) attached to \(entry.name), saved as \(file) so the \(preferred) "
            + "already in Manuals is left as it was"
    }

    static func detach(from entry: LibraryEntry) {
        var map = attached()
        map.removeValue(forKey: entry.name)
        UserDefaults.standard.set(map, forKey: attachedKey)
    }

    /// A PDF that arrived through any import method: kept in the folder, where it matches by name.
    ///
    /// NEVER OVER ANOTHER FILE, for the reason `attach` gives: the PDF already there under this name
    /// may be some game's attached manual, and deleting it to make room silently took it away. The
    /// one already there is kept and the new one goes beside it, and because a manual is found by
    /// its name, the status line says which name it got and what that means.
    static func importLoose(_ url: URL) -> String {
        guard let dir = folder() else { return "manual not saved: no Documents folder" }
        let fm = FileManager.default
        let name = url.lastPathComponent
        let stem = (name as NSString).deletingPathExtension
        // A name already holding these exact bytes counts as free, so importing the same PDF twice
        // finds the copy it made the first time instead of making another.
        let file = firstUsableName(preferred: name, stem: stem, suffix: "copy") { candidate in
            let candidateURL = dir.appendingPathComponent(candidate)
            return !fm.fileExists(atPath: candidateURL.path)
                || fm.contentsEqual(atPath: url.path, andPath: candidateURL.path)
        } ?? "\(UUID().uuidString).pdf"
        let target = dir.appendingPathComponent(file)
        if fm.fileExists(atPath: target.path) {
            return file == name
                ? "manual \(name) is already in Manuals; it opens for the game with the same name"
                : "manual \(name) is already in Manuals as \(file); attach it from the game's card, "
                    + "or rename it in the Files app, to have a game open it"
        }
        do {
            try fm.copyItem(at: url, to: target)
        } catch {
            return "manual \(name) not saved: \(error.localizedDescription)"
        }
        if file == name {
            return "manual \(name) saved in Manuals; it opens for the game with the same name"
        }
        return "manual \(name) saved in Manuals as \(file), because a different \(name) is already "
            + "there and was kept; a manual opens for the game whose name it matches, so attach this "
            + "one from the game's card, or rename it in the Files app"
    }

    /// Forgets a deleted game's manual. Called by the host when the user deletes the game.
    ///
    /// THE ATTACHMENT ALWAYS GOES. It is keyed by the game's file name, so leaving it would hand
    /// this manual to the next game imported under that name.
    ///
    /// THE PDF GOES ONLY WHEN THIS APP WROTE IT FOR THIS GAME AND NO OTHER GAME IS ATTACHED TO IT.
    /// A PDF the user put in the folder themselves is theirs, and so is one the app cannot account
    /// for, such as an attachment made by an older build or synced from another phone: when in
    /// doubt the file is kept, because a stray PDF costs a tap in the Files app to delete and a lost
    /// manual cannot be undone.
    static func forget(_ entry: LibraryEntry) {
        var map = attached()
        guard let file = map.removeValue(forKey: entry.name) else { return }
        UserDefaults.standard.set(map, forKey: attachedKey)

        var made = writtenFiles()
        guard made[file] == entry.name else { return }
        // The record goes whatever happens to the file: with this game gone, nothing says the PDF
        // belongs to one game alone any more, so from here on it is kept like the user's own.
        made.removeValue(forKey: file)
        UserDefaults.standard.set(made, forKey: writtenKey)

        let sharedWithAnother = map.values.contains { $0.lowercased() == file.lowercased() }
        guard !sharedWithAnother, !file.contains("/"), let dir = folder() else { return }
        try? FileManager.default.removeItem(at: dir.appendingPathComponent(file))
    }
}

// MARK: - Viewer

struct PDFKitView: UIViewRepresentable {
    let url: URL

    func makeUIView(context: Context) -> PDFView {
        let view = PDFView()
        view.autoScales = true
        view.displayMode = .singlePageContinuous
        view.backgroundColor = .black
        view.document = PDFDocument(url: url)
        return view
    }

    func updateUIView(_ view: PDFView, context: Context) {
        if view.document?.documentURL != url {
            view.document = PDFDocument(url: url)
        }
    }
}

struct ManualViewer: View {
    let url: URL
    let title: String
    let onDone: () -> Void

    var body: some View {
        NavigationView {
            Group {
                if PDFDocument(url: url) != nil {
                    PDFKitView(url: url).ignoresSafeArea(edges: .bottom)
                } else {
                    Text("\(url.lastPathComponent) could not be opened as a PDF.")
                        .foregroundStyle(.secondary)
                        .padding()
                }
            }
            .navigationTitle(title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button("Done", action: onDone) }
            }
        }
        .navigationViewStyle(.stack)
        .preferredColorScheme(.dark)
    }
}

// MARK: - The host's entry points

extension EngineHost {
    /// Opens the running game's manual over the player, pausing the game while it is read.
    /// Returns a plain status line. Called by the skins worker's button dispatcher.
    @MainActor func showGameplayManual() -> String {
        guard let entry = activeEntry else {
            let text = "manual: no game is running"
            status = text
            return text
        }
        guard let url = GameplayManuals.manual(for: entry) else {
            let stem = (entry.name as NSString).deletingPathExtension
            let text = "no manual for \(entry.name): attach a PDF from the game's card, or put "
                + "\(stem).pdf in Continuum's Manuals folder"
            status = text
            return text
        }
        let text = presentManual(url, for: entry)
        status = text
        return text
    }

    /// Presents a manual modally. Pauses a running game and resumes it on Done when this paused it.
    @MainActor func presentManual(_ url: URL, for entry: LibraryEntry) -> String {
        guard let presenter = Self.topmostViewController() else {
            return "manual: there is no screen to show it on"
        }
        let pausedHere = running && !paused
        if pausedHere { togglePause() }
        var controller: UIViewController?
        let viewer = ManualViewer(url: url, title: (entry.name as NSString).deletingPathExtension) { [weak self] in
            controller?.dismiss(animated: true)
            if pausedHere, let self, self.running, self.paused { self.togglePause() }
        }
        let hosting = UIHostingController(rootView: viewer)
        hosting.modalPresentationStyle = .fullScreen
        controller = hosting
        presenter.present(hosting, animated: true)
        return "manual: \(url.lastPathComponent)"
    }
}

/// The manual row on a game's card.
struct GameplayManualBlock: View {
    let entry: LibraryEntry
    @ObservedObject var host: EngineHost
    @State private var line = ""
    @State private var epoch = 0

    var body: some View {
        let manual = { _ = epoch; return GameplayManuals.manual(for: entry) }()
        VStack(alignment: .leading, spacing: 10) {
            Text("MANUAL")
                .font(.system(size: 11, weight: .bold))
                .tracking(1.6)
                .foregroundStyle(ShellPalette.secondaryText)
            if let manual {
                SettingsButton(title: "Read the manual (\(manual.lastPathComponent))", role: .normal) {
                    line = host.presentManual(manual, for: entry)
                }
            }
            SettingsButton(title: manual == nil ? "Attach a PDF manual" : "Attach a different PDF", role: .normal) {
                FilePicker.shared.present(onPick: { data, name in
                    line = GameplayManuals.attach(data, named: name, to: entry)
                    host.status = line
                    epoch += 1
                }, onFailure: { reason in
                    line = "manual not attached: \(reason)"
                })
            }
            SettingsNote(line.isEmpty
                ? "A PDF in Continuum's Manuals folder with the game's name is found by itself."
                : line)
        }
    }
}
