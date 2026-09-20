// Continuum - fetching, decoding, storing and serving real cover art.
//
// GameArtwork.swift decides WHICH addresses are worth asking for. This file does the asking, keeps
// what comes back, and hands it to the library. Five things in here are deliberate and each one is
// the answer to a question the browser build had to leave open:
//
//  1. THE BYTES ARE KEPT, NOT THE URL. URLSession is not subject to CORS, so a cover downloads
//     once and is then shown from disk with no network at all. The browser could only persist an
//     address and hope the HTTP cache still had the image.
//  2. NOTHING BLOCKS. The library renders on procedural plates immediately and covers fade in as
//     they arrive. There is no synchronous disk read and no synchronous decode on the main actor.
//  3. AT MOST THREE LOOKUPS AT ONCE, through an actor that hands a slot straight to whoever is
//     waiting. Importing thirty ROMs would otherwise open up to 270 connections, and the radio and
//     the memory are better spent on the game the user is waiting for.
//  4. A MISS IS REMEMBERED FOR A WEEK, NOT FOREVER. The libretro repository gains thumbnails over
//     time, so a permanent miss would eventually be wrong; re-probing nine URLs per unknown ROM on
//     every launch would be worse. Only a genuine 404 counts as a miss: being offline must never
//     poison the cache, which is why a URLError and a 503 are reported and NOT recorded.
//  5. ART LIVES OUTSIDE DOCUMENTS. Documents is user-visible through the Files app because
//     UIFileSharingEnabled is set, and a folder of hashed PNGs appearing next to a user's ROMs
//     would read as clutter the app had lost track of. Artwork is not user content.

import SwiftUI
import UIKit
import UniformTypeIdentifiers

// MARK: - What a resolved cover is

/// A decoded cover, plus where it came from.
///
/// `@unchecked Sendable` because UIImage is not formally Sendable and this value crosses from the
/// background decode back to the main actor. It is safe: the image is created inside the decode,
/// never mutated afterwards, and CoreGraphics-backed UIImages are documented as safe to read from
/// any thread. The alternative, handing Data across and decoding on the main actor, is the thing
/// this type exists to avoid.
struct CoverImage: @unchecked Sendable {
    let image: UIImage
    /// A sentence for the detail sheet: "box art, exact name" or "picked from Files".
    let provenance: String
}

/// What one walk of the ladder produced.
enum ArtworkFetchOutcome: Sendable {
    /// A real image, with the bytes to store and the provenance to record.
    case found(data: Data, tier: String, provenance: String, address: String)
    /// Every candidate answered 404. This is the only outcome that is worth remembering as a miss.
    case noArtOnServer(probes: Int)
    /// The network or the server failed. NOT a miss: reported, and retried next time.
    case failure(String)
}

// MARK: - Three at a time

/// The concurrency cap, as an actor.
///
/// Ports the browser's acquire/release queue exactly, including the detail that matters: when a
/// slot is released and someone is waiting, the slot is handed straight over rather than being
/// given back and re-taken, so the count cannot drift.
actor ArtworkGate {
    static let shared = ArtworkGate()

    /// Three, as the browser used. See the file header for why.
    private let limit = 3
    private var active = 0
    private var waiting: [CheckedContinuation<Void, Never>] = []

    func acquire() async {
        if active < limit {
            active += 1
            return
        }
        await withCheckedContinuation { continuation in
            waiting.append(continuation)
        }
    }

    func release() {
        if waiting.isEmpty {
            active = max(0, active - 1)
        } else {
            let next = waiting.removeFirst()
            next.resume()
        }
    }

    /// For the diagnostics read-out: how many lookups are in flight and how many are queued.
    func load() -> (active: Int, waiting: Int) {
        (active, waiting.count)
    }
}

// MARK: - Where the bytes live

/// The on-disk artwork store. Non-isolated on purpose: every function in here does file I/O or
/// image decoding, and an async non-isolated function runs off the main actor, which is what keeps
/// a 300 KB PNG decode out of the frame the library is trying to draw.
enum ArtworkDisk {
    /// The stored-cover directory, created on demand.
    ///
    /// Application Support rather than Caches, and rather than Documents. Caches can be purged by
    /// the OS under storage pressure, which would silently undo the one property this whole file
    /// exists for: that real box art shows with no network. Documents is user-visible through the
    /// Files app and artwork is not user content. It is a SUBDIRECTORY rather than loose files
    /// because Application Support itself is the system directory handed to the cores, where a
    /// core goes looking for its BIOS.
    static func directory() -> URL? {
        guard let support = FileManager.default.urls(for: .applicationSupportDirectory,
                                                     in: .userDomainMask).first else {
            return nil
        }
        var directory = support.appendingPathComponent("Artwork", isDirectory: true)
        if !FileManager.default.fileExists(atPath: directory.path) {
            do {
                try FileManager.default.createDirectory(at: directory,
                                                        withIntermediateDirectories: true)
                // Downloaded art is reproducible, so it has no business in a device backup.
                var resourceValues = URLResourceValues()
                resourceValues.isExcludedFromBackup = true
                try? directory.setResourceValues(resourceValues)
            } catch {
                return nil
            }
        }
        return directory
    }

