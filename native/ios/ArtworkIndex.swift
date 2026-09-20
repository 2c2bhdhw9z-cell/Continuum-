// Continuum - the server's own cover list, and the honest fuzzy match that closes the last gap.
//
// WHY THIS FILE EXISTS. A device run resolved real box art for three games out of six. The three
// that worked were named the way libretro names things, which is No-Intro: "Crash Bandicoot (USA)",
// "Pokemon - Emerald Version (USA, Europe)". Two of the three that failed were named the way
// GoodTools names things, "(U)" and "[!]", and ArtworkNames now REWRITES those rather than only
// deleting them, which fixes them with one extra request and no download. The third one cannot be
// fixed that way at all, and it is the reason this file exists:
//
//     Kart Fighter.nes            ->  404 for every name this build can derive
//     the server actually has     ->  "Kart Fighter (199x)(-)(AS)[p].png"
//
// That is a third convention (a TOSEC-style date, publisher and country, plus a pirate flag). No
// amount of tag stripping or region substitution will ever produce it from "Kart Fighter.nes",
// because the extra text is not in the filename to begin with. The only thing that can find it is
// the server's own list of what it has. So, once per system and only when everything cheaper has
// already failed, this file fetches the box art directory index, reduces it to titles, and matches
// on the title alone.
//
// THREE PROPERTIES THIS FILE IS BUILT AROUND.
//
//  1. IT IS HONEST. The match is an EQUALITY of normalised titles, never a substring and never an
//     edit distance. "Simpsons, The - Krusty's Fun House" normalises to
//     "simpsons the krustys fun house", and the three Simpsons covers the Game Gear folder really
//     has normalise to "simpsons the bartman meets radioactive man",
//     "simpsons the bart vs the space mutants" and "simpsons the bart vs the world". None of them
//     is equal, so nothing matches, and the game keeps its plate and is recorded as a genuine
//     miss. A substring rule or a distance threshold would have put Bart on the shelf under
//     Krusty's name, and a confidently wrong cover is worse than a plate.
//
//  2. IT IS PAID FOR ONCE. The NES box art index is 4,054,023 bytes of HTML carrying 13,418 png
//     names. That HTML is never stored: the names are extracted, reduced to one entry per title,
//     and only that map is written to disk, which is a few hundred kilobytes. One fetch per system,
//     never twice at the same time, and good for thirty days.
//
//  3. IT IS DETERMINISTIC. When several files share a normalised title, the least decorated one
//     wins: fewest bracketed groups, then shortest, then alphabetically. For Kart Fighter that
//     picks "(199x)(-)(AS)[p]" over "(199x)(-)(AS)[p][b2]" and over the "[tr en ...]" translations,
//     which is right, because a bad dump or a fan translation is not the cover anyone means.

import Foundation

// MARK: - The pure half

/// Normalising, parsing and matching. Nothing in here touches the network or the disk, so every
/// rule can be checked by reading it and by running it against a saved copy of a real listing.
enum ArtworkIndexNames {
    /// The comparison key for a title, with every convention's decoration removed.
    ///
    /// Both sides of a match go through this, which is the whole trick: the game's filename and the
    /// server's filename are written by different tools with different rules, and this reduces both
    /// to the part they agree on.
    ///
    ///     "Kart Fighter.nes"                        -> "kart fighter"
    ///     "Kart Fighter (199x)(-)(AS)[p].png"       -> "kart fighter"
    ///     "Simpsons, The - Krusty's Fun House (U)"  -> "simpsons the krustys fun house"
    ///
    /// The apostrophe is DELETED while other punctuation becomes a space, and that asymmetry is
    /// deliberate: "Krusty's" and "Krustys" are the same title spelled by two dumpers, so they have
    /// to land on the same key, while "Bart vs. the World" and "Bart vs the World" only agree if
    /// the full stop becomes a space. Diacritics are folded so "Pokemon" and "Pokémon" agree too.
    static func normalisedTitle(_ value: String) -> String {
        var name = value
        // A listing entry carries the extension, a filename on disk has already had its own
        // dropped by `ArtworkNames.baseName`. Accept either.
        if name.count > 4, name.suffix(4).lowercased() == ".png" {
            name = String(name.dropLast(4))
        }
        // Every (..) and [..] group goes, which is what makes a No-Intro region, a GoodTools dump
        // flag and a TOSEC date all disappear at once. Shared with the ladder rather than restated.
        let untagged = ArtworkNames.stripTags(name)
        let folded = untagged.folding(options: [.diacriticInsensitive, .caseInsensitive],
                                     locale: nil)

        var out = ""
        out.reserveCapacity(folded.count)
        for character in folded {
            if character == "'" || character == "\u{2019}" {
                // Deleted, not spaced: "krusty's" has to equal "krustys".
                continue
            }
            if character.isLetter || character.isNumber {
                out.append(character)
            } else {
                out.append(" ")
            }
        }
        return ArtworkNames.collapse(out).lowercased()
    }

