// Continuum - Delta .deltaskin package import for on-screen control layouts.
//
// A .deltaskin is a ZIP with a flat info.json (Delta Custom Skins docs). This file:
//   1. Opens a document picker for .deltaskin / .zip / bare info.json
//   2. Extracts and parses info.json
//   3. Maps item frames into a TouchLayout (buttonFrees + cluster centres)
//   4. Loads PDF/PNG assets and `screens` outputFrame for the game picture
//   5. The editor stores layout + art under the skin's GameSystem (per-console),
//      so a GBA skin does not overwrite PS1 button positions.
//
// Cancelling or picking nothing fails with a clear message on the layout editor
// panel — not a silent no-op.

import Foundation
import zlib
import UniformTypeIdentifiers
import UIKit
import CoreGraphics

// MARK: - Screen / art payloads

/// A rectangle in mappingSize space, stored as fractions of that size (origin top-left).
struct DeltaSkinNormalizedRect: Codable, Equatable, Sendable {
    var x: Double
    var y: Double
    var width: Double
    var height: Double

    /// Maps this fraction rect onto a view that fills the same mapping aspect.
    func cgRect(in bounds: CGRect) -> CGRect {
        CGRect(
            x: bounds.minX + CGFloat(x) * bounds.width,
            y: bounds.minY + CGFloat(y) * bounds.height,
            width: CGFloat(width) * bounds.width,
            height: CGFloat(height) * bounds.height
        )
    }

    /// Letterboxes `mapping` inside `bounds`. The skin's own aspect is kept, so a portrait
    /// hole rotated onto a wide phone stays a portrait hole instead of being stretched.
    static func aspectFitCanvas(mapping: CGSize, in bounds: CGRect) -> CGRect {
        guard mapping.width > 0, mapping.height > 0,
              bounds.width > 0, bounds.height > 0 else { return bounds }
        let scale = min(bounds.width / mapping.width, bounds.height / mapping.height)
        let width = mapping.width * scale
        let height = mapping.height * scale
        return CGRect(
            x: bounds.midX - width / 2,
            y: bounds.midY - height / 2,
            width: width,
            height: height
        )
    }

    /// The screen hole in view space, aspect-fit, never stretched to `bounds`.
    func placed(mapping: CGSize, in bounds: CGRect) -> CGRect {
        cgRect(in: Self.aspectFitCanvas(mapping: mapping, in: bounds))
    }

    static func from(frame: CGRect, mappingSize: CGSize) -> DeltaSkinNormalizedRect? {
        guard mappingSize.width > 0, mappingSize.height > 0,
              frame.width > 0, frame.height > 0 else { return nil }
        return DeltaSkinNormalizedRect(
            x: Double(frame.minX / mappingSize.width),
            y: Double(frame.minY / mappingSize.height),
            width: Double(frame.width / mappingSize.width),
            height: Double(frame.height / mappingSize.height)
        )
    }
}

/// Decoded skin artwork bytes plus how to turn them into a UIImage.
enum DeltaSkinAssetKind: String, Codable, Sendable {
    case pdf
    case png
}

/// One orientation of an imported skin: its own mapping, hole, art, and button layout.
struct DeltaSkinFace: Codable, Equatable, Sendable {
    var mappingWidth: Double
    var mappingHeight: Double
    var screenOutput: DeltaSkinNormalizedRect?
    var assetFileName: String?
    var assetKind: DeltaSkinAssetKind?
    var layout: TouchLayout
}

/// Artwork + screen hole from one imported representation (persisted per console).
struct DeltaSkinVisual: Codable, Equatable, Sendable {
    var skinName: String
    var translucent: Bool
    var mappingWidth: Double
    var mappingHeight: Double
    /// First `screens[].outputFrame`, as fractions of mappingSize. Nil keeps Continuum's free band.
    var screenOutput: DeltaSkinNormalizedRect?
    var assetFileName: String?
    var assetKind: DeltaSkinAssetKind?
    /// Landscape representation, when the package had one. Nil on skins imported before both
    /// orientations were kept, and on packages that only ship portrait.
    var landscape: DeltaSkinFace?
}

/// What importing one Delta skin package produced.
struct DeltaSkinImportResult: Sendable {
    /// Layout positions taken from the chosen representation's `items`.
    let layout: TouchLayout
    /// Skin display name from info.json, when present.
    let skinName: String
    /// Continuum preview system inferred from `gameTypeIdentifier`, if recognised.
    let previewSystem: GameSystem?
    /// Short line for the editor panel (what was read, what was skipped).
    let summary: String
    /// Metadata for persistence (screens + asset names). Always set on success.
    let visual: DeltaSkinVisual
    /// Raw PDF or PNG bytes from the package, when an asset file was present and readable.
    let assetData: Data?
    /// Landscape art bytes, when that representation named a file that was in the package.
    let landscapeAssetData: Data?
}

