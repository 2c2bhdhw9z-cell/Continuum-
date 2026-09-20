// Continuum - where cover art comes from, and the plate that is never blank.
//
// This file is the PURE half of the artwork system: the libretro playlist directory names, the
// filename to thumbnail-name transform, the ordered candidate ladder, and the procedural console
// plate. Nothing here touches the network, the filesystem or the engine, which is what makes every
// decision in it checkable by reading it. The fetching, caching and storing live in
// ArtworkStore.swift.
//
// THE PORT NOTE THAT MATTERS, because it inverts the browser build's weakest tier into this
// build's strongest. The browser could not download a thumbnail at all: thumbnails.libretro.com
// sends no Access-Control-Allow-Origin header, so fetch in cors mode is refused, no-cors yields an
// opaque response whose status is always 0 (so it cannot even tell a hit from a miss) and drawing
// the image into a canvas taints it. web/src/data/boxart.js therefore probes with <img> objects
// and persists only the resolved URL, and says plainly that scraped art is not guaranteed offline.
// CORS is a browser policy. URLSession is not subject to it, so this build downloads the real
// bytes, keeps them, and shows real box art with no network at all. The <img> probe workaround is
// deliberately NOT ported: it is a scar from a restriction that does not exist here.

import SwiftUI

// MARK: - The libretro thumbnail catalogue

/// Which libretro playlist directory each system's thumbnails live in, and the hue its fallback
/// plate is anchored on.
///
/// THE DIRECTORY NAMES ARE COPIED, NOT DERIVED. Every one of them was checked against the live
/// server for the browser build (web/src/data/boxart.js LIBRETRO_DIRS) and they are not guessable:
/// Master System is "Sega - Master System - Mark III", not "Sega - Master System", and a wrong
/// directory is indistinguishable from a game having no art, because both are a 404.
enum SystemArtwork {
    static let thumbnailHost = "https://thumbnails.libretro.com"

    /// The playlist directory for a system, or nil when this build has no name for it.
    ///
    /// Nil is a real answer and not a gap to paper over: a system with no directory is never
    /// probed at all, which is better than nine guaranteed 404s per game.
    static func playlistDirectory(for system: GameSystem) -> String? {
        switch system {
        case .nes: return "Nintendo - Nintendo Entertainment System"
        case .snes: return "Nintendo - Super Nintendo Entertainment System"
        case .gb: return "Nintendo - Game Boy"
        case .gbc: return "Nintendo - Game Boy Color"
        case .gba: return "Nintendo - Game Boy Advance"
        // The "Mark III" suffix is the trap in this table. It is the server's name, not a typo.
        case .sms: return "Sega - Master System - Mark III"
        case .genesis: return "Sega - Mega Drive - Genesis"
        case .ps1: return "Sony - PlayStation"
        // GAME GEAR HAD NO ENTRY IN THE BROWSER TABLE, so this one row is new rather than copied.
        // It was confirmed against the live server from the build sandbox rather than assumed:
        // the directory listing at /Sega%20-%20Game%20Gear/Named_Boxarts/ answers 200 and a file
        // from it downloads as image/png. That listing is also independent evidence for the
        // substitution rule below, because it contains
        // "Adventures of Batman _ Robin, The (USA, Europe)", which is an ampersand replaced by a
        // single underscore with both of its spaces intact. Still worth one on-device check,
        // because a sandbox and a phone are not the same network.
        case .gg: return "Sega - Game Gear"
        }
    }

    /// The hue the system's fallback plate is anchored on, in degrees.
    ///
    /// Copied from web/src/data/systems.js so a Game Boy plate lands in the same green family it
    /// did in the browser build and a shelf reads as a set. Game Gear is the one value chosen
    /// here, because the browser had no Game Gear row: 165 is a teal that sits far from Master
    /// System's 18 and Mega Drive's 220, so the three Sega systems stay apart on one shelf.
    static func hue(for system: GameSystem?) -> Double {
        guard let system else { return 210 }
        switch system {
        case .nes: return 355
        case .snes: return 268
        case .gb: return 88
        case .gbc: return 45
        case .gba: return 200
        case .sms: return 18
        case .genesis: return 220
        case .ps1: return 240
        case .gg: return 165
        }
    }

    /// True when this build knows where to look for the system's thumbnails.
    static func hasThumbnails(for system: GameSystem?) -> Bool {
        guard let system else { return false }
        return playlistDirectory(for: system) != nil
    }
}

