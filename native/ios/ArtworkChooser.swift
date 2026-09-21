// Continuum - every cover the server has for one game, offered in that game's card and nowhere else.
//
// WHY THIS IS A SEPARATE FILE, AND WHY NONE OF IT RUNS WHILE THE LIBRARY IS BEING BROWSED.
// Automatic resolution is deliberately cheap and deliberately silent: the first address that answers
// with an image wins, nothing on a card hints that there might have been another, and a title that
// two covers both match keeps its plate rather than being guessed at. That is the whole contract of
// the ladder and the list search in ArtworkStore.swift and ArtworkIndex.swift, and this file must not
// touch it. So the full enumeration lives here, it is reached only by opening a game's detail sheet,
// and the store holds no @Published state for it: the library shell observes the store, so a
// thumbnail published there would rebuild the whole shell for every image that lands.
//
// WHAT THE ENUMERATION IS. For the game's own system, every one of the three folder lists; then, for
// the other systems, whatever is already on disk, plus more on an explicit tap. Each listed filename
// that matches the game's title, by equality or by the whole-word alignment, becomes one option with
// a label that says what it is and where it came from: "box art", "title screen", "in-game shot",
// "box art from Game Boy". It deliberately includes the AMBIGUOUS matches automatic resolution
// refused, because that is the elegance of the arrangement: two covers is a question, and this is
// where the question gets asked.
//
// A CHOSEN COVER OUTRANKS EVERYTHING. It is stored under the game's own cover key like any other, it
// is recorded against the entry's stable id, and `ArtworkStore.resolve` restores it rather than
// walking the ladder if the file behind it ever goes missing. Nothing automatic overwrites it.

import SwiftUI
// UIKit explicitly, as ArtworkStore.swift does: this file names UIImage, and relying on SwiftUI to
// re-export it is the kind of implicit dependency that breaks on a compiler upgrade.
import UIKit

// MARK: - One offer

/// One cover a user could choose, and everything needed to fetch it, label it and recognise it again
/// after a relaunch.
///
/// The id is built from the system, the folder and the thumbnail name rather than from the URL,
/// because it is persisted: a percent-encoding change would make a stored choice unrecognisable,
/// while these three strings are what the server itself is organised by.
struct ArtworkOption: Identifiable, Sendable, Equatable {
    /// Whose thumbnail directory the URL addresses. Not necessarily the game's own system.
    let system: GameSystem
    let folder: ThumbnailFolder
    /// The listing filename with .png dropped, which is the name the server files it under.
    let thumbnailName: String
    let url: URL
    /// True when this cover belongs to a different console than the game does.
    let isCrossSystem: Bool
    /// The listing title that matched, kept so the sheet can show WHY this cover is on offer for a
    /// game whose name is not quite the same.
    let matchedTitle: String

    var id: String { "\(system.rawValue)#\(folder.rawValue)#\(thumbnailName)" }

    /// What this is, in the words the rest of the app already uses: "box art", "title screen",
    /// "in-game shot", or "box art from Game Boy" when it is borrowed.
    ///
    /// Derived from `ThumbnailFolder.readableName` and `GameSystem.displayName` rather than restated,
    /// so the label on a thumbnail and the provenance recorded for a stored cover can never drift
    /// apart.
    var label: String {
        isCrossSystem ? "\(folder.readableName) from \(system.displayName)" : folder.readableName
    }
}

/// Building the offers from lists that have already been fetched. Pure, so the whole enumeration can
/// be run against saved copies of the real server listings without a device.
enum ArtworkOptions {
    /// Every option one game has across the lists it is handed, in the order they are handed over.
    ///
    /// EQUALITY AND ALIGNMENT BOTH COUNT HERE, which is the one place they are treated alike. An
    /// exact title is an option, and so is every title that aligns, including the ones
    /// `ArtworkIndexNames.uniqueAlignedMatch` refuses to auto-pick because there are several. The
    /// refusal is what keeps a wrong cover off a card; offering all of them here is what means the
    /// refusal costs a user nothing.
    ///
    /// Deduped by URL, because the same filename can be reached through the exact title and through
    /// an alignment, and because two systems can share a thumbnail name.
    static func enumerate(query: String,
                          ownSystem: GameSystem,
                          lists: [(system: GameSystem, folder: ThumbnailFolder,
                                   titles: [String: String])]) -> [ArtworkOption] {
        guard !query.isEmpty else { return [] }

        var out: [ArtworkOption] = []
        var seen = Set<String>()
        for list in lists {
            var matchedTitles: [String] = []
            if ArtworkIndexNames.match(title: query, in: list.titles) != nil {
                matchedTitles.append(query)
            }
            matchedTitles.append(contentsOf: ArtworkIndexNames.alignedTitles(for: query,
                                                                            in: list.titles))
            for title in matchedTitles {
                guard let filename = list.titles[title] else { continue }
                let thumbnailName = ArtworkNames.baseName(filename)
                guard let candidate = ArtworkNames.candidates(system: list.system,
                                                             thumbnailName: thumbnailName,
                                                             form: .indexed,
                                                             folders: [list.folder]).first else {
                    continue
                }
                let address = candidate.url.absoluteString
                guard !seen.contains(address) else { continue }
                seen.insert(address)
                out.append(
                    ArtworkOption(
                        system: list.system,
                        folder: list.folder,
                        thumbnailName: thumbnailName,
                        url: candidate.url,
                        isCrossSystem: list.system != ownSystem,
                        matchedTitle: title
                    )
                )
            }
        }
        return out
    }

