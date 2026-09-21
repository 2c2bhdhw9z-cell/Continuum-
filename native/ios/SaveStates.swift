// Continuum - save states: the metadata index in memory, the payloads on disk, and the gate that
// decides whether a state may be handed back to a core at all.
//
// This is a rebuild of a feature the browser build had and the .ipa lost. The web app kept the
// same shape as this file does, one index plus separate payloads, and the reasoning below is
// carried over from it rather than invented here.
//
// ## THE COMPATIBILITY GATE IS THE WHOLE POINT OF THIS FILE
//
// A libretro save state is an opaque dump of a core's internal structs. `retro_unserialize` is
// NOT versioned and it is not required to validate what it is given. Handing a core a foreign
// state therefore does not reliably fail: it can SUCCEED into a machine whose internals are now
// subtly wrong, and that wrongness surfaces minutes later as a hang, a corrupted save file or a
// crash in code that has nothing to do with loading. That is the worst failure mode available,
// because the cause and the symptom are far enough apart that nobody connects them.
//
// So every record carries the core's id, the core's self-reported version and the exact byte
// length of the payload, and `refusal(for:)` refuses BEFORE the load rather than hoping the core
// notices. The four checks and what each one says are documented on `SaveStateRefusal`. An
// unknown on either side is treated as "cannot check this one" and falls through to the remaining
// checks, because a record written by an older build that did not store a version must not become
// unloadable: degrading to a weaker check is right, silently loading anyway is not, and refusing
// on missing metadata would throw away states that are almost certainly fine.
//
// ## Metadata in memory, payloads on disk
//
// Metadata is a few dozen bytes per state. Payloads are about 13 KB for the NES and a megabyte or
// more for the PlayStation. A list must never have to read a payload to draw itself, or opening a
// game's card would take time proportional to how much someone has played it. So the index is one
// small JSON file, read once when this object is built, and a payload is only ever read at the
// moment a state is actually being loaded.
//
// ## Numbered slots, plus exactly one auto-save per game
//
// Manual saves take numbered slots and accumulate. The auto-save lives in its own namespace
// (`autoSlot`, which is -1, the same sentinel the browser build used) and is overwritten in place.
// Keeping the two apart means an auto-save can never consume a slot number the user was using,
// and the resume path never has to guess which of several records is the newest.
//
// ## Application Support, not Documents
//
// Documents is user-visible through the Files app because the app sets `UIFileSharingEnabled` so
// that cue and bin tracks can be dropped in. A slot system has to own its own namespace: if a
// payload can be renamed, moved or deleted underneath the index by anyone browsing the Continuum
// folder, then the index is a set of claims about files that may no longer be what it says they
// are. Artwork is kept out of Documents for the matching reason, and `ArtworkDisk.directory()`
// states it; this is the same decision applied to something less replaceable, because a lost
// cover is one download and a lost save state is lost progress.
//
// Application Support itself is the system directory handed to the cores, where a core looks for
// its BIOS and writes its savedata, so this is a SUBDIRECTORY of it rather than loose files.

import Foundation
// For the resign-active and background notifications that trigger the auto-save. Imported
// explicitly rather than relying on Foundation to bring UIKit along, which it does not.
import UIKit

// MARK: - One state

/// The metadata for a single save state. The payload is a separate file.
///
/// `coreId` and `coreVersion` are optional because the gate has to cope with a record written by a
/// build that did not know to store them. Everything the gate can check is stored; nothing it
/// checks is derived at load time from something that could have changed since.
struct SaveStateRecord: Identifiable, Codable, Hashable {

    /// The auto-save's slot. Outside the numbered sequence on purpose, so it can never collide
    /// with a slot the user is using.
    static let autoSlot = -1

    /// Which game this belongs to.
    ///
    /// THE GAME'S FILENAME, not its absolute path, and that difference is deliberate. An iOS app's
    /// container path contains a UUID that does not survive being uninstalled and reinstalled,
    /// while a game in Documents keeps the filename it arrived with (the cue-sheet rule in
    /// `LibraryEntry` guarantees it: renaming a track would break the game). `ArtworkStore` keys on
    /// the absolute path because a cover that loses its key is one download; a save state that
    /// loses its key is somebody's progress. The filename is also exactly what the engine is told
    /// as `contentId` at launch, so this matches what the core thinks it is running.
    let gameId: String

    /// `autoSlot` for the auto-save, otherwise the number shown to the user.
    let slot: Int

    /// Stored as well as being derivable from `slot`, because it is the flag every caller actually
    /// wants and a second reader should not have to know that -1 is magic.
    let isAuto: Bool

    let createdAt: Date

    /// The emulated frame the state was taken at. Metadata only: it is how a user tells two states
    /// of the same game apart when both were taken in the same minute.
    let frame: UInt64

    /// The payload length in bytes, as written. Checked against `saveStateSize()` before a load,
    /// and against the real length of the file after it is read.
    let byteCount: Int

    /// The core that wrote it, for example "pcsx_rearmed". Optional: see the type's note.
    let coreId: String?

    /// The core's own `library_version` string at the time of the save.
    let coreVersion: String?

    /// Unique per game and slot, which is also the identity the UI needs: saving over the
    /// auto-save replaces a record rather than adding one.
    var id: String { isAuto ? "\(gameId)#auto" : "\(gameId)#\(slot)" }

