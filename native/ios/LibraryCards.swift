// Continuum - the pieces the library is built out of: the hero, the shelf cards, the list rows and
// the palette they share.
//
// Every one of these takes PLAIN VALUES plus plain object references, and observes nothing. That is
// deliberate and it is a performance decision rather than a style one: `EngineHost` publishes
// often (status lines, read-outs, player state), and anything that declares
// `@ObservedObject var host` has its body re-evaluated on every one of those changes. The shell observes the two objects once, at the
// top, and hands values down, so a card is only rebuilt when the card's own inputs change.

import SwiftUI
// For the dynamic colours below. Imported explicitly rather than relying on SwiftUI to re-export
// UIKit, which it is not documented to do (the same note as SettingsScreen.swift).
import UIKit

// MARK: - The palette

/// The shell's colours, in one place.
///
/// Red is the accent the design reference uses for Play, the active tab and the logo tile; teal is
/// for metadata values. Named rather than inlined so a second red cannot drift in.
///
/// EVERY COLOUR HAS A DARK AND A LIGHT VALUE (Settings, APPEARANCE: Dark, Light, System; owner,
/// 13 October 2026). The Dark values are exactly the colours this palette always had, so Dark,
/// the default, looks the way the app always did. The Light values mirror them: black where Dark
/// has white, white where Dark has black, at opacities picked so text reads as well on white as it
/// does on black. The right value is picked by whatever the view is drawn in, so one name works on
/// both and a screen changes the moment the setting does.
///
/// `text.opacity(x)` is the Light-aware form of the old `Color.white.opacity(x)`, and
/// `background.opacity(x)` of `Color.black.opacity(x)` used as a wash behind the page's own
/// content. Colours drawn OVER COVER ART (a card's badge and star) or over a running game stay
/// fixed white and black, because the art and the game look the same in both modes.
enum ShellPalette {
    static let accent = Color(red: 0.93, green: 0.16, blue: 0.29)
    /// The light teal on black; a deeper teal on white, so it reads there too.
    static let metadata = adaptive(dark: Dark.metadata,
                                   light: UIColor(red: 0.0, green: 0.47, blue: 0.42, alpha: 1))
    /// The page: black in Dark, white in Light. A game's own screen stays black in both.
    static let background = adaptive(dark: .black, light: .white)
    /// Text and icons on the page: white in Dark, black in Light.
    static let text = adaptive(dark: .white, light: .black)
    static let surface = adaptive(dark: UIColor(white: 1, alpha: 0.08),
                                  light: UIColor(white: 0, alpha: 0.05))
    static let surfaceStrong = adaptive(dark: Dark.surfaceStrong,
                                        light: UIColor(white: 0, alpha: 0.08))
    static let hairline = adaptive(dark: UIColor(white: 1, alpha: 0.12),
                                   light: UIColor(white: 0, alpha: 0.12))
    static let secondaryText = adaptive(dark: UIColor(white: 1, alpha: 0.62),
                                        light: UIColor(white: 0, alpha: 0.6))
    /// The small capital labels over a control ("VOLUME 80%", a read-out's name). A little
    /// stronger in Light, where black at 45% on white is fainter than white at 45% on black.
    static let tertiaryText = adaptive(dark: UIColor(white: 1, alpha: 0.45),
                                       light: UIColor(white: 0, alpha: 0.55))
    /// The recessed groove a segmented selector's thumb sits in (`SegmentedChoice`). Black at 28%
    /// in Dark; in Light a black that strong would be a dark bar, so it is the faint grey iOS's own
    /// light selector uses.
    static let groove = adaptive(dark: UIColor(white: 0, alpha: 0.28),
                                 light: UIColor(white: 0, alpha: 0.07))
    /// Under the selector's glass thumb. Nothing in Dark, as always; in Light, white, so the thumb
    /// stands out of the light groove the way iOS's own does instead of melting into it.
    static let thumbFill = adaptive(dark: .clear, light: UIColor(white: 1, alpha: 0.75))