    /// The filename a game's cover is stored under: a hash of its absolute path.
    ///
    /// Hashed rather than the filename itself because a filename can contain a slash-free but still
    /// filesystem-hostile mixture of characters, and because the key has to be stable and fixed
    /// length for the UserDefaults maps that sit beside the files.
    ///
    /// SIXTY-FOUR BITS HERE, THIRTY-TWO IN ArtPlate, AND THAT IS NOT AN INCONSISTENCY. The plate's
    /// hash has to stay 32-bit FNV-1a to keep producing the same gradient the browser build did.
    /// This one is a storage key, where a collision would show one game's cover on another, so it
    /// takes the wider variant.
    static func key(forPath path: String) -> String {
        var hash: UInt64 = 0xcbf2_9ce4_8422_2325
        for byte in Array(path.utf8) {
            hash ^= UInt64(byte)
            hash = hash &* 0x0000_0100_0000_01b3
        }
        return String(format: "%016llx", hash)
    }

    private static func fileURL(for key: String) -> URL? {
        directory()?.appendingPathComponent("\(key).cover")
    }

    /// A decoded image on its way back to the main actor. Boxed for the same reason `CoverImage`
    /// is.
    struct DecodedImage: @unchecked Sendable {
        let image: UIImage
    }

    /// Loads and decodes a stored cover, or nil when there is none.
    ///
    /// `preparingForDisplay()` forces the decode to happen HERE, off the main actor, rather than
    /// lazily inside the first draw, which is what would otherwise put a JPEG decode on the frame
    /// that scrolls a shelf.
    static func load(key: String) async -> DecodedImage? {
        guard let url = fileURL(for: key) else { return nil }
        guard let data = try? Data(contentsOf: url, options: .mappedIfSafe) else { return nil }
        guard let image = UIImage(data: data) else { return nil }
        return DecodedImage(image: image.preparingForDisplay() ?? image)
    }

    /// The result of storing a cover: the decoded image either way, and a note when the write
    /// failed, because a cover that displays now and is gone after a relaunch must not look like a
    /// success.
    struct StoreResult: @unchecked Sendable {
        let image: UIImage?
        let writeFailure: String?
    }

    static func store(data: Data, key: String) async -> StoreResult {
        let decoded = UIImage(data: data)
        let prepared = decoded?.preparingForDisplay() ?? decoded
        guard let url = fileURL(for: key) else {
            return StoreResult(image: prepared,
                               writeFailure: "no Application Support directory, so it was not kept")
        }
        do {
            try data.write(to: url, options: .atomic)
            return StoreResult(image: prepared, writeFailure: nil)
        } catch {
            return StoreResult(image: prepared,
                               writeFailure: "could not be written: \(error.localizedDescription)")
        }
    }

    /// Reads a file the user picked. Returns nil with a reason rather than throwing, so the caller
    /// has a string to put on screen.
    static func read(pickedFile url: URL) async -> (data: Data?, failure: String?) {
        // The artwork picker uses asCopy: true, exactly as the ROM import does, so this URL is an
        // app-owned copy and no scope should be involved. Defensive only, and a false result means
        // there was nothing to release, which is why only a true result is balanced.
        let scoped = url.startAccessingSecurityScopedResource()
        defer {
            if scoped { url.stopAccessingSecurityScopedResource() }
        }
        do {
            let data = try Data(contentsOf: url)
            guard !data.isEmpty else {
                return (nil, "the picked file is empty")
            }
            guard UIImage(data: data) != nil else {
                return (nil, "the picked file is not an image this device can decode")
            }
            return (data, nil)
        } catch {
            return (nil, "the picked file could not be read: \(error.localizedDescription)")
        }
    }

    static func remove(key: String) async -> Bool {
        guard let url = fileURL(for: key) else { return false }
        return (try? FileManager.default.removeItem(at: url)) != nil
    }

    /// Empties the store. Returns how many files went and the bytes they held, so the Settings
    /// action can report what it did rather than just claiming to have worked.
    static func clear() async -> (files: Int, bytes: Int64) {
        guard let directory = directory() else { return (0, 0) }
        guard let contents = try? FileManager.default.contentsOfDirectory(
            at: directory,
            includingPropertiesForKeys: [.fileSizeKey],
            options: [.skipsHiddenFiles]
        ) else { return (0, 0) }

        var files = 0
        var bytes: Int64 = 0
        for url in contents {
            let size = (try? url.resourceValues(forKeys: [.fileSizeKey]))?.fileSize ?? 0
            if (try? FileManager.default.removeItem(at: url)) != nil {
                files += 1
                bytes += Int64(size)
            }
        }
        return (files, bytes)
    }

