// Continuum - the skin library: many skins per system, a default per system, a skin per game.
//
// PURE FOUNDATION ON PURPOSE. Everything here is plain data and rules, so it compiles with real
// `swiftc` on Linux and the rules can be checked there by a program rather than only read. The
// files and pictures for each skin live on disk under its id (see `EngineHost`'s skin storage);
// this file only says which skin a system or a game uses.
//
// Systems are keyed by their id STRING ("gba", "segacd", "dos"), never by `GameSystem`, so a skin
// for a console this build has no enum case for yet is still stored, listed and kept. The caller
// turns the string into a `GameSystem` with `GameSystem(rawValue:)` at the moment it needs one.
//
// Skins from before the library existed were stored one per system, keyed by the system id. They
// become library records whose id IS that system id, so their files on disk (named after the
// system) are found without moving anything.

import Foundation

// MARK: - Which console a skin file is for

/// `gameTypeIdentifier` values from Manic EMU and Delta, mapped onto Continuum system ids.
enum SkinGameTypes {
    /// Manic writes `public.aoshuang.game.<suffix>`. One suffix can mean several systems here:
    /// Manic's `pce` covers both the HuCard and the CD add-on.
    static let manicPrefix = "public.aoshuang.game."

    static let manic: [String: [String]] = [
        "wsc": ["wswan"],
        "flash": ["flash"],
        "wii": ["wii"],
        "ngc": ["gamecube"],
        "amiga": ["amiga"],
        "c64": ["c64"],
        "ngp": ["ngp"],
        "pce": ["tg16", "pcecd"],
        "symbian": ["symbian"],
        "dos": ["dos"],
        "j2me": ["j2me"],
        "doom": ["doom"],
        "jaguar": ["jaguar"],
        "lynx": ["lynx"],
        "7800": ["atari7800"],
        "5200": ["atari5200"],
        "2600": ["atari2600"],
        "arcade": ["arcade"],
        "dc": ["dreamcast"],
        "ps1": ["ps1"],
        "pm": ["pokemini"],
        "vb": ["vb"],
        "n64": ["n64"],
        "ss": ["saturn"],
        "md": ["genesis"],
        "mcd": ["segacd"],
        "32x": ["sega32x"],
        "ms": ["sms"],
        "gg": ["gg"],
        "sg1000": ["sg1000"],
        "psp": ["psp"],
        "3ds": ["n3ds"],
        "ds": ["ds"],
        "gba": ["gba"],
        "gbc": ["gbc"],
        "gb": ["gb"],
        "nes": ["nes"],
        "snes": ["snes"],
        // Not in Manic's published list, accepted because a skin maker may well write them.
        "fds": ["fds"],
        "sgx": ["sgx"],
    ]

    /// Delta's identifiers. Delta's Game Boy Color core plays both Game Boys, so its id names both.
    static let delta: [String: [String]] = [
        "com.rileytestut.delta.game.gb": ["gb"],
        "com.rileytestut.delta.game.gbc": ["gbc", "gb"],
        "com.rileytestut.delta.game.gba": ["gba"],
        "com.rileytestut.delta.game.ds": ["ds"],
        "com.rileytestut.delta.game.nes": ["nes"],
        "com.rileytestut.delta.game.snes": ["snes"],
        "com.rileytestut.delta.game.n64": ["n64"],
        "com.rileytestut.delta.game.genesis": ["genesis"],
        "com.rileytestut.delta.game.ps1": ["ps1"],
    ]

    /// Every Continuum system id the identifier names, most specific first. Empty when unknown.
    static func systemIDs(forGameType identifier: String?) -> [String] {
        guard let raw = identifier?.trimmingCharacters(in: .whitespacesAndNewlines).lowercased(),
              !raw.isEmpty else { return [] }
        if raw.hasPrefix(manicPrefix) {
            let suffix = String(raw.dropFirst(manicPrefix.count))
            return manic[suffix] ?? []
        }
        return delta[raw] ?? []
    }

    /// True for a Manic identifier, so the import summary can say which format it read.
    static func isManic(_ identifier: String?) -> Bool {
        identifier?.lowercased().hasPrefix(manicPrefix) ?? false
    }
}

// MARK: - Sharing across related systems

/// Systems whose skins fit each other, the way Manic shares them. A Game Boy skin works on a Game
/// Boy Color game, a Mega Drive skin on a Sega CD or 32X game, and so on.
enum SkinSharing {
    static let groups: [[String]] = [
        ["gb", "gbc"],
        ["genesis", "segacd", "sega32x"],
        ["sms", "gg", "sg1000"],
        ["nes", "fds"],
        ["dos", "doom"],
        // Beyond Manic's list: the PC Engine family shares one pad.
        ["tg16", "pcecd", "sgx"],
    ]

