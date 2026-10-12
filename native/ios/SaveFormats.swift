// Save files in other emulators' formats (Manic EMU's list): import and export per system.
//
// The conversion is all in Rust (crates/emulator-bridge/src/import/saves.rs): DeSmuME's .dsv
// footer stripped for melonDS, PlayStation memory card headers (.gme .vmp .vgs) removed, N64
// .eep/.sra/.fla/.mpk placed into the one .srm the core writes, and so on. This file only moves
// the bytes to where they belong:
//
//   * a BATTERY save goes through SaveStates.importBatterySave, the existing path, so a running
//     game gets the bytes in its save RAM at once;
//   * a CORE FILE (Dreamcast VMU, Saturn .bkr) is written under the save directory
//     the cores are given (Application Support);
//   * a FOLDER (PSP SAVEDATA, 3DS) arrives as a zip and is unpacked into the core's folder, with
//     every entry that would escape it refused in Rust.
//
// Whichever it is, the save it replaces is first copied to Files, Continuum, Older copies
// (`SaveFolders.keepBeforeImport`, Rust's `saves::keep_before_import`), and when that copy can't
// be made the import stops with nothing changed. An import never costs the player a save.

import Foundation
import SwiftUI
import UIKit

enum SaveFolders {
    /// The directory every core is handed as its save directory (see EngineHost.systemDirectory).
    static func saveDirectory() -> URL? {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
    }

    /// Files, Continuum, Older copies: where an import keeps the save it replaces, the same folder
    /// a restore uses for the copies it didn't keep.
    static func olderCopiesDirectory() -> URL? {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first?
            .appendingPathComponent(CloudSync.olderCopiesFolderName, isDirectory: true)
    }

    /// Keeps the save at `url` in Older copies (under `folder`) before an import writes `incoming`
    /// over it. Returns the copy, or nil when nothing would have been lost. Throws when no copy
    /// could be made, and then the import must not go ahead.
    static func keepBeforeImport(_ url: URL, incoming: Data, folder: String) throws -> URL? {
        guard let older = olderCopiesDirectory() else {
            throw CocoaError(.fileNoSuchFile)
        }
        let kept: String? = try saveKeepBeforeImport(path: url.path, incoming: incoming,
                                                     olderDir: older.path, folder: folder)
        return kept.map { URL(fileURLWithPath: $0) }
    }

    /// The end of an import's line when the old save was kept.
    static let keptLine = "Your old save is in Files, Continuum, Older copies."

    /// The line when the old save couldn't be kept, so nothing was changed.
    static let notKeptLine = "Your old save couldn't be copied first, so nothing was changed."
}

extension SaveStates {
    /// Imports a save file of any supported format for `entry`. Returns the status line.
    @discardableResult
    func importSaveFile(_ data: Data, named name: String, for entry: LibraryEntry,
                        system: String) -> String {
        // Flash and J2ME saves are the bundled players' own (WebPlayers.swift), Manic's .json and
        // .J2meJS.srm, not a core's battery save.
        if let kind = WebPlayerKind(systemId: system), let host = EngineHost.shared {
            let text = host.importWebPlayerSave(data, named: name, for: entry, kind: kind)
            reportOnly(text)
            return text
        }
        let gameId = Self.gameId(for: entry)
        let stem = (entry.name as NSString).deletingPathExtension
        let current = SaveStateDisk.batteryURL(gameId: gameId)
            .flatMap { try? Data(contentsOf: $0) } ?? Data()
        let converted: SaveFileImport
        do {
            converted = try saveFileImport(system: system, fileName: name, data: data,
                                           current: current, gameStem: stem)
        } catch {
            let text = "save import refused for \(entry.name): \(error)"
            reportOnly(text)
            return text
        }
        switch converted.kind {
        case .battery:
            let ok = importBatterySave(converted.data, named: name, for: entry)
            let text = ok ? "\(name) (\(converted.note)): \(line)" : line
            if ok { reportOnly(text) }
            return text
        case .coreFile:
            guard let root = SaveFolders.saveDirectory() else {
                let text = "save import failed: no Application Support directory"
                reportOnly(text)
                return text
            }
            let target = root.appendingPathComponent(converted.relativePath)
            let files = FileManager.default
            // The save it replaces goes to Older copies first; if that fails, nothing changes.
            let kept: URL?
            do {
                kept = try SaveFolders.keepBeforeImport(
                    target, incoming: converted.data,
                    folder: (converted.relativePath as NSString).deletingLastPathComponent)
            } catch {
                let text = "save import stopped for \(entry.name). \(SaveFolders.notKeptLine)"
                reportOnly(text)
                return text
            }
            do {
                try files.createDirectory(at: target.deletingLastPathComponent(),
                                          withIntermediateDirectories: true)
                try converted.data.write(to: target, options: .atomic)
            } catch {
                let text = "save import failed for \(entry.name): \(error.localizedDescription)"
                reportOnly(text)
                return text
            }
            let text = "imported \(name) for \(entry.name) as \(converted.relativePath) "
                + "(\(converted.note)). Restart the game to read it."
                + (kept != nil ? " \(SaveFolders.keptLine)" : "")
            reportOnly(text)
            return text
        case .folder:
            guard let root = SaveFolders.saveDirectory(), let older = SaveFolders.olderCopiesDirectory() else {
                let text = "save import failed: no Application Support directory"
                reportOnly(text)
                return text
            }
            let zip = FileManager.default.temporaryDirectory
                .appendingPathComponent("ContinuumSaveZip-\(UUID().uuidString).zip")
            defer { try? FileManager.default.removeItem(at: zip) }
            do {
                try converted.data.write(to: zip)
                let dest = root.appendingPathComponent(converted.relativePath, isDirectory: true)
                // Every file it replaces is kept in Older copies first.
                let done = try saveFolderImport(archivePath: zip.path, destDir: dest.path,
                                                olderDir: older.path, folder: converted.relativePath)
                let count = done.written.count
                let text = "unpacked \(count) save \(count == 1 ? "file" : "files") from \(name) into "
                    + "\(converted.relativePath). Restart the game to read them."
                    + (done.kept > 0 ? " \(SaveFolders.keptLine)" : "")
                reportOnly(text)
                return text
            } catch {
                let text = "save folder import failed for \(entry.name): \(error)"
                reportOnly(text)
                return text
            }
        }
    }

