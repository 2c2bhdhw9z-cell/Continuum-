// Continuum - the on-screen pad, and the one place that knows the engine's input wire format.
//
// Until this file existed the app had no input at all. `apply_gamepad(port:buttons:axes:)` and
// `connectedPads()` were exported through UniFFI and nothing in Swift had ever called either, so
// five working cores ran real games that could not be started, paused or played.
//
// Three things in here are load-bearing and are explained where they are defined rather than
// here: the button array's ORDER (`PadSlot`), the D-pad being ONE surface rather than four
// buttons (`TouchControlsView.directions`), and real multi-touch (`TouchControlsView.grabs`).

import Foundation
import SwiftUI
import UIKit

// MARK: - The wire format

/// A slot in the button array that `engine.applyGamepadFrom(port:source:buttons:axes:)` expects.
///
/// Shared with the physical controllers in PhysicalControllers.swift, which build the same array
/// for the same call and differ only in the source layer they name. One table, so the two cannot
/// drift into disagreeing about what button 3 is.
///
/// THE RAW VALUE IS THE ARRAY INDEX, AND THIS IS **NOT** LIBRETRO'S BUTTON ORDER.
///
/// That distinction is the single easiest thing to get wrong in this file, and getting it wrong
/// would not look broken: every button would simply do a different button's job, which reads as
/// a confusing game rather than as a bug. So the path was traced end to end rather than assumed:
///
/// ```text
///   uniffi_api.rs   apply_gamepad_from(port, source, buttons, axes)
///     -> bridge.rs    apply_gamepad_from(port, source, &buttons, &axes)
///       -> input/gamepad.rs  apply_standard_gamepad_from(port, source, buttons, axes)
/// ```
///
/// `apply_standard_gamepad_from` does not read the array positionally into libretro ids. It walks
/// `STANDARD_GAMEPAD_MAP` (gamepad.rs, the table just above it) and pulls `buttons[index]` for
/// each entry, and that table is the **W3C "standard gamepad" layout**. The order below is that
/// table, transcribed entry by entry.
///
/// For contrast, libretro's own order is the `Button` enum in `input/mod.rs`
/// (B 0, Y 1, Select 2, Start 3, Up 4, Down 5, Left 6, Right 7, A 8, X 9, L 10, R 11) and it is
/// what the core is finally asked for through `InputSnapshot::libretro_state`. It is NOT the wire
/// format of this call. Sending it would put Y where A belongs and Start where X belongs.
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

    /// Stable key for optional free placement in `TouchLayout.buttonFrees`.
    ///
    /// Short names rather than raw integers so a stored layout stays readable in a dump and a
    /// renamed case cannot silently remap someone else's arrangement.
    var layoutKey: String {
        switch self {
        case .b: return "b"
        case .a: return "a"
        case .y: return "y"
        case .x: return "x"
        case .l: return "l"
        case .r: return "r"
        case .l2: return "l2"
        case .r2: return "r2"
        case .select: return "select"
        case .start: return "start"
        case .l3: return "l3"
        case .r3: return "r3"
        case .up: return "up"
        case .down: return "down"
        case .left: return "left"
        case .right: return "right"
        }
    }
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

    /// Where the stylus is, as a fraction of the WHOLE framebuffer with the origin top left, and
    /// whether it is down. See `EmulatorBridge.applyPointer` for why fractions and not pixels.
    ///
    /// Carried in the pad frame rather than pushed from the touch handler, for the reason
    /// `MetalCanvas.gamepadSource` exists: the tick holds the engine's mutex for its whole
    /// duration, so every engine call belongs inside it and the touch handlers only ever write to
    /// this box. It also means a stroke and the buttons pressed during it land in the same frame,
    /// which matters for a game where a tap and a button are one action.
    ///
    /// `(0, 0)` while nothing has been touched. Harmless because `pointerPressed` is false, and
    /// the engine remembers the last position rather than reading this one.
    let pointer: CGPoint
    let pointerPressed: Bool

    /// Buttons held as TURBO by an extra button, in the same W3C order as `buttons`.
    ///
    /// Sent through `engine.applyTurbo(port:buttons:)`, never through `buttons`: a turbo slot that
    /// was also in `buttons` would simply be held, and the pulse would never be seen. The engine
    /// pulses it per core frame. See `GamepadBridge::turbo_step`.
    let turbo: [Bool]

    /// Nothing held. What is pushed when no game is running, so a button cannot survive a
    /// session ending while a finger was down.
    ///
    /// Note that this also lifts the stylus, which is the same guarantee for the same reason: a
    /// session that ended mid-stroke must not leave the DS believing the screen is still held.
    static let released = PadFrame(pressed: [])

    /// Left analog stick deflection, each component `-1...1`, y positive DOWNWARD to match the
    /// engine's axis convention.
    ///
    /// Zero for every system whose pad has no stick, which is all of them but the N64. See
    /// `GameSystem.dpadDrivesAnalogStick` for why the N64 cannot do without it.
    init(pressed: Set<PadSlot>,
         stick: CGPoint = .zero,
         rightStick: CGPoint = .zero,
         pointer: CGPoint = .zero,
         pointerPressed: Bool = false,
         turbo: Set<PadSlot> = []) {
        self.pointer = pointer
        self.pointerPressed = pointerPressed
        var slots = [Bool](repeating: false, count: PadSlot.arrayLength)
        for slot in pressed {
            slots[slot.rawValue] = true
        }
        buttons = slots
        var pulsed = [Bool](repeating: false, count: PadSlot.arrayLength)
        for slot in turbo {
            pulsed[slot.rawValue] = true
        }
        self.turbo = pulsed

        // Left stick in 0 and 1. Right stick in 2 and 3 (the 3DS C-stick). A centred stick
        // cannot cancel a D-pad press: `apply_standard_gamepad` ORs stick-derived directions
        // onto the button bits only past AXIS_DEADZONE, so zeroes contribute nothing.
        axes = [
            Float(stick.x).clampedToStick,
            Float(stick.y).clampedToStick,
            Float(rightStick.x).clampedToStick,
            Float(rightStick.y).clampedToStick,
        ]
    }
}

private extension Float {
    /// Clamped to the engine's axis range. A value outside it is not a stick position, and
    /// `apply_standard_gamepad_from` clamps again on its side; doing it here as well means the
    /// frame this app publishes is already truthful rather than relying on the far end to fix it.
    var clampedToStick: Float { Swift.min(Swift.max(self, -1), 1) }
}

/// A live read-through to whichever `TouchControlsView` is on screen.
///
/// The render loop in `MetalCanvas` needs the pad state every frame, and the view that owns that
/// state is created and destroyed by SwiftUI. A small box that both sides hold, with a weak link
/// to the view, keeps the canvas from having to know anything about the view hierarchy, and makes
/// "no controls on screen" a released pad rather than a missing value to handle.
final class PadInputSource {
    weak var view: TouchControlsView?

    /// Where an extra button's app action goes (quick save, fast forward and the rest), with
    /// `pressed` true on the press and false on the release. Set once by `EngineHost`, which owns
    /// both this box and everything the actions reach. Nil in the layout editor, which is why a
    /// button tapped there cannot save a state.
    var onAppAction: ((PadAppAction, Bool) -> Void)?

    /// Where a skin function button goes (SkinFunctions.swift), press and release. Set once by
    /// `EngineHost.wireSkinFunctions`.
    var onSkinFunction: ((SkinFunction, Bool) -> Void)?
    /// The real on/off state a switch bound to a function shows. Nil for a function with none.
    var skinFunctionState: ((SkinFunction) -> Bool?)?

    /// The state for the frame about to run. Released when there is no control surface.
    func currentFrame() -> PadFrame {
        view?.currentFrame() ?? .released
    }

    /// The trackpad's mouse for the frame about to run, consuming the motion since the last call.
    /// Nil while trackpad mode is off or no pad is on screen, so the engine is not told about a
    /// mouse nobody is using.
    func takeMouse() -> MouseFrame? {
        view?.takeMouseFrame()
    }
}

/// One frame of the touch screen used as a trackpad: relative motion in the core's mouse units
/// and which buttons are down. See `ContinuumEngine.applyMouse`.
struct MouseFrame: Sendable, Equatable {
    var dx: Float
    var dy: Float
    var left: Bool
    var right: Bool
    var middle: Bool
}

/// Where the emulated touch screen is, answered by the engine.
///
/// THE ENGINE OWNS THIS ARITHMETIC. The DS and 3DS screens can be stacked, side by side, one big
/// and one small, one alone, swapped, or cut into a skin's holes, and only the engine knows which
/// layout it drew. Both closures take the size of the picture view the fractions belong to (the
/// Metal view's rect, or the skin canvas), and work in fractions of it.
struct TouchScreenMapper {
    /// The touch screen's rect as fractions of a picture view of this size, or nil for none.
    let rect: (CGSize) -> CGRect?
    /// A point as fractions of that view, to the framebuffer fraction `applyPointer` takes.
    /// Clamped to the screen's edge.
    let map: (CGSize, CGPoint) -> CGPoint?
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
    case ds
    /// The Famicom's disk drive add-on. A separate system rather than a flavour of NES, because it
    /// has its own library, its own box art directory and its own BIOS requirement, and a shelf
    /// that called those games "NES" would be wrong about all three.
    case fds
    /// Sega's first console, the Master System's predecessor. Runs on the same core.
    case sg1000
    /// NEC's PC Engine, sold as the TurboGrafx-16 outside Japan. HuCard games only; the CD add-on
    /// needs a system card BIOS that cannot ship with the app.
    case tg16
    /// The Atari 2600. One button, and the reason `oneFace` exists.
    case atari2600
    /// The Nintendo 64. The first system here whose primary control is an ANALOG STICK rather than
    /// a D-pad, which is why `dpadDrivesAnalogStick` exists.
    case n64
    /// The Nintendo 3DS. The raw value cannot start with a digit, so the case is `n3ds`.
    /// The Circle Pad is an analog stick, same problem as the N64, so the D-pad drives the
    /// left stick. The C-stick is the right stick and is not drawn.
    case n3ds
    /// The PlayStation Portable. The face buttons are the PlayStation's, and the
    /// analog nub is the left stick, so the D-pad surface drives that stick the
    /// same way the N64 and the 3DS already do.
    case psp