    init(gameId: String, slot: Int, isAuto: Bool, createdAt: Date, frame: UInt64,
         byteCount: Int, coreId: String?, coreVersion: String?) {
        self.gameId = gameId
        self.slot = slot
        self.isAuto = isAuto
        self.createdAt = createdAt
        self.frame = frame
        self.byteCount = byteCount
        self.coreId = coreId
        self.coreVersion = coreVersion
    }

    // MARK: Storage

    private enum CodingKeys: String, CodingKey {
        case gameId, slot, isAuto, createdAt, frame, byteCount, coreId, coreVersion
    }

    /// Decodes field by field, each one falling back rather than throwing, exactly as
    /// `TouchLayout` does and for a much sharper version of the same reason.
    ///
    /// THE SYNTHESIZED INITIALISER THROWS THE MOMENT ANY ONE KEY IS MISSING, and this decoder runs
    /// over the whole index. So a single field renamed by a later build, or one record written by a
    /// build that did not have `coreVersion` yet, would fail the decode of the array and wipe every
    /// save state the user has. Per-field recovery keeps what it can still understand, and a record
    /// that keeps a weaker set of facts simply gets weaker compatibility checks, which is the
    /// documented behaviour of the gate rather than a special case.
    ///
    /// `createdAt` is carried as epoch seconds rather than as a `Date` so that it does not depend
    /// on the encoder's date strategy: a strategy changed in a later build would otherwise turn
    /// every timestamp into a decode failure.
    init(from decoder: Decoder) throws {
        let box = try decoder.container(keyedBy: CodingKeys.self)
        gameId = try box.decodeIfPresent(String.self, forKey: .gameId) ?? ""
        let storedSlot = try box.decodeIfPresent(Int.self, forKey: .slot) ?? Self.autoSlot
        slot = storedSlot
        // Falls back to the slot sentinel rather than to `false`. Reading a record with no flag as
        // a manual save would let it collide with a real numbered slot, and a record that claims
        // slot -1 is an auto-save by construction.
        isAuto = try box.decodeIfPresent(Bool.self, forKey: .isAuto) ?? (storedSlot < 0)
        let seconds = try box.decodeIfPresent(Double.self, forKey: .createdAt) ?? 0
        createdAt = Date(timeIntervalSince1970: seconds)
        frame = try box.decodeIfPresent(UInt64.self, forKey: .frame) ?? 0
        byteCount = try box.decodeIfPresent(Int.self, forKey: .byteCount) ?? 0
        // Absent stays absent rather than becoming an empty string: "" would read as a core id
        // that differs from every real one and would refuse a state that is probably fine.
        coreId = try box.decodeIfPresent(String.self, forKey: .coreId)
        coreVersion = try box.decodeIfPresent(String.self, forKey: .coreVersion)
    }

    func encode(to encoder: Encoder) throws {
        var box = encoder.container(keyedBy: CodingKeys.self)
        try box.encode(gameId, forKey: .gameId)
        try box.encode(slot, forKey: .slot)
        try box.encode(isAuto, forKey: .isAuto)
        try box.encode(createdAt.timeIntervalSince1970, forKey: .createdAt)
        try box.encode(frame, forKey: .frame)
        try box.encode(byteCount, forKey: .byteCount)
        try box.encodeIfPresent(coreId, forKey: .coreId)
        try box.encodeIfPresent(coreVersion, forKey: .coreVersion)
    }

    // MARK: Display

    /// "Auto" or "Slot 3". The auto-save is never given a number, because the number would be -1.
    var slotLabel: String { isAuto ? "Auto" : "Slot \(slot)" }

    var sizeText: String {
        if byteCount <= 0 { return "unknown size" }
        let megabytes = Double(byteCount) / (1024.0 * 1024.0)
        if megabytes >= 1.0 { return String(format: "%.1f MB", megabytes) }
        return String(format: "%.0f KB", Double(byteCount) / 1024.0)
    }

    /// How long ago it was taken, in the shape the browser build used.
    ///
    /// Hand-rolled rather than `RelativeDateTimeFormatter` so that the wording is fixed and cannot
    /// change under a locale or an OS revision. These strings sit in a list next to a slot number
    /// and a frame count, and they need to stay short enough not to wrap.
    var ageText: String {
        let seconds = max(0, Date().timeIntervalSince(createdAt))
        if createdAt.timeIntervalSince1970 <= 0 { return "date not recorded" }
        if seconds < 60 { return "\(Int(seconds))s ago" }
        if seconds < 3600 { return "\(Int(seconds / 60))m ago" }
        if seconds < 86_400 { return "\(Int(seconds / 3600))h ago" }
        if seconds < 30 * 86_400 { return "\(Int(seconds / 86_400))d ago" }
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .none
        return formatter.string(from: createdAt)
    }

    /// One line for a menu or a list row.
    var summaryLine: String {
        "\(slotLabel), \(ageText), \(sizeText), frame \(frame)"
    }
}

// MARK: - Why a load was refused

/// The four reasons a state may not be loaded, each with the sentence the user is shown.
///
/// ORDER MATTERS AND IS FIXED IN `SaveStates.refusal(for:)`: missing payload, then core id, then
/// core version, then byte length. It runs cheapest and most certain first, and it ends on the one
/// check that catches a core whose state layout moved without its version string changing.
///
/// Every message names the actual values. "That state cannot be loaded" teaches the user nothing
/// and makes the app look broken; "saved by snes9x, mgba is running now" tells them what happened
/// and what to do about it.
enum SaveStateRefusal {
    /// The index has a record and the payload file is gone.
    case payloadMissing
    /// A different core is running now.
    case coreMismatch(saved: String, running: String)
    /// The same core, rebuilt since.
    case versionMismatch(core: String, saved: String, running: String)
    /// The length the running core expects is not the length that was written.
    case sizeMismatch(saved: Int, expected: Int)