/// Why a skin import did not produce a layout.
enum DeltaSkinImportError: LocalizedError, Equatable {
    case noFileSelected
    case cancelled
    case unreadable(String)
    case missingInfoJSON
    case invalidJSON(String)
    case noUsableRepresentation
    case noMappableItems

    var errorDescription: String? {
        switch self {
        case .noFileSelected:
            return "No skin selected. Pick a .deltaskin, .zip, or info.json."
        case .cancelled:
            return "Import cancelled — no skin selected."
        case .unreadable(let detail):
            return "Could not read the skin package: \(detail)"
        case .missingInfoJSON:
            return "No info.json in that package. A .deltaskin must contain a flat info.json."
        case .invalidJSON(let detail):
            return "info.json is not valid: \(detail)"
        case .noUsableRepresentation:
            return "info.json has no representation with mappingSize and items."
        case .noMappableItems:
            return "That skin's items use no button names Continuum maps yet."
        }
    }
}

// MARK: - Parse entry points

enum DeltaSkinImporter {

    /// Reads a picked file URL (app-owned copy from the document picker) into a layout + art.
    static func importPackage(at url: URL) throws -> DeltaSkinImportResult {
        let package = try loadPackageBytes(from: url)
        return try parseInfoJSON(
            package.infoJSON,
            sourceName: url.lastPathComponent,
            assetLookup: package.assetLookup
        )
    }

    /// Same parse path for tests and for a bare info.json drop (no ZIP assets).
    static func parseInfoJSON(_ data: Data, sourceName: String) throws -> DeltaSkinImportResult {
        try parseInfoJSON(data, sourceName: sourceName, assetLookup: { _ in nil })
    }