    /// The Dark values as UIKit colours, for UIKit views on a screen that stays dark in both modes
    /// (the on-screen control editor's console menu).
    enum Dark {
        static let metadata = UIColor(red: 0.36, green: 0.86, blue: 0.79, alpha: 1)
        static let surfaceStrong = UIColor(white: 1, alpha: 0.14)
    }

    /// One UIKit colour with a Dark and a Light value, for UIKit views that follow the setting.
    /// Anything that is not Light (Dark, or not decided yet) gets the Dark value: the app was
    /// always dark.
    static func dynamic(dark: UIColor, light: UIColor) -> UIColor {
        UIColor(dynamicProvider: { traits in
            traits.userInterfaceStyle == .light ? light : dark
        })
    }

    /// The same as a SwiftUI colour. SwiftUI resolves it against the colour scheme the view is
    /// drawn in, so a screen that stays dark in Light (the layout editor and the skin editor, with
    /// `.preferredColorScheme(.dark)`) gets the Dark value there.
    static func adaptive(dark: UIColor, light: UIColor) -> Color {
        Color(uiColor: dynamic(dark: dark, light: light))
    }
}

/// Settings, APPEARANCE: how the whole app looks (owner, 13 October 2026). The words are the
/// owner's own, Dark, Light and System, never "Match my phone" or "Automatic".
///
/// DARK IS THE DEFAULT because the app was always dark, so nothing changes for anyone who leaves
/// it alone. Applied once, at the root (`RootView`), so every screen, sheet and menu follows at
/// once, with no restart. A game's own screen stays black around the picture in both, like every
/// emulator; the menus and sheets opened from a game follow the setting.
enum AppAppearance: String, CaseIterable, Identifiable {
    case dark
    case light
    case system

    var id: String { rawValue }

    var title: String {
        switch self {
        case .dark: return "Dark"
        case .light: return "Light"
        case .system: return "System"
        }
    }

    /// For SwiftUI's `preferredColorScheme`. Nil is System: the iPhone's own light or dark.
    var colorScheme: ColorScheme? {
        switch self {
        case .dark: return .dark
        case .light: return .light
        case .system: return nil
        }
    }

    /// The same choice for UIKit. Unspecified is System.
    var interfaceStyle: UIUserInterfaceStyle {
        switch self {
        case .dark: return .dark
        case .light: return .light
        case .system: return .unspecified
        }
    }

    /// Puts the choice on every window of the app's own scenes, and on each window's root view
    /// controller, alongside the root's `preferredColorScheme`.
    ///
    /// BOTH, ON PURPOSE. The window is what every sheet, alert, menu, picker and share sheet takes
    /// its look from, including the ones UIKit presents by itself (the Files picker, Mail, a
    /// game's manual). And going back to System through `preferredColorScheme(nil)` alone has been
    /// reported to leave the old style in place on some iOS versions until the app restarts;
    /// `.unspecified` here is UIKit's own documented way to follow the phone. The TV's window is
    /// another kind of scene and is left alone: it only ever shows the game, on black. So are
    /// windows above the normal level (the keyboard's own), which iOS styles itself.
    @MainActor
    func applyToWindows() {
        let style = interfaceStyle
        for scene in UIApplication.shared.connectedScenes {
            guard let windowScene = scene as? UIWindowScene,
                  windowScene.session.role == .windowApplication else { continue }
            for window in windowScene.windows where window.windowLevel == .normal {
                window.overrideUserInterfaceStyle = style
                window.rootViewController?.overrideUserInterfaceStyle = style
            }
        }
    }
}

/// What colour the little status dot is, read from the last status line.
///
/// A dot is not a diagnostic on its own, so this only ever summarises: a line that is not everyday
/// pops up in full just above the Library's tabs, and the whole block is behind the diagnostics
/// toggle. The same answer decides which status lines pop up (LibraryShell). The vocabulary
/// below is taken from the status strings this app actually writes, not from a general idea of what
/// an error looks like.
enum StatusTint {
    static func tint(for status: String) -> Color {
        switch level(for: status) {
        case .broken: return ShellPalette.accent
        case .wary: return Color.orange
        case .everyday: return ShellPalette.metadata
        }
    }