    /// The save of `entry` converted to `format`, as a file for the share sheet.
    func exportSaveFile(for entry: LibraryEntry, system: String, format: String) -> URL? {
        if let kind = WebPlayerKind(systemId: system), let host = EngineHost.shared {
            let (url, text) = host.exportWebPlayerSave(for: entry, kind: kind)
            reportOnly(text)
            return url
        }
        let stem = (entry.name as NSString).deletingPathExtension
        let location = saveFileLocation(system: system, gameStem: stem)
        let safeStem = stem.replacingOccurrences(of: "/", with: "_")
        // The export folder is asked for in each branch, once there is something to write. Asked
        // for up front it was asked for twice on the battery path (`exportBatteryFile` asks too),
        // and while every ask emptied the folder, that was one export deleting another's file.
        // Each ask now gets a folder of its own (see `SaveStateDisk.exportDirectory`).
        switch location.kind {
        case .folder:
            guard let root = SaveFolders.saveDirectory() else { return nil }
            let folder = root.appendingPathComponent(location.relativePath, isDirectory: true)
            guard FileManager.default.fileExists(atPath: folder.path) else {
                reportOnly("\(entry.name) has no saves yet in \(location.relativePath)")
                return nil
            }
            guard let outDir = SaveStateDisk.exportDirectory() else {
                reportOnly("save export failed: no temporary directory")
                return nil
            }
            let url = outDir.appendingPathComponent("\(safeStem) saves.zip")
            do {
                let count = try importZipDirectory(dir: folder.path, outPath: url.path)
                reportOnly("exported \(count) save \(count == 1 ? "file" : "files") from \(location.relativePath) as a zip")
                return url
            } catch {
                reportOnly("save export failed: \(error)")
                return nil
            }
        case .coreFile, .battery:
            var stored = Data()
            if location.kind == .coreFile {
                if let root = SaveFolders.saveDirectory() {
                    stored = (try? Data(contentsOf: root.appendingPathComponent(location.relativePath))) ?? Data()
                }
            } else if let srm = exportBatteryFile(for: entry) {
                // Read live from a running game by the existing exporter.
                stored = (try? Data(contentsOf: srm)) ?? Data()
            }
            guard !stored.isEmpty else {
                reportOnly("\(entry.name) has no save yet. A save is made from the game's own menu")
                return nil
            }
            guard let outDir = SaveStateDisk.exportDirectory() else {
                reportOnly("save export failed: no temporary directory")
                return nil
            }
            do {
                let bytes = try saveFileExport(system: system, format: format, stored: stored)
                let url = outDir.appendingPathComponent("\(safeStem).\(format)")
                try bytes.write(to: url, options: .atomic)
                reportOnly("exported the save for \(entry.name) as .\(format), "
                           + "\(SaveStates.byteText(Int64(bytes.count)))")
                return url
            } catch {
                reportOnly("save export as .\(format) failed: \(error)")
                return nil
            }
        }
    }
}

/// The two buttons on the save slots screen: import any save format, export as a chosen format.
struct SaveFileButtons: View {
    let entry: LibraryEntry
    @ObservedObject var host: EngineHost
    @ObservedObject var saveStates: SaveStates
    @State private var choosingFormat = false

    private var system: String? { host.importCenter.systemId(for: entry) }

    var body: some View {
        SettingsButton(title: "Import a save file (.sav .dsv .mcr .eep .vmu and more)", role: .normal) {
            guard let system else {
                saveStates.reportOnly("\(entry.name) has no system, so its save format is unknown")
                return
            }
            FilePicker.shared.present(onPick: { data, name in
                saveStates.importSaveFile(data, named: name, for: entry, system: system)
            }, onFailure: { reason in
                saveStates.reportOnly("save import failed: \(reason)")
            })
        }
        SettingsButton(title: "Export the save as...", role: .normal) {
            choosingFormat = true
        }
        .confirmationDialog("Export the save as", isPresented: $choosingFormat, titleVisibility: .visible) {
            ForEach(EngineHost.webPlayerSaveFormats(system: system ?? "")
                    ?? saveFileFormats(system: system ?? ""), id: \.self) { format in
                Button(".\(format)") {
                    if let system, let url = saveStates.exportSaveFile(for: entry, system: system, format: format) {
                        FileShare.present(url)
                    }
                }
            }
            Button("Cancel", role: .cancel) {}
        }
    }
}
