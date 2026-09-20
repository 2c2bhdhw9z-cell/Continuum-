// Continuum - the on-screen pad, and the one place that knows the engine's input wire format.
//
// Until this file existed the app had no input at all. `apply_gamepad(port:buttons:axes:)` and
// `connectedPads()` were exported through UniFFI and nothing in Swift had ever called either, so
// five working cores ran real games that could not be started, paused or played.
//
// Three things in here are load-bearing and are explained where they are defined rather than
// here: the button array's ORDER (`PadSlot`), the D-pad being ONE surface rather than four
// buttons (`TouchControlsView.directions`), and real multi-touch (`TouchControlsView.grabs`).

import SwiftUI
import UIKit

// MARK: - The wire format

/// A slot in the button array that `engine.applyGamepad(port:buttons:axes:)` expects.
///
/// THE RAW VALUE IS THE ARRAY INDEX, AND THIS IS **NOT** LIBRETRO'S BUTTON ORDER.
///
/// That distinction is the single easiest thing to get wrong in this file, and getting it wrong
/// would not look broken: every button would simply do a different button's job, which reads as
/// a confusing game rather than as a bug. So the path was traced end to end rather than assumed:
///
/// ```text
///   uniffi_api.rs:548   apply_gamepad(port, buttons, axes)
///     -> bridge.rs:554    apply_gamepad(port, &buttons, &axes)
///       -> input/gamepad.rs  apply_standard_gamepad(port, buttons, axes)
/// ```
///
/// `apply_standard_gamepad` does not read the array positionally into libretro ids. It walks
/// `STANDARD_GAMEPAD_MAP` (gamepad.rs, the table just above it) and pulls `buttons[index]` for
/// each entry, and that table is the **W3C "standard gamepad" layout**. The order below is that
/// table, transcribed entry by entry.
///
/// For contrast, libretro's own order is the `Button` enum in `input/mod.rs`
/// (B 0, Y 1, Select 2, Start 3, Up 4, Down 5, Left 6, Right 7, A 8, X 9, L 10, R 11) and it is
/// what the core is finally asked for through `InputSnapshot::libretro_state`. It is NOT the wire
/// format of `apply_gamepad`. Sending it would put Y where A belongs and Start where X belongs.
///
/// Why the bottom face button is retro B and not retro A is worth restating, because it also
/// looks like a mistake: retro follows the Nintendo arrangement where A sits to the RIGHT of B,
/// so the bottom button of a diamond is B. The comment on `STANDARD_GAMEPAD_MAP` says the same.
enum PadSlot: Int, CaseIterable, Sendable {
    case b = 0
    case a = 1
    case y = 2
    case x = 3
    case l = 4
    case r = 5
    case l2 = 6
    case r2 = 7
    case select = 8
    case start = 9
    case l3 = 10
    case r3 = 11
    case up = 12
    case down = 13
    case left = 14
    case right = 15

    /// How many booleans the engine's table can index. Sixteen, and not a count of the buttons
    /// any one system happens to show.
    static let arrayLength = 16
}

/// One frame of pad state, in exactly the shape the engine wants.
///
/// Built as a fixed-size array rather than appended to, so a short or long `buttons` is not
/// expressible at any call site. The Rust side would tolerate a short one, because it reads
/// `buttons.get(index)` and treats a missing entry as unpressed, but relying on that would hide
/// the contract instead of stating it.
struct PadFrame: Sendable, Equatable {
    /// `AXIS_COUNT` in `input/mod.rs`: left_x, left_y, right_x, right_y.
    static let axisCount = 4

    let buttons: [Bool]
    let axes: [Float]

    /// Nothing held. What is pushed when no game is running, so a button cannot survive a
    /// session ending while a finger was down.
    static let released = PadFrame(pressed: [])

    init(pressed: Set<PadSlot>) {
        var slots = [Bool](repeating: false, count: PadSlot.arrayLength)
        for slot in pressed {
            slots[slot.rawValue] = true
        }
        buttons = slots

        // Zeroed, and that is correct rather than a stub. Stage 1 has no analog stick, and
        // `apply_standard_gamepad` ORs the stick-derived D-pad onto the button bits past
        // AXIS_DEADZONE (0.35), so a centred stick provably cannot cancel a D-pad press that
        // came from `buttons`. Sending nothing at all would behave identically; sending four
        // explicit zeroes makes the axis half of the contract visible at the call site.
        axes = [Float](repeating: 0, count: Self.axisCount)
    }
}

/// A live read-through to whichever `TouchControlsView` is on screen.
///
/// The render loop in `MetalCanvas` needs the pad state every frame, and the view that owns that
/// state is created and destroyed by SwiftUI. A small box that both sides hold, with a weak link
/// to the view, keeps the canvas from having to know anything about the view hierarchy, and makes
/// "no controls on screen" a released pad rather than a missing value to handle.
final class PadInputSource {
    weak var view: TouchControlsView?

    /// The state for the frame about to run. Released when there is no control surface.
    func currentFrame() -> PadFrame {
        view?.currentFrame() ?? .released
    }
}

// MARK: - What each system's pad actually has

/// Where a control sits. Clusters are laid out as units, never as absolute points, so one
/// description works in portrait and landscape and at every layout scale.
enum PadCluster: Sendable {
    /// The direction surface, bottom left.
    case dpad
    /// The face buttons, bottom right.
    case face
    /// Shoulder pills, stacked above the D-pad.
    case shoulderLeft
    /// Shoulder pills, stacked above the face buttons.
    case shoulderRight
    /// SELECT and START, in their own row along the bottom.
    case system
}

/// How a control is drawn.
enum PadControlShape: Sendable {
    /// A circle. Face buttons.
    case round
    /// A rounded rectangle. Shoulders, SELECT and START.
    case pill
}

/// One button on screen: what it is called, which engine slot it sends, and where it sits.
///
/// `offset` is in units from its cluster's centre, with +x right and +y down to match UIKit.
/// Written out per control rather than derived from an index, because a diamond that is correct
/// for SNES has to be correct for PS1 too, and an explicit offset can be checked by eye against
/// the real hardware while an index formula cannot.
struct PadControl: Sendable {
    let slot: PadSlot
    let label: String
    let cluster: PadCluster
    let shape: PadControlShape
    let offset: CGPoint
}

/// The systems this build can run, as the pad layout sees them.
///
/// This is the dimension `CoreCatalog`'s routing table was missing. A core id cannot drive the
/// layout on its own: mgba is GBA plus GB plus GBC and genesis_plus_gx is Mega Drive plus Master
/// System plus Game Gear, and those want different button counts.
enum GameSystem: String, Sendable, CaseIterable {
    case nes
    case snes
    case gb
    case gbc
    case gba
    case sms
    case gg
    case genesis
    case ps1