    /// The same enumeration, OFF THE MAIN ACTOR.
    ///
    /// NOT A WRAPPER FOR TIDINESS. `enumerate` walks every key of up to twelve title maps, splitting
    /// each one into words, and the NES lists carry around ten thousand titles each. A nonisolated
    /// async function runs on the generic executor rather than on the caller's actor, which is the
    /// same reason `ArtworkDisk` and `ArtworkCoverLists` are nonisolated: that scan has no business
    /// happening inside the frame the sheet is animating in.
    static func enumerated(query: String, ownSystem: GameSystem,
                           lists: [(system: GameSystem, folder: ThumbnailFolder,
                                    titles: [String: String])]) async -> [ArtworkOption] {
        enumerate(query: query, ownSystem: ownSystem, lists: lists)
    }

    /// The one sentence under the row of covers.
    ///
    /// IT HAS TO READ CORRECTLY WITH ONE OFFER, because one is the commonest answer and is not an
    /// error state: no count, no "1 of 1", no empty-looking box. Pure and here rather than on the
    /// sheet's model so the harness runs the exact sentence a user sees.
    static func summary(for options: [ArtworkOption], searchedOtherSystems: Bool) -> String {
        let borrowed = options.filter { $0.isCrossSystem }.count
        switch options.count {
        case 0:
            return searchedOtherSystems
                ? "The server has no cover for this game under its own system or any other, so the "
                    + "generated plate is showing. An image from Files is the answer for a game the "
                    + "thumbnail database has never heard of."
                : "No cover was found for this game in its own system's lists. Looking in the other "
                    + "systems may still find one."
        case 1:
            return options[0].isCrossSystem
                ? "One cover exists for this game, and it is filed under "
                    + "\(options[0].system.displayName)."
                : "One cover exists for this game: \(options[0].label)."
        default:
            var text = "\(options.count) covers exist for this game"
            if borrowed > 0 {
                text += ", \(borrowed) of them from another system"
            }
            return text + ". The one in use is marked."
        }
    }
}

// MARK: - What is remembered

/// A cover the user chose, as it is persisted.
///
/// Keyed on `ArtworkDisk.key(forPath:)` exactly as the remembered misses and the provenance map are,
/// so every piece of per-entry artwork state shares one key space and is pruned in one place.
struct ArtworkChoice: Codable, Sendable, Equatable {
    /// Where the bytes came from, which decides what can be done if they ever go missing.
    enum Kind: String, Codable, Sendable {
        /// A thumbnail on the server. Can always be downloaded again.
        case remote
        /// An image the user picked out of Files. The stored copy is the ONLY copy.
        case pickedFile
    }

    let kind: Kind
    /// The option's stable id, for marking it as the one in use. Nil for a picked file, which has no
    /// option to point at.
    let optionID: String?
    /// The address to fetch again if the stored cover is ever lost. Nil for a picked file.
    let address: String?
    /// What to call it on screen: "box art from Game Boy", or the picked file's name.
    let label: String
    let chosenAt: Double

    /// The folder the chosen cover came from, recovered from the option id.
    ///
    /// PARSED RATHER THAN STORED TWICE, so the id and the folder can never disagree after a relaunch.
    /// Nil for a picked file, which has no option behind it. The id is
    /// "<system>#<folder>#<thumbnail name>" and a thumbnail name may itself contain a "#", so the
    /// split is limited to the first two separators.
    var optionFolder: ThumbnailFolder? {
        guard let parts = ArtworkChoice.idParts(optionID), parts.count >= 2 else { return nil }
        return ThumbnailFolder(rawValue: parts[1])
    }

