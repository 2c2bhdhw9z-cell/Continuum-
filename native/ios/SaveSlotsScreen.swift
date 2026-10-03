// Continuum - the 50-slot save manager, and the two pieces of UIKit that move files in and out of
// the app: the share sheet and the document picker.
//
// THE STORE DOES EVERYTHING; THIS FILE ONLY ASKS. Every save, load, overwrite, rename, delete,
// export and import is a call into `SaveStates`, which keeps the compatibility gate and the
// synchronous payload write exactly as they were. Nothing here touches a payload.
//
// The grid is the same on both screens that open it: the player (where saving and loading happen)
// and a game's card (where a game that is not running can still have its slots managed, exported
// and imported, and a slot can be launched straight into).

import SwiftUI
import UIKit
import UniformTypeIdentifiers

// MARK: - Files out

/// Hands a file to the system share sheet, which is also the way to "Save to Files".
@MainActor
enum FileShare {
    static func present(_ url: URL) {
        guard let presenter = EngineHost.topmostViewController() else { return }
        let sheet = UIActivityViewController(activityItems: [url], applicationActivities: nil)
        // iPad needs an anchor or it traps; the centre of the presenting view is honest enough.
        if let popover = sheet.popoverPresentationController {
            popover.sourceView = presenter.view
            popover.sourceRect = CGRect(x: presenter.view.bounds.midX,
                                        y: presenter.view.bounds.midY, width: 1, height: 1)
            popover.permittedArrowDirections = []
        }
        presenter.present(sheet, animated: true)
    }
}

// MARK: - Files in

/// A document picker that reads the picked file immediately and hands back its bytes and name on
/// the main actor.
///
/// UIKit rather than `.fileImporter`, for the reason ROM, artwork and skin import already give:
/// `.fileImporter` was proven on device not to call its completion. The bytes are read INSIDE the
/// delegate callback because an `asCopy` pick lives in a temporary inbox the system may clear.
/// Retained by its owner (`FilePicker.shared`), because the picker's delegate is weak.
final class FilePicker: NSObject, UIDocumentPickerDelegate,
                        UIAdaptivePresentationControllerDelegate {
    static let shared = FilePicker()

    private var activePicker: UIDocumentPickerViewController?
    private var onPick: ((Data, String) -> Void)?
    private var onFailure: ((String) -> Void)?

    /// Opens the picker. Every content type is offered, because iOS has no type for `.srm`,
    /// `.cht` or `.continuumstate`, and a picker built from types it does not know greys the file
    /// out. The store checks what it was given.
    @MainActor
    func present(onPick: @escaping (Data, String) -> Void,
                 onFailure: @escaping (String) -> Void) {
        guard let presenter = EngineHost.topmostViewController() else {
            onFailure("there is no screen to show the file picker on")
            return
        }
        self.onPick = onPick
        self.onFailure = onFailure
        let picker = UIDocumentPickerViewController(forOpeningContentTypes: [UTType.item],
                                                    asCopy: true)
        picker.delegate = self
        picker.allowsMultipleSelection = false
        picker.shouldShowFileExtensions = true
        picker.presentationController?.delegate = self
        activePicker = picker
        presenter.present(picker, animated: true)
    }

    func documentPicker(_ controller: UIDocumentPickerViewController,
                        didPickDocumentsAt urls: [URL]) {
        guard let url = urls.first else {
            settle { $1?("no file was picked") }
            return
        }
        let name = url.lastPathComponent
        do {
            let data = try Data(contentsOf: url)
            settle { pick, _ in pick?(data, name) }
        } catch {
            let reason = "\(name) could not be read: \(error.localizedDescription)"
            settle { $1?(reason) }
        }
    }

    func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
        settle { _, _ in }
    }

    func presentationControllerDidDismiss(_ presentationController: UIPresentationController) {
        settle { _, _ in }
    }

    /// Delivers once on the main actor and forgets the callbacks.
    private func settle(
        _ deliver: @escaping (((Data, String) -> Void)?, ((String) -> Void)?) -> Void
    ) {
        Task { @MainActor in
            let pick = self.onPick
            let failure = self.onFailure
            self.onPick = nil
            self.onFailure = nil
            self.activePicker = nil
            deliver(pick, failure)
        }
    }
}

// MARK: - The grid

/// Every slot for one game, the auto-save above them, and the extras below.
struct SaveSlotsSheet: View {
    let entry: LibraryEntry
    @ObservedObject var host: EngineHost
    @ObservedObject var saveStates: SaveStates
    /// Called after a pick that should close the sheet (loading a state into the running game, or
    /// launching into one from the library).
    var onDone: () -> Void

