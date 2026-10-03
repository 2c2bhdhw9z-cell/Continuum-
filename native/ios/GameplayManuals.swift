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
        let file = wifiSafeFileName(name: name) ?? "\(UUID().uuidString).pdf"
        do {
            try data.write(to: dir.appendingPathComponent(file), options: .atomic)
        } catch {
            return "manual not saved: \(error.localizedDescription)"
        }
        var map = attached()
        map[entry.name] = file
        UserDefaults.standard.set(map, forKey: attachedKey)
        return "manual \(file) attached to \(entry.name)"
    }

    static func detach(from entry: LibraryEntry) {
        var map = attached()
        map.removeValue(forKey: entry.name)
        UserDefaults.standard.set(map, forKey: attachedKey)
    }

    /// A PDF that arrived through any import method: kept in the folder, where it matches by name.
    static func importLoose(_ url: URL) -> String {
        guard let dir = folder() else { return "manual not saved: no Documents folder" }
        let target = dir.appendingPathComponent(url.lastPathComponent)
        do {
            try? FileManager.default.removeItem(at: target)
            try FileManager.default.copyItem(at: url, to: target)
        } catch {
            return "manual \(url.lastPathComponent) not saved: \(error.localizedDescription)"
        }
        return "manual \(url.lastPathComponent) saved in Manuals; it opens for the game with the same name"
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