    /// The png filenames in a browsable Apache directory index.
    ///
    /// Scanned over the RAW BYTES rather than over a String, because the NES listing is four
    /// megabytes and building a String of it first, then splitting that String, is several copies
    /// of four megabytes on a phone for no reason. Every href in this listing is ASCII, because the
    /// server percent-encodes them, so byte scanning is safe as well as cheap.
    ///
    /// Three kinds of href in the listing are NOT files and are dropped: the column sort links
    /// ("?C=N;O=D"), the parent directory (an absolute "/..." path), and anything that is not a
    /// .png. The href is HTML-unescaped before it is percent-decoded, in that order, because the
    /// server writes a literal ampersand in a filename as "&amp;" and percent-decoding first would
    /// leave the entity behind.
    static func filenames(inListing data: Data) -> [String] {
        let needle = Array("href=\"".utf8)
        let quote = UInt8(ascii: "\"")
        let bytes = [UInt8](data)
        let count = bytes.count

        var out: [String] = []
        var index = 0
        while index + needle.count <= count {
            var matched = 0
            while matched < needle.count,
                  lowercasedASCII(bytes[index + matched]) == needle[matched] {
                matched += 1
            }
            guard matched == needle.count else {
                index += 1
                continue
            }
            var end = index + needle.count
            while end < count, bytes[end] != quote {
                end += 1
            }
            guard end < count else { break }
            let raw = String(decoding: bytes[(index + needle.count)..<end], as: UTF8.self)
            index = end + 1

            guard let filename = listedFilename(fromHref: raw) else { continue }
            out.append(filename)
        }
        return out
    }

    /// The String door onto the scanner above, for a test or a saved fixture.
    static func filenames(inListingText text: String) -> [String] {
        filenames(inListing: Data(text.utf8))
    }

    /// One href to the filename it names, or nil when it is not a file in this directory.
    static func listedFilename(fromHref href: String) -> String? {
        guard !href.isEmpty else { return nil }
        // A sort link, the parent directory, or a link off to another host.
        guard !href.hasPrefix("?"), !href.hasPrefix("/"), !href.contains("://") else { return nil }

        let unescaped = htmlUnescaped(href)
        let decoded = unescaped.removingPercentEncoding ?? unescaped
        guard decoded.count > 4, decoded.suffix(4).lowercased() == ".png" else { return nil }
        // A decoded name with a path separator in it is not a file in this directory, and a
        // filename cannot contain one anyway.
        guard !decoded.contains("/") else { return nil }
        return decoded
    }

    /// The five entities Apache writes into an href. Not a general HTML decoder, and deliberately
    /// not: anything else in this position would be a malformed listing, and silently "repairing"
    /// one would turn a server change into a wrong cover rather than into a miss.
    static func htmlUnescaped(_ text: String) -> String {
        guard text.contains("&") else { return text }
        var out = text
        // Ampersand LAST, so "&amp;lt;" cannot be turned into "<".
        for (entity, replacement) in [("&lt;", "<"), ("&gt;", ">"), ("&quot;", "\""),
                                      ("&#39;", "'"), ("&amp;", "&")] {
            out = out.replacingOccurrences(of: entity, with: replacement)
        }
        return out
    }