    /// Whether a status line is everyday (teal), rather than something skipped (orange) or
    /// something broken (red). What pops up, and how long a line stays, ask this. They used to
    /// compare the colour itself with the teal, which stopped being a safe question once the teal
    /// had a light and a dark value.
    static func isEveryday(_ status: String) -> Bool {
        level(for: status) == .everyday
    }

    private enum Level {
        case everyday
        case wary
        case broken
    }

    private static func level(for status: String) -> Level {
        let text = status.lowercased()
        // "won't start" is the engine's line for a game that did not start (launch_failure.rs).
        let broken = ["failed", "cannot", "could not", "refused", "is missing", "not loaded",
                      "unavailable", "no core", "did not take", "error", "won't start",
                      "won\u{2019}t start"]
        if broken.contains(where: { text.contains($0) }) { return .broken }
        let wary = ["skipped", "missing", "unreadable", "without a pick", "cancelled", "ignored",
                    "no output path", "not scanned", "empty"]
        if wary.contains(where: { text.contains($0) }) { return .wary }
        return .everyday
    }
}

// MARK: - Small parts

/// The system badge a card carries in its top-left corner: NES, SMS, PS1.
struct SystemBadge: View {
    let text: String
    /// Settings, LAYOUT, "Game card size". Medium is the badge as it always was.
    var cardSize: LibraryCardSize = .medium

    var body: some View {
        Text(text)
            .font(.system(size: cardSize.badgeFont, weight: .bold, design: .rounded))
            .foregroundStyle(.white)
            // One line that shrinks before it is cut short, so a long badge on a small card fits
            // beside the favourite star (`LibraryCardSize.starReserve`). A badge that fits, which
            // at Medium is every badge, is drawn exactly as before.
            .lineLimit(1)
            .minimumScaleFactor(0.7)
            .padding(.horizontal, cardSize.badgeHorizontalPadding)
            .padding(.vertical, cardSize.badgeVerticalPadding)
            .background(.black.opacity(0.62),
                        in: RoundedRectangle(cornerRadius: cardSize.badgeCornerRadius))
    }
}

/// A pill button: filled red for the primary action, grey for the secondary one.
struct PillButton: View {
    let title: String
    var systemImage: String?
    var filled: Bool
    var action: () -> Void

    private var fill: AnyShapeStyle {
        filled ? AnyShapeStyle(ShellPalette.accent) : AnyShapeStyle(ShellPalette.surfaceStrong)
    }

    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                if let systemImage {
                    Image(systemName: systemImage)
                        .font(.system(size: 13, weight: .bold))
                }
                Text(title)
                    .font(.system(size: 15, weight: .semibold))
            }
            // White on the red pill in both modes; the grey pill's text follows the page.
            .foregroundStyle(filled ? Color.white : ShellPalette.text)
            .padding(.horizontal, 18)
            .padding(.vertical, 11)
            .frame(minWidth: 108)
            // Type-erased because the two branches are different shape styles and a ternary has to
            // produce one type.
            .background(fill, in: Capsule())
        }
        .buttonStyle(.plain)
    }
}

/// A star that says whether a game is a favourite.
struct FavouriteButton: View {
    let isFavourite: Bool
    var size: CGFloat = 15
    var action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: isFavourite ? "star.fill" : "star")
                .font(.system(size: size, weight: .semibold))
                .foregroundStyle(isFavourite ? Color.yellow : ShellPalette.text.opacity(0.75))
                .frame(width: size + 18, height: size + 18)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(isFavourite ? "Remove from favorites" : "Add to favorites")
    }
}

// MARK: - The featured hero

/// The top of the Home tab: full-bleed art, the title, the metadata line, and the two pills.
///
/// The hero is chosen deterministically by the caller (the most recently added game) rather than at
/// random, so it does not shuffle on every rescan, and every rescan is exactly what an import and a
/// delete both trigger.
struct HeroCard: View {
    let entry: LibraryEntry
    let system: GameSystem?
    let store: ArtworkStore
    let generation: Int
    let isFavourite: Bool
    let height: CGFloat
    let host: EngineHost