/// The three thumbnail folders, in the order the specification asks for.
///
/// Box art first because it is what the user means by the art of the game; the title screen and
/// the in-game shot are what a game with no scanned cover still has.
enum ThumbnailFolder: String, CaseIterable, Sendable {
    case boxart = "Named_Boxarts"
    case title = "Named_Titles"
    case snap = "Named_Snaps"

    /// The short tier label recorded with a hit, matching the browser's vocabulary.
    var tier: String {
        switch self {
        case .boxart: return "boxart"
        case .title: return "title"
        case .snap: return "snap"
        }
    }

    /// What the tier is called in front of a user.
    var readableName: String {
        switch self {
        case .boxart: return "box art"
        case .title: return "title screen"
        case .snap: return "in-game shot"
        }
    }
}

/// One address worth trying, with enough provenance to say afterwards where a cover came from.
struct ArtworkCandidate: Sendable, Equatable {
    let url: URL
    /// "boxart", "title-relaxed", "snap-untagged" and so on: folder plus name form.
    let tier: String
    /// The thumbnail name this candidate asked for, after substitution.
    let name: String
    let folder: ThumbnailFolder
    /// Which of the three name forms produced it, for the detail sheet's read-out.
    let form: ArtworkNames.NameForm
}

// MARK: - Filename to thumbnail name

/// The transform from a filename on disk to a libretro thumbnail name, and the ladder built from
/// it. Pure, so every rule below can be read and checked without a device or a network.
enum ArtworkNames {
    /// Which of the three name forms a candidate came from, most specific first.
    enum NameForm: String, Sendable, Equatable {
        /// The base name exactly as it sits on disk: "Super Mario World (USA) [!]".
        case exact
        /// Square-bracket dump tags dropped: "Super Mario World (USA)".
        case relaxed
        /// Every (..) and [..] tag dropped: "Super Mario World".
        case untagged

        /// The suffix appended to the tier label, matching the browser's strings.
        var tierSuffix: String {
            switch self {
            case .exact: return ""
            case .relaxed: return "-relaxed"
            case .untagged: return "-untagged"
            }
        }

        var readableName: String {
            switch self {
            case .exact: return "exact name"
            case .relaxed: return "dump tags dropped"
            case .untagged: return "all tags dropped"
            }
        }
    }

    /// The characters libretro requires to be replaced in a thumbnail filename.
    ///
    /// Listed one per line so the set cannot be misread: ampersand, asterisk, forward slash,
    /// colon, backtick, less-than, greater-than, question mark, backslash, pipe, double quote.
    static let invalidCharacters: Set<Character> = [
        "&",
        "*",
        "/",
        ":",
        "`",
        "<",
        ">",
        "?",
        "\\",
        "|",
        "\"",
    ]

    /// Replaces every invalid character with a single underscore.
    ///
    /// THIS IS A SUBSTITUTION AND NOT A STRIP, and the difference is the whole rule. The
    /// underscore holds the character's position, so "Ratchet & Clank" becomes "Ratchet _ Clank"
    /// with both spaces intact. Deleting the character instead would produce "Ratchet  Clank",
    /// with a double space, and never match anything on the server.
    static func sanitizeForLibretro(_ name: String) -> String {
        String(name.map { invalidCharacters.contains($0) ? "_" : $0 })
    }

    /// "Super Mario World (USA) [!].sfc" becomes "Super Mario World (USA) [!]".
    ///
    /// Drops the last dot and everything after it, and only when something follows the dot, which
    /// is what the browser's /\.[^.]+$/ did. A name with no dot comes back untouched.
    static func baseName(_ filename: String) -> String {
        guard let dot = filename.lastIndex(of: "."), dot < filename.index(before: filename.endIndex)
        else { return filename }
        return String(filename[filename.startIndex..<dot])
    }

