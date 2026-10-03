// Continuum - skin screen holes, analog sticks, and the buttons a skin file actually names.
//
// The picture goes in the hole the skin measured (`screens[].outputFrame`), not in the
// fallback strip above the buttons. A second screen keeps its own hole and its own crop of
// the framebuffer (`inputFrame`). A thumbstick is an analog stick with its own rect. A
// pressed/highlight image is shown only when the file has one.

import CoreGraphics
import Foundation
import UIKit

/// One screen hole. `output` is a fraction of mappingSize (origin top-left).
/// `inputWidth <= 0` means the whole framebuffer (a one-screen skin).
struct DeltaSkinScreen: Codable, Equatable, Sendable {
    var output: DeltaSkinNormalizedRect
    var inputX: Double
    var inputY: Double
    var inputWidth: Double
    var inputHeight: Double

    var cropsFramebuffer: Bool { inputWidth > 0 && inputHeight > 0 }
}

/// A circle pad / thumbstick. `side` is "left" or "right". The rect is the skin's frame,
/// not the D-pad.
struct DeltaSkinStick: Codable, Equatable, Sendable {
    var x: Double
    var y: Double
    var width: Double
    var height: Double
    var side: String
    var assetFileName: String?
    var assetKind: DeltaSkinAssetKind?

    var frame: DeltaSkinNormalizedRect {
        DeltaSkinNormalizedRect(x: x, y: y, width: width, height: height)
    }
}

/// One skin button, already mapped onto this system's real input.
/// `pressedFileName` is nil when the file has no pressed/highlight image. Nothing is invented.
struct DeltaSkinButton: Codable, Equatable, Sendable {
    var slot: String
    var x: Double
    var y: Double
    var width: Double
    var height: Double
    var normalFileName: String?
    var normalKind: DeltaSkinAssetKind?
    var pressedFileName: String?
    var pressedKind: DeltaSkinAssetKind?
    /// A Manic function (`SkinFunction` raw value) this button runs instead of a game input.
    /// `slot` is then "function". Nil on every game button and on skins saved before functions.
    var function: String? = nil
    /// Manic switch fields: the "on" picture, the knob's two frames, momentary or latching.
    var toggle: DeltaSkinSwitch? = nil
    /// Further slots held together with `slot` (a skin item naming several buttons, such as
    /// A+B). Nil or empty for a single button.
    var comboSlots: [String]? = nil

    var frame: DeltaSkinNormalizedRect {
        DeltaSkinNormalizedRect(x: x, y: y, width: width, height: height)
    }

    /// The function, when this button runs one.
    var skinFunction: SkinFunction? { function.flatMap(SkinFunction.named) }

    /// True for a button the pad drives through the function/switch path rather than as a chip.
    var isSpecial: Bool { function != nil || toggle != nil || !(comboSlots ?? []).isEmpty }
}

/// What one info.json item was, before it is mapped onto a system's slots.
struct DeltaSkinRawItem: Sendable, Equatable {
    enum Kind: Equatable, Sendable {
        case leftStick
        case rightStick
        case dpad
        case touch
        case button(String)
    }

    var kind: Kind
    var frame: DeltaSkinNormalizedRect
    var normalFileName: String?
    var pressedFileName: String?
    var stickAssetFileName: String?
    /// Every input name the item listed, so a combo or a function is not cut down to one name.
    var names: [String] = []
    /// Manic switch fields, when the item is a switch.
    var toggle: DeltaSkinSwitch? = nil
}

/// Bytes for a per-button or thumbstick image pulled out of the package.
struct DeltaSkinPiece: Sendable {
    var fileName: String
    var kind: DeltaSkinAssetKind
    var data: Data
}

enum SkinControls {

    /// Pressed-state asset keys a Delta/Manic item actually uses. Order is the preference:
    /// an explicit pressed image wins over highlight, which wins over Delta's `selected`.
    static let pressedAssetKeys = ["pressed", "highlight", "highlighted", "selected"]