    /// Parse `info.json` and optionally pull named assets through `assetLookup`.
    static func parseInfoJSON(
        _ data: Data,
        sourceName: String,
        assetLookup: (String) -> Data?
    ) throws -> DeltaSkinImportResult {
        let root: [String: Any]
        do {
            let object = try JSONSerialization.jsonObject(with: data, options: [])
            guard let dict = object as? [String: Any] else {
                throw DeltaSkinImportError.invalidJSON("top level is not an object")
            }
            root = dict
        } catch let error as DeltaSkinImportError {
            throw error
        } catch {
            throw DeltaSkinImportError.invalidJSON(error.localizedDescription)
        }

        let name = (root["name"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines)
            ?? "Untitled skin"
        let gameType = root["gameTypeIdentifier"] as? String
        let preview = GameSystem.fromDeltaGameType(gameType)

        guard let representations = root["representations"] as? [String: Any] else {
            throw DeltaSkinImportError.noUsableRepresentation
        }
        let orientations = pickOrientations(from: representations)
        // Portrait is the editor's stored layout. Landscape is kept beside it and swapped
        // in when the phone is wider than it is tall. A package with only one orientation
        // still imports; the missing half is not invented.
        guard let chosen = orientations.portrait ?? orientations.landscape else {
            throw DeltaSkinImportError.noUsableRepresentation
        }
        let landscapeChoice = orientations.portrait == nil ? nil : orientations.landscape

        let mapping = chosen.mappingSize
        guard mapping.width > 0, mapping.height > 0 else {
            throw DeltaSkinImportError.noUsableRepresentation
        }
        guard !chosen.items.isEmpty else {
            throw DeltaSkinImportError.noUsableRepresentation
        }

        let mapped = mapItems(chosen.items, mappingSize: mapping)
        guard mapped.appliedCount > 0 else {
            throw DeltaSkinImportError.noMappableItems
        }

        let screenOutput = firstScreenOutput(from: chosen.screens, mappingSize: mapping)
        let assetPick = pickAssetFileName(from: chosen.assets)
        var assetData: Data?
        var assetKind: DeltaSkinAssetKind?
        if let assetPick {
            if let bytes = assetLookup(assetPick.name), !bytes.isEmpty {
                assetData = bytes
                assetKind = assetPick.kind
            }
        }

        var landscapeFace: DeltaSkinFace?
        var landscapeAssetData: Data?
        if let landscapeChoice {
            let built = buildFace(landscapeChoice, assetLookup: assetLookup)
            if built.applied > 0 {
                landscapeFace = built.face
                landscapeAssetData = built.assetData
            }
        }

        var summaryBits = [
            "\(name)",
            "via \(sourceName)",
            "\(chosen.path)",
            "\(mapped.appliedCount) control(s)",
        ]
        if !mapped.skipped.isEmpty {
            let skipList = mapped.skipped.prefix(4).joined(separator: ", ")
            let more = mapped.skipped.count > 4 ? "…" : ""
            summaryBits.append("skipped \(skipList)\(more)")
        }
        if screenOutput != nil {
            summaryBits.append("game screen frame applied")
        } else {
            summaryBits.append("no screens[] — picture keeps free band")
        }
        if landscapeFace != nil {
            summaryBits.append("landscape kept")
            if landscapeFace?.screenOutput != nil {
                summaryBits.append("landscape screen hole")
            }
        }
        if assetData != nil, let assetPick {
            summaryBits.append("art \(assetPick.name)")
        } else if assetPick != nil {
            summaryBits.append("art file missing in package")
        } else {
            summaryBits.append("no assets[] art")
        }

        let visual = DeltaSkinVisual(
            skinName: name,
            translucent: chosen.translucent,
            mappingWidth: Double(mapping.width),
            mappingHeight: Double(mapping.height),
            screenOutput: screenOutput,
            assetFileName: assetPick?.name,
            assetKind: assetKind,
            landscape: landscapeFace
        )

        return DeltaSkinImportResult(
            layout: mapped.layout.sanitised,
            skinName: name,
            previewSystem: preview,
            summary: summaryBits.joined(separator: " · "),
            visual: visual,
            assetData: assetData,
            landscapeAssetData: landscapeAssetData
        )
    }

    // MARK: Package bytes

    private struct LoadedPackage {
        let infoJSON: Data
        let assetLookup: (String) -> Data?
    }

    private static func loadPackageBytes(from url: URL) throws -> LoadedPackage {
        let ext = url.pathExtension.lowercased()
        if ext == "json" || url.lastPathComponent.lowercased() == "info.json" {
            let data: Data
            do {
                data = try Data(contentsOf: url)
            } catch {
                throw DeltaSkinImportError.unreadable(error.localizedDescription)
            }
            return LoadedPackage(infoJSON: data, assetLookup: { _ in nil })
        }

        if ext == "deltaskin" || ext == "zip" {
            let bytes: Data
            do {
                bytes = try Data(contentsOf: url)
            } catch {
                throw DeltaSkinImportError.unreadable(error.localizedDescription)
            }
            let info: Data
            if let flat = try ZipStore.data(forEntryNamed: "info.json", in: bytes) {
                info = flat
            } else if let nested = try ZipStore.dataMatchingInfoJSON(in: bytes) {
                info = nested
            } else {
                throw DeltaSkinImportError.missingInfoJSON
            }
            return LoadedPackage(infoJSON: info) { name in
                (try? ZipStore.data(forEntryNamed: name, in: bytes))
                    ?? (try? ZipStore.dataMatchingFileName(name, in: bytes))
            }
        }

        // Last resort: treat the bytes as JSON (Files sometimes strips extensions).
        do {
            let data = try Data(contentsOf: url)
            if (try? JSONSerialization.jsonObject(with: data)) is [String: Any] {
                return LoadedPackage(infoJSON: data, assetLookup: { _ in nil })
            }
        } catch {
            throw DeltaSkinImportError.unreadable(error.localizedDescription)
        }
        throw DeltaSkinImportError.unreadable(
            "expected a .deltaskin, .zip, or info.json (got .\(ext.isEmpty ? "unknown" : ext))"
        )
    }

    // MARK: Representation pick

    private struct ChosenOrientation {
        let path: String
        let mappingSize: CGSize
        let items: [[String: Any]]
        let screens: [[String: Any]]
        let assets: [String: Any]
        let translucent: Bool
    }

    private struct OrientationPick {
        var portrait: ChosenOrientation?
        var landscape: ChosenOrientation?
    }

    /// Best portrait and best landscape, independently. iPhone edge-to-edge wins over
    /// standard, which wins over iPad. Finding portrait does not throw landscape away.
    private static func pickOrientations(from representations: [String: Any]) -> OrientationPick {
        let deviceOrder = ["iphone", "ipad"]
        let sizeOrder = ["edgeToEdge", "standard", "splitView"]
        var bestPortraitRank = Int.max
        var bestLandscapeRank = Int.max
        var picked = OrientationPick()

        for (deviceIndex, device) in deviceOrder.enumerated() {
            guard let deviceNode = representations[device] as? [String: Any] else { continue }
            for (sizeIndex, size) in sizeOrder.enumerated() {
                guard let sizeNode = deviceNode[size] as? [String: Any] else { continue }
                let rank = deviceIndex * 10 + sizeIndex
                for orientation in ["portrait", "landscape"] {
                    guard let node = sizeNode[orientation] as? [String: Any],
                          let mapping = readSize(node["mappingSize"]),
                          let items = node["items"] as? [[String: Any]],
                          !items.isEmpty else { continue }
                    let chosen = ChosenOrientation(
                        path: "\(device)/\(size)/\(orientation)",
                        mappingSize: mapping,
                        items: items,
                        screens: node["screens"] as? [[String: Any]] ?? [],
                        assets: node["assets"] as? [String: Any] ?? [:],
                        translucent: (node["translucent"] as? Bool) ?? false
                    )
                    if orientation == "portrait", rank < bestPortraitRank {
                        picked.portrait = chosen
                        bestPortraitRank = rank
                    }
                    if orientation == "landscape", rank < bestLandscapeRank {
                        picked.landscape = chosen
                        bestLandscapeRank = rank
                    }
                }
            }
        }
        return picked
    }

    private struct BuiltFace {
        var face: DeltaSkinFace
        var assetData: Data?
        var applied: Int
    }

    private static func buildFace(
        _ chosen: ChosenOrientation,
        assetLookup: (String) -> Data?
    ) -> BuiltFace {
        let mapping = chosen.mappingSize
        let mapped = mapItems(chosen.items, mappingSize: mapping)
        let assetPick = pickAssetFileName(from: chosen.assets)
        var assetData: Data?
        var assetKind: DeltaSkinAssetKind?
        if let assetPick, let bytes = assetLookup(assetPick.name), !bytes.isEmpty {
            assetData = bytes
            assetKind = assetPick.kind
        }
        let face = DeltaSkinFace(
            mappingWidth: Double(mapping.width),
            mappingHeight: Double(mapping.height),
            screenOutput: firstScreenOutput(from: chosen.screens, mappingSize: mapping),
            assetFileName: assetData == nil ? nil : assetPick?.name,
            assetKind: assetKind,
            layout: mapped.layout.sanitised
        )
        return BuiltFace(face: face, assetData: assetData, applied: mapped.appliedCount)
    }

    private static func firstScreenOutput(
        from screens: [[String: Any]],
        mappingSize: CGSize
    ) -> DeltaSkinNormalizedRect? {
        for screen in screens {
            guard let frame = readFrame(screen["outputFrame"]),
                  let normalized = DeltaSkinNormalizedRect.from(frame: frame, mappingSize: mappingSize)
            else { continue }
            return normalized
        }
        return nil
    }

    private struct AssetPick {
        let name: String
        let kind: DeltaSkinAssetKind
    }

    /// Prefer Delta's resizable PDF; otherwise medium → large → small PNG.
    private static func pickAssetFileName(from assets: [String: Any]) -> AssetPick? {
        func kind(for name: String) -> DeltaSkinAssetKind {
            name.lowercased().hasSuffix(".pdf") ? .pdf : .png
        }
        if let name = (assets["resizable"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines),
           !name.isEmpty {
            return AssetPick(name: name, kind: kind(for: name))
        }
        for key in ["medium", "large", "small"] {
            if let name = (assets[key] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines),
               !name.isEmpty {
                return AssetPick(name: name, kind: kind(for: name))
            }
        }
        return nil
    }

    /// Renders imported asset bytes into a UIImage sized to the mapping (points).
    static func makeUIImage(from data: Data, kind: DeltaSkinAssetKind, mappingSize: CGSize) -> UIImage? {
        switch kind {
        case .png:
            return UIImage(data: data)
        case .pdf:
            return renderPDF(data, pointSize: mappingSize)
        }
    }

    private static func renderPDF(_ data: Data, pointSize: CGSize) -> UIImage? {
        guard pointSize.width > 0, pointSize.height > 0,
              let provider = CGDataProvider(data: data as CFData),
              let document = CGPDFDocument(provider),
              let page = document.page(at: 1) else { return nil }
        let format = UIGraphicsImageRendererFormat.default()
        format.opaque = false
        format.scale = UIScreen.main.scale
        let renderer = UIGraphicsImageRenderer(size: pointSize, format: format)
        return renderer.image { ctx in
            UIColor.clear.setFill()
            ctx.fill(CGRect(origin: .zero, size: pointSize))
            let cg = ctx.cgContext
            cg.saveGState()
            cg.translateBy(x: 0, y: pointSize.height)
            cg.scaleBy(x: 1, y: -1)
            let media = page.getBoxRect(.mediaBox)
            guard media.width > 0, media.height > 0 else {
                cg.restoreGState()
                return
            }
            cg.scaleBy(x: pointSize.width / media.width, y: pointSize.height / media.height)
            cg.drawPDFPage(page)
            cg.restoreGState()
        }
    }

    private static func readSize(_ value: Any?) -> CGSize? {
        guard let box = value as? [String: Any] else { return nil }
        let width = number(box["width"])
        let height = number(box["height"])
        guard width > 0, height > 0 else { return nil }
        return CGSize(width: width, height: height)
    }

    private static func number(_ value: Any?) -> CGFloat {
        if let n = value as? NSNumber { return CGFloat(truncating: n) }
        if let d = value as? Double { return CGFloat(d) }
        if let i = value as? Int { return CGFloat(i) }
        if let s = value as? String, let d = Double(s) { return CGFloat(d) }
        return 0
    }

    // MARK: Items → TouchLayout

    private struct MappedItems {
        let layout: TouchLayout
        let appliedCount: Int
        let skipped: [String]
    }

    private static func mapItems(_ items: [[String: Any]], mappingSize: CGSize) -> MappedItems {
        var layout = TouchLayout.standard
        var frees: [String: ButtonFree] = [:]
        var applied = 0
        var skipped: [String] = []

        for item in items {
            guard let frame = readFrame(item["frame"]) else {
                skipped.append("(item missing frame)")
                continue
            }
            let centreX = Double((frame.midX) / mappingSize.width)
            let centreY = Double((frame.midY) / mappingSize.height)

            if let directions = item["inputs"] as? [String: Any], isDirectional(directions) {
                layout.dpadX = centreX
                layout.dpadY = centreY
                applied += 1
                continue
            }

            let names = inputNames(from: item["inputs"])
            guard let first = names.first else {
                skipped.append("(empty inputs)")
                continue
            }

            if let slot = PadSlot.fromDeltaInput(first) {
                switch slot {
                case .select:
                    layout.selectX = centreX
                    layout.selectY = centreY
                case .start:
                    layout.startX = centreX
                    layout.startY = centreY
                case .up, .down, .left, .right:
                    // Lone direction chips are unusual in Delta skins; treat as D-pad centre.
                    layout.dpadX = centreX
                    layout.dpadY = centreY
                default:
                    frees[slot.layoutKey] = ButtonFree(x: centreX, y: centreY)
                }
                applied += 1
            } else {
                skipped.append(first)
            }
        }

        layout.buttonFrees = frees
        return MappedItems(layout: layout, appliedCount: applied, skipped: skipped)
    }

    private static func isDirectional(_ inputs: [String: Any]) -> Bool {
        let keys = Set(inputs.keys.map { $0.lowercased() })
        return keys.contains("up") && keys.contains("down")
            && keys.contains("left") && keys.contains("right")
    }

    private static func inputNames(from value: Any?) -> [String] {
        if let list = value as? [String] {
            return list.map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty }
        }
        if let dict = value as? [String: Any] {
            // Directional dictionaries are handled separately; other dict shapes are ignored.
            return dict.values.compactMap { $0 as? String }
        }
        if let single = value as? String {
            let trimmed = single.trimmingCharacters(in: .whitespacesAndNewlines)
            return trimmed.isEmpty ? [] : [trimmed]
        }
        return []
    }

    private static func readFrame(_ value: Any?) -> CGRect? {
        guard let box = value as? [String: Any] else { return nil }
        let x = number(box["x"])
        let y = number(box["y"])
        let width = number(box["width"])
        let height = number(box["height"])
        guard width > 0, height > 0 else { return nil }
        return CGRect(x: x, y: y, width: width, height: height)
    }
}

// MARK: - Delta name maps

extension GameSystem {
    /// Continuum system for a Delta `gameTypeIdentifier`, when Continuum has that console.
    static func fromDeltaGameType(_ identifier: String?) -> GameSystem? {
        guard let raw = identifier?.trimmingCharacters(in: .whitespacesAndNewlines).lowercased(),
              !raw.isEmpty else { return nil }
        switch raw {
        case "com.rileytestut.delta.game.gbc":
            return .gbc
        case "com.rileytestut.delta.game.gba":
            return .gba
        case "com.rileytestut.delta.game.ds":
            return .ds
        case "com.rileytestut.delta.game.nes":
            return .nes
        case "com.rileytestut.delta.game.snes":
            return .snes
        case "com.rileytestut.delta.game.n64":
            return .n64
        case "com.rileytestut.delta.game.genesis":
            return .genesis
        default:
            return nil
        }
    }
}

extension PadSlot {
    /// Maps a Delta item input name onto Continuum's layout key slot.
    static func fromDeltaInput(_ name: String) -> PadSlot? {
        switch name.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() {
        case "a": return .a
        case "b": return .b
        case "x": return .x
        case "y": return .y
        // PS1 face names (same retro slots Continuum labels Triangle/Circle/Cross/Square).
        case "triangle": return .x
        case "circle": return .a
        case "cross": return .b
        case "square": return .y
        case "l", "l1": return .l
        case "r", "r1": return .r
        case "l2": return .l2
        case "r2": return .r2
        case "select": return .select
        case "start": return .start
        case "up": return .up
        case "down": return .down
        case "left": return .left
        case "right": return .right
        default: return nil
        }
    }
}

// MARK: - Document picker (UIKit, not SwiftUI fileImporter)

/// Presents a skin file picker and delivers one import result on the main actor.
///
/// Uses `UIDocumentPickerViewController` for the same reason ROM and artwork import do:
/// SwiftUI `.fileImporter` was proven on device not to call its completion. The class itself is
/// not `@MainActor` — UIKit delegate methods carry no isolation the compiler can see — so every
/// finish hops onto the main actor before touching the editor callback.
final class DeltaSkinPicker: NSObject, UIDocumentPickerDelegate, UIAdaptivePresentationControllerDelegate {

