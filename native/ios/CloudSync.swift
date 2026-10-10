// Continuum - cloud sync of save states, battery saves, Flash and J2ME saves, cheats, manuals,
// Amiibo, artwork choices and settings.
//
// ## Why a folder and not iCloud
//
// This app is sideloaded and re-signed on the phone, and re-signing usually strips the iCloud and
// CloudKit entitlements. So nothing here depends on CloudKit or on an iCloud container. The user
// picks ONE folder through the document picker (iCloud Drive, Google Drive, Dropbox, or anything
// else that appears in Files), the app keeps a security-scoped bookmark to it, and syncs a
// `Continuum Sync` subfolder inside it. Every read and write there goes through NSFileCoordinator,
// which is what a File Provider (the thing behind each cloud in Files) needs to see.
//
// ## Where the rules live
//
// In Rust (`crates/emulator-bridge/src/sync`), so Android makes the same decisions: which side
// changed, who wins a conflict, what is merged, what a deletion means, and when a listing looks
// like an unreachable folder rather than a real change. This file only lists folders, copies
// files and reports. The one piece of data shaping here is turning the two record files
// (`SaveStates/index.json`, `Cheats/index.json`) into keyed records for the Rust merge.
//
// ## What is synced, by relative path
//
//   SaveStates/<key>-<slot>.state and SaveStates/index.json   <AppSupport>/SaveStates/
//   Cheats/index.json                                         <AppSupport>/Cheats/index.json
//   BatterySaves/<name>.srm                                   <AppSupport>/BatterySaves/ (engine SAVE_RAM)
//   Battery/<name>.srm (and other save extensions)            loose in <AppSupport>, where the
//                                                             cores write them (it is their
//                                                             save directory)
//   PlayerSaves/<stem>.json, PlayerSaves/<stem>.J2meJS.srm    <AppSupport>/PlayerSaves/ (Flash
//                                                             and J2ME saves, written by the
//                                                             bundled players)
//   Skins/<id>.(pdf|png), Skins/<id>-landscape.(pdf|png),     <AppSupport>/Continuum/Skins/
//   Skins/sounds/<id>.caf, Skins/pieces/<id>/<file>
//   Manuals/<name>.pdf                                        <Documents>/Manuals/
//   Amiibo/<name>.bin                                         <Documents>/Amiibo/
//   Settings/defaults.plist                                   exported from UserDefaults: the
//                                                             `continuum.` keys and three older
//                                                             ones (`includedKeys`), never the
//                                                             per-phone ones (`excludedKeys`)
//   Artwork/choices.json, Artwork/covers/<hash>.cover         exported artwork choices, re-keyed
//                                                             by ROM filename
//
// Kept on this phone on purpose: the RetroAchievements login, the last online-play address,
// microphone and camera consent, and the sync's own bookmark and history. `excludedKeys` and
// `excludedPrefixes` say why for each.
//
// SKINS AND FAVOURITES USED TO BE ON THAT LIST and are not any more, because both exclusions
// rested on something that has been fixed rather than on anything per-phone. Favourites were
// stored by absolute path, which contains the install's container id; they are keyed by file name
// now (`continuum.favourites.v2`). Skins were excluded because their BYTES did not sync, so an
// index arriving from elsewhere would name art the phone did not have; the bytes sync now. Both
// mattered for one reason: this app's owner deletes it before every install, so anything that
// does not come back from the cloud folder is lost on every single build.
//
// Settings and artwork choices are EXPORTED to a staging folder before each sync and IMPORTED at
// the next launch, before any store reads them. Importing into a live app would race the objects
// that already hold those values in memory and write their own copies back.
//
// ## Never the only copy
//
// A conflict keeps both (the loser goes to `Continuum Sync/Conflicts/` with a date). Only a
// deleted save-state slot or cover counts as a deletion: it moves the cloud copy into
// `Continuum Sync/Deleted/<date>/`, and a local copy is only removed after it has been copied
// there. Any other file that goes missing on one side (a battery save, a player save, a manual,
// an Amiibo) is copied back, because losing one by accident costs far more than having to remove
// it from the cloud folder too. Sync never runs while a game is on screen.

import Foundation
import SwiftUI
import UIKit
import UniformTypeIdentifiers

// MARK: - Paths

enum CloudSyncPaths {
    static let remoteFolderName = "Continuum Sync"

    /// File extensions the cores use for battery saves, memory cards and RTC data in their save
    /// directory. BIOS files live in the same directory and are deliberately not in this list.
    static let batteryExtensions: Set<String> = [
        "srm", "sav", "rtc", "eep", "sra", "fla", "mpk", "mcr", "mcd", "dsv", "nv", "ldci", "bcr",
    ]

    static func support() -> URL? {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
    }

    /// Where imported skins keep their files. Must stay in step with
    /// `EngineHost.skinsDirectory()`, which is the only other place this path is spelled out.
    static func skinsRoot() -> URL? {
        support()?.appendingPathComponent("Continuum/Skins", isDirectory: true)
    }

    /// The switch behind the `Games/` category. Off unless the user turned it on.
    ///
    /// Read from `UserDefaults` here rather than passed in, because `listLocal` and `localURL`
    /// are static and are called from the sync worker off the main actor. A missing key is false,
    /// which is what makes this opt-in for every existing install.
    ///
    /// NOT under `continuum.sync.`: that prefix is never exported, so the switch used to be wiped
    /// with the app and never came back from the cloud, and a restore brought no games back.
    static let includeGamesKey = "continuum.backup.includeGames.v1"
    private static let legacyIncludeGamesKey = "continuum.sync.includeGames.v1"
    static func gamesAreIncluded() -> Bool {
        UserDefaults.standard.bool(forKey: includeGamesKey)
    }

    /// Carries a switch stored under the old name over to the new one, once, then reads it.
    static func migratedGamesSwitch() -> Bool {
        let defaults = UserDefaults.standard
        if defaults.object(forKey: legacyIncludeGamesKey) != nil {
            if defaults.object(forKey: includeGamesKey) == nil {
                defaults.set(defaults.bool(forKey: legacyIncludeGamesKey), forKey: includeGamesKey)
            }
            defaults.removeObject(forKey: legacyIncludeGamesKey)
        }
        return gamesAreIncluded()
    }

    /// Is this file in Documents a game or part of one?
    ///
    /// Deliberately WIDER than `CoreCatalog.isLaunchable`: that answers "would this be a row in
    /// the Library", and a disc track must sync without being one. Restoring a `.cue` without its
    /// `.bin` tracks would put back a game that cannot load, which is worse than not restoring it.
    static func isSyncableGameFile(_ name: String) -> Bool {
        let ext = (name as NSString).pathExtension.lowercased()
        guard !ext.isEmpty, CoreCatalog.syncableGameExtensions.contains(ext) else { return false }
        // Firmware is not a game. Same list the Library uses to keep BIOS files out of itself.
        return !CoreCatalog.isFirmwareName(name)
    }

    /// The app's Documents folder, where manuals and Amiibo live so they show in Files.
    static func documents() -> URL? {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first
    }

    /// The bundled players' save extensions (`WebPlayerKind.saveExtension`), with their dot.
    /// Spelled out here because the worker runs off the main actor and needs no player type.
    static let playerSaveSuffixes = [".json", ".J2meJS.srm"]