    /// Reads every screen hole. The first one is not the only one.
    static func screens(from raw: [[String: Any]], mappingSize: CGSize) -> [DeltaSkinScreen] {
        var out: [DeltaSkinScreen] = []
        for screen in raw {
            guard let frame = readFrame(screen["outputFrame"]),
                  let output = DeltaSkinNormalizedRect.from(frame: frame, mappingSize: mappingSize)
            else { continue }
            let input = readFrame(screen["inputFrame"])
            out.append(DeltaSkinScreen(
                output: output,
                inputX: Double(input?.minX ?? 0),
                inputY: Double(input?.minY ?? 0),
                inputWidth: Double(input?.width ?? 0),
                inputHeight: Double(input?.height ?? 0)
            ))
        }
        return out
    }

    /// Classifies one item. A thumbstick is a stick even though its `inputs` dictionary has
    /// up/down/left/right. That dictionary is not a D-pad.
    static func classify(_ item: [String: Any], mappingSize: CGSize) -> DeltaSkinRawItem? {
        guard let frame = readFrame(item["frame"]),
              let normalized = DeltaSkinNormalizedRect.from(frame: frame, mappingSize: mappingSize)
        else { return nil }
        let art = assetNames(item["asset"])
        if let side = stickSide(item) {
            let stickFile = thumbstickFile(item["thumbstick"])
            return DeltaSkinRawItem(
                kind: side == "right" ? .rightStick : .leftStick,
                frame: normalized,
                normalFileName: art.normal,
                pressedFileName: art.pressed,
                stickAssetFileName: stickFile
            )
        }
        if let inputs = item["inputs"] as? [String: Any] {
            let values = inputs.values.compactMap { $0 as? String }.map { canonical($0) }
            if values.contains(where: { $0.contains("touchscreen") }) {
                return DeltaSkinRawItem(kind: .touch, frame: normalized,
                                        normalFileName: nil, pressedFileName: nil,
                                        stickAssetFileName: nil)
            }
            if isPlainDpad(inputs) {
                return DeltaSkinRawItem(kind: .dpad, frame: normalized,
                                        normalFileName: art.normal, pressedFileName: art.pressed,
                                        stickAssetFileName: nil)
            }
        }
        let names = ManicItems.inputNames(item["inputs"])
        guard let first = names.first else { return nil }
        let toggle = ManicItems.toggle(item, itemFrame: frame)
        // A switch's `selected` picture is its "on" position, not a pressed picture.
        let pressed = toggle == nil
            ? art.pressed
            : assetNames(item["asset"], pressedKeys: ["pressed", "highlight", "highlighted"]).pressed
        return DeltaSkinRawItem(kind: .button(first), frame: normalized,
                                normalFileName: art.normal, pressedFileName: pressed,
                                stickAssetFileName: nil, names: names, toggle: toggle)
    }

    struct Resolved {
        var layout: TouchLayout
        var buttons: [DeltaSkinButton]
        var sticks: [DeltaSkinStick]
        var dpadFrame: DeltaSkinNormalizedRect?
        var applied: Int
        /// One plain line per item that was left out on purpose (a function mixed with inputs).
        var refused: [String] = []
        /// How many buttons run a function, and how many are switches.
        var functions: Int = 0
        var switches: Int = 0
    }