    var message: String {
        switch self {
        case .payloadMissing:
            return "That state is no longer stored."
        case .coreMismatch(let saved, let running):
            return "A state can only be loaded by the core that wrote it: this one was saved by "
                + "\(saved), and \(running) is running now."
        case .versionMismatch(let core, let saved, let running):
            return "The core has been rebuilt since this state was saved (\(core) \(saved) wrote "
                + "it, this build is \(running)), so its internal layout may have moved. Not "
                + "loading it, to avoid corrupting the game."
        case .sizeMismatch(let saved, let expected):
            return "This state does not match what the core expects (the state is \(saved) bytes "
                + "and this core expects \(expected)), so it was almost certainly written by a "
                + "different build."
        }
    }

    /// A short tag for the diagnostic line, where the sentence above would not fit.
    var tag: String {
        switch self {
        case .payloadMissing: return "payload missing"
        case .coreMismatch: return "different core"
        case .versionMismatch: return "core rebuilt"
        case .sizeMismatch: return "wrong state size"
        }
    }
}

// MARK: - Where the bytes live

/// The on-disk half. Synchronous on purpose, which is the opposite of `ArtworkDisk`.
///
/// Artwork reads and decodes off the main actor because it happens while the user is scrolling a
/// shelf and a stall would be visible. Every write in here happens at one of three moments: a
/// deliberate tap, a session being torn down, or the app being told it is about to stop being
/// active. THE LAST ONE IS WHY THESE ARE NOT ASYNC. After `willResignActive` the app has a short
/// and unspecified amount of time before iOS suspends it, and work handed to a task that has not
/// run yet is work that may never run. A state written synchronously on the notification is a
/// state that exists; one dispatched asynchronously is a promise. The serialize itself already
/// takes the engine lock on this actor, so the alternative was never free either.
enum SaveStateDisk {

    /// The directory, created on demand. Nil only when there is no Application Support at all,
    /// which would mean a broken container rather than a condition to design for.
    static func directory() -> URL? {
        guard let support = FileManager.default.urls(for: .applicationSupportDirectory,
                                                     in: .userDomainMask).first else {
            return nil
        }
        var directory = support.appendingPathComponent("SaveStates", isDirectory: true)
        if !FileManager.default.fileExists(atPath: directory.path) {
            do {
                try FileManager.default.createDirectory(at: directory,
                                                        withIntermediateDirectories: true)
            } catch {
                return nil
            }
            // NOT excluded from backup, and that is the one difference from the artwork
            // directory that matters. Downloaded covers are reproducible, so they have no
            // business in a backup. A save state cannot be reproduced by anything: it is the
            // only file in this app that represents time the user spent.
            var resourceValues = URLResourceValues()
            resourceValues.isExcludedFromBackup = false
            try? directory.setResourceValues(resourceValues)
        }
        return directory
    }

    static func indexURL() -> URL? {
        directory()?.appendingPathComponent("index.json")
    }

    /// `<hash of the game's filename>-<slot>.state`, or `-auto.state`.
    ///
    /// The hash comes from `ArtworkDisk.key(forPath:)` rather than a second copy of FNV-1a in this
    /// file. It is a plain 64-bit string hash; the label says "path" because artwork hashes one,
    /// and what is handed to it here is the game's filename, for the reason recorded on
    /// `SaveStateRecord.gameId`. A game id and an artwork key can therefore be the same string,
    /// which is harmless: they are different directories and different suffixes.
    static func payloadURL(gameId: String, slot: Int, isAuto: Bool) -> URL? {
        let key = ArtworkDisk.key(forPath: gameId)
        let suffix = isAuto ? "auto" : String(slot)
        return directory()?.appendingPathComponent("\(key)-\(suffix).state")
    }

    static func exists(gameId: String, slot: Int, isAuto: Bool) -> Bool {
        guard let url = payloadURL(gameId: gameId, slot: slot, isAuto: isAuto) else { return false }
        return FileManager.default.fileExists(atPath: url.path)
    }

    /// Writes a payload. Returns nil on success, or the sentence to put on screen.
    ///
    /// `.atomic` so that a write interrupted by the app being killed leaves the previous payload
    /// intact rather than a truncated file that the byte-length check would later reject. That
    /// matters most for the auto-save, which is written precisely when the app is going away.
    static func write(_ data: Data, gameId: String, slot: Int, isAuto: Bool) -> String? {
        guard let url = payloadURL(gameId: gameId, slot: slot, isAuto: isAuto) else {
            return "no Application Support directory, so there is nowhere to keep it"
        }
        do {
            try data.write(to: url, options: .atomic)
            return nil
        } catch {
            return "it could not be written: \(error.localizedDescription)"
        }
    }

    /// Reads a payload, or nil when it is not there or cannot be read.
    ///
    /// NOT memory-mapped, unlike the artwork loader. A mapped file whose contents change or
    /// disappear underneath the mapping is undefined behaviour, and this buffer is handed
    /// straight to a core that copies it into its own structs.
    static func read(gameId: String, slot: Int, isAuto: Bool) -> Data? {
        guard let url = payloadURL(gameId: gameId, slot: slot, isAuto: isAuto) else { return nil }
        return try? Data(contentsOf: url)
    }

    @discardableResult
    static func remove(gameId: String, slot: Int, isAuto: Bool) -> Bool {
        guard let url = payloadURL(gameId: gameId, slot: slot, isAuto: isAuto) else { return false }
        return (try? FileManager.default.removeItem(at: url)) != nil
    }