    private var activePicker: UIDocumentPickerViewController?
    private var onFinish: ((Result<DeltaSkinImportResult, DeltaSkinImportError>) -> Void)?
    private var settled = false
    /// Set synchronously on the UIKit callback thread before the main-actor hop, so a later
    /// dismissal notice cannot overwrite a real pick / cancel that already started finishing.
    private var outcomeDelivered = false

    /// Opens the picker. `onFinish` always runs once (success, cancel, or failure).
    @MainActor
    func present(onFinish: @escaping (Result<DeltaSkinImportResult, DeltaSkinImportError>) -> Void) {
        self.onFinish = onFinish
        settled = false
        outcomeDelivered = false

        guard let presenter = EngineHost.topmostViewController() else {
            finish(.failure(.unreadable("no root view controller to present the picker")))
            return
        }

        let types = Self.contentTypes
        let picker = UIDocumentPickerViewController(forOpeningContentTypes: types, asCopy: true)
        picker.delegate = self
        picker.allowsMultipleSelection = false
        picker.shouldShowFileExtensions = true
        picker.presentationController?.delegate = self
        activePicker = picker

        presenter.present(picker, animated: true)
    }

    private static var contentTypes: [UTType] {
        var types: [UTType] = []
        if let delta = UTType(filenameExtension: "deltaskin") {
            types.append(delta)
        }
        types.append(.zip)
        types.append(.json)
        // Fallback so Files does not grey out packs when the custom extension has no UTI.
        types.append(.item)
        return types
    }