    /// `<AppSupport>/Sync`, the sync's own bookkeeping. A directory, so the battery scan of the
    /// Application Support root (files only) never sees it.
    static func syncDirectory() -> URL? {
        guard let support = support() else { return nil }
        let dir = support.appendingPathComponent("Sync", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    static func staging() -> URL? { syncDirectory()?.appendingPathComponent("Staging", isDirectory: true) }
    static func baseCopies() -> URL? { syncDirectory()?.appendingPathComponent("Base", isDirectory: true) }
    static func scratch() -> URL? { syncDirectory()?.appendingPathComponent("Scratch", isDirectory: true) }
    static func manifestURL() -> URL? { syncDirectory()?.appendingPathComponent("manifest.txt") }

    static let settingsPath = "Settings/defaults.plist"
    static let artworkChoicesPath = "Artwork/choices.json"

    /// Where a sync path lives on this device, or nil when the path is not one this app syncs.
    /// Remote files that map to nil are never downloaded.
    static func localURL(for relative: String) -> URL? {
        guard syncIsValidPath(path: relative), let support = support() else { return nil }
        let parts = relative.split(separator: "/").map(String.init)
        switch (parts.first, parts.count) {
        case ("SaveStates", 2):
            let name = parts[1]
            // `.png` is a slot thumbnail, where a build writes them beside the payloads.
            guard name == "index.json" || name.hasSuffix(".state") || name.hasSuffix(".png") else {
                return nil
            }
            return support.appendingPathComponent("SaveStates", isDirectory: true)
                .appendingPathComponent(name)
        case ("Cheats", 2) where parts[1] == "index.json":
            return support.appendingPathComponent("Cheats", isDirectory: true)
                .appendingPathComponent("index.json")
        case ("Battery", 2):
            let name = parts[1]
            guard batteryExtensions.contains((name as NSString).pathExtension.lowercased()) else {
                return nil
            }
            return support.appendingPathComponent(name)
        case ("BatterySaves", 2):
            // Where the engine itself writes SAVE_RAM (`Application Support/BatterySaves/`), as
            // opposed to the files some cores write into their save directory, above.
            let name = parts[1]
            guard batteryExtensions.contains((name as NSString).pathExtension.lowercased()) else {
                return nil
            }
            return support.appendingPathComponent("BatterySaves", isDirectory: true)
                .appendingPathComponent(name)
        case ("PlayerSaves", 2):
            // Flash and J2ME saves, where `EngineHost.webPlayerSaveURL` puts them. Only the save
            // itself: the `.bak` a save import leaves beside it is this phone's own undo.
            let name = parts[1]
            guard playerSaveSuffixes.contains(where: { name.hasSuffix($0) }) else { return nil }
            return support.appendingPathComponent("PlayerSaves", isDirectory: true)
                .appendingPathComponent(name)
        case ("Manuals", 2):
            // `GameplayManuals.folder()`. Only PDFs, the one kind the manual viewer opens.
            let name = parts[1]
            guard (name as NSString).pathExtension.lowercased() == "pdf",
                  let documents = documents() else { return nil }
            return documents.appendingPathComponent("Manuals", isDirectory: true)
                .appendingPathComponent(name)
        case ("Amiibo", 2):
            // `Peripherals.amiiboFolder`. Only `.bin`: the import names every tag that way, and
            // the Amiibo list shows nothing else.
            let name = parts[1]
            guard (name as NSString).pathExtension.lowercased() == "bin",
                  let documents = documents() else { return nil }
            return documents.appendingPathComponent("Amiibo", isDirectory: true)
                .appendingPathComponent(name)
        // THE GAMES THEMSELVES, and the one category that is OFF unless asked for.
        //
        // Everything else here is small: saves, indexes, covers, a PDF. ROMs are not. A shelf of
        // PlayStation discs is tens of gigabytes, and quietly pushing that into somebody's iCloud
        // or Dropbox would be a far worse surprise than the problem it solves. So it is a switch
        // in Settings, off by default, and when it is off a remote `Games/` path maps to nil and
        // is never downloaded either — the opt-in works in both directions.
        //
        // The extension test is `sharedExtensions` plus every per-system one, NOT `isLaunchable`,
        // and that difference is the whole point: a PlayStation `.bin` is a disc TRACK and never
        // a Library row, so `isLaunchable` says no to it, and syncing a `.cue` without its tracks
        // would restore a game that cannot load. Firmware is excluded by name: a BIOS is not a
        // game, and the firmware list is the same one the Library uses to keep BIOS files out.
        case ("Games", 2) where gamesAreIncluded():
            let name = parts[1]
            guard let documents = documents(), isSyncableGameFile(name) else { return nil }
            return documents.appendingPathComponent(name)
        // SKINS. `EngineHost.skinsDirectory()` is `<AppSupport>/Continuum/Skins`, and one skin id
        // owns up to four shapes of file: its art in each orientation, its pieces, and its sound.
        //
        // These used to be left on the phone on purpose, and the skin index keys with them. The
        // reason given was circular: the index could not sync because the BYTES did not sync, so
        // another phone's index would list skins whose files it did not have. Syncing the bytes
        // removes the reason. It also removes the thing the owner hits hardest — they delete the
        // app before every install, so every imported skin had to be imported again, every build.
        //
        // A skin file is never deleted from the other side: `propagates_deletion` in the Rust
        // rules is false for anything outside SaveStates and Artwork/covers, so a skin missing on
        // one phone is copied back rather than removed, same as a manual or an Amiibo.
        case ("Skins", 2):
            // `<id>.pdf`, `<id>.png`, and the `-landscape` variants, which are just part of the
            // name. Nothing else: the skin's own `info.json` is parsed at import and lives in the
            // index, not on disk here.
            let name = parts[1]
            guard ["pdf", "png"].contains((name as NSString).pathExtension.lowercased()) else {
                return nil
            }
            return skinsRoot()?.appendingPathComponent(name)
        case ("Skins", 3) where parts[1] == "sounds":
            // `sounds/<id>.caf`, the button sound a Manic skin can carry.
            guard (parts[2] as NSString).pathExtension.lowercased() == "caf" else { return nil }
            return skinsRoot()?.appendingPathComponent("sounds", isDirectory: true)
                .appendingPathComponent(parts[2])
        case ("Skins", let count) where count >= 4 && parts[1] == "pieces":
            // `pieces/<id>/<file>`, or deeper: a skin can keep its button images in subfolders,
            // and `EngineHost.pieceURL` keeps the "/" in their names. Extensions are whatever the
            // skin author used, so this checks the SHAPE of the path and not the extension;
            // `syncIsValidPath` above has already refused empty parts, `..` and leading dots.
            guard var url = skinsRoot()?.appendingPathComponent("pieces", isDirectory: true)
            else { return nil }
            for (index, part) in parts.enumerated().dropFirst(2) {
                url = url.appendingPathComponent(part, isDirectory: index < count - 1)
            }
            return url
        case ("Settings", 2) where relative == settingsPath:
            return staging()?.appendingPathComponent(relative)
        case ("Artwork", 2) where relative == artworkChoicesPath:
            return staging()?.appendingPathComponent(relative)
        case ("Artwork", 3) where parts[1] == "covers" && parts[2].hasSuffix(".cover"):
            return staging()?.appendingPathComponent(relative)
        default:
            return nil
        }
    }

    static func millis(_ date: Date?) -> Int64 {
        Int64(((date ?? Date(timeIntervalSince1970: 0)).timeIntervalSince1970 * 1000).rounded())
    }

    static func stat(_ url: URL, path: String) -> SyncFileStat? {
        guard let values = try? url.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey,
                                                             .contentModificationDateKey]),
              values.isRegularFile == true else { return nil }
        return SyncFileStat(path: path, size: UInt64(max(0, values.fileSize ?? 0)),
                            mtimeMs: millis(values.contentModificationDate))
    }

    /// Everything this device has to offer, as sync paths.
    static func listLocal() -> [SyncFileStat] {
        guard let support = support() else { return [] }
        let fm = FileManager.default
        var out: [SyncFileStat] = []
        func files(in dir: URL) -> [URL] {
            (try? fm.contentsOfDirectory(at: dir, includingPropertiesForKeys:
                [.isRegularFileKey, .fileSizeKey, .contentModificationDateKey],
                options: [.skipsHiddenFiles])) ?? []
        }
        for url in files(in: support.appendingPathComponent("SaveStates", isDirectory: true)) {
            let path = "SaveStates/\(url.lastPathComponent)"
            if localURL(for: path) != nil, let s = stat(url, path: path) { out.append(s) }
        }
        for url in files(in: support.appendingPathComponent("BatterySaves", isDirectory: true)) {
            let path = "BatterySaves/\(url.lastPathComponent)"
            if localURL(for: path) != nil, let s = stat(url, path: path) { out.append(s) }
        }
        let cheats = support.appendingPathComponent("Cheats/index.json")
        if let s = stat(cheats, path: "Cheats/index.json") { out.append(s) }
        for url in files(in: support) {
            let path = "Battery/\(url.lastPathComponent)"
            if localURL(for: path) != nil, let s = stat(url, path: path) { out.append(s) }
        }
        for url in files(in: support.appendingPathComponent("PlayerSaves", isDirectory: true)) {
            let path = "PlayerSaves/\(url.lastPathComponent)"
            if localURL(for: path) != nil, let s = stat(url, path: path) { out.append(s) }
        }
        // The games, only when the user asked for them. Top level of Documents only: the
        // subfolders there are Manuals and Amiibo, which have their own categories above.
        if gamesAreIncluded(), let documents = documents() {
            for url in files(in: documents) {
                let path = "Games/\(url.lastPathComponent)"
                if localURL(for: path) != nil, let s = stat(url, path: path) { out.append(s) }
            }
        }
        // Skins: the art beside the root, then the two subfolders. `pieces` is one level deeper
        // than anything else that syncs, which is why it is walked rather than listed flat.
        if let skins = skinsRoot() {
            for url in files(in: skins) {
                let path = "Skins/\(url.lastPathComponent)"
                if localURL(for: path) != nil, let s = stat(url, path: path) { out.append(s) }
            }
            for url in files(in: skins.appendingPathComponent("sounds", isDirectory: true)) {
                let path = "Skins/sounds/\(url.lastPathComponent)"
                if localURL(for: path) != nil, let s = stat(url, path: path) { out.append(s) }
            }
            // Walked to any depth, because pieces can sit in subfolders of `pieces/<id>/`.
            let piecesRoot = skins.appendingPathComponent("pieces", isDirectory: true)
            let piecesPath = piecesRoot.standardizedFileURL.path
            if let walker = fm.enumerator(at: piecesRoot, includingPropertiesForKeys:
                [.isRegularFileKey, .fileSizeKey, .contentModificationDateKey],
                options: [.skipsHiddenFiles]) {
                for case let url as URL in walker {
                    let full = url.standardizedFileURL.path
                    guard full.hasPrefix(piecesPath + "/") else { continue }
                    let path = "Skins/pieces/\(full.dropFirst(piecesPath.count + 1))"
                    if localURL(for: path) != nil, let s = stat(url, path: path) { out.append(s) }
                }
            }
        }
        if let documents = documents() {
            for folder in ["Manuals", "Amiibo"] {
                for url in files(in: documents.appendingPathComponent(folder, isDirectory: true)) {
                    let path = "\(folder)/\(url.lastPathComponent)"
                    if localURL(for: path) != nil, let s = stat(url, path: path) { out.append(s) }
                }
            }
        }
        if let staging = staging() {
            for path in [settingsPath, artworkChoicesPath] {
                if let s = stat(staging.appendingPathComponent(path), path: path) { out.append(s) }
            }
            for url in files(in: staging.appendingPathComponent("Artwork/covers", isDirectory: true)) {
                let path = "Artwork/covers/\(url.lastPathComponent)"
                if localURL(for: path) != nil, let s = stat(url, path: path) { out.append(s) }
            }
        }
        return out
    }
}