    /// The system whose directory the chosen cover came from. Nil for a picked file.
    var optionSystem: GameSystem? {
        guard let parts = ArtworkChoice.idParts(optionID), parts.count >= 1 else { return nil }
        return GameSystem(rawValue: parts[0])
    }

    private static func idParts(_ optionID: String?) -> [String]? {
        guard let optionID, !optionID.isEmpty else { return nil }
        return optionID.split(separator: "#", maxSplits: 2, omittingEmptySubsequences: false)
            .map(String.init)
    }
}

/// The persisted map of choices, and nothing else. Pure, so the encoding a relaunch depends on can be
/// proven without a device.
///
/// One JSON blob under one key rather than a dictionary of strings, because a choice is five fields
/// and flattening it into a string would be a format to get wrong. A map that cannot be decoded comes
/// back empty rather than throwing: the covers themselves are still on disk, so the worst case is
/// that an automatic resolve is allowed to replace one, which is recoverable, while a crash on launch
/// is not.
enum ArtworkChoices {
    static func decode(_ data: Data?) -> [String: ArtworkChoice] {
        guard let data else { return [:] }
        return (try? JSONDecoder().decode([String: ArtworkChoice].self, from: data)) ?? [:]
    }

    static func encode(_ map: [String: ArtworkChoice]) -> Data? {
        try? JSONEncoder().encode(map)
    }
}

// MARK: - The sheet's model

/// The state behind the artwork section of one game's detail sheet.
///
/// OWNED BY THE SHEET, not by the store, and that is a deliberate split. The store owns the caches
/// and the persisted choice, because those outlive the sheet; this object owns what is being shown
/// right now, because publishing that from the store would invalidate every card in the library each
/// time one thumbnail arrived.
@MainActor
final class ArtworkChooserModel: ObservableObject {
    /// Every cover on offer, in the order the enumeration produced them.
    @Published private(set) var options: [ArtworkOption] = []
    /// The thumbnails that have arrived so far, by option id. Progressive on purpose: the sheet draws
    /// plates for the rest and never waits.
    @Published private(set) var thumbnails: [String: UIImage] = [:]
    /// True while the enumeration or a download is running, for the sheet's one spinner.
    @Published private(set) var isWorking = false
    /// One sentence about what just happened, always complete, never empty once anything has run.
    @Published private(set) var line = ""
    /// Whether the other systems have already been asked for, so the button can say so.
    @Published private(set) var searchedOtherSystems = false

    /// Set the first time `load` runs, so a body re-evaluation cannot re-enumerate.
    private var loaded = false
    /// Everything started here, so closing the sheet stops it. A thumbnail already in flight is left
    /// to finish and cache: it is cheaper than asking for it again.
    private var tasks: [Task<Void, Never>] = []

    /// Enumerates and starts resolving, once.
    func load(entry: LibraryEntry, system: GameSystem?, store: ArtworkStore) {
        guard !loaded else { return }
        loaded = true

        guard let system, SystemArtwork.hasThumbnails(for: system) else {
            line = "This app has no thumbnail directory for \(system?.displayName ?? "this format"), "
                + "so there is nothing to choose from. An image from Files still works."
            return
        }

        // What a previous open of this card found, straight away and for nothing. The run below still
        // happens, and every thumbnail it asks for is a cache hit, so reopening a card costs nothing
        // and shows the row immediately rather than after a round trip.
        if let cached = store.cachedArtworkOptions(for: entry) {
            options = cached
            line = ArtworkOptions.summary(for: cached, searchedOtherSystems: false)
        }

        // `guard let self` rather than `await self?.run(...)`. Optional chaining on an async call
        // makes the closure return `()?`, so the task is a `Task<()?, Never>` and does not fit
        // `tasks`, which is the error the offline parse check cannot see. The other task closures in
        // this file already unwrap first; these two now match them.
        let task = Task { [weak self] in
            guard let self else { return }
            await self.run(entry: entry, system: system, store: store,
                           includeCrossSystemDownloads: false)
        }
        tasks.append(task)
    }

    /// The explicit "look further" action: downloads the other systems' box art lists, cheapest
    /// first, and adds whatever they name.
    func searchOtherSystems(entry: LibraryEntry, system: GameSystem?, store: ArtworkStore) {
        guard let system, SystemArtwork.hasThumbnails(for: system) else { return }
        guard !isWorking else {
            line = "Still looking through the lists this device already has. Try again in a moment."
            return
        }
        searchedOtherSystems = true
        let task = Task { [weak self] in
            guard let self else { return }
            await self.run(entry: entry, system: system, store: store,
                           includeCrossSystemDownloads: true)
        }
        tasks.append(task)
    }