    /// The short code a library card badges itself with.
    var badge: String {
        switch self {
        case .nes: return "NES"
        case .snes: return "SNES"
        case .gb: return "GB"
        case .gbc: return "GBC"
        case .gba: return "GBA"
        case .sms: return "SMS"
        case .gg: return "GG"
        case .genesis: return "MD"
        case .ps1: return "PS1"
        }
    }

    var displayName: String {
        switch self {
        case .nes: return "Nintendo Entertainment System"
        case .snes: return "Super Nintendo"
        case .gb: return "Game Boy"
        case .gbc: return "Game Boy Color"
        case .gba: return "Game Boy Advance"
        case .sms: return "Master System"
        case .gg: return "Game Gear"
        case .genesis: return "Mega Drive"
        case .ps1: return "PlayStation"
        }
    }

    /// The controls to draw, and the engine slot each one sends.
    ///
    /// EVERY FACE MAPPING HERE WAS READ FROM THE CORE'S OWN SOURCE, NOT FROM THE BUTTON'S NAME.
    /// Two of them are not what the name suggests, and both were corrected by doing this:
    ///
    ///   - Mega Drive A, B, C are retro **Y, B, A**. From Genesis-Plus-GX
    ///     `libretro/libretro.c`: the input descriptors declare JOYPAD_Y as "A", JOYPAD_B as "B"
    ///     and JOYPAD_A as "C", and the `DEVICE_PAD3B` case (which falls through to
    ///     `DEVICE_PAD2B`) sets INPUT_A from JOYPAD_Y, INPUT_B from JOYPAD_B and INPUT_C from
    ///     JOYPAD_A. Labelling three buttons A, B, C and sending retro A, B, C would be wrong on
    ///     two of the three.
    ///   - Master System and Game Gear buttons 1 and 2 are retro **B and A**, from the same
    ///     two-button path, with Start doubling as the Game Gear's Start and the Master System's
    ///     Pause.
    ///   - PS1 Cross, Circle, Square, Triangle are retro **B, A, Y, X**. From pcsx_rearmed
    ///     `frontend/libretro.c`, where the descriptors read JOYPAD_B "Cross", JOYPAD_A "Circle",
    ///     JOYPAD_Y "Square", JOYPAD_X "Triangle".
    ///
    /// NES, SNES, GB, GBC and GBA use the retro names directly, so B is B and A is A there.
    ///
    /// No analog sticks in Stage 1. PS1 gets a digital pad, which is what the vast majority of
    /// PS1 games were designed around and what Crash Bandicoot wants; an analog stick needs a
    /// second surface and `RETRO_DEVICE_ANALOG` reporting, and it is recorded as later work
    /// rather than half-drawn here.
    var controls: [PadControl] {
        switch self {
        case .nes, .gb, .gbc:
            return Self.twoFace(right: (.a, "A"), left: (.b, "B")) + Self.selectStart

        case .gba:
            return Self.twoFace(right: (.a, "A"), left: (.b, "B"))
                + Self.shoulders(left: [(.l, "L")], right: [(.r, "R")])
                + Self.selectStart

        case .snes:
            return Self.diamondFace(top: (.x, "X"), right: (.a, "A"),
                                    bottom: (.b, "B"), left: (.y, "Y"))
                + Self.shoulders(left: [(.l, "L")], right: [(.r, "R")])
                + Self.selectStart

        case .genesis:
            // Three buttons in a shallow arc, as the real pad had them. Labels are the Mega
            // Drive's; the slots are retro Y, B, A in that order. See the note above.
            return Self.threeFace(left: (.y, "A"), middle: (.b, "B"), right: (.a, "C"))
                + [PadControl(slot: .start, label: "START", cluster: .system,
                              shape: .pill, offset: CGPoint(x: 0, y: 0))]

        case .sms, .gg:
            // Two buttons and Start. The Master System's console-mounted Pause arrives as Start,
            // which is the only way a core can offer it.
            return Self.twoFace(right: (.a, "2"), left: (.b, "1"))
                + [PadControl(slot: .start, label: "START", cluster: .system,
                              shape: .pill, offset: CGPoint(x: 0, y: 0))]

        case .ps1:
            return Self.diamondFace(top: (.x, "Triangle"), right: (.a, "Circle"),
                                    bottom: (.b, "Cross"), left: (.y, "Square"))
                + Self.shoulders(left: [(.l, "L1"), (.l2, "L2")],
                                 right: [(.r, "R1"), (.r2, "R2")])
                + Self.selectStart
        }
    }

    /// How many controls this system draws, D-pad excluded. Used by the diagnostic line so the
    /// per-system layout is legible on device without counting glyphs.
    var controlCount: Int { controls.count }

    // ------------------------------------------------------------------ cluster templates
    //
    // The offsets below are the whole no-overlap argument at cluster level, so they are stated
    // once. A face button is `faceDiameter` (1.4) units across, so its radius is 0.7, and two
    // buttons clear each other whenever their centres are more than 1.4 units apart.

    private static func twoFace(right: (PadSlot, String),
                                left: (PadSlot, String)) -> [PadControl] {
        // A diagonal pair, the way a NES or Game Boy pad angles them. Centres are
        // hypot(1.7, 0.9) = 1.92 units apart, clear of 1.4.
        [
            PadControl(slot: left.0, label: left.1, cluster: .face,
                       shape: .round, offset: CGPoint(x: -0.85, y: 0.45)),
            PadControl(slot: right.0, label: right.1, cluster: .face,
                       shape: .round, offset: CGPoint(x: 0.85, y: -0.45)),
        ]
    }

    private static func threeFace(left: (PadSlot, String),
                                  middle: (PadSlot, String),
                                  right: (PadSlot, String)) -> [PadControl] {
        // A shallow arc. Adjacent centres are hypot(1.6, 0.3) = 1.63 units apart.
        [
            PadControl(slot: left.0, label: left.1, cluster: .face,
                       shape: .round, offset: CGPoint(x: -1.6, y: 0.3)),
            PadControl(slot: middle.0, label: middle.1, cluster: .face,
                       shape: .round, offset: CGPoint(x: 0, y: 0)),
            PadControl(slot: right.0, label: right.1, cluster: .face,
                       shape: .round, offset: CGPoint(x: 1.6, y: -0.3)),
        ]
    }