    /// Writes the index. Returns nil on success, or a sentence.
    static func writeIndex(_ records: [SaveStateRecord]) -> String? {
        guard let url = indexURL() else {
            return "no Application Support directory, so the list could not be kept"
        }
        do {
            let encoder = JSONEncoder()
            // Sorted keys so two identical indexes produce identical bytes, which makes a
            // corrupted file obvious when it is read by eye on a device with no debugger.
            encoder.outputFormatting = [.sortedKeys]
            try encoder.encode(records).write(to: url, options: .atomic)
            return nil
        } catch {
            return "the list could not be written: \(error.localizedDescription)"
        }
    }

    /// Reads the index. An absent file is an empty list, not a failure: that is a first launch.
    ///
    /// Malformed JSON is the one thing per-field decoding cannot recover from, so it is caught
    /// here and reported as a count rather than thrown. The file is NOT deleted on a parse
    /// failure: leaving it alone means a future build with a fixed decoder can still read it,
    /// and the payloads it describes are still on disk either way.
    static func readIndex() -> (records: [SaveStateRecord], failure: String?) {
        guard let url = indexURL() else { return ([], "no Application Support directory") }
        guard let data = try? Data(contentsOf: url), !data.isEmpty else { return ([], nil) }
        do {
            let records = try JSONDecoder().decode([SaveStateRecord].self, from: data)
            return (records, nil)
        } catch {
            return ([], "the stored list could not be read: \(error.localizedDescription)")
        }
    }

    /// Every `.state` file in the directory, for the sweep that deletes them all.
    static func payloadFiles() -> [URL] {
        guard let directory = directory() else { return [] }
        guard let contents = try? FileManager.default.contentsOfDirectory(
            at: directory,
            includingPropertiesForKeys: [.fileSizeKey],
            options: [.skipsHiddenFiles]
        ) else { return [] }
        return contents.filter { $0.pathExtension == "state" }
    }
}

// MARK: - The store

/// Every save state on the device: the index, the four-check gate, and the auto-save.
///
/// An `ObservableObject` of its own rather than more properties on `EngineHost`, following
/// `ArtworkStore` and `EmulationSettings`: it has its own storage, its own defaults and its own
/// reason to exist, and `EngineHost` is already the largest type in the app.
@MainActor
final class SaveStates: ObservableObject {

    // MARK: State

    /// Every record, newest first within a game. Published so a list redraws when a state is
    /// written or deleted.
    @Published private(set) var records: [SaveStateRecord] = []

    /// This object's own read-out, for Settings and the diagnostics. Never an empty string, so a
    /// reader can always tell the difference between "nothing has happened" and "the line is not
    /// being written".
    @Published private(set) var line = "save states: nothing written yet"

    /// Whether launching a game restores its auto-save.
    ///
    /// Default ON, which is what the browser build did. The argument for the default is that the
    /// auto-save exists precisely because a phone takes the app away mid-game: a resume that has
    /// to be asked for is a resume that has already been forgotten about by the time the user
    /// comes back. It is a toggle rather than a fixed behaviour because starting from the
    /// beginning is a legitimate thing to want, and the alternative would be deleting the
    /// auto-save to get it.
    @Published var resumesAutomatically: Bool = true {
        didSet {
            guard oldValue != resumesAutomatically else { return }
            UserDefaults.standard.set(resumesAutomatically, forKey: Self.resumeKey)
        }
    }

    /// How many records named a payload that is not on disk, counted when the index was read.
    ///
    /// Reported rather than repaired. Dropping those records silently would be the app deciding on
    /// its own that a state the user took no longer exists, and the commonest cause is not
    /// corruption at all: it is a payload that has not been restored yet, or a directory that was
    /// not writable at the moment of a write. The row stays in the list and says so, and the gate
    /// refuses it with "That state is no longer stored." if it is tapped.
    @Published private(set) var missingPayloads = 0

    private let engine: ContinuumEngine

    /// Weak, and the same shape as `ArtworkStore.attach(host:)`. This object needs two things from
    /// the host that it cannot know by itself: which game is running, and the status line. Weak
    /// because the host owns this object.
    private weak var host: EngineHost?

    /// Notification tokens, removed in `deinit`. Block-based observers outlive the object that
    /// registered them unless they are removed by hand.
    private var observers: [NSObjectProtocol] = []

    /// The id and frame of the last auto-save written.
    ///
    /// The dedup for a real double-fire: leaving the player screen calls `stopSession()`, and
    /// closing the app calls `willResignActive` and `didEnterBackground`, and two of those three
    /// arrive together often enough to matter. Comparing against the emulated frame is better than
    /// a timestamp because it answers the question that actually matters, which is whether
    /// anything has happened since the last write. Nothing has advanced, so there is nothing to
    /// write, so the second trigger costs no serialize at all.
    private var lastAutoSaveKey: String?

    /// Guards against re-entering the write while one is in progress. `saveState()` takes the
    /// engine lock and does not call back into this object, so this is belt and braces rather
    /// than a known path.
    private var writingAuto = false

    private static let resumeKey = "continuum.saveStates.autoResume.v1"

    // MARK: Lifecycle

    init(engine: ContinuumEngine) {
        self.engine = engine

        // Read before anything can display, falling back to the default rather than to nil so a
        // first launch and a value corrupted by a crash behave identically. Note `didSet` does not
        // fire for an assignment made inside `init`, so this cannot loop back into the write above.
        if let stored = UserDefaults.standard.object(forKey: Self.resumeKey) as? Bool {
            resumesAutomatically = stored
        }

        hydrate()
        observe()
    }