// MARK: - The worker (off the main thread)

/// What one sync run produced, carried back to the main actor.
struct CloudSyncOutcome: Sendable {
    var uploaded: UInt32 = 0
    var downloaded: UInt32 = 0
    var conflicts: UInt32 = 0
    var merged: UInt32 = 0
    var archived: UInt32 = 0
    var errors: [String] = []
    /// Paths written on this device, so the app knows what to reload.
    var changedLocally: [String] = []
    /// Cloud files not downloaded to the provider yet; retried next time.
    var pendingInCloud = 0
    /// Set when nothing was attempted, with the reason.
    var refusal: String?
    var manifestText: String?
    var finishedAt = Date()
}

private struct CloudSyncError: LocalizedError {
    let text: String
    var errorDescription: String? { text }
}

/// One sync, start to finish, on a background thread. Holds no app state: it is handed the cloud
/// root and the stored manifest and returns an outcome.
final class CloudSyncWorker {
    private let remoteRoot: URL
    private let manifestText: String
    private let fm = FileManager.default
    private let coordinator = NSFileCoordinator(filePresenter: nil)

    init(remoteRoot: URL, manifestText: String) {
        self.remoteRoot = remoteRoot
        self.manifestText = manifestText
    }

    // ---------------------------------------------------------------- coordination

    private func coordinatedRead<T>(_ url: URL, metadataOnly: Bool = false,
                                    _ body: (URL) throws -> T) throws -> T {
        var coordinationError: NSError?
        var result: Result<T, Error>?
        let options: NSFileCoordinator.ReadingOptions = metadataOnly
            ? [.immediatelyAvailableMetadataOnly] : [.withoutChanges]
        coordinator.coordinate(readingItemAt: url, options: options, error: &coordinationError) { u in
            result = Result { try body(u) }
        }
        if let coordinationError { throw coordinationError }
        guard let result else { throw CloudSyncError(text: "the cloud did not hand the file over") }
        return try result.get()
    }

    private func coordinatedWrite(_ url: URL, _ body: (URL) throws -> Void) throws {
        var coordinationError: NSError?
        var result: Result<Void, Error>?
        coordinator.coordinate(writingItemAt: url, options: [.forReplacing],
                               error: &coordinationError) { u in
            result = Result { try body(u) }
        }
        if let coordinationError { throw coordinationError }
        guard let result else { throw CloudSyncError(text: "the cloud did not allow the write") }
        try result.get()
    }

    private func coordinatedMove(from source: URL, to destination: URL) throws {
        var coordinationError: NSError?
        var result: Result<Void, Error>?
        coordinator.coordinate(writingItemAt: source, options: [.forMoving],
                               writingItemAt: destination, options: [.forReplacing],
                               error: &coordinationError) { src, dst in
            result = Result {
                try self.fm.createDirectory(at: dst.deletingLastPathComponent(),
                                            withIntermediateDirectories: true)
                if self.fm.fileExists(atPath: dst.path) { try self.fm.removeItem(at: dst) }
                try self.fm.moveItem(at: src, to: dst)
                self.coordinator.item(at: src, didMoveTo: dst)
            }
        }
        if let coordinationError { throw coordinationError }
        guard let result else { throw CloudSyncError(text: "the cloud did not allow the move") }
        try result.get()
    }

    private func remoteURL(_ relative: String) -> URL {
        remoteRoot.appendingPathComponent(relative)
    }

    // ---------------------------------------------------------------- copying

    /// Local file to a cloud path (data path or aside path).
    ///
    /// COPIED IN BESIDE IT FIRST, THEN RENAMED INTO PLACE, all inside the one coordinated write.
    /// This used to remove the cloud file and then copy, so a copy that failed part way (the
    /// provider out of space, the app suspended mid-copy) left the cloud with NO copy, and the next
    /// sync could read that as a deletion. Now a failed copy leaves the previous cloud copy where
    /// it was. The temporary name starts with a dot, which `syncIsValidPath` never accepts, so one
    /// left behind by a killed app is never listed or synced back.
    private func upload(_ local: URL, to relative: String) throws {
        let destination = remoteURL(relative)
        try coordinatedWrite(destination) { url in
            let folder = url.deletingLastPathComponent()
            try fm.createDirectory(at: folder, withIntermediateDirectories: true)
            let temporary = folder.appendingPathComponent(
                ".\(url.lastPathComponent).\(UUID().uuidString).upload")
            do {
                try fm.copyItem(at: local, to: temporary)
                // Remove, then rename, as `coordinatedMove` does, rather than `replaceItemAt`: a
                // rename within one folder works on every provider in Files, an in-place swap may
                // not, and a swap that fails part way leaves no telling which copy is where. The
                // new copy is already whole, so the cloud is without one only during a rename.
                if fm.fileExists(atPath: url.path) { try fm.removeItem(at: url) }
                try fm.moveItem(at: temporary, to: url)
            } catch {
                try? fm.removeItem(at: temporary)
                throw error
            }
        }
    }

    /// Cloud file to this device, replacing atomically so a reader never sees half a file.
    private func download(_ relative: String, to local: URL) throws {
        guard let scratch = CloudSyncPaths.scratch() else {
            throw CloudSyncError(text: "no Application Support directory")
        }
        try fm.createDirectory(at: scratch, withIntermediateDirectories: true)
        let temporary = scratch.appendingPathComponent(UUID().uuidString)
        // Removed on EVERY way out. On success `place` has already moved it, so this finds
        // nothing; on a failed copy or a failed `place` it is the only thing that removes it, and
        // Sync/Scratch used to keep every one of those for good.
        defer { try? fm.removeItem(at: temporary) }
        try coordinatedRead(remoteURL(relative)) { url in
            try fm.copyItem(at: url, to: temporary)
        }
        try place(temporary, at: local)
    }

    /// Empties Sync/Scratch at the start of a run, for the files a run that never finished left
    /// (the app killed between the copy and `place`). Only one sync runs at a time
    /// (`CloudSync.isSyncing`), so nothing in it belongs to a live download when a run starts.
    private func clearStaleScratch() {
        guard let scratch = CloudSyncPaths.scratch() else { return }
        try? fm.removeItem(at: scratch)
    }

    private func place(_ temporary: URL, at local: URL) throws {
        try fm.createDirectory(at: local.deletingLastPathComponent(), withIntermediateDirectories: true)
        if fm.fileExists(atPath: local.path) {
            _ = try fm.replaceItemAt(local, withItemAt: temporary)
        } else {
            try fm.moveItem(at: temporary, to: local)
        }
    }

    private func identical(local: URL, relative: String) -> Bool {
        (try? coordinatedRead(remoteURL(relative)) { url in
            syncFilesIdentical(pathA: local.path, pathB: url.path)
        }) ?? false
    }

    // ---------------------------------------------------------------- listing

    /// The cloud side as sync paths, plus paths that exist there but are not downloaded yet.
    private func listRemote() throws -> (stats: [SyncFileStat], pending: Set<String>) {
        if !fm.fileExists(atPath: remoteRoot.path) {
            return ([], [])
        }
        return try coordinatedRead(remoteRoot, metadataOnly: true) { root in
            var stats: [SyncFileStat] = []
            var pending: Set<String> = []
            let keys: [URLResourceKey] = [.isRegularFileKey, .isDirectoryKey, .fileSizeKey,
                                          .contentModificationDateKey]
            guard let walker = fm.enumerator(at: root, includingPropertiesForKeys: keys,
                                             options: []) else {
                throw CloudSyncError(text: "the cloud folder could not be listed")
            }
            let rootPath = root.standardizedFileURL.path
            for case let url as URL in walker {
                let full = url.standardizedFileURL.path
                guard full.hasPrefix(rootPath + "/") else { continue }
                var relative = String(full.dropFirst(rootPath.count + 1))
                let first = relative.split(separator: "/").first.map(String.init) ?? ""
                if first == "Conflicts" || first == "Deleted" {
                    walker.skipDescendants()
                    continue
                }
                let name = url.lastPathComponent
                if (try? url.resourceValues(forKeys: [.isDirectoryKey]))?.isDirectory == true {
                    continue
                }
                // An iCloud file that is in the cloud but not on this phone yet appears as a
                // hidden `.Name.icloud` placeholder. Ask for it and leave that path alone this time.
                if name.hasPrefix("."), name.hasSuffix(".icloud") {
                    let logical = String(name.dropFirst().dropLast(".icloud".count))
                    let directory = (relative as NSString).deletingLastPathComponent
                    relative = directory.isEmpty ? logical : "\(directory)/\(logical)"
                    if CloudSyncPaths.localURL(for: relative) != nil {
                        pending.insert(relative)
                        try? fm.startDownloadingUbiquitousItem(
                            at: url.deletingLastPathComponent().appendingPathComponent(logical))
                    }
                    continue
                }
                guard CloudSyncPaths.localURL(for: relative) != nil,
                      let s = CloudSyncPaths.stat(url, path: relative) else { continue }
                stats.append(s)
            }
            return (stats, pending)
        }
    }