    private static func diamondFace(top: (PadSlot, String),
                                    right: (PadSlot, String),
                                    bottom: (PadSlot, String),
                                    left: (PadSlot, String)) -> [PadControl] {
        // Adjacent centres are hypot(1.15, 1.15) = 1.63 units apart, clear of 1.4. The diamond
        // is described by POSITION, which is why one template serves both SNES (X top, A right,
        // B bottom, Y left) and PS1 (Triangle top, Circle right, Cross bottom, Square left):
        // the two consoles put the same retro slots in the same places.
        [
            PadControl(slot: top.0, label: top.1, cluster: .face,
                       shape: .round, offset: CGPoint(x: 0, y: -1.15)),
            PadControl(slot: right.0, label: right.1, cluster: .face,
                       shape: .round, offset: CGPoint(x: 1.15, y: 0)),
            PadControl(slot: bottom.0, label: bottom.1, cluster: .face,
                       shape: .round, offset: CGPoint(x: 0, y: 1.15)),
            PadControl(slot: left.0, label: left.1, cluster: .face,
                       shape: .round, offset: CGPoint(x: -1.15, y: 0)),
        ]
    }

    private static func shoulders(left: [(PadSlot, String)],
                                  right: [(PadSlot, String)]) -> [PadControl] {
        // Stacked upward from the cluster anchor, 1.3 units apart, which clears a 1.0 unit tall
        // pill with 0.3 to spare.
        var out: [PadControl] = []
        for (index, entry) in left.enumerated() {
            let y: CGFloat = -1.3 * CGFloat(index)
            out.append(PadControl(slot: entry.0, label: entry.1, cluster: .shoulderLeft,
                                  shape: .pill, offset: CGPoint(x: 0, y: y)))
        }
        for (index, entry) in right.enumerated() {
            let y: CGFloat = -1.3 * CGFloat(index)
            out.append(PadControl(slot: entry.0, label: entry.1, cluster: .shoulderRight,
                                  shape: .pill, offset: CGPoint(x: 0, y: y)))
        }
        return out
    }

    /// SELECT and START, side by side. Centres are 2.85 units apart, clear of a 2.6 unit pill.
    private static let selectStart: [PadControl] = [
        PadControl(slot: .select, label: "SELECT", cluster: .system,
                   shape: .pill, offset: CGPoint(x: -1.425, y: 0)),
        PadControl(slot: .start, label: "START", cluster: .system,
                   shape: .pill, offset: CGPoint(x: 1.425, y: 0)),
    ]
}

// MARK: - Where the controls sit

/// The six numbers that describe a control layout.
///
/// Deliberately the SAME six the browser build used (`web/src/data/touch-layout.js`), with the
/// same limits, because the intention is to let the user rearrange the controls. Stage 1 ships no
/// editor and only ever uses `standard`, but taking the layout as a value now means that editor
/// is a view that writes six numbers rather than a rewrite of the layout engine.
///
/// The limits are not cosmetic margins. A control centred at 0 would be half off screen, and on
/// iOS the outer few millimetres belong to the system's edge gestures, so a control parked there
/// would fight the OS for the touch.
struct TouchLayout: Sendable, Equatable {
    static let minScale = 0.7
    static let maxScale = 1.6
    static let minOpacity = 0.15
    static let maxOpacity = 1.0
    static let minX = 0.08
    static let maxX = 0.92
    static let minY = 0.12
    static let maxY = 0.9

    var scale: Double
    var opacity: Double
    var dpadX: Double
    var dpadY: Double
    var faceX: Double
    var faceY: Double

    /// The default, tuned for a phone held at the bottom corners.
    ///
    /// The browser's defaults put the clusters at y 0.66, which suited a desktop-shaped viewport.
    /// On a phone that leaves the controls short of the thumbs, so they sit lower here. The model
    /// and its limits are carried over unchanged; only this one number is retuned.
    static let standard = TouchLayout(scale: 1.0, opacity: 0.55,
                                      dpadX: 0.17, dpadY: 0.78,
                                      faceX: 0.83, faceY: 0.78)

    /// Forces any layout into range. Applied on every read, not only on write, so a layout
    /// restored from storage by a future build cannot put a control off screen.
    var sanitised: TouchLayout {
        TouchLayout(
            scale: Self.clamp(scale, Self.minScale, Self.maxScale, Self.standard.scale),
            opacity: Self.clamp(opacity, Self.minOpacity, Self.maxOpacity, Self.standard.opacity),
            dpadX: Self.clamp(dpadX, Self.minX, Self.maxX, Self.standard.dpadX),
            dpadY: Self.clamp(dpadY, Self.minY, Self.maxY, Self.standard.dpadY),
            faceX: Self.clamp(faceX, Self.minX, Self.maxX, Self.standard.faceX),
            faceY: Self.clamp(faceY, Self.minY, Self.maxY, Self.standard.faceY)
        )
    }

    private static func clamp(_ value: Double, _ low: Double, _ high: Double,
                              _ fallback: Double) -> Double {
        guard value.isFinite else { return fallback }
        return min(high, max(low, value))
    }
}

// MARK: - The controls themselves

/// One button: a rounded background and a label, and nothing that can be touched.
///
/// `isUserInteractionEnabled` is false on purpose. Every touch has to reach the parent, because
/// the parent is the only thing that can see two fingers at once and decide what each is holding.
/// A chip that handled its own touches would break multi-touch and the D-pad surface both.
final class ControlChip: UIView {
    let control: PadControl

    private let title = UILabel()

    init(control: PadControl) {
        self.control = control
        super.init(frame: .zero)
        isUserInteractionEnabled = false
        layer.borderWidth = 1
        layer.borderColor = UIColor.white.withAlphaComponent(0.28).cgColor
        backgroundColor = UIColor.white.withAlphaComponent(0.10)

        title.text = control.label
        title.textAlignment = .center
        title.adjustsFontSizeToFitWidth = true
        title.minimumScaleFactor = 0.5
        title.textColor = UIColor.white.withAlphaComponent(0.88)
        title.isUserInteractionEnabled = false
        addSubview(title)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not supported") }

    override func layoutSubviews() {
        super.layoutSubviews()
        title.frame = bounds.insetBy(dx: 3, dy: 3)
        // Half the height rounds a square into a circle and a wide rect into a pill, so one line
        // covers both shapes.
        layer.cornerRadius = control.shape == .round
            ? bounds.height / 2
            : min(bounds.height / 2, 12)
        let side = min(bounds.width, bounds.height)
        title.font = .systemFont(ofSize: max(9, side * (control.shape == .round ? 0.34 : 0.28)),
                                 weight: .semibold)
    }

    func setPressed(_ pressed: Bool) {
        backgroundColor = UIColor.white.withAlphaComponent(pressed ? 0.34 : 0.10)
        layer.borderColor = UIColor.white
            .withAlphaComponent(pressed ? 0.70 : 0.28).cgColor
    }
}

/// The direction surface: drawn as a cross, treated as one region.
///
/// The four arms are decoration only. Nothing here hit-tests them, because a press is decided
/// from WHERE on the whole pad the finger is, which is the only way a thumb resting between Up
/// and Right can mean both.
final class DPadView: UIView {
    private let upArm = UIView()
    private let downArm = UIView()
    private let leftArm = UIView()
    private let rightArm = UIView()
    private let hub = UIView()