    /// Collapses runs of two or more whitespace characters into one space, then trims.
    ///
    /// Mirrors the browser's replace(/\s{2,}/g, ' ').trim(): a SINGLE whitespace character is left
    /// exactly as it was, because the only thing being repaired here is the gap a dropped tag
    /// leaves behind.
    static func collapse(_ name: String) -> String {
        var out = ""
        var run: [Character] = []
        func flushRun() {
            if run.count >= 2 {
                out.append(" ")
            } else {
                out.append(contentsOf: run)
            }
            run.removeAll(keepingCapacity: true)
        }
        for character in name {
            if character.isWhitespace {
                run.append(character)
            } else {
                flushRun()
                out.append(character)
            }
        }
        flushRun()
        return out.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// Drops only the square-bracket dump tags: [!], [b1], [T+Eng] and the rest of that
    /// vocabulary. Each tag becomes a space, and the spacing is then collapsed.
    ///
    /// THIS FORM EARNS ITS PLACE, and it is why the ladder has three rungs rather than two.
    /// Libretro names games after No-Intro, which uses PARENTHESISED regions, while dump tags in
    /// square brackets are a different convention layered on top. Dropping everything on the first
    /// retry throws the region away with the noise: "Super Mario World (USA) [!]" becomes "Super
    /// Mario World", which the server does not have, while "Super Mario World (USA)" is a 200.
    /// Measured against the live server rather than assumed, twice now: once for the browser build
    /// and again from the sandbox that wrote this file.
    ///
    /// An unterminated "[" is left alone, because the browser's regex required a closing bracket
    /// to match and a half-written name must not be silently rewritten.
    static func stripDumpTags(_ name: String) -> String {
        collapse(dropBracketed(name, openers: ["["], closers: ["]"]))
    }

    /// Drops every [..] and (..) tag. The last resort, tried only after the two above.
    ///
    /// The closing set is deliberately shared between both opener kinds, exactly as the browser's
    /// /[([][^)\]]*[)\]]/g was: "(USA]" is treated as one tag. Reproducing that is the point, so
    /// that a name which resolved in the browser resolves here.
    static func stripTags(_ name: String) -> String {
        collapse(dropBracketed(name, openers: ["(", "["], closers: [")", "]"]))
    }

    /// The shared scanner behind both strippers. Each matched tag becomes one space.
    private static func dropBracketed(_ name: String,
                                      openers: Set<Character>,
                                      closers: Set<Character>) -> String {
        var out = ""
        let characters = Array(name)
        var index = 0
        while index < characters.count {
            let character = characters[index]
            guard openers.contains(character) else {
                out.append(character)
                index += 1
                continue
            }
            // Look for the closer. The body may contain anything that is not itself a closer,
            // including another opener, which is what the browser's character class allowed.
            var scan = index + 1
            while scan < characters.count, !closers.contains(characters[scan]) {
                scan += 1
            }
            if scan < characters.count {
                out.append(" ")
                index = scan + 1
            } else {
                // No closer anywhere. Nothing matched, so the opener is a literal character.
                out.append(character)
                index += 1
            }
        }
        return out
    }

    /// The three name forms for a filename, most specific first, with the empty and the duplicate
    /// dropped.
    ///
    /// The dedupe is not tidiness. "Tetris (World)" has no square-bracket tags, so its relaxed
    /// form is identical to its exact one, and probing it again would be three wasted requests per
    /// game. A form that reduces to nothing, which is what a filename that is only tags does, is
    /// dropped for the same reason.
    static func nameForms(for filename: String) -> [(form: NameForm, name: String)] {
        let base = baseName(filename)
        let candidates: [(NameForm, String)] = [
            (.exact, sanitizeForLibretro(collapse(base))),
            (.relaxed, sanitizeForLibretro(stripDumpTags(base))),
            (.untagged, sanitizeForLibretro(stripTags(base))),
        ]

        var seen = Set<String>()
        var forms: [(form: NameForm, name: String)] = []
        for (form, name) in candidates {
            guard !name.isEmpty, !seen.contains(name) else { continue }
            seen.insert(name)
            forms.append((form: form, name: name))
        }
        return forms
    }

    /// The ordered candidate list for one game: three name forms, each across three folders.
    ///
    /// NAME-MAJOR, NOT FOLDER-MAJOR, and that ordering is the performance argument. An exact-name
    /// box art is by far the commonest outcome, so the common case costs ONE request and only a
    /// genuine miss walks all nine. Folder-major would put "the title screen under the exact name"
    /// ahead of "the box art with the dump tag dropped", which is both slower and worse art.
    static func candidates(system: GameSystem, filename: String) -> [ArtworkCandidate] {
        guard let directory = SystemArtwork.playlistDirectory(for: system) else { return [] }
        guard let encodedDirectory = percentEncoded(directory) else { return [] }

        var out: [ArtworkCandidate] = []
        for (form, name) in nameForms(for: filename) {
            guard let encodedName = percentEncoded(name) else { continue }
            for folder in ThumbnailFolder.allCases {
                let address = "\(SystemArtwork.thumbnailHost)/\(encodedDirectory)"
                    + "/\(folder.rawValue)/\(encodedName).png"
                guard let url = URL(string: address) else { continue }
                out.append(
                    ArtworkCandidate(
                        url: url,
                        tier: "\(folder.tier)\(form.tierSuffix)",
                        name: name,
                        folder: folder,
                        form: form
                    )
                )
            }
        }
        return out
    }

    /// Percent-encodes one path component the way the browser's encodeURIComponent did.
    ///
    /// The allowed set is encodeURIComponent's unreserved set, so the addresses this build asks
    /// for are byte for byte the ones the browser build asked for: alphanumerics plus - _ . ! ~ *
    /// ' ( ). Parentheses stay raw, which matters because No-Intro regions are parenthesised and
    /// the server's own directory listing spells them raw too.
    static func percentEncoded(_ component: String) -> String? {
        var allowed = CharacterSet.alphanumerics
        allowed.insert(charactersIn: "-_.!~*'()")
        return component.addingPercentEncoding(withAllowedCharacters: allowed)
    }
}

// MARK: - The plate that is never blank

/// The procedural console plate: the fallback that means no card is ever empty, and the loading
/// state for a cover that has not arrived yet.
///
/// Ported from web/src/ui/art.js rather than reinvented. A stable FNV-1a hash of the title plus
/// the entry id picks one of five patterns, and the SYSTEM anchors the hue, which is what makes a
/// shelf read as a set instead of as noise. Zero requests, zero bytes, no layout shift, and it
/// works with no network.
///
/// REAL ART IS DRAWN OVER THIS, NEVER INSTEAD OF IT. That is what makes the plate the letterbox
/// behind a cover that does not fill its box, and the placeholder for one that has not decoded.
struct ArtPlate: View {
    let entry: LibraryEntry
    let system: GameSystem?
    /// Whether to draw the badge and title. Off behind real art, and off for a thumbnail too
    /// small to read.
    var showsCaption: Bool = true