    // ---------------------------------------------------------------- record files

    private static func key(for path: String, record: [String: Any]) -> String? {
        guard let gameId = record["gameId"] as? String else { return nil }
        if path.hasPrefix("SaveStates/") {
            if (record["isAuto"] as? Bool) == true { return "\(gameId)#auto" }
            guard let slot = record["slot"] as? Int else { return nil }
            return "\(gameId)#\(slot)"
        }
        guard let id = record["id"] as? String else { return nil }
        return "\(gameId)#\(id)"
    }

    /// Parses a record file. Nil data is an absent file (no records). Data that does not parse
    /// THROWS, because treating a damaged list as empty would read as "every record deleted".
    private func records(_ data: Data?, path: String) throws -> [SyncRecord] {
        guard let data, !data.isEmpty else { return [] }
        guard let array = try JSONSerialization.jsonObject(with: data) as? [[String: Any]] else {
            throw CloudSyncError(text: "\(path) is not a list of records")
        }
        return try array.map { record in
            let body = try JSONSerialization.data(withJSONObject: record, options: [.sortedKeys])
            let text = String(decoding: body, as: UTF8.self)
            let stamp = (record["createdAt"] as? Double) ?? 0
            return SyncRecord(key: Self.key(for: path, record: record) ?? text, stamp: stamp, body: text)
        }
    }

    private func merge(_ path: String, local: URL) throws {
        let localData = fm.fileExists(atPath: local.path) ? try Data(contentsOf: local) : nil
        let remoteData: Data? = try coordinatedRead(remoteURL(path)) { url in
            fm.fileExists(atPath: url.path) ? try Data(contentsOf: url) : nil
        }
        let baseData = CloudSyncPaths.baseCopies().flatMap {
            try? Data(contentsOf: $0.appendingPathComponent(path))
        }
        let merged = syncMergeRecords(base: (try? records(baseData, path: path)) ?? [],
                                      local: try records(localData, path: path),
                                      remote: try records(remoteData, path: path))
        let objects: [Any] = try merged.map {
            try JSONSerialization.jsonObject(with: Data($0.body.utf8))
        }
        let output = try JSONSerialization.data(withJSONObject: objects, options: [.sortedKeys])
        try fm.createDirectory(at: local.deletingLastPathComponent(), withIntermediateDirectories: true)
        try output.write(to: local, options: .atomic)
        try coordinatedWrite(remoteURL(path)) { url in
            try fm.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            try output.write(to: url, options: .atomic)
        }
    }

    /// The copy a later three-way merge compares against.
    private func keepBaseCopies(failed: Set<String>) {
        guard let base = CloudSyncPaths.baseCopies() else { return }
        for path in ["SaveStates/index.json", "Cheats/index.json"] where !failed.contains(path) {
            guard let local = CloudSyncPaths.localURL(for: path),
                  let data = try? Data(contentsOf: local) else { continue }
            let target = base.appendingPathComponent(path)
            try? fm.createDirectory(at: target.deletingLastPathComponent(), withIntermediateDirectories: true)
            try? data.write(to: target, options: .atomic)
        }
    }

    // ---------------------------------------------------------------- the run

    /// Removes the hidden copies `upload` leaves when the app is closed mid-copy
    /// (`.<name>.<UUID>.upload`), which nothing else ever deletes and which can each be a whole
    /// game. Only ones untouched for an hour: a younger one may be another phone's upload that is
    /// still running.
    private func removeAbandonedUploads(now: Date) {
        let keys: [URLResourceKey] = [.isRegularFileKey, .contentModificationDateKey,
                                      .attributeModificationDateKey, .creationDateKey]
        let found: [URL] = (try? coordinatedRead(remoteRoot, metadataOnly: true) { (root: URL) -> [URL] in
            var out: [URL] = []
            guard let walker = fm.enumerator(at: root, includingPropertiesForKeys: keys,
                                             options: []) else { return out }
            for case let url as URL in walker {
                guard Self.isAbandonedUploadName(url.lastPathComponent),
                      let values = try? url.resourceValues(forKeys: Set(keys)),
                      values.isRegularFile == true else { continue }
                // The newest of the three, because `copyItem` keeps the SOURCE file's
                // modification date, which can be years old on a file copied a minute ago.
                let stamps = [values.contentModificationDate, values.attributeModificationDate,
                              values.creationDate].compactMap { $0 }
                guard let newest = stamps.max(), now.timeIntervalSince(newest) > 3600 else { continue }
                out.append(url)
            }
            return out
        }) ?? []
        for url in found {
            try? coordinatedWrite(url) { u in try fm.removeItem(at: u) }
        }
    }

    /// Exactly the name `upload` gives its temporary copy, and nothing else.
    private static func isAbandonedUploadName(_ name: String) -> Bool {
        guard name.hasPrefix("."), name.hasSuffix(".upload") else { return false }
        let inner = name.dropFirst().dropLast(".upload".count)
        guard let dot = inner.lastIndex(of: "."), dot > inner.startIndex else { return false }
        return UUID(uuidString: String(inner[inner.index(after: dot)...])) != nil
    }

    /// Whether the cloud folder already holds games, read straight from `Games/`, because the
    /// listing ignores that folder while the games switch is off.
    private func cloudHoldsGames() -> Bool {
        let games = remoteURL("Games")
        guard fm.fileExists(atPath: games.path) else { return false }
        let names = (try? coordinatedRead(games, metadataOnly: true) { (url: URL) -> [String] in
            try fm.contentsOfDirectory(atPath: url.path)
        }) ?? []
        return names.contains { name in
            // A game iCloud has not brought to this phone yet is a `.Name.icloud` placeholder.
            var logical = name
            if name.hasPrefix("."), name.hasSuffix(".icloud") {
                logical = String(name.dropFirst().dropLast(".icloud".count))
            }
            return CloudSyncPaths.isSyncableGameFile(logical)
        }
    }

    /// Saves how far this run got. Without it a run stopped part way (the app closed during a
    /// long games copy) left no history, so the next run found every copied game on both sides
    /// with no record and read each whole file twice to compare them. Paths not done yet are
    /// passed as FAILED, which keeps their previous record: recording them "as listed" would mark
    /// a conflict nobody handled as synced.
    private func saveProgress(local: [SyncFileStat], remote: [SyncFileStat], pending: Set<String>,
                              done: [String], notYet: [String], failed: Set<String>, now: Int64) {
        guard let url = CloudSyncPaths.manifestURL(),
              let remoteAfter = try? listRemote().stats else { return }
        let localAfter = CloudSyncPaths.listLocal().filter { !pending.contains($0.path) }
        let text = syncCommit(manifestText: manifestText, localBefore: local, remoteBefore: remote,
                              localAfter: localAfter, remoteAfter: remoteAfter,
                              touchedPaths: done, failedPaths: Array(failed) + notYet, nowMs: now)
        try? text.write(to: url, atomically: true, encoding: .utf8)
    }