    override init(frame: CGRect) {
        super.init(frame: frame)
        isUserInteractionEnabled = false
        for arm in [upArm, downArm, leftArm, rightArm, hub] {
            arm.isUserInteractionEnabled = false
            arm.backgroundColor = UIColor.white.withAlphaComponent(0.10)
            arm.layer.borderWidth = 1
            arm.layer.borderColor = UIColor.white.withAlphaComponent(0.28).cgColor
            arm.layer.cornerRadius = 6
            addSubview(arm)
        }
        // The hub sits on top so the seams between the arms do not read as gaps.
        hub.layer.borderWidth = 0
        bringSubviewToFront(hub)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not supported") }

    override func layoutSubviews() {
        super.layoutSubviews()
        // A 3 by 3 grid. The pad's bounds are 3 units square, so each cell is one unit.
        let cell = CGSize(width: bounds.width / 3, height: bounds.height / 3)
        upArm.frame = CGRect(x: cell.width, y: 0, width: cell.width, height: cell.height)
        downArm.frame = CGRect(x: cell.width, y: cell.height * 2,
                               width: cell.width, height: cell.height)
        leftArm.frame = CGRect(x: 0, y: cell.height, width: cell.width, height: cell.height)
        rightArm.frame = CGRect(x: cell.width * 2, y: cell.height,
                                width: cell.width, height: cell.height)
        hub.frame = CGRect(x: cell.width, y: cell.height, width: cell.width, height: cell.height)
    }

    func setDirections(up: Bool, down: Bool, left: Bool, right: Bool) {
        apply(up, to: upArm)
        apply(down, to: downArm)
        apply(left, to: leftArm)
        apply(right, to: rightArm)
    }

    private func apply(_ pressed: Bool, to arm: UIView) {
        arm.backgroundColor = UIColor.white.withAlphaComponent(pressed ? 0.34 : 0.10)
        arm.layer.borderColor = UIColor.white
            .withAlphaComponent(pressed ? 0.70 : 0.28).cgColor
    }
}

// MARK: - The surface that reads the fingers

/// The whole on-screen pad: layout, drawing and every touch.
///
/// UIKit rather than SwiftUI gestures, and that is a decision rather than a preference. This view
/// needs to know about several fingers at once, needs to keep tracking a finger that has moved
/// off the control it started on, and needs to decide a D-pad direction from a position inside a
/// region. `touchesBegan`/`Moved`/`Ended`/`Cancelled` with `isMultipleTouchEnabled` gives all
/// three directly. Layering separate SwiftUI drag gestures would leave their interaction with
/// each other as the thing the whole feature depends on.
final class TouchControlsView: UIView {

    // ------------------------------------------------------------------ tuning

    // Every number in this block is CGFloat on purpose. All of it feeds CGRect and CGPoint
    // arithmetic, and keeping one numeric type through the whole layout means there is no
    // Double-to-CGFloat conversion anywhere for a reviewer to have to check.

    /// Deadzone as a fraction of the D-pad's half extent. Below it, nothing is pressed.
    ///
    /// 0.22 and 0.42 below are carried over verbatim from the browser build
    /// (`web/src/engine/input.js`, `_attachDpadSurface`). SESSION_HANDOFF.md's appendix records
    /// the single-surface D-pad as a decision taken there, so these are transcribed rather than
    /// re-derived: they are the numbers that shipped and felt right.
    private static let dpadDeadzone: CGFloat = 0.22

    /// How far off-axis a press still counts as including that direction.
    private static let dpadDiagonalRatio: CGFloat = 0.42

    /// Face button diameter, in units. Every cluster offset in `GameSystem` clears this.
    private static let faceDiameter: CGFloat = 1.4
    private static let dpadSpan: CGFloat = 3.0
    private static let shoulderSize = CGSize(width: 2.6, height: 1.0)
    private static let systemSize = CGSize(width: 2.6, height: 0.95)

    /// Upper bound on a unit, so the pad does not become absurd on a large screen.
    private static let maxUnit: CGFloat = 62

    /// Where a cluster centre sits vertically in landscape, as a fraction of the play area.
    ///
    /// Overrides the layout's own y in landscape. A phone held sideways is gripped along the long
    /// edges with the thumbs near the middle, not at the bottom corners, and the column also has
    /// to fit a shoulder above the cluster and a system pill below it.
    private static let landscapeClusterY: CGFloat = 0.5

    /// How much of the play area's height the control band may claim. A landscape phone is short,
    /// so the band needs most of it; in portrait that much would put the shoulders halfway up the
    /// picture.
    private static let bandFractionPortrait: CGFloat = 0.44
    private static let bandFractionLandscape: CGFloat = 0.94

    /// Breathing room inside the safe area, so nothing sits against a rounded corner.
    private static let edgeMargin: CGFloat = 10

    /// Hit areas are grown by this much, because a thumb aiming at the edge of a button should
    /// still get it.
    private static let hitSlop: CGFloat = 5

    // ------------------------------------------------------------------ inputs

    var system: GameSystem {
        didSet {
            guard system != oldValue else { return }
            rebuild()
        }
    }

    var layout: TouchLayout {
        didSet {
            guard layout != oldValue else { return }
            alpha = CGFloat(layout.sanitised.opacity)
            setNeedsLayout()
        }
    }

    /// Every line this view has to say out loud. On a sideloaded build with no debugger a silent
    /// layout fault is invisible, so the one thing that can still go wrong after all the
    /// arithmetic (two controls overlapping) is reported rather than assumed away.
    var onDiagnostic: ((String) -> Void)?

    /// The rect the emulated picture may occupy without a control ever sitting on top of it, in
    /// this view's own coordinate space, which spans the whole window.
    ///
    /// Reported by the view that actually did the layout rather than recomputed by the player
    /// screen, because two copies of this arithmetic would eventually disagree and the symptom
    /// would be a control resting on the game.
    var onPictureArea: ((CGRect) -> Void)?

    // ------------------------------------------------------------------ state