    /// What the store currently holds, for the Settings read-out.
    static func usage() async -> (files: Int, bytes: Int64) {
        guard let directory = directory() else { return (0, 0) }
        guard let contents = try? FileManager.default.contentsOfDirectory(
            at: directory,
            includingPropertiesForKeys: [.fileSizeKey],
            options: [.skipsHiddenFiles]
        ) else { return (0, 0) }

        var bytes: Int64 = 0
        for url in contents {
            bytes += Int64((try? url.resourceValues(forKeys: [.fileSizeKey]))?.fileSize ?? 0)
        }
        return (contents.count, bytes)
    }
}

// MARK: - Asking the server

/// Walks the candidate ladder for one game. Non-isolated, so the requests and the response handling
/// stay off the main actor.
enum ArtworkFetcher {
    /// One session for the whole app.
    ///
    /// Ephemeral, because the bytes are stored by this file: a second copy in the URL cache would
    /// be pure waste on a phone. `waitsForConnectivity` is off so an offline lookup fails quickly
    /// and legibly instead of hanging until the user walks back into signal, and the per-host
    /// connection limit is a second belt on the three-at-a-time cap.
    static let session: URLSession = {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 20
        configuration.timeoutIntervalForResource = 40
        configuration.waitsForConnectivity = false
        configuration.httpMaximumConnectionsPerHost = 3
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        return URLSession(configuration: configuration)
    }()

    /// Tries each candidate in order and returns the first real image.
    ///
    /// A plain GET rather than a HEAD followed by a GET: the server answers a hit with the image,
    /// so one request both proves the hit and produces the bytes, and a HEAD would double the round
    /// trips on the common case to save nothing.
    ///
    /// THE CONTENT TYPE IS CHECKED, and that is not paranoia. A 404 from this server comes back as
    /// text/html, so a status code alone is enough today, but an error page served as 200 by a
    /// captive portal or a proxy is exactly the input that would otherwise be stored as a cover and
    /// then fail to decode on every launch.
    static func resolve(candidates: [ArtworkCandidate]) async -> ArtworkFetchOutcome {
        var probes = 0
        var serverProblems: [String] = []

        for candidate in candidates {
            probes += 1
            var request = URLRequest(url: candidate.url)
            request.httpMethod = "GET"
            request.setValue("image/png,image/*;q=0.8", forHTTPHeaderField: "Accept")

            do {
                let (data, response) = try await session.data(for: request)
                guard let http = response as? HTTPURLResponse else {
                    serverProblems.append("\(candidate.folder.rawValue): no HTTP response")
                    continue
                }

                if http.statusCode == 404 {
                    // The ordinary answer for a game the database has never heard of.
                    continue
                }
                guard http.statusCode == 200 else {
                    serverProblems.append("\(candidate.folder.rawValue): HTTP \(http.statusCode)")
                    continue
                }

                let declared = http.value(forHTTPHeaderField: "Content-Type") ?? ""
                let contentType = declared.lowercased()
                guard contentType.hasPrefix("image/") else {
                    serverProblems.append(
                        "\(candidate.folder.rawValue): answered 200 as "
                            + "\(contentType.isEmpty ? "no content type" : contentType)"
                    )
                    continue
                }
                guard !data.isEmpty else {
                    serverProblems.append("\(candidate.folder.rawValue): 200 with no bytes")
                    continue
                }

                return .found(
                    data: data,
                    tier: candidate.tier,
                    provenance: "\(candidate.folder.readableName), \(candidate.form.readableName)",
                    address: candidate.url.absoluteString
                )
            } catch let error as URLError {
                // Offline, DNS, TLS or a timeout. The ladder is abandoned and NOTHING is recorded
                // as a miss: this says nothing about whether the art exists.
                return .failure(networkText(for: error))
            } catch {
                return .failure("lookup failed: \(error.localizedDescription)")
            }
        }

        if !serverProblems.isEmpty {
            // Some candidates answered something other than 200 or 404. Reported, and not cached
            // as a miss, because a server having a bad minute is not evidence about the artwork.
            let shown = serverProblems.prefix(3).joined(separator: ", ")
            return .failure("the thumbnail server answered oddly after \(probes) lookup(s): "
                            + shown)
        }
        return .noArtOnServer(probes: probes)
    }

    /// A URLError as a sentence a user can act on, rather than a code.
    private static func networkText(for error: URLError) -> String {
        switch error.code {
        case .notConnectedToInternet:
            return "no network, so no cover could be looked up"
        case .timedOut:
            return "the thumbnail server timed out"
        case .cannotFindHost, .dnsLookupFailed:
            return "thumbnails.libretro.com could not be found"
        case .networkConnectionLost:
            return "the connection dropped during the lookup"
        case .cancelled:
            return "the lookup was cancelled"
        case .dataNotAllowed:
            return "the lookup was blocked, cellular data is off for this app"
        case .secureConnectionFailed, .serverCertificateUntrusted:
            return "the secure connection to the thumbnail server failed"
        default:
            return "network error during the lookup: \(error.localizedDescription)"
        }
    }
}

