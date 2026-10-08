// Continuum - Delta .deltaskin package import for on-screen control layouts.
//
// A .deltaskin is a ZIP with a flat info.json (Delta Custom Skins docs). A Manic EMU .manicskin is
// the same package with Manic's extra item fields (functions, switches, sound.caf), read by
// ManicSkinItems.swift. This file:
//   1. Opens a document picker for .manicskin / .deltaskin / .zip / bare info.json
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

/// One orientation of an imported skin: its own mapping, holes, art, and button layout.
struct DeltaSkinFace: Equatable, Sendable {
    var mappingWidth: Double
    var mappingHeight: Double
    var screenOutput: DeltaSkinNormalizedRect?
    var assetFileName: String?
    var assetKind: DeltaSkinAssetKind?
    var layout: TouchLayout
    /// Every screen hole, top first. Empty on skins saved before both holes were kept;
    /// `screenOutput` is then the only hole.
    var screens: [DeltaSkinScreen] = []
    var sticks: [DeltaSkinStick] = []
    var buttons: [DeltaSkinButton] = []
    var dpadFrame: DeltaSkinNormalizedRect?

    init(mappingWidth: Double, mappingHeight: Double,
         screenOutput: DeltaSkinNormalizedRect?, assetFileName: String?,
         assetKind: DeltaSkinAssetKind?, layout: TouchLayout,
         screens: [DeltaSkinScreen] = [], sticks: [DeltaSkinStick] = [],
         buttons: [DeltaSkinButton] = [], dpadFrame: DeltaSkinNormalizedRect? = nil) {
        self.mappingWidth = mappingWidth
        self.mappingHeight = mappingHeight
        self.screenOutput = screenOutput
        self.assetFileName = assetFileName
        self.assetKind = assetKind
        self.layout = layout
        self.screens = screens
        self.sticks = sticks
        self.buttons = buttons
        self.dpadFrame = dpadFrame
    }

    /// Holes to draw. A skin saved before `screens` existed still has `screenOutput`.
    var effectiveScreens: [DeltaSkinScreen] {
        if !screens.isEmpty { return screens }
        if let screenOutput {
            return [DeltaSkinScreen(output: screenOutput, inputX: 0, inputY: 0,
                                    inputWidth: 0, inputHeight: 0)]
        }
        return []
    }
}

extension DeltaSkinFace: Codable {
    enum CodingKeys: String, CodingKey {
        case mappingWidth, mappingHeight, screenOutput, assetFileName, assetKind, layout
        case screens, sticks, buttons, dpadFrame
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        mappingWidth = try c.decode(Double.self, forKey: .mappingWidth)
        mappingHeight = try c.decode(Double.self, forKey: .mappingHeight)
        screenOutput = try c.decodeIfPresent(DeltaSkinNormalizedRect.self, forKey: .screenOutput)
        assetFileName = try c.decodeIfPresent(String.self, forKey: .assetFileName)
        assetKind = try c.decodeIfPresent(DeltaSkinAssetKind.self, forKey: .assetKind)
        layout = try c.decode(TouchLayout.self, forKey: .layout)
        screens = try c.decodeIfPresent([DeltaSkinScreen].self, forKey: .screens) ?? []
        sticks = try c.decodeIfPresent([DeltaSkinStick].self, forKey: .sticks) ?? []
        buttons = try c.decodeIfPresent([DeltaSkinButton].self, forKey: .buttons) ?? []
        dpadFrame = try c.decodeIfPresent(DeltaSkinNormalizedRect.self, forKey: .dpadFrame)
        if screens.isEmpty, let screenOutput {
            screens = [DeltaSkinScreen(output: screenOutput, inputX: 0, inputY: 0,
                                       inputWidth: 0, inputHeight: 0)]
        }
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(mappingWidth, forKey: .mappingWidth)
        try c.encode(mappingHeight, forKey: .mappingHeight)
        try c.encodeIfPresent(screenOutput, forKey: .screenOutput)
        try c.encodeIfPresent(assetFileName, forKey: .assetFileName)
        try c.encodeIfPresent(assetKind, forKey: .assetKind)
        try c.encode(layout, forKey: .layout)
        try c.encode(screens, forKey: .screens)
        try c.encode(sticks, forKey: .sticks)
        try c.encode(buttons, forKey: .buttons)
        try c.encodeIfPresent(dpadFrame, forKey: .dpadFrame)
    }
}