    /// What one finger is holding.
    ///
    /// A chip grab is STICKY: it keeps its control until the finger lifts, even if the finger
    /// wanders off. That mirrors the browser's `setPointerCapture` and means thumb drift cannot
    /// release a button mid-jump. A D-pad grab carries its point and is recomputed on every move,
    /// which is what makes sliding from Up into Up-Right work.
    private enum Grab {
        case dpad(CGPoint)
        case chip(Int)
    }

    /// Keyed on `UITouch` identity, which is stable across began, moved, ended and cancelled and
    /// is therefore the only reliable way to know which finger let go of what.
    private var grabs: [ObjectIdentifier: Grab] = [:]

    private var chips: [ControlChip] = []
    private let dpad = DPadView()

    /// Hit rectangles in this view's coordinate space, rebuilt on every layout.
    private var chipRects: [CGRect] = []
    private var dpadRect: CGRect = .zero

    /// The frame the render loop reads. Recomputed from `grabs` on every touch event, never
    /// mutated incrementally, so two fingers on one button, a cancelled touch and a gesture
    /// recogniser stealing a sequence all resolve correctly with no counter to get wrong.
    private var padState: PadFrame = .released

    /// The last overlap report, so the same line is not repeated on every layout pass.
    private var lastOverlapReport: String?

    /// The last picture area published, so an unchanged layout does not resize the surface.
    private var lastPictureArea: CGRect?

    // ------------------------------------------------------------------ life cycle