    /// The other systems in `system`'s group, not including itself, in group order.
    static func related(to system: String) -> [String] {
        guard let group = groups.first(where: { $0.contains(system) }) else { return [] }
        return group.filter { $0 != system }
    }

    /// True when a skin made for `skinSystems` may be drawn on `system`.
    static func fits(skinSystems: [String], on system: String) -> Bool {
        if skinSystems.contains(system) { return true }
        let family = related(to: system)
        return skinSystems.contains { family.contains($0) }
    }
}

// MARK: - The records

/// One skin in the library. Its art, pieces and sound live on disk under `id`.
struct SkinRecord: Codable, Equatable, Identifiable, Sendable {
    var id: String
    var name: String
    /// The skin's own reverse-DNS identifier from info.json, when it had one.
    var identifier: String
    var gameTypeIdentifier: String
    /// Continuum system ids this skin was made for. Related systems are added at use, not here.
    var systems: [String]
    var importedAt: Date
    var sourceFileName: String
    /// "manic", "delta", or "legacy" for a skin from before the library.
    var format: String
    /// True when the package had a `sound.caf` that was kept.
    var hasSound: Bool

    init(id: String, name: String, identifier: String = "", gameTypeIdentifier: String = "",
         systems: [String], importedAt: Date = Date(), sourceFileName: String = "",
         format: String = "delta", hasSound: Bool = false) {
        self.id = id
        self.name = name
        self.identifier = identifier
        self.gameTypeIdentifier = gameTypeIdentifier
        self.systems = systems
        self.importedAt = importedAt
        self.sourceFileName = sourceFileName
        self.format = format
        self.hasSound = hasSound
    }

    private enum CodingKeys: String, CodingKey {
        case id, name, identifier, gameTypeIdentifier, systems, importedAt, sourceFileName
        case format, hasSound
    }

    /// Forgiving per field, like every stored layout in this app.
    init(from decoder: Decoder) throws {
        let box = try decoder.container(keyedBy: CodingKeys.self)
        id = try box.decode(String.self, forKey: .id)
        name = (try? box.decodeIfPresent(String.self, forKey: .name)) ?? "Skin"
        identifier = (try? box.decodeIfPresent(String.self, forKey: .identifier)) ?? ""
        gameTypeIdentifier = (try? box.decodeIfPresent(String.self, forKey: .gameTypeIdentifier))
            ?? ""
        systems = (try? box.decodeIfPresent([String].self, forKey: .systems)) ?? []
        importedAt = (try? box.decodeIfPresent(Date.self, forKey: .importedAt))
            ?? Date(timeIntervalSince1970: 0)
        sourceFileName = (try? box.decodeIfPresent(String.self, forKey: .sourceFileName))
            ?? ""
        format = (try? box.decodeIfPresent(String.self, forKey: .format)) ?? "delta"
        hasSound = (try? box.decodeIfPresent(Bool.self, forKey: .hasSound)) ?? false
    }
}

/// What a game or a system was told to use.
enum SkinChoice: Equatable, Sendable {
    /// Follow the next rule down (the system default, then any skin that fits).
    case automatic
    /// The built-in pad, no skin.
    case none
    case skin(String)
}

/// The whole library index: records, the default per system and the choice per game.
struct SkinLibraryIndex: Codable, Equatable, Sendable {
    /// Stored as the value of a default or a game choice to mean "no skin, the built-in pad".
    static let noSkin = "none"

    var records: [String: SkinRecord] = [:]
    /// System id -> skin id or `noSkin`. Absent means automatic.
    var defaults: [String: String] = [:]
    /// Game key (the file name) -> skin id or `noSkin`. Absent means the system's choice.
    var perGame: [String: String] = [:]

    init() {}

    private enum CodingKeys: String, CodingKey { case records, defaults, perGame }

    init(from decoder: Decoder) throws {
        let box = try decoder.container(keyedBy: CodingKeys.self)
        // One record at a time. A dictionary decode fails the WHOLE map when a single value
        // fails, and this map used to then come back empty and get saved, which wiped every skin.
        records = Self.lossy(SkinRecord.self, from: box, key: .records)
        defaults = (try? box.decodeIfPresent([String: String].self, forKey: .defaults)) ?? [:]
        perGame = (try? box.decodeIfPresent([String: String].self, forKey: .perGame)) ?? [:]
    }

    /// Decodes a string-keyed map and keeps the values that decode, instead of dropping all of
    /// them because one was unreadable.
    private static func lossy<T: Decodable>(
        _ type: T.Type, from box: KeyedDecodingContainer<CodingKeys>, key: CodingKeys
    ) -> [String: T] {
        guard let wrapped = try? box.decode([String: LossyDecodable<T>].self, forKey: key) else {
            return [:]
        }
        return wrapped.compactMapValues(\.value)
    }