// MARK: - The store the UI talks to

/// Serves covers to the library, and owns every piece of artwork state.
///
/// One instance, owned by `EngineHost` for the app's lifetime, which is also what keeps the tier 5
/// picker's delegate alive long enough to be called back. See `artworkPickerDelegate`.
@MainActor
final class ArtworkStore: ObservableObject {
    /// Bumped whenever something invalidates what a card is showing: the switch changed, the cache
    /// was cleared, misses were reset, or the user picked an image. Cards key their lookup task on
    /// it, so a bump is what makes them all resolve again.
    @Published private(set) var generation = 0

    /// Whether lookups are allowed at all.
    ///
    /// ON by default, and the Settings row says in plain words what it does, because resolving art
    /// sends the ROM's filename to a third party. The browser build treated that as a real
    /// disclosure rather than an implementation detail.
    @Published var fetchEnabled: Bool {
        didSet {
            guard oldValue != fetchEnabled else { return }
            defaults.set(fetchEnabled, forKey: Self.fetchKey)
            generation += 1
            if fetchEnabled {
                report("artwork: lookups are on, covers resolve from thumbnails.libretro.com")
            } else {
                report("artwork: lookups are off, only covers already stored are shown")
            }
        }
    }

    @Published private(set) var storedFiles = 0
    @Published private(set) var storedBytes: Int64 = 0
    @Published private(set) var rememberedMisses = 0
    @Published private(set) var resolvedThisRun = 0
    @Published private(set) var missedThisRun = 0
    @Published private(set) var failedThisRun = 0
    /// The artwork line, always a complete sentence, shown in Settings and in the diagnostics
    /// panel. Never empty.
    @Published private(set) var line = "artwork: nothing looked up yet"

    /// The engine host, for the one case where an artwork problem deserves the main status line.
    /// Weak, because the host owns this object.
    weak var host: EngineHost?

    private let defaults = UserDefaults.standard
    private static let fetchKey = "continuum.artwork.fetch.v1"
    private static let missKey = "continuum.artwork.misses.v1"
    private static let provenanceKey = "continuum.artwork.sources.v1"
    /// A week. The repository gains thumbnails over time, so a miss is not forever.
    private static let missLifetime: TimeInterval = 7 * 24 * 60 * 60

    /// Decoded covers, so scrolling a shelf does not re-read the disk.
    ///
    /// BOUNDED BY BYTES AND NOT ONLY BY COUNT, which matters more than it looks. A libretro box
    /// art is around 600 by 850, and DECODED that is roughly two megabytes whatever the file size
    /// was, so a hundred of them is two hundred megabytes of live pixels. NSCache does evict under
    /// pressure, but on a sideloaded build the OS is entitled to kill the app first. The cost limit
    /// keeps the ceiling where it can be reasoned about, and anything evicted is re-read from disk,
    /// which is a memory-mapped read rather than a network round trip.
    private let memory: NSCache<NSString, UIImage> = {
        let cache = NSCache<NSString, UIImage>()
        cache.countLimit = 120
        cache.totalCostLimit = 64 * 1024 * 1024
        return cache
    }()

    /// What one decoded cover costs in live pixels, for the cache's byte ceiling.
    private static func memoryCost(of image: UIImage) -> Int {
        let pixels = image.size.width * image.scale * image.size.height * image.scale
        // Four bytes per pixel, and never zero: a cache cost of 0 would make an image free and
        // exempt it from the ceiling it is supposed to be under.
        return max(1, Int(pixels * 4))
    }

    /// One lookup per game, however many cards are asking. The hero and a shelf card and an All
    /// Games row are routinely the same game on screen three times over.
    private var inFlight: [String: Task<CoverImage?, Never>] = [:]

    /// The last lookup failure REASON promoted to the main status line.
    ///
    /// Keyed on the reason and not on the game, which is the whole point: thirty games looked up
    /// with the network down share one reason, so they write one status line between them instead
    /// of thirty, and the import summary the user is reading survives.
    private var lastNetworkReason = ""

    // ------------------------------------------------------------------ tier 5, a picked image

    /// THIS PROPERTY IS A FIX, NOT A STYLE CHOICE, and it is the same fix the ROM import needed.
    /// `UIDocumentPickerViewController.delegate` is WEAK, so a delegate created as a local inside
    /// the presenting function is released the moment that function returns, the picker's weak
    /// reference goes nil, and the callback never fires: the sheet dismisses and nothing happens.
    /// This store is owned by `EngineHost`, which is owned by a `@StateObject` on the root view for
    /// the app's lifetime, so this strong reference provably outlives any picker it is handed to.
    private lazy var artworkPickerDelegate = ArtworkPickerDelegate(store: self)
    /// The picker while it is up, for the same belt-and-braces reason the import path holds one.
    private var activePicker: UIDocumentPickerViewController?
    /// Which game the picked image belongs to. Cleared when an outcome arrives.
    private var pickTarget: LibraryEntry?