    /// How many (..) and [..] groups a name carries. The first term of the tie-break.
    static func decorationCount(in name: String) -> Int {
        var groups = 0
        var depth = 0
        for character in name {
            if character == "(" || character == "[" {
                depth += 1
            } else if character == ")" || character == "]" {
                if depth > 0 {
                    groups += 1
                    depth -= 1
                }
            }
        }
        return groups
    }

    /// The tie-break, as a strict ordering: fewest groups, then shortest, then alphabetical.
    ///
    /// Total and deterministic, which matters because the map below is built by folding over a
    /// listing whose order this build does not control. Two different launches must pick the same
    /// cover, or a library would shuffle its own art.
    static func isLessDecorated(_ lhs: String, _ rhs: String) -> Bool {
        let lhsGroups = decorationCount(in: lhs)
        let rhsGroups = decorationCount(in: rhs)
        if lhsGroups != rhsGroups { return lhsGroups < rhsGroups }
        if lhs.count != rhs.count { return lhs.count < rhs.count }
        return lhs < rhs
    }

    /// The whole listing reduced to one filename per title.
    ///
    /// THIS IS WHAT IS KEPT, AND THE FOUR MEGABYTES OF HTML ARE NOT. A listing of 13,418 names
    /// collapses to roughly ten thousand titles, which is a few hundred kilobytes of strings rather
    /// than four megabytes of markup, and it is the only shape any later search needs.
    static func titleMap(from filenames: [String]) -> [String: String] {
        var map: [String: String] = [:]
        map.reserveCapacity(filenames.count)
        for filename in filenames {
            let title = normalisedTitle(filename)
            guard !title.isEmpty else { continue }
            if let existing = map[title] {
                if isLessDecorated(filename, existing) {
                    map[title] = filename
                }
            } else {
                map[title] = filename
            }
        }
        return map
    }

    /// The filename for a title, or nil when the server has nothing under that title.
    ///
    /// An EQUALITY and nothing looser. See the file header: this one line is the safeguard that
    /// keeps a fuzzy match from putting the wrong cover on a card. An empty title never matches,
    /// because a filename that was nothing but tags would otherwise match every other one.
    static func match(title: String, in map: [String: String]) -> String? {
        guard !title.isEmpty else { return nil }
        return map[title]
    }

    /// The title to search for, from the filename on disk.
    static func searchTitle(forFilename filename: String) -> String {
        normalisedTitle(ArtworkNames.baseName(filename))
    }

    private static func lowercasedASCII(_ byte: UInt8) -> UInt8 {
        (byte >= 65 && byte <= 90) ? byte + 32 : byte
    }
}

// MARK: - What is kept on disk

/// One system's cover list, as it is persisted.
///
/// Codable because this is the one thing in the artwork system that is structured data rather than
/// image bytes, and a hand-rolled format for it would be a parser to get wrong. The counts are
/// carried so Settings can say what the download actually cost instead of estimating it.
struct ArtworkCoverList: Codable, Sendable {
    /// The playlist directory the list was read from, so a list is discarded rather than trusted if
    /// this build's directory table ever changes for that system.
    let directory: String
    /// The thumbnail folder, which is always Named_Boxarts today. Stored rather than assumed so a
    /// future list of title screens is a new value and not a silent reinterpretation of this one.
    let folder: String
    let fetchedAt: Double
    /// How many png names the listing carried, before the fold onto titles.
    let listedFiles: Int
    /// How many bytes of HTML that listing cost. The honest figure for the disclosure.
    let listingBytes: Int
    /// Normalised title to the least decorated filename carrying it.
    let titles: [String: String]