    deinit {
        for observer in observers {
            NotificationCenter.default.removeObserver(observer)
        }
    }

    /// Wired after `EngineHost.init` has finished with `self`, exactly as `ArtworkStore.attach` is.
    func attach(host: EngineHost) {
        self.host = host
    }

    /// Reads the index once. Payloads are deliberately not touched beyond asking whether each one
    /// exists, which is a stat and not a read.
    private func hydrate() {
        let (stored, failure) = SaveStateDisk.readIndex()
        // A record with no game id cannot be matched to a game, shown under one or deleted with
        // one, so it is the one thing dropped here. Counted, not ignored.
        let usable = stored.filter { !$0.gameId.isEmpty }
        let dropped = stored.count - usable.count
        records = Self.sortedNewestFirst(usable)
        refreshMissingCount()

        var parts: [String] = []
        if let failure {
            parts.append("save states: \(failure)")
        } else {
            parts.append("save states: \(records.count) record(s) for \(gameCount) game(s)")
        }
        if dropped > 0 {
            parts.append("\(dropped) unreadable record(s) skipped")
        }
        if missingPayloads > 0 {
            parts.append("\(missingPayloads) payload(s) not on disk")
        }
        line = parts.joined(separator: ", ")
    }

    /// The two triggers, plus the reason each one is here.
    private func observe() {
        let centre = NotificationCenter.default

        // `queue: .main` makes the delivery queue this app's guarantee rather than a framework's
        // promise, and `Task { @MainActor in }` is the ISOLATION, which is a different thing from
        // the thread. Whether the block NotificationCenter takes is `@Sendable` varies by SDK, and
        // calling a main-actor method straight from the nonisolated case is a compile error rather
        // than a warning. `PhysicalControllers.observe()` carries the full account.
        //
        // The hop costs one main-actor turn. That is acceptable here and it is worth being honest
        // about why: iOS gives an app a short window after these notifications, and a turn is
        // nothing next to it, but a task that had to wait behind a long main-actor job could still
        // lose the race. The `stopSession()` trigger is the one that is guaranteed to have run,
        // because it is called inline on the way out of a game.
        observers = [
            // The load-bearing one. Fires for a home swipe, an app switch, a call, or the control
            // centre, and it fires BEFORE the app is suspended, which `didEnterBackground` does
            // not always do in time.
            centre.addObserver(forName: UIApplication.willResignActiveNotification,
                               object: nil, queue: .main) { [weak self] _ in
                Task { @MainActor in self?.writeAutoSave(reason: "the app was leaving the front") }
            },
            // The backstop. A transient interruption that never became a background transition has
            // already been covered above, and the frame comparison makes this second trigger free
            // when nothing has been emulated in between.
            centre.addObserver(forName: UIApplication.didEnterBackgroundNotification,
                               object: nil, queue: .main) { [weak self] _ in
                Task { @MainActor in self?.writeAutoSave(reason: "the app went to the background") }
            },
        ]

        // DELIBERATELY NO TIMER. `retro_serialize` on a PlayStation state is a megabyte of struct
        // copying inside the engine lock, and a hitch every few seconds while a game is being
        // played is a worse experience than an auto-save that is a few minutes older than it could
        // have been. The rewind tape already covers the "undo the last few seconds" case, and it
        // is built for that: it snapshots on the engine's own thread against a memory budget.
        //
        // These are the APP-level notifications rather than the scene ones, and the app has exactly
        // one `WindowGroup`, so the two are the same event here. If a future iOS ever stops posting
        // the app-level pair to a scene-based app, the replacement is a `.onChange(of: scenePhase)`
        // in `ContinuumApp.body` calling this method, NOT a timer: the trigger would move and the
        // reasoning above would not. Worth naming, because an auto-save that silently stopped
        // firing looks exactly like a save system that never worked, and `stopSession()` would go on
        // covering the "left the game" half and hide it.
    }

    // MARK: Reading the index

    private static func sortedNewestFirst(_ list: [SaveStateRecord]) -> [SaveStateRecord] {
        list.sorted { $0.createdAt > $1.createdAt }
    }

    /// Every state for one game, newest first.
    var gameCount: Int { Set(records.map { $0.gameId }).count }

    func states(forGameId gameId: String) -> [SaveStateRecord] {
        records.filter { $0.gameId == gameId }
    }

    func states(for entry: LibraryEntry) -> [SaveStateRecord] {
        states(forGameId: Self.gameId(for: entry))
    }

    func autoState(forGameId gameId: String) -> SaveStateRecord? {
        records.first { $0.gameId == gameId && $0.isAuto }
    }

    func hasAutoState(for entry: LibraryEntry) -> Bool {
        autoState(forGameId: Self.gameId(for: entry)) != nil
    }

    /// The id a library entry maps to. One place, so a saver and a lister cannot disagree.
    static func gameId(for entry: LibraryEntry) -> String { entry.name }

    /// Whether a payload is actually on disk, for a row that wants to say so before it is tapped.
    func isStored(_ record: SaveStateRecord) -> Bool {
        SaveStateDisk.exists(gameId: record.gameId, slot: record.slot, isAuto: record.isAuto)
    }