    private func finish(_ result: Result<DeltaSkinImportResult, DeltaSkinImportError>) {
        // UIKit may call back off the main actor; hop before the editor mutates `@State`.
        Task { @MainActor in
            self.deliver(result)
        }
    }

    @MainActor
    private func deliver(_ result: Result<DeltaSkinImportResult, DeltaSkinImportError>) {
        guard !settled else { return }
        settled = true
        activePicker = nil
        let callback = onFinish
        onFinish = nil
        callback?(result)
    }

    // MARK: UIDocumentPickerDelegate

    func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
        outcomeDelivered = true
        guard let url = urls.first else {
            finish(.failure(.noFileSelected))
            return
        }
        do {
            let imported = try DeltaSkinImporter.importPackage(at: url)
            finish(.success(imported))
        } catch let error as DeltaSkinImportError {
            finish(.failure(error))
        } catch {
            finish(.failure(.unreadable(error.localizedDescription)))
        }
    }

    func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
        outcomeDelivered = true
        finish(.failure(.cancelled))
    }

    func presentationControllerDidDismiss(_ presentationController: UIPresentationController) {
        // Swipe-away with no pick. Skip if a pick/cancel callback already ran (UIKit dismisses
        // after those too).
        guard !outcomeDelivered else { return }
        outcomeDelivered = true
        finish(.failure(.cancelled))
    }
}