    var age: TimeInterval {
        Date().timeIntervalSince1970 - fetchedAt
    }
}

/// What asking for a system's cover list produced.
enum ArtworkCoverListOutcome: Sendable {
    /// A list that can be searched. `downloadedBytes` is nil when it was read from disk, which is
    /// the difference between "this cost nothing" and "this cost four megabytes" and is therefore
    /// worth saying out loud.
    case ready(ArtworkCoverList, downloadedBytes: Int?, writeFailure: String?)
    /// The list could not be had. NOT evidence about any game's artwork, so nothing is recorded as
    /// a miss on the strength of it.
    case failure(String)
}

/// The cover lists on disk, and the one fetch that fills them.
///
/// Non-isolated for the same reason `ArtworkDisk` is: every function here does I/O or parses four
/// megabytes of HTML, and an async non-isolated function runs off the main actor, which is what
/// keeps that parse out of the frame the library is drawing.
enum ArtworkCoverLists {
    /// Thirty days. The repository gains thumbnails over time, so a list is not forever; it changes
    /// slowly enough that re-downloading four megabytes more often than monthly would be rude.
    static let lifetime: TimeInterval = 30 * 24 * 60 * 60

    /// A SUBDIRECTORY of the artwork directory, which puts it outside Documents by construction:
    /// this is not user content and must never appear in the Files app beside a user's ROMs.
    ///
    /// Its own folder rather than loose files next to the covers, so "clear the artwork cache" and
    /// "forget the cover lists" stay two separate actions with two separate sizes. `ArtworkDisk`
    /// skips directories for exactly that reason.
    static func directory() -> URL? {
        guard let artwork = ArtworkDisk.directory() else { return nil }
        let directory = artwork.appendingPathComponent("CoverLists", isDirectory: true)
        if !FileManager.default.fileExists(atPath: directory.path) {
            do {
                try FileManager.default.createDirectory(at: directory,
                                                        withIntermediateDirectories: true)
            } catch {
                return nil
            }
        }
        return directory
    }

    private static func fileURL(for system: GameSystem) -> URL? {
        // The system's own short name, so the file is legible to anyone who ever looks: "nes.json".
        directory()?.appendingPathComponent("\(system.rawValue).json")
    }

    /// The stored list for a system, or nil when there is none, it is too old, or it was written
    /// for a different playlist directory.
    static func stored(for system: GameSystem) async -> ArtworkCoverList? {
        guard let url = fileURL(for: system) else { return nil }
        guard let data = try? Data(contentsOf: url, options: .mappedIfSafe) else { return nil }
        guard let list = try? JSONDecoder().decode(ArtworkCoverList.self, from: data) else {
            return nil
        }
        guard list.directory == SystemArtwork.playlistDirectory(for: system) else { return nil }
        guard list.age < lifetime else { return nil }
        return list
    }

    /// One session, for listings only.
    ///
    /// SEPARATE FROM `ArtworkFetcher.session` ON PURPOSE. That one is tuned for a 300 KB PNG: a 40
    /// second resource timeout and an Accept header asking for an image. A four megabyte listing on
    /// a phone can legitimately take longer than that, and a timeout halfway through would look
    /// exactly like the server refusing.
    static let session: URLSession = {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 30
        configuration.timeoutIntervalForResource = 180
        configuration.waitsForConnectivity = false
        configuration.httpMaximumConnectionsPerHost = 2
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        return URLSession(configuration: configuration)
    }()

    /// The list for a system: from disk when it is there and fresh, otherwise downloaded once.
    ///
    /// The caller is responsible for making sure this is not called twice at the same moment for
    /// one system. `ArtworkStore` does that by holding a task per system, the same way it holds one
    /// per game, because ten cards asking at once must not become ten four megabyte downloads.
    static func list(for system: GameSystem) async -> ArtworkCoverListOutcome {
        if let stored = await stored(for: system) {
            return .ready(stored, downloadedBytes: nil, writeFailure: nil)
        }
        return await download(for: system)
    }