    /// `gamesSwitchedOn` runs when this run turned the games switch on. `onlyGamesLeft` runs once,
    /// when everything except `Games/` is done, with the paths written on this device so far.
    func run(gamesSwitchedOn: () async -> Void,
             onlyGamesLeft: ([String]) async -> Void) async -> CloudSyncOutcome {
        var outcome = CloudSyncOutcome()
        let now = CloudSyncPaths.millis(Date())
        clearStaleScratch()
        do {
            if !fm.fileExists(atPath: remoteRoot.path) {
                try coordinatedWrite(remoteRoot) { url in
                    try fm.createDirectory(at: url, withIntermediateDirectories: true)
                }
            }
            removeAbandonedUploads(now: Date())
            // A restore: the folder has games and this install never answered the question
            // (absent, not false), so the games come back with everything else.
            if UserDefaults.standard.object(forKey: CloudSyncPaths.includeGamesKey) == nil,
               cloudHoldsGames() {
                UserDefaults.standard.set(true, forKey: CloudSyncPaths.includeGamesKey)
                await gamesSwitchedOn()
            }
            let (remoteAll, pending) = try listRemote()
            outcome.pendingInCloud = pending.count
            let local = CloudSyncPaths.listLocal().filter { !pending.contains($0.path) }
            let remote = remoteAll.filter { !pending.contains($0.path) }
            let plan = syncPlan(local: local, remote: remote, manifestText: manifestText, nowMs: now)
            if let refusal = plan.refusal {
                outcome.refusal = refusal
                return outcome
            }
            var failed = Set(pending)
            let listedRemote = Dictionary(remote.map { ($0.path, $0) }, uniquingKeysWith: { a, _ in a })
            // Everything but the games first, in the plan's order (record files still last among
            // them), so the files a game uses are settled before launching one is allowed again.
            // Games can take minutes; nothing a running game reads or writes is under `Games/`.
            let isGame: (SyncAction) -> Bool = { $0.path.hasPrefix("Games/") }
            let ordered = plan.actions.filter { !isGame($0) } + plan.actions.filter(isGame)
            var done: [String] = []
            var gamesStarted = false
            var lastSave = Date()
            for (index, action) in ordered.enumerated() {
                if isGame(action) {
                    let notYet = ordered[index...].map(\.path)
                    if !gamesStarted {
                        gamesStarted = true
                        keepBaseCopies(failed: failed)
                        saveProgress(local: local, remote: remote, pending: pending, done: done,
                                     notYet: notYet, failed: failed, now: now)
                        lastSave = Date()
                        await onlyGamesLeft(outcome.changedLocally)
                    } else if Date().timeIntervalSince(lastSave) > 120 {
                        saveProgress(local: local, remote: remote, pending: pending, done: done,
                                     notYet: notYet, failed: failed, now: now)
                        lastSave = Date()
                    }
                }
                done.append(action.path)
                guard let localURL = CloudSyncPaths.localURL(for: action.path) else {
                    failed.insert(action.path)
                    continue
                }
                do {
                    if action.kind == .upload {
                        try keepUnlistedCloudCopy(action.path, listed: listedRemote[action.path],
                                                  now: now, outcome: &outcome)
                    }
                    try perform(action, local: localURL, outcome: &outcome)
                } catch {
                    failed.insert(action.path)
                    outcome.errors.append("\(action.path): \(error.localizedDescription)")
                }
            }
            keepBaseCopies(failed: failed)
            let localAfter = CloudSyncPaths.listLocal().filter { !pending.contains($0.path) }
            let remoteAfter = (try? listRemote().stats) ?? remote
            outcome.manifestText = syncCommit(manifestText: manifestText,
                                              localBefore: local, remoteBefore: remote,
                                              localAfter: localAfter, remoteAfter: remoteAfter,
                                              touchedPaths: plan.actions.map(\.path),
                                              failedPaths: Array(failed), nowMs: now)
        } catch {
            outcome.refusal = "the cloud folder could not be read (\(error.localizedDescription)). "
                + "Open it in Files once, or choose it again"
        }
        outcome.finishedAt = Date()
        return outcome
    }

    /// A plain upload replaces the cloud file, and the plan chose it from a listing. If the cloud
    /// now holds something the listing did not show (a provider that listed lazily, another phone
    /// that wrote in the meantime), that file is kept aside as a conflict copy first rather than
    /// replaced unseen.
    private func keepUnlistedCloudCopy(_ path: String, listed: SyncFileStat?, now: Int64,
                                       outcome: inout CloudSyncOutcome) throws {
        let url = remoteURL(path)
        let current: SyncFileStat? = try coordinatedRead(url, metadataOnly: true) { u in
            CloudSyncPaths.stat(u, path: path)
        }
        guard let current else { return }
        if let listed, listed.size == current.size, listed.mtimeMs == current.mtimeMs { return }
        let aside = syncConflictPath(path: path, nowMs: now, loserIsLocal: false)
        try coordinatedMove(from: url, to: remoteURL(aside))
        outcome.conflicts += 1
    }

    private func perform(_ action: SyncAction, local: URL, outcome: inout CloudSyncOutcome) throws {
        switch action.kind {
        case .upload:
            try upload(local, to: action.path)
            outcome.uploaded += 1
        case .download:
            try download(action.path, to: local)
            outcome.downloaded += 1
            outcome.changedLocally.append(action.path)
        case .conflictKeepLocal:
            if identical(local: local, relative: action.path) { return }
            try coordinatedMove(from: remoteURL(action.path), to: remoteURL(action.aside))
            try upload(local, to: action.path)
            outcome.conflicts += 1
            outcome.uploaded += 1
        case .conflictKeepRemote:
            if identical(local: local, relative: action.path) { return }
            try upload(local, to: action.aside)
            try download(action.path, to: local)
            outcome.conflicts += 1
            outcome.downloaded += 1
            outcome.changedLocally.append(action.path)
        case .mergeRecords:
            try merge(action.path, local: local)
            outcome.merged += 1
            outcome.changedLocally.append(action.path)
        case .archiveLocal:
            try upload(local, to: action.aside)
            try fm.removeItem(at: local)
            outcome.archived += 1
            outcome.changedLocally.append(action.path)
        case .archiveRemote:
            try coordinatedMove(from: remoteURL(action.path), to: remoteURL(action.aside))
            outcome.archived += 1
        }
    }
}

// MARK: - The app-facing object

@MainActor
final class CloudSync: ObservableObject {
    private static let bookmarkKey = "continuum.sync.folderBookmark.v1"
    private static let folderNameKey = "continuum.sync.folderName.v1"
    private static let lastLineKey = "continuum.sync.lastLine.v1"
    private static let pendingSettingsKey = "continuum.sync.pendingSettings.v1"
    private static let pendingArtworkKey = "continuum.sync.pendingArtwork.v1"

    /// Never exported: device-specific, references files that are not synced, or is the sync's
    /// own state.
    private static let excludedPrefixes = [
        "continuum.sync.", "continuum.artwork.choice.", "continuum.artwork.address.",
        "continuum.artwork.sources.", "continuum.artwork.misses.", "continuum.n64.lastCrumb.",
        // `continuum.controls.touchSkins.` was here, excluded because the skin bytes under
        // Skins/ did not sync. They do now, so the per-system skin visuals travel with them.
    ]

    /// Single `continuum.` keys that belong to this phone. Each would do harm on another one, and
    /// is also refused on import, so a file exported by an older build cannot bring one in.
    private static let excludedKeys: Set<String> = [
        // The RetroAchievements token is in this phone's Keychain under this name. Another
        // phone's name here would leave that token unfound and log this phone out.
        "continuum.achievements.username.v1",
        // v1 ONLY, which stored the game's full path, including this install's own container
        // folder. On another phone those match no game and would replace its own list. It is
        // read once by `EngineHost.init` to carry an old list over and never written again.
        // `continuum.favourites.v2` stores FILE NAMES and is deliberately absent from this list:
        // it is the identity the rest of the app uses (`skinGameKey`, `import.systemChoices.v1`,
        // `manuals.attached.v1`), it survives a reinstall, and it is meaningful on another phone.
        "continuum.favourites.v1",
        // The skin index and the skin editor's changes used to be here, both by skin id, because
        // the skins' FILES did not sync and so another phone's index would list skins it had no
        // art for. The files sync now (`Skins/` above), which removes the reason: the index, the
        // editor overlays and the per-system visuals all travel with the bytes they describe.
        // A skin that names itself now shares one id on every phone (`SkinLibraryIndex.stableID`).
        // A skin that does not name itself still gets a random id, so two phones can each have a
        // copy. Merging by id keeps both instead of letting one list replace the other.
        // The address this phone last joined for online play, which is usually the other phone.
        "continuum.netplay.lastAddress.v1",
        // Permission to use this phone's microphone and camera is given on this phone.
        "continuum.peripherals.microphoneAllowed", "continuum.peripherals.cameraAllowed",
    ]

    /// Settings that matter on every phone but were named before the `continuum.` prefix was the
    /// rule. Named here rather than renamed, because a rename would lose what is already stored.
    private static let includedKeys: Set<String> = [
        // The answers to "which system is this file?", by file name.
        "import.systemChoices.v1",
        // Saved network servers. Their passwords stay in each phone's Keychain and do not sync,
        // so a server that arrives from another phone has no password here until it is added
        // again on this phone.
        "remote.servers.v1",
        // Which PDF in Manuals belongs to which game, by file name. The PDFs sync too.
        "manuals.attached.v1",
    ]

    /// Whether the optional `Games/` category is on. See `CloudSyncPaths.includeGamesKey`.
    ///
    /// Persisted on change like the other switches in this app, because a sideloaded build can be
    /// killed at any moment and a switch that did not stick would look like the feature failing.
    /// The sync worker reads the stored value directly, so nothing has to be threaded through.
    @Published var includesGames: Bool = CloudSyncPaths.migratedGamesSwitch() {
        didSet {
            guard oldValue != includesGames else { return }
            UserDefaults.standard.set(includesGames, forKey: CloudSyncPaths.includeGamesKey)
        }
    }

    /// The plain status line: last synced, files up and down, errors.
    @Published private(set) var line: String
    @Published private(set) var folderName: String?
    @Published private(set) var isSyncing = false

    /// False while a sync is moving anything a game uses (saves, states, settings, skins). True
    /// when idle, and once only `Games/` copies remain: those can take minutes, and nothing a
    /// running game reads or writes is under `Games/`. Set on the main actor only.
    @Published private(set) var allowsGameLaunch: Bool = true

    /// Set when the stores were reloaded as the games phase began, so `finish` does not reload
    /// them again under a game that may be running by then.
    private var storesReloaded = false