    var body: some View {
        ZStack {
            fill
            if showsCaption {
                caption
            }
        }
    }

    /// One of five gradients, chosen by the hash. Reproduced as SwiftUI gradients rather than as
    /// CSS, with the same hue arithmetic so a plate keeps the family it had in the browser.
    @ViewBuilder
    private var fill: some View {
        let seed = ArtPlate.seed(for: entry)
        let hueA = ArtPlate.hueA(seed: seed, system: system)
        let hueB = ArtPlate.hueB(seed: seed, hueA: hueA)

        switch ArtPlate.pattern(for: seed) {
        case 0:
            // Diagonal duotone.
            LinearGradient(
                colors: [ArtPlate.hsl(hueA, 62, 42), ArtPlate.hsl(hueB, 58, 14)],
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            )
        case 1:
            // Vertical fade with a bright horizon.
            LinearGradient(
                stops: [
                    .init(color: ArtPlate.hsl(hueA, 70, 52), location: 0),
                    .init(color: ArtPlate.hsl(hueB, 60, 22), location: 0.55),
                    .init(color: ArtPlate.hsl(hueB, 55, 10), location: 1),
                ],
                startPoint: .top,
                endPoint: .bottom
            )
        case 2:
            // Radial spotlight. The radius is taken from the box rather than fixed, so the same
            // plate reads the same on a shelf card and on the hero.
            GeometryReader { proxy in
                RadialGradient(
                    colors: [ArtPlate.hsl(hueA, 78, 56), ArtPlate.hsl(hueB, 62, 12)],
                    center: UnitPoint(x: 0.3, y: 0.2),
                    startRadius: 0,
                    endRadius: max(proxy.size.width, proxy.size.height) * 1.1
                )
            }
        case 3:
            // Hard-edged retro bands.
            LinearGradient(
                stops: [
                    .init(color: ArtPlate.hsl(hueA, 68, 48), location: 0),
                    .init(color: ArtPlate.hsl(hueA, 68, 48), location: 0.38),
                    .init(color: ArtPlate.hsl(hueB, 64, 30), location: 0.38),
                    .init(color: ArtPlate.hsl(hueB, 64, 30), location: 0.62),
                    .init(color: ArtPlate.hsl(hueB, 60, 12), location: 0.62),
                    .init(color: ArtPlate.hsl(hueB, 60, 12), location: 1),
                ],
                startPoint: UnitPoint(x: 0.2, y: 0),
                endPoint: UnitPoint(x: 0.8, y: 1)
            )
        default:
            // Corner sweep.
            AngularGradient(
                colors: [
                    ArtPlate.hsl(hueA, 72, 50),
                    ArtPlate.hsl(hueB, 58, 16),
                    ArtPlate.hsl(hueA, 60, 28),
                ],
                center: UnitPoint(x: 0.7, y: 0.3),
                angle: .degrees(200)
            )
        }
    }