    init() {
        // Absent means on. `bool(forKey:)` cannot tell "never set" from "set to false", so the
        // object is checked first: a fresh install fetches art, and a user who turned it off stays
        // turned off.
        if let stored = defaults.object(forKey: Self.fetchKey) as? Bool {
            fetchEnabled = stored
        } else {
            fetchEnabled = true
        }
        rememberedMisses = readMisses().count
        Task { await refreshUsage() }
    }

    /// Wires the status line back to the host. Called once, by `EngineHost.init`.
    func attach(host: EngineHost) {
        self.host = host
        host.artworkLine = line
    }

    // ------------------------------------------------------------------ serving a cover

    /// The cover for one game, or nil when there is none to show yet.
    ///
    /// Everything expensive is awaited, nothing is synchronous, and a nil answer is a perfectly
    /// good one: the caller is already drawing a plate underneath.
    func cover(for entry: LibraryEntry, system: GameSystem?) async -> CoverImage? {
        let key = ArtworkDisk.key(forPath: entry.path)

        if let cached = memory.object(forKey: key as NSString) {
            return CoverImage(image: cached, provenance: provenance(forKey: key) ?? "stored cover")
        }
        if let running = inFlight[key] {
            return await running.value
        }

        // Unstructured on purpose, so a card scrolling off screen does not cancel a download that
        // is nearly finished: the cover lands in the cache and the next card that asks gets it
        // free.
        let task = Task { [weak self] () -> CoverImage? in
            guard let self else { return nil }
            return await self.resolve(entry: entry, system: system, key: key)
        }
        inFlight[key] = task
        let cover = await task.value
        inFlight[key] = nil
        return cover
    }

    private func resolve(entry: LibraryEntry, system: GameSystem?,
                         key: String) async -> CoverImage? {
        // 1. Already on disk. This is the path every launch after the first one takes, and it needs
        //    no network at all.
        if let stored = await ArtworkDisk.load(key: key) {
            memory.setObject(stored.image, forKey: key as NSString,
                             cost: Self.memoryCost(of: stored.image))
            return CoverImage(image: stored.image,
                              provenance: provenance(forKey: key) ?? "stored cover")
        }

        // 2. Turned off. Not a failure, and not worth a status line per game.
        guard fetchEnabled else { return nil }

        // 3. Nothing to ask. A system with no playlist directory is never probed, because nine
        //    guaranteed 404s per game is worse than a plate.
        guard let system, SystemArtwork.hasThumbnails(for: system) else {
            noteOnce("artwork: no thumbnail directory is known for "
                     + "\(system?.displayName ?? entry.ext.uppercased()), so those games keep "
                     + "their generated plate")
            return nil
        }

        // 4. Recently established that the server has nothing. Expires after a week.
        if isKnownMiss(key) { return nil }

        let candidates = ArtworkNames.candidates(system: system, filename: entry.name)
        guard !candidates.isEmpty else {
            noteOnce("artwork: \(entry.name) reduced to no usable thumbnail name, so it keeps its "
                     + "generated plate")
            return nil
        }

        // Three at a time, around the WHOLE ladder walk rather than around each request, which is
        // what the browser did: at most three games are being looked up, each walking its own
        // candidates in order. Nothing between acquire and release can throw, so the slot cannot
        // leak.
        await ArtworkGate.shared.acquire()
        let outcome = await ArtworkFetcher.resolve(candidates: candidates)
        await ArtworkGate.shared.release()

        let title = GameMetadata.displayTitle(for: entry)

        switch outcome {
        case let .found(data, tier, source, address):
            let result = await ArtworkDisk.store(data: data, key: key)
            guard let image = result.image else {
                failedThisRun += 1
                reportLookupFailure(
                    reason: "the server answered with bytes that would not decode as an image",
                    game: title
                )
                return nil
            }
            memory.setObject(image, forKey: key as NSString,
                             cost: Self.memoryCost(of: image))
            clearMiss(key)
            recordProvenance(source, forKey: key)
            if let failure = result.writeFailure {
                // Showing now, gone after a relaunch. Said out loud rather than left looking
                // permanent, because a cover that silently re-downloads every launch is a bug that
                // only shows up as a data bill.
                failedThisRun += 1
                reportLookupFailure(reason: "the cover \(failure), so it will be fetched again",
                                    game: title)
                return CoverImage(image: image, provenance: source)
            }
            resolvedThisRun += 1
            await refreshUsage()
            note("artwork: \(resolvedThisRun) cover(s) resolved this run, latest \(title) as "
                 + "\(tier) from \(shortAddress(address))")
            return CoverImage(image: image, provenance: source)

        case let .noArtOnServer(probes):
            recordMiss(key)
            missedThisRun += 1
            note("artwork: \(missedThisRun) title(s) have no art on the server, latest \(title) "
                 + "after \(probes) lookup(s); it will be retried in a week")
            return nil

        case let .failure(text):
            failedThisRun += 1
            // Legible rather than silent, and NOT recorded as a miss: being offline says nothing
            // about whether the art exists, and a week of plates would be the wrong conclusion.
            reportLookupFailure(reason: text, game: title)
            return nil
        }
    }