    private weak var host: EngineHost?
    private var pickerDelegate: SyncFolderPickerDelegate?
    private let defaults = UserDefaults.standard

    init() {
        folderName = UserDefaults.standard.string(forKey: Self.folderNameKey)
        line = UserDefaults.standard.string(forKey: Self.lastLineKey)
            ?? (UserDefaults.standard.data(forKey: Self.bookmarkKey) == nil
                ? "sync folder: off, no folder chosen" : "sync folder: not run yet")
    }

    func attach(host: EngineHost) {
        self.host = host
        // Read again: this object is built before `applyPendingSettings` runs, and the switch can
        // arrive from the cloud with the other settings.
        includesGames = CloudSyncPaths.gamesAreIncluded()
    }

    var isConfigured: Bool { defaults.data(forKey: Self.bookmarkKey) != nil }

    private func setLine(_ text: String, persist: Bool = true) {
        line = text
        if persist { defaults.set(text, forKey: Self.lastLineKey) }
    }

    // ---------------------------------------------------------------- folder

    func chooseFolder() {
        guard let presenter = EngineHost.topmostViewController() else {
            setLine("sync folder: cannot show the folder picker, no window to present it from")
            return
        }
        let picker = UIDocumentPickerViewController(forOpeningContentTypes: [UTType.folder])
        picker.allowsMultipleSelection = false
        let delegate = SyncFolderPickerDelegate { [weak self] url in
            Task { @MainActor in self?.adoptFolder(url) }
        }
        pickerDelegate = delegate
        picker.delegate = delegate
        presenter.present(picker, animated: true)
        setLine("sync folder: choose the folder to sync into", persist: false)
    }

    /// What SwiftUI's `.fileImporter(allowedContentTypes: [.folder])` handed back. This is the
    /// picker path used by Settings and the backup prompt since build 162: on build 161 the UIKit
    /// picker presented by hand from `topmostViewController()` never accepted Open (folder
    /// highlighted or entered, the picker stayed up and no delegate call arrived). SwiftUI owns
    /// the presentation and the callback here, so nothing can be presented from the wrong
    /// controller or lose its delegate.
    func adoptPickedFolder(_ result: Result<URL, Error>) {
        switch result {
        case .success(let url): adoptFolder(url)
        case .failure(let error):
            pickerDelegate = nil
            setLine("sync folder: the folder picker failed (\(error.localizedDescription))", persist: false)
        }
    }

    func noteFolderPickerShown() {
        setLine("sync folder: choose the folder to sync into", persist: false)
    }

    private func adoptFolder(_ url: URL?) {
        pickerDelegate = nil
        guard let url else {
            setLine("sync folder: folder choice cancelled; nothing changed", persist: false)
            return
        }
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        do {
            let bookmark = try url.bookmarkData(options: [], includingResourceValuesForKeys: nil,
                                                relativeTo: nil)
            defaults.set(bookmark, forKey: Self.bookmarkKey)
            defaults.set(url.lastPathComponent, forKey: Self.folderNameKey)
            folderName = url.lastPathComponent
            // A different folder has a different history. Keeping the old manifest would read
            // every file the new folder lacks as "deleted in the cloud".
            forgetHistory()
            setLine("sync folder: folder \(url.lastPathComponent) chosen, syncing")
            syncNow(reason: "a folder was chosen")
        } catch {
            setLine("sync folder: the folder could not be remembered (\(error.localizedDescription))")
        }
    }

    func forgetFolder() {
        defaults.removeObject(forKey: Self.bookmarkKey)
        defaults.removeObject(forKey: Self.folderNameKey)
        folderName = nil
        forgetHistory()
        setLine("sync folder: off, folder forgotten (nothing in it was deleted)")
    }

    private func forgetHistory() {
        if let manifest = CloudSyncPaths.manifestURL() { try? FileManager.default.removeItem(at: manifest) }
        if let base = CloudSyncPaths.baseCopies() { try? FileManager.default.removeItem(at: base) }
    }

    private func resolveFolder() -> URL? {
        guard let bookmark = defaults.data(forKey: Self.bookmarkKey) else { return nil }
        var stale = false
        guard let url = try? URL(resolvingBookmarkData: bookmark, options: [], relativeTo: nil,
                                 bookmarkDataIsStale: &stale) else { return nil }
        if stale, let fresh = try? url.bookmarkData(options: [], includingResourceValuesForKeys: nil,
                                                    relativeTo: nil) {
            defaults.set(fresh, forKey: Self.bookmarkKey)
        }
        return url
    }

    // ---------------------------------------------------------------- running

    /// Syncs if a folder is chosen and no game is running. Safe to call from any trigger.
    func syncIfConfigured(reason: String) {
        guard isConfigured else { return }
        syncNow(reason: reason)
    }

    func syncNow(reason: String) {
        guard isConfigured else {
            setLine("sync folder: choose a folder first")
            return
        }
        guard !isSyncing else { return }
        if host?.activeEntry != nil {
            setLine("sync folder: skipped while a game is running; it runs when you return to the library",
                    persist: false)
            return
        }
        guard let folder = resolveFolder() else {
            setLine("sync folder failed: the folder could not be found; choose it again in Settings")
            return
        }
        let manifestText = CloudSyncPaths.manifestURL()
            .flatMap { try? String(contentsOf: $0, encoding: .utf8) } ?? ""
        // NOT EXPORTED ON A SYNC WITH NO HISTORY for this folder (the first one after choosing
        // it, which after a reinstall is the restore). A fresh export is a near-empty file stamped
        // now, and with no history the newer file wins: the cloud's real settings would go to
        // Conflicts/ and the empty ones would be uploaded. Not exporting lets the cloud copy come
        // down and apply at the next launch. Both exports also skip themselves while a
        // downloaded copy is waiting to be applied (`pendingSettingsKey`, `pendingArtworkKey`).
        if syncLastSyncedMs(manifestText: manifestText) > 0 {
            exportSettings()
            if let host { exportArtwork(library: host.library) }
        }
        isSyncing = true
        allowsGameLaunch = false
        storesReloaded = false
        setLine("sync folder: syncing (\(reason))", persist: false)
        let root = folder.appendingPathComponent(CloudSyncPaths.remoteFolderName, isDirectory: true)
        Task.detached(priority: .utility) {
            let scoped = folder.startAccessingSecurityScopedResource()
            let outcome = await CloudSyncWorker(remoteRoot: root, manifestText: manifestText).run(
                gamesSwitchedOn: { await MainActor.run { self.includesGames = true } },
                onlyGamesLeft: { changed in await MainActor.run { self.onlyGamesLeft(changed) } })
            if scoped { folder.stopAccessingSecurityScopedResource() }
            await MainActor.run { self.finish(outcome) }
        }
    }

    /// Everything but the games is done. The stores are reloaded first, so a game launched now
    /// reads the merged save-state and cheat lists rather than writing its stale copy over them.
    private func onlyGamesLeft(_ changed: [String]) {
        reloadStores(for: Set(changed))
        storesReloaded = true
        allowsGameLaunch = true
    }

    private func reloadStores(for changed: Set<String>) {
        if changed.contains(where: { $0.hasPrefix("SaveStates/") }) {
            host?.saveStates.reloadFromDisk()
        }
        if changed.contains("Cheats/index.json") {
            host?.cheats.reloadFromDisk()
        }
        // The Amiibo list keeps its own copy of the folder's contents, so it is refreshed here.
        // Player saves and manuals need nothing: each is read from disk when it is opened.
        if changed.contains(where: { $0.hasPrefix("Amiibo/") }) {
            host?.peripherals.refreshAmiibo()
        }
    }

    private func finish(_ outcome: CloudSyncOutcome) {
        isSyncing = false
        // Every way out of a run ends here, refusals and failures included.
        allowsGameLaunch = true
        let reloadedAlready = storesReloaded
        storesReloaded = false
        if let refusal = outcome.refusal {
            setLine("sync folder failed: \(refusal)")
            host?.status = "sync folder failed: \(refusal)"
            return
        }
        if let text = outcome.manifestText, let url = CloudSyncPaths.manifestURL() {
            try? text.write(to: url, atomically: true, encoding: .utf8)
        }
        let report = SyncReport(uploaded: outcome.uploaded, downloaded: outcome.downloaded,
                                conflicts: outcome.conflicts, merged: outcome.merged,
                                archived: outcome.archived, errors: outcome.errors)
        let formatter = DateFormatter()
        formatter.dateStyle = .short
        formatter.timeStyle = .short
        var text = "sync folder: last synced \(formatter.string(from: outcome.finishedAt)), "
            + syncReportLine(report: report)
        if outcome.pendingInCloud > 0 {
            text += ", \(outcome.pendingInCloud) still downloading from the cloud (next sync)"
        }
        let changed = Set(outcome.changedLocally)
        // The games phase only writes `Games/`, so stores reloaded when it began are current.
        if !reloadedAlready {
            reloadStores(for: changed)
        }
        if changed.contains(CloudSyncPaths.settingsPath) {
            defaults.set(true, forKey: Self.pendingSettingsKey)
            text += "; settings from the cloud apply the next time Continuum opens"
        }
        // Games that arrived are on disk but not in the Library until it is rescanned, and the
        // Library is built from a scan of Documents rather than from an index, so this is all it
        // takes. Done here rather than at the next launch because a game is usable immediately.
        if changed.contains(where: { $0.hasPrefix("Games/") }) {
            host?.refreshLibrary()
        }
        // SKINS ARRIVE IN TWO HALVES and both have to be in place before either is used: the
        // files under `Skins/`, here, and the index naming them, which travels in the settings
        // plist above and is applied by `applyPendingSettings` at the next launch. So this says
        // so rather than reloading now, which would otherwise leave the in-memory skin library
        // listing ids whose index entry has not been read yet.
        if changed.contains(where: { $0.hasPrefix("Skins/") }) {
            text += "; skins from the cloud apply the next time Continuum opens"
        }
        if changed.contains(where: { $0.hasPrefix("Artwork/") }) {
            defaults.set(true, forKey: Self.pendingArtworkKey)
            text += "; cover choices from the cloud apply the next time Continuum opens"
        }
        setLine(text)
        if outcome.uploaded + outcome.downloaded + outcome.conflicts + outcome.archived > 0
            || !outcome.errors.isEmpty {
            host?.status = text
        }
    }