    /// Maps the skin's own controls onto the slots this system actually has.
    static func resolve(_ items: [DeltaSkinRawItem],
                        system: GameSystem,
                        assetKind: (String) -> DeltaSkinAssetKind) -> Resolved {
        var layout = TouchLayout.standard
        var frees: [String: ButtonFree] = [:]
        var buttons: [DeltaSkinButton] = []
        var sticks: [DeltaSkinStick] = []
        var dpad: DeltaSkinNormalizedRect?
        var applied = 0
        var refused: [String] = []
        var functionCount = 0
        var switchCount = 0

        for item in items {
            let centreX = item.frame.x + item.frame.width / 2
            let centreY = item.frame.y + item.frame.height / 2
            switch item.kind {
            case .leftStick, .rightStick:
                let side = item.kind == .rightStick ? "right" : "left"
                let file = item.stickAssetFileName
                sticks.append(DeltaSkinStick(
                    x: item.frame.x, y: item.frame.y,
                    width: item.frame.width, height: item.frame.height,
                    side: side,
                    assetFileName: file,
                    assetKind: file.map(assetKind)
                ))
                applied += 1
            case .dpad:
                layout.dpadX = centreX
                layout.dpadY = centreY
                dpad = item.frame
                buttons.append(DeltaSkinButton(
                    slot: "dpad",
                    x: item.frame.x, y: item.frame.y,
                    width: item.frame.width, height: item.frame.height,
                    normalFileName: item.normalFileName,
                    normalKind: item.normalFileName.map(assetKind),
                    pressedFileName: item.pressedFileName,
                    pressedKind: item.pressedFileName.map(assetKind)
                ))
                applied += 1
            case .touch:
                break
            case .button(let name):
                let names = item.names.isEmpty ? [name] : item.names
                if let refusal = ManicItems.refusal(for: names, systemID: system.rawValue) {
                    refused.append(refusal)
                    continue
                }
                if let function = ManicItems.function(for: names, systemID: system.rawValue) {
                    buttons.append(DeltaSkinButton(
                        slot: "function",
                        x: item.frame.x, y: item.frame.y,
                        width: item.frame.width, height: item.frame.height,
                        normalFileName: item.normalFileName,
                        normalKind: item.normalFileName.map(assetKind),
                        pressedFileName: item.pressedFileName,
                        pressedKind: item.pressedFileName.map(assetKind),
                        function: function.rawValue,
                        toggle: item.toggle
                    ))
                    functionCount += 1
                    if item.toggle != nil { switchCount += 1 }
                    applied += 1
                    continue
                }
                var slots: [PadSlot] = []
                for each in names {
                    if let found = slot(forSkinName: each, system: system), !slots.contains(found) {
                        slots.append(found)
                    }
                }
                guard let slot = slots.first else { continue }
                let combo = slots.dropFirst().map(\.layoutKey)
                if !combo.isEmpty || item.toggle != nil {
                    // A combo or a switch is its own control. It does not move the chip that
                    // the single button of the same name would use.
                    buttons.append(DeltaSkinButton(
                        slot: slot.layoutKey,
                        x: item.frame.x, y: item.frame.y,
                        width: item.frame.width, height: item.frame.height,
                        normalFileName: item.normalFileName,
                        normalKind: item.normalFileName.map(assetKind),
                        pressedFileName: item.pressedFileName,
                        pressedKind: item.pressedFileName.map(assetKind),
                        toggle: item.toggle,
                        comboSlots: combo.isEmpty ? nil : Array(combo)
                    ))
                    if item.toggle != nil { switchCount += 1 }
                    applied += 1
                    continue
                }
                switch slot {
                case .select:
                    layout.selectX = centreX
                    layout.selectY = centreY
                case .start:
                    layout.startX = centreX
                    layout.startY = centreY
                case .up, .down, .left, .right:
                    layout.dpadX = centreX
                    layout.dpadY = centreY
                    dpad = item.frame
                default:
                    frees[slot.layoutKey] = ButtonFree(x: centreX, y: centreY)
                }
                buttons.append(DeltaSkinButton(
                    slot: slot.layoutKey,
                    x: item.frame.x, y: item.frame.y,
                    width: item.frame.width, height: item.frame.height,
                    normalFileName: item.normalFileName,
                    normalKind: item.normalFileName.map(assetKind),
                    pressedFileName: item.pressedFileName,
                    pressedKind: item.pressedFileName.map(assetKind)
                ))
                applied += 1
            }
        }
        layout.buttonFrees = frees
        return Resolved(layout: layout.sanitised, buttons: buttons, sticks: sticks,
                        dpadFrame: dpad, applied: applied, refused: refused,
                        functions: functionCount, switches: switchCount)
    }

