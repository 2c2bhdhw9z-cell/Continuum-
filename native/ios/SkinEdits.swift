// Continuum - edits to an imported skin, kept as an overlay on top of the file.
//
// THE IMPORTED SKIN IS NEVER CHANGED. What the user drags in the skin editor is stored here, per
// system and per orientation, as a list of overrides keyed by the position of the item in the
// imported face. The player asks `EngineHost` for a skin face and gets the file's values with
// these laid over them; "Reset to the file" is deleting the overlay. Re-importing or clearing a
// skin drops its overlay, because an index into a different file means nothing.
//
// Only the Swift data model is touched. The renderer still receives holes the way it always has,
// through `applySkinHoles`, so nothing in the Rust compositor knows an edit happened.

import CoreGraphics
import Foundation

/// Overrides for one orientation of one imported skin.
struct SkinFaceEdits: Equatable, Sendable {
    /// Hit area overrides, keyed by the index (as a string) in the imported face's `buttons`.
    var buttonFrames: [String: DeltaSkinNormalizedRect] = [:]
    /// Input overrides, same keys, values are `PadSlot.layoutKey`. The D-pad entry is never here.
    var buttonSlots: [String: String] = [:]
    /// Analog stick rect overrides, keyed by index in `sticks`.
    var stickFrames: [String: DeltaSkinNormalizedRect] = [:]
    /// Screen hole overrides, keyed by index in the face's effective screens.
    var screenFrames: [String: DeltaSkinNormalizedRect] = [:]
    /// Skin art opacity, `0.2...1`. Nil means the file as drawn, fully opaque.
    var opacity: Double?

    static let minOpacity = 0.2

    var isEmpty: Bool {
        buttonFrames.isEmpty && buttonSlots.isEmpty && stickFrames.isEmpty
            && screenFrames.isEmpty && opacity == nil
    }

    var effectiveOpacity: Double {
        guard let opacity, opacity.isFinite else { return 1 }
        return min(1, max(Self.minOpacity, opacity))
    }

    /// The face with every override laid over it. Out-of-range indices are ignored, so an overlay
    /// that somehow outlived its file can do no harm.
    func applied(to face: SkinEditableFace) -> SkinEditableFace {
        var out = face
        for (key, frame) in buttonFrames {
            guard let index = Int(key), out.buttons.indices.contains(index) else { continue }
            let clean = frame.clampedToCanvas
            out.buttons[index].x = clean.x
            out.buttons[index].y = clean.y
            out.buttons[index].width = clean.width
            out.buttons[index].height = clean.height
            // The D-pad is drawn from the button entry and placed from `dpadFrame`, so moving one
            // has to move the other or the art and the hit area would separate.
            if out.buttons[index].slot == "dpad" {
                out.dpadFrame = clean
            }
        }
        for (key, slot) in buttonSlots {
            guard let index = Int(key), out.buttons.indices.contains(index),
                  out.buttons[index].slot != "dpad",
                  PadSlot.allCases.contains(where: { $0.layoutKey == slot }) else { continue }
            out.buttons[index].slot = slot
        }
        for (key, frame) in stickFrames {
            guard let index = Int(key), out.sticks.indices.contains(index) else { continue }
            let clean = frame.clampedToCanvas
            out.sticks[index].x = clean.x
            out.sticks[index].y = clean.y
            out.sticks[index].width = clean.width
            out.sticks[index].height = clean.height
        }
        for (key, frame) in screenFrames {
            guard let index = Int(key), out.screens.indices.contains(index) else { continue }
            out.screens[index].output = frame.clampedToCanvas
        }
        out.opacity = effectiveOpacity
        return out
    }
}

extension SkinFaceEdits: Codable {
    private enum CodingKeys: String, CodingKey {
        case buttonFrames, buttonSlots, stickFrames, screenFrames, opacity
    }

    /// Forgiving per field, the rule every stored layout in this app follows.
    init(from decoder: Decoder) throws {
        let box = try decoder.container(keyedBy: CodingKeys.self)
        buttonFrames = (try? box.decodeIfPresent([String: DeltaSkinNormalizedRect].self,
                                                 forKey: .buttonFrames)) ?? [:]
        buttonSlots = (try? box.decodeIfPresent([String: String].self, forKey: .buttonSlots))
            ?? [:]
        stickFrames = (try? box.decodeIfPresent([String: DeltaSkinNormalizedRect].self,
                                                forKey: .stickFrames)) ?? [:]
        screenFrames = (try? box.decodeIfPresent([String: DeltaSkinNormalizedRect].self,
                                                 forKey: .screenFrames)) ?? [:]
        opacity = (try? box.decodeIfPresent(Double.self, forKey: .opacity)) ?? nil
    }

    func encode(to encoder: Encoder) throws {
        var box = encoder.container(keyedBy: CodingKeys.self)
        try box.encode(buttonFrames, forKey: .buttonFrames)
        try box.encode(buttonSlots, forKey: .buttonSlots)
        try box.encode(stickFrames, forKey: .stickFrames)
        try box.encode(screenFrames, forKey: .screenFrames)
        try box.encodeIfPresent(opacity, forKey: .opacity)
    }
}

/// Both orientations' overrides for one system's skin.
struct SkinEdits: Equatable, Sendable {
    var portrait = SkinFaceEdits()
    var landscape = SkinFaceEdits()

    func face(landscape wide: Bool) -> SkinFaceEdits { wide ? landscape : portrait }

    mutating func setFace(_ edits: SkinFaceEdits, landscape wide: Bool) {
        if wide { landscape = edits } else { portrait = edits }
    }

    var isEmpty: Bool { portrait.isEmpty && landscape.isEmpty }
}

extension SkinEdits: Codable {
    private enum CodingKeys: String, CodingKey { case portrait, landscape }

    init(from decoder: Decoder) throws {
        let box = try decoder.container(keyedBy: CodingKeys.self)
        portrait = (try? box.decodeIfPresent(SkinFaceEdits.self, forKey: .portrait))
            ?? SkinFaceEdits()
        landscape = (try? box.decodeIfPresent(SkinFaceEdits.self, forKey: .landscape))
            ?? SkinFaceEdits()
    }

    func encode(to encoder: Encoder) throws {
        var box = encoder.container(keyedBy: CodingKeys.self)
        try box.encode(portrait, forKey: .portrait)
        try box.encode(landscape, forKey: .landscape)
    }
}

/// The parts of one orientation of an imported skin that the editor can change, in skin
/// fractions. Built by `EngineHost` from the stored file.
struct SkinEditableFace: Equatable, Sendable {
    var mapping: CGSize
    var screens: [DeltaSkinScreen]
    var buttons: [DeltaSkinButton]
    var sticks: [DeltaSkinStick]
    var dpadFrame: DeltaSkinNormalizedRect?
    /// Skin art opacity after edits. 1 on the file itself.
    var opacity: Double = 1
}

extension DeltaSkinNormalizedRect {
    /// Kept inside the skin canvas and at least a sliver in size, so no edit can lose a control off
    /// the edge of the art or shrink a hole to nothing the renderer would then refuse.
    var clampedToCanvas: DeltaSkinNormalizedRect {
        func finite(_ value: Double, _ fallback: Double) -> Double {
            value.isFinite ? value : fallback
        }
        let minSide = 0.02
        let w = min(1, max(minSide, finite(width, 0.1)))
        let h = min(1, max(minSide, finite(height, 0.1)))
        let x = min(1 - w, max(0, finite(self.x, 0)))
        let y = min(1 - h, max(0, finite(self.y, 0)))
        return DeltaSkinNormalizedRect(x: x, y: y, width: w, height: h)
    }
}