    func encode(to encoder: Encoder) throws {
        var box = encoder.container(keyedBy: CodingKeys.self)
        try box.encode(records, forKey: .records)
        try box.encode(defaults, forKey: .defaults)
        try box.encode(perGame, forKey: .perGame)
    }

    // MARK: Reading

    /// Every skin that may be drawn on `system`: its own first, then its relatives', newest first
    /// within each.
    func skins(for system: String) -> [SkinRecord] {
        let all = records.values.filter { SkinSharing.fits(skinSystems: $0.systems, on: system) }
        return all.sorted { lhs, rhs in
            let lOwn = lhs.systems.contains(system)
            let rOwn = rhs.systems.contains(system)
            if lOwn != rOwn { return lOwn }
            if lhs.importedAt != rhs.importedAt { return lhs.importedAt > rhs.importedAt }
            return lhs.id < rhs.id
        }
    }

    func fits(_ id: String, on system: String) -> Bool {
        guard let record = records[id] else { return false }
        return SkinSharing.fits(skinSystems: record.systems, on: system)
    }

    func defaultChoice(for system: String) -> SkinChoice {
        Self.choice(from: defaults[system])
    }

    func gameChoice(for gameKey: String) -> SkinChoice {
        Self.choice(from: perGame[gameKey])
    }

    private static func choice(from raw: String?) -> SkinChoice {
        guard let raw, !raw.isEmpty else { return .automatic }
        return raw == noSkin ? .none : .skin(raw)
    }

    /// The skin id to draw on `system`, or nil for the built-in pad.
    ///
    /// In order: the game's own choice, the system's default, a related system's default, the
    /// newest skin made for this system, the newest skin made for a relative. A choice naming a
    /// skin that was deleted, or one that does not fit, is skipped rather than obeyed.
    func resolve(system: String, gameKey: String?) -> String? {
        if let gameKey {
            switch gameChoice(for: gameKey) {
            case .none: return nil
            case .skin(let id) where fits(id, on: system): return id
            default: break
            }
        }
        switch defaultChoice(for: system) {
        case .none: return nil
        case .skin(let id) where fits(id, on: system): return id
        default: break
        }
        for relative in SkinSharing.related(to: system) {
            if case .skin(let id) = defaultChoice(for: relative), fits(id, on: system) {
                return id
            }
        }
        return skins(for: system).first?.id
    }

    // MARK: Writing

    /// Adds or replaces a record. A brand new skin becomes its systems' default, which is what
    /// importing one in the layout editor has always meant.
    mutating func add(_ record: SkinRecord, makeDefaultFor systems: [String]) {
        records[record.id] = record
        for system in systems {
            defaults[system] = record.id
        }
    }

    mutating func rename(_ id: String, to name: String) -> Bool {
        let clean = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !clean.isEmpty, var record = records[id] else { return false }
        record.name = String(clean.prefix(80))
        records[id] = record
        return true
    }

    /// Removes a record and every default and game choice that named it.
    @discardableResult
    mutating func delete(_ id: String) -> SkinRecord? {
        guard let removed = records.removeValue(forKey: id) else { return nil }
        defaults = defaults.filter { $0.value != id }
        perGame = perGame.filter { $0.value != id }
        return removed
    }

    mutating func setDefault(_ choice: SkinChoice, for system: String) {
        switch choice {
        case .automatic: defaults.removeValue(forKey: system)
        case .none: defaults[system] = Self.noSkin
        case .skin(let id): defaults[system] = id
        }
    }

    mutating func setGame(_ choice: SkinChoice, for gameKey: String) {
        switch choice {
        case .automatic: perGame.removeValue(forKey: gameKey)
        case .none: perGame[gameKey] = Self.noSkin
        case .skin(let id): perGame[gameKey] = id
        }
    }

    /// Makes a record for a skin saved before the library existed. Those were stored one per
    /// system, and the storage key IS the system id, so the files on disk (named after the system)
    /// are found without moving anything.
    ///
    /// `knownSystems`, when given, is the list of real system ids. A key that is not one of them is
    /// left alone: a skin the index does not know is not a console, and treating its random id as
    /// a system name is how "SKIN-1A2B" rows used to appear.
    mutating func adoptLegacy(_ stored: [String: String], knownSystems: Set<String>? = nil) {
        for (key, name) in stored where records[key] == nil {
            if let knownSystems, !knownSystems.contains(key) { continue }
            records[key] = SkinRecord(id: key, name: name, systems: [key],
                                      importedAt: Date(timeIntervalSince1970: 0),
                                      format: "legacy")
            if defaults[key] == nil { defaults[key] = key }
        }
    }

    /// Drops records whose files are gone, and choices naming them.
    mutating func keepOnly(_ ids: Set<String>) {
        for id in Array(records.keys) where !ids.contains(id) {
            delete(id)
        }
    }