// MARK: - Minimal ZIP reader (info.json only)

/// Enough ZIP to pull `info.json` out of a .deltaskin. Supports store (0) and deflate (8).
enum ZipStore {

    static func data(forEntryNamed name: String, in archive: Data) throws -> Data? {
        for entry in try listEntries(in: archive) {
            if entry.name == name || entry.name.hasSuffix("/\(name)") {
                return try payload(for: entry, in: archive)
            }
        }
        return nil
    }

    static func dataMatchingInfoJSON(in archive: Data) throws -> Data? {
        let entries = try listEntries(in: archive)
        let matches = entries.filter {
            $0.name.lowercased() == "info.json" || $0.name.lowercased().hasSuffix("/info.json")
        }
        guard let entry = matches.first else { return nil }
        return try payload(for: entry, in: archive)
    }

    /// Finds an asset by basename, including packs that nest one folder.
    static func dataMatchingFileName(_ fileName: String, in archive: Data) throws -> Data? {
        let want = fileName.lowercased()
        let entries = try listEntries(in: archive)
        for entry in entries {
            let lower = entry.name.lowercased()
            if lower == want || lower.hasSuffix("/\(want)") {
                return try payload(for: entry, in: archive)
            }
        }
        return nil
    }

    private struct Entry {
        let name: String
        let method: UInt16
        let compressedSize: UInt32
        let uncompressedSize: UInt32
        let dataOffset: Int
    }