    init(system: GameSystem, layout: TouchLayout) {
        self.system = system
        self.layout = layout.sanitised
        super.init(frame: .zero)
        // The reason this class exists. Without it UIKit delivers one touch and holding a
        // direction while pressing a face button, which is most of playing a game, is impossible.
        isMultipleTouchEnabled = true
        backgroundColor = .clear
        alpha = CGFloat(self.layout.opacity)
        addSubview(dpad)
        rebuild()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not supported") }

    override func willMove(toWindow newWindow: UIWindow?) {
        super.willMove(toWindow: newWindow)
        // Leaving the window with a finger down would otherwise leave that button held forever,
        // because no touch callback is coming.
        if newWindow == nil {
            releaseAll()
        }
    }

    override func safeAreaInsetsDidChange() {
        super.safeAreaInsetsDidChange()
        setNeedsLayout()
    }

    /// The state for the frame about to run.
    func currentFrame() -> PadFrame {
        padState
    }

    /// Drops every held button. Called when the player screen goes away, when the app leaves the
    /// foreground, and whenever the control set changes under a finger.
    func releaseAll() {
        grabs.removeAll()
        recompute()
    }

    // ------------------------------------------------------------------ building

    private func rebuild() {
        // A held button from the previous system's pad must not survive into the new one.
        grabs.removeAll()

        for chip in chips {
            chip.removeFromSuperview()
        }
        chips = system.controls.map { ControlChip(control: $0) }
        for chip in chips {
            addSubview(chip)
        }
        // The D-pad stays behind the chips, which only matters if a future layout puts them
        // close enough to touch.
        bringSubviewToFront(dpad)
        recompute()
        lastOverlapReport = nil
        setNeedsLayout()
    }

    // ------------------------------------------------------------------ layout

    override func layoutSubviews() {
        super.layoutSubviews()

        let safe = bounds.inset(by: safeAreaInsets)
        let play = safe.insetBy(dx: Self.edgeMargin, dy: Self.edgeMargin)
        guard play.width > 0, play.height > 0 else {
            // Nothing sane to lay out. Report it rather than leaving invisible controls that
            // silently swallow nothing: a zero play area means the safe area consumed the view.
            chipRects = [CGRect](repeating: .zero, count: chips.count)
            dpadRect = .zero
            for chip in chips { chip.frame = .zero }
            dpad.frame = .zero
            report("touch controls: no room to lay out, play area is "
                   + "\(Int(play.width))x\(Int(play.height))")
            return
        }

        let landscape = bounds.width > bounds.height
        let unit = self.unit(in: play, landscape: landscape)
        let faceExtent = self.faceHalfExtent()
        let hasSystemRow = system.controls.contains { $0.cluster == .system }

        // WHERE SELECT AND START GO IS ORIENTATION DEPENDENT, and that is what keeps the picture
        // clear in both.
        //
        // In PORTRAIT they sit in their own band along the bottom, and the thumb clusters are
        // clamped ABOVE it. Two disjoint bands makes "the system row cannot collide with a
        // cluster" true by construction rather than by luck, and the picture then takes the whole
        // area above the controls.
        //
        // In LANDSCAPE that same bottom band would run straight through the middle of the screen,
        // which is where the game is. So SELECT goes under the D-pad and START under the face
        // buttons, both inside their own column, leaving a clear column down the centre for the
        // picture.
        let systemBandHeight = (hasSystemRow && !landscape)
            ? Self.systemSize.height * unit + 0.5 * unit
            : 0
        let clusterArea = CGRect(x: play.minX, y: play.minY,
                                 width: play.width,
                                 height: max(0, play.height - systemBandHeight))

        let dpadHalf = Self.dpadSpan * unit / 2
        // In landscape the clusters sit higher in their columns, because a landscape grip puts the
        // thumbs at the middle of the edge rather than at the bottom corner, and because the
        // column has to hold a shoulder above and a system pill below.
        let live = layout.sanitised
        let clusterY: CGFloat? = landscape ? Self.landscapeClusterY : nil
        let dpadCentre = clampedCentre(
            fractionX: CGFloat(live.dpadX), fractionY: clusterY ?? CGFloat(live.dpadY),
            halfWidth: dpadHalf, halfHeight: dpadHalf,
            into: clusterArea, wholePlayArea: play
        )
        let faceCentre = clampedCentre(
            fractionX: CGFloat(live.faceX), fractionY: clusterY ?? CGFloat(live.faceY),
            halfWidth: faceExtent.width * unit, halfHeight: faceExtent.height * unit,
            into: clusterArea, wholePlayArea: play
        )

        dpadRect = CGRect(x: dpadCentre.x - dpadHalf, y: dpadCentre.y - dpadHalf,
                          width: dpadHalf * 2, height: dpadHalf * 2)
        dpad.frame = dpadRect

        // Shoulders are anchored to the cluster they belong with, above it, so they travel with it
        // and can never land on top of it.
        let shoulderLeftAnchor = CGPoint(
            x: dpadCentre.x,
            y: dpadCentre.y - dpadHalf - (Self.shoulderSize.height / 2 + 0.45) * unit
        )
        let shoulderRightAnchor = CGPoint(
            x: faceCentre.x,
            y: faceCentre.y - faceExtent.height * unit
                - (Self.shoulderSize.height / 2 + 0.45) * unit
        )

        chipRects = []
        chipRects.reserveCapacity(chips.count)
        for chip in chips {
            let control = chip.control
            let size = self.size(of: control, unit: unit)
            let centre: CGPoint

            switch control.cluster {
            case .dpad, .face:
                centre = CGPoint(x: faceCentre.x + control.offset.x * unit,
                                 y: faceCentre.y + control.offset.y * unit)
            case .shoulderLeft:
                centre = CGPoint(x: shoulderLeftAnchor.x + control.offset.x * unit,
                                 y: shoulderLeftAnchor.y + control.offset.y * unit)
            case .shoulderRight:
                centre = CGPoint(x: shoulderRightAnchor.x + control.offset.x * unit,
                                 y: shoulderRightAnchor.y + control.offset.y * unit)
            case .system:
                if landscape {
                    // SELECT to the D-pad's column, everything else (START) to the face column.
                    // The control's own offset is ignored here on purpose: those offsets describe
                    // the portrait row and would push a pill out of its column.
                    let inLeftColumn = control.slot == .select
                    let columnX = inLeftColumn ? dpadCentre.x : faceCentre.x
                    let columnBottom = inLeftColumn
                        ? dpadCentre.y + dpadHalf
                        : faceCentre.y + faceExtent.height * unit
                    centre = CGPoint(
                        x: columnX,
                        y: columnBottom + (Self.systemSize.height / 2 + 0.45) * unit
                    )
                } else {
                    centre = CGPoint(x: play.midX + control.offset.x * unit,
                                     y: play.maxY - Self.systemSize.height * unit / 2
                                        + control.offset.y * unit)
                }
            }

            var rect = CGRect(x: centre.x - size.width / 2, y: centre.y - size.height / 2,
                              width: size.width, height: size.height)
            // The last line of defence: whatever the arithmetic produced, it ends up on screen.
            rect = Self.clamp(rect, into: play)
            chip.frame = rect
            chipRects.append(rect)
        }

        verifyNoOverlap()
        publishPictureArea(landscape: landscape)
    }

    /// Hands the player screen the rect the picture may use.
    ///
    /// Derived from the control rects that were just laid out, so it cannot drift from them. In
    /// portrait the picture takes everything above the topmost control; in landscape it takes the
    /// column between the left group and the right group. Either way no control sits on the game,
    /// which is the requirement docs/mobile-player.png failed.
    private func publishPictureArea(landscape: Bool) {
        var allRects = chipRects
        allRects.append(dpadRect)
        let occupied = allRects.filter { !$0.isEmpty }
        guard !occupied.isEmpty else {
            deliverPictureArea(bounds)
            return
        }

        let area: CGRect
        if landscape {
            let midX = bounds.midX
            let leftEdge = occupied.filter { $0.midX < midX }.map(\.maxX).max() ?? bounds.minX
            let rightEdge = occupied.filter { $0.midX >= midX }.map(\.minX).min() ?? bounds.maxX
            let width = rightEdge - leftEdge
            // A degenerate column means the controls met in the middle. Fall back to the whole
            // view rather than handing the engine a zero or negative surface, and say so: a
            // surface of nothing would read on device as a black screen with no explanation.
            if width < bounds.width * 0.2 {
                report("touch layout: no clear column for the picture in landscape, "
                       + "the controls span \(Int(bounds.width - width)) of "
                       + "\(Int(bounds.width)) points; drawing full width instead")
                area = bounds
            } else {
                area = CGRect(x: leftEdge, y: bounds.minY, width: width, height: bounds.height)
            }
        } else {
            let topEdge = occupied.map(\.minY).min() ?? bounds.maxY
            let height = topEdge - bounds.minY
            if height < bounds.height * 0.2 {
                report("touch layout: no clear band for the picture in portrait, "
                       + "the controls reach \(Int(height)) points from the top of "
                       + "\(Int(bounds.height)); drawing full height instead")
                area = bounds
            } else {
                area = CGRect(x: bounds.minX, y: bounds.minY,
                              width: bounds.width, height: height)
            }
        }
        deliverPictureArea(area)
    }

    private func deliverPictureArea(_ area: CGRect) {
        // Only on a real change. The engine resizes its surface from this, and resizing every
        // layout pass would rebuild GPU targets for no reason.
        guard area != lastPictureArea else { return }
        lastPictureArea = area
        onPictureArea?(area)
    }

    /// The unit every control is sized and placed in.
    ///
    /// Derived from the space available and only THEN scaled, which is the actual fix for the
    /// layout bug in docs/mobile-player.png: that layout collided because the controls had fixed
    /// sizes and positions while the area they sat in did not.
    private func unit(in play: CGRect, landscape: Bool) -> CGFloat {
        let faceExtent = faceHalfExtent()
        // The widest row is the D-pad beside the face cluster, with a gap between them. In
        // landscape that gap is the picture's column, so it is asked for generously.
        let gapUnits: CGFloat = landscape ? 6 : 1
        let widthUnits = Self.dpadSpan + faceExtent.width * 2 + gapUnits
        // Tallest column: shoulders, a gap, the cluster, a gap, the system row. The same in both
        // orientations, because in landscape the system pill is stacked under the cluster rather
        // than laid out along the bottom.
        let shoulderRows = CGFloat(max(
            system.controls.filter { $0.cluster == .shoulderLeft }.count,
            system.controls.filter { $0.cluster == .shoulderRight }.count
        ))
        let heightUnits = shoulderRows * 1.3 + 0.45
            + faceExtent.height * 2 + 0.5
            + Self.systemSize.height + 0.4

        let bandFraction = landscape ? Self.bandFractionLandscape : Self.bandFractionPortrait
        let bandHeight = play.height * bandFraction

        // What exactly fills the band. Nothing is allowed past this.
        let fitUnit = min(play.width / max(widthUnits, 1), bandHeight / max(heightUnits, 1))
        // The default leaves headroom so scaling up has somewhere to go, and is capped so a
        // large screen does not get comedy controls.
        let baseUnit = min(fitUnit * 0.85, Self.maxUnit)
        return max(1, min(baseUnit * CGFloat(layout.sanitised.scale), fitUnit))
    }

    /// Half the face cluster's extent, in units, including the buttons' own radius.
    private func faceHalfExtent() -> CGSize {
        let radius = Self.faceDiameter / 2
        var halfWidth = radius
        var halfHeight = radius
        for control in system.controls where control.cluster == .face {
            halfWidth = max(halfWidth, abs(control.offset.x) + radius)
            halfHeight = max(halfHeight, abs(control.offset.y) + radius)
        }
        return CGSize(width: halfWidth, height: halfHeight)
    }

    private func size(of control: PadControl, unit: CGFloat) -> CGSize {
        switch control.shape {
        case .round:
            return CGSize(width: Self.faceDiameter * unit, height: Self.faceDiameter * unit)
        case .pill:
            let template = control.cluster == .system ? Self.systemSize : Self.shoulderSize
            return CGSize(width: template.width * unit, height: template.height * unit)
        }
    }

    /// A cluster centre from its two fractions, pulled back until the cluster fits.
    ///
    /// `wholePlayArea` is passed separately so a cluster that cannot fit inside the reduced
    /// cluster area at all still lands somewhere legal instead of being pinned to a negative
    /// height rectangle.
    private func clampedCentre(fractionX: CGFloat, fractionY: CGFloat,
                               halfWidth: CGFloat, halfHeight: CGFloat,
                               into area: CGRect, wholePlayArea: CGRect) -> CGPoint {
        let target = area.height >= halfHeight * 2 ? area : wholePlayArea
        let x = min(max(wholePlayArea.minX + wholePlayArea.width * fractionX,
                        target.minX + halfWidth),
                    max(target.minX + halfWidth, target.maxX - halfWidth))
        let y = min(max(wholePlayArea.minY + wholePlayArea.height * fractionY,
                        target.minY + halfHeight),
                    max(target.minY + halfHeight, target.maxY - halfHeight))
        return CGPoint(x: x, y: y)
    }

    private static func clamp(_ rect: CGRect, into area: CGRect) -> CGRect {
        var out = rect
        if out.width > area.width { out.size.width = area.width }
        if out.height > area.height { out.size.height = area.height }
        out.origin.x = min(max(out.origin.x, area.minX), area.maxX - out.width)
        out.origin.y = min(max(out.origin.y, area.minY), area.maxY - out.height)
        return out
    }

    /// Checks the laid-out result instead of trusting the arithmetic.
    ///
    /// A wrong control layout is exactly the bug docs/mobile-player.png captured, where the D-pad
    /// sat on top of the aspect selector and B sat on top of START, so it gets a live check and a
    /// named report rather than a comment claiming it cannot happen.
    private func verifyNoOverlap() {
        var collisions: [String] = []

        for (index, rect) in chipRects.enumerated() {
            if Self.overlap(rect, Self.hitShape(of: chips[index].control), dpadRect, .rect) {
                collisions.append("D-pad/\(chips[index].control.label)")
            }
        }
        for i in chipRects.indices {
            for j in chipRects.indices where j > i {
                if Self.overlap(chipRects[i], Self.hitShape(of: chips[i].control),
                                chipRects[j], Self.hitShape(of: chips[j].control)) {
                    collisions.append("\(chips[i].control.label)/\(chips[j].control.label)")
                }
            }
        }

        let line: String?
        if collisions.isEmpty {
            line = nil
        } else {
            line = "touch layout overlap on \(system.badge): "
                + collisions.prefix(4).joined(separator: ", ")
                + (collisions.count > 4 ? " and \(collisions.count - 4) more" : "")
        }
        // Reported once per distinct outcome. Repeating it on every layout pass would bury the
        // rest of the diagnostics.
        if line != lastOverlapReport {
            lastOverlapReport = line
            if let line {
                report(line)
            }
        }
    }

    private func report(_ line: String) {
        NSLog("[continuum] %@", line)
        onDiagnostic?(line)
    }

    // ------------------------------------------------------------------ shape geometry

    /// What a control actually occupies on screen, as opposed to the rectangle it is laid out in.
    private enum HitShape {
        case circle
        case rect
    }

    private static func hitShape(of control: PadControl) -> HitShape {
        control.shape == .round ? .circle : .rect
    }

    /// Whether two laid-out controls genuinely overlap.
    ///
    /// SHAPE AWARE, AND IT HAS TO BE. This started out comparing rectangles and that was wrong in
    /// a way worth recording, because simulating the layout is what caught it rather than reading
    /// it. A face diamond puts four CIRCLES of diameter 1.4 units at offsets of 1.15, so their
    /// centres are hypot(1.15, 1.15) = 1.63 units apart and the circles clear each other by 0.23.
    /// Their bounding BOXES, though, overlap at the corners, because 1.15 is less than 1.4. A
    /// rectangle test therefore reported four collisions on every layout pass for SNES and PS1,
    /// and since a report also writes the status line, it would have buried every real diagnostic
    /// behind a permanent false alarm on the two systems with the most buttons.
    private static func overlap(_ a: CGRect, _ aShape: HitShape,
                                _ b: CGRect, _ bShape: HitShape) -> Bool {
        // Touching is not overlapping. Half a point of slack keeps two controls laid out exactly
        // edge to edge from reading as a fault.
        let slack: CGFloat = 0.5
        switch (aShape, bShape) {
        case (.circle, .circle):
            let dx = a.midX - b.midX
            let dy = a.midY - b.midY
            let reach = a.width / 2 + b.width / 2 - slack
            return (dx * dx + dy * dy).squareRoot() < reach
        case (.circle, .rect):
            return circleMeetsRect(circle: a, rect: b, slack: slack)
        case (.rect, .circle):
            return circleMeetsRect(circle: b, rect: a, slack: slack)
        case (.rect, .rect):
            return a.insetBy(dx: slack, dy: slack).intersects(b.insetBy(dx: slack, dy: slack))
        }
    }

    private static func circleMeetsRect(circle: CGRect, rect: CGRect, slack: CGFloat) -> Bool {
        // Nearest point on the rectangle to the circle's centre, then one distance comparison.
        let cx = circle.midX
        let cy = circle.midY
        let nx = min(max(cx, rect.minX), rect.maxX)
        let ny = min(max(cy, rect.minY), rect.maxY)
        let dx = cx - nx
        let dy = cy - ny
        return (dx * dx + dy * dy).squareRoot() < circle.width / 2 - slack
    }

    // ------------------------------------------------------------------ hit testing

    /// Only the controls are touchable.
    ///
    /// Without this the whole view would swallow every touch over the picture, including the back
    /// button in the chrome above it, and leaving a running game would be impossible.
    override func point(inside point: CGPoint, with event: UIEvent?) -> Bool {
        if dpadRect.insetBy(dx: -Self.hitSlop, dy: -Self.hitSlop).contains(point) {
            return true
        }
        return chipIndex(at: point) != nil
    }

    /// Which control a point lands on, tested against the control's real shape.
    ///
    /// A round button is hit-tested as a CIRCLE, not as its bounding box. On a diamond those boxes
    /// overlap at the corners, so a box test would let a thumb in the empty corner between Triangle
    /// and Circle press whichever of the two happened to be earlier in the array. A circle test
    /// means a button is pressed when it looks pressed.
    ///
    /// Every shape is grown by `hitSlop` first, because a thumb aiming at the edge of a button
    /// should still get it.
    private func chipIndex(at point: CGPoint) -> Int? {
        for (index, rect) in chipRects.enumerated() where index < chips.count {
            switch Self.hitShape(of: chips[index].control) {
            case .circle:
                let dx = point.x - rect.midX
                let dy = point.y - rect.midY
                if (dx * dx + dy * dy).squareRoot() <= rect.width / 2 + Self.hitSlop {
                    return index
                }
            case .rect:
                if rect.insetBy(dx: -Self.hitSlop, dy: -Self.hitSlop).contains(point) {
                    return index
                }
            }
        }
        return nil
    }

    // ------------------------------------------------------------------ touches

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            let point = touch.location(in: self)
            if dpadRect.insetBy(dx: -Self.hitSlop, dy: -Self.hitSlop).contains(point) {
                grabs[ObjectIdentifier(touch)] = .dpad(touch.location(in: dpad))
            } else if let index = chipIndex(at: point) {
                grabs[ObjectIdentifier(touch)] = .chip(index)
            }
        }
        recompute()
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        var changed = false
        for touch in touches {
            let key = ObjectIdentifier(touch)
            // Only a D-pad grab tracks movement. A chip grab is sticky by design: see `Grab`.
            if case .dpad = grabs[key] {
                grabs[key] = .dpad(touch.location(in: dpad))
                changed = true
            }
        }
        if changed {
            recompute()
        }
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        endTouches(touches)
    }