    // Wave two. Raw values are the shared system ids in wt-notes/wave2/BRIEF.md, exactly.
    /// Bandai WonderSwan and WonderSwan Color, one system as the core sees it.
    case wswan
    /// SNK Neo Geo Pocket and Pocket Color.
    case ngp
    /// PC Engine CD (TurboGrafx-CD), on the full Beetle PCE. HuCards stay `tg16`.
    case pcecd
    /// PC Engine SuperGrafx HuCards (.sgx).
    case sgx
    /// Commodore Amiga.
    case amiga
    /// Commodore 64.
    case c64
    /// MS-DOS.
    case dos
    /// DOOM-engine games (IWADs and PWADs) on PrBoom.
    case doom
    /// Atari Jaguar.
    case jaguar
    /// Atari Lynx.
    case lynx
    /// Atari 7800 ProSystem.
    case atari7800
    /// Atari 5200.
    case atari5200
    /// Arcade romsets (FinalBurn Neo by default, MAME 2003-Plus as a choice).
    case arcade
    /// Nintendo Pokemon Mini.
    case pokemini
    /// Nintendo Virtual Boy.
    case vb
    /// Sega Saturn (Yabause by default, Beetle Saturn as a choice).
    case saturn
    /// Sega Mega-CD / Sega CD, on Genesis Plus GX.
    case segacd
    /// Sega 32X, on PicoDrive.
    case sega32x
    /// Sega Dreamcast, on Flycast.
    case dreamcast
    /// Adobe Flash (.swf), in the bundled Ruffle player view, not a libretro core. See
    /// WebPlayers.swift. The pad sends keyboard keys, remappable per game.
    case flash
    /// J2ME phone games (.jar), in the bundled J2meJS player view, not a libretro core. The pad
    /// is a phone keypad: D-pad, OK, the two soft keys and the number keys.
    case j2me

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
        case .ds: return "DS"
        case .fds: return "FDS"
        case .sg1000: return "SG"
        case .tg16: return "TG16"
        case .atari2600: return "2600"
        case .n64: return "N64"
        case .n3ds: return "3DS"
        case .psp: return "PSP"
        case .wswan: return "WS"
        case .ngp: return "NGP"
        case .pcecd: return "PCECD"
        case .sgx: return "SGX"
        case .amiga: return "AMIGA"
        case .c64: return "C64"
        case .dos: return "DOS"
        case .doom: return "DOOM"
        case .jaguar: return "JAG"
        case .lynx: return "LYNX"
        case .atari7800: return "7800"
        case .atari5200: return "5200"
        case .arcade: return "ARC"
        case .pokemini: return "MINI"
        case .vb: return "VB"
        case .saturn: return "SAT"
        case .segacd: return "MCD"
        case .sega32x: return "32X"
        case .dreamcast: return "DC"
        case .flash: return "FLASH"
        case .j2me: return "J2ME"
        }
    }

    /// Whether the D-pad surface should also report an ANALOG STICK deflection.
    ///
    /// True for the N64, the 3DS and the PSP, and it is the difference between those systems being
    /// playable and looking broken. Almost every N64 game reads the Control Stick and ignores
    /// the D-pad entirely: Mario 64 does not move at all from the D-pad. A 3DS game reads the
    /// Circle Pad the same way. The core maps that stick to the LEFT analog axes, which this
    /// app used to send as zeroes because no earlier system had a stick.
    ///
    /// The surface is already a continuous touch point rather than four buttons, so the deflection
    /// is real analog rather than eight fixed directions. See `Self.stickVector(at:in:)`.
    ///
    /// The digital D-pad bits are still sent alongside it, because a few N64 games do read the
    /// D-pad and sending both costs nothing.
    var dpadDrivesAnalogStick: Bool {
        switch self {
        case .nes, .snes, .gb, .gbc, .gba, .sms, .gg, .genesis, .ps1, .ds, .fds, .sg1000,
             .tg16, .atari2600:
            return false
        case .n64:
            return true
        case .n3ds:
            // Azahar binds the Circle Pad to analog axis 0, the left stick. A 3DS game that
            // only reads that stick would not move from a digital D-pad, which is the N64
            // failure this flag already exists to prevent. The digital bits are still sent.
            return true
        case .psp:
            // PPSSPP's descriptors name the left analog, and most PSP games read that
            // nub rather than the D-pad. The same surface sends both, as on the N64.
            return true
        case .dreamcast:
            // Flycast's descriptors put the Dreamcast stick on the left analog, and 3D games
            // read only that. The digital D-pad bits still go too, as on the N64.
            return true
        case .atari5200:
            // a5200's DEFAULT descriptors are "Joystick X/Y (Analog)" on the left stick: the
            // 5200 stick was analog. The digital directions are sent alongside.
            return true
        case .wswan, .ngp, .pcecd, .sgx, .amiga, .c64, .dos, .doom, .jaguar, .lynx,
             .atari7800, .arcade, .pokemini, .vb, .saturn, .segacd, .sega32x:
            return false
        case .flash, .j2me:
            // Keys, not sticks: the bundled players read key presses.
            return false
        }
    }

    /// Which part of the framebuffer is a touch screen, as a fraction of it with the origin top
    /// left, or nil for a system that has none.
    ///
    /// Expressed against the framebuffer rather than against the display because that is the one
    /// description that survives rotation, the aspect setting and every layout the pad can take:
    /// wherever the picture ends up being drawn, the touch screen is still the same part of it.
    ///
    /// The DS framebuffer is both screens stacked, top over bottom, so its touch screen is the
    /// LOWER HALF and nothing above y = 0.5 responds. That is not a simplification of the
    /// hardware, it is the hardware: only the bottom screen was ever a digitiser.
    ///
    /// Every other system returns nil, and that is what keeps this whole feature off their pads.
    /// Listed case by case rather than defaulted, so adding a system is a compile error here
    /// instead of a touch screen that silently does nothing.
    var touchScreen: CGRect? {
        switch self {
        case .nes, .snes, .gb, .gbc, .gba, .sms, .gg, .genesis, .ps1, .fds, .sg1000,
             .tg16, .atari2600, .n64, .psp:
            return nil
        case .wswan, .ngp, .pcecd, .sgx, .amiga, .c64, .dos, .doom, .jaguar, .lynx,
             .atari7800, .atari5200, .arcade, .pokemini, .vb, .saturn, .segacd, .sega32x,
             .dreamcast:
            return nil
        case .flash, .j2me:
            // The whole picture takes taps, but as the web view's own touches (mouse clicks for
            // Flash, the touch screen for J2ME), never through the engine's pointer.
            return nil
        case .ds:
            return CGRect(x: 0, y: 0.5, width: 1, height: 0.5)
        case .n3ds:
            // Stacked default: both screens are 240 tall in a 480-tall frame, and the bottom
            // screen is 320 wide inside a 400-wide frame, centred. Only that screen is a
            // digitiser. A side-by-side layout option would put the touch screen somewhere
            // else; the core's default is stacked, and this matches that.
            return CGRect(x: 0.1, y: 0.5, width: 0.8, height: 0.5)
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
        case .ds: return "Nintendo DS"
        case .fds: return "Famicom Disk System"
        case .sg1000: return "Sega SG-1000"
        case .tg16: return "TurboGrafx-16"
        case .atari2600: return "Atari 2600"
        case .n64: return "Nintendo 64"
        case .n3ds: return "Nintendo 3DS"
        case .psp: return "PlayStation Portable"
        case .wswan: return "WonderSwan"
        case .ngp: return "Neo Geo Pocket"
        case .pcecd: return "PC Engine CD"
        case .sgx: return "SuperGrafx"
        case .amiga: return "Commodore Amiga"
        case .c64: return "Commodore 64"
        case .dos: return "DOS"
        case .doom: return "DOOM"
        case .jaguar: return "Atari Jaguar"
        case .lynx: return "Atari Lynx"
        case .atari7800: return "Atari 7800"
        case .atari5200: return "Atari 5200"
        case .arcade: return "Arcade"
        case .pokemini: return "Pokemon Mini"
        case .vb: return "Virtual Boy"
        case .saturn: return "Sega Saturn"
        case .segacd: return "Sega CD"
        case .sega32x: return "Sega 32X"
        case .dreamcast: return "Dreamcast"
        case .flash: return "Flash"
        case .j2me: return "J2ME"
        }
    }

    /// Whether this system is played with a computer KEYBOARD as well as a pad.
    ///
    /// THE SPOT THE KEYBOARD ATTACHES TO. The input worker's on-screen and hardware keyboard
    /// reads this to decide whether to offer itself; nothing in this file draws a keyboard. The
    /// pads below give each of these systems a joystick with fire buttons, plus the core's own
    /// on-screen keyboard toggle where the core has one (VICE and PUAE on Select, DOSBox Pure on
    /// L3), so they are playable before the keyboard exists.
    var wantsKeyboard: Bool {
        switch self {
        case .c64, .amiga, .dos:
            return true
        case .nes, .snes, .gb, .gbc, .gba, .sms, .gg, .genesis, .ps1, .ds, .fds, .sg1000,
             .tg16, .atari2600, .n64, .n3ds, .psp, .wswan, .ngp, .pcecd, .sgx, .doom, .jaguar,
             .lynx, .atari7800, .atari5200, .arcade, .pokemini, .vb, .saturn, .segacd,
             .sega32x, .dreamcast, .flash, .j2me:
            return false
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
        case .nes, .gb, .gbc, .fds:
            // The Famicom Disk System used the Famicom's own controller, so its pad is the NES pad
            // unchanged. Disk swapping is not a button: it arrives as a core option, and a game
            // that asks for side B has to be answered there rather than here.
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

        case .sms, .gg, .sg1000:
            // Two buttons and Start. The Master System's console-mounted Pause arrives as Start,
            // which is the only way a core can offer it. The SG-1000 is the same shape of pad for
            // the same reason: two buttons, and a Pause that lived on the console rather than in
            // your hand.
            return Self.twoFace(right: (.a, "2"), left: (.b, "1"))
                + [PadControl(slot: .start, label: "START", cluster: .system,
                              shape: .pill, offset: CGPoint(x: 0, y: 0))]

        case .ps1:
            return Self.diamondFace(top: (.x, "Triangle"), right: (.a, "Circle"),
                                    bottom: (.b, "Cross"), left: (.y, "Square"))
                + Self.shoulders(left: [(.l, "L1"), (.l2, "L2")],
                                 right: [(.r, "R1"), (.r2, "R2")])
                + Self.selectStart

        case .tg16:
            // READ FROM beetle-pce-fast's OWN DESCRIPTORS: JOYPAD_A is "I" and JOYPAD_B is "II",
            // so the right-hand button is I and the left is II. That is the opposite way round
            // from how the numerals read, and guessing from the names would have swapped every
            // PC Engine game's two buttons.
            //
            // START is labelled RUN, because that is what the console printed on it.
            return Self.twoFace(right: (.a, "I"), left: (.b, "II"))
                + Self.systemPair(select: "SELECT", start: "RUN")

        case .atari2600:
            // One button. Stella's descriptors call JOYPAD_B "Fire", JOYPAD_START "Reset" and
            // JOYPAD_SELECT "Select", and those last two were console switches rather than pad
            // buttons: on real hardware you reached over to the machine to reset a game. They are
            // the only way a core can offer them, so they sit in the bottom row with their real
            // names.
            //
            // The 2600's other controls are deliberately absent. Its difficulty switches arrive as
            // shoulder buttons and the paddle and driving controllers as analog axes, and none of
            // them is a thing a player reaches for mid-game. A pad with six controls where the
            // hardware had one would be a worse reproduction, not a more complete one.
            return Self.oneFace((.b, "FIRE"))
                + Self.systemPair(select: "SELECT", start: "RESET")

        case .n64:
            // EVERY ONE OF THESE WAS READ FROM THE CORE, and this is the most counter-intuitive
            // table in this file. parallel-n64's own descriptors in
            // `emulate_game_controller_via_libretro.c` say:
            //
            //     retro B      -> N64 A          retro A   -> C-Down
            //     retro Y      -> N64 B          retro X   -> C-Up
            //     retro L2     -> Z Trigger      retro L   -> C-Left
            //     retro SELECT -> L Shoulder     retro R   -> C-Right
            //     retro R2     -> R Shoulder     left stick -> Control Stick
            //
            // So the N64's A button is retro B and its B button is retro Y, and L Shoulder arrives
            // on SELECT of all things. Mapping these from their names would have swapped A with B,
            // put Z on the wrong control and left L unreachable, and every one of those would have
            // been blamed on the core rather than on this table.
            //
            // The diamond puts A right and B left, which is Nintendo's arrangement, with the two
            // most-used C buttons above and below. The other two C buttons are left off rather
            // than crammed in: six face buttons on a phone is not a control scheme.
            return Self.diamondFace(top: (.x, "C\u{2191}"), right: (.b, "A"),
                                    bottom: (.a, "C\u{2193}"), left: (.y, "B"))
                + Self.shoulders(left: [(.l2, "Z"), (.select, "L")], right: [(.r2, "R")])
                + [PadControl(slot: .start, label: "START", cluster: .system,
                              shape: .pill, offset: CGPoint(x: 0, y: 0))]

        case .n3ds:
            // Read from Azahar's own descriptors in citra_libretro.cpp: retro A/B/X/Y are
            // 3DS A/B/X/Y, so the diamond is Nintendo's (X top, A right, B bottom, Y left),
            // the same arrangement as the DS. L2 and R2 are ZL and ZR. The Circle Pad is
            // not a button: `dpadDrivesAnalogStick` sends it as the left stick.
            //
            // Not drawn, on purpose. The C-stick is the right analog and this pad has no
            // second stick surface. Home is retro L3; a third system pill has nowhere to
            // sit without covering Select or Start. The touch screen is the digitiser in
            // `touchScreen`, not a button.
            return Self.diamondFace(top: (.x, "X"), right: (.a, "A"),
                                    bottom: (.b, "B"), left: (.y, "Y"))
                + Self.shoulders(left: [(.l, "L"), (.l2, "ZL")],
                                 right: [(.r, "R"), (.r2, "ZR")])
                + Self.selectStart

        case .psp:
            // Read from PPSSPP's own descriptors in libretro.cpp: retro B/A/X/Y are
            // Cross/Circle/Triangle/Square, the same slots the PS1 pad uses. L and R
            // are the only shoulders. The analog nub is not a button; see
            // `dpadDrivesAnalogStick`. There is no second stick on the hardware.
            return Self.diamondFace(top: (.x, "Triangle"), right: (.a, "Circle"),
                                    bottom: (.b, "Cross"), left: (.y, "Square"))
                + Self.shoulders(left: [(.l, "L")], right: [(.r, "R")])
                + Self.selectStart

        case .ds:
            // The DS diamond is the Super Nintendo's arrangement, not the PlayStation's: A sits on
            // the RIGHT and B at the BOTTOM, which is Nintendo's layout, so the labels here are the
            // same slots the SNES uses rather than a rotation of the PS1 set. Getting it wrong
            // would not look broken, it would look like a game with its buttons swapped, which is
            // the kind of thing that gets blamed on the core.
            //
            // Two shoulders, because the DS has exactly L and R and no triggers.
            //
            // THE TOUCH SCREEN IS NOT IN THIS LIST, and that is correct rather than missing: it is
            // a pointer rather than a button, so it cannot be a `PadControl`. It lives in
            // `touchScreen` above, which describes the lower half of the framebuffer as a
            // digitiser, and the pad answers it as a `.pointer` grab.
            return Self.diamondFace(top: (.x, "X"), right: (.a, "A"),
                                    bottom: (.b, "B"), left: (.y, "Y"))
                + Self.shoulders(left: [(.l, "L")], right: [(.r, "R")])
                + Self.selectStart

        // ------------------------------------------------------------ wave two
        //
        // EVERY SLOT BELOW WAS READ FROM THE CORE'S OWN INPUT DESCRIPTORS (or, where a core maps
        // through core options, from the option's default value), not from the button's name.
        // The source file is named on each case. Labels are what the hardware printed.

        case .wswan:
            // beetle-wswan libretro.c, horizontal layout: the X pad is the D-pad; the Y pad is
            // retro L (left), R2 (up), L2 (down), R (right); A and B are retro A and B; Start;
            // Select is "Rotate screen + active D-Pad". Both pads sit on the left of the real
            // console; here the Y pad is the small diamond beside A and B so a thumb can reach it.
            return Self.place([
                (.r2, "Y\u{2191}", -1.3, -1.15), (.r, "Y\u{2192}", -0.15, 0),
                (.l2, "Y\u{2193}", -1.3, 1.15), (.l, "Y\u{2190}", -2.45, 0),
                (.b, "B", 1.2, 1.1), (.a, "A", 1.7, -0.6),
            ])
                + Self.systemPair(select: "ROTATE", start: "START")

        case .ngp:
            // beetle-ngp libretro.c: retro B is "A", retro A is "B", Start is "Option".
            return Self.twoFace(right: (.a, "B"), left: (.b, "A"))
                + [PadControl(slot: .start, label: "OPTION", cluster: .system,
                              shape: .pill, offset: CGPoint(x: 0, y: 0))]

        case .pcecd, .sgx:
            // beetle-pce libretro.cpp and beetle-supergrafx libretro.cpp, same descriptors as
            // the Fast core: JOYPAD_A is "I", JOYPAD_B is "II", Select, Run.
            return Self.twoFace(right: (.a, "I"), left: (.b, "II"))
                + Self.systemPair(select: "SELECT", start: "RUN")

        case .amiga:
            // libretro-uae libretro-core.c: B is "Fire / Red", A "2nd fire / Blue"; the
            // puae_mapper_x default is RETROK_SPACE and puae_mapper_select's is TOGGLE_VKBD, the
            // core's own on-screen keyboard.
            return Self.place([
                (.b, "FIRE", -0.85, 0.45), (.a, "FIRE 2", 0.85, -0.45),
                (.x, "SPACE", 0.85, 1.25),
            ])
                + Self.systemPair(select: "KEYS", start: "START")

        case .c64:
            // vice-libretro libretro-core.c: B is "Fire", A is the 2nd fire; vice_mapper_x
            // defaults to RETROK_SPACE, vice_mapper_l2 to RETROK_ESCAPE (RUN/STOP), r2 to
            // RETROK_RETURN, and Select to TOGGLE_VKBD, VICE's own on-screen keyboard.
            return Self.place([
                (.b, "FIRE", -0.85, 0.45), (.a, "FIRE 2", 0.85, -0.45),
                (.x, "SPACE", 0.85, 1.25),
            ])
                + Self.shoulders(left: [(.l2, "RUN/STOP")], right: [(.r2, "RETURN")])
                + Self.systemPair(select: "KEYS", start: "START")

        case .dos:
            // dosbox-pure dosbox_pure_pad.h: the joystick presets put DOS button 1 on retro B and
            // button 2 on Y or A depending on the preset the core picks per game, so both are
            // drawn; L3 is the core's own menu and on-screen keyboard (its "Always bind L3"
            // option, on by default in core_options.h).
            return Self.place([
                (.b, "FIRE 1", -0.85, 0.45), (.a, "FIRE 2", 0.85, -0.45),
                (.y, "FIRE 3", -0.85, -1.25), (.x, "FIRE 4", 0.85, 1.25),
            ])
                + Self.shoulders(left: [], right: [(.l3, "MENU")])
                + Self.selectStart

        case .doom:
            // libretro-prboom libretro.c gp_classic (the default device): X Fire, A Use, B Strafe,
            // Y Run, L/R strafe left and right, L2/R2 previous and next weapon, Select the map,
            // Start the menu. The D-pad moves and turns.
            return Self.diamondFace(top: (.x, "FIRE"), right: (.a, "USE"),
                                    bottom: (.b, "STRAFE"), left: (.y, "RUN"))
                + Self.shoulders(left: [(.l, "STRAFE \u{25C0}"), (.l2, "WEAPON -")],
                                 right: [(.r, "STRAFE \u{25B6}"), (.r2, "WEAPON +")])
                + Self.systemPair(select: "MAP", start: "MENU")

        case .jaguar:
            // virtualjaguar libretro_core_options.h defaults: retro A is Jaguar A, B is B, Y is
            // C, Select is Pause, Start is Option, and the keypad: X 0, L 1, R 2, L2 3, R2 4,
            // L3 5, R3 6. Keypad 7, 8, 9, * and # are reachable only from a keyboard (the core's
            // numpad-to-keyboard option), which the keyboard worker adds.
            return Self.threeFace(left: (.a, "A"), middle: (.b, "B"), right: (.y, "C"))
                + Self.place([(.x, "0", 0, -1.6)])
                + Self.shoulders(left: [(.l, "1"), (.l2, "3"), (.l3, "5")],
                                 right: [(.r, "2"), (.r2, "4"), (.r3, "6")])
                + Self.systemPair(select: "PAUSE", start: "OPTION")

        case .lynx:
            // libretro-handy libretro.cpp btn_map_no_rot: A, B, L Option 1, R Option 2,
            // Start Pause.
            return Self.twoFace(right: (.a, "A"), left: (.b, "B"))
                + Self.shoulders(left: [(.l, "OPT 1")], right: [(.r, "OPT 2")])
                + [PadControl(slot: .start, label: "PAUSE", cluster: .system,
                              shape: .pill, offset: CGPoint(x: 0, y: 0))]

        case .atari7800:
            // prosystem-libretro core/libretro.c: B "1", A "2", X "Console Reset", Select
            // "Console Select", Start "Console Pause", L and R the difficulty switches.
            return Self.twoFace(right: (.a, "2"), left: (.b, "1"))
                + Self.place([(.x, "RESET", 0, -1.7)])
                + Self.shoulders(left: [(.l, "DIFF L")], right: [(.r, "DIFF R")])
                + Self.systemPair(select: "SELECT", start: "PAUSE")

        case .atari5200:
            // a5200 libretro.c: A "Fire 1", B "Fire 2", X "#", Y "*", R "0", R2 "1", L2 "3",
            // R3 "5", L3 "7", L "Show/Hide OSK" (the core's own on-screen keypad, which has every
            // key), Select "Pause", Start "Start".
            return Self.place([
                (.b, "FIRE 2", -0.85, 0.45), (.a, "FIRE 1", 0.85, -0.45),
                (.y, "*", -1.0, -1.3), (.x, "#", 1.0, 1.3),
            ])
                + Self.shoulders(left: [(.l, "KEYPAD"), (.l2, "3"), (.l3, "7")],
                                 right: [(.r, "0"), (.r2, "1"), (.r3, "5")])
                + Self.systemPair(select: "PAUSE", start: "START")

        case .arcade:
            // FBNeo's retro_input.cpp maps per game, so these labels are positions rather than
            // a promise about any one game: six buttons in the arrangement its fighting-game maps
            // use (Y X L over B A R), Select is Coin and Start is Start, which both FBNeo and
            // MAME 2003-Plus use for every set.
            return Self.sixFace(top: [(.y, "3"), (.x, "4"), (.l, "5")],
                                bottom: [(.b, "1"), (.a, "2"), (.r, "6")])
                + Self.systemPair(select: "COIN", start: "START")

        case .pokemini:
            // PokeMini libretro.c: B "B", A "A", R "C", L "Shake", Select "Power". The phone's
            // motion sensor will press Shake too once the input worker wires it; the pill is the
            // fallback that always works.
            return Self.threeFace(left: (.b, "B"), middle: (.a, "A"), right: (.r, "C"))
                + Self.shoulders(left: [(.l, "SHAKE")], right: [])
                + [PadControl(slot: .select, label: "POWER", cluster: .system,
                              shape: .pill, offset: CGPoint(x: 0, y: 0))]

        case .vb:
            // beetle-vb libretro.cpp: the left D-pad is the D-pad; the RIGHT D-pad is retro L2
            // up, L3 down, R2 left, R3 right, which no name would have guessed. A and B, L and
            // R, Select and Start are themselves.
            return Self.place([
                (.l2, "R\u{2191}", 0, -1.15), (.r3, "R\u{2192}", 1.15, 0),
                (.l3, "R\u{2193}", 0, 1.15), (.r2, "R\u{2190}", -1.15, 0),
                (.a, "A", 2.3, 0.9), (.b, "B", 1.2, 2.2),
            ])
                + Self.shoulders(left: [(.l, "L")], right: [(.r, "R")])
                + Self.selectStart

        case .saturn:
            // yabause libretro.c and beetle-saturn input.c agree: B "A", A "B", R "C",
            // Y "X", X "Y", L "Z", L2 "L", R2 "R", Start.
            return Self.sixFace(top: [(.y, "X"), (.x, "Y"), (.l, "Z")],
                                bottom: [(.b, "A"), (.a, "B"), (.r, "C")])
                + Self.shoulders(left: [(.l2, "L")], right: [(.r2, "R")])
                + [PadControl(slot: .start, label: "START", cluster: .system,
                              shape: .pill, offset: CGPoint(x: 0, y: 0))]

        case .segacd:
            // Genesis Plus GX, the same pad as the Mega Drive: retro Y, B, A are A, B, C.
            return Self.threeFace(left: (.y, "A"), middle: (.b, "B"), right: (.a, "C"))
                + [PadControl(slot: .start, label: "START", cluster: .system,
                              shape: .pill, offset: CGPoint(x: 0, y: 0))]

        case .sega32x:
            // picodrive libretro.c: Y "A", B "B", A "C", L "X", X "Y", R "Z", Select "Mode".
            return Self.sixFace(top: [(.l, "X"), (.x, "Y"), (.r, "Z")],
                                bottom: [(.y, "A"), (.b, "B"), (.a, "C")])
                + Self.systemPair(select: "MODE", start: "START")

        case .dreamcast:
            // flycast shell/libretro/libretro.cpp: B "A", A "B", X "Y", Y "X", L2 "L Trigger",
            // R2 "R Trigger", Start. The stick is the left analog, which the D-pad surface drives
            // (`dpadDrivesAnalogStick`).
            return Self.diamondFace(top: (.x, "Y"), right: (.a, "B"),
                                    bottom: (.b, "A"), left: (.y, "X"))
                + Self.shoulders(left: [(.l2, "L")], right: [(.r2, "R")])
                + [PadControl(slot: .start, label: "START", cluster: .system,
                              shape: .pill, offset: CGPoint(x: 0, y: 0))]

        case .flash:
            // Not a core: each slot sends a KEYBOARD key, from the per-game table in Rust
            // (players/keys.rs). Labelled by position rather than by key, because the keys are
            // remappable per game and a label naming the default would then be wrong; the Flash
            // settings sheet says what each one sends. Defaults: A Space, B Z, X X, Y C, L Shift,
            // R Ctrl, Select Esc, Start Enter, D-pad the arrows.
            return Self.diamondFace(top: (.x, "X"), right: (.a, "A"),
                                    bottom: (.b, "B"), left: (.y, "Y"))
                + Self.shoulders(left: [(.l, "L")], right: [(.r, "R")])
                + Self.selectStart

        case .j2me:
            // A phone keypad around OK, slots per players/keys.rs: 1 and 3 above, 7, 0 and 9
            // below, * and # on the shoulders, the soft keys where Select and Start sit. 2, 4, 6
            // and 8 are the D-pad when the game's phone type is "standard" (J2ME settings); on the
            // default Nokia type the D-pad sends the arrow keys most games read. Every centre is
            // more than 1.4 units from its neighbours (the closest pairs are 1.45 apart).
            return Self.place([
                (.y, "1", -1.45, -1.45), (.x, "3", 1.45, -1.45),
                (.a, "OK", 0, 0),
                (.l2, "7", -1.45, 1.45), (.b, "0", 0, 1.45), (.r2, "9", 1.45, 1.45),
            ])
                + Self.shoulders(left: [(.l, "*")], right: [(.r, "#")])
                + Self.systemPair(select: "LSK", start: "RSK")
        }
    }

    /// Round face buttons at explicit offsets, for the layouts no template covers. Offsets are
    /// in face units like every template, and each list here was checked to keep centres more
    /// than 1.4 units (one button) apart.
    private static func place(_ items: [(PadSlot, String, CGFloat, CGFloat)]) -> [PadControl] {
        items.map {
            PadControl(slot: $0.0, label: $0.1, cluster: .face, shape: .round,
                       offset: CGPoint(x: $0.2, y: $0.3))
        }
    }

    /// Six buttons in two rows of three, the Saturn / six-button Mega Drive / arcade shape.
    /// Columns 1.6 apart and rows 1.6 apart, so every pair clears 1.4.
    private static func sixFace(top: [(PadSlot, String)],
                                bottom: [(PadSlot, String)]) -> [PadControl] {
        var out: [PadControl] = []
        for (index, entry) in top.prefix(3).enumerated() {
            out.append(PadControl(slot: entry.0, label: entry.1, cluster: .face, shape: .round,
                                  offset: CGPoint(x: -1.6 + 1.6 * CGFloat(index), y: -0.8)))
        }
        for (index, entry) in bottom.prefix(3).enumerated() {
            out.append(PadControl(slot: entry.0, label: entry.1, cluster: .face, shape: .round,
                                  offset: CGPoint(x: -1.6 + 1.6 * CGFloat(index), y: 0.8)))
        }
        return out
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
    /// A single face button, centred in the cluster.
    ///
    /// The Atari 2600 is the one system here with exactly one, and centring it rather than putting
    /// it where a two-button pad's right button goes is deliberate: an off-centre lone button reads
    /// as a pad with a missing button rather than as a joystick with a fire button, which is what a
    /// 2600 controller was.
    private static func oneFace(_ only: (PadSlot, String)) -> [PadControl] {
        [
            PadControl(slot: only.0, label: only.1, cluster: .face,
                       shape: .round, offset: CGPoint(x: 0, y: 0)),
        ]
    }

    /// The bottom row of two pills, with the labels the console printed on them.
    ///
    /// Parameterised because the SLOTS are universal and the NAMES are not: every system sends
    /// retro SELECT and START, but the PC Engine wrote RUN on the second one and the Atari 2600
    /// wrote RESET. Drawing START on those would be labelling a button with a name it never had.
    private static func systemPair(select: String, start: String) -> [PadControl] {
        [
            PadControl(slot: .select, label: select, cluster: .system,
                       shape: .pill, offset: CGPoint(x: -1.425, y: 0)),
            PadControl(slot: .start, label: start, cluster: .system,
                       shape: .pill, offset: CGPoint(x: 1.425, y: 0)),
        ]
    }

    private static let selectStart: [PadControl] = systemPair(select: "SELECT", start: "START")
}