    /// Chooses one cover. The store does the downloading, the storing and the remembering.
    func choose(_ option: ArtworkOption, entry: LibraryEntry, store: ArtworkStore) {
        guard !isWorking else {
            line = "One cover at a time: \(option.label) was not used because something else is "
                + "still running."
            return
        }
        isWorking = true
        let task = Task { [weak self] in
            await store.chooseArtwork(option, for: entry)
            guard let self else { return }
            self.isWorking = false
            // WHICH COVER IS IN USE IS NOT MIRRORED HERE. The view asks the store, which is the one
            // thing that knows, so a cover picked from Files or cleared from Settings can never leave
            // this object holding a stale mark. The store bumps its generation, which is what redraws
            // the sheet along with every card.
            self.line = store.line
        }
        tasks.append(task)
    }

    /// Drops the choice and lets automatic resolution have the game back.
    func useAutomatic(entry: LibraryEntry, store: ArtworkStore) {
        store.clearArtworkChoice(for: entry)
        // Said here rather than copied from the store's line, because the store finishes the removal
        // in a task of its own and its line is still the previous sentence at this instant.
        line = "Back to automatic artwork for this game. It will be looked up again, and a cover can "
            + "be chosen here at any time."
    }

    /// Stops everything this sheet started. Called from the view's `onDisappear`, because a
    /// @MainActor class cannot cancel its own tasks from `deinit`.
    func stop() {
        for task in tasks {
            task.cancel()
        }
        tasks.removeAll()
    }

    private func run(entry: LibraryEntry, system: GameSystem, store: ArtworkStore,
                     includeCrossSystemDownloads: Bool) async {
        isWorking = true
        let found = await store.enumerateArtworkOptions(
            for: entry,
            system: system,
            includeCrossSystemDownloads: includeCrossSystemDownloads
        )
        guard !Task.isCancelled else {
            isWorking = false
            return
        }
        options = found
        line = ArtworkOptions.summary(for: found, searchedOtherSystems: searchedOtherSystems)
        isWorking = false

        // PROGRESSIVE, AND CAPPED BY THE GATE THE REST OF THE APP USES. One child task per option,
        // each publishing the moment its own image lands, so the sheet is usable while the rest
        // arrive and a slow one never holds up a fast one. `ArtworkGate` inside the store keeps the
        // whole app to three concurrent lookups, so this cannot starve the cards behind the sheet.
        for option in found {
            let task = Task { [weak self] in
                guard let image = await store.artworkOptionThumbnail(option) else { return }
                guard let self, !Task.isCancelled else { return }
                self.thumbnails[option.id] = image
            }
            tasks.append(task)
        }
    }

}

// MARK: - The row of covers

/// One selectable cover: its thumbnail over the game's plate, its label, and a mark when it is the
/// one in use.
///
/// The plate is drawn underneath for exactly the reason it is everywhere else in this app: a
/// thumbnail that has not arrived, or never will, must not leave a hole.
struct ArtworkOptionCell: View {
    let entry: LibraryEntry
    let option: ArtworkOption
    let image: UIImage?
    let isInUse: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(alignment: .leading, spacing: 5) {
                ZStack {
                    ArtPlate(entry: entry, system: option.system, showsCaption: false)
                    if let image {
                        Image(uiImage: image)
                            .resizable()
                            .aspectRatio(contentMode: .fill)
                    }
                }
                .frame(width: 74, height: 99)
                .clipped()
                .clipShape(RoundedRectangle(cornerRadius: 8))
                .overlay(
                    RoundedRectangle(cornerRadius: 8)
                        .strokeBorder(isInUse ? ShellPalette.accent : Color.white.opacity(0.12),
                                      lineWidth: isInUse ? 2 : 0.5)
                )
                .overlay(alignment: .topTrailing) {
                    if isInUse {
                        Image(systemName: "checkmark.circle.fill")
                            .font(.system(size: 14, weight: .bold))
                            .foregroundStyle(ShellPalette.accent)
                            .background(.black.opacity(0.6), in: Circle())
                            .padding(4)
                    }
                }

                Text(option.label)
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(isInUse ? ShellPalette.accent : Color.white.opacity(0.85))
                    .lineLimit(2)
                    .multilineTextAlignment(.leading)
                    .frame(width: 74, alignment: .leading)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .buttonStyle(.plain)
        .accessibilityLabel(isInUse ? "\(option.label), in use" : "Use \(option.label)")
    }
}