    var body: some View {
        ZStack(alignment: .bottomLeading) {
            CoverArtView(entry: entry, system: system, store: store,
                         generation: generation, showsCaption: false)
                .frame(height: height)
                .frame(maxWidth: .infinity)
                // A LONG PRESS OPENS THE DETAIL SHEET HERE TOO, which is the one surface that used to
                // lack it: a shelf card, a grid card and a list row all have it, so a hero that did
                // not was the odd one out, and there is no way to discover that except by trying.
                //
                // ON THE ART LAYER RATHER THAN ON THE ZSTACK, deliberately. The Play and More info
                // pills and the favourite star are Buttons drawn ABOVE this in the same stack, so
                // they hit-test first and keep working exactly as they did; the press only belongs to
                // the art, which is the part of a hero a finger lands on.
                //
                // THE TAP IS NOT TOUCHED. On this surface Play is what launches the game, as it
                // always has been, and a full-bleed banner that launched a game when a finger brushed
                // it would be a trap rather than a convenience.
                .contentShape(Rectangle())
                .onLongPressGesture(minimumDuration: 0.4) {
                    host.detailEntry = entry
                }

            // The scrim is what keeps the title legible over art nobody has vetted: a cover can be
            // any brightness at all, and light text on a white box art is unreadable. It fades
            // into the page colour, so the cover runs into the shelves below it: black under white
            // text in Dark, white under black text in Light, at the same strengths.
            LinearGradient(
                stops: [
                    .init(color: ShellPalette.background.opacity(0), location: 0),
                    .init(color: ShellPalette.background.opacity(0.35), location: 0.45),
                    .init(color: ShellPalette.background.opacity(0.88), location: 0.86),
                    .init(color: ShellPalette.background, location: 1),
                ],
                startPoint: .top,
                endPoint: .bottom
            )
            .frame(height: height)
            .allowsHitTesting(false)

            content
                .padding(.horizontal, 18)
                .padding(.bottom, 14)
        }
        .frame(height: height)
        .clipped()
    }

    private var content: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("FEATURED")
                .font(.system(size: 12, weight: .bold))
                .tracking(2.4)
                .foregroundStyle(ShellPalette.accent)

            Text(GameMetadata.displayTitle(for: entry))
                .font(.system(size: 34, weight: .heavy))
                .foregroundStyle(ShellPalette.text)
                .lineLimit(2)
                .minimumScaleFactor(0.6)
                .fixedSize(horizontal: false, vertical: true)

            Text(GameMetadata.metaLine(for: entry, system: system))
                .font(.system(size: 15, weight: .medium))
                .foregroundStyle(ShellPalette.metadata)
                .lineLimit(1)
                .minimumScaleFactor(0.7)

            // The raw filename and the row detail, which is the honest substitute for the
            // description a store page would have. It names the file the core is actually handed,
            // the real size, any missing cue track, and the core the tap will route to.
            Text("Imported \(entry.name) \u{00B7} \(entry.routedDetail(ps1CoreId: host.ps1CoreChoice.coreId))")
                .font(.system(size: 13))
                .foregroundStyle(ShellPalette.text.opacity(0.78))
                .lineLimit(2)
                .fixedSize(horizontal: false, vertical: true)

            HStack(spacing: 10) {
                PillButton(title: "Play", systemImage: "play.fill", filled: true) {
                    host.launch(entry: entry)
                }
                PillButton(title: "More info", systemImage: nil, filled: false) {
                    host.detailEntry = entry
                }
                FavouriteButton(isFavourite: isFavourite, size: 17) {
                    host.toggleFavourite(entry)
                }
            }
            .padding(.top, 2)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

// MARK: - A shelf card

/// One cover-art card on a shelf or in a grid.
///
/// The badge sits in the top-left exactly as the design reference has it, over the art, because a
/// shelf of covers with no badges makes a Master System dump and a Mega Drive dump of the same game
/// indistinguishable.
struct CoverCard: View {
    let entry: LibraryEntry
    let system: GameSystem?
    let store: ArtworkStore
    let generation: Int
    let isFavourite: Bool
    let width: CGFloat
    let host: EngineHost
    /// The title and metadata under the card. On for a grid, off for a shelf, which is how the
    /// design reference reads: a shelf is covers, a grid is a catalogue.
    var showsLabel = false
    /// Settings, LAYOUT, "Game card size": the card's corners, fonts, badge and star. The width is
    /// passed on its own because a shelf card and a grid card are different widths at every size
    /// (`LibraryCardSize`). Medium is the card as it always was.
    var cardSize: LibraryCardSize = .medium