    /// Where a game's stored cover came from, without decoding it. Nil when there is none. Read by
    /// the detail sheet, which has to be able to say "picked from Files" rather than only showing a
    /// picture.
    func provenance(for entry: LibraryEntry) -> String? {
        provenance(forKey: ArtworkDisk.key(forPath: entry.path))
    }

    // ------------------------------------------------------------------ the Settings actions

    /// Throws away every stored cover. The plates come straight back, so nothing goes blank.
    func clearStoredCovers() {
        Task {
            let (files, bytes) = await ArtworkDisk.clear()
            memory.removeAllObjects()
            defaults.removeObject(forKey: Self.provenanceKey)
            resolvedThisRun = 0
            await refreshUsage()
            generation += 1
            if files == 0 {
                report("artwork: there were no stored covers to clear")
            } else {
                report("artwork: cleared \(files) stored cover(s), \(Self.byteText(bytes))")
            }
        }
    }

    /// Forgets every remembered miss, so the next pass asks the server again.
    func retryFailedLookups() {
        let count = readMisses().count
        defaults.removeObject(forKey: Self.missKey)
        rememberedMisses = 0
        missedThisRun = 0
        failedThisRun = 0
        lastNetworkReason = ""
        generation += 1
        if count == 0 {
            report("artwork: there were no remembered misses to retry")
        } else {
            report("artwork: \(count) remembered miss(es) cleared, they will be looked up again")
        }
    }

    /// Drops one game's cover, so it falls back to its plate and can be looked up or picked again.
    func clearCover(for entry: LibraryEntry) {
        let key = ArtworkDisk.key(forPath: entry.path)
        Task {
            let removed = await ArtworkDisk.remove(key: key)
            memory.removeObject(forKey: key as NSString)
            recordProvenance(nil, forKey: key)
            clearMiss(key)
            await refreshUsage()
            generation += 1
            if removed {
                report("artwork: removed the stored cover for \(entry.name)")
            } else {
                report("artwork: \(entry.name) had no stored cover to remove")
            }
        }
    }

    /// Looks one game up again right now, ignoring a remembered miss.
    func lookUpAgain(_ entry: LibraryEntry) {
        let key = ArtworkDisk.key(forPath: entry.path)
        clearMiss(key)
        memory.removeObject(forKey: key as NSString)
        Task {
            _ = await ArtworkDisk.remove(key: key)
            recordProvenance(nil, forKey: key)
            generation += 1
            if fetchEnabled {
                report("artwork: looking \(entry.name) up again")
            } else {
                report("artwork: \(entry.name) will be looked up when artwork lookups are turned "
                       + "back on in Settings")
            }
        }
    }

    // ------------------------------------------------------------------ tier 5

    /// Presents the image picker for one game, with the delegate discipline the import flow proved.
    func presentArtworkPicker(for entry: LibraryEntry) {
        pickTarget = entry
        report("artwork: choosing an image for \(entry.name)...")

        guard let presenter = EngineHost.topmostViewController() else {
            report("artwork: cannot present the image picker, no root view controller")
            return
        }

        // UTType.image is safe where a ROM type list was not: iOS ships no UTI for .nes or .cue,
        // so a picker built from those greys the user's own files out, but every image the system
        // can decode conforms to public.image. asCopy: true for the same reason the ROM import uses
        // it: an app-owned copy in a temporary directory, with no security scope to be refused.
        let picker = UIDocumentPickerViewController(
            forOpeningContentTypes: [UTType.image],
            asCopy: true
        )
        picker.delegate = artworkPickerDelegate
        picker.allowsMultipleSelection = false
        picker.shouldShowFileExtensions = true
        picker.presentationController?.delegate = artworkPickerDelegate
        artworkPickerDelegate.prepareForNewPicker()
        activePicker = picker

        presenter.present(picker, animated: true) { [weak self] in
            // Only a line written in the completion handler can honestly claim the sheet is up.
            Task { @MainActor in
                self?.report("artwork: the image picker is up, choose a cover")
            }
        }
    }

    func releaseActivePicker() {
        activePicker = nil
    }