    private static func listEntries(in archive: Data) throws -> [Entry] {
        // Prefer the central directory when the end-of-central-directory record is present.
        if let fromCentral = try? readCentralDirectory(archive) {
            return fromCentral
        }
        return try readLocalHeaders(archive)
    }

    private static func readCentralDirectory(_ archive: Data) throws -> [Entry] {
        guard archive.count >= 22 else {
            throw DeltaSkinImportError.unreadable("archive too small")
        }
        // Scan backward for EOCD signature 0x06054b50.
        let eocdSig: UInt32 = 0x0605_4b50
        var eocdOffset: Int?
        let start = max(0, archive.count - 65_535 - 22)
        var i = archive.count - 22
        while i >= start {
            if readU32(archive, i) == eocdSig {
                eocdOffset = i
                break
            }
            i -= 1
        }
        guard let eocd = eocdOffset else {
            throw DeltaSkinImportError.unreadable("no ZIP end record")
        }
        let entryCount = Int(readU16(archive, eocd + 10))
        let centralSize = Int(readU32(archive, eocd + 12))
        let centralOffset = Int(readU32(archive, eocd + 16))
        guard centralOffset >= 0,
              centralOffset + max(centralSize, 0) <= archive.count else {
            throw DeltaSkinImportError.unreadable("ZIP central directory out of range")
        }

        var entries: [Entry] = []
        var cursor = centralOffset
        let centralSig: UInt32 = 0x0201_4b50
        for _ in 0..<entryCount {
            guard cursor + 46 <= archive.count, readU32(archive, cursor) == centralSig else {
                break
            }
            let method = readU16(archive, cursor + 10)
            let compSize = readU32(archive, cursor + 20)
            let uncompSize = readU32(archive, cursor + 24)
            let nameLen = Int(readU16(archive, cursor + 28))
            let extraLen = Int(readU16(archive, cursor + 30))
            let commentLen = Int(readU16(archive, cursor + 32))
            let localHeaderOffset = Int(readU32(archive, cursor + 42))
            let nameStart = cursor + 46
            guard nameStart + nameLen <= archive.count else { break }
            let nameData = archive.subdata(in: nameStart..<(nameStart + nameLen))
            let name = String(data: nameData, encoding: .utf8) ?? ""
            let dataOffset = try localDataOffset(archive, localHeaderOffset: localHeaderOffset)
            if !name.hasSuffix("/") {
                entries.append(Entry(
                    name: name,
                    method: method,
                    compressedSize: compSize,
                    uncompressedSize: uncompSize,
                    dataOffset: dataOffset
                ))
            }
            cursor = nameStart + nameLen + extraLen + commentLen
        }
        return entries
    }

    private static func localDataOffset(_ archive: Data, localHeaderOffset: Int) throws -> Int {
        let localSig: UInt32 = 0x0403_4b50
        guard localHeaderOffset + 30 <= archive.count,
              readU32(archive, localHeaderOffset) == localSig else {
            throw DeltaSkinImportError.unreadable("ZIP local header missing")
        }
        let nameLen = Int(readU16(archive, localHeaderOffset + 26))
        let extraLen = Int(readU16(archive, localHeaderOffset + 28))
        return localHeaderOffset + 30 + nameLen + extraLen
    }