    /// Roughly a box-art aspect. Cards are a fixed shape so a shelf stays a straight line whatever
    /// shape the covers turn out to be.
    private var height: CGFloat { (width * 4.0 / 3.0).rounded() }

    var body: some View {
        VStack(alignment: .leading, spacing: cardSize.labelSpacing) {
            // The plate's own caption, on a game with no cover. On a shelf it is the only title
            // such a game has, so it shows at every size, scaled to the card. Under a grid card's
            // label it shows where it always did, on a card 92 points or wider; a smaller grid
            // card has the title right under it.
            CoverArtView(entry: entry, system: system, store: store,
                         generation: generation, showsCaption: !showsLabel || width >= 92,
                         captionScale: cardSize.plateCaptionScale)
                .frame(width: width, height: height)
                .clipShape(RoundedRectangle(cornerRadius: cardSize.cornerRadius))
                .overlay(
                    RoundedRectangle(cornerRadius: cardSize.cornerRadius)
                        .strokeBorder(ShellPalette.text.opacity(0.10), lineWidth: 0.5)
                )
                .overlay(alignment: .topLeading) {
                    SystemBadge(text: system?.badge ?? entry.ext.uppercased(), cardSize: cardSize)
                        .padding(cardSize.badgeInset)
                        // Room for the star on a favourite, so the two never overlap.
                        .padding(.trailing, isFavourite ? cardSize.starReserve : 0)
                }
                .overlay(alignment: .topTrailing) {
                    if isFavourite {
                        Image(systemName: "star.fill")
                            .font(.system(size: cardSize.starFont, weight: .bold))
                            .foregroundStyle(Color.yellow)
                            .padding(cardSize.starPadding)
                            .background(.black.opacity(0.55), in: Circle())
                            .padding(cardSize.badgeInset)
                    }
                }

            if showsLabel {
                Text(GameMetadata.displayTitle(for: entry))
                    .font(.system(size: cardSize.titleFont, weight: .semibold))
                    .foregroundStyle(ShellPalette.text)
                    .lineLimit(2)
                    .multilineTextAlignment(.leading)
                    .fixedSize(horizontal: false, vertical: true)
                if cardSize.showsCardLine {
                    Text(GameMetadata.cardLine(for: entry, system: system))
                        .font(.system(size: cardSize.cardLineFont))
                        .foregroundStyle(ShellPalette.secondaryText)
                        .lineLimit(1)
                        .minimumScaleFactor(cardSize.cardLineMinimumScale)
                }
            }
        }
        .frame(width: width)
        // A tap plays, a long press opens the detail sheet. Both are wired here rather than in the
        // parent so every card in the app behaves the same way.
        .contentShape(Rectangle())
        .onTapGesture {
            host.launch(entry: entry)
        }
        .onLongPressGesture(minimumDuration: 0.4) {
            host.detailEntry = entry
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(GameMetadata.displayTitle(for: entry)), "
                            + "\(GameMetadata.cardLine(for: entry, system: system))")
    }
}

// MARK: - A shelf

/// A titled horizontal row of cards, with its count, as the design reference has it.
struct ShelfRow: View {
    let title: String
    let entries: [LibraryEntry]
    let store: ArtworkStore
    let generation: Int
    let favourites: Set<String>
    let host: EngineHost
    /// Settings, LAYOUT, "Game card size": how wide the cards are and how far apart.
    var cardSize: LibraryCardSize = .medium

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(title)
                    .font(.system(size: 21, weight: .bold))
                    .foregroundStyle(ShellPalette.text)
                Text(entries.count == 1 ? "1 title" : "\(entries.count) titles")
                    .font(.system(size: 13))
                    .foregroundStyle(ShellPalette.secondaryText)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 16)