    /// The badge and the title. The browser drew a one or two letter glyph; this build has a real
    /// system badge and a display title already, and a plate that says which game it is beats a
    /// prettier one that does not.
    private var caption: some View {
        VStack(alignment: .leading, spacing: 4) {
            Spacer(minLength: 0)
            Text(system?.badge ?? entry.ext.uppercased())
                .font(.system(size: 13, weight: .heavy, design: .rounded))
                .foregroundStyle(Color.white.opacity(0.85))
            Text(GameMetadata.displayTitle(for: entry))
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(Color.white.opacity(0.92))
                .lineLimit(3)
                .multilineTextAlignment(.leading)
        }
        .shadow(color: .black.opacity(0.55), radius: 3, x: 0, y: 1)
        .padding(8)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomLeading)
    }

    // ------------------------------------------------------------------ the pure arithmetic

    /// FNV-1a over the display title plus the entry id.
    ///
    /// Cheap, and stable for the life of the file, so a plate never changes between launches. The
    /// id is the absolute path, so two dumps of the same game get different plates rather than
    /// looking like duplicates.
    static func seed(for entry: LibraryEntry) -> UInt32 {
        fnv1a(GameMetadata.displayTitle(for: entry) + entry.id)
    }

    /// 32-bit FNV-1a over the string's UTF-16 code units.
    ///
    /// UTF-16 rather than UTF-8 on purpose: the browser hashed charCodeAt, so matching it keeps a
    /// plate identical across the two builds, which is one less thing to explain when comparing a
    /// screenshot to docs/mobile-after.png.
    static func fnv1a(_ text: String) -> UInt32 {
        var hash: UInt32 = 0x811c_9dc5
        for unit in text.utf16 {
            hash ^= UInt32(unit)
            hash = hash &* 0x0100_0193
        }
        return hash
    }

    static func pattern(for seed: UInt32) -> Int {
        Int((seed >> 16) % 5)
    }

    static func hueA(seed: UInt32, system: GameSystem?) -> Double {
        let base = SystemArtwork.hue(for: system)
        let wobble = Double(seed % 40) - 20
        return (base + wobble + 360).truncatingRemainder(dividingBy: 360)
    }

    static func hueB(seed: UInt32, hueA: Double) -> Double {
        (hueA + 150 + Double((seed >> 8) % 60)).truncatingRemainder(dividingBy: 360)
    }

    /// CSS hsl() as a SwiftUI Color.
    ///
    /// Written out rather than routed through Color(hue:saturation:brightness:), which is HSB and
    /// would shift every one of these colours: HSL's lightness 0.5 is HSB's brightness 1.0 at full
    /// saturation, so the plates would come out pale.
    static func hsl(_ hueDegrees: Double, _ saturationPercent: Double,
                    _ lightnessPercent: Double) -> Color {
        let hue = ((hueDegrees.truncatingRemainder(dividingBy: 360)) + 360)
            .truncatingRemainder(dividingBy: 360)
        let saturation = min(max(saturationPercent / 100, 0), 1)
        let lightness = min(max(lightnessPercent / 100, 0), 1)

        let chroma = (1 - abs(2 * lightness - 1)) * saturation
        let sector = hue / 60
        let secondary = chroma * (1 - abs(sector.truncatingRemainder(dividingBy: 2) - 1))
        let match = lightness - chroma / 2

        var red = 0.0
        var green = 0.0
        var blue = 0.0
        switch Int(sector) {
        case 0: (red, green, blue) = (chroma, secondary, 0)
        case 1: (red, green, blue) = (secondary, chroma, 0)
        case 2: (red, green, blue) = (0, chroma, secondary)
        case 3: (red, green, blue) = (0, secondary, chroma)
        case 4: (red, green, blue) = (secondary, 0, chroma)
        default: (red, green, blue) = (chroma, 0, secondary)
        }

        return Color(red: red + match, green: green + match, blue: blue + match)
    }
}