    // ---------------------------------------------------------------- settings

    /// Whether a UserDefaults key travels in `Settings/defaults.plist`. Used on export AND on
    /// import, so both directions agree.
    private static func exportable(_ key: String) -> Bool {
        if includedKeys.contains(key) { return true }
        return key.hasPrefix("continuum.") && !excludedKeys.contains(key)
            && !excludedPrefixes.contains { key.hasPrefix($0) }
    }

    /// Writes the current settings to the staging file, only when they differ from what is there,
    /// so an unchanged setting does not look like a change to the sync.
    private func exportSettings() {
        // A downloaded file waiting for the next launch must not be overwritten by this phone's
        // older values, or this phone would upload them straight back.
        guard !defaults.bool(forKey: Self.pendingSettingsKey),
              let url = CloudSyncPaths.staging()?.appendingPathComponent(CloudSyncPaths.settingsPath)
        else { return }
        let current = defaults.dictionaryRepresentation().filter { Self.exportable($0.key) }
        if let data = try? Data(contentsOf: url),
           let stored = try? PropertyListSerialization.propertyList(from: data, format: nil) as? NSDictionary,
           stored.isEqual(to: current) {
            return
        }
        guard let data = try? PropertyListSerialization.data(fromPropertyList: current,
                                                             format: .xml, options: 0) else { return }
        try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(),
                                                 withIntermediateDirectories: true)
        try? data.write(to: url, options: .atomic)
    }

    /// Runs first thing at launch, before any store reads its settings.
    static func applyPendingSettings() {
        let defaults = UserDefaults.standard
        guard defaults.bool(forKey: pendingSettingsKey) else { return }
        defaults.removeObject(forKey: pendingSettingsKey)
        guard let url = CloudSyncPaths.staging()?.appendingPathComponent(CloudSyncPaths.settingsPath),
              let data = try? Data(contentsOf: url),
              let values = try? PropertyListSerialization.propertyList(from: data, format: nil)
                as? [String: Any] else { return }
        for (key, value) in values where exportable(key) {
            if mergedSkinKeys.contains(key) {
                // Nil means one side could not be read: this phone's value is left as it is.
                if let merged = mergedSkinValue(key: key, local: defaults.object(forKey: key),
                                                incoming: value) {
                    defaults.set(merged, forKey: key)
                }
                continue
            }
            defaults.set(value, forKey: key)
        }
    }

    // The skin keys, as `EngineHost` stores them: JSON `Data` from `JSONEncoder`.
    private static let skinLibraryKey = "continuum.skins.library.v1"
    private static let touchSkinsKey = "continuum.controls.touchSkins.v1"
    private static let skinEditsKey = "continuum.controls.skinEdits.v1"

    /// MERGED rather than replaced. A skin that names itself shares one id, so that id updates.
    /// A skin that does not name itself is still a different id on each phone. Replacing the
    /// whole dictionary would drop every skin that exists only on this phone. This phone's
    /// entries stay; the incoming copy wins on the same id, system or game.
    private static let mergedSkinKeys: Set<String> = [skinLibraryKey, touchSkinsKey, skinEditsKey]

    /// The merged value, or nil to leave this phone's value untouched.
    private static func mergedSkinValue(key: String, local: Any?, incoming: Any) -> Data? {
        guard let theirs = incoming as? Data else { return nil }
        let mine = local as? Data
        // Something is stored here that is not data: not ours to overwrite.
        if local != nil, mine == nil { return nil }
        let decoder = JSONDecoder()
        let encoder = JSONEncoder()
        switch key {
        case skinLibraryKey:
            guard let incomingIndex = decodedIndex(theirs) else { return nil }
            var merged = SkinLibraryIndex()
            if let mine {
                guard let localIndex = decodedIndex(mine) else { return nil }
                merged = localIndex
            }
            merged.records.merge(incomingIndex.records) { _, new in new }
            merged.defaults.merge(incomingIndex.defaults) { _, new in new }
            merged.perGame.merge(incomingIndex.perGame) { _, new in new }
            return try? encoder.encode(merged)
        case touchSkinsKey:
            guard let incomingVisuals = try? decoder.decode([String: DeltaSkinVisual].self,
                                                             from: theirs) else { return nil }
            var merged: [String: DeltaSkinVisual] = [:]
            if let mine {
                guard let localVisuals = try? decoder.decode([String: DeltaSkinVisual].self,
                                                              from: mine) else { return nil }
                merged = localVisuals
            }
            merged.merge(incomingVisuals) { _, new in new }
            return try? encoder.encode(merged)
        case skinEditsKey:
            guard let incomingEdits = try? decoder.decode([String: SkinEdits].self,
                                                           from: theirs) else { return nil }
            var merged: [String: SkinEdits] = [:]
            if let mine {
                guard let localEdits = try? decoder.decode([String: SkinEdits].self,
                                                            from: mine) else { return nil }
                merged = localEdits
            }
            merged.merge(incomingEdits) { _, new in new }
            return try? encoder.encode(merged)
        default:
            return nil
        }
    }

    /// One bad skin is dropped on decode now, instead of emptying the whole list. The counts
    /// still have to match the raw file: a list that lost a skin is unreadable, not "no skins",
    /// and must not be merged over a good library.
    private static func decodedIndex(_ data: Data) -> SkinLibraryIndex? {
        guard let index = try? JSONDecoder().decode(SkinLibraryIndex.self, from: data),
              let raw = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return nil }
        let counts = [("records", index.records.count), ("defaults", index.defaults.count),
                      ("perGame", index.perGame.count)]
        for (field, count) in counts {
            if let map = raw[field] as? [String: Any], map.count != count { return nil }
        }
        return index
    }

    // ---------------------------------------------------------------- artwork

    private static let choiceKey = "continuum.artwork.choice.v1"

    /// Cover file name for a ROM, the same on every phone (the ROM's path is not).
    private static func coverName(for romName: String) -> String {
        "\(ArtworkDisk.key(forPath: romName)).cover"
    }

    private static func choiceMap() -> [String: Any] {
        guard let data = UserDefaults.standard.data(forKey: choiceKey),
              let map = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return [:] }
        return map
    }

    /// Re-keys the chosen covers by ROM filename and copies the ones that exist only on this phone
    /// (picked files and captured frames) into the staging folder.
    private func exportArtwork(library: [LibraryEntry]) {
        guard !defaults.bool(forKey: Self.pendingArtworkKey),
              let staging = CloudSyncPaths.staging(),
              let artwork = ArtworkDisk.directory() else { return }
        let fm = FileManager.default
        let map = Self.choiceMap()
        var out: [String: Any] = [:]
        let covers = staging.appendingPathComponent("Artwork/covers", isDirectory: true)
        try? fm.createDirectory(at: covers, withIntermediateDirectories: true)
        for entry in library {
            let key = ArtworkDisk.key(forPath: entry.path)
            guard let choice = map[key] as? [String: Any] else { continue }
            out[entry.name] = choice
            let kind = choice["kind"] as? String
            guard kind == "pickedFile" || kind == "capturedFrame" else { continue }
            let source = artwork.appendingPathComponent("\(key).cover")
            let target = covers.appendingPathComponent(Self.coverName(for: entry.name))
            let sourceSize = (try? source.resourceValues(forKeys: [.fileSizeKey]))?.fileSize
            let targetSize = (try? target.resourceValues(forKeys: [.fileSizeKey]))?.fileSize
            if sourceSize != nil, sourceSize != targetSize {
                try? fm.removeItem(at: target)
                try? fm.copyItem(at: source, to: target)
            }
        }
        let url = staging.appendingPathComponent(CloudSyncPaths.artworkChoicesPath)
        if let data = try? Data(contentsOf: url),
           let stored = try? JSONSerialization.jsonObject(with: data) as? NSDictionary,
           stored.isEqual(to: out) {
            return
        }
        guard let data = try? JSONSerialization.data(withJSONObject: out, options: [.sortedKeys]) else { return }
        try? data.write(to: url, options: .atomic)
    }

    /// Runs at launch once the library is scanned, before the artwork store attaches. A cloud
    /// choice replaces this phone's only when it was made more recently.
    static func applyPendingArtwork(library: [LibraryEntry]) {
        let defaults = UserDefaults.standard
        guard defaults.bool(forKey: pendingArtworkKey) else { return }
        defaults.removeObject(forKey: pendingArtworkKey)
        guard let staging = CloudSyncPaths.staging(),
              let artwork = ArtworkDisk.directory(),
              let data = try? Data(contentsOf: staging.appendingPathComponent(CloudSyncPaths.artworkChoicesPath)),
              let incoming = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return }
        var map = choiceMap()
        let fm = FileManager.default
        for entry in library {
            guard let choice = incoming[entry.name] as? [String: Any] else { continue }
            let key = ArtworkDisk.key(forPath: entry.path)
            let mine = (map[key] as? [String: Any])?["chosenAt"] as? Double ?? -1
            let theirs = choice["chosenAt"] as? Double ?? 0
            guard theirs > mine else { continue }
            let kind = choice["kind"] as? String
            if kind == "pickedFile" || kind == "capturedFrame" {
                let source = staging.appendingPathComponent("Artwork/covers")
                    .appendingPathComponent(coverName(for: entry.name))
                guard fm.fileExists(atPath: source.path) else { continue }
                let target = artwork.appendingPathComponent("\(key).cover")
                // Copied beside it first, then swapped in, so a copy that fails leaves the old
                // cover in place rather than a choice pointing at no file.
                let temporary = artwork.appendingPathComponent(".\(key).\(UUID().uuidString).incoming")
                do {
                    try fm.copyItem(at: source, to: temporary)
                    if fm.fileExists(atPath: target.path) {
                        _ = try fm.replaceItemAt(target, withItemAt: temporary)
                    } else {
                        try fm.moveItem(at: temporary, to: target)
                    }
                } catch {
                    try? fm.removeItem(at: temporary)
                    continue
                }
            }
            map[key] = choice
        }
        if let encoded = try? JSONSerialization.data(withJSONObject: map, options: [.sortedKeys]) {
            defaults.set(encoded, forKey: choiceKey)
        }
    }
}

