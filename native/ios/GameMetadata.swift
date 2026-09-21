// Continuum - the metadata a filename can honestly be made to yield.
//
// The library shows "NES - USA - 24.0 KB - Imported" under a title, and every one of those four
// parts has to come from something this app actually knows. It knows three things about a game: the
// filename, the file size, and which system the extension routed to. So that is what this file
// derives, and nothing else. There is no year, no genre, no publisher and no rating anywhere in
// here, because nothing in a ROM file reliably carries them, and a front end that prints a
// confident "1994 - JRPG - 8.4/10" under a file it was handed five seconds ago is making it up.
//
// THE RAW FILENAME IS NEVER REPLACED, ONLY ACCOMPANIED. `LibraryEntry.name` is deliberately the
// filename exactly as it sits on disk, because it is what the core is handed and because two dumps
// of one game have to stay distinguishable. So the display title derived here is shown ALONGSIDE
// the filename, never instead of it: the filename is on every card, in every All Games row, in the
// hero and in the detail sheet. FEAT-003 argued against having a display-title parser at all for
// exactly that reason, and the constraint it was protecting is kept by showing both.

import Foundation

/// Display strings derived from a filename. Pure, so the parsing rules can be checked by reading
/// them and by running them, which is how the region table below was settled.
enum GameMetadata {
    /// The title to show: the extension gone, the square-bracket dump tags gone, the region kept.
    ///
    /// "Tetris (USA).nes" becomes "Tetris (USA)", and "Super Mario World (USA) [!].sfc" becomes
    /// "Super Mario World (USA)". The parenthesised region STAYS, which matches the design
    /// reference (docs/library-mobile.png shows the hero titled "Tetris (USA)") and is also the
    /// honest choice: "(USA)" and "(Japan)" are different games often enough to matter, and a
    /// title that hides the difference would make two rows look like a duplicated import.
    ///
    /// Shares its two transforms with the thumbnail ladder rather than restating them, so the title
    /// on screen and the name asked of the server cannot drift apart.
    static func displayTitle(for entry: LibraryEntry) -> String {
        displayTitle(fromFilename: entry.name)
    }

    static func displayTitle(fromFilename filename: String) -> String {
        let base = ArtworkNames.stripDumpTags(ArtworkNames.baseName(filename))
        // A filename that was nothing but an extension and tags leaves nothing to show. The
        // filename itself is then the only honest title.
        return base.isEmpty ? filename : base
    }

    /// The region a No-Intro or GoodTools style tag names, or nil when no tag says.
    ///
    /// Only PARENTHESISED tags are considered, because that is where No-Intro puts the region, and
    /// every tag on the name is offered in order so that "Sonic (USA, Europe) (Rev A)" answers
    /// "USA, Europe" rather than stopping at the first tag it finds and calling "Rev A" a region.
    /// A tag that is not in the table below is skipped rather than guessed at: nil is a fine
    /// answer, and the metadata line simply leaves the region out.
    static func region(fromFilename filename: String) -> String? {
        for tag in parentheticalTags(in: ArtworkNames.baseName(filename)) {
            if let region = regionName(forTag: tag) { return region }
        }
        return nil
    }

    /// Every (..) tag in a name, in order, trimmed, without its brackets.
    static func parentheticalTags(in name: String) -> [String] {
        var tags: [String] = []
        var current: String?
        for character in name {
            if character == "(" {
                // A nested or repeated opener restarts the tag, which is what the ladder's
                // stripper does too: the body of a tag never contains a closing bracket.
                current = ""
            } else if character == ")" {
                if let tag = current?.trimmingCharacters(in: .whitespaces), !tag.isEmpty {
                    tags.append(tag)
                }
                current = nil
            } else if current != nil {
                current?.append(character)
            }
        }
        return tags
    }