    /// The next free numbered slot for a game.
    ///
    /// The highest number seen plus one, rather than the lowest unused number. Recycling a number
    /// means a state the user deleted is replaced by an unrelated one under a name they still
    /// recognise, and the slot number is shown in a list, so it is a name. Monotonic numbering
    /// leaves gaps, and a gap is a much cheaper thing to explain.
    private func nextSlot(forGameId gameId: String) -> Int {
        let numbered = states(forGameId: gameId).filter { !$0.isAuto }.map { $0.slot }
        return (numbered.max() ?? 0) + 1
    }

    // MARK: The gate

    /// Whether this state may be handed to the running core, and if not, why not.
    ///
    /// THE ORDER IS THE CONTRACT, and it runs from the cheapest and most certain to the strongest:
    ///
    ///   1. is the payload still on disk
    ///   2. is the running core the one that wrote it
    ///   3. is it the same BUILD of that core
    ///   4. does the length the core expects right now match the length that was written
    ///
    /// Each check needs both halves to be known. A record with no stored core id, or a running core
    /// that reports no id, means that particular question cannot be answered, so it falls through
    /// to the next check rather than failing: the alternative would make every state written by an
    /// older build permanently unloadable. Check 4 is the one that catches a core whose internal
    /// layout moved without its version string changing, which is why it is last rather than first.
    ///
    /// Returns nil when the state may be loaded.
    func refusal(for record: SaveStateRecord) -> SaveStateRefusal? {
        guard SaveStateDisk.exists(gameId: record.gameId, slot: record.slot,
                                   isAuto: record.isAuto) else {
            return .payloadMissing
        }

        let runningCore = engine.currentCoreId()
        if let saved = record.coreId, !saved.isEmpty,
           let running = runningCore, !running.isEmpty, saved != running {
            return .coreMismatch(saved: saved, running: running)
        }

        let runningVersion = engine.coreVersion()
        if let saved = record.coreVersion, !saved.isEmpty,
           let running = runningVersion, !running.isEmpty, saved != running {
            return .versionMismatch(core: record.coreId ?? runningCore ?? "the core",
                                    saved: saved, running: running)
        }

        // `saveStateSize()` is what the core expects RIGHT NOW: it is asked of the live session
        // rather than remembered from the launch, because it is the only figure here that can be
        // read straight from the machine the state is about to be pushed into. Zero means the core
        // does not support states at all, which is not a mismatch and is refused by the save and
        // load paths on their own terms.
        let expected = Int(engine.saveStateSize())
        if record.byteCount > 0, expected > 0, record.byteCount != expected {
            return .sizeMismatch(saved: record.byteCount, expected: expected)
        }

        return nil
    }

    // MARK: Saving

    /// Saves the running game into its next free numbered slot.
    ///
    /// Reports on `line` and on the host's status line, because both matter: the status line is
    /// what a player is looking at, and `line` is what the Settings screen and the diagnostics
    /// panel show later.
    func saveToNewSlot() {
        guard let entry = runningEntry() else {
            report("save state ignored: no game is running")
            return
        }
        let gameId = Self.gameId(for: entry)
        write(gameId: gameId, slot: nextSlot(forGameId: gameId), isAuto: false,
              describe: { record in
                  "saved \(record.slotLabel.lowercased()) for \(gameId), \(record.sizeText) "
                      + "at frame \(record.frame)"
              })
    }

    /// Writes this game's auto-save, overwriting the previous one.
    ///
    /// IT DOES NOT TOUCH THE STATUS LINE, and that is the one deliberate difference from a manual
    /// save. This fires while the player is being left or the app is being put away, so the status
    /// line is about to say something the user asked for, and an auto-save announcing itself over
    /// the top of that would be the app talking about its own housekeeping. The auto-save is still
    /// recorded on `line`, which is what Settings and the diagnostics panel read, because a lost
    /// auto-save that left no trace anywhere is how somebody loses an hour and never finds out why.
    func writeAutoSave(reason: String) {
        guard !writingAuto else { return }
        guard let entry = runningEntry() else { return }
        let gameId = Self.gameId(for: entry)

        // Nothing has been emulated since the last auto-save, so the payload would be identical.
        // See `lastAutoSaveKey`.
        let frame = engine.frameCount()
        let key = "\(gameId)#\(frame)"
        guard key != lastAutoSaveKey else { return }

        writingAuto = true
        defer { writingAuto = false }

        let written = write(gameId: gameId, slot: SaveStateRecord.autoSlot, isAuto: true,
                           describe: { record in
                               "auto-saved \(gameId) at frame \(record.frame), "
                                   + "\(record.sizeText) (\(reason))"
                           }, announce: false)
        // Recorded from the record that was actually written rather than from the frame read at the
        // top of this function. They are the same number today, because the engine is ticked from
        // the display link on this actor and nothing can advance a frame in the middle of this
        // function, but taking it from the record means the dedup stays correct if that ever stops
        // being true.
        if let written {
            lastAutoSaveKey = "\(gameId)#\(written.frame)"
        }
    }