/// Artwork + screen holes from one imported representation (persisted per console).
struct DeltaSkinVisual: Equatable, Sendable {
    var skinName: String
    var translucent: Bool
    var mappingWidth: Double
    var mappingHeight: Double
    /// First hole. Kept so a skin saved before `screens` still has somewhere to draw.
    var screenOutput: DeltaSkinNormalizedRect?
    var assetFileName: String?
    var assetKind: DeltaSkinAssetKind?
    /// Landscape representation, when the package had one. Nil on skins imported before both
    /// orientations were kept, and on packages that only ship portrait.
    var landscape: DeltaSkinFace?
    var screens: [DeltaSkinScreen] = []
    var sticks: [DeltaSkinStick] = []
    var buttons: [DeltaSkinButton] = []
    var dpadFrame: DeltaSkinNormalizedRect?

    init(skinName: String, translucent: Bool, mappingWidth: Double, mappingHeight: Double,
         screenOutput: DeltaSkinNormalizedRect?, assetFileName: String?,
         assetKind: DeltaSkinAssetKind?, landscape: DeltaSkinFace?,
         screens: [DeltaSkinScreen] = [], sticks: [DeltaSkinStick] = [],
         buttons: [DeltaSkinButton] = [], dpadFrame: DeltaSkinNormalizedRect? = nil) {
        self.skinName = skinName
        self.translucent = translucent
        self.mappingWidth = mappingWidth
        self.mappingHeight = mappingHeight
        self.screenOutput = screenOutput
        self.assetFileName = assetFileName
        self.assetKind = assetKind
        self.landscape = landscape
        self.screens = screens
        self.sticks = sticks
        self.buttons = buttons
        self.dpadFrame = dpadFrame
    }

    var effectiveScreens: [DeltaSkinScreen] {
        if !screens.isEmpty { return screens }
        if let screenOutput {
            return [DeltaSkinScreen(output: screenOutput, inputX: 0, inputY: 0,
                                    inputWidth: 0, inputHeight: 0)]
        }
        return []
    }
}