    /// Fetches, parses and stores one system's listing.
    static func download(for system: GameSystem) async -> ArtworkCoverListOutcome {
        guard let directory = SystemArtwork.playlistDirectory(for: system) else {
            return .failure("no thumbnail directory is known for \(system.displayName), so "
                            + "there is no cover list to search")
        }
        guard let url = ArtworkNames.listingURL(system: system) else {
            return .failure("the cover list address for \(system.displayName) could not be built")
        }

        var request = URLRequest(url: url)
        request.httpMethod = "GET"
        request.setValue("text/html,*/*;q=0.8", forHTTPHeaderField: "Accept")

        let data: Data
        let response: URLResponse
        do {
            (data, response) = try await session.data(for: request)
        } catch let error as URLError {
            return .failure("the cover list for \(system.displayName) could not be downloaded: "
                            + networkText(for: error))
        } catch {
            return .failure("the cover list for \(system.displayName) could not be downloaded: "
                            + error.localizedDescription)
        }

        guard let http = response as? HTTPURLResponse else {
            return .failure("the cover list for \(system.displayName) came back with no HTTP "
                            + "response")
        }
        guard http.statusCode == 200 else {
            return .failure("the cover list for \(system.displayName) answered HTTP "
                            + "\(http.statusCode)")
        }
        let contentType = (http.value(forHTTPHeaderField: "Content-Type") ?? "").lowercased()
        // A listing is HTML. Anything else means this is not the browsable index it looks like, and
        // parsing it anyway would be inventing names.
        guard contentType.contains("text/html") else {
            return .failure("the cover list for \(system.displayName) came back as "
                            + "\(contentType.isEmpty ? "no content type" : contentType)"
                            + ", not a directory listing")
        }

        let filenames = ArtworkIndexNames.filenames(inListing: data)
        guard !filenames.isEmpty else {
            return .failure("the cover list for \(system.displayName) downloaded "
                            + "\(data.count) byte(s) and named no cover at all")
        }

        let list = ArtworkCoverList(
            directory: directory,
            folder: ThumbnailFolder.boxart.rawValue,
            fetchedAt: Date().timeIntervalSince1970,
            listedFiles: filenames.count,
            listingBytes: data.count,
            titles: ArtworkIndexNames.titleMap(from: filenames)
        )
        let writeFailure = write(list, for: system)
        return .ready(list, downloadedBytes: data.count, writeFailure: writeFailure)
    }

    /// Persists a list. Returns a reason on failure rather than throwing, so the caller has a
    /// sentence to show: a list that is searched now and gone after a relaunch means the next
    /// launch downloads four megabytes again, which must never be silent.
    private static func write(_ list: ArtworkCoverList, for system: GameSystem) -> String? {
        guard let url = fileURL(for: system) else {
            return "there is no artwork directory, so it was not kept"
        }
        do {
            let data = try JSONEncoder().encode(list)
            try data.write(to: url, options: .atomic)
            return nil
        } catch {
            return "it could not be written: \(error.localizedDescription)"
        }
    }

    /// Throws every stored list away. Returns what went, so the Settings action can report it.
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

    /// What the lists currently cost, for the Settings read-out.
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

    /// A URLError as a sentence, in the same vocabulary `ArtworkFetcher` uses.
    private static func networkText(for error: URLError) -> String {
        switch error.code {
        case .notConnectedToInternet:
            return "there is no network"
        case .timedOut:
            return "the server timed out, and a cover list is several megabytes"
        case .cannotFindHost, .dnsLookupFailed:
            return "thumbnails.libretro.com could not be found"
        case .networkConnectionLost:
            return "the connection dropped part way through"
        case .cancelled:
            return "the download was cancelled"
        case .dataNotAllowed:
            return "cellular data is off for this app"
        case .secureConnectionFailed, .serverCertificateUntrusted:
            return "the secure connection failed"
        default:
            return error.localizedDescription
        }
    }
}
