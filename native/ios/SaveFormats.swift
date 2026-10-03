// Save files in other emulators' formats (Manic EMU's list): import and export per system.
//
// The conversion is all in Rust (crates/emulator-bridge/src/import/saves.rs): DeSmuME's .dsv
// footer stripped for melonDS, PlayStation memory card headers (.gme .vmp .vgs) removed, N64
// .eep/.sra/.fla/.mpk placed into the one .srm the core writes, and so on. This file only moves
// the bytes to where they belong:
//
//   * a BATTERY save goes through SaveStates.importBatterySave, the existing path, so the old save
//     is kept as .srm.bak and a running game gets the bytes in its save RAM at once;
//   * a CORE FILE (Dreamcast VMU, Saturn .bkr, arcade NVRAM) is written under the save directory
//     the cores are given (Application Support), the old one kept as .bak beside it;
//   * a FOLDER (PSP SAVEDATA, 3DS) arrives as a zip and is unpacked into the core's folder, with
//     every entry that would escape it refused in Rust.

import Foundation
import SwiftUI
import UIKit

enum SaveFolders {
    /// The directory every core is handed as its save directory (see EngineHost.systemDirectory).
    static func saveDirectory() -> URL? {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
    }
}

extension SaveStates {
    /// Imports a save file of any supported format for `entry`. Returns the status line.
    @discardableResult
    func importSaveFile(_ data: Data, named name: String, for entry: LibraryEntry,
                        system: String) -> String {
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
            do {
                try files.createDirectory(at: target.deletingLastPathComponent(),
                                          withIntermediateDirectories: true)
                if files.fileExists(atPath: target.path) {
                    let backup = target.appendingPathExtension("bak")
                    try? files.removeItem(at: backup)
                    try? files.copyItem(at: target, to: backup)
                }
                try converted.data.write(to: target, options: .atomic)
            } catch {
                let text = "save import failed for \(entry.name): \(error.localizedDescription)"
                reportOnly(text)
                return text
            }
            let text = "imported \(name) for \(entry.name) as \(converted.relativePath) "
                + "(\(converted.note)); the old one is kept as .bak. Restart the game to read it"
            reportOnly(text)
            return text
        case .folder:
            guard let root = SaveFolders.saveDirectory() else {
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
                let written = try importArchiveExtractTree(path: zip.path, destDir: dest.path)
                let text = "unpacked \(written.count) save file(s) from \(name) into "
                    + "\(converted.relativePath). Restart the game to read them"
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
        let stem = (entry.name as NSString).deletingPathExtension
        let location = saveFileLocation(system: system, gameStem: stem)
        guard let outDir = SaveStateDisk.exportDirectory() else {
            reportOnly("save export failed: no temporary directory")
            return nil
        }
        let safeStem = stem.replacingOccurrences(of: "/", with: "_")
        switch location.kind {
        case .folder:
            guard let root = SaveFolders.saveDirectory() else { return nil }
            let folder = root.appendingPathComponent(location.relativePath, isDirectory: true)
            guard FileManager.default.fileExists(atPath: folder.path) else {
                reportOnly("\(entry.name) has no saves yet in \(location.relativePath)")
                return nil
            }
            let url = outDir.appendingPathComponent("\(safeStem) saves.zip")
            do {
                let count = try importZipDirectory(dir: folder.path, outPath: url.path)
                reportOnly("exported \(count) save file(s) from \(location.relativePath) as a zip")
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
            ForEach(saveFileFormats(system: system ?? ""), id: \.self) { format in
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