    /// Cancellation is not an edge case here. A system gesture, an incoming call or the app
    /// switcher all arrive this way, and a cancelled touch that is not released is a button that
    /// stays down for the rest of the session.
    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        endTouches(touches)
    }

    private func endTouches(_ touches: Set<UITouch>) {
        for touch in touches {
            grabs.removeValue(forKey: ObjectIdentifier(touch))
        }
        recompute()
    }

    /// Rebuilds the entire pad state from the live grabs.
    ///
    /// Whole-state recomputation rather than per-control counters. Two fingers on one button, a
    /// finger cancelled while another is still down, or a control set swapped mid-press all fall
    /// out correctly because there is no accumulated count to drift.
    private func recompute() {
        var pressed = Set<PadSlot>()
        var up = false
        var down = false
        var left = false
        var right = false

        for grab in grabs.values {
            switch grab {
            case .chip(let index):
                // The control set can change while a finger is down, so the index is checked
                // rather than trusted.
                if index >= 0 && index < chips.count {
                    pressed.insert(chips[index].control.slot)
                }
            case .dpad(let point):
                let d = Self.directions(at: point, in: dpad.bounds)
                up = up || d.up
                down = down || d.down
                left = left || d.left
                right = right || d.right
            }
        }

        if up { pressed.insert(.up) }
        if down { pressed.insert(.down) }
        if left { pressed.insert(.left) }
        if right { pressed.insert(.right) }

        padState = PadFrame(pressed: pressed)

        dpad.setDirections(up: up, down: down, left: left, right: right)
        for chip in chips {
            chip.setPressed(pressed.contains(chip.control.slot))
        }
    }