    static func newID() -> String {
        "skin-" + UUID().uuidString.lowercased()
    }

    /// The id every phone uses for a skin that names itself.
    ///
    /// A random id made at import is different on every phone, so the same skin synced from two
    /// phones showed up twice. The name inside the file plus the consoles it was made for are the
    /// same everywhere, so the id built from them is too. A skin that does not name itself still
    /// gets `newID()`: there is nothing stable to share.
    static func stableID(identifier: String, systems: [String]) -> String? {
        let ident = identifier.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !ident.isEmpty else { return nil }
        let systemsKey = Set(systems.map { $0.lowercased() }).sorted().joined(separator: ",")
        return "skin-" + fnv64(ident + "|" + systemsKey)
    }

    /// Folds skins that name the same file into the one shared id.
    ///
    /// `renames` is every old id that moved, so art and edits can follow. `winners` is the shared
    /// id to the id the kept record came from: that copy is the newest, and its files are the ones
    /// that should end up on the shared id. An older copy must not be the one that lands there
    /// just because it was already using the shared id.
    mutating func coalesceNamedSkins() -> SkinFold {
        var byStable: [String: [String]] = [:]
        for (id, record) in records {
            guard let stable = Self.stableID(identifier: record.identifier, systems: record.systems)
            else { continue }
            byStable[stable, default: []].append(id)
        }
        var fold = SkinFold()
        for (stable, ids) in byStable {
            let unique = Array(Set(ids))
            let needsMove = unique.contains { $0 != stable }
            guard needsMove || records[stable] == nil else { continue }
            let chosenID = unique.sorted { a, b in
                let left = records[a]!
                let right = records[b]!
                if left.importedAt != right.importedAt { return left.importedAt > right.importedAt }
                if (a == stable) != (b == stable) { return a == stable }
                return a < b
            }.first!
            var kept = records[chosenID]!
            kept.id = stable
            fold.winners[stable] = chosenID
            for id in unique where id != stable {
                records.removeValue(forKey: id)
                fold.renames[id] = stable
            }
            records[stable] = kept
            let movedSystems = defaults.filter { unique.contains($0.value) }.map(\.key)
            for system in movedSystems { defaults[system] = stable }
            let movedGames = perGame.filter { unique.contains($0.value) }.map(\.key)
            for game in movedGames { perGame[game] = stable }
        }
        return fold
    }

    /// Drops legacy rows whose id is not a real console.
    ///
    /// Those rows are skins the index did not know, adopted as if the skin's random id were a
    /// system. A real legacy skin is stored under the system id itself (`gba`, `ps1`) and is left
    /// alone. Returns the ids removed, so the caller can drop their art metadata too.
    mutating func dropFakeConsoles(knownSystems: Set<String>) -> [String] {
        var dropped: [String] = []
        for id in Array(records.keys) {
            guard let record = records[id], record.format == "legacy",
                  !knownSystems.contains(id) else { continue }
            delete(id)
            dropped.append(id)
        }
        return dropped
    }

    /// The library in `data`, or nil when it cannot be trusted.
    ///
    /// Nil means "do not replace what is stored and do not save". A file that is not a library,
    /// or one whose every skin failed to read, used to decode as an empty library and that empty
    /// library was then saved. One unreadable skin is dropped and the rest are kept.
    static func stored(_ data: Data) -> SkinLibraryIndex? {
        guard let index = try? JSONDecoder().decode(SkinLibraryIndex.self, from: data) else {
            return nil
        }
        guard let raw = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let map = raw["records"] as? [String: Any] else {
            return index
        }
        if !map.isEmpty, index.records.isEmpty { return nil }
        return index
    }

    /// 64-bit FNV-1a, hex. Stable across phones and needs nothing but the standard library, so the
    /// Linux check of this file can run it.
    private static func fnv64(_ text: String) -> String {
        var hash: UInt64 = 14695981039346656037
        for byte in text.utf8 {
            hash ^= UInt64(byte)
            hash &*= 1099511628211
        }
        return String(hash, radix: 16)
    }
}

/// The result of folding named skins onto one shared id.
struct SkinFold: Equatable, Sendable {
    /// Old id to the shared id, for every record that moved.
    var renames: [String: String] = [:]
    /// Shared id to the id the kept record came from. That copy's files are the ones to keep.
    var winners: [String: String] = [:]
}

/// One dictionary value. A value that will not decode becomes nil and the rest of the dictionary
/// is kept, which a plain `[String: T]` decode does not do.
struct LossyDecodable<T: Decodable>: Decodable {
    let value: T?

    init(from decoder: Decoder) throws {
        let box = try decoder.singleValueContainer()
        value = try? box.decode(T.self)
    }
}