    /// The one write path: serialize, check, store the payload, then update the index.
    ///
    /// ORDER IS LOAD-BEARING. The payload is written BEFORE the index is, so an interrupted write
    /// leaves an orphaned payload, which costs some bytes and nothing else. The other order would
    /// leave the index claiming a state that was never stored, which is a row that fails when it is
    /// tapped, and the whole point of keeping metadata separate is that the list can be trusted.
    @discardableResult
    private func write(gameId: String, slot: Int, isAuto: Bool,
                       describe: (SaveStateRecord) -> String,
                       announce: Bool = true) -> SaveStateRecord? {
        guard engine.saveStateSize() > 0 else {
            report("save state refused: \(engine.currentCoreId() ?? "this core") does not "
                   + "support save states", announce: announce)
            return nil
        }

        let bytes: Data
        do {
            bytes = Data(try engine.saveState())
        } catch {
            report("save state refused by \(engine.currentCoreId() ?? "the core"): \(error)",
                   announce: announce)
            return nil
        }
        guard !bytes.isEmpty else {
            report("save state came back empty for \(gameId); nothing was written",
                   announce: announce)
            return nil
        }

        if let failure = SaveStateDisk.write(bytes, gameId: gameId, slot: slot, isAuto: isAuto) {
            report("save state for \(gameId) was not kept: \(failure)", announce: announce)
            return nil
        }

        let record = SaveStateRecord(
            gameId: gameId,
            slot: slot,
            isAuto: isAuto,
            createdAt: Date(),
            frame: engine.frameCount(),
            byteCount: bytes.count,
            // Stored at the moment of the save and never derived later. These three values are the
            // whole gate, and reading them back from a live engine at load time would be comparing
            // the core against itself.
            coreId: engine.currentCoreId(),
            coreVersion: engine.coreVersion()
        )

        // Replaces any record with the same identity, which is how the auto-save is overwritten in
        // place and why a numbered slot cannot be shadowed by a second record.
        var updated = records.filter { $0.id != record.id }
        updated.append(record)
        records = Self.sortedNewestFirst(updated)

        if let failure = SaveStateDisk.writeIndex(records) {
            // The payload is on disk and this session can see it, but a relaunch will not. Said
            // plainly rather than reported as a success. The record is NOT returned, so the
            // auto-save path treats this as the failure it is and keeps the explanation on screen.
            report("saved the state for \(gameId), but \(failure), so it will not survive a "
                   + "relaunch", announce: announce)
            return nil
        }

        refreshMissingCount()
        report(describe(record), announce: announce)
        return record
    }

    /// Recounts the records whose payload is not on disk. Called after every mutation, because the
    /// count is shown in Settings and a stale one would be a claim about the filesystem that is no
    /// longer true. A `stat` per record over a list of dozens, so it costs nothing worth saving.
    private func refreshMissingCount() {
        missingPayloads = records.filter {
            !SaveStateDisk.exists(gameId: $0.gameId, slot: $0.slot, isAuto: $0.isAuto)
        }.count
    }

    // MARK: Loading

    /// Loads a state into the running session, or explains why it will not.
    ///
    /// Returns whether the load happened, because the resume path needs to know and the UI does
    /// not. Synchronous, all the way through: the payload read is one file, and doing it here
    /// rather than in a task removes the whole class of bug the browser build needed a launch token
    /// for, where an awaited read completes after the user has already started a different game and
    /// pushes one game's state into another's core.
    @discardableResult
    func load(_ record: SaveStateRecord) -> Bool {
        guard let entry = runningEntry() else {
            report("states can only be loaded into a running game, so nothing was loaded")
            return false
        }
        // The running game has to be the game the state belongs to. The core id check below would
        // usually catch this, and usually is not good enough: two Mega Drive games run on the same
        // core, produce the same state size and would sail through every check in the gate.
        guard Self.gameId(for: entry) == record.gameId else {
            report("that state belongs to \(record.gameId) and \(entry.name) is running, so it "
                   + "was not loaded")
            return false
        }

        if let refusal = refusal(for: record) {
            report("state not loaded (\(refusal.tag)): \(refusal.message)")
            return false
        }

        guard let data = SaveStateDisk.read(gameId: record.gameId, slot: record.slot,
                                            isAuto: record.isAuto) else {
            report("state not loaded: \(SaveStateRefusal.payloadMissing.message)")
            return false
        }
        // The gate compared the LENGTH THE INDEX CLAIMS against the core. This compares the length
        // the file actually has, which is the same question asked of a different source and catches
        // the one case the index cannot see: a payload truncated by a write that did not finish.
        // Its own sentence rather than `SaveStateRefusal.sizeMismatch`, because that message says
        // "this core expects", and the figure being disagreed with here is the list's, not a core's.
        if record.byteCount > 0, data.count != record.byteCount {
            report("state not loaded: its file is \(data.count) bytes and the list recorded "
                   + "\(record.byteCount), so the file was not written completely.")
            return false
        }

        do {
            try engine.loadState(data: Array(data))
        } catch {
            // The gate passed and the core still refused. Reported rather than swallowed: this is
            // the one path that says the four checks were not enough, and it is worth knowing.
            report("\(engine.currentCoreId() ?? "the core") rejected that state: \(error)")
            return false
        }

        // The engine clears the rewind tape on a load of its own accord, because winding back past
        // a load would take the player somewhere they never were. Nothing to do here, and this note
        // is here so the next reader does not go looking for the missing call.
        report("loaded \(record.slotLabel.lowercased()) for \(record.gameId), taken "
               + "\(record.ageText) at frame \(record.frame)")
        return true
    }

    /// The resume path, called by `EngineHost.launch` on a successful launch.
    ///
    /// Returns whether the game was resumed. Every outcome that is not "there was nothing to
    /// resume" writes a line, because a game that silently starts from the beginning when the user
    /// expected their auto-save is indistinguishable from lost progress.
    @discardableResult
    func resumeIfPossible(entry: LibraryEntry) -> Bool {
        guard resumesAutomatically else { return false }
        guard let record = autoState(forGameId: Self.gameId(for: entry)) else { return false }

        if let refusal = refusal(for: record) {
            report("could not resume \(entry.name) (\(refusal.tag)): \(refusal.message) Starting "
                   + "from the beginning.")
            return false
        }

        guard load(record) else { return false }
        // Overwrites the line `load` just wrote, on purpose: "resumed" is the thing that happened
        // from the user's point of view, and a launch that resumed should not read like a manual
        // load they did not ask for.
        report("resumed \(entry.name) from the auto-save, taken \(record.ageText) at frame "
               + "\(record.frame)")
        // The auto-save was just consumed, and the emulated frame is now the frame it was taken at,
        // so the next trigger would find nothing to do. Recorded so that is the case for real
        // rather than by luck.
        lastAutoSaveKey = "\(record.gameId)#\(engine.frameCount())"
        return true
    }