    /// Which directions a touch at this point means.
    ///
    /// THE D-PAD IS ONE SURFACE, AND THAT IS THE WHOLE POINT. Four separate buttons cannot
    /// express a diagonal: a thumb resting between Up and Right would land on one of them, or
    /// flicker between the two, and most action games would be unplayable. SESSION_HANDOFF.md's
    /// appendix records this decision from the browser build, and this is that implementation
    /// (`web/src/engine/input.js`, `_attachDpadSurface`) transcribed, constants included.
    ///
    /// The point is normalised against the pad's half extents, so it is scale independent. A
    /// direction counts when its own component dominates, or when the other component is still
    /// large enough for the press to be a genuine diagonal.
    ///
    /// UIKit's y axis points down, exactly as the browser's `clientY` did, so `y < 0` meaning Up
    /// is correct here and is not an inversion waiting to be noticed.
    static func directions(at point: CGPoint,
                           in bounds: CGRect) -> (up: Bool, down: Bool, left: Bool, right: Bool) {
        let halfWidth = bounds.width / 2
        let halfHeight = bounds.height / 2
        guard halfWidth > 0, halfHeight > 0 else { return (false, false, false, false) }

        let x = (point.x - bounds.midX) / halfWidth
        let y = (point.y - bounds.midY) / halfHeight
        let magnitude = (x * x + y * y).squareRoot()
        guard magnitude > dpadDeadzone else { return (false, false, false, false) }

        var up = false
        var down = false
        var left = false
        var right = false
        if y < 0 && abs(y) > abs(x) * dpadDiagonalRatio { up = true }
        if y > 0 && abs(y) > abs(x) * dpadDiagonalRatio { down = true }
        if x < 0 && abs(x) > abs(y) * dpadDiagonalRatio { left = true }
        if x > 0 && abs(x) > abs(y) * dpadDiagonalRatio { right = true }
        return (up, down, left, right)
    }
}

// MARK: - SwiftUI bridge

/// Puts `TouchControlsView` in a SwiftUI tree and links it to the render loop's `PadInputSource`.
struct TouchControlsHost: UIViewRepresentable {
    let system: GameSystem
    let layout: TouchLayout
    /// The box `MetalCanvas` reads through. Held by the caller, so it survives this view being
    /// rebuilt, and pointed at the live view here.
    let input: PadInputSource
    let onDiagnostic: (String) -> Void
    /// Where the picture may be drawn, reported after every layout. See
    /// `TouchControlsView.onPictureArea`.
    let onPictureArea: (CGRect) -> Void

    func makeUIView(context: Context) -> TouchControlsView {
        let view = TouchControlsView(system: system, layout: layout)
        view.onDiagnostic = onDiagnostic
        view.onPictureArea = onPictureArea
        input.view = view
        return view
    }

    func updateUIView(_ view: TouchControlsView, context: Context) {
        view.system = system
        view.layout = layout
        view.onDiagnostic = onDiagnostic
        view.onPictureArea = onPictureArea
        // Re-pointed on every update because SwiftUI may hand back a different instance after a
        // rebuild, and a stale box would silently report a released pad forever.
        input.view = view
    }

    static func dismantleUIView(_ view: TouchControlsView, coordinator: Coordinator) {
        // Ordered: drop the held buttons first, then break the links, so the engine cannot be
        // handed a stale press on the way out.
        view.releaseAll()
        view.onDiagnostic = nil
        view.onPictureArea = nil
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    final class Coordinator {}
}
