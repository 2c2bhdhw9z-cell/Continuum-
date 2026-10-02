// Continuum - Delta .deltaskin package import for on-screen control layouts.
//
// A .deltaskin is a ZIP with a flat info.json (Delta Custom Skins docs). This file:
//   1. Opens a document picker for .deltaskin / .zip / bare info.json
//   2. Extracts and parses info.json
//   3. Maps item frames into a TouchLayout (buttonFrees + cluster centres)
//
// Assets (PDF/PNG) are NOT applied yet. Cancelling or picking nothing fails with a clear
// message on the layout editor panel — not a silent no-op.

import Foundation
import zlib
import UniformTypeIdentifiers
import UIKit

// MARK: - Public result

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

    /// Reads a picked file URL (app-owned copy from the document picker) into a layout.
    static func importPackage(at url: URL) throws -> DeltaSkinImportResult {
        let infoData = try loadInfoJSON(from: url)
        return try parseInfoJSON(infoData, sourceName: url.lastPathComponent)
    }

    /// Same parse path for tests and for a bare info.json drop.
    static func parseInfoJSON(_ data: Data, sourceName: String) throws -> DeltaSkinImportResult {
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
        guard let chosen = pickRepresentation(from: representations) else {
            throw DeltaSkinImportError.noUsableRepresentation
        }

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
        summaryBits.append("layout only — skin art not drawn yet")

        return DeltaSkinImportResult(
            layout: mapped.layout.sanitised,
            skinName: name,
            previewSystem: preview,
            summary: summaryBits.joined(separator: " · ")
        )
    }

    // MARK: Load info.json

    private static func loadInfoJSON(from url: URL) throws -> Data {
        let ext = url.pathExtension.lowercased()
        if ext == "json" || url.lastPathComponent.lowercased() == "info.json" {
            do {
                return try Data(contentsOf: url)
            } catch {
                throw DeltaSkinImportError.unreadable(error.localizedDescription)
            }
        }

        if ext == "deltaskin" || ext == "zip" {
            do {
                let bytes = try Data(contentsOf: url)
                if let info = try ZipStore.data(forEntryNamed: "info.json", in: bytes) {
                    return info
                }
                // Some packs nest one folder; accept a single */info.json.
                if let nested = try ZipStore.dataMatchingInfoJSON(in: bytes) {
                    return nested
                }
                throw DeltaSkinImportError.missingInfoJSON
            } catch let error as DeltaSkinImportError {
                throw error
            } catch {
                throw DeltaSkinImportError.unreadable(error.localizedDescription)
            }
        }

        // Last resort: treat the bytes as JSON (Files sometimes strips extensions).
        do {
            let data = try Data(contentsOf: url)
            if (try? JSONSerialization.jsonObject(with: data)) is [String: Any] {
                return data
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
    }

    private static func pickRepresentation(from representations: [String: Any]) -> ChosenOrientation? {
        let deviceOrder = ["iphone", "ipad"]
        let sizeOrder = ["edgeToEdge", "standard", "splitView"]
        let orientationOrder = ["portrait", "landscape"]

        var fallback: ChosenOrientation?

        for device in deviceOrder {
            guard let deviceNode = representations[device] as? [String: Any] else { continue }
            for size in sizeOrder {
                guard let sizeNode = deviceNode[size] as? [String: Any] else { continue }
                for orientation in orientationOrder {
                    guard let node = sizeNode[orientation] as? [String: Any],
                          let mapping = readSize(node["mappingSize"]),
                          let items = node["items"] as? [[String: Any]],
                          !items.isEmpty else { continue }
                    let path = "\(device)/\(size)/\(orientation)"
                    let chosen = ChosenOrientation(path: path, mappingSize: mapping, items: items)
                    if device == "iphone", size == "edgeToEdge", orientation == "portrait" {
                        return chosen
                    }
                    if device == "iphone", size == "standard", orientation == "portrait",
                       fallback == nil {
                        fallback = chosen
                    } else if fallback == nil {
                        fallback = chosen
                    }
                }
            }
        }
        return fallback
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