            ScrollView(.horizontal, showsIndicators: false) {
                // LAZY, and that is not a micro-optimisation. A plain HStack builds every card in
                // the shelf immediately, and every card starts an artwork lookup task when it is
                // created, so a per-system shelf holding two hundred games would spin up two
                // hundred tasks for the dozen cards a phone can actually show.
                LazyHStack(alignment: .top, spacing: cardSize.shelfSpacing) {
                    // Identity is the entry id, the absolute path, never an index: the array is
                    // rebuilt from disk after every import and every delete.
                    ForEach(entries) { entry in
                        CoverCard(
                            entry: entry,
                            system: CoreCatalog.system(for: entry),
                            store: store,
                            generation: generation,
                            isFavourite: favourites.contains(entry.name),
                            width: cardSize.shelfCardWidth,
                            host: host,
                            cardSize: cardSize
                        )
                    }
                }
                .padding(.horizontal, 16)
                .padding(.bottom, 2)
            }
        }
    }
}

// MARK: - A list row

/// One All Games row in list layout: a small cover, the display title, the raw filename and the
/// detail line the debug list always had.
///
/// THE FILENAME AND THE DETAIL LINE BOTH STAY ON THE ROW, and neither is decoration. The filename
/// is what the core is handed, it is the only thing that tells two dumps of one game apart, and it
/// is the string every diagnostic in this app names. The detail line ends with the core id from the
/// one routing table, so a .sms that says anything but gearsystem is visible here, before a
/// tap, rather than as a game that boots on the wrong emulator.
struct GameListRow: View {
    let entry: LibraryEntry
    let system: GameSystem?
    let store: ArtworkStore
    let generation: Int
    let isFavourite: Bool
    let host: EngineHost

    var body: some View {
        HStack(spacing: 12) {
            CoverArtView(entry: entry, system: system, store: store,
                         generation: generation, showsCaption: false)
                .frame(width: 44, height: 59)
                .clipShape(RoundedRectangle(cornerRadius: 6))

            VStack(alignment: .leading, spacing: 2) {
                Text(GameMetadata.displayTitle(for: entry))
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(ShellPalette.text)
                    .lineLimit(1)
                // The filename on disk, in the monospace the debug list used. Truncated in the
                // MIDDLE, so a long No-Intro name keeps both its start and its extension, which is
                // what tells two dumps of one game apart.
                Text(entry.name)
                    .font(.system(.caption2, design: .monospaced))
                    .foregroundStyle(ShellPalette.text.opacity(0.7))
                    .lineLimit(1)
                    .truncationMode(.middle)
                // Exactly what the debug list's second line carried, core id last: a routing
                // mistake is visible across the whole library at a glance rather than after a tap.
                Text(entry.routedDetail(ps1CoreId: host.ps1CoreChoice.coreId))
                    .font(.system(.caption2, design: .monospaced))
                    .foregroundStyle(ShellPalette.secondaryText)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }

            Spacer(minLength: 0)

            if isFavourite {
                Image(systemName: "star.fill")
                    .font(.system(size: 12))
                    .foregroundStyle(Color.yellow)
            }

            Button {
                host.detailEntry = entry
            } label: {
                Image(systemName: "info.circle")
                    .font(.system(size: 16))
                    .foregroundStyle(ShellPalette.text.opacity(0.7))
                    .frame(width: 34, height: 34)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("More info")
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
    }
}

// MARK: - An empty state

/// The empty state, which on this device has to be guidance rather than a shrug.
struct EmptyLibraryNotice: View {
    let title: String
    let guidance: String
    var actionTitle: String?
    var action: (() -> Void)?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title)
                .font(.system(size: 19, weight: .bold))
                .foregroundStyle(ShellPalette.text)
            Text(guidance)
                .font(.system(size: 13))
                .foregroundStyle(ShellPalette.text.opacity(0.75))
                .fixedSize(horizontal: false, vertical: true)
            if let actionTitle, let action {
                PillButton(title: actionTitle, systemImage: "plus", filled: true, action: action)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(18)
        .background(ShellPalette.surface, in: RoundedRectangle(cornerRadius: 14))
        .padding(.horizontal, 16)
    }
}
