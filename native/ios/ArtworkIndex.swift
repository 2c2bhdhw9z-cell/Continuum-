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
// the server's own list of what it has. So, once per system and folder and only when everything
// cheaper has already failed, this file fetches a directory index, reduces it to titles, and matches
// on the title alone.
//
// WHAT THE SECOND COVERAGE PASS CHANGED IN HERE, AND WHY IT HAD TO. One game still had no art after
// the first pass, and it had art on the server the whole time:
//
//     on disk                 "Simpsons, The - Krusty's Fun House (U) [!].gg"
//     the server has          Sega - Game Gear/Named_Boxarts/Krusty's Fun House (USA, Europe).png
//                             Sega - Game Gear/Named_Titles/Krusty's Fun House (USA, Europe).png
//                             Sega - Game Gear/Named_Snaps/Krusty's Fun House (USA, Europe).png
//
// Two independent bugs kept it from being found, and both are fixed here. FIRST, the list search
// only ever downloaded Named_Boxarts: a system had exactly one list, keyed on the system alone, so
// the search that exists precisely for names the ladder cannot derive was blind to two of the three
// folders it is supposed to cover. Lists are now per (system, folder), searched in the existing
// preference order and downloaded only when the cheaper folders found nothing. SECOND, the match was
// an exact equality of normalised titles, and these two titles genuinely differ: the file normalises
// to "the simpsons krustys fun house" and the listing to "krustys fun house". That is a SERIES
// PREFIX, not a tag, and no amount of tag stripping closes it. So equality is now followed by a
// token alignment that accepts a contiguous prefix or suffix, with four hard guards. See `aligns`.
//
// THREE PROPERTIES THIS FILE IS BUILT AROUND.
//
//  1. IT IS HONEST. Equality first, and after it a WHOLE-WORD prefix or suffix alignment with hard
//     guards: never a mid-string substring, never an edit distance, never a similarity score. The
//     three other Simpsons covers the Game Gear folder really has normalise to
//     "the simpsons bartman meets radioactive man", "the simpsons bart vs the space mutants" and
//     "the simpsons bart vs the world", and not one of them aligns with Krusty's title, because
//     none of them is a contiguous prefix or suffix of it. And when two listing titles DO align
//     with one game, nothing is auto-picked at all: the chooser in the detail sheet offers both,
//     so ambiguity becomes a user's choice rather than a coin flip.
//
//  2. IT IS PAID FOR ONCE. The NES box art index is 4,054,023 bytes of HTML carrying 13,418 png
//     names, and its title and snapshot indexes are 4,859,773 and 4,861,401 bytes. That HTML is
//     never stored: the names are extracted, reduced to one entry per title, and only that map is
//     written to disk, which is a few hundred kilobytes. One fetch per system AND FOLDER, never
//     twice at the same time, and good for thirty days.
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
    ///     "Simpsons, The - Krusty's Fun House (U)"  -> "the simpsons krustys fun house"
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
        // The inversion is repaired BEFORE the comma becomes a space, because the comma is the only
        // evidence that "The" was moved to the back. See `uninvertArticle`.
        let uninverted = uninvertArticle(untagged)
        let folded = uninverted.folding(options: [.diacriticInsensitive, .caseInsensitive],
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

    // ------------------------------------------------------------------ the two spellings of "The"

    /// The articles that get moved to the back of a title, lowercased for comparison.
    static let invertedArticles: Set<String> = ["the", "a", "an"]

    /// Repairs the article inversion: "Simpsons, The - Bart vs. the World" becomes
    /// "The Simpsons - Bart vs. the World".
    ///
    /// WHY THIS RUNS BEFORE EVERYTHING ELSE. No-Intro writes a leading article at the BACK, after a
    /// comma; GoodTools and most other tools write it at the FRONT. Those are the same title, and
    /// until the comma is repaired they are two different keys, because `normalisedTitle` turns the
    /// comma into a space and the word order then disagrees. Both sides of every comparison go
    /// through `normalisedTitle`, so repairing it here makes the two conventions agree for the exact
    /// match AND puts the words in the order the alignment below needs.
    ///
    /// DELIBERATELY NARROW. Only a comma whose next word is exactly "the", "a" or "an" AND which is
    /// followed by the end of the name or by a " - " segment break is treated as an inversion. A
    /// comma anywhere else, "(USA, Europe)" for instance, is left exactly as it was; that one is
    /// already gone by the time this runs, because the tags are stripped first, and a title that
    /// really does contain a list stays a list.
    static func uninvertArticle(_ value: String) -> String {
        let characters = Array(value)
        var index = 0
        while index < characters.count {
            guard characters[index] == "," else {
                index += 1
                continue
            }
            var wordStart = index + 1
            while wordStart < characters.count, characters[wordStart].isWhitespace {
                wordStart += 1
            }
            var wordEnd = wordStart
            while wordEnd < characters.count, characters[wordEnd].isLetter {
                wordEnd += 1
            }
            let word = String(characters[wordStart..<wordEnd])
            guard invertedArticles.contains(word.lowercased()) else {
                index += 1
                continue
            }
            let rest = String(characters[wordEnd...])
            guard isSegmentEnd(rest) else {
                index += 1
                continue
            }
            let head = String(characters[0..<index])
            return ArtworkNames.collapse("\(word) \(head)\(rest)")
        }
        return value
    }

    /// True when what follows an article is the end of the title or a " - " segment break.
    ///
    /// Those are the only two positions a moved article can sit in: "Simpsons, The" and
    /// "Simpsons, The - Bart vs. the World". Anything else is a comma in the middle of a sentence.
    private static func isSegmentEnd(_ rest: String) -> Bool {
        var seenWhitespace = false
        for character in rest {
            if character.isWhitespace {
                seenWhitespace = true
                continue
            }
            // A hyphen straight after whitespace is the segment break No-Intro uses.
            return seenWhitespace && character == "-"
        }
        // Nothing but whitespace left, so the article was at the end of the title.
        return true
    }

    // ------------------------------------------------------------------ the token alignment

    /// A normalised title as its words. Splitting on a single space is safe because
    /// `normalisedTitle` has already collapsed every run of whitespace.
    static func titleTokens(_ title: String) -> [String] {
        title.split(separator: " ").map(String.init)
    }

    /// The bare sequence markers a sequel is told apart by. Roman numerals to ten, which is as far
    /// as any game in this library's era numbered its sequels.
    static let sequenceMarkers: Set<String> = [
        "i", "ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x",
    ]

    /// True when a word is nothing but a sequence number: "2", "1993" or "iii".
    static func isSequenceMarker(_ word: String) -> Bool {
        guard !word.isEmpty else { return false }
        if word.allSatisfy({ $0.isNumber }) { return true }
        return sequenceMarkers.contains(word)
    }

    /// Whether two normalised titles line up as a series prefix or suffix of one another.
    ///
    /// THE ONE FUZZY RULE IN THIS BUILD, AND IT IS STILL NOT A SIMILARITY SCORE. One side's words
    /// must be a CONTIGUOUS PREFIX or a CONTIGUOUS SUFFIX of the other's, whole words only. That is
    /// what finds the cover for "Simpsons, The - Krusty's Fun House", which the server files as
    /// "Krusty's Fun House (USA, Europe).png": after the inversion repair the file reads
    /// "the simpsons krustys fun house" and the listing reads "krustys fun house", which is a
    /// suffix. It is never a mid-string substring, never an edit distance and never a threshold,
    /// because a confidently wrong cover is worse than a plate.
    ///
    /// FOUR GUARDS, EACH ONE PAYING FOR A REAL FALSE POSITIVE MEASURED AGAINST THE SAVED LISTINGS.
    ///
    ///   1. The shorter side must be at least TWO words. Without it "Batman" would claim every
    ///      "Batman <anything>" cover in the folder.
    ///   2. The shorter side must be at least EIGHT characters once joined. Without it "sonic 2"
    ///      and "the game" would align with half a library.
    ///   3. The residue, the longer side's remaining words, must not be made up ENTIRELY of
    ///      sequence markers. This is what stops "sonic drift 2" taking "Sonic Drift"'s cover.
    ///   4. The residue must not BEGIN or END with a sequence marker. This is what stops
    ///      "chuck rock ii son of chuck" taking "Chuck Rock"'s cover, which guard 3 misses because
    ///      the residue carries real words after the numeral.
    ///
    /// The cases that must still align, all of them real entries in the saved Game Gear listing:
    /// "desert strike return to the gulf" with "desert strike" (a subtitle), "taito chase h q" with
    /// "chase h q" (a publisher prefix) and "baku baku animal ..." with "baku baku".
    ///
    /// Equal-length token arrays never align: they are either the same title, which
    /// `match(title:in:)` has already answered, or they are two different titles.
    static func aligns(_ lhs: [String], _ rhs: [String]) -> Bool {
        guard !lhs.isEmpty, !rhs.isEmpty, lhs.count != rhs.count else { return false }
        let shorter = lhs.count < rhs.count ? lhs : rhs
        let longer = lhs.count < rhs.count ? rhs : lhs

        guard shorter.count >= 2 else { return false }
        guard shorter.joined(separator: " ").count >= 8 else { return false }

        // WHERE the residue sits is the whole basis of guards 3 and 4, and the first version of this
        // function ignored it. A sequel number goes at the END of a title ("Sonic Drift 2"), so a
        // trailing residue of bare numbers is the dangerous case those guards exist for. A LEADING
        // run of digits is something else entirely: a catalogue index from a numbered ROM set, as in
        // "1190 - Super Mario Advance 4 - Super Mario Bros 3 (E) (Menace)", whose cover the server
        // files as "Super Mario Advance 4 - Super Mario Bros. 3 (Europe) (En,Fr,De,Es,It).png". The
        // server title is a suffix of the file's, leaving "1190" at the front, and guards 3 and 4
        // both refused it, so a real cover was reported as a genuine miss.
        let residue: [String]
        let residueIsLeading: Bool
        if Array(longer.prefix(shorter.count)) == shorter {
            residue = Array(longer.dropFirst(shorter.count))
            residueIsLeading = false
        } else if Array(longer.suffix(shorter.count)) == shorter {
            residue = Array(longer.dropLast(shorter.count))
            residueIsLeading = true
        } else {
            return false
        }

        guard let first = residue.first, let last = residue.last else { return false }

        // A leading residue of nothing but DIGITS is a catalogue index, so it aligns. Digits only,
        // not `isSequenceMarker`: a roman numeral at the front of a title is not a set number, and
        // widening this to cover one would start matching sequels from the wrong end. The guards on
        // the shorter side still apply, so the matched title is still at least two words and eight
        // characters, and a match still has to be unique before anything is auto-picked.
        if residueIsLeading, residue.allSatisfy({ $0.allSatisfy(\.isNumber) }) {
            return true
        }

        guard residue.contains(where: { !isSequenceMarker($0) }) else { return false }
        guard !isSequenceMarker(first), !isSequenceMarker(last) else { return false }
        return true
    }

    /// Every listing title that aligns with this one, sorted so the answer never depends on the
    /// order a dictionary happened to hand its keys over in.
    ///
    /// The exact match is NOT in here. Equality is `match(title:in:)`, it is tried first, and a
    /// title that is equal is not an alignment.
    static func alignedTitles(for query: String, in map: [String: String]) -> [String] {
        guard !query.isEmpty else { return [] }
        let tokens = titleTokens(query)
        guard tokens.count >= 1 else { return [] }
        var out: [String] = []
        for key in map.keys where key != query {
            if aligns(tokens, titleTokens(key)) {
                out.append(key)
            }
        }
        return out.sorted()
    }

    /// The filename for the ONE listing title that aligns with this one, or nil when none does or
    /// more than one does.
    ///
    /// NIL ON AMBIGUITY IS THE WHOLE POINT, and it is what keeps automatic selection honest: two
    /// covers that both align is a question, and answering a question with a coin flip is how a
    /// wrong cover ends up on a card. The chooser in the detail sheet offers every one of them, so
    /// ambiguity becomes a user's choice instead.
    static func uniqueAlignedMatch(for query: String, in map: [String: String]) -> String? {
        let titles = alignedTitles(for: query, in: map)
        guard titles.count == 1, let title = titles.first else { return nil }
        return map[title]
    }

    /// The filename one system's folder list is persisted under: "gg-Named_Titles.json".
    ///
    /// PER FOLDER AND NOT PER SYSTEM, which is the fix for the bug that made this whole search
    /// blind. A system used to have exactly one list and it was always box art, so a game whose art
    /// exists only as a title screen or an in-game shot could never be found by the search that
    /// exists precisely for names the ladder cannot derive.
    static func listFilename(system: GameSystem, folder: ThumbnailFolder) -> String {
        "\(system.rawValue)-\(folder.rawValue).json"
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
    /// What the normalising and matching rules were when this list was folded onto titles.
    ///
    /// NOT DECORATION. The keys in `titles` are produced by `ArtworkIndexNames.normalisedTitle`, so
    /// a change to that function silently invalidates every stored map: the search would run, find
    /// nothing, and look exactly like a server with no art. Version 2 is the article inversion. A
    /// list written by version 1 has no `schema` key at all, so decoding it fails, which is the
    /// intended outcome: it is downloaded again under the new rules.
    static let currentSchema = 2

    let schema: Int
    /// The playlist directory the list was read from, so a list is discarded rather than trusted if
    /// this build's directory table ever changes for that system.
    let directory: String
    /// Which of the three thumbnail folders this list names. Checked against the folder being asked
    /// for, so a box art list can never be searched as though it were the title screens.
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

    private static func fileURL(for system: GameSystem, folder: ThumbnailFolder) -> URL? {
        // The system's short name and the server's own folder name, so the file is legible to
        // anyone who ever looks: "gg-Named_Titles.json".
        directory()?.appendingPathComponent(ArtworkIndexNames.listFilename(system: system,
                                                                          folder: folder))
    }

    /// The list a previous build wrote, which was one box art list per system under "nes.json".
    ///
    /// Kept only so it can be deleted. It cannot be read: its titles were normalised by the older
    /// rules and it carries no folder that can be trusted for the three-folder search.
    private static func legacyFileURL(for system: GameSystem) -> URL? {
        directory()?.appendingPathComponent("\(system.rawValue).json")
    }

    /// The stored list for one system's folder, or nil when there is none, it is too old, it was
    /// written for a different playlist directory or folder, or it was folded by older rules.
    static func stored(for system: GameSystem, folder: ThumbnailFolder) async -> ArtworkCoverList? {
        guard let url = fileURL(for: system, folder: folder) else { return nil }
        guard let data = try? Data(contentsOf: url, options: .mappedIfSafe) else { return nil }
        guard let list = try? JSONDecoder().decode(ArtworkCoverList.self, from: data) else {
            return nil
        }
        guard list.schema == ArtworkCoverList.currentSchema else { return nil }
        guard list.directory == SystemArtwork.playlistDirectory(for: system) else { return nil }
        guard list.folder == folder.rawValue else { return nil }
        guard list.age < lifetime else { return nil }
        return list
    }

    /// Searches one list for a title, OFF THE MAIN ACTOR.
    ///
    /// Non-isolated and async for the same reason `stored` and `download` are: the NES box art list
    /// carries 13,418 names folded onto roughly ten thousand titles, and the alignment walks every
    /// one of those keys splitting it into words. That is work, and it has no business happening
    /// inside the frame the library is drawing. `ArtworkStore` is @MainActor, so it must come
    /// through here rather than calling the pure functions directly.
    ///
    /// All three answers come back in one pass, because the caller needs the difference between
    /// them: an exact hit is taken, a single alignment is taken, and two or more alignments are
    /// deliberately NOT taken but are still worth reporting, since art for the title plainly exists.
    static func search(title: String,
                       in list: ArtworkCoverList) async -> (exact: String?,
                                                            uniqueAligned: String?,
                                                            aligned: [String]) {
        let exact = ArtworkIndexNames.match(title: title, in: list.titles)
        if let exact {
            return (exact: exact, uniqueAligned: nil, aligned: [])
        }
        let aligned = ArtworkIndexNames.alignedTitles(for: title, in: list.titles)
        let unique = aligned.count == 1 ? aligned.first.flatMap { list.titles[$0] } : nil
        return (exact: nil, uniqueAligned: unique, aligned: aligned)
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

    /// What one list is called in front of a user: "the Game Gear box art list".
    ///
    /// One function rather than a string built at each of the failure sites, so the three folders
    /// can never end up sharing a sentence and leaving a user unable to tell which one failed.
    static func listName(system: GameSystem, folder: ThumbnailFolder) -> String {
        "the \(system.displayName) \(folder.readableName) list"
    }

    /// The list for one system's folder: from disk when it is there and fresh, otherwise downloaded
    /// once.
    ///
    /// The caller is responsible for making sure this is not called twice at the same moment for one
    /// (system, folder) pair. `ArtworkStore` does that by holding a task per pair, the same way it
    /// holds one per game, because ten cards asking at once must not become ten megabyte downloads.
    static func list(for system: GameSystem,
                     folder: ThumbnailFolder) async -> ArtworkCoverListOutcome {
        if let stored = await stored(for: system, folder: folder) {
            return .ready(stored, downloadedBytes: nil, writeFailure: nil)
        }
        return await download(for: system, folder: folder)
    }

    /// Fetches, parses and stores one folder of one system's listing.
    static func download(for system: GameSystem,
                         folder: ThumbnailFolder) async -> ArtworkCoverListOutcome {
        guard let directory = SystemArtwork.playlistDirectory(for: system) else {
            return .failure("no thumbnail directory is known for \(system.displayName), so "
                            + "there is no \(folder.readableName) list to search")
        }
        guard let url = ArtworkNames.listingURL(system: system, folder: folder) else {
            return .failure("the address of \(listName(system: system, folder: folder))"
                            + " could not be built")
        }

        var request = URLRequest(url: url)
        request.httpMethod = "GET"
        request.setValue("text/html,*/*;q=0.8", forHTTPHeaderField: "Accept")

        // Named once and reused, so every failure below says WHICH of the three lists it was. Three
        // folders sharing one sentence would leave a user unable to tell a missing box art list from
        // a missing snapshot list.
        let name = listName(system: system, folder: folder)

        let data: Data
        let response: URLResponse
        do {
            (data, response) = try await session.data(for: request)
        } catch let error as URLError {
            return .failure("\(name) could not be downloaded: " + networkText(for: error))
        } catch {
            return .failure("\(name) could not be downloaded: " + error.localizedDescription)
        }

        guard let http = response as? HTTPURLResponse else {
            return .failure("\(name) came back with no HTTP response")
        }
        guard http.statusCode == 200 else {
            return .failure("\(name) answered HTTP \(http.statusCode)")
        }
        let contentType = (http.value(forHTTPHeaderField: "Content-Type") ?? "").lowercased()
        // A listing is HTML. Anything else means this is not the browsable index it looks like, and
        // parsing it anyway would be inventing names.
        guard contentType.contains("text/html") else {
            return .failure("\(name) came back as "
                            + "\(contentType.isEmpty ? "no content type" : contentType)"
                            + ", not a directory listing")
        }

        let filenames = ArtworkIndexNames.filenames(inListing: data)
        guard !filenames.isEmpty else {
            return .failure("\(name) downloaded \(data.count) byte(s) and named no cover at all")
        }

        let list = ArtworkCoverList(
            schema: ArtworkCoverList.currentSchema,
            directory: directory,
            folder: folder.rawValue,
            fetchedAt: Date().timeIntervalSince1970,
            listedFiles: filenames.count,
            listingBytes: data.count,
            titles: ArtworkIndexNames.titleMap(from: filenames)
        )
        let writeFailure = write(list, for: system, folder: folder)
        return .ready(list, downloadedBytes: data.count, writeFailure: writeFailure)
    }

    /// Persists a list. Returns a reason on failure rather than throwing, so the caller has a
    /// sentence to show: a list that is searched now and gone after a relaunch means the next
    /// launch downloads four megabytes again, which must never be silent.
    private static func write(_ list: ArtworkCoverList, for system: GameSystem,
                              folder: ThumbnailFolder) -> String? {
        guard let url = fileURL(for: system, folder: folder) else {
            return "there is no artwork directory, so it was not kept"
        }
        do {
            let data = try JSONEncoder().encode(list)
            try data.write(to: url, options: .atomic)
            // A list from the previous build sat under "nes.json", cannot be read by these rules and
            // will never be read again. Removed while we are here so the size in Settings is the
            // size of what is actually searchable, rather than that plus a few hundred dead
            // kilobytes. Housekeeping, so it is silent: there is no event here to report.
            if let legacy = legacyFileURL(for: system),
               FileManager.default.fileExists(atPath: legacy.path) {
                try? FileManager.default.removeItem(at: legacy)
            }
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