    private static func readLocalHeaders(_ archive: Data) throws -> [Entry] {
        var entries: [Entry] = []
        var cursor = 0
        let localSig: UInt32 = 0x0403_4b50
        while cursor + 30 <= archive.count {
            if readU32(archive, cursor) != localSig { break }
            let method = readU16(archive, cursor + 8)
            let compSize = readU32(archive, cursor + 18)
            let uncompSize = readU32(archive, cursor + 22)
            let nameLen = Int(readU16(archive, cursor + 26))
            let extraLen = Int(readU16(archive, cursor + 28))
            let nameStart = cursor + 30
            guard nameStart + nameLen <= archive.count else { break }
            let nameData = archive.subdata(in: nameStart..<(nameStart + nameLen))
            let name = String(data: nameData, encoding: .utf8) ?? ""
            let dataOffset = nameStart + nameLen + extraLen
            guard dataOffset + Int(compSize) <= archive.count else { break }
            if !name.hasSuffix("/") {
                entries.append(Entry(
                    name: name,
                    method: method,
                    compressedSize: compSize,
                    uncompressedSize: uncompSize,
                    dataOffset: dataOffset
                ))
            }
            cursor = dataOffset + Int(compSize)
        }
        if entries.isEmpty {
            throw DeltaSkinImportError.unreadable("no ZIP entries found")
        }
        return entries
    }

    private static func payload(for entry: Entry, in archive: Data) throws -> Data {
        let start = entry.dataOffset
        let end = start + Int(entry.compressedSize)
        guard start >= 0, end <= archive.count else {
            throw DeltaSkinImportError.unreadable("ZIP entry data out of range")
        }
        let slice = archive.subdata(in: start..<end)
        switch entry.method {
        case 0:
            return slice
        case 8:
            return try inflateRawDeflate(slice, expectedSize: Int(entry.uncompressedSize))
        default:
            throw DeltaSkinImportError.unreadable("unsupported ZIP compression method \(entry.method)")
        }
    }

    /// ZIP method 8 is raw DEFLATE (no zlib wrapper). `inflateInit2(..., -MAX_WBITS)` is the
    /// documented way to decode that; Compression.framework's ZLIB path expects a wrapper and
    /// fails on ordinary .deltaskin packs.
    private static func inflateRawDeflate(_ compressed: Data, expectedSize: Int) throws -> Data {
        var stream = z_stream()
        let initStatus = zlib.inflateInit2_(&stream, -MAX_WBITS, ZLIB_VERSION,
                                       Int32(MemoryLayout<z_stream>.size))
        guard initStatus == Z_OK else {
            throw DeltaSkinImportError.unreadable("deflate init failed (\(initStatus))")
        }
        defer { zlib.inflateEnd(&stream) }

        let capacity = max(expectedSize, 1)
        var out = Data(count: capacity)
        var total = 0

        try compressed.withUnsafeBytes { srcPtr in
            guard let srcBase = srcPtr.bindMemory(to: UInt8.self).baseAddress else {
                throw DeltaSkinImportError.unreadable("deflate source unavailable")
            }
            stream.next_in = UnsafeMutablePointer(mutating: srcBase)
            stream.avail_in = uInt(compressed.count)

            while true {
                if total >= out.count {
                    out.count = max(out.count * 2, capacity * 2)
                }
                // Snapshot length before borrowing `out` — Swift exclusivity forbids
                // reading `out.count` inside `withUnsafeMutableBytes`.
                let outCount = out.count
                let availBefore = uInt(outCount - total)
                let wrote: Int = try out.withUnsafeMutableBytes { dstPtr in
                    guard let dstBase = dstPtr.bindMemory(to: UInt8.self).baseAddress else {
                        throw DeltaSkinImportError.unreadable("deflate destination unavailable")
                    }
                    stream.next_out = dstBase.advanced(by: total)
                    stream.avail_out = availBefore
                    let status = zlib.inflate(&stream, Z_NO_FLUSH)
                    let produced = Int(availBefore) - Int(stream.avail_out)
                    total += produced
                    if status == Z_STREAM_END {
                        return -1
                    }
                    if status != Z_OK {
                        throw DeltaSkinImportError.unreadable("deflate decode failed (\(status))")
                    }
                    return produced
                }
                if wrote < 0 { break }
                if stream.avail_in == 0 && stream.avail_out > 0 {
                    break
                }
            }
        }
        out.count = total
        return out
    }

    private static func readU16(_ data: Data, _ offset: Int) -> UInt16 {
        UInt16(data[offset]) | (UInt16(data[offset + 1]) << 8)
    }

    private static func readU32(_ data: Data, _ offset: Int) -> UInt32 {
        UInt32(data[offset])
            | (UInt32(data[offset + 1]) << 8)
            | (UInt32(data[offset + 2]) << 16)
            | (UInt32(data[offset + 3]) << 24)
    }
}
