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
//   Manuals/<name>.pdf                                        <Documents>/Manuals/
//   Amiibo/<name>.bin                                         <Documents>/Amiibo/
//   Settings/defaults.plist                                   exported from UserDefaults: the
//                                                             `continuum.` keys and three older
//                                                             ones (`includedKeys`), never the
//                                                             per-phone ones (`excludedKeys`)
//   Artwork/choices.json, Artwork/covers/<hash>.cover         exported artwork choices, re-keyed
//                                                             by ROM filename
//
// Kept on this phone on purpose: skins (Skins/ and their settings), the RetroAchievements login,
// favourites (stored by full path, which differs per install), the last online-play address,
// microphone and camera consent, and the sync's own bookmark and history. `excludedKeys` and
// `excludedPrefixes` say why for each.
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
    private func upload(_ local: URL, to relative: String) throws {
        let destination = remoteURL(relative)
        try coordinatedWrite(destination) { url in
            try fm.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            if fm.fileExists(atPath: url.path) { try fm.removeItem(at: url) }
            try fm.copyItem(at: local, to: url)
        }
    }

    /// Cloud file to this device, replacing atomically so a reader never sees half a file.
    private func download(_ relative: String, to local: URL) throws {
        guard let scratch = CloudSyncPaths.scratch() else {
            throw CloudSyncError(text: "no Application Support directory")
        }
        try fm.createDirectory(at: scratch, withIntermediateDirectories: true)
        let temporary = scratch.appendingPathComponent(UUID().uuidString)
        try coordinatedRead(remoteURL(relative)) { url in
            try fm.copyItem(at: url, to: temporary)
        }
        try place(temporary, at: local)
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

    func run() -> CloudSyncOutcome {
        var outcome = CloudSyncOutcome()
        let now = CloudSyncPaths.millis(Date())
        do {
            if !fm.fileExists(atPath: remoteRoot.path) {
                try coordinatedWrite(remoteRoot) { url in
                    try fm.createDirectory(at: url, withIntermediateDirectories: true)
                }
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
            for action in plan.actions {
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
        "continuum.controls.touchSkins.",
    ]

    /// Single `continuum.` keys that belong to this phone. Each would do harm on another one, and
    /// is also refused on import, so a file exported by an older build cannot bring one in.
    private static let excludedKeys: Set<String> = [
        // The RetroAchievements token is in this phone's Keychain under this name. Another
        // phone's name here would leave that token unfound and log this phone out.
        "continuum.achievements.username.v1",
        // Favourites are stored by the game's full path, which includes this install's own
        // container folder. On another phone they match no game and would replace its own.
        "continuum.favourites.v1",
        // The skin index and the skin editor's changes, both by skin id. The skins' files live
        // under Skins/, which does not sync, so another phone's index would drop this phone's
        // skins from its library (the same reason as `continuum.controls.touchSkins.` above).
        "continuum.skins.library.v1", "continuum.controls.skinEdits.v1",
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

    /// The plain status line: last synced, files up and down, errors.
    @Published private(set) var line: String
    @Published private(set) var folderName: String?
    @Published private(set) var isSyncing = false

    private weak var host: EngineHost?
    private var pickerDelegate: SyncFolderPickerDelegate?
    private let defaults = UserDefaults.standard

    init() {
        folderName = UserDefaults.standard.string(forKey: Self.folderNameKey)
        line = UserDefaults.standard.string(forKey: Self.lastLineKey)
            ?? (UserDefaults.standard.data(forKey: Self.bookmarkKey) == nil
                ? "cloud sync: off, no folder chosen" : "cloud sync: not run yet")
    }

    func attach(host: EngineHost) {
        self.host = host
    }

    var isConfigured: Bool { defaults.data(forKey: Self.bookmarkKey) != nil }

    private func setLine(_ text: String, persist: Bool = true) {
        line = text
        if persist { defaults.set(text, forKey: Self.lastLineKey) }
    }

    // ---------------------------------------------------------------- folder

    func chooseFolder() {
        guard let presenter = EngineHost.topmostViewController() else {
            setLine("cloud sync: cannot show the folder picker, no window to present it from")
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
        setLine("cloud sync: choose the folder to sync into", persist: false)
    }

    private func adoptFolder(_ url: URL?) {
        pickerDelegate = nil
        guard let url else {
            setLine("cloud sync: folder choice cancelled; nothing changed", persist: false)
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
            setLine("cloud sync: folder \(url.lastPathComponent) chosen, syncing")
            syncNow(reason: "a folder was chosen")
        } catch {
            setLine("cloud sync: the folder could not be remembered (\(error.localizedDescription))")
        }
    }

    func forgetFolder() {
        defaults.removeObject(forKey: Self.bookmarkKey)
        defaults.removeObject(forKey: Self.folderNameKey)
        folderName = nil
        forgetHistory()
        setLine("cloud sync: off, folder forgotten (nothing in it was deleted)")
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
            setLine("cloud sync: choose a folder first")
            return
        }
        guard !isSyncing else { return }
        if host?.activeEntry != nil {
            setLine("cloud sync: skipped while a game is running; it runs when you return to the library",
                    persist: false)
            return
        }
        guard let folder = resolveFolder() else {
            setLine("cloud sync failed: the folder could not be found; choose it again in Settings")
            return
        }
        exportSettings()
        if let host { exportArtwork(library: host.library) }

        let manifestText = CloudSyncPaths.manifestURL()
            .flatMap { try? String(contentsOf: $0, encoding: .utf8) } ?? ""
        isSyncing = true
        setLine("cloud sync: syncing (\(reason))", persist: false)
        let root = folder.appendingPathComponent(CloudSyncPaths.remoteFolderName, isDirectory: true)
        Task.detached(priority: .utility) {
            let scoped = folder.startAccessingSecurityScopedResource()
            let outcome = CloudSyncWorker(remoteRoot: root, manifestText: manifestText).run()
            if scoped { folder.stopAccessingSecurityScopedResource() }
            await MainActor.run { self.finish(outcome) }
        }
    }

    private func finish(_ outcome: CloudSyncOutcome) {
        isSyncing = false
        if let refusal = outcome.refusal {
            setLine("cloud sync failed: \(refusal)")
            host?.status = "cloud sync failed: \(refusal)"
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
        var text = "cloud sync: last synced \(formatter.string(from: outcome.finishedAt)), "
            + syncReportLine(report: report)
        if outcome.pendingInCloud > 0 {
            text += ", \(outcome.pendingInCloud) still downloading from the cloud (next sync)"
        }
        let changed = Set(outcome.changedLocally)
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
        if changed.contains(CloudSyncPaths.settingsPath) {
            defaults.set(true, forKey: Self.pendingSettingsKey)
            text += "; settings from the cloud apply the next time Continuum opens"
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
            defaults.set(value, forKey: key)
        }
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
                try? fm.removeItem(at: target)
                guard (try? fm.copyItem(at: source, to: target)) != nil else { continue }
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

struct CloudSyncSection: View {
    @ObservedObject var sync: CloudSync

    var body: some View {
        SettingsSection(title: "CLOUD SYNC") {
            SettingsReadout(label: "Folder", value: sync.folderName ?? "none chosen")
            SettingsNote(sync.line)
            SettingsButton(title: sync.folderName == nil ? "Choose a sync folder" : "Choose a different folder",
                           role: .normal) {
                sync.chooseFolder()
            }
            if sync.folderName != nil {
                SettingsButton(title: sync.isSyncing ? "Syncing..." : "Sync now", role: .normal) {
                    sync.syncNow(reason: "Sync now")
                }
                SettingsButton(title: "Stop syncing (keeps every file)", role: .destructive) {
                    sync.forgetFolder()
                }
            }
            SettingsNote("Save states, battery saves, Flash and J2ME saves, cheats, manuals, Amiibo, "
                         + "cover choices and settings sync with a \"Continuum Sync\" folder inside "
                         + "the folder you choose. Any folder in Files works: iCloud Drive, Google "
                         + "Drive, Dropbox. Newest wins; a conflict keeps both copies in Continuum "
                         + "Sync/Conflicts, and a deleted save state is moved to Continuum Sync/Deleted "
                         + "rather than removed. Sync runs when the app opens, when you leave a game, "
                         + "and from this button.")
        }
    }
}