    // MARK: Deleting

    /// Deletes one state, payload first.
    ///
    /// The index is rewritten even when the payload could not be removed, because a record whose
    /// payload is stuck is worse than an orphaned file: the row would stay in the list and refuse
    /// every tap.
    func delete(_ record: SaveStateRecord) {
        let removed = SaveStateDisk.remove(gameId: record.gameId, slot: record.slot,
                                           isAuto: record.isAuto)
        records = records.filter { $0.id != record.id }
        let indexFailure = SaveStateDisk.writeIndex(records)
        refreshMissingCount()

        if let indexFailure {
            report("deleted \(record.slotLabel.lowercased()) for \(record.gameId), but "
                   + indexFailure)
        } else if removed {
            report("deleted \(record.slotLabel.lowercased()) for \(record.gameId)")
        } else {
            report("removed \(record.slotLabel.lowercased()) for \(record.gameId) from the list; "
                   + "its file was already gone")
        }
    }

    /// Deletes every state for one game. Used by the detail sheet, and by the game delete path so
    /// that deleting a game does not leave its states behind claiming it still exists.
    func deleteAll(forGameId gameId: String) {
        let doomed = states(forGameId: gameId)
        guard !doomed.isEmpty else { return }
        for record in doomed {
            SaveStateDisk.remove(gameId: record.gameId, slot: record.slot, isAuto: record.isAuto)
        }
        records = records.filter { $0.gameId != gameId }
        let failure = SaveStateDisk.writeIndex(records)
        refreshMissingCount()
        if let failure {
            report("deleted \(doomed.count) state(s) for \(gameId), but \(failure)")
        } else {
            report("deleted \(doomed.count) state(s) for \(gameId)")
        }
    }

    /// Deletes everything, including payloads the index does not know about.
    ///
    /// The directory sweep is the point rather than a flourish: an interrupted write can leave a
    /// payload with no record, and a "delete everything" that left megabytes behind would be a
    /// button that does not do what it says.
    func deleteEverything() {
        let knownCount = records.count
        var files = 0
        var bytes: Int64 = 0
        for url in SaveStateDisk.payloadFiles() {
            let size = (try? url.resourceValues(forKeys: [.fileSizeKey]))?.fileSize ?? 0
            if (try? FileManager.default.removeItem(at: url)) != nil {
                files += 1
                bytes += Int64(size)
            }
        }
        records = []
        missingPayloads = 0
        let failure = SaveStateDisk.writeIndex(records)
        let orphans = files - knownCount
        var text = "deleted every save state: \(files) file(s), \(Self.byteText(bytes))"
        if orphans > 0 {
            text += ", including \(orphans) the list did not know about"
        }
        if let failure {
            text += ", but \(failure)"
        }
        report(text)
    }

    // MARK: Read-outs

    /// What is stored, for the Settings screen. Read from the index rather than from the disk, so
    /// drawing it costs nothing.
    var storageLine: String {
        guard !records.isEmpty else { return "no save states stored yet" }
        let bytes = records.reduce(Int64(0)) { $0 + Int64(max(0, $1.byteCount)) }
        let autoCount = records.filter { $0.isAuto }.count
        var parts = [
            "\(records.count) state(s) for \(gameCount) game(s)",
            Self.byteText(bytes),
            "\(autoCount) auto-save(s)",
        ]
        if missingPayloads > 0 {
            parts.append("\(missingPayloads) payload(s) missing")
        }
        return parts.joined(separator: " \u{00B7} ")
    }

    /// One line for the diagnostics HUD. Short, because it shares a strip.
    var diagnosticLine: String {
        var parts = ["states \(records.count)"]
        parts.append(resumesAutomatically ? "resume on" : "resume off")
        if missingPayloads > 0 {
            parts.append("\(missingPayloads) missing")
        }
        if let entry = host?.activeEntry, hasAutoState(for: entry) {
            parts.append("auto-save present")
        }
        return parts.joined(separator: " | ")
    }

    static func byteText(_ bytes: Int64) -> String {
        if bytes <= 0 { return "0 KB" }
        let megabytes = Double(bytes) / (1024.0 * 1024.0)
        if megabytes >= 1.0 { return String(format: "%.1f MB", megabytes) }
        return String(format: "%.0f KB", Double(bytes) / 1024.0)
    }

    // MARK: Plumbing

    /// The running game, or nil. Asked of the host rather than tracked here, so there is one answer
    /// to "what is running" in the app and it is the one the launch path set.
    private func runningEntry() -> LibraryEntry? {
        guard let host, host.running else { return nil }
        return host.activeEntry
    }

    /// Writes this object's read-out, and the host's status line unless told not to.
    ///
    /// Both, rather than one: the status line is what a player is looking at during a game and is
    /// overwritten by the next thing that happens, while `line` survives to be read in Settings.
    private func report(_ text: String, announce: Bool = true) {
        line = text
        if announce {
            host?.status = text
        }
    }
}