    /// Stores what the artwork picker handed back, on the main actor.
    func handlePickedArtwork(_ outcome: PickerOutcome) {
        guard let entry = pickTarget else {
            report("artwork: an image came back with no game waiting for it, nothing was stored")
            return
        }
        pickTarget = nil

        if let failure = outcome.failureText {
            report("artwork: \(failure)")
            return
        }
        guard let url = outcome.urls.first else {
            report("artwork: the image picker returned no file")
            return
        }

        let key = ArtworkDisk.key(forPath: entry.path)
        Task {
            let (data, failure) = await ArtworkDisk.read(pickedFile: url)
            guard let data else {
                report("artwork: \(url.lastPathComponent) was not used for \(entry.name), "
                       + "\(failure ?? "the reason was not reported")")
                return
            }
            let result = await ArtworkDisk.store(data: data, key: key)
            guard let image = result.image else {
                report("artwork: \(url.lastPathComponent) could not be decoded for \(entry.name)")
                return
            }
            if let writeFailure = result.writeFailure {
                report("artwork: \(url.lastPathComponent) is showing for \(entry.name) but it "
                       + "\(writeFailure)")
            } else {
                report("artwork: \(url.lastPathComponent) is now the cover for \(entry.name)")
            }
            memory.setObject(image, forKey: key as NSString,
                             cost: Self.memoryCost(of: image))
            recordProvenance("picked from Files", forKey: key)
            clearMiss(key)
            await refreshUsage()
            generation += 1
        }
    }

    // ------------------------------------------------------------------ the negative cache

    private func readMisses() -> [String: Double] {
        defaults.object(forKey: Self.missKey) as? [String: Double] ?? [:]
    }

    private func isKnownMiss(_ key: String) -> Bool {
        guard let at = readMisses()[key] else { return false }
        return Date().timeIntervalSince1970 - at < Self.missLifetime
    }

    private func recordMiss(_ key: String) {
        var misses = readMisses()
        misses[key] = Date().timeIntervalSince1970
        // An entry older than the lifetime is dropped while we are here, so the map cannot grow
        // without bound across years of imports.
        let cutoff = Date().timeIntervalSince1970 - Self.missLifetime
        misses = misses.filter { $0.value >= cutoff }
        defaults.set(misses, forKey: Self.missKey)
        rememberedMisses = misses.count
    }

    private func clearMiss(_ key: String) {
        var misses = readMisses()
        guard misses.removeValue(forKey: key) != nil else { return }
        defaults.set(misses, forKey: Self.missKey)
        rememberedMisses = misses.count
    }

    // ------------------------------------------------------------------ provenance

    private func provenance(forKey key: String) -> String? {
        (defaults.object(forKey: Self.provenanceKey) as? [String: String])?[key]
    }

    private func recordProvenance(_ provenance: String?, forKey key: String) {
        var map = defaults.object(forKey: Self.provenanceKey) as? [String: String] ?? [:]
        if let provenance {
            map[key] = provenance
        } else {
            map.removeValue(forKey: key)
        }
        defaults.set(map, forKey: Self.provenanceKey)
    }

    // ------------------------------------------------------------------ read-outs

    private func refreshUsage() async {
        let (files, bytes) = await ArtworkDisk.usage()
        storedFiles = files
        storedBytes = bytes
    }

    /// The Settings summary: what is stored, what is remembered, what went wrong.
    var storageLine: String {
        var parts = ["\(storedFiles) cover(s) stored", Self.byteText(storedBytes)]
        if rememberedMisses > 0 {
            parts.append("\(rememberedMisses) title(s) remembered as having no art")
        }
        if failedThisRun > 0 {
            parts.append("\(failedThisRun) lookup(s) failed this run")
        }
        return parts.joined(separator: " \u{00B7} ")
    }

    static func byteText(_ bytes: Int64) -> String {
        if bytes <= 0 { return "0 KB" }
        let megabytes = Double(bytes) / (1024.0 * 1024.0)
        if megabytes >= 1.0 { return String(format: "%.1f MB", megabytes) }
        return String(format: "%.0f KB", Double(bytes) / 1024.0)
    }

    private func shortAddress(_ address: String) -> String {
        // The host plus the folder is the useful part. The full URL is several hundred characters
        // once a long No-Intro name is percent-encoded and would push everything else off the line.
        guard let url = URL(string: address) else { return address }
        let folder = url.deletingLastPathComponent().lastPathComponent
        return "\(url.host ?? "thumbnails.libretro.com")/\(folder)"
    }

    /// The ordinary read-out path: updates the artwork line and mirrors it to the host so the
    /// diagnostics panel shows it.
    private func note(_ text: String) {
        line = text
        host?.artworkLine = text
    }

    /// A note that must not repeat. Used for the conditions that are true for a whole system rather
    /// than for one game, so thirty Game Gear carts do not write thirty identical lines.
    private func noteOnce(_ text: String) {
        guard line != text else { return }
        note(text)
    }