    /// One region tag to its display form, or nil when the tag is not a region.
    ///
    /// Two conventions are covered because both are common in the wild. No-Intro spells regions
    /// out, as "(USA)", "(Europe)" and "(USA, Europe)". GoodTools abbreviates them to single
    /// letters and runs them together, as "(U)", "(E)", "(J)", "(UE)" and "(JUE)". Both are listed
    /// explicitly rather than pattern-matched, because a one or two letter pattern would swallow
    /// half the other tag vocabulary: "(M3)" is a multi-language count and "(PD)" is public
    /// domain, and neither is a region.
    static func regionName(forTag tag: String) -> String? {
        let key = tag.lowercased()
        if let direct = abbreviatedRegions[key] { return direct }

        // A comma-separated list of spelled-out regions, which is how No-Intro writes a
        // multi-region dump: "USA, Europe" or "Japan, USA, Korea". Every element has to be a known
        // region, so "Rev A" and "1995-04-27" cannot slip through as one.
        let parts = key.split(separator: ",").map {
            $0.trimmingCharacters(in: .whitespaces)
        }
        guard parts.count > 1 else { return spelledRegions[key] }
        var named: [String] = []
        for part in parts {
            guard let name = spelledRegions[part] else { return nil }
            named.append(name)
        }
        return named.joined(separator: ", ")
    }

    /// Regions spelled out, as No-Intro writes them.
    private static let spelledRegions: [String: String] = [
        "usa": "USA",
        "europe": "Europe",
        "japan": "Japan",
        "world": "World",
        "korea": "Korea",
        "china": "China",
        "taiwan": "Taiwan",
        "asia": "Asia",
        "australia": "Australia",
        "brazil": "Brazil",
        "canada": "Canada",
        "france": "France",
        "germany": "Germany",
        "hong kong": "Hong Kong",
        "italy": "Italy",
        "netherlands": "Netherlands",
        "russia": "Russia",
        "scandinavia": "Scandinavia",
        "spain": "Spain",
        "sweden": "Sweden",
        "uk": "UK",
    ]

    /// The GoodTools letter codes, including the combinations that are common enough to see.
    private static let abbreviatedRegions: [String: String] = [
        "u": "USA",
        "e": "Europe",
        "j": "Japan",
        "w": "World",
        "ue": "USA, Europe",
        "eu": "USA, Europe",
        "ju": "Japan, USA",
        "uj": "Japan, USA",
        "je": "Japan, Europe",
        "jue": "Japan, USA, Europe",
        "jeu": "Japan, USA, Europe",
        "uje": "Japan, USA, Europe",
        "k": "Korea",
        "a": "Australia",
        "b": "Brazil",
        "c": "China",
        "f": "France",
        "g": "Germany",
        "i": "Italy",
        "s": "Spain",
        "nl": "Netherlands",
        "hk": "Hong Kong",
    ]

    /// The hero and card metadata line: "NES - USA - 24.0 KB - Imported", with the middle dot the
    /// design reference uses.
    ///
    /// The size comes from `LibraryEntry.sizeText`, which for a cue sheet is the whole game and not
    /// the 87 bytes of the sheet, so a PlayStation disc reads as the hundreds of megabytes it is.
    /// "Imported" is stated rather than assumed: everything in the library got there by being
    /// copied into Documents, and saying so is what makes the line true if a bundled cart is ever
    /// added.
    static func metaLine(for entry: LibraryEntry, system: GameSystem?) -> String {
        var parts = [system?.badge ?? entry.ext.uppercased()]
        if let region = region(fromFilename: entry.name) { parts.append(region) }
        parts.append(entry.sizeText)
        parts.append("Imported")
        return parts.joined(separator: " \u{00B7} ")
    }

    /// The shorter line a card sits under: "NES - 24.0 KB", with the region when there is one.
    static func cardLine(for entry: LibraryEntry, system: GameSystem?) -> String {
        var parts = [system?.badge ?? entry.ext.uppercased()]
        if let region = region(fromFilename: entry.name) { parts.append(region) }
        parts.append(entry.sizeText)
        return parts.joined(separator: " \u{00B7} ")
    }
}
