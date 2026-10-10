// Continuum - the parts of a Manic EMU skin item that Delta's format does not have.
//
// Manic extends Delta's `info.json` item with:
//   - `inputs` naming a FUNCTION (`quickSave`, `reverseScreens`...) instead of a game button;
//   - SWITCH buttons: `asset.selected` (the "on" picture), `animation` { type: spring, begin, end }
//     with begin and end frames relative to the item's own frame, `inputs` as a single string, and
//     `selfRetracting` for a momentary switch that springs back on release;
//   - `sound.caf` in the package root, played on every press.
//
// PURE FOUNDATION, so the parsing compiles and is checked with real `swiftc` on Linux. The UIKit
// side (drawing the knob, playing the sound) reads only the plain values produced here.

#if canImport(CoreGraphics)
import CoreGraphics
#endif
import Foundation

/// A switch button's extra fields. Frames are fractions of the ITEM's size, origin at the item's
/// top-left, so they survive the skin being scaled onto any screen.
struct DeltaSkinSwitch: Codable, Equatable, Sendable {
    /// The "on" picture. Nil means the normal picture is used in both positions.
    var selectedFileName: String?
    /// Where the knob sits when off. Nil means the whole item.
    var begin: DeltaSkinNormalizedRect?
    /// Where the knob sits when on. Nil means the whole item.
    var end: DeltaSkinNormalizedRect?
    /// Manic's `animation.type`. `spring` bounces; anything else eases.
    var spring: Bool
    /// True for a momentary switch: on while held, back to off on release.
    var selfRetracting: Bool

    init(selectedFileName: String? = nil, begin: DeltaSkinNormalizedRect? = nil,
         end: DeltaSkinNormalizedRect? = nil, spring: Bool = true, selfRetracting: Bool = false) {
        self.selectedFileName = selectedFileName
        self.begin = begin
        self.end = end
        self.spring = spring
        self.selfRetracting = selfRetracting
    }
}

enum ManicItems {
    /// The button sound's file name in the package root.
    static let soundFileName = "sound.caf"

    /// Every input name on the item: an array, a single string (Manic switches), or the values of
    /// a dictionary. Trimmed, empties dropped, order kept.
    static func inputNames(_ value: Any?) -> [String] {
        if let list = value as? [Any] {
            return list.compactMap { $0 as? String }
                .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
                .filter { !$0.isEmpty }
        }
        if let single = value as? String {
            let trimmed = single.trimmingCharacters(in: .whitespacesAndNewlines)
            return trimmed.isEmpty ? [] : [trimmed]
        }
        if let dict = value as? [String: Any] {
            return dict.keys.sorted().compactMap { dict[$0] as? String }
                .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
                .filter { !$0.isEmpty }
        }
        return []
    }

    /// True when the item is a Manic switch rather than a plain button.
    ///
    /// `animation` or `selfRetracting` say so outright. `asset.selected` alone is NOT enough: Delta
    /// skins have used `selected` as their pressed picture, and those must keep working. A
    /// `selected` picture together with `inputs` written as a single string is Manic's switch.
    static func isSwitch(_ item: [String: Any]) -> Bool {
        if item["animation"] != nil || item["selfRetracting"] != nil { return true }
        let asset = item["asset"] as? [String: Any]
        let selected = (asset?["selected"] as? String)?.isEmpty == false
        return selected && item["inputs"] is String
    }

    /// The switch fields for `item`, or nil when it is not a switch. `itemFrame` is the item's own
    /// frame in mapping points, which the begin and end frames are relative to.
    static func toggle(_ item: [String: Any], itemFrame: CGRect) -> DeltaSkinSwitch? {
        guard isSwitch(item) else { return nil }
        let asset = item["asset"] as? [String: Any]
        let selected = (asset?["selected"] as? String)?
            .trimmingCharacters(in: .whitespacesAndNewlines)
        let animation = item["animation"] as? [String: Any]
        let type = (animation?["type"] as? String)?.lowercased() ?? "spring"
        let begin = relativeFrame(animation?["begin"] ?? animation?["beginFrame"],
                                  itemFrame: itemFrame)
        let end = relativeFrame(animation?["end"] ?? animation?["endFrame"], itemFrame: itemFrame)
        return DeltaSkinSwitch(
            selectedFileName: (selected?.isEmpty == false) ? selected : nil,
            begin: begin,
            end: end,
            spring: type == "spring",
            selfRetracting: bool(item["selfRetracting"])
        )
    }

    /// The function a button runs on `systemID`, or nil for a game button.
    ///
    /// Delta's `menu` opens the in-game menu, which here is the all-functions menu. On the 3DS
    /// `menu` stays the Home button, a real input, as it always was.
    static func function(for names: [String], systemID: String) -> SkinFunction? {
        for name in names {
            if let function = SkinFunction.named(name) { return function }
            if SkinFunction.canonical(name) == "menu", systemID != "n3ds" { return .flex }
        }
        return nil
    }

    /// Refuses a function mixed with other inputs (Manic's own rule). Nil when the item is fine.
    static func refusal(for names: [String], systemID: String) -> String? {
        guard names.count > 1, function(for: names, systemID: systemID) != nil else { return nil }
        return "a button names \(names.joined(separator: "+")); a function button may not have "
            + "other inputs, so it was left out"
    }

    // MARK: Helpers

    static func bool(_ value: Any?) -> Bool {
        if let flag = value as? Bool { return flag }
        if let number = value as? NSNumber { return number.intValue != 0 }
        if let text = value as? String {
            return ["true", "yes", "1"].contains(text.lowercased())
        }
        return false
    }

    static func number(_ value: Any?) -> CGFloat? {
        if let n = value as? NSNumber { return CGFloat(n.doubleValue) }
        if let d = value as? Double { return CGFloat(d) }
        if let i = value as? Int { return CGFloat(i) }
        if let s = value as? String, let d = Double(s) { return CGFloat(d) }
        return nil
    }

    /// A begin or end frame as a fraction of the item. A frame with no size takes the item's.
    static func relativeFrame(_ value: Any?, itemFrame: CGRect) -> DeltaSkinNormalizedRect? {
        guard let box = value as? [String: Any],
              itemFrame.width > 0, itemFrame.height > 0 else { return nil }
        let x = number(box["x"]) ?? 0
        let y = number(box["y"]) ?? 0
        let width = number(box["width"]).flatMap { $0 > 0 ? $0 : nil } ?? itemFrame.width
        let height = number(box["height"]).flatMap { $0 > 0 ? $0 : nil } ?? itemFrame.height
        return DeltaSkinNormalizedRect(
            x: Double(x / itemFrame.width),
            y: Double(y / itemFrame.height),
            width: Double(width / itemFrame.width),
            height: Double(height / itemFrame.height)
        )
    }
}