// MARK: - Where the controls sit

/// The numbers that describe a control layout.
///
/// Started as the same six the browser build used (`web/src/data/touch-layout.js`) for the two
/// thumb clusters, then grew SELECT and START positions so every on-screen control the editor
/// outlines can be dragged and persisted. Same limits as before: a control centred at 0 would be
/// half off screen, and on iOS the outer few millimetres belong to the system's edge gestures.
///
/// `Codable` so the editor's result survives a relaunch. Decoding is deliberately forgiving and
/// Absolute play-area centre for one face or shoulder button that has been dragged free of its
/// cluster. SELECT and START keep dedicated fields on `TouchLayout` for backward compatibility
/// with phase 1 payloads.
struct ButtonFree: Sendable, Equatable, Codable {
    var x: Double
    var y: Double
}

/// `sanitised` is applied on the way out, so an absent value, a truncated one and one written by a
/// different build all land on something legal. Older payloads without SELECT/START or `buttonFrees`
/// keys restore those from `standard` (clustered defaults, no frees). See `restored(from:)`.
struct TouchLayout: Sendable, Equatable, Codable {
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
    /// SELECT (or the system's equivalent pill) centre, as a fraction of the play area.
    var selectX: Double
    var selectY: Double
    /// START / RUN / RESET / Pause pill centre, as a fraction of the play area.
    var startX: Double
    var startY: Double
    /// Face and shoulder buttons dragged free of their cluster, keyed by `PadSlot.layoutKey`.
    ///
    /// Absent key means that button still rides its cluster anchor (D-pad or face) plus the
    /// template offset. SELECT and START are never stored here: they keep `selectX`/`startX`.
    var buttonFrees: [String: ButtonFree]

    /// Spelled out rather than left to the synthesized memberwise initialiser, because declaring
    /// `init(from:)` below in the body of the type suppresses that synthesis, and losing it would
    /// break `standard` and `sanitised` with an error that points at those two rather than here.
    init(scale: Double, opacity: Double,
         dpadX: Double, dpadY: Double,
         faceX: Double, faceY: Double,
         selectX: Double, selectY: Double,
         startX: Double, startY: Double,
         buttonFrees: [String: ButtonFree] = [:]) {
        self.scale = scale
        self.opacity = opacity
        self.dpadX = dpadX
        self.dpadY = dpadY
        self.faceX = faceX
        self.faceY = faceY
        self.selectX = selectX
        self.selectY = selectY
        self.startX = startX
        self.startY = startY
        self.buttonFrees = buttonFrees
    }

    /// The free centre for a face/shoulder slot, if the user has dragged that button off its cluster.
    func freeCentre(for slot: PadSlot) -> ButtonFree? {
        buttonFrees[slot.layoutKey]
    }

    /// Records a free centre for a face or shoulder button.
    mutating func setFreeCentre(for slot: PadSlot, x: Double, y: Double) {
        buttonFrees[slot.layoutKey] = ButtonFree(x: x, y: y)
    }

    /// The default, tuned for a phone held at the bottom corners.
    ///
    /// Cluster y sits lower than the browser's 0.66 so thumbs reach them. SELECT and START default
    /// to a bottom centre pair that matches where the pad used to pin them before they became
    /// editable, so a fresh layout and an upgraded one look the same until the user moves them.
    static let standard = TouchLayout(scale: 1.0, opacity: 0.55,
                                      dpadX: 0.17, dpadY: 0.78,
                                      faceX: 0.83, faceY: 0.78,
                                      selectX: 0.35, selectY: 0.90,
                                      startX: 0.65, startY: 0.90)

    // MARK: Storage

    private enum CodingKeys: String, CodingKey {
        case scale, opacity, dpadX, dpadY, faceX, faceY
        case selectX, selectY, startX, startY
        case buttonFrees
    }

    /// Decodes field by field, each one falling back to the default rather than throwing.
    ///
    /// THE POINT IS THAT A PARTIAL PAYLOAD IS NOT A FAILURE. The synthesized initialiser throws
    /// the moment any single key is missing, which would turn a layout written by a build that
    /// named one field differently into a total reset, and the user would read that as the app
    /// forgetting their arrangement. Per-field recovery keeps the five numbers it can still
    /// understand. Malformed JSON, which is not recoverable, is handled one level up in
    /// `restored(from:)`.
    init(from decoder: Decoder) throws {
        let box = try decoder.container(keyedBy: CodingKeys.self)
        let fallback = Self.standard
        scale = try box.decodeIfPresent(Double.self, forKey: .scale) ?? fallback.scale
        opacity = try box.decodeIfPresent(Double.self, forKey: .opacity) ?? fallback.opacity
        dpadX = try box.decodeIfPresent(Double.self, forKey: .dpadX) ?? fallback.dpadX
        dpadY = try box.decodeIfPresent(Double.self, forKey: .dpadY) ?? fallback.dpadY
        faceX = try box.decodeIfPresent(Double.self, forKey: .faceX) ?? fallback.faceX
        faceY = try box.decodeIfPresent(Double.self, forKey: .faceY) ?? fallback.faceY
        selectX = try box.decodeIfPresent(Double.self, forKey: .selectX) ?? fallback.selectX
        selectY = try box.decodeIfPresent(Double.self, forKey: .selectY) ?? fallback.selectY
        startX = try box.decodeIfPresent(Double.self, forKey: .startX) ?? fallback.startX
        startY = try box.decodeIfPresent(Double.self, forKey: .startY) ?? fallback.startY
        buttonFrees = try box.decodeIfPresent([String: ButtonFree].self, forKey: .buttonFrees)
            ?? fallback.buttonFrees
    }

    /// Written out rather than synthesized, only so that the encoded shape and the forgiving
    /// decode above sit next to each other and cannot drift apart unnoticed.
    func encode(to encoder: Encoder) throws {
        var box = encoder.container(keyedBy: CodingKeys.self)
        try box.encode(scale, forKey: .scale)
        try box.encode(opacity, forKey: .opacity)
        try box.encode(dpadX, forKey: .dpadX)
        try box.encode(dpadY, forKey: .dpadY)
        try box.encode(faceX, forKey: .faceX)
        try box.encode(faceY, forKey: .faceY)
        try box.encode(selectX, forKey: .selectX)
        try box.encode(selectY, forKey: .selectY)
        try box.encode(startX, forKey: .startX)
        try box.encode(startY, forKey: .startY)
        try box.encode(buttonFrees, forKey: .buttonFrees)
    }

    /// The bytes to hand UserDefaults. Nil only if encoding the layout somehow fails,
    /// which the caller treats as "do not write" rather than as "store nothing": clearing the key
    /// on a failed encode would silently reset a layout the user can still see on screen.
    var storedRepresentation: Data? {
        try? JSONEncoder().encode(sanitised)
    }

    /// The layout to start from, given whatever was in UserDefaults.
    ///
    /// Absent, unreadable and out of range all end in a usable layout, and the caller does not
    /// have to tell them apart, because there is nothing different to do about any of them. The
    /// sanitise on the way out is what makes that safe rather than optimistic.
    static func restored(from data: Data?) -> TouchLayout {
        guard let data,
              let decoded = try? JSONDecoder().decode(TouchLayout.self, from: data) else {
            return standard
        }
        return decoded.sanitised
    }

    // MARK: Derived

    /// Whether this is already the shipped arrangement, so a Reset control can say so instead of
    /// offering to do nothing.
    var isStandard: Bool { sanitised == Self.standard }

    /// The same layout with the two clusters swapped left to right.
    ///
    /// For a left-handed player, who otherwise has to drag both clusters past each other and
    /// through the region where the overlap check complains. Mirroring x rather than EXCHANGING the
    /// two values is the difference between "swap sides" and "swap clusters": a D-pad nudged in to
    /// 0.3 ends up 0.3 from the right edge, keeping its own distance from its own edge, where an
    /// exchange would have given it the other cluster's inset. Both results are inside the x limits
    /// for any legal input, since those limits are symmetric about 0.5, and `sanitised` still runs
    /// because that symmetry is a property of today's constants rather than a promise.
    ///
    /// ROUNDED, and that is not tidiness. `1.0 - 0.17` is `0.8300000000000001` in binary floating
    /// point, and `1.0 - 0.8300000000000001` is `0.16999999999999993`, so without the rounding this
    /// would not be its own inverse: pressing Swap twice would leave the layout a hair off where it
    /// started and `isStandard` would answer false about an arrangement indistinguishable from the
    /// default, which would in turn leave Reset offering to do something invisible. Four decimals is
    /// far finer than a point on any screen this runs on, so nothing observable is given up for it.
    var mirrored: TouchLayout {
        var out = sanitised
        out.dpadX = Self.rounded(1.0 - out.dpadX)
        out.faceX = Self.rounded(1.0 - out.faceX)
        out.selectX = Self.rounded(1.0 - out.selectX)
        out.startX = Self.rounded(1.0 - out.startX)
        var flipped: [String: ButtonFree] = [:]
        for (key, point) in out.buttonFrees {
            flipped[key] = ButtonFree(x: Self.rounded(1.0 - point.x), y: point.y)
        }
        out.buttonFrees = flipped
        return out.sanitised
    }

    /// To four decimal places, which on a 400 point wide screen is four hundredths of a point.
    private static func rounded(_ value: Double) -> Double {
        (value * 10_000).rounded() / 10_000
    }