    /// The real slot for a name the skin used, on this system.
    ///
    /// A label match wins, so N64 "A" hits the button this app labels A (retro B), and GBA "A"
    /// hits retro A. Aliases only fill names the label does not use (`l1`, `zl`, `cUp`).
    static func slot(forSkinName name: String, system: GameSystem) -> PadSlot? {
        let key = canonical(name)
        if let control = system.controls.first(where: { canonical($0.label) == key }) {
            return control.slot
        }
        let candidates = aliases[key] ?? [key]
        for cand in candidates {
            if let control = system.controls.first(where: {
                $0.slot.layoutKey == cand || canonical($0.label) == cand
            }) {
                return control.slot
            }
            // A real retro slot counts even when this system's procedural pad does not draw
            // it. The 3DS Home button is L3 and is not a chip; a skin that declares `menu`
            // still has to land on that input.
            if let slot = PadSlot.allCases.first(where: { $0.layoutKey == cand }) {
                return slot
            }
        }
        return nil
    }

    /// Skin token -> candidate layout keys / labels, most specific first.
    static let aliases: [String: [String]] = [
        "l1": ["l"],
        "r1": ["r"],
        "l2": ["l2", "zl"],
        "r2": ["r2", "zr"],
        "zl": ["zl", "l2"],
        "zr": ["zr", "r2"],
        "cross": ["cross", "b"],
        "circle": ["circle", "a"],
        "square": ["square", "y"],
        "triangle": ["triangle", "x"],
        "cup": ["c↑", "x"],
        "cdown": ["c↓", "a"],
        "cleft": ["c←", "l"],
        "cright": ["c→", "r"],
        "z": ["z", "l2"],
        "trigger": ["z", "l2"],
        "menu": ["l3", "home"],
        "home": ["l3", "menu"],
    ]

    static func canonical(_ name: String) -> String {
        name.trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()
            .replacingOccurrences(of: " ", with: "")
            .replacingOccurrences(of: "_", with: "")
            .replacingOccurrences(of: "-", with: "")
    }

    // MARK: item shape

    private static func stickSide(_ item: [String: Any]) -> String? {
        let blob = inputBlob(item)
        let hasThumbstickKey = item["thumbstick"] != nil
        if blob.contains("rightthumbstick") || blob.contains("rightstick") || blob.contains("cstick") {
            return "right"
        }
        if hasThumbstickKey || blob.contains("leftthumbstick") || blob.contains("analogstick")
            || blob.contains("circlepad") || blob.contains("thumbstick") {
            return "left"
        }
        return nil
    }

    /// A directional dictionary whose values are the D-pad itself, not a stick.
    private static func isPlainDpad(_ inputs: [String: Any]) -> Bool {
        let keys = Set(inputs.keys.map { canonical($0) })
        guard keys.contains("up") && keys.contains("down")
                && keys.contains("left") && keys.contains("right") else { return false }
        let values = inputs.values.compactMap { $0 as? String }.map { canonical($0) }
        if values.contains(where: {
            $0.contains("thumbstick") || $0.contains("analog") || $0.contains("stick")
        }) {
            return false
        }
        return true
    }

    private static func inputBlob(_ item: [String: Any]) -> String {
        var parts: [String] = []
        if let inputs = item["inputs"] as? [String: Any] {
            for (key, value) in inputs {
                parts.append(key)
                if let text = value as? String { parts.append(text) }
            }
        }
        if let list = item["inputs"] as? [String] {
            parts.append(contentsOf: list)
        }
        if let text = item["inputs"] as? String { parts.append(text) }
        return parts.map { canonical($0) }.joined(separator: " ")
    }