extension DeltaSkinVisual: Codable {
    enum CodingKeys: String, CodingKey {
        case skinName, translucent, mappingWidth, mappingHeight, screenOutput
        case assetFileName, assetKind, landscape
        case screens, sticks, buttons, dpadFrame
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        skinName = try c.decode(String.self, forKey: .skinName)
        translucent = try c.decodeIfPresent(Bool.self, forKey: .translucent) ?? false
        mappingWidth = try c.decode(Double.self, forKey: .mappingWidth)
        mappingHeight = try c.decode(Double.self, forKey: .mappingHeight)
        screenOutput = try c.decodeIfPresent(DeltaSkinNormalizedRect.self, forKey: .screenOutput)
        assetFileName = try c.decodeIfPresent(String.self, forKey: .assetFileName)
        assetKind = try c.decodeIfPresent(DeltaSkinAssetKind.self, forKey: .assetKind)
        landscape = try c.decodeIfPresent(DeltaSkinFace.self, forKey: .landscape)
        screens = try c.decodeIfPresent([DeltaSkinScreen].self, forKey: .screens) ?? []
        sticks = try c.decodeIfPresent([DeltaSkinStick].self, forKey: .sticks) ?? []
        buttons = try c.decodeIfPresent([DeltaSkinButton].self, forKey: .buttons) ?? []
        dpadFrame = try c.decodeIfPresent(DeltaSkinNormalizedRect.self, forKey: .dpadFrame)
        if screens.isEmpty, let screenOutput {
            screens = [DeltaSkinScreen(output: screenOutput, inputX: 0, inputY: 0,
                                       inputWidth: 0, inputHeight: 0)]
        }
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(skinName, forKey: .skinName)
        try c.encode(translucent, forKey: .translucent)
        try c.encode(mappingWidth, forKey: .mappingWidth)
        try c.encode(mappingHeight, forKey: .mappingHeight)
        try c.encodeIfPresent(screenOutput, forKey: .screenOutput)
        try c.encodeIfPresent(assetFileName, forKey: .assetFileName)
        try c.encodeIfPresent(assetKind, forKey: .assetKind)
        try c.encodeIfPresent(landscape, forKey: .landscape)
        try c.encode(screens, forKey: .screens)
        try c.encode(sticks, forKey: .sticks)
        try c.encode(buttons, forKey: .buttons)
        try c.encodeIfPresent(dpadFrame, forKey: .dpadFrame)
    }
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
    /// Items before they are mapped onto a system. `applying(system:)` is what makes
    /// shoulders and the circle pad belong to the console the skin was imported for.
    let portraitItems: [DeltaSkinRawItem]
    let landscapeItems: [DeltaSkinRawItem]
    /// Per-button and thumbstick images named by those items.
    let pieces: [DeltaSkinPiece]
    /// Every Continuum system id the file's `gameTypeIdentifier` names (Manic or Delta). Empty
    /// when the identifier is unknown. Strings, so a system with no enum case yet is kept.
    var systemIDs: [String] = []
    var gameTypeIdentifier: String = ""
    /// The skin's own `identifier` from info.json.
    var identifier: String = ""
    /// The file the skin came from, for the library list.
    var sourceName: String = ""
    /// "manic" or "delta", from the identifier and the file extension.
    var format: String = "delta"
    /// `sound.caf` from the package root, when there was one.
    var soundData: Data?
    /// Lines for items left out on purpose (a function mixed with other inputs).
    var refused: [String] = []

    /// Layout, button frames and sticks for `system`. Screen holes do not change.
    func applying(system: GameSystem) -> DeltaSkinImportResult {
        let kind: (String) -> DeltaSkinAssetKind = { name in
            name.lowercased().hasSuffix(".pdf") ? .pdf : .png
        }
        let portrait = SkinControls.resolve(portraitItems, system: system, assetKind: kind)
        var visual = self.visual
        visual.layoutStandIn(portrait)
        let landscapeItems = self.landscapeItems
        if var face = visual.landscape {
            let land = SkinControls.resolve(landscapeItems, system: system, assetKind: kind)
            face.layout = land.layout
            face.buttons = land.buttons
            face.sticks = land.sticks
            face.dpadFrame = land.dpadFrame
            visual.landscape = face
        }
        var out = DeltaSkinImportResult(
            layout: portrait.layout,
            skinName: skinName,
            previewSystem: previewSystem,
            summary: summary,
            visual: visual,
            assetData: assetData,
            landscapeAssetData: landscapeAssetData,
            portraitItems: portraitItems,
            landscapeItems: landscapeItems,
            pieces: pieces
        )
        out.systemIDs = systemIDs
        out.gameTypeIdentifier = gameTypeIdentifier
        out.identifier = identifier
        out.sourceName = sourceName
        out.format = format
        out.soundData = soundData
        out.refused = portrait.refused
        return out
    }
}

private extension DeltaSkinVisual {
    mutating func layoutStandIn(_ resolved: SkinControls.Resolved) {
        // TouchLayout lives on the import result. The face fields are the ones the pad reads.
        buttons = resolved.buttons
        sticks = resolved.sticks
        dpadFrame = resolved.dpadFrame
    }
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
            return "No skin selected. Pick a .manicskin, .deltaskin, .zip, or info.json."
        case .cancelled:
            return "Import cancelled — no skin selected."
        case .unreadable(let detail):
            return "Could not read the skin package: \(detail)"
        case .missingInfoJSON:
            return "No info.json in that package. A .manicskin or .deltaskin must contain a flat info.json."
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
        let systemIDs = SkinGameTypes.systemIDs(forGameType: gameType)
        let isManic = SkinGameTypes.isManic(gameType)
            || sourceName.lowercased().hasSuffix(".manicskin")

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