    /// Forces any layout into range. Applied on every read, not only on write, so a layout
    /// restored from storage by a future build cannot put a control off screen.
    var sanitised: TouchLayout {
        var frees: [String: ButtonFree] = [:]
        for (key, point) in buttonFrees {
            let cleaned = key.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !cleaned.isEmpty else { continue }
            frees[cleaned] = ButtonFree(
                x: Self.clamp(point.x, Self.minX, Self.maxX, Self.standard.faceX),
                y: Self.clamp(point.y, Self.minY, Self.maxY, Self.standard.faceY)
            )
        }
        return TouchLayout(
            scale: Self.clamp(scale, Self.minScale, Self.maxScale, Self.standard.scale),
            opacity: Self.clamp(opacity, Self.minOpacity, Self.maxOpacity, Self.standard.opacity),
            dpadX: Self.clamp(dpadX, Self.minX, Self.maxX, Self.standard.dpadX),
            dpadY: Self.clamp(dpadY, Self.minY, Self.maxY, Self.standard.dpadY),
            faceX: Self.clamp(faceX, Self.minX, Self.maxX, Self.standard.faceX),
            faceY: Self.clamp(faceY, Self.minY, Self.maxY, Self.standard.faceY),
            selectX: Self.clamp(selectX, Self.minX, Self.maxX, Self.standard.selectX),
            selectY: Self.clamp(selectY, Self.minY, Self.maxY, Self.standard.selectY),
            startX: Self.clamp(startX, Self.minX, Self.maxX, Self.standard.startX),
            startY: Self.clamp(startY, Self.minY, Self.maxY, Self.standard.startY),
            buttonFrees: frees
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

/// A labelled outline round a cluster that can be dragged. Only on screen while the layout is
/// being edited.
///
/// DRAWN BY THE PAD RATHER THAN BY THE EDITOR SCREEN ABOVE IT, and that is the whole reason this
/// class exists instead of a rectangle in SwiftUI. The pad is the only thing that knows where a
/// cluster actually ENDED UP: the editor knows the six fractions it asked for, and the two differ
/// whenever a clamp bit, which is most of the time near an edge. An outline positioned from the
/// fractions would drift away from the control it is supposed to be pointing at, and it would
/// drift furthest exactly where the user is most likely to be aiming.
///
/// It is also why this is a sibling of the chips rather than a change to them: at a low opacity
/// setting the pad is close to invisible, and a grab target you cannot see is a broken editor. The
/// outline is kept at full strength while the controls preview the real opacity. See
/// `TouchControlsView.applyOpacity`.
final class ClusterHandle: UIView {
    /// The same red as `ShellPalette.accent`, restated as a `UIColor`.
    ///
    /// Restated rather than imported, because the palette is a SwiftUI type owned by the library
    /// shell and this file sits underneath it: the pad knows nothing about the shell and reaching
    /// up for a colour would invert that. The cost is one number to keep in step, which is why it
    /// is named here.
    private static let accent = UIColor(red: 0.93, green: 0.16, blue: 0.29, alpha: 1)

    private let caption = UILabel()

    init(title: String) {
        super.init(frame: .zero)
        // Every touch has to reach the pad, which is the only thing that can tell a drag of one
        // cluster from a drag of the other. Same reason as `ControlChip`.
        isUserInteractionEnabled = false
        layer.borderWidth = 2
        layer.cornerRadius = 12
        caption.text = title
        caption.font = .systemFont(ofSize: 10, weight: .bold)
        caption.textColor = .white
        caption.textAlignment = .center
        caption.adjustsFontSizeToFitWidth = true
        caption.minimumScaleFactor = 0.7
        caption.isUserInteractionEnabled = false
        addSubview(caption)
        setActive(false)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not supported") }

    override func layoutSubviews() {
        super.layoutSubviews()
        // Inside the outline at the top, where a cluster has no control drawn: the D-pad's top
        // arm is the middle third, and the face cluster's top button is centred, so a caption
        // pinned to the top-left corner of the outline sits over empty space in both.
        caption.frame = CGRect(x: 6, y: 4, width: max(0, bounds.width - 12), height: 12)
    }

    /// Updates the caption when the preview console changes (e.g. START vs RUN vs RESET).
    func setTitle(_ title: String) {
        caption.text = title
    }

    /// Thickens while this control is being dragged, so a finger that has wandered still says
    /// which outline it is carrying.
    func setActive(_ active: Bool) {
        backgroundColor = Self.accent.withAlphaComponent(active ? 0.24 : 0.10)
        layer.borderColor = Self.accent.withAlphaComponent(active ? 1.0 : 0.8).cgColor
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

    /// Minimum fraction of the view height reserved for the picture in portrait.
    ///
    /// The pad used to fall back to drawing the game full-height whenever the topmost control
    /// left less than 20 percent free (reported as "controls reach 188 points from the top of
    /// 956"). That fallback put controls on top of the game. Clamping every control into the
    /// band below this floor keeps a usable strip clear by construction instead.
    private static let minClearPortraitFraction: CGFloat = 0.28

    /// Breathing room inside the safe area, so nothing sits against a rounded corner.
    private static let edgeMargin: CGFloat = 10

    /// Hit areas are grown by this much, because a thumb aiming at the edge of a button should
    /// still get it.
    private static let hitSlop: CGFloat = 5

    /// How far outside a cluster its editing outline is drawn, and therefore how far outside it
    /// can be grabbed. One number for both on purpose: the outline the user can see IS the area
    /// that responds, so there is no invisible margin to discover by accident.
    /// How far outside a cluster's own bounds the editor's outline is drawn and responds.
    ///
    /// Raised from 7 after the editor was tried on a real phone and grabbing a group was reported
    /// as unreliable. Seven points is under a millimetre and a half: it means a finger aiming at
    /// the outline itself, rather than at a button inside it, misses more often than not. Twelve
    /// is still tight enough that the two outlines do not merge at the default layout, and a genuine
    /// overlap resolves to the nearer centre rather than ambiguously, so being generous here costs
    /// nothing. This is the hit area AND the drawn outline, deliberately: an outline that was not
    /// the responsive area would be a drawing that lies about where to put your finger.
    private static let handleInset: CGFloat = 12

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
            applyOpacity()
            setNeedsLayout()
        }
    }

    /// The running game's display aspect ratio, when one is known.
    ///
    /// Needed only so this view can work out where inside the free area the picture is actually
    /// drawn, which is where a touch screen has to be. It is passed in from the same
    /// `EngineHost.activePictureAspect` that `RootView` positions the canvas with, rather than
    /// derived here from the system, because two routes to that number would eventually disagree
    /// and the symptom would be a stylus offset from the finger by the difference.
    var pictureAspect: CGFloat? {
        didSet {
            guard pictureAspect != oldValue else { return }
            setNeedsLayout()
        }
    }

    /// True while the layout editor owns this pad.
    ///
    /// The mode swap is total rather than additive: an editing pad sends NOTHING to the engine and
    /// its touches move clusters instead of pressing buttons. Trying to do both at once was
    /// rejected on purpose. A drag that also counted as a press would fire a button on every
    /// adjustment, and the alternative of a modifier or a long press would mean a user aiming at a
    /// cluster in a full-screen editor could still miss and shoot. Nothing is running while the
    /// editor is up, because it is reached from Settings and Settings only exists with no game on
    /// screen, so there is no input to preserve either.
    var isEditing: Bool = false {
        didSet {
            guard isEditing != oldValue else { return }
            // Both directions drop everything held. Entering with a button down would leave it
            // pressed with no touch left to release it, and leaving mid-drag would keep a handle
            // lit over a cluster nobody is carrying.
            releaseAll()
            floatingPinch?.isEnabled = isEditing
            applyOpacity()
            setNeedsLayout()
        }
    }

    @objc private func floatingPinched(_ recogniser: UIPinchGestureRecognizer) {
        guard isEditing else { return }
        floating.pinch(scale: recogniser.scale, state: recogniser.state)
    }

    /// True while the layout editor's system list is on screen.
    ///
    /// The list is drawn above this pad, but a row whose point lands on an outline still loses
    /// the tap if this view answers `point(inside:)` with yes: UIKit then delivers the touch
    /// here, the row never runs, and the preview console does not change. The editor sets this
    /// for the whole time the list is open, including a dismissal that picks nothing, and
    /// clears it when the list is gone. Ignored while a game is actually playing (`isEditing`
    /// is false there). See `claimsEditingHit(onOutline:menuListOpen:)`.
    var editingHitsSuspended = false

    /// Called while a cluster is dragged in editing mode, with the layout that drag produced.
    ///
    /// `settled` is true on the touch that ENDS the drag, and it is the signal to write the value
    /// somewhere permanent. Every move reports too, because the preview has to follow the finger,
    /// but a caller that persisted on each of those would encode and store a layout sixty times a
    /// second for as long as a finger is down.
    ///
    /// The layout handed over is already sanitised, so a caller cannot be given a value the pad
    /// would then refuse.
    var onLayoutEdited: ((_ layout: TouchLayout, _ settled: Bool) -> Void)?

    /// The overlap check's verdict, published on every CHANGE including back to nil.
    ///
    /// Separate from `onDiagnostic`, which only ever fires on a real collision: the player's status
    /// line must not be overwritten with "all clear" on every layout pass. An editor needs the
    /// other half, because a warning that cannot be taken down again would accuse the user of a
    /// collision they had just dragged their way out of.
    var onOverlapState: ((String?) -> Void)?

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

    /// Full-bleed controller skin art from an imported .deltaskin (PDF/PNG). Drawn behind chips.
    var skinArtwork: UIImage? {
        didSet {
            guard skinArtwork !== oldValue else { return }
            skinImageView.image = skinArtwork
            skinImageView.isHidden = skinArtwork == nil
            applyOpacity()
            setNeedsLayout()
        }
    }

    /// First Delta `screens[].outputFrame` as fractions of mappingSize (= this view when art is up).
    /// When set, the game picture uses this hole instead of the free band between controls.
    var skinScreenNormalized: DeltaSkinNormalizedRect? {
        didSet {
            guard skinScreenNormalized != oldValue else { return }
            setNeedsLayout()
        }
    }

    /// Portrait (or only) representation's mappingSize. Zero means no imported skin canvas,
    /// and the procedural layout is used.
    var skinMapping: CGSize = .zero {
        didSet { if oldValue != skinMapping { setNeedsLayout() } }
    }

    /// Landscape representation. Used only when the view is wider than it is tall.
    var landscapeArtwork: UIImage? {
        didSet {
            guard landscapeArtwork !== oldValue else { return }
            applyOpacity()
            setNeedsLayout()
        }
    }
    var landscapeScreen: DeltaSkinNormalizedRect? {
        didSet { if oldValue != landscapeScreen { setNeedsLayout() } }
    }
    var landscapeMapping: CGSize = .zero {
        didSet { if oldValue != landscapeMapping { setNeedsLayout() } }
    }
    var landscapeLayout: TouchLayout? {
        didSet { if oldValue != landscapeLayout { setNeedsLayout() } }
    }

    /// Landscape-skin drags. Portrait drags still use `onLayoutEdited`.
    var onLandscapeLayoutEdited: ((TouchLayout, Bool) -> Void)?

    /// Buttons, sticks and screen holes for the portrait skin. Empty when there is no skin.
    var portraitFace = SkinPadFace() {
        didSet { setNeedsLayout() }
    }
    /// Same, for the landscape representation. Used only when the view is wider than it is tall.
    var landscapeFace = SkinPadFace() {
        didSet { setNeedsLayout() }
    }
    /// The holes to cut, in skin fractions. The player forwards these to the renderer.
    /// Empty means aspect-fit the whole framebuffer again.
    var onSkinHoles: (([DeltaSkinScreen]) -> Void)?

    /// True while a landscape skin's own layout is what the last pass placed.
    private var placingLandscapeSkin = false

    var isClusterDragging: Bool { clusterDrag != nil }

    private struct ResolvedSkin {
        var artwork: UIImage?
        var screen: DeltaSkinNormalizedRect?
        var mapping: CGSize
        var layout: TouchLayout
        var usesLandscapeLayout: Bool
        var face: SkinPadFace
    }

    /// The skin face for the current orientation. Portrait fractions are never applied to a
    /// landscape view: the canvas is aspect-fit, and a stored landscape face replaces it.
    private func resolvedSkin() -> ResolvedSkin? {
        let wide = bounds.width > bounds.height
        if wide, landscapeMapping.width > 0, landscapeMapping.height > 0 {
            var face = landscapeFace
            if face.screens.isEmpty, let landscapeScreen {
                face.screens = [DeltaSkinScreen(output: landscapeScreen, inputX: 0, inputY: 0,
                                                inputWidth: 0, inputHeight: 0)]
            }
            return ResolvedSkin(
                artwork: landscapeArtwork,
                screen: face.screens.first?.output ?? landscapeScreen,
                mapping: landscapeMapping,
                layout: landscapeLayout ?? layout,
                usesLandscapeLayout: landscapeLayout != nil,
                face: face
            )
        }
        if skinMapping.width > 0, skinMapping.height > 0 {
            var face = portraitFace
            if face.screens.isEmpty, let skinScreenNormalized {
                face.screens = [DeltaSkinScreen(output: skinScreenNormalized, inputX: 0, inputY: 0,
                                                inputWidth: 0, inputHeight: 0)]
            }
            return ResolvedSkin(
                artwork: skinArtwork,
                screen: face.screens.first?.output ?? skinScreenNormalized,
                mapping: skinMapping,
                layout: layout,
                usesLandscapeLayout: false,
                face: face
            )
        }
        return nil
    }

    /// Drawn under the procedural chips so a Delta PDF/PNG shows through.
    private let skinImageView: UIImageView = {
        let view = UIImageView()
        view.contentMode = .scaleToFill
        view.isUserInteractionEnabled = false
        view.isHidden = true
        view.accessibilityIdentifier = "deltaSkinArtwork"
        return view
    }()

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
        /// A finger on the emulated touch screen, carrying the point in this view's coordinates.
        ///
        /// Tracked on move like the D-pad and unlike a chip, because a stylus that did not follow
        /// the finger would make every DS game that asks you to draw, drag or slide unplayable.
        /// Deliberately NOT sticky: a finger that leaves the touch screen lifts the stylus, because
        /// sliding off the digitiser is a release on the hardware too.
        case pointer(CGPoint)
        /// A finger on a skin's analog stick. The point is in that stick's local coordinates
        /// (origin top-left of the stick rect). `side` is "left" or "right".
        case stick(side: String, local: CGPoint)
        /// A skin button that this system's procedural pad does not draw (3DS Home / menu).
        case skinSlot(PadSlot)
        /// A skin button that runs a function, is a switch, or holds a combo. The index is into
        /// the face's `buttons`, looked up again on every recompute.
        case special(Int)
        /// An extra button the player placed (`FloatingButtons.swift`). Sticky like a chip. Keyed
        /// by the button's id rather than its index, so a relayout cannot move a held finger onto
        /// a different button.
        case floating(UUID)
    }

    /// Keyed on `UITouch` identity, which is stable across began, moved, ended and cancelled and
    /// is therefore the only reliable way to know which finger let go of what.
    private var grabs: [ObjectIdentifier: Grab] = [:]

    /// Which outlined control a drag is carrying.
    ///
    /// The D-pad is one surface, so it stays one target. Every face button, shoulder, and system
    /// pill is its own target: face/shoulders write `buttonFrees`, SELECT/START keep their
    /// dedicated centres. There is no remaining "BUTTONS" group box.
    private enum DragTarget: Equatable {
        case dpad
        case chip(Int)
    }

    /// One cluster being dragged, and enough to finish the drag without re-deriving anything.
    ///
    /// `offset` is the gap between where the finger landed and the cluster's centre, so a cluster
    /// grabbed by its corner does not jump its centre under the fingertip on the first move.
    /// `layout` is the last value published, which is what a cancelled touch settles on: a
    /// cancellation has a location, but it is the location of an interruption rather than of an
    /// intention.
    private struct ClusterDrag {
        let touch: ObjectIdentifier
        let target: DragTarget
        let offset: CGSize
        var layout: TouchLayout
    }

    /// The drag in progress, or nil.
    ///
    /// ONE AT A TIME, unlike the pad's real touches. Two fingers dragging both clusters would be
    /// writing two halves of one value from two independent streams, and each would be editing a
    /// layout the other had already moved on from. The pad needs multi-touch because a game does;
    /// an editor does not.
    private var clusterDrag: ClusterDrag?

    private var chips: [ControlChip] = []
    private let dpad = DPadView()

    /// D-pad editing outline. Built once; per-chip outlines are rebuilt with the control set.
    private let dpadHandle = ClusterHandle(title: "D-PAD")

    /// One outline per chip (face, shoulders, SELECT/START). Rebuilt in `rebuild` so previewing
    /// another console gets that console's labels on the outlines, not leftover PS1 names.
    private var chipHandles: [ClusterHandle] = []

    /// Hit rectangles in this view's coordinate space, rebuilt on every layout.
    private var chipRects: [CGRect] = []
    private var dpadRect: CGRect = .zero

    /// What the last layout pass worked out, kept only for the editor's drag arithmetic.
    ///
    /// Recorded rather than recomputed because a drag has to invert exactly the mapping the layout
    /// pass used. Two copies of "where does fraction 0.17 land" would eventually disagree, and the
    /// symptom would be a cluster that creeps away from the finger.
    private var editPlayArea: CGRect = .zero
    private var dpadCentreNow: CGPoint = .zero
    private var chipCentreNow: [CGPoint] = []

    /// The D-pad hit area while editing (the surface alone; shoulders have their own outlines).
    private var dpadGroupRect: CGRect = .zero

    /// The frame the render loop reads. Recomputed from `grabs` on every touch event, never
    /// mutated incrementally, so two fingers on one button, a cancelled touch and a gesture
    /// recogniser stealing a sequence all resolve correctly with no counter to get wrong.
    private var padState: PadFrame = .released

    /// The last overlap report, so the same line is not repeated on every layout pass.
    private var lastOverlapReport: String?

    /// The last picture area published, so an unchanged layout does not resize the surface.
    private var lastPictureArea: CGRect?

    /// Where the touch screen is on the glass, in this view's coordinates, or nil when the running
    /// system has none or the picture has not been laid out yet.
    ///
    /// Recomputed on every layout pass rather than on demand, because a touch has to be answered
    /// from whatever the last layout decided and a rotation must not leave a stale rect behind.
    private var touchScreenRect: CGRect?

    /// The last place the stylus was, as a framebuffer fraction, so a released frame reports where
    /// the finger lifted instead of the top-left corner.
    private var lastPointerFraction: CGPoint = .zero

    /// The picture view the engine's touch fractions are relative to, in this view's coordinates.
    /// Set together with `touchScreenRect` when a `touchMapper` placed it.
    private var touchPictureRect: CGRect?

    /// Asks the engine where the touch screen is. Nil falls back to `system.touchScreen`, which is
    /// the hardware's stacked layout and nothing else.
    /// Not observed: closures cannot be compared, and re-laying out on every SwiftUI update would
    /// be churn. Changes that matter arrive with `screenLayoutVersion`.
    var touchMapper: TouchScreenMapper?

    /// Bumped by the host when the engine's screen layout changed (swap, layout choice, a TV
    /// connecting), so the touch screen is re-asked even though nothing here moved.
    var screenLayoutVersion: Int = 0 {
        didSet {
            guard screenLayoutVersion != oldValue else { return }
            lastHoleSignature = ""
            setNeedsLayout()
        }
    }

    // ------------------------------------------------------------------ trackpad mode

    /// The picture as a trackpad: drag moves the mouse, tap is a left click, two-finger tap is a
    /// right click, three-finger tap a middle click, and press-and-hold then drag holds the left
    /// button. Controls still win wherever they are.
    var trackpadEnabled = false {
        didSet {
            guard trackpadEnabled != oldValue else { return }
            // Switching off mid-click must still deliver one released frame, or the core keeps
            // the button the last frame said was down.
            owesMouseRelease = oldValue
            trackpadTouches.removeAll()
            pendingMouse = .zero
            leftPulse = 0
            rightPulse = 0
            middlePulse = 0
        }
    }

    private struct TrackpadTouch {
        var last: CGPoint
        let start: CGPoint
        let began: TimeInterval
        var moved: Bool
        var holding: Bool
    }

    private var trackpadTouches: [ObjectIdentifier: TrackpadTouch] = [:]
    /// Set when trackpad mode was switched off, so one released mouse frame is still sent.
    private var owesMouseRelease = false
    /// Most fingers down at once during the current trackpad gesture.
    private var trackpadFingers = 0
    /// Whether any finger in the current gesture moved past the tap slop.
    private var trackpadGestureMoved = false
    private var pendingMouse = CGPoint.zero
    private var leftPulse = 0
    private var rightPulse = 0
    private var middlePulse = 0

    /// Mouse units per point of finger travel.
    static let trackpadSensitivity: CGFloat = 1.0
    /// Travel under this is still a tap.
    static let trackpadTapSlop: CGFloat = 8
    /// A tap is shorter than this.
    static let trackpadTapTime: TimeInterval = 0.3
    /// Still for this long, then drag: the left button is held for the drag.
    static let trackpadHoldTime: TimeInterval = 0.45
    /// Frames a click is held for, so a core polling once a frame cannot miss it.
    static let trackpadClickFrames = 3

    /// The area the trackpad answers in: the picture, or the skin canvas.
    private var trackpadArea: CGRect? {
        guard trackpadEnabled else { return nil }
        return lastPictureArea
    }

    /// The mouse for the frame about to run. Consumes the motion and one frame of any click.
    func takeMouseFrame() -> MouseFrame? {
        guard trackpadEnabled else {
            if owesMouseRelease {
                owesMouseRelease = false
                return MouseFrame(dx: 0, dy: 0, left: false, right: false, middle: false)
            }
            return nil
        }
        let now = ProcessInfo.processInfo.systemUptime
        var holding = false
        for (key, touch) in trackpadTouches {
            var touch = touch
            if !touch.moved && trackpadFingers == 1 && now - touch.began >= Self.trackpadHoldTime {
                touch.holding = true
                trackpadTouches[key] = touch
            }
            holding = holding || touch.holding
        }
        let frame = MouseFrame(dx: Float(pendingMouse.x),
                               dy: Float(pendingMouse.y),
                               left: holding || leftPulse > 0,
                               right: rightPulse > 0,
                               middle: middlePulse > 0)
        pendingMouse = .zero
        leftPulse = max(0, leftPulse - 1)
        rightPulse = max(0, rightPulse - 1)
        middlePulse = max(0, middlePulse - 1)
        return frame
    }

    private func trackpadBegan(_ touch: UITouch, at point: CGPoint) {
        if trackpadTouches.isEmpty {
            trackpadFingers = 0
            trackpadGestureMoved = false
        }
        trackpadTouches[ObjectIdentifier(touch)] = TrackpadTouch(
            last: point, start: point, began: touch.timestamp, moved: false, holding: false)
        trackpadFingers = max(trackpadFingers, trackpadTouches.count)
    }

    private func trackpadMoved(_ touch: UITouch) {
        let key = ObjectIdentifier(touch)
        guard var state = trackpadTouches[key] else { return }
        let point = touch.location(in: self)
        // One finger steers. With two or more down the gesture is a click being formed, and
        // moving the cursor while the second finger lands would put the click in the wrong place.
        if trackpadTouches.count == 1 {
            pendingMouse.x += (point.x - state.last.x) * Self.trackpadSensitivity
            pendingMouse.y += (point.y - state.last.y) * Self.trackpadSensitivity
        }
        state.last = point
        let travel = hypot(point.x - state.start.x, point.y - state.start.y)
        if !state.moved && travel > Self.trackpadTapSlop {
            state.moved = true
            trackpadGestureMoved = true
            if trackpadFingers == 1 && touch.timestamp - state.began >= Self.trackpadHoldTime {
                state.holding = true
            }
        }
        trackpadTouches[key] = state
    }

    private func trackpadEnded(_ touch: UITouch) {
        let key = ObjectIdentifier(touch)
        guard let state = trackpadTouches.removeValue(forKey: key) else { return }
        guard trackpadTouches.isEmpty else { return }
        // The whole gesture is over. A short one that never moved is a click.
        let short = touch.timestamp - state.began < Self.trackpadTapTime
        if !trackpadGestureMoved && !state.holding && short {
            switch trackpadFingers {
            case 1: leftPulse = Self.trackpadClickFrames
            case 2: rightPulse = Self.trackpadClickFrames
            default: middlePulse = Self.trackpadClickFrames
            }
        }
        trackpadFingers = 0
        trackpadGestureMoved = false
    }

    /// Skin analog sticks, in this view's coordinates. Hit before the D-pad so a circle pad
    /// is not the digital pad underneath it.
    private var stickHits: [(side: String, rect: CGRect)] = []
    /// Skin buttons with no procedural chip (Home / menu). 
    private var extraHits: [(slot: PadSlot, rect: CGRect)] = []
    /// Skin buttons that run a function, are switches, or hold a combo, by index in the face.
    private var specialHits: [(index: Int, rect: CGRect)] = []
    private var specialButtons: [Int: DeltaSkinButton] = [:]
    /// Where each special button's art was laid out, so a switch's knob can be moved later.
    private var specialRects: [Int: CGRect] = [:]
    /// Special buttons held on the last recompute, so press and release are each acted on once.
    private var lastSpecialHeld: Set<Int> = []
    /// Switch positions, keyed by `switchKey`. Bound switches are re-read from the real state on
    /// every layout, so they show the truth when the skin loads.
    private var switchOn: [String: Bool] = [:]
    /// Which skin and orientation the switches belong to, part of every `switchKey`.
    private var switchScope = ""
    /// Which slots an art view stands for, and which special button.
    private var artSlots: [String: [PadSlot]] = [:]
    private var artSpecial: [String: Int] = [:]
    /// A switch's "on" picture, by art key.
    private var artSelected: [String: UIImage] = [:]
    /// Art views currently drawn pressed by the press animation.
    private var artDown: Set<String> = []
    /// The skin editor's opacity, the alpha art returns to after a press.
    private var currentSkinAlpha: CGFloat = 1
    /// The sound the current skin plays on a press.
    private var skinSoundURL: URL?
    /// The hidden-controls state last applied, so a change re-applies the opacities once.
    private var hiddenApplied = false
    /// Where a skin function goes. Wired by `TouchControlsHost` to the `PadInputSource` box.
    var onSkinFunction: ((SkinFunction, Bool) -> Void)?
    var skinFunctionState: ((SkinFunction) -> Bool?)?
    /// Per-button and stick images. Hidden when the skin did not name a file.
    private var artViews: [String: UIImageView] = [:]
    /// Which images a press may swap to. Nil pressed means the normal image stays.
    private var artNormal: [String: UIImage] = [:]
    private var artPressed: [String: UIImage] = [:]
    /// The holes last reported, so a layout that did not move them does not reset the renderer.
    private var lastHoleSignature: String = ""

    /// Extra buttons the player placed. Draws, hit-tests and (in the editor) drags them.
    private let floating: FloatingButtonLayer
    /// Resizes the selected extra button in the editor. Disabled while playing.
    private var floatingPinch: UIPinchGestureRecognizer?
    /// Where an extra button's app action goes. Wired by `TouchControlsHost` to the
    /// `PadInputSource` box, so the player screen needed no new parameter.
    var onAppAction: ((PadAppAction, Bool) -> Void)?
    /// Action buttons held on the last recompute, so a press and a release are each sent once.
    private var heldActionButtons: [UUID: PadAppAction] = [:]
    /// What was down on the last recompute, so a tap is played on a NEW press only.
    private var lastHapticPressed: Set<PadSlot> = []
    private var lastHapticFloating: Set<UUID> = []

    // ------------------------------------------------------------------ life cycle

    init(system: GameSystem, layout: TouchLayout) {
        self.system = system
        self.layout = layout.sanitised
        self.floating = FloatingButtonLayer()
        super.init(frame: .zero)
        floating.attach(to: self)
        floating.onChange = { [weak self] in self?.setNeedsLayout() }
        let pinch = UIPinchGestureRecognizer(target: self, action: #selector(floatingPinched(_:)))
        // Off until the editor turns it on: a pinch recogniser over a live pad would cancel the
        // touches of a player who happens to press two buttons and spread their thumbs.
        pinch.isEnabled = false
        pinch.cancelsTouchesInView = false
        addGestureRecognizer(pinch)
        floatingPinch = pinch
        // The reason this class exists. Without it UIKit delivers one touch and holding a
        // direction while pressing a face button, which is most of playing a game, is impossible.
        isMultipleTouchEnabled = true
        backgroundColor = .clear
        insertSubview(skinImageView, at: 0)
        addSubview(dpad)
        // D-pad outline added once; per-chip outlines arrive in `rebuild` with the control set.
        dpadHandle.isHidden = true
        addSubview(dpadHandle)
        applyOpacity()
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
        } else {
            // Read once so the stored strength is applied, then spin the Taptic Engine up so the
            // first press of the session is not the slow one.
            _ = ControlFeel.shared
            ButtonHaptics.shared.prepare()
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
        trackpadTouches.removeAll()
        pendingMouse = .zero
        leftPulse = 0
        rightPulse = 0
        middlePulse = 0
        // A drag is abandoned rather than settled. Nothing is published, because the value the
        // caller already has from the last move is the last thing the user actually saw, and
        // reporting a settle from a teardown would persist a layout on the way out of a screen the
        // user may have been leaving to get away from it.
        clusterDrag = nil
        floating.cancelEdit()
        dpadHandle.setActive(false)
        for handle in chipHandles {
            handle.setActive(false)
        }
        recompute()
    }

    // ------------------------------------------------------------------ building

    private func rebuild() {
        // A held button from the previous system's pad must not survive into the new one.
        grabs.removeAll()
        clusterDrag = nil
        // Nor may the previous system's touch screen. Cleared here rather than left to the next
        // layout pass, because until that pass runs a DS-shaped rect would be claiming touches
        // over an NES game's picture, and `point(inside:)` is answered from whatever this holds.
        touchScreenRect = nil

        for chip in chips {
            chip.removeFromSuperview()
        }
        chips = system.controls.map { ControlChip(control: $0) }
        for chip in chips {
            addSubview(chip)
        }
        for handle in chipHandles {
            handle.removeFromSuperview()
        }
        // Fresh outlines so a preview console change cannot leave PS1 captions on NES chips.
        chipHandles = chips.map { chip in
            let handle = ClusterHandle(title: chip.control.label)
            handle.isHidden = true
            addSubview(handle)
            return handle
        }
        // The D-pad stays behind the chips, which only matters if a future layout puts them
        // close enough to touch.
        bringSubviewToFront(dpad)
        // The outlines stay in front of everything, including the chips just added.
        bringSubviewToFront(dpadHandle)
        for handle in chipHandles {
            bringSubviewToFront(handle)
        }
        // New chips arrive fully opaque, so the previewed opacity has to be re-applied to them.
        applyOpacity()
        recompute()
        // Cleared THROUGH the publisher rather than by assigning nil, so a listener is told. A
        // different system has a different control set, so last system's collision is not this
        // system's problem, and an editor whose warning could not be taken down by switching the
        // preview would be accusing the user of an overlap that is no longer on screen. The next
        // layout pass re-checks and re-reports if the new set collides too.
        setOverlapReport(nil)
        setNeedsLayout()
    }

    /// Puts the previewed opacity where it belongs for the current mode.
    ///
    /// Normally on this VIEW, which is one property to set and fades the whole pad together. While
    /// editing, on the controls INDIVIDUALLY, so the outlines can stay at full strength: `alpha` is
    /// inherited by subviews, so a pad faded to the minimum 0.15 would take its own grab targets
    /// down with it and leave the user dragging something they cannot see. The controls still show
    /// the real setting, which is the point of previewing it at all.
    private func applyOpacity() {
        let previewed = CGFloat(layout.sanitised.opacity)
        let hasSkin = skinArtwork != nil || landscapeArtwork != nil
        // Skin art stays fully opaque; procedural chrome fades so Delta PDFs are visible.
        // In the editor, keep chips readable enough to drag (floor 0.35) even with a skin.
        skinImageView.alpha = 1
        if isEditing {
            alpha = 1
            let chipAlpha = hasSkin ? max(previewed, 0.35) : previewed
            dpad.alpha = chipAlpha
            for chip in chips {
                chip.alpha = chipAlpha
            }
        } else if hasSkin {
            // View stays opaque so the skin does not inherit the layout opacity slider.
            alpha = 1
            let ghost = max(0.08, previewed * 0.2)
            dpad.alpha = ghost
            for chip in chips {
                chip.alpha = ghost
            }
        } else {
            alpha = previewed
            dpad.alpha = 1
            for chip in chips {
                chip.alpha = 1
            }
        }
        applyHiddenControls()
    }

    /// True while the `toggleControlls` function has hidden the controls. Never in the editor.
    private var controlsHiddenNow: Bool {
        !isEditing && SkinRuntime.shared.controlsHidden
    }

    /// Hidden controls are invisible, not gone: every control still answers a touch where it is,
    /// and the button that hides them stays visible so the player can bring them back. The view
    /// itself keeps its alpha, because a view below 0.01 stops receiving touches at all.
    private func applyHiddenControls() {
        guard controlsHiddenNow else { return }
        skinImageView.alpha = 0
        dpad.alpha = 0
        for chip in chips {
            chip.alpha = 0
        }
        for (key, view) in artViews {
            view.alpha = keepsVisibleWhenHidden(key) ? currentSkinAlpha : 0
        }
    }

    private func keepsVisibleWhenHidden(_ key: String) -> Bool {
        guard let index = artSpecial[key] else { return false }
        return specialButtons[index]?.skinFunction == .toggleControlls
    }

    // ------------------------------------------------------------------ layout

    override func layoutSubviews() {
        super.layoutSubviews()

        // Hiding or showing the controls re-applies every opacity once, both ways.
        let hidden = controlsHiddenNow
        if hidden != hiddenApplied {
            hiddenApplied = hidden
            applyOpacity()
        }

        let safe = bounds.inset(by: safeAreaInsets)
        let safePlay = safe.insetBy(dx: Self.edgeMargin, dy: Self.edgeMargin)
        let skin = resolvedSkin()
        let skinCanvas = skin.map {
            DeltaSkinNormalizedRect.aspectFitCanvas(mapping: $0.mapping, in: bounds)
        }
        if let skinCanvas {
            skinImageView.image = skin?.artwork
            skinImageView.isHidden = skin?.artwork == nil
            skinImageView.frame = skinCanvas
            sendSubviewToBack(skinImageView)
        } else if !skinImageView.isHidden {
            skinImageView.image = skinArtwork
            skinImageView.frame = bounds
            sendSubviewToBack(skinImageView)
        }

        let play = skinCanvas ?? safePlay
        guard play.width > 0, play.height > 0 else {
            // Nothing sane to lay out. Report it rather than leaving invisible controls that
            // silently swallow nothing: a zero play area means the safe area consumed the view.
            chipRects = [CGRect](repeating: .zero, count: chips.count)
            dpadRect = .zero
            for chip in chips { chip.frame = .zero }
            dpad.frame = .zero
            // No geometry means nothing to grab. Cleared rather than left stale so a drag cannot
            // be started against rectangles from a layout that no longer applies.
            editPlayArea = .zero
            dpadGroupRect = .zero
            chipCentreNow = [CGPoint](repeating: .zero, count: chips.count)
            // The touch screen is in the same position, for the same reason: a stylus answered
            // from a rect this pass just decided does not exist would be reporting a press at a
            // coordinate derived from a layout that is gone.
            clearTouchScreenRect()
            dpadHandle.isHidden = true
            for handle in chipHandles {
                handle.isHidden = true
            }
            report("touch controls: no room to lay out, play area is "
                   + "\(Int(play.width))x\(Int(play.height))")
            return
        }

        let orientLandscape = bounds.width > bounds.height
        // A skin already names where the thumbs sit. Do not restack them with the
        // procedural landscape grip, and do not reserve a portrait strip inside the canvas.
        let landscape = orientLandscape && skinCanvas == nil
        placingLandscapeSkin = skin?.usesLandscapeLayout ?? false
        let unit = self.unit(in: play, landscape: landscape || (orientLandscape && skinCanvas != nil))
        let faceExtent = self.faceHalfExtent()

        // PORTRAIT keeps a clear strip for the game above every control. SELECT and START are no
        // longer pinned to a reserved bottom row; they use the same play-area fractions as the
        // thumb clusters. Without a floor the shoulders of a low cluster used to climb inside the
        // top fifth of a tall phone and the picture publisher fell back to full-height draw.
        let clearTop = (landscape || skinCanvas != nil) ? 0 : play.height * Self.minClearPortraitFraction
        let controlArea = CGRect(x: play.minX, y: play.minY + clearTop,
                                 width: play.width,
                                 height: max(0, play.height - clearTop))

        let dpadHalf = Self.dpadSpan * unit / 2
        // In landscape the clusters sit higher in their columns, because a landscape grip puts the
        // thumbs at the middle of the edge rather than at the bottom corner.
        var live = (skin?.layout ?? layout).sanitised
        live.scale = layout.sanitised.scale
        live.opacity = layout.sanitised.opacity
        let clusterY: CGFloat? = landscape ? Self.landscapeClusterY : nil
        // Shoulder stacks hang ABOVE the cluster centres. Count them into the top half-extent so
        // clamping into `controlArea` keeps the whole group under the portrait clear strip, not
        // just the D-pad / face diamond.
        let leftShoulderRows = CGFloat(
            system.controls.filter { $0.cluster == .shoulderLeft }.count
        )
        let rightShoulderRows = CGFloat(
            system.controls.filter { $0.cluster == .shoulderRight }.count
        )
        let dpadTopExtent = dpadHalf + (leftShoulderRows > 0
            ? (leftShoulderRows * 1.3 + 0.45) * unit
            : 0)
        let faceTopExtent = faceExtent.height * unit + (rightShoulderRows > 0
            ? (rightShoulderRows * 1.3 + 0.45) * unit
            : 0)
        let dpadCentre = clampedCentre(
            fractionX: CGFloat(live.dpadX), fractionY: clusterY ?? CGFloat(live.dpadY),
            halfWidth: dpadHalf,
            halfHeightTop: dpadTopExtent, halfHeightBottom: dpadHalf,
            into: controlArea, wholePlayArea: play
        )
        let faceCentre = clampedCentre(
            fractionX: CGFloat(live.faceX), fractionY: clusterY ?? CGFloat(live.faceY),
            halfWidth: faceExtent.width * unit,
            halfHeightTop: faceTopExtent, halfHeightBottom: faceExtent.height * unit,
            into: controlArea, wholePlayArea: play
        )

        dpadRect = CGRect(x: dpadCentre.x - dpadHalf, y: dpadCentre.y - dpadHalf,
                          width: dpadHalf * 2, height: dpadHalf * 2)
        dpadRect = Self.clamp(dpadRect, into: (landscape || skinCanvas != nil) ? play : controlArea)
        dpad.frame = dpadRect
        dpadCentreNow = CGPoint(x: dpadRect.midX, y: dpadRect.midY)

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
        chipCentreNow = []
        chipCentreNow.reserveCapacity(chips.count)
        for chip in chips {
            let control = chip.control
            let size = self.size(of: control, unit: unit)
            let halfW = size.width / 2
            let halfH = size.height / 2
            let centre: CGPoint

            switch control.cluster {
            case .face, .shoulderLeft, .shoulderRight:
                // A freed button sits at its own play-area fraction. Otherwise it still rides the
                // cluster anchor plus template offset (shoulders included).
                if let free = live.freeCentre(for: control.slot) {
                    centre = clampedCentre(
                        fractionX: CGFloat(free.x), fractionY: CGFloat(free.y),
                        halfWidth: halfW,
                        halfHeightTop: halfH, halfHeightBottom: halfH,
                        into: controlArea, wholePlayArea: play
                    )
                } else {
                    let anchor: CGPoint
                    switch control.cluster {
                    case .face:
                        anchor = faceCentre
                    case .shoulderLeft:
                        anchor = shoulderLeftAnchor
                    case .shoulderRight:
                        anchor = shoulderRightAnchor
                    default:
                        anchor = faceCentre
                    }
                    centre = CGPoint(x: anchor.x + control.offset.x * unit,
                                     y: anchor.y + control.offset.y * unit)
                }
            case .system:
                // Each system pill has its own stored centre. Offsets on the template are ignored:
                // they described the old fixed bottom row and would fight the editable position.
                let isSelect = control.slot == .select
                centre = clampedCentre(
                    fractionX: CGFloat(isSelect ? live.selectX : live.startX),
                    fractionY: CGFloat(isSelect ? live.selectY : live.startY),
                    halfWidth: halfW,
                    halfHeightTop: halfH, halfHeightBottom: halfH,
                    into: controlArea, wholePlayArea: play
                )
            case .dpad:
                // No chip uses the D-pad cluster; directions are the surface itself.
                centre = faceCentre
            }

            var rect = CGRect(x: centre.x - size.width / 2, y: centre.y - size.height / 2,
                              width: size.width, height: size.height)
            // Last line of defence: stay on screen, and in portrait stay out of the game strip.
            rect = Self.clamp(rect, into: (landscape || skinCanvas != nil) ? play : controlArea)
            chip.frame = rect
            chipRects.append(rect)
            chipCentreNow.append(CGPoint(x: rect.midX, y: rect.midY))
        }

        // Recorded from the values this pass actually used, so a drag inverts the same mapping
        // rather than a second copy of it.
        editPlayArea = play
        // dpadCentreNow already recorded from the clamped dpad rect above.
        layoutHandles()

        verifyNoOverlap()
        if let skin, let skinCanvas, !isEditing {
            applyDeclaredSkinControls(skin: skin, canvas: skinCanvas)
        } else {
            clearDeclaredSkinArt()
        }
        publishPictureArea(landscape: orientLandscape, skinCanvas: skinCanvas, skin: skin)
        // Last, so extra buttons sit above the pad, the skin art and the editing outlines.
        floating.layout(in: bounds, system: system, editing: isEditing)
    }

    /// Works out what each drag would carry, and outlines it.
    ///
    /// The D-pad keeps one outline over the direction surface alone. Every chip — face buttons,
    /// L1/L2/R1/R2 (and L/R), SELECT/START — gets its own outline from its chip rect, labelled
    /// with that chip's system-appropriate caption from `rebuild`.
    private func layoutHandles() {
        guard isEditing else {
            dpadHandle.isHidden = true
            for handle in chipHandles {
                handle.isHidden = true
            }
            return
        }

        dpadGroupRect = dpadRect
        dpadHandle.isHidden = dpadGroupRect.isEmpty
        dpadHandle.frame = Self.clamp(Self.grown(dpadGroupRect), into: bounds)

        for index in chips.indices {
            guard index < chipHandles.count, index < chipRects.count else { continue }
            let rect = chipRects[index]
            let handle = chipHandles[index]
            handle.setTitle(chips[index].control.label)
            handle.isHidden = rect.isEmpty
            handle.frame = Self.clamp(Self.grown(rect), into: bounds)
        }
        // Drop any leftover handles if the control set shrank (should not happen after rebuild).
        if chipHandles.count > chips.count {
            for handle in chipHandles[chips.count...] {
                handle.isHidden = true
            }
        }
    }

    /// A group rectangle grown into the area its outline occupies and responds over.
    private static func grown(_ rect: CGRect) -> CGRect {
        guard !rect.isEmpty else { return rect }
        return rect.insetBy(dx: -handleInset, dy: -handleInset)
    }

    /// Hands the player screen the rect the picture may use.
    ///
    /// Derived from the control rects that were just laid out, so it cannot drift from them. In
    /// portrait the picture takes everything above the topmost control; in landscape it takes the
    /// column between the left group and the right group. Either way no control sits on the game,
    /// which is the requirement docs/mobile-player.png failed.
    private func publishPictureArea(landscape: Bool, skinCanvas: CGRect? = nil,
                                    skin: ResolvedSkin? = nil) {
        // A hole is a fraction of the skin's mapping, drawn inside the aspect-fit canvas.
        // The metal view is that canvas, and each hole is a crop of the framebuffer inside
        // it. Publishing only the first hole was what left the bottom screen empty and let
        // the fallback strip float above the skin.
        let screens = skin?.face.screens ?? []
        if let skinCanvas, !screens.isEmpty {
            maskSkinArtwork(canvas: skinCanvas, screens: screens)
            // Holes first, so the engine already has them when it is asked where the touch
            // screen landed.
            deliverSkinHoles(screens)
            placeDigitiser(on: screens, canvas: skinCanvas)
            deliverPictureArea(skinCanvas)
            return
        }
        skinImageView.layer.mask = nil
        deliverSkinHoles([])

        // No hole in the file: the fallback strip above the buttons. That strip is not used
        // once a hole exists (the branch above returned).
        var allRects = chipRects
        allRects.append(dpadRect)
        let occupied = allRects.filter { !$0.isEmpty }
        guard !occupied.isEmpty else {
            // A pad with no controls at all still shows a picture, and on a system with a
            // digitiser that picture is still touchable, so the rect is recomputed against the
            // same area being published rather than left disagreeing with it.
            updateTouchScreenRect(in: bounds)
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
            let limitWidth = skinCanvas?.width ?? bounds.width
            if width < limitWidth * 0.2 {
                if let skinCanvas {
                    // The buttons met, but the game stays in the skin canvas. Giving it the
                    // whole phone is what put PlayStation full-bleed under the controls.
                    area = skinCanvas
                } else {
                    report("touch layout: no clear column for the picture in landscape, "
                           + "the controls span \(Int(bounds.width - width)) of "
                           + "\(Int(bounds.width)) points; drawing full width instead")
                    area = bounds
                }
            } else if let skinCanvas {
                let top = skinCanvas.minY
                area = CGRect(x: leftEdge, y: top, width: width, height: skinCanvas.height)
            } else {
                area = CGRect(x: leftEdge, y: bounds.minY, width: width, height: bounds.height)
            }
        } else {
            let topEdge = occupied.map(\.minY).min() ?? bounds.maxY
            let natural = topEdge - bounds.minY
            // Never fall back to full-height draw: that was the failure mode that put controls on
            // the game. Prefer the reserved clear strip, even if a layout somehow still crowded it.
            let floor = bounds.height * Self.minClearPortraitFraction
            let height = max(natural, floor)
            if natural < floor {
                report("touch layout: portrait clear band forced to \(Int(floor)) of "
                       + "\(Int(bounds.height)) points; controls reached \(Int(natural)) from top")
            }
            area = CGRect(x: bounds.minX, y: bounds.minY,
                          width: bounds.width, height: min(height, bounds.height))
        }
        updateTouchScreenRect(in: area)
        deliverPictureArea(area)
    }

    /// Works out where on the glass the emulated touch screen landed.
    ///
    /// Two steps, and the first is the one that is easy to get wrong: the picture does not fill the
    /// free area, it is letterboxed inside it by `PictureFit` exactly as `RootView` draws it. So the
    /// fitted rect has to be reproduced here before the system's framebuffer fraction can be
    /// applied to it. Mapping the fraction onto the free area instead would put the stylus wherever
    /// the letterbox happened to be thick, which is a miss that grows with the mismatch between the
    /// screen's shape and the game's.
    private func updateTouchScreenRect(in area: CGRect) {
        guard let fraction = system.touchScreen else {
            clearTouchScreenRect()
            return
        }
        // Without a known aspect the free area is the honest best guess, and it is what the canvas
        // is drawn into in that case too, so the two still agree.
        let picture = pictureAspect.map { PictureFit.rect(aspect: $0, in: area) } ?? area
        guard picture.width >= 1, picture.height >= 1 else {
            clearTouchScreenRect()
            return
        }
        if let touchMapper {
            // The engine drew it, so the engine says where it is. `picture` is exactly the rect
            // RootView gives the Metal view, so the engine's fractions are fractions of it.
            placeMappedTouchScreen(touchMapper, in: picture)
            return
        }
        touchPictureRect = nil
        touchScreenRect = CGRect(
            x: picture.minX + fraction.minX * picture.width,
            y: picture.minY + fraction.minY * picture.height,
            width: fraction.width * picture.width,
            height: fraction.height * picture.height
        )
        revalidatePointerGrabs()
    }

    /// Drops the touch screen, and any finger that was resting on it.
    private func clearTouchScreenRect() {
        touchScreenRect = nil
        touchPictureRect = nil
        revalidatePointerGrabs()
    }

    /// Places the touch screen where the engine says it is inside `picture`.
    private func placeMappedTouchScreen(_ mapper: TouchScreenMapper, in picture: CGRect) {
        guard let fraction = mapper.rect(picture.size), fraction.width > 0, fraction.height > 0 else {
            clearTouchScreenRect()
            return
        }
        touchPictureRect = picture
        touchScreenRect = CGRect(
            x: picture.minX + fraction.minX * picture.width,
            y: picture.minY + fraction.minY * picture.height,
            width: fraction.width * picture.width,
            height: fraction.height * picture.height
        )
        revalidatePointerGrabs()
    }

    /// Re-tests every finger currently held on the touch screen against the rect as it is NOW.
    ///
    /// A layout pass can move the digitiser out from under a finger that is already down: a
    /// rotation, the aspect arriving after the first pass, a safe-area change, or a layout with no
    /// room at all. A stationary finger never fires `touchesMoved`, so without this it would go on
    /// reporting a fraction derived from a rect that has moved, and in the worst case report a
    /// press against a rect that no longer exists.
    ///
    /// Only pointer grabs are touched. A button held through a rotation should stay held, which is
    /// why this is not `releaseAll`.
    private func revalidatePointerGrabs() {
        guard !grabs.isEmpty else { return }
        var changed = false
        for (key, grab) in grabs {
            guard case .pointer(let point) = grab else { continue }
            if let touchScreenRect, touchScreenRect.contains(point) {
                continue
            }
            // Dropped rather than re-aimed. The finger has not moved; the screen under it has, and
            // guessing where the user now means to be pointing would be inventing input.
            grabs.removeValue(forKey: key)
            changed = true
        }
        if changed {
            recompute()
        }
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

    /// A control centre from its two fractions, pulled back until the control fits.
    ///
    /// Top and bottom half-extents are separate so a shoulder stack above a cluster can reserve
    /// more room above the centre than below it, which is what keeps the portrait clear strip
    /// free of controls rather than only free of cluster centres.
    ///
    /// `wholePlayArea` is passed separately so a control that cannot fit inside the reduced area
    /// at all still lands somewhere legal instead of being pinned to a negative height rectangle.
    private func clampedCentre(fractionX: CGFloat, fractionY: CGFloat,
                               halfWidth: CGFloat,
                               halfHeightTop: CGFloat, halfHeightBottom: CGFloat,
                               into area: CGRect, wholePlayArea: CGRect) -> CGPoint {
        let need = halfHeightTop + halfHeightBottom
        let target = area.height >= need ? area : wholePlayArea
        let x = min(max(wholePlayArea.minX + wholePlayArea.width * fractionX,
                        target.minX + halfWidth),
                    max(target.minX + halfWidth, target.maxX - halfWidth))
        let y = min(max(wholePlayArea.minY + wholePlayArea.height * fractionY,
                        target.minY + halfHeightTop),
                    max(target.minY + halfHeightTop, target.maxY - halfHeightBottom))
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
        setOverlapReport(line)
    }

    /// Records the overlap verdict and tells whoever is listening, once per distinct outcome.
    ///
    /// Repeating it on every layout pass would bury the rest of the diagnostics, which is why the
    /// comparison is here rather than at the call sites. The two listeners get different halves on
    /// purpose: `onOverlapState` hears the all-clear as well, and `report` only ever hears a real
    /// collision, because the player's always-visible status line must not be overwritten with good
    /// news on every layout pass.
    private func setOverlapReport(_ line: String?) {
        guard line != lastOverlapReport else { return }
        lastOverlapReport = line
        onOverlapState?(line)
        if let line {
            report(line)
        }
    }

    private func report(_ line: String) {
        // Once per line per view size. These lines are written from inside `layoutSubviews`, the
        // host publishes them, the player screen re-renders, `updateUIView` marks this view dirty
        // again, and the same pass reports the same line again: an endless layout loop on the
        // main thread. That was the landscape freeze with no skin imported, where the built-in pad
        // reports "no clear column" on every pass. A new size (a rotation) allows them again.
        if bounds.size != reportedForSize {
            reportedForSize = bounds.size
            reportedLines.removeAll()
        }
        guard reportedLines.insert(line).inserted else { return }
        NSLog("[continuum] %@", line)
        onDiagnostic?(line)
    }

    private var reportedLines: Set<String> = []
    private var reportedForSize: CGSize = .zero

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

    /// Whether an editing touch belongs to this pad.
    ///
    /// `onOutline` is the standing rule: only a drawn control outline is tappable, so Done, the
    /// sliders and the system menu (all of which sit above this view, off the outlines) keep
    /// their taps.
    ///
    /// `menuListOpen` is the other half of the build-98 failure, where tapping another system
    /// did nothing. The system list opens on top of the pad. UIKit still asks this view
    /// `point(inside:)` for that tap. If the row's point is inside an outline and this returns
    /// true, this view becomes the hit target, the row's action is never delivered, and
    /// `selectPreviewSystem` does not run. Returning false while the list is open lets the row
    /// receive the tap. The list is also no longer a child of the panel's scroller; a row
    /// inside that scroller was not delivering its action even when the point missed every
    /// outline. Both have to be true for a switch to happen. The switch function itself is not
    /// involved in this decision.
    static func claimsEditingHit(onOutline: Bool, menuListOpen: Bool) -> Bool {
        onOutline && !menuListOpen
    }

    /// Only the controls are touchable.
    ///
    /// Without this the whole view would swallow every touch over the picture, including the back
    /// button in the chrome above it, and leaving a running game would be impossible.
    override func point(inside point: CGPoint, with event: UIEvent?) -> Bool {
        if isEditing {
            // Only the outlines, and not even those while the system list is open. See
            // `claimsEditingHit`. A pad that swallowed the whole screen in edit mode would
            // leave the user unable to reach Done; a pad that swallowed a menu row drawn on
            // top of an outline would leave the preview console stuck.
            return Self.claimsEditingHit(
                onOutline: dragTarget(at: point) != nil || floating.index(at: point, slop: 8) != nil
                    || floating.claimsPinch(at: point),
                menuListOpen: editingHitsSuspended
            )
        }
        if floating.index(at: point) != nil {
            return true
        }
        if stickHit(at: point) != nil {
            return true
        }
        if specialHit(at: point) != nil {
            return true
        }
        if dpadRect.insetBy(dx: -Self.hitSlop, dy: -Self.hitSlop).contains(point) {
            return true
        }
        if chipIndex(at: point) != nil {
            return true
        }
        if extraHit(at: point) != nil {
            return true
        }
        // The emulated touch screen, on the systems that have one. No hit slop: this is a region
        // rather than a control, its edge is the edge of the digitiser, and growing it would put
        // the stylus outside the screen it belongs to.
        //
        // Claiming this area is safe for the chrome because the DS touch screen is the LOWER half
        // of the picture and the player's buttons are all in a bar along the top. It is the reason
        // `touchScreen` describes half a framebuffer instead of all of one: claiming the whole
        // picture would reach the back button, and a game you cannot leave is worse than a game
        // whose top screen ignores taps it should ignore anyway.
        if let touchScreenRect, touchScreenRect.contains(point) {
            return true
        }
        // Trackpad mode claims the whole picture. The player's chrome sits above this view in
        // the z-order, so the back button and the session buttons still get their taps first.
        if let trackpadArea, trackpadArea.contains(point) {
            return true
        }
        return false
    }

    /// Which control a point would drag, if any.
    ///
    /// The responsive area is exactly the outline that is drawn, so there is no invisible margin.
    /// A tie (overlapping outlines) resolves to the nearer centre: it has to resolve the SAME way
    /// every time, because a grab that picked a different control on each attempt would read as
    /// the editor ignoring the finger.
    private func dragTarget(at point: CGPoint) -> DragTarget? {
        var hits: [(DragTarget, CGPoint)] = []
        if !dpadGroupRect.isEmpty, Self.grown(dpadGroupRect).contains(point) {
            hits.append((.dpad, dpadCentreNow))
        }
        for index in chipRects.indices where index < chips.count {
            let rect = chipRects[index]
            guard !rect.isEmpty, Self.grown(rect).contains(point) else { continue }
            let centre = index < chipCentreNow.count ? chipCentreNow[index]
                : CGPoint(x: rect.midX, y: rect.midY)
            hits.append((.chip(index), centre))
        }
        guard let first = hits.first else { return nil }
        return hits.dropFirst().reduce(first) { best, next in
            Self.distanceSquared(from: point, to: next.1)
                < Self.distanceSquared(from: point, to: best.1) ? next : best
        }.0
    }

    /// Squared, because only the comparison is needed and a square root would add nothing but a
    /// chance of a rounding difference between two distances that are meant to tie.
    private static func distanceSquared(from point: CGPoint, to other: CGPoint) -> CGFloat {
        let dx = point.x - other.x
        let dy = point.y - other.y
        return dx * dx + dy * dy
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
        if isEditing {
            // An extra button is on top of everything, so it is offered the touch first. Only
            // when no extra button took it does it become a cluster drag.
            if clusterDrag == nil, !editingHitsSuspended,
               touches.contains(where: { floating.beginEdit($0, in: self) }) {
                return
            }
            // A second finger during an extra button's drag is the other half of a pinch, not
            // the start of a cluster drag underneath it.
            if floating.isDragging { return }
            beginDrag(touches)
            return
        }
        for touch in touches {
            let point = touch.location(in: self)
            if let index = floating.index(at: point), let button = floating.button(at: index) {
                // Tested first because it is drawn on top: an extra button placed over the D-pad
                // is the thing the finger can see.
                grabs[ObjectIdentifier(touch)] = .floating(button.id)
            } else if let hit = stickHit(at: point) {
                let local = CGPoint(x: point.x - hit.rect.minX, y: point.y - hit.rect.minY)
                grabs[ObjectIdentifier(touch)] = .stick(side: hit.side, local: local)
            } else if let index = specialHit(at: point) {
                // Before the D-pad and the chips: a function or switch the skin drew is the
                // thing under the finger, and the procedural controls behind it are hidden.
                grabs[ObjectIdentifier(touch)] = .special(index)
            } else if dpadRect.insetBy(dx: -Self.hitSlop, dy: -Self.hitSlop).contains(point) {
                grabs[ObjectIdentifier(touch)] = .dpad(touch.location(in: dpad))
            } else if let index = chipIndex(at: point) {
                grabs[ObjectIdentifier(touch)] = .chip(index)
            } else if let slot = extraHit(at: point) {
                grabs[ObjectIdentifier(touch)] = .skinSlot(slot)
            } else if let trackpadArea, trackpadArea.contains(point) {
                // Before the stylus: a user who switched the picture to a trackpad asked for a
                // mouse, even on a system that has a touch screen.
                trackpadBegan(touch, at: point)
            } else if let touchScreenRect, touchScreenRect.contains(point) {
                // Tested last, so a control that happens to sit over the picture still wins. The
                // layout keeps them apart, but the order costs nothing and means a future layout
                // that does overlap degrades into a working button rather than a dead one.
                grabs[ObjectIdentifier(touch)] = .pointer(point)
            }
        }
        recompute()
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        if isEditing {
            if floating.continueEdit(touches, in: self) { return }
            continueDrag(touches)
            return
        }
        var changed = false
        for touch in touches {
            let key = ObjectIdentifier(touch)
            if trackpadTouches[key] != nil {
                trackpadMoved(touch)
                continue
            }
            guard let grab = grabs[key] else { continue }
            // A D-pad and a stylus grab track movement. A chip grab is sticky by design: see `Grab`.
            switch grab {
            case .dpad:
                grabs[key] = .dpad(touch.location(in: dpad))
                changed = true
            case .pointer:
                let point = touch.location(in: self)
                if let touchScreenRect, touchScreenRect.contains(point) {
                    grabs[key] = .pointer(point)
                } else {
                    // Slid off the digitiser, which is a release. The grab is dropped rather than
                    // clamped to the edge: clamping would hold the stylus against the border for as
                    // long as the finger stayed down, and a game reads that as a deliberate press.
                    grabs.removeValue(forKey: key)
                }
                changed = true
            case .chip, .skinSlot, .floating, .special:
                break
            case .stick(let side, _):
                guard let hit = stickHits.first(where: { $0.side == side }) else {
                    grabs.removeValue(forKey: key)
                    changed = true
                    break
                }
                let point = touch.location(in: self)
                let local = CGPoint(x: point.x - hit.rect.minX, y: point.y - hit.rect.minY)
                grabs[key] = .stick(side: side, local: local)
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
        if isEditing {
            if floating.endEdit(touches) { return }
            endDrag(touches)
            return
        }
        for touch in touches {
            grabs.removeValue(forKey: ObjectIdentifier(touch))
            trackpadEnded(touch)
        }
        recompute()
    }

    // ------------------------------------------------------------------ dragging a cluster

    private func beginDrag(_ touches: Set<UITouch>) {
        // `point(inside:)` already refuses these. This is the same rule again so a touch that
        // was claimed before the list opened cannot start a drag underneath a row.
        guard Self.claimsEditingHit(onOutline: true, menuListOpen: editingHitsSuspended) else { return }
        // A second finger while one is already dragging is ignored rather than queued. See
        // `clusterDrag`.
        guard clusterDrag == nil else { return }
        for touch in touches {
            let point = touch.location(in: self)
            guard let target = dragTarget(at: point) else { continue }
            let centre: CGPoint
            switch target {
            case .dpad:
                centre = dpadCentreNow
            case .chip(let index):
                if index >= 0, index < chipCentreNow.count {
                    centre = chipCentreNow[index]
                } else if index >= 0, index < chipRects.count {
                    let rect = chipRects[index]
                    centre = CGPoint(x: rect.midX, y: rect.midY)
                } else {
                    continue
                }
            }
            clusterDrag = ClusterDrag(
                touch: ObjectIdentifier(touch),
                target: target,
                offset: CGSize(width: point.x - centre.x, height: point.y - centre.y),
                layout: layout.sanitised
            )
            setActiveHandle(target)
            return
        }
    }

    private func continueDrag(_ touches: Set<UITouch>) {
        guard var drag = clusterDrag else { return }
        // Matched on identity rather than taken from `touches.first`, because a second finger
        // anywhere on the pad also delivers moves and would otherwise steer the drag.
        guard let touch = touches.first(where: { ObjectIdentifier($0) == drag.touch }) else {
            return
        }
        let point = touch.location(in: self)
        let wanted = CGPoint(x: point.x - drag.offset.width, y: point.y - drag.offset.height)
        let next = layoutMovingCentre(of: drag.target, to: wanted)
        guard next != drag.layout else { return }
        drag.layout = next
        clusterDrag = drag
        if placingLandscapeSkin {
            landscapeLayout = next
            onLandscapeLayoutEdited?(next, false)
        } else {
            onLayoutEdited?(next, false)
        }
    }

    private func endDrag(_ touches: Set<UITouch>) {
        guard let drag = clusterDrag,
              touches.contains(where: { ObjectIdentifier($0) == drag.touch }) else { return }
        clusterDrag = nil
        setActiveHandle(nil)
        // Settled even when the finger never moved. A tap that produced no change still publishes
        // the value the pad is holding, which a caller can compare against its own and ignore; the
        // alternative, staying silent, would lose the ONE case that matters: a drag whose last move
        // was clamped, where the value the caller has is the one to keep and it needs to be told
        // that it is final.
        if placingLandscapeSkin {
            landscapeLayout = drag.layout
            onLandscapeLayoutEdited?(drag.layout, true)
        } else {
            onLayoutEdited?(drag.layout, true)
        }
    }

    private func setActiveHandle(_ target: DragTarget?) {
        dpadHandle.setActive(target == .dpad)
        let activeIndex: Int?
        if case .chip(let index) = target {
            activeIndex = index
        } else {
            activeIndex = nil
        }
        for (index, handle) in chipHandles.enumerated() {
            handle.setActive(index == activeIndex)
        }
    }

    /// The layout that would put one control's centre at this point.
    ///
    /// Inverts the layout pass exactly: a fraction is measured against `editPlayArea`, which is the
    /// same rectangle `clampedCentre` multiplies its fractions by. The result is sanitised, so a
    /// drag cannot express an arrangement the pad would then refuse.
    ///
    /// LANDSCAPE WRITES ONLY X for the D-pad cluster, and that is not an oversight. The layout
    /// pass overrides the D-pad's y with `landscapeClusterY` when the screen is wider than it is
    /// tall. Writing y there would store a number with no visible effect. Face buttons, shoulders,
    /// SELECT and START are free in both orientations, so both axes are written for them. The first
    /// drag on a still-clustered face/shoulder button stores a `buttonFrees` entry and leaves its
    /// siblings on the cluster.
    private func layoutMovingCentre(of target: DragTarget, to point: CGPoint) -> TouchLayout {
        var next = (placingLandscapeSkin ? (landscapeLayout ?? layout) : layout).sanitised
        let area = editPlayArea
        guard area.width > 0, area.height > 0 else { return next }

        let fractionX = Double((point.x - area.minX) / area.width)
        let fractionY = Double((point.y - area.minY) / area.height)
        let landscape = bounds.width > bounds.height

        switch target {
        case .dpad:
            next.dpadX = fractionX
            // A skin stores the D-pad's y. The procedural override only applies when there
            // is no skin canvas, which is the only time `placingLandscapeSkin` is false
            // and landscape still ignores y.
            if placingLandscapeSkin || !landscape { next.dpadY = fractionY }
        case .chip(let index):
            guard index >= 0, index < chips.count else { return next }
            let control = chips[index].control
            switch control.cluster {
            case .system:
                if control.slot == .select {
                    next.selectX = fractionX
                    next.selectY = fractionY
                } else {
                    next.startX = fractionX
                    next.startY = fractionY
                }
            case .face, .shoulderLeft, .shoulderRight:
                next.setFreeCentre(for: control.slot, x: fractionX, y: fractionY)
            case .dpad:
                break
            }
        }
        return next.sanitised
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

        var stylus: CGPoint?
        var stick = CGPoint.zero
        var rightStick = CGPoint.zero
        var stickLeftDown = false
        var stickRightDown = false
        // A skin circle pad is the analog stick. The D-pad stays digital when that pad exists,
        // so the two are not the same control.
        let skinLeftStick = stickHits.contains { $0.side != "right" }
        var turbo = Set<PadSlot>()
        var floatingHeld = Set<UUID>()
        var actionsHeld: [UUID: PadAppAction] = [:]
        var specialHeld = Set<Int>()

        for grab in grabs.values {
            switch grab {
            case .floating(let id):
                // Looked up by id each time, so a button deleted or retyped under a finger is
                // answered from what it is NOW.
                guard let button = floating.buttons.first(where: { $0.id == id }) else { break }
                floatingHeld.insert(id)
                switch button.kind {
                case .press:
                    pressed.formUnion(button.padSlots)
                case .turbo:
                    turbo.formUnion(button.padSlots)
                case .action:
                    if let action = button.action { actionsHeld[id] = action }
                }
            case .chip(let index):
                // The control set can change while a finger is down, so the index is checked
                // rather than trusted.
                if index >= 0 && index < chips.count {
                    pressed.insert(chips[index].control.slot)
                }
            case .skinSlot(let slot):
                pressed.insert(slot)
            case .special(let index):
                guard let button = specialButtons[index] else { break }
                specialHeld.insert(index)
                if button.skinFunction == nil, button.toggle == nil {
                    // A combo: every slot the item named, held together.
                    pressed.formUnion(Self.slots(of: button))
                }
            case .stick(let side, let local):
                guard let hit = stickHits.first(where: { $0.side == side }) else { break }
                let vector = Self.stickVector(at: local, in: CGRect(origin: .zero, size: hit.rect.size))
                if side == "right" {
                    rightStick = vector
                    stickRightDown = true
                } else {
                    stick = vector
                    stickLeftDown = true
                }
            case .dpad(let point):
                let d = Self.directions(at: point, in: dpad.bounds)
                up = up || d.up
                down = down || d.down
                left = left || d.left
                right = right || d.right
                // Only asked for when the running system has a stick AND the skin did not
                // declare its own. A declared circle pad is the stick; the D-pad stays digital.
                if system.dpadDrivesAnalogStick && !skinLeftStick {
                    stick = Self.stickVector(at: point, in: dpad.bounds)
                }
            case .pointer(let point):
                // Last finger down wins if two are somehow on the screen at once. The DS digitiser
                // was resistive and reported ONE position, so there is no honest way to represent
                // two and averaging them would invent a touch where neither finger is.
                stylus = point
            }
        }

        if up { pressed.insert(.up) }
        if down { pressed.insert(.down) }
        if left { pressed.insert(.left) }
        if right { pressed.insert(.right) }

        // Skin functions and switches: a release is always acted on (so entering the editor with
        // a held fast forward still ends it), a press only while playing.
        let specialsDown = isEditing ? Set<Int>() : specialHeld.subtracting(lastSpecialHeld)
        for index in lastSpecialHeld.subtracting(specialHeld).sorted() {
            specialReleased(index)
        }
        for index in specialsDown.sorted() {
            specialPressed(index)
        }
        lastSpecialHeld = isEditing ? [] : specialHeld
        // Switches that send game buttons: a latching one holds them while on, a momentary one
        // while the finger is down.
        if !isEditing {
            for (index, button) in specialButtons where button.skinFunction == nil {
                guard let toggle = button.toggle else { continue }
                let on = toggle.selfRetracting
                    ? specialHeld.contains(index)
                    : (switchOn[switchKey(index)] ?? false)
                if on { pressed.formUnion(Self.slots(of: button)) }
            }
        }

        // Remembered so a release reports the point the finger lifted from rather than the origin.
        // The engine keeps the last position too, for the same reason, but the frame it publishes
        // has to be consistent on its own: a released frame carrying (0, 0) would be a lie about
        // where the stylus is, even though nothing currently acts on it.
        if let stylus {
            lastPointerFraction = pointerFraction(of: stylus)
        }
        // A slot both held and turbo is held: holding wins, because that is what the player is
        // doing with the other finger.
        turbo.subtract(pressed)
        padState = PadFrame(pressed: pressed,
                            stick: stick,
                            rightStick: rightStick,
                            pointer: lastPointerFraction,
                            pointerPressed: stylus != nil,
                            turbo: turbo)

        // Extra buttons that run an app action: one call on the press, one on the release.
        // Released ones first, so a held fast forward ends before anything new begins.
        if !isEditing {
            for (id, action) in heldActionButtons where actionsHeld[id] == nil {
                onAppAction?(action, false)
            }
            for (id, action) in actionsHeld where heldActionButtons[id] == nil {
                onAppAction?(action, true)
            }
        }
        heldActionButtons = isEditing ? [:] : actionsHeld

        // The tap. Only for something NEWLY down, so a held button does not buzz on every move
        // of another finger, and never in the editor, where a touch is a drag.
        let downNow = pressed.union(turbo)
        let newGameDown = !downNow.subtracting(lastHapticPressed).isEmpty
        if !isEditing,
           newGameDown
            || !floatingHeld.subtracting(lastHapticFloating).isEmpty
            || !specialsDown.isEmpty {
            ButtonHaptics.shared.tap()
        }
        // The skin's own click (`sound.caf`), on a new press of a skin control.
        if !isEditing, skinSoundURL != nil, newGameDown || !specialsDown.isEmpty {
            SkinButtonSound.shared.play(skinSoundURL)
        }
        lastHapticPressed = isEditing ? [] : downNow
        lastHapticFloating = isEditing ? [] : floatingHeld
        floating.setPressed(Set(floating.buttons.indices.filter {
            floatingHeld.contains(floating.buttons[$0].id)
        }))

        dpad.setDirections(up: up, down: down, left: left, right: right)
        for chip in chips {
            chip.setPressed(pressed.contains(chip.control.slot))
        }
        refreshPressedArt(pressed: pressed, stickLeft: stickLeftDown, stickRight: stickRightDown,
                          specialHeld: specialHeld)
    }

    // MARK: Skin functions and switches

    /// The slots a skin button holds: its own and any combo partners.
    private static func slots(of button: DeltaSkinButton) -> [PadSlot] {
        let keys = [button.slot] + (button.comboSlots ?? [])
        return keys.compactMap { key in PadSlot.allCases.first { $0.layoutKey == key } }
    }

    private func switchKey(_ index: Int) -> String {
        switchScope + "#\(index)"
    }

    private func specialPressed(_ index: Int) {
        guard let button = specialButtons[index] else { return }
        let function = button.skinFunction
        guard let toggle = button.toggle else {
            if let function { onSkinFunction?(function, true) }
            return
        }
        let key = switchKey(index)
        let now = toggle.selfRetracting ? true : !(switchOn[key] ?? false)
        switchOn[key] = now
        if let function {
            // A held function on a latching switch is held for as long as the switch is on.
            if function.isHold && !toggle.selfRetracting {
                onSkinFunction?(function, now)
            } else {
                onSkinFunction?(function, true)
            }
            rereadBoundSwitch(index)
        }
        placeSwitchArt(index, animated: true)
    }

    private func specialReleased(_ index: Int) {
        guard let button = specialButtons[index] else { return }
        let function = button.skinFunction
        guard let toggle = button.toggle else {
            if let function { onSkinFunction?(function, false) }
            return
        }
        guard toggle.selfRetracting else { return }
        switchOn[switchKey(index)] = false
        if let function { onSkinFunction?(function, false) }
        placeSwitchArt(index, animated: true)
    }

    /// A bound switch shows the real state, read right after its function ran and again a moment
    /// later, because some functions (a sheet, the engine) settle on the next turn.
    private func rereadBoundSwitch(_ index: Int) {
        guard let function = specialButtons[index]?.skinFunction,
              function.boundState != nil else { return }
        let key = switchKey(index)
        if let real = skinFunctionState?(function) { switchOn[key] = real }
        Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: 150_000_000)
            guard let self, self.switchKey(index) == key,
                  let real = self.skinFunctionState?(function),
                  real != self.switchOn[key] else { return }
            self.switchOn[key] = real
            self.placeSwitchArt(index, animated: true)
        }
    }

    /// Moves a switch's knob to its begin (off) or end (on) frame, spring-animated when the skin
    /// asks for a spring, and swaps to the "on" picture when the skin has one.
    private func placeSwitchArt(_ index: Int, animated: Bool) {
        let key = "btn-\(index)"
        guard let toggle = specialButtons[index]?.toggle,
              let rect = specialRects[index],
              let view = artViews[key] else { return }
        let on = switchOn[switchKey(index)] ?? false
        let full = DeltaSkinNormalizedRect(x: 0, y: 0, width: 1, height: 1)
        let knob = (on ? toggle.end : toggle.begin) ?? full
        let target = CGRect(x: rect.minX + CGFloat(knob.x) * rect.width,
                            y: rect.minY + CGFloat(knob.y) * rect.height,
                            width: CGFloat(knob.width) * rect.width,
                            height: CGFloat(knob.height) * rect.height)
        view.image = on ? (artSelected[key] ?? artNormal[key]) : (artNormal[key] ?? artSelected[key])
        view.isHidden = view.image == nil
        view.transform = .identity
        guard animated else {
            view.frame = target
            return
        }
        if toggle.spring {
            UIView.animate(withDuration: 0.4, delay: 0, usingSpringWithDamping: 0.55,
                           initialSpringVelocity: 0.8,
                           options: [.beginFromCurrentState, .allowUserInteraction]) {
                view.frame = target
            }
        } else {
            UIView.animate(withDuration: 0.18, delay: 0,
                           options: [.beginFromCurrentState, .allowUserInteraction]) {
                view.frame = target
            }
        }
    }

    private func specialHit(at point: CGPoint) -> Int? {
        // Last drawn wins, the way the art stacks.
        for hit in specialHits.reversed()
        where hit.rect.insetBy(dx: -Self.hitSlop, dy: -Self.hitSlop).contains(point) {
            return hit.index
        }
        return nil
    }

    /// A point on the glass as a fraction of the WHOLE framebuffer, origin top left.
    ///
    /// The inverse of `updateTouchScreenRect`, and it has to stay the inverse: that method placed
    /// the system's framebuffer fraction onto the picture, so this one maps back through the same
    /// fraction. For the DS that means a finger at the very top of the touch screen comes out at
    /// y = 0.5 rather than y = 0, because the top half of that framebuffer is the OTHER screen.
    /// Reporting 0 there would put every touch on the wrong screen, and the core would discard it.
    ///
    /// Clamped, because a finger can sit a fraction of a point outside the rect it was grabbed in
    /// and a fraction outside 0...1 is not a place on the framebuffer.
    private func pointerFraction(of point: CGPoint) -> CGPoint {
        if let touchMapper, let picture = touchPictureRect, picture.width >= 1, picture.height >= 1 {
            let inView = CGPoint(x: (point.x - picture.minX) / picture.width,
                                 y: (point.y - picture.minY) / picture.height)
            return touchMapper.map(picture.size, inView) ?? lastPointerFraction
        }
        guard let rect = touchScreenRect,
              let fraction = system.touchScreen,
              rect.width >= 1, rect.height >= 1 else { return lastPointerFraction }

        let withinX = Self.unitClamped((point.x - rect.minX) / rect.width)
        let withinY = Self.unitClamped((point.y - rect.minY) / rect.height)
        return CGPoint(x: fraction.minX + withinX * fraction.width,
                       y: fraction.minY + withinY * fraction.height)
    }

    private static func unitClamped(_ value: CGFloat) -> CGFloat {
        min(max(value, 0), 1)
    }

    /// The analog deflection a touch at this point means, each component `-1...1`.
    ///
    /// Shares `directions`' normalisation and deadzone deliberately, so the stick and the digital
    /// bits agree about where the centre is and about when a touch counts at all. What it does NOT
    /// share is quantisation: `directions` collapses to eight compass points, and this keeps the
    /// real vector, which is the whole reason an N64 game can be steered gently rather than only
    /// slammed to full deflection.
    ///
    /// Rescaled so that the edge of the DEADZONE is zero and the edge of the surface is full
    /// deflection. Without that, the smallest movement that registers at all would jump straight to
    /// 22 percent, and a character would start walking rather than creeping.
    ///
    /// Magnitude is clamped rather than the components individually, because clamping x and y
    /// separately would let a diagonal reach 1.41 and read as harder than any straight push.
    static func stickVector(at point: CGPoint, in bounds: CGRect) -> CGPoint {
        let halfWidth = bounds.width / 2
        let halfHeight = bounds.height / 2
        guard halfWidth > 0, halfHeight > 0 else { return .zero }

        let x = (point.x - bounds.midX) / halfWidth
        let y = (point.y - bounds.midY) / halfHeight
        let magnitude = (x * x + y * y).squareRoot()
        guard magnitude > dpadDeadzone else { return .zero }

        // Beyond the deadzone, remap [deadzone, 1] onto [0, 1] and cap at full.
        let scaled = min((magnitude - dpadDeadzone) / (1 - dpadDeadzone), 1)
        let unitX = x / magnitude
        let unitY = y / magnitude
        return CGPoint(x: unitX * scaled, y: unitY * scaled)
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

// MARK: - Skin holes, analog sticks, pressed art

    /// Puts every declared control on the rectangle the skin measured.
    ///
    /// Procedural chips stay as the hit target when the file has no image for that button.
    /// When it does, the chip is hidden and the image is shown, and a press swaps to the
    /// pressed image only if the file named one.
    private func applyDeclaredSkinControls(skin: ResolvedSkin, canvas: CGRect) {
        var usedArt = Set<String>()
        stickHits = []
        extraHits = []
        specialHits = []
        specialButtons = [:]
        specialRects = [:]
        artSlots = [:]
        artSpecial = [:]
        artSelected = [:]
        var chipHidden = Set<Int>()
        let wide = bounds.width > bounds.height && landscapeMapping.width > 0
            && landscapeMapping.height > 0
        switchScope = skin.face.skinID + (wide ? "#landscape" : "#portrait")
        if skinSoundURL != skin.face.soundURL {
            skinSoundURL = skin.face.soundURL
            SkinButtonSound.shared.prepare(skinSoundURL)
        }

        for (index, button) in skin.face.buttons.enumerated() where button.slot != "dpad" {
            let rect = button.frame.cgRect(in: canvas)
            guard rect.width >= 1, rect.height >= 1 else { continue }
            let images = skin.face.imagesByIndex[index] ?? skin.face.buttonImages[button.slot]
            let key = "btn-\(index)"
            if button.isSpecial {
                specialHits.append((index, rect))
                specialButtons[index] = button
                specialRects[index] = rect
                artSpecial[key] = index
                if let selected = images?.selected { artSelected[key] = selected }
                if let function = button.skinFunction, button.toggle != nil,
                   function.boundState != nil, let real = skinFunctionState?(function) {
                    // The real state, every time the skin is laid out, so a switch never shows
                    // a position the game is not in.
                    switchOn[switchKey(index)] = real
                }
                if let art = images?.normal ?? images?.selected {
                    showArt(key: key, image: art, pressed: images?.pressed, frame: rect)
                    if images?.normal == nil { artNormal.removeValue(forKey: key) }
                    usedArt.insert(key)
                    if button.toggle != nil { placeSwitchArt(index, animated: false) }
                }
                continue
            }
            if let chipIndex = chips.firstIndex(where: { $0.control.slot.layoutKey == button.slot }) {
                chips[chipIndex].frame = rect
                if chipIndex < chipRects.count { chipRects[chipIndex] = rect }
                if chipIndex < chipCentreNow.count {
                    chipCentreNow[chipIndex] = CGPoint(x: rect.midX, y: rect.midY)
                }
                if images?.normal != nil {
                    chipHidden.insert(chipIndex)
                    showArt(key: key, image: images?.normal, pressed: images?.pressed, frame: rect)
                    artSlots[key] = Self.slots(of: button)
                    usedArt.insert(key)
                }
            } else if let slot = PadSlot.allCases.first(where: { $0.layoutKey == button.slot }) {
                extraHits.append((slot, rect))
                if images?.normal != nil {
                    showArt(key: key, image: images?.normal, pressed: images?.pressed, frame: rect)
                    artSlots[key] = [slot]
                    usedArt.insert(key)
                }
            }
        }
        // A special button whose index left the face (a different skin) is not held any more.
        lastSpecialHeld = lastSpecialHeld.filter { specialButtons[$0] != nil }

        for index in chips.indices {
            chips[index].isHidden = chipHidden.contains(index)
        }

        if let frame = skin.face.dpadFrame {
            let rect = frame.cgRect(in: canvas)
            if rect.width >= 1, rect.height >= 1 {
                dpad.frame = rect
                dpadRect = rect
                dpadCentreNow = CGPoint(x: rect.midX, y: rect.midY)
            }
        }
        if let normal = skin.face.dpadImage {
            showArt(key: "dpad", image: normal, pressed: skin.face.dpadPressedImage, frame: dpadRect)
            usedArt.insert("dpad")
            dpad.isHidden = true
        } else {
            dpad.isHidden = false
        }

        for stick in skin.face.sticks {
            let rect = stick.frame.cgRect(in: canvas)
            guard rect.width >= 1, rect.height >= 1 else { continue }
            stickHits.append((stick.side, rect))
            if let image = skin.face.stickImages[stick.side] {
                showArt(key: "stick-" + stick.side, image: image, pressed: nil, frame: rect)
                usedArt.insert("stick-" + stick.side)
            }
        }

        for (key, view) in artViews where !usedArt.contains(key) {
            view.isHidden = true
        }

        // The skin editor's overall opacity. Applied to the background art and every piece, so
        // the whole skin fades together; hit areas are unaffected.
        let skinAlpha = CGFloat(min(1, max(0.05, skin.face.opacity)))
        currentSkinAlpha = skinAlpha
        skinImageView.alpha = skinAlpha
        for (key, view) in artViews {
            view.alpha = artDown.contains(key) ? skinAlpha * 0.7 : skinAlpha
        }
        applyHiddenControls()
    }

    /// Editor drags and a pad with no skin use the procedural controls, not skin images.
    private func clearDeclaredSkinArt() {
        stickHits = []
        extraHits = []
        specialHits = []
        specialButtons = [:]
        specialRects = [:]
        artSlots = [:]
        artSpecial = [:]
        artSelected = [:]
        artDown = []
        skinImageView.alpha = 1
        dpad.isHidden = false
        for chip in chips { chip.isHidden = false }
        for view in artViews.values { view.isHidden = true }
        artNormal.removeAll()
        artPressed.removeAll()
    }

    private func showArt(key: String, image: UIImage?, pressed: UIImage?, frame: CGRect) {
        guard let image else { return }
        let view = artViews[key] ?? {
            let created = UIImageView()
            created.contentMode = .scaleToFill
            created.isUserInteractionEnabled = false
            addSubview(created)
            artViews[key] = created
            return created
        }()
        view.image = image
        // Identity first: a frame set under a press animation's scale would be the wrong size.
        view.transform = .identity
        artDown.remove(key)
        view.frame = frame
        view.isHidden = false
        artNormal[key] = image
        if let pressed {
            artPressed[key] = pressed
        } else {
            artPressed.removeValue(forKey: key)
        }
    }

    /// Cuts the skin image at every hole so the canvas behind it shows through.
    private func maskSkinArtwork(canvas: CGRect, screens: [DeltaSkinScreen]) {
        guard skinImageView.image != nil, canvas.width > 1, canvas.height > 1 else {
            skinImageView.layer.mask = nil
            return
        }
        let path = UIBezierPath(rect: skinImageView.bounds)
        for screen in screens {
            let hole = screen.output.cgRect(in: canvas)
            let local = convert(hole, to: skinImageView)
            path.append(UIBezierPath(rect: local))
        }
        path.usesEvenOddFillRule = true
        let mask = CAShapeLayer()
        mask.frame = skinImageView.bounds
        mask.path = path.cgPath
        mask.fillRule = .evenOdd
        skinImageView.layer.mask = mask
    }

    /// The bottom screen is the digitiser. Its hole maps straight onto `touchScreen`,
    /// not through a letterbox of the stacked picture.
    private func placeDigitiser(on screens: [DeltaSkinScreen], canvas: CGRect) {
        if let touchMapper, system.touchScreen != nil {
            // The holes have just been handed to the engine (see `publishPictureArea`), and with
            // the swap a hole's picture may be the other screen, so the engine says which hole
            // holds the touch screen and which part of that hole it is.
            placeMappedTouchScreen(touchMapper, in: canvas)
            return
        }
        guard system.touchScreen != nil, screens.count >= 2 else {
            // One screen, or a system with no digitiser: nothing to point at inside a hole.
            if system.touchScreen == nil || screens.count < 2 {
                clearTouchScreenRect()
                return
            }
            return
        }
        let bottom = screens.max { lhs, rhs in
            if lhs.cropsFramebuffer && rhs.cropsFramebuffer {
                if lhs.inputY != rhs.inputY { return lhs.inputY < rhs.inputY }
            }
            return lhs.output.y < rhs.output.y
        }
        guard let bottom else {
            clearTouchScreenRect()
            return
        }
        let hole = bottom.output.cgRect(in: canvas)
        guard hole.width >= 1, hole.height >= 1 else {
            clearTouchScreenRect()
            return
        }
        touchScreenRect = hole
        revalidatePointerGrabs()
    }

    private func deliverSkinHoles(_ screens: [DeltaSkinScreen]) {
        let signature = screens.map { screen in
            "\(screen.output.x),\(screen.output.y),\(screen.output.width),\(screen.output.height),"
                + "\(screen.inputX),\(screen.inputY),\(screen.inputWidth),\(screen.inputHeight)"
        }.joined(separator: "|")
        guard signature != lastHoleSignature else { return }
        lastHoleSignature = signature
        onSkinHoles?(screens)
    }

    private func stickHit(at point: CGPoint) -> (side: String, rect: CGRect)? {
        for hit in stickHits where hit.rect.insetBy(dx: -Self.hitSlop, dy: -Self.hitSlop).contains(point) {
            return hit
        }
        return nil
    }

    private func extraHit(at point: CGPoint) -> PadSlot? {
        for hit in extraHits where hit.rect.insetBy(dx: -Self.hitSlop, dy: -Self.hitSlop).contains(point) {
            return hit.slot
        }
        return nil
    }

    /// Shows a press on the skin's own art. A pressed picture in the file is swapped in; without
    /// one, the button's own layer is pushed in (scaled down and dimmed) and springs back on
    /// release, the way Manic animates `asset.normal`. Sticks keep their picture, and a switch's
    /// feedback is its knob moving.
    private func refreshPressedArt(pressed: Set<PadSlot>, stickLeft: Bool, stickRight: Bool,
                                   specialHeld: Set<Int>) {
        for (key, view) in artViews where !view.isHidden {
            let down: Bool
            var animates = true
            if key == "dpad" {
                down = pressed.contains(.up) || pressed.contains(.down)
                    || pressed.contains(.left) || pressed.contains(.right)
            } else if let slots = artSlots[key] {
                down = slots.contains { pressed.contains($0) }
            } else if let index = artSpecial[key] {
                if specialButtons[index]?.toggle != nil { continue }
                down = specialHeld.contains(index)
            } else if key == "stick-left" {
                down = stickLeft
                animates = false
            } else if key == "stick-right" {
                down = stickRight
                animates = false
            } else {
                down = false
            }
            if let pressedImage = artPressed[key] {
                view.image = down ? pressedImage : artNormal[key]
            } else if animates {
                animatePress(key: key, view: view, down: down)
            } else {
                view.image = artNormal[key]
            }
        }
    }

    private func animatePress(key: String, view: UIImageView, down: Bool) {
        guard artDown.contains(key) != down else { return }
        if down { artDown.insert(key) } else { artDown.remove(key) }
        let hidden = controlsHiddenNow && !keepsVisibleWhenHidden(key)
        let base = hidden ? 0 : currentSkinAlpha
        UIView.animate(withDuration: down ? 0.05 : 0.14, delay: 0,
                       options: [.beginFromCurrentState, .allowUserInteraction]) {
            view.transform = down ? CGAffineTransform(scaleX: 0.9, y: 0.9) : .identity
            view.alpha = down ? base * 0.7 : base
        }
    }

}

// MARK: - SwiftUI bridge

/// Puts `TouchControlsView` in a SwiftUI tree and links it to the render loop's `PadInputSource`.
struct TouchControlsHost: UIViewRepresentable {
    let system: GameSystem
    let layout: TouchLayout
    /// The running game's display aspect, so the pad can find the picture inside the area it
    /// reserved and put the emulated touch screen on it. See `TouchControlsView.pictureAspect`.
    let pictureAspect: CGFloat?
    /// The box `MetalCanvas` reads through. Held by the caller, so it survives this view being
    /// rebuilt, and pointed at the live view here.
    let input: PadInputSource
    let onDiagnostic: (String) -> Void
    /// Where the picture may be drawn, reported after every layout. See
    /// `TouchControlsView.onPictureArea`.
    let onPictureArea: (CGRect) -> Void

    /// True when this pad is the one inside `TouchLayoutEditor`. See `TouchControlsView.isEditing`
    /// for what changes, which is everything about what a touch means.
    let isEditing: Bool

    /// Where a drag's result goes. See `TouchControlsView.onLayoutEdited` for what `settled` means.
    let onLayoutEdited: (TouchLayout, Bool) -> Void

    /// The overlap check's verdict including the all-clear. See
    /// `TouchControlsView.onOverlapState`.
    let onOverlapState: (String?) -> Void

    /// Imported Delta skin PDF/PNG, when one is active for this console.
    let skinArtwork: UIImage?
    /// Delta `screens` outputFrame as fractions of mappingSize, when the pack named one.
    let skinScreenNormalized: DeltaSkinNormalizedRect?
    let skinMapping: CGSize
    let landscapeArtwork: UIImage?
    let landscapeScreen: DeltaSkinNormalizedRect?
    let landscapeMapping: CGSize
    let landscapeLayout: TouchLayout?
    let onLandscapeLayoutEdited: (TouchLayout, Bool) -> Void
    let portraitFace: SkinPadFace
    let landscapeFace: SkinPadFace
    let onSkinHoles: ([DeltaSkinScreen]) -> Void

    /// True while the editor's system list is open. See `TouchControlsView.editingHitsSuspended`.
    /// Default false so the player, which never opens that list, does not have to mention it.
    let editingHitsSuspended: Bool

    /// The engine's answer for where the touch screen is. See `TouchScreenMapper`.
    let touchMapper: TouchScreenMapper?
    /// Bumped when the engine's screen layout changes. See `TouchControlsView.screenLayoutVersion`.
    let screenLayoutVersion: Int
    /// The picture as a trackpad. See `TouchControlsView.trackpadEnabled`.
    let trackpadEnabled: Bool

    /// Spelled out rather than left to the synthesized memberwise initialiser.
    ///
    /// Two reasons, and the second is the load-bearing one. It lets the three editing parameters
    /// default, so the player screen's call site is untouched by a feature it does not use. And a
    /// synthesized memberwise initialiser gives default arguments only for `var` properties with
    /// initial values, so getting the same effect implicitly would mean making three `let`s into
    /// `var`s and relying on that rule holding: an explicit initialiser states the contract where
    /// a reader will look for it.
    init(system: GameSystem,
         layout: TouchLayout,
         pictureAspect: CGFloat? = nil,
         input: PadInputSource,
         onDiagnostic: @escaping (String) -> Void,
         onPictureArea: @escaping (CGRect) -> Void,
         isEditing: Bool = false,
         onLayoutEdited: @escaping (TouchLayout, Bool) -> Void = { _, _ in },
         onOverlapState: @escaping (String?) -> Void = { _ in },
         skinArtwork: UIImage? = nil,
         skinScreenNormalized: DeltaSkinNormalizedRect? = nil,
         skinMapping: CGSize = .zero,
         landscapeArtwork: UIImage? = nil,
         landscapeScreen: DeltaSkinNormalizedRect? = nil,
         landscapeMapping: CGSize = .zero,
         landscapeLayout: TouchLayout? = nil,
         onLandscapeLayoutEdited: @escaping (TouchLayout, Bool) -> Void = { _, _ in },
         portraitFace: SkinPadFace = SkinPadFace(),
         landscapeFace: SkinPadFace = SkinPadFace(),
         onSkinHoles: @escaping ([DeltaSkinScreen]) -> Void = { _ in },
         editingHitsSuspended: Bool = false,
         touchMapper: TouchScreenMapper? = nil,
         screenLayoutVersion: Int = 0,
         trackpadEnabled: Bool = false) {
        self.system = system
        self.layout = layout
        self.pictureAspect = pictureAspect
        self.input = input
        self.onDiagnostic = onDiagnostic
        self.onPictureArea = onPictureArea
        self.isEditing = isEditing
        self.onLayoutEdited = onLayoutEdited
        self.onOverlapState = onOverlapState
        self.skinArtwork = skinArtwork
        self.skinScreenNormalized = skinScreenNormalized
        self.skinMapping = skinMapping
        self.landscapeArtwork = landscapeArtwork
        self.landscapeScreen = landscapeScreen
        self.landscapeMapping = landscapeMapping
        self.landscapeLayout = landscapeLayout
        self.onLandscapeLayoutEdited = onLandscapeLayoutEdited
        self.portraitFace = portraitFace
        self.landscapeFace = landscapeFace
        self.onSkinHoles = onSkinHoles
        self.editingHitsSuspended = editingHitsSuspended
        self.touchMapper = touchMapper
        self.screenLayoutVersion = screenLayoutVersion
        self.trackpadEnabled = trackpadEnabled
    }

    func makeUIView(context: Context) -> TouchControlsView {
        let view = TouchControlsView(system: system, layout: layout)
        view.pictureAspect = pictureAspect
        view.onDiagnostic = onDiagnostic
        view.onPictureArea = onPictureArea
        view.onLayoutEdited = onLayoutEdited
        view.onOverlapState = onOverlapState
        view.skinArtwork = skinArtwork
        view.skinScreenNormalized = skinScreenNormalized
        view.skinMapping = skinMapping
        view.landscapeArtwork = landscapeArtwork
        view.landscapeScreen = landscapeScreen
        view.landscapeMapping = landscapeMapping
        if !view.isClusterDragging {
            view.landscapeLayout = landscapeLayout
        }
        view.onLandscapeLayoutEdited = onLandscapeLayoutEdited
        view.portraitFace = portraitFace
        view.landscapeFace = landscapeFace
        view.onSkinHoles = onSkinHoles
        view.isEditing = isEditing
        view.editingHitsSuspended = editingHitsSuspended
        view.touchMapper = touchMapper
        view.screenLayoutVersion = screenLayoutVersion
        view.trackpadEnabled = trackpadEnabled && !isEditing
        view.onAppAction = { [weak input] action, pressed in
            input?.onAppAction?(action, pressed)
        }
        view.onSkinFunction = { [weak input] function, pressed in
            input?.onSkinFunction?(function, pressed)
        }
        view.skinFunctionState = { [weak input] function in
            input?.skinFunctionState?(function)
        }
        input.view = view
        return view
    }

    func updateUIView(_ view: TouchControlsView, context: Context) {
        view.system = system
        // Set BEFORE the layout, because entering or leaving edit mode drops everything held and
        // re-applies the previewed opacity, and doing that after the new layout arrived would
        // re-apply the old one. Both paths end in `setNeedsLayout`, so the order only decides
        // which value the opacity is read from, not whether a pass happens.
        view.isEditing = isEditing
        // Before layout, so a touch that arrives in the same turn as the list opening already
        // sees the gate. Assigning it does not itself mark the view dirty.
        view.editingHitsSuspended = editingHitsSuspended
        view.layout = layout
        view.pictureAspect = pictureAspect
        view.onDiagnostic = onDiagnostic
        view.onPictureArea = onPictureArea
        view.onLayoutEdited = onLayoutEdited
        view.onOverlapState = onOverlapState
        view.skinArtwork = skinArtwork
        view.skinScreenNormalized = skinScreenNormalized
        view.skinMapping = skinMapping
        view.landscapeArtwork = landscapeArtwork
        view.landscapeScreen = landscapeScreen
        view.landscapeMapping = landscapeMapping
        if !view.isClusterDragging {
            view.landscapeLayout = landscapeLayout
        }
        view.onLandscapeLayoutEdited = onLandscapeLayoutEdited
        view.portraitFace = portraitFace
        view.landscapeFace = landscapeFace
        view.onSkinHoles = onSkinHoles
        view.touchMapper = touchMapper
        view.screenLayoutVersion = screenLayoutVersion
        view.trackpadEnabled = trackpadEnabled && !isEditing
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
        view.onLayoutEdited = nil
        view.onLandscapeLayoutEdited = nil
        view.onOverlapState = nil
        view.onSkinHoles?([])
        view.onSkinHoles = nil
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    final class Coordinator {}
}