    private static func assetNames(_ value: Any?,
                                   pressedKeys: [String] = pressedAssetKeys) -> (normal: String?, pressed: String?) {
        guard let box = value as? [String: Any] else { return (nil, nil) }
        let normal = (box["normal"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines)
        var pressed: String?
        for key in pressedKeys {
            if let name = (box[key] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines),
               !name.isEmpty {
                pressed = name
                break
            }
        }
        let cleanNormal = (normal?.isEmpty == false) ? normal : nil
        return (cleanNormal, pressed)
    }

    private static func thumbstickFile(_ value: Any?) -> String? {
        if let name = value as? String {
            let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
            return trimmed.isEmpty ? nil : trimmed
        }
        if let box = value as? [String: Any] {
            for key in ["name", "normal", "image"] {
                if let name = (box[key] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines),
                   !name.isEmpty {
                    return name
                }
            }
        }
        return nil
    }

    private static func readFrame(_ value: Any?) -> CGRect? {
        guard let box = value as? [String: Any] else { return nil }
        func number(_ value: Any?) -> CGFloat {
            if let n = value as? NSNumber { return CGFloat(truncating: n) }
            if let d = value as? Double { return CGFloat(d) }
            if let i = value as? Int { return CGFloat(i) }
            if let s = value as? String, let d = Double(s) { return CGFloat(d) }
            return 0
        }
        let x = number(box["x"])
        let y = number(box["y"])
        let width = number(box["width"])
        let height = number(box["height"])
        guard width > 0, height > 0 else { return nil }
        return CGRect(x: x, y: y, width: width, height: height)
    }
}

/// Where the game view sits. A present hole is that hole. A missing hole is the fallback
/// strip above the buttons, and that strip is not used once a hole exists.
enum SkinPicturePlacement {
    struct Result: Equatable {
        var canvas: CGRect
        var holes: [CGRect]
        var usedFallback: Bool
    }

    static func place(screens: [DeltaSkinScreen],
                      mapping: CGSize,
                      in bounds: CGRect,
                      fallback: CGRect) -> Result {
        let canvas = DeltaSkinNormalizedRect.aspectFitCanvas(mapping: mapping, in: bounds)
        let holes = screens.map { $0.output.cgRect(in: canvas) }
            .filter { $0.width >= 1 && $0.height >= 1 }
        if holes.isEmpty {
            return Result(canvas: canvas, holes: [fallback], usedFallback: true)
        }
        return Result(canvas: canvas, holes: holes, usedFallback: false)
    }
}

/// One button's images. `pressed` is nil when the skin file had no pressed/highlight image.
struct SkinButtonImages {
    var normal: UIImage?
    var pressed: UIImage?
    /// A switch's "on" picture (`asset.selected`). Nil on plain buttons.
    var selected: UIImage? = nil
}

/// Everything the pad needs from one orientation of a skin, besides the background image.
struct SkinPadFace {
    var screens: [DeltaSkinScreen] = []
    var buttons: [DeltaSkinButton] = []
    var sticks: [DeltaSkinStick] = []
    var dpadFrame: DeltaSkinNormalizedRect?
    var buttonImages: [String: SkinButtonImages] = [:]
    var stickImages: [String: UIImage] = [:]
    var dpadImage: UIImage?
    var dpadPressedImage: UIImage?
    /// Skin art opacity from the in-app skin editor (`SkinFaceEdits.opacity`). 1 is the file.
    var opacity: Double = 1
    /// Images per button INDEX in `buttons`. Two buttons naming the same slot (or two function
    /// buttons) each keep their own picture.
    var imagesByIndex: [Int: SkinButtonImages] = [:]
    /// The skin this face came from, so a switch's remembered position belongs to one skin.
    var skinID: String = ""
    /// The skin's `sound.caf`, played on every press while Settings allows it. Nil for none.
    var soundURL: URL? = nil
}
