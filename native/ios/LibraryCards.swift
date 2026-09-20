// Continuum - the pieces the library is built out of: the hero, the shelf cards, the list rows and
// the palette they share.
//
// Every one of these takes PLAIN VALUES plus plain object references, and observes nothing. That is
// deliberate and it is a performance decision rather than a style one: the display link publishes
// telemetry to `EngineHost` on every frame, so anything that declares `@ObservedObject var host`
// has its body re-evaluated sixty times a second. The shell observes the two objects once, at the
// top, and hands values down, so a card is only rebuilt when the card's own inputs change.

import SwiftUI

// MARK: - The palette

/// The shell's colours, in one place.
///
/// Red is the accent the design reference uses for Play, the active tab and the logo tile; teal is
/// for metadata values. Named rather than inlined so a second red cannot drift in.
enum ShellPalette {
    static let accent = Color(red: 0.93, green: 0.16, blue: 0.29)
    static let metadata = Color(red: 0.36, green: 0.86, blue: 0.79)
    static let surface = Color.white.opacity(0.08)
    static let surfaceStrong = Color.white.opacity(0.14)
    static let hairline = Color.white.opacity(0.12)
    static let secondaryText = Color.white.opacity(0.62)
}

/// What colour the little status dot is, read from the last status line.
///
/// A dot is not a diagnostic on its own, so this only ever summarises: the full sentence is one tap
/// away on the status strip and the whole block is behind the diagnostics toggle. The vocabulary
/// below is taken from the status strings this app actually writes, not from a general idea of what
/// an error looks like.
enum StatusTint {
    static func tint(for status: String) -> Color {
        let text = status.lowercased()
        let broken = ["failed", "cannot", "could not", "refused", "is missing", "not loaded",
                      "unavailable", "no core", "did not take", "error"]
        if broken.contains(where: { text.contains($0) }) { return ShellPalette.accent }
        let wary = ["skipped", "missing", "unreadable", "without a pick", "cancelled", "ignored",
                    "no output path", "not scanned", "empty"]
        if wary.contains(where: { text.contains($0) }) { return Color.orange }
        return ShellPalette.metadata
    }
}

// MARK: - Small parts

/// The system badge a card carries in its top-left corner: NES, SMS, PS1.
struct SystemBadge: View {
    let text: String

    var body: some View {
        Text(text)
            .font(.system(size: 10, weight: .bold, design: .rounded))
            .foregroundStyle(.white)
            .padding(.horizontal, 6)
            .padding(.vertical, 3)
            .background(.black.opacity(0.62), in: RoundedRectangle(cornerRadius: 6))
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
            .foregroundStyle(.white)
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
                .foregroundStyle(isFavourite ? Color.yellow : Color.white.opacity(0.75))
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

            // The scrim is what keeps the title legible over art nobody has vetted: a cover can be
            // any brightness at all, and light text on a white box art is unreadable.
            LinearGradient(
                stops: [
                    .init(color: .black.opacity(0), location: 0),
                    .init(color: .black.opacity(0.35), location: 0.45),
                    .init(color: .black.opacity(0.88), location: 0.86),
                    .init(color: .black, location: 1),
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
                .foregroundStyle(.white)
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
            Text("Imported \(entry.name) \u{00B7} \(entry.detail)")
                .font(.system(size: 13))
                .foregroundStyle(Color.white.opacity(0.78))
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

    /// Roughly a box-art aspect. Cards are a fixed shape so a shelf stays a straight line whatever
    /// shape the covers turn out to be.
    private var height: CGFloat { (width * 4.0 / 3.0).rounded() }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            CoverArtView(entry: entry, system: system, store: store,
                         generation: generation, showsCaption: width >= 92)
                .frame(width: width, height: height)
                .clipShape(RoundedRectangle(cornerRadius: 10))
                .overlay(
                    RoundedRectangle(cornerRadius: 10)
                        .strokeBorder(Color.white.opacity(0.10), lineWidth: 0.5)
                )
                .overlay(alignment: .topLeading) {
                    SystemBadge(text: system?.badge ?? entry.ext.uppercased())
                        .padding(6)
                }
                .overlay(alignment: .topTrailing) {
                    if isFavourite {
                        Image(systemName: "star.fill")
                            .font(.system(size: 11, weight: .bold))
                            .foregroundStyle(Color.yellow)
                            .padding(5)
                            .background(.black.opacity(0.55), in: Circle())
                            .padding(6)
                    }
                }

            if showsLabel {
                Text(GameMetadata.displayTitle(for: entry))
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(.white)
                    .lineLimit(2)
                    .multilineTextAlignment(.leading)
                    .fixedSize(horizontal: false, vertical: true)
                Text(GameMetadata.cardLine(for: entry, system: system))
                    .font(.system(size: 10))
                    .foregroundStyle(ShellPalette.secondaryText)
                    .lineLimit(1)
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

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(title)
                    .font(.system(size: 21, weight: .bold))
                    .foregroundStyle(.white)
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
                LazyHStack(alignment: .top, spacing: 12) {
                    // Identity is the entry id, the absolute path, never an index: the array is
                    // rebuilt from disk after every import and every delete.
                    ForEach(entries) { entry in
                        CoverCard(
                            entry: entry,
                            system: CoreCatalog.system(forExtension: entry.ext),
                            store: store,
                            generation: generation,
                            isFavourite: favourites.contains(entry.id),
                            width: 124,
                            host: host
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
/// one routing table, so a .sms that says anything but genesis_plus_gx is visible here, before a
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
                    .foregroundStyle(.white)
                    .lineLimit(1)
                // The filename on disk, in the monospace the debug list used. Truncated in the
                // MIDDLE, so a long No-Intro name keeps both its start and its extension, which is
                // what tells two dumps of one game apart.
                Text(entry.name)
                    .font(.system(.caption2, design: .monospaced))
                    .foregroundStyle(Color.white.opacity(0.7))
                    .lineLimit(1)
                    .truncationMode(.middle)
                // Exactly what the debug list's second line carried, core id last: a routing
                // mistake is visible across the whole library at a glance rather than after a tap.
                Text(entry.detail)
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
                    .foregroundStyle(Color.white.opacity(0.7))
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
                .foregroundStyle(.white)
            Text(guidance)
                .font(.system(size: 13))
                .foregroundStyle(Color.white.opacity(0.75))
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