/// The folder picker's delegate. Held strongly by `CloudSync` while the picker is up, because a
/// picker holds its delegate weakly.
final class SyncFolderPickerDelegate: NSObject, UIDocumentPickerDelegate {
    private let done: (URL?) -> Void

    init(done: @escaping (URL?) -> Void) {
        self.done = done
    }

    func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
        done(urls.first)
    }

    func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
        done(nil)
    }
}

// MARK: - Settings section

/// Asks, once, whether Continuum should keep a backup — instead of waiting to be found.
///
/// WHY THIS EXISTS, in the owner's words: "I've never had a chance to do it before, so I'm not
/// going to start now when this should have been one of the first things ever done in the whole
/// project." They are right. Everything needed to survive deleting the app has been in Settings
/// for builds, and they delete the app before every single install, and nothing ever told them.
/// A feature nobody is told about is a feature nobody has.
///
/// WHY IT STILL NEEDS ONE TAP, which is the honest part. iOS deletes an app's entire sandbox when
/// the app is deleted, and gives an app no storage outside it that survives. The one place that
/// does survive is a folder the USER grants access to through the system picker, and that grant
/// cannot be faked or pre-filled — it is the whole point of it. iCloud's own container would not
/// need a picker, and is not an option here: re-signing an app on the phone strips the iCloud
/// entitlement, which is why this sync was built around a chosen folder in the first place (see
/// the note at the top of this file). So: one tap, asked for at the right moment, and after that
/// it is automatic forever — `syncIfConfigured` already runs when the app opens and when a game
/// is left.
///
/// ASKED WHEN THERE IS SOMETHING TO LOSE, not on a first launch with an empty library, where it
/// would be one more dialog in front of someone who has not used the app yet. And asked once:
/// "Not now" is remembered, and the Settings row is still there for later.
struct BackupFolderPrompt: ViewModifier {
    @ObservedObject var host: EngineHost
    @ObservedObject var sync: CloudSync
    /// The crash question (Feedback.swift) is the other alert at launch. This one waits for it.
    @ObservedObject private var feedback = FeedbackCenter.shared
    @State private var shown = false
    @State private var pickingFolder = false
    private static let promptMessage: String = "Deleting Continuum deletes everything in it. If you pick a folder, Continuum keeps a copy of your saves, skins, starred games and settings there, and your games too if you choose \"with games\" (they can take a lot of space). After installing again, choose the same folder in Settings and it all comes back.\n\nAny folder in Files works — iCloud Drive, Google Drive, Dropbox."

    private static let askedKey = "continuum.sync.backupOffered.v1"

    func body(content: Content) -> some View {
        content
            // On a view of its own, never on `content`: the crash question is an alert on the same
            // root view, and two alerts on one view can leave one of them stuck.
            .background(
                Color.clear
                    .fileImporter(isPresented: $pickingFolder,
                                  allowedContentTypes: [UTType.folder]) { (result: Result<URL, Error>) in
                        sync.adoptPickedFolder(result)
                    }
                    .alert("Keep your games and saves safe?", isPresented: $shown) {
                        Button("Choose a folder, with games") { choose(withGames: true) }
                        Button("Choose a folder, without games") { choose(withGames: false) }
                        Button("Not now", role: .cancel) {
                            UserDefaults.standard.set(true, forKey: Self.askedKey)
                        }
                    } message: {
                        Text(Self.promptMessage)
                    }
            )
            .onChange(of: host.library.count) { _ in offerIfItIsTime() }
            .onChange(of: feedback.crashPromptShown) { _ in offerSoon() }
            .onChange(of: feedback.crashReportOpen) { _ in offerSoon() }
            .onAppear { offerIfItIsTime() }
    }

    private func choose(withGames: Bool) {
        UserDefaults.standard.set(true, forKey: Self.askedKey)
        // Before the folder is chosen, so its first sync already includes or leaves out games.
        sync.includesGames = withGames
        sync.noteFolderPickerShown()
        // After the alert has gone, so the importer is not presented over a closing alert.
        Task { @MainActor in
            try? await Task.sleep(nanoseconds: 400_000_000)
            pickingFolder = true
        }
    }

    /// A moment after the crash question or its form goes, so this is not presented while the
    /// other is still on its way out.
    private func offerSoon() {
        Task { @MainActor in
            try? await Task.sleep(nanoseconds: 1_000_000_000)
            offerIfItIsTime()
        }
    }

    private func offerIfItIsTime() {
        guard !shown,
              !feedback.crashPromptShown,
              !feedback.crashReportOpen,
              !sync.isConfigured,
              !UserDefaults.standard.bool(forKey: Self.askedKey),
              !host.library.isEmpty,
              // Never over a running game.
              host.activeEntry == nil
        else { return }
        shown = true
    }
}

struct CloudSyncSection: View {
    @ObservedObject var sync: CloudSync
    @State private var pickingFolder = false

    var body: some View {
        SettingsSection(title: "SYNC FOLDER") {
            SettingsReadout(label: "Folder", value: sync.folderName ?? "none chosen")
            SettingsNote(sync.line)
            SettingsButton(title: sync.folderName == nil ? "Choose a sync folder" : "Choose a different folder",
                           role: .normal) {
                sync.noteFolderPickerShown()
                pickingFolder = true
            }
            .fileImporter(isPresented: $pickingFolder,
                          allowedContentTypes: [UTType.folder]) { (result: Result<URL, Error>) in
                sync.adoptPickedFolder(result)
            }
            if sync.folderName != nil {
                SettingsButton(title: sync.isSyncing ? "Syncing..." : "Sync now", role: .normal) {
                    sync.syncNow(reason: "Sync now")
                }
                SettingsButton(title: "Stop syncing (keeps every file)", role: .destructive) {
                    sync.forgetFolder()
                }
            }
            // OFF BY DEFAULT, and the only category that is. Everything else here is small; a
            // shelf of PlayStation discs is tens of gigabytes, and putting that in somebody's
            // cloud without asking would be a worse surprise than the problem it solves.
            Toggle(isOn: $sync.includesGames) {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Back up the games too")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(.white)
                    Text("Off by default, because games are big: a few PlayStation discs can be "
                         + "tens of gigabytes and it all goes in the folder you chose. With it "
                         + "on, deleting Continuum and installing it again brings your games "
                         + "back along with everything else. Disc games keep their track files, "
                         + "so they still load. BIOS files are never copied.")
                        .font(.system(size: 12))
                        .foregroundStyle(ShellPalette.secondaryText)
                }
            }
            .tint(ShellPalette.accent)
            SettingsNote("Save states, battery saves, Flash and J2ME saves, cheats, skins, "
                         + "starred games, manuals, Amiibo, cover choices and settings sync with a "
                         + "\"Continuum Sync\" folder inside "
                         + "the folder you choose. Any folder in Files works: iCloud Drive, Google "
                         + "Drive, Dropbox. Newest wins; a conflict keeps both copies in Continuum "
                         + "Sync/Conflicts, and a deleted save state is moved to Continuum Sync/Deleted "
                         + "rather than removed. Sync runs when the app opens, when you leave a game, "
                         + "and from this button.")
        }
    }
}