    /// Something the user just asked for: the artwork line AND the main status line, always.
    ///
    /// Every one of these is the direct result of a tap, so the main line is where the answer
    /// belongs, and there is no risk of it arriving unprompted over something the user was reading.
    private func report(_ text: String) {
        note(text)
        host?.status = text
    }

    /// A lookup that failed rather than missed.
    ///
    /// The artwork line takes it every time. The main status line takes it once per distinct
    /// reason, because a network failure that only ever appears behind the Settings tab is a silent
    /// failure, and a whole library's worth of the same failure is one condition rather than
    /// thirty.
    private func reportLookupFailure(reason: String, game: String) {
        let text = "artwork: \(failedThisRun) lookup(s) failed, \(reason). Latest: \(game)"
        note(text)
        guard lastNetworkReason != reason else { return }
        lastNetworkReason = reason
        host?.status = text
    }
}

// MARK: - The artwork picker's delegate

/// The tier 5 picker's delegate, shaped exactly like `ImportPickerDelegate` and for the same
/// reasons: a UIKit delegate must be an NSObject, the picker holds it weakly, and every callback
/// has to name itself so a silent sheet cannot be mistaken for a silent delegate.
final class ArtworkPickerDelegate: NSObject, UIDocumentPickerDelegate,
                                   UIAdaptivePresentationControllerDelegate {
    private unowned let store: ArtworkStore

    /// Set as soon as any pick or cancel callback fires, so the dismissal notice cannot overwrite a
    /// real result. The picker dismisses itself after a successful pick.
    private var outcomeDelivered = false

    init(store: ArtworkStore) {
        self.store = store
        super.init()
    }

    func prepareForNewPicker() {
        outcomeDelivered = false
    }

    @objc func documentPicker(_ controller: UIDocumentPickerViewController,
                              didPickDocumentsAt urls: [URL]) {
        let outcome = PickerOutcome(
            urls: urls,
            failureText: urls.isEmpty
                ? "the image picker fired with 0 urls, nothing to store"
                : nil
        )
        deliver(outcome)
    }

    /// The deprecated single-URL callback, kept alongside the array form rather than instead of it,
    /// so "the array selector is not being delivered" cannot hide.
    @objc func documentPicker(_ controller: UIDocumentPickerViewController,
                              didPickDocumentAt url: URL) {
        deliver(PickerOutcome(urls: [url], failureText: nil))
    }

    @objc func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
        outcomeDelivered = true
        let store = self.store
        Task { @MainActor in
            store.releaseActivePicker()
            store.handlePickedArtwork(
                PickerOutcome(urls: [], failureText: "the image picker was cancelled, "
                              + "the cover was left as it was")
            )
        }
    }

    @objc func presentationControllerDidDismiss(
        _ presentationController: UIPresentationController
    ) {
        if outcomeDelivered { return }
        let store = self.store
        Task { @MainActor in
            store.releaseActivePicker()
            store.handlePickedArtwork(
                PickerOutcome(urls: [], failureText: "the image picker was dismissed without a "
                              + "pick, the cover was left as it was")
            )
        }
    }

    private func deliver(_ outcome: PickerOutcome) {
        outcomeDelivered = true
        let store = self.store
        Task { @MainActor in
            store.releaseActivePicker()
            store.handlePickedArtwork(outcome)
        }
    }
}

// MARK: - The view that shows a cover

/// A cover, with its plate underneath.
///
/// THE PLATE IS ALWAYS DRAWN, and real art goes OVER it. That single decision is what makes the
/// library never blank: the plate is the empty state, the loading state, and the letterbox behind a
/// cover whose shape does not match its box, all without a second code path.
/// The store is held as a PLAIN reference rather than as an `@ObservedObject`, and the generation
/// is passed in as a value. That is deliberate: the store publishes a status line and a set of
/// counters every time a cover resolves, and observing it here would invalidate every card on
/// screen each time any one of them finished. The parent that lays the cards out observes the store
/// and hands the generation down, which is the only thing a card needs to know about.
struct CoverArtView: View {
    let entry: LibraryEntry
    let system: GameSystem?
    let store: ArtworkStore
    /// Bumped by the store when what a card is showing has been invalidated.
    let generation: Int
    /// Off for a thumbnail too small for the caption to be readable.
    var showsCaption = true

    @State private var cover: CoverImage?

    var body: some View {
        ZStack {
            ArtPlate(entry: entry, system: system, showsCaption: showsCaption && cover == nil)
            if let cover {
                Image(uiImage: cover.image)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
            }
        }
        .clipped()
        .animation(.easeIn(duration: 0.18), value: cover != nil)
        // Keyed on the entry AND the generation, so clearing the cache or turning lookups on makes
        // every visible card resolve again without the library being rebuilt.
        .task(id: "\(entry.id)#\(generation)") {
            cover = await store.cover(for: entry, system: system)
        }
    }
}