        let portraitItems = chosen.items.compactMap {
            SkinControls.classify($0, mappingSize: mapping)
        }
        let usable = portraitItems.contains { item in
            if case .touch = item.kind { return false }
            return true
        }
        guard usable else { throw DeltaSkinImportError.noMappableItems }
        let mapSystem = preview ?? .gbc
        let mapped = SkinControls.resolve(portraitItems, system: mapSystem, assetKind: Self.assetKind(for:))
        guard mapped.applied > 0 else { throw DeltaSkinImportError.noMappableItems }

        let screens = SkinControls.screens(from: chosen.screens, mappingSize: mapping)
        let screenOutput = screens.first?.output
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
            isManic ? "Manic skin" : "Delta skin",
            "\(chosen.path)",
            "\(mapped.applied) control(s)",
        ]
        if systemIDs.isEmpty {
            summaryBits.append("console not named by the file")
        } else {
            summaryBits.append("for " + systemIDs.joined(separator: "/"))
        }
        if mapped.functions > 0 {
            summaryBits.append("\(mapped.functions) function button(s)")
        }
        if mapped.switches > 0 {
            summaryBits.append("\(mapped.switches) switch(es)")
        }
        if !mapped.refused.isEmpty {
            summaryBits.append("\(mapped.refused.count) refused: " + mapped.refused.joined(separator: "; "))
        }
        if screens.count > 1 {
            summaryBits.append("\(screens.count) screen holes")
        } else if screenOutput != nil {
            summaryBits.append("game screen frame applied")
        } else {
            summaryBits.append("no screens[] — picture keeps free band")
        }
        if !mapped.sticks.isEmpty {
            summaryBits.append("\(mapped.sticks.count) analog stick(s)")
        }
        if landscapeFace != nil {
            summaryBits.append("landscape kept")
            if landscapeFace?.effectiveScreens.isEmpty == false {
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
            landscape: landscapeFace,
            screens: screens,
            sticks: mapped.sticks,
            buttons: mapped.buttons,
            dpadFrame: mapped.dpadFrame
        )

        var landscapeItems: [DeltaSkinRawItem] = []
        if let landscapeChoice {
            landscapeItems = landscapeChoice.items.compactMap {
                SkinControls.classify($0, mappingSize: landscapeChoice.mappingSize)
            }
        }
        let pieces = Self.pieces(for: portraitItems + landscapeItems, assetLookup: assetLookup)
        let sound = assetLookup(ManicItems.soundFileName).flatMap { $0.isEmpty ? nil : $0 }
        if sound != nil {
            summaryBits.append("button sound kept")
        }

        var result = DeltaSkinImportResult(
            layout: mapped.layout,
            skinName: name,
            previewSystem: preview,
            summary: summaryBits.joined(separator: " · "),
            visual: visual,
            assetData: assetData,
            landscapeAssetData: landscapeAssetData,
            portraitItems: portraitItems,
            landscapeItems: landscapeItems,
            pieces: pieces
        )
        result.systemIDs = systemIDs
        result.gameTypeIdentifier = gameType ?? ""
        result.identifier = (root["identifier"] as? String) ?? ""
        result.sourceName = sourceName
        result.format = isManic ? "manic" : "delta"
        result.soundData = sound
        result.refused = mapped.refused
        return result
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

        if ext == "deltaskin" || ext == "manicskin" || ext == "zip" {
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
            "expected a .manicskin, .deltaskin, .zip, or info.json (got .\(ext.isEmpty ? "unknown" : ext))"
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

    private static func assetKind(for name: String) -> DeltaSkinAssetKind {
        name.lowercased().hasSuffix(".pdf") ? .pdf : .png
    }

    private static func pieces(for items: [DeltaSkinRawItem],
                               assetLookup: (String) -> Data?) -> [DeltaSkinPiece] {
        var seen = Set<String>()
        var out: [DeltaSkinPiece] = []
        for item in items {
            for name in [item.normalFileName, item.pressedFileName, item.stickAssetFileName,
                         item.toggle?.selectedFileName] {
                guard let name, !name.isEmpty, seen.insert(name).inserted else { continue }
                guard let data = assetLookup(name), !data.isEmpty else { continue }
                out.append(DeltaSkinPiece(fileName: name, kind: assetKind(for: name), data: data))
            }
        }
        return out
    }

    private static func buildFace(
        _ chosen: ChosenOrientation,
        assetLookup: (String) -> Data?
    ) -> BuiltFace {
        let mapping = chosen.mappingSize
        let raw = chosen.items.compactMap { SkinControls.classify($0, mappingSize: mapping) }
        // Placeholder system. `applying(system:)` rewrites buttons and sticks for the console
        // the skin is actually saved under. Screens do not depend on that.
        let mapped = SkinControls.resolve(raw, system: .gbc, assetKind: assetKind(for:))
        let screens = SkinControls.screens(from: chosen.screens, mappingSize: mapping)
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
            screenOutput: screens.first?.output,
            assetFileName: assetData == nil ? nil : assetPick?.name,
            assetKind: assetKind,
            layout: mapped.layout,
            screens: screens,
            sticks: mapped.sticks,
            buttons: mapped.buttons,
            dpadFrame: mapped.dpadFrame
        )
        let kept = raw.contains { item in
            if case .touch = item.kind { return false }
            return true
        }
        return BuiltFace(face: face, assetData: assetData, applied: kept ? max(mapped.applied, 1) : 0)
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
        // Strings first (SkinLibrary.swift), so an id for a console this build has no case for
        // is simply skipped here and still kept on the skin's record.
        SkinGameTypes.systemIDs(forGameType: identifier).lazy.compactMap(GameSystem.init(rawValue:)).first
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
    /// Set instead of `onFinish` by `presentMany`, which takes every file picked.
    private var onFinishMany: (([SkinPickOutcome]) -> Void)?
    /// Which of the two the open picker answers. Set before it is shown.
    private var wantsMany = false
    private var settled = false
    /// Set synchronously on the UIKit callback thread before the main-actor hop, so a later
    /// dismissal notice cannot overwrite a real pick / cancel that already started finishing.
    private var outcomeDelivered = false

    /// Opens the picker for ONE skin. `onFinish` always runs once (success, cancel, or failure).
    /// The layout editor uses this, because it previews the one skin it imported.
    @MainActor
    func present(onFinish: @escaping (Result<DeltaSkinImportResult, DeltaSkinImportError>) -> Void) {
        self.onFinish = onFinish
        onFinishMany = nil
        wantsMany = false
        settled = false
        outcomeDelivered = false

        guard let presenter = EngineHost.topmostViewController() else {
            finish(.failure(.unreadable("no root view controller to present the picker")))
            return
        }
        presenter.present(makePicker(multiple: false), animated: true)
    }

    /// Opens the picker for ANY NUMBER of skins at once: select several in Files and every one is
    /// imported. `onFinish` always runs once, with one outcome per picked file in the order they
    /// were picked, or a single `.cancelled` failure when nothing was picked.
    ///
    /// Asked for by the owner: importing a set of skins one at a time, each through the picker
    /// again, was the slow way round.
    @MainActor
    func presentMany(onFinish: @escaping ([SkinPickOutcome]) -> Void) {
        onFinishMany = onFinish
        self.onFinish = nil
        wantsMany = true
        settled = false
        outcomeDelivered = false

        guard let presenter = EngineHost.topmostViewController() else {
            finishMany([SkinPickOutcome(
                fileName: "",
                result: .failure(.unreadable("no root view controller to present the picker"))
            )])
            return
        }
        presenter.present(makePicker(multiple: true), animated: true)
    }

    @MainActor
    private func makePicker(multiple: Bool) -> UIDocumentPickerViewController {
        let picker = UIDocumentPickerViewController(forOpeningContentTypes: Self.contentTypes,
                                                    asCopy: true)
        picker.delegate = self
        picker.allowsMultipleSelection = multiple
        picker.shouldShowFileExtensions = true
        picker.presentationController?.delegate = self
        activePicker = picker
        return picker
    }

    /// One picked file, imported or not.
    private static func importOne(_ url: URL) -> Result<DeltaSkinImportResult, DeltaSkinImportError> {
        do {
            return .success(try DeltaSkinImporter.importPackage(at: url))
        } catch let error as DeltaSkinImportError {
            return .failure(error)
        } catch {
            return .failure(.unreadable(error.localizedDescription))
        }
    }

    private func finishMany(_ outcomes: [SkinPickOutcome]) {
        // Same hop as `finish`, for the same reason.
        Task { @MainActor in
            self.deliverMany(outcomes)
        }
    }

    @MainActor
    private func deliverMany(_ outcomes: [SkinPickOutcome]) {
        guard !settled else { return }
        settled = true
        activePicker = nil
        let callback = onFinishMany
        onFinishMany = nil
        callback?(outcomes)
    }

    private static var contentTypes: [UTType] {
        var types: [UTType] = []
        if let delta = UTType(filenameExtension: "deltaskin") {
            types.append(delta)
        }
        if let manic = UTType(filenameExtension: "manicskin") {
            types.append(manic)
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
        if wantsMany {
            guard !urls.isEmpty else {
                finishMany([SkinPickOutcome(fileName: "", result: .failure(.noFileSelected))])
                return
            }
            // Off the main thread: unpacking several large skins used to freeze the screen.
            // `finishMany` hops back to the main actor, so the callers see no change.
            DispatchQueue.global(qos: .userInitiated).async {
                self.finishMany(urls.map { url in
                    SkinPickOutcome(fileName: url.lastPathComponent, result: Self.importOne(url))
                })
            }
            return
        }
        guard let url = urls.first else {
            finish(.failure(.noFileSelected))
            return
        }
        DispatchQueue.global(qos: .userInitiated).async {
            self.finish(Self.importOne(url))
        }
    }

    func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
        outcomeDelivered = true
        finishCancelled()
    }

    func presentationControllerDidDismiss(_ presentationController: UIPresentationController) {
        // Swipe-away with no pick. Skip if a pick/cancel callback already ran (UIKit dismisses
        // after those too).
        guard !outcomeDelivered else { return }
        outcomeDelivered = true
        finishCancelled()
    }

    private func finishCancelled() {
        if wantsMany {
            finishMany([SkinPickOutcome(fileName: "", result: .failure(.cancelled))])
        } else {
            finish(.failure(.cancelled))
        }
    }
}

/// One file from `DeltaSkinPicker.presentMany`: its name, and what importing it produced.
struct SkinPickOutcome {
    let fileName: String
    let result: Result<DeltaSkinImportResult, DeltaSkinImportError>
}

// MARK: - Minimal ZIP reader (info.json only)

/// Enough ZIP to pull `info.json` out of a .deltaskin. Supports store (0) and deflate (8).
enum ZipStore {

    /// The entry at the package root wins over one with the same name in a subfolder, whatever
    /// order the archive lists them in: a nested `info.json` is usually a stray copy.
    static func data(forEntryNamed name: String, in archive: Data) throws -> Data? {
        let entries = try listEntries(in: archive)
        if let root = entries.first(where: { $0.name == name }) {
            return try payload(for: root, in: archive)
        }
        if let nested = entries.first(where: { $0.name.hasSuffix("/\(name)") }) {
            return try payload(for: nested, in: archive)
        }
        return nil
    }

    static func dataMatchingInfoJSON(in archive: Data) throws -> Data? {
        let entries = try listEntries(in: archive)
        let matches = entries.filter {
            $0.name.lowercased() == "info.json" || $0.name.lowercased().hasSuffix("/info.json")
        }
        // Shallowest first, so the root copy wins over a nested one.
        let depth: (Entry) -> Int = { $0.name.filter { $0 == "/" }.count }
        guard let entry = matches.min(by: { depth($0) < depth($1) }) else { return nil }
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
        let entries: [Entry]
        if let fromCentral = try? readCentralDirectory(archive) {
            entries = fromCentral
        } else {
            entries = try readLocalHeaders(archive)
        }
        // The whole package, by what its entries claim. Each entry is then held to its claim.
        let claimed = entries.reduce(0) { $0 + Int($1.uncompressedSize) }
        guard archive.count <= maxPackageBytes, claimed <= maxPackageBytes else {
            throw DeltaSkinImportError.unreadable(
                "the skin unpacks to more than 512 MB, which is far more than any skin needs, "
                    + "so it was not opened")
        }
        return entries
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
        // A header claiming more than any skin file needs is refused before anything is unpacked.
        guard Int(entry.uncompressedSize) <= maxEntryBytes else { throw tooLarge }
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
    ///
    /// BOUNDED, because the sizes in a ZIP are only claims. This used to start from the claimed
    /// size and keep doubling with no limit, so a zip bomb or a false size could use up memory
    /// until iOS killed the app. Now the output may not pass the entry's claimed size (or
    /// `maxEntryBytes` when it claims none), and a stream that runs out before its end marker is
    /// an error rather than a silently short file.
    private static func inflateRawDeflate(_ compressed: Data, expectedSize: Int) throws -> Data {
        var stream = z_stream()
        let initStatus = zlib.inflateInit2_(&stream, -MAX_WBITS, ZLIB_VERSION,
                                       Int32(MemoryLayout<z_stream>.size))
        guard initStatus == Z_OK else {
            throw DeltaSkinImportError.unreadable("a file inside the skin could not be unpacked")
        }
        defer { zlib.inflateEnd(&stream) }

        let limit = expectedSize > 0 ? min(expectedSize, maxEntryBytes) : maxEntryBytes
        // One byte past the limit, so a file of exactly the limit can still reach its end marker.
        let ceiling = limit + 1
        var out = Data(count: min(max(expectedSize, 1), limit) + 1)
        var total = 0
        var ended = false

        try compressed.withUnsafeBytes { srcPtr in
            guard let srcBase = srcPtr.bindMemory(to: UInt8.self).baseAddress else {
                throw DeltaSkinImportError.unreadable("a file inside the skin could not be read")
            }
            stream.next_in = UnsafeMutablePointer(mutating: srcBase)
            stream.avail_in = uInt(compressed.count)

            while !ended {
                if total >= out.count {
                    guard out.count < ceiling else { throw tooLarge }
                    out.count = min(ceiling, max(out.count * 2, 64 * 1024))
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
                        throw DeltaSkinImportError.unreadable(
                            "a file inside the skin is damaged and could not be unpacked")
                    }
                    return produced
                }
                if wrote < 0 {
                    ended = true
                } else if stream.avail_in == 0 && stream.avail_out > 0 {
                    // All input used, room left, and no end marker: the file was cut short.
                    throw DeltaSkinImportError.unreadable(
                        "a file inside the skin is cut short; the skin may be damaged or only "
                            + "partly downloaded")
                }
            }
        }
        guard total <= limit else { throw tooLarge }
        out.count = total
        return out
    }

    /// No real skin comes near these. They exist so a hostile or broken file cannot use up memory.
    static let maxEntryBytes = 64 << 20
    static let maxPackageBytes = 512 << 20

    private static let tooLarge = DeltaSkinImportError.unreadable(
        "a file inside the skin is bigger than 64 MB or bigger than it says it is, so it was "
            + "not opened")

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