    /// The filled slot whose actions are showing.
    @State private var chosen: SaveStateRecord?
    /// The empty or filled slot about to be saved over, waiting for the confirm.
    @State private var overwriteSlot: Int?
    @State private var deleting: SaveStateRecord?
    @State private var renaming: SaveStateRecord?
    @State private var draftName = ""
    /// The line under the header: the store's own read-out, which every action writes.
    private var line: String { saveStates.line }

    private var gameId: String { SaveStates.gameId(for: entry) }
    private var isRunningThisGame: Bool { host.running && host.activeEntry?.id == entry.id }

    private let columns = [GridItem(.adaptive(minimum: 150), spacing: 10)]

    var body: some View {
        NavigationView {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    Text(line)
                        .font(.system(.caption2, design: .monospaced))
                        .foregroundStyle(ShellPalette.secondaryText)
                        .fixedSize(horizontal: false, vertical: true)
                    autoRow
                    // Read so the grid redraws when a thumbnail lands.
                    let _ = saveStates.thumbnailGeneration
                    LazyVGrid(columns: columns, spacing: 10) {
                        ForEach(1...SaveStates.slotCount, id: \.self) { slot in
                            slotCell(slot)
                        }
                    }
                    extras
                    transferBlock
                }
                .padding(14)
            }
            .background(Color.black.ignoresSafeArea())
            .navigationTitle("Save slots")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { onDone() }
                }
            }
        }
        .navigationViewStyle(.stack)
        .preferredColorScheme(.dark)
        .confirmationDialog(chosen?.displayName ?? "", isPresented: Binding(
            get: { chosen != nil }, set: { if !$0 { chosen = nil } }
        ), titleVisibility: .visible) {
            if let record = chosen { actions(for: record) }
        } message: {
            if let record = chosen {
                Text("\(record.dateText), \(record.coreLine), \(record.sizeText)")
            }
        }
        .alert("Save over this slot?", isPresented: Binding(
            get: { overwriteSlot != nil }, set: { if !$0 { overwriteSlot = nil } }
        )) {
            Button("Overwrite", role: .destructive) {
                if let slot = overwriteSlot { saveStates.saveToSlot(slot) }
                overwriteSlot = nil
            }
            Button("Cancel", role: .cancel) { overwriteSlot = nil }
        } message: {
            Text("The state already in this slot is replaced by the game as it is now.")
        }
        .alert("Delete this state?", isPresented: Binding(
            get: { deleting != nil }, set: { if !$0 { deleting = nil } }
        )) {
            Button("Delete", role: .destructive) {
                if let record = deleting { saveStates.delete(record) }
                deleting = nil
            }
            Button("Cancel", role: .cancel) { deleting = nil }
        } message: {
            Text(deleting.map { "\($0.displayName) is deleted from this device." } ?? "")
        }
        .alert("Name this state", isPresented: Binding(
            get: { renaming != nil }, set: { if !$0 { renaming = nil } }
        )) {
            TextField("Before the boss", text: $draftName)
            Button("Save") {
                if let record = renaming { saveStates.rename(record, to: draftName) }
                renaming = nil
            }
            Button("Cancel", role: .cancel) { renaming = nil }
        }
    }

    // MARK: Rows

    @ViewBuilder
    private var autoRow: some View {
        if let auto = saveStates.autoState(forGameId: gameId) {
            Button {
                chosen = auto
            } label: {
                HStack {
                    Image(systemName: "clock.arrow.circlepath")
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Auto-save")
                            .font(.system(size: 14, weight: .semibold))
                        Text("\(auto.dateText) \u{00B7} \(auto.coreLine)")
                            .font(.system(.caption2, design: .monospaced))
                            .foregroundStyle(ShellPalette.secondaryText)
                    }
                    Spacer()
                }
                .foregroundStyle(.white)
                .padding(12)
                .background(ShellPalette.surface, in: RoundedRectangle(cornerRadius: 10))
            }
            .buttonStyle(.plain)
        }
    }

    private func slotCell(_ slot: Int) -> some View {
        let record = saveStates.manualState(forGameId: gameId, slot: slot)
        return Button {
            if let record {
                chosen = record
            } else if isRunningThisGame {
                saveStates.saveToSlot(slot)
            } else {
                saveStates.reportOnly("slot \(slot) is empty; start the game to save into it")
            }
        } label: {
            VStack(alignment: .leading, spacing: 4) {
                ZStack {
                    RoundedRectangle(cornerRadius: 8).fill(Color.white.opacity(0.06))
                    if let record, let image = saveStates.thumbnail(for: record) {
                        Image(uiImage: image)
                            .resizable()
                            .interpolation(.none)
                            .aspectRatio(contentMode: .fit)
                    } else if record != nil {
                        Image(systemName: "photo")
                            .foregroundStyle(ShellPalette.secondaryText)
                    } else {
                        Text(isRunningThisGame ? "Tap to save" : "Empty")
                            .font(.system(size: 12))
                            .foregroundStyle(ShellPalette.secondaryText)
                    }
                }
                .frame(height: 96)
                .clipShape(RoundedRectangle(cornerRadius: 8))
                Text(record?.displayName ?? "Slot \(slot)")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(.white)
                    .lineLimit(1)
                if let record {
                    Text(record.dateText)
                        .font(.system(size: 11))
                        .foregroundStyle(ShellPalette.secondaryText)
                        .lineLimit(1)
                    Text(record.coreLine)
                        .font(.system(.caption2, design: .monospaced))
                        .foregroundStyle(ShellPalette.secondaryText)
                        .lineLimit(1)
                    if !saveStates.isStored(record) {
                        Text("its file is no longer stored")
                            .font(.system(size: 11))
                            .foregroundStyle(ShellPalette.accent)
                    }
                }
            }
            .padding(8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(ShellPalette.surface, in: RoundedRectangle(cornerRadius: 10))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(record?.displayName ?? "Empty slot \(slot)")
    }

    /// The actions for a filled slot. Load and overwrite only make sense with the game running;
    /// from the library, "Play from here" launches the game into the state.
    @ViewBuilder
    private func actions(for record: SaveStateRecord) -> some View {
        if isRunningThisGame {
            Button("Load") {
                if saveStates.load(record) { onDone() }
            }
            if !record.isAuto {
                Button("Save over it") { overwriteSlot = record.slot }
            }
        } else {
            Button("Play from here") {
                let target = entry
                onDone()
                host.detailEntry = nil
                host.launchAndLoad(entry: target, record: record)
            }
        }
        Button("Rename") {
            draftName = record.label
            renaming = record
        }
        Button("Export") {
            if let url = saveStates.exportFile(for: record) { FileShare.present(url) }
        }
        Button("Delete", role: .destructive) { deleting = record }
        Button("Cancel", role: .cancel) {}
    }

    @ViewBuilder
    private var extras: some View {
        let overflow = saveStates.overflowStates(forGameId: gameId)
        if !overflow.isEmpty {
            VStack(alignment: .leading, spacing: 8) {
                Text("MORE STATES")
                    .font(.system(size: 11, weight: .bold))
                    .tracking(1.6)
                    .foregroundStyle(ShellPalette.secondaryText)
                SettingsNote(
                    "This game had more than \(SaveStates.slotCount) states before slots had a "
                    + "fixed number, so these are kept here rather than deleted. They load, export "
                    + "and delete like any other; free a slot to have the next save go there."
                )
                ForEach(overflow) { record in
                    Button { chosen = record } label: {
                        HStack {
                            Text(record.summaryLine)
                                .font(.system(.caption2, design: .monospaced))
                                .foregroundStyle(.white)
                            Spacer()
                        }
                        .padding(10)
                        .background(ShellPalette.surface, in: RoundedRectangle(cornerRadius: 8))
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }

    // MARK: Files

    private var transferBlock: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("FILES")
                .font(.system(size: 11, weight: .bold))
                .tracking(1.6)
                .foregroundStyle(ShellPalette.secondaryText)
            SettingsButton(title: "Import a state file", role: .normal) {
                FilePicker.shared.present(onPick: { data, name in
                    _ = saveStates.importState(data, named: name, intoGameId: gameId)
                }, onFailure: { reason in
                    saveStates.reportOnly("import failed: \(reason)")
                })
            }
            SettingsButton(title: "Export the battery save (.srm)", role: .normal) {
                if let url = saveStates.exportBatteryFile(for: entry) { FileShare.present(url) }
            }
            SettingsButton(title: "Import a battery save (.srm)", role: .normal) {
                FilePicker.shared.present(onPick: { data, name in
                    _ = saveStates.importBatterySave(data, named: name, for: entry)
                }, onFailure: { reason in
                    saveStates.reportOnly("battery save import failed: \(reason)")
                })
            }
            SettingsNote(
                "A state file carries the core and core version that wrote it, so an imported state "
                + "is checked exactly like one saved here and is refused with the reason if the core "
                + "differs. The battery save is the game's own save, the one made from its menu; it "
                + "is the .srm file other emulators use, and it is kept automatically whenever you "
                + "leave the game."
            )
        }
        .padding(14)
        .background(ShellPalette.surface, in: RoundedRectangle(cornerRadius: 12))
    }
}
