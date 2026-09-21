// Continuum, physical controllers: MFi, Xbox, DualShock and DualSense, through GameController.
//
// This was one of the last two entries in the Settings screen's NOT WIRED list, and the thing
// holding it back was never Apple's framework. The engine has kept ONE INDEPENDENT INPUT LAYER
// PER SOURCE since the browser build, precisely so that a real pad and an on-screen pad could be
// held at the same time. What was missing was an exported call that could say WHICH layer a poll
// belonged to: `applyGamepad` writes the `.gamepad` layer and no other layer was addressable from
// Swift, so a real controller and the overlay would have fought over one layer. Both halves are
// exported now, `applyGamepadFrom(port:source:buttons:axes:)` and `releaseInputSource(source:)`,
// which is what turns this from a plan into a file.
//
// THE TRAP THIS FILE IS SHAPED AROUND: A POLL REPLACES ITS OWN LAYER, IT DOES NOT ADD TO IT.
//
// `apply_standard_gamepad_from` in crates/emulator-bridge/src/input/gamepad.rs ends with
// `self.sources[source].ports[port] = state`. An assignment, not an OR. That is correct, because
// a poll is a complete statement about one device, and it is also exactly why the on-screen pad
// and this file must not share a layer. The overlay is read every frame whether or not a finger
// is on it, so an overlay reporting "nothing held" sixty times a second into `.gamepad` would
// wipe out a real controller's press before the core ever saw it. The symptom is the confusing
// kind rather than the obvious kind: a controller that works perfectly until a thumb comes near
// the glass, which reads as a broken controller rather than as a wiring mistake.
//
// So `MetalCanvas.tick` sends the overlay to `.touch` and the frames from here to `.gamepad`, and
// the merge happens in Rust, where buttons are OR-ed across layers and the larger axis magnitude
// wins. BOTH halves of that change are load-bearing: routing only this file to `.gamepad` while
// leaving the overlay where it was is the bug described above, and it would look like this file's
// fault.
//
// THE OTHER HALF OF THE TRAP: A DISCONNECT PRODUCES NO POLL.
//
// Every other correction to the gamepad layer arrives as the next frame's poll. A controller that
// is unplugged, or that walks out of Bluetooth range, sends nothing further at all, so whatever it
// was holding stays held for the rest of the session: one button down forever, on a layer nothing
// writes to any more. That is what `releaseInputSource(source: .gamepad)` is for, and it is called
// from `refresh` below on ANY change to the set of attached pads rather than only on a removal.
// See the comment there for why the blunt version is the correct one.
//
// POLLED ONCE PER FRAME, NOT DRIVEN BY `valueChangedHandler`.
//
// GameController offers both. Polling is the right one here, and not marginally:
//
//   - The engine consumes ONE COHERENT SNAPSHOT PER FRAME. `EmulatorBridge::tick` merges the
//     layers once and hands that same snapshot to every catch-up step of the tick. A handler
//     firing between two frames cannot be acted on any earlier than the next frame regardless,
//     so it buys nothing and costs a partial state: a handler has to rebuild the whole 16 button
//     array anyway, because the wire format is a complete statement, so per button callbacks
//     would do the same work more often.
//   - Read HERE, inside the display link, the state is as fresh as it can possibly be: the poll
//     happens microseconds before the step that consumes it, on the same thread, with nothing in
//     between. A handler's value is by definition from some earlier moment.
//   - Handlers run on `GCController.handlerQueue`, which defaults to the main queue but is a
//     mutable global that any other code can repoint. Polling has no such dependency.
//
// The honest cost: a press and release entirely inside one display interval, under about 16 ms,
// is not seen. That is not reachable by a human thumb, and it is not representable in the engine's
// model either, since the layer is replaced wholesale every frame. If a core ever needs sub frame
// input it needs latching in the engine, not handlers here.
//
// THE BUTTON ORDER, WHICH LOOKS WRONG AND IS RIGHT. See `frame(from:port:)`.
//
// WHAT IS DELIBERATELY ABSENT.
//
//   - `GCController.startWirelessControllerDiscovery`. That call drives the legacy MFi Bluetooth
//     pairing flow. Xbox, DualShock and DualSense pads are paired in iOS Settings, then Bluetooth,
//     by the system, and they arrive here through `GCControllerDidConnect` with no help from the
//     app. Calling it would put the app into a discovery mode that tells us nothing the
//     notification does not, so the pairing instructions in Settings point at iOS instead.
//   - `buttonHome`, the PS button and the Xbox guide button. iOS may keep it for itself, and a
//     mapping that might never fire is not a feature, it is a control that appears to do nothing.
//   - `microGamepad`, the Siri Remote. Two buttons and no shoulders is not a game pad. Such a
//     device is still reported as attached, because "I plugged something in and nothing happened"
//     deserves an answer, and it is reported as unable to play rather than counted as a pad.
//   - Button remapping. The mapping below is the W3C standard layout, which is the wire format the
//     engine documents, and a remapping UI is a feature of its own rather than a detail of this
//     one.

import Foundation
import GameController
import UIKit

/// One frame of one physical controller, in exactly the shape `applyGamepadFrom` wants.
///
/// Carries its own port so `MetalCanvas.tick` can push a variable number of controllers without
/// having to know how ports are assigned. Player 1 is port 0, which is also the port the on-screen
/// pad writes to, and that overlap is the point: the two layers merge there.
struct ControllerFrame: Sendable, Equatable {
    /// The engine port this frame belongs to. Never above `PhysicalControllers.maxPorts - 1`.
    let port: UInt32
    /// Sixteen booleans in `PadSlot` order, which is the W3C standard gamepad order and NOT
    /// libretro's. See `PadSlot` in TouchControls.swift for the full reasoning.
    let buttons: [Bool]
    /// Four floats, left X, left Y, right X, right Y, each in -1.0...1.0 with Y NEGATIVE for up.
    let axes: [Float]
}

/// Every physical controller attached to the device, and the frames the render loop reads.
///
/// Its own observable object rather than more properties on `EngineHost`, following
/// `EmulationSettings` and `ArtworkStore`: it owns one stored preference with its own key, it owns
/// the notification observers, and `EngineHost` is already the largest type in the app. The
/// Settings screen and the player screen observe it directly so that plugging a pad in updates
/// what is on screen without the host having to mirror every field.
///
/// `@MainActor` for the same reason `EngineHost` is. The notification observers below are
/// registered with `queue: .main` so that delivery on the main thread is guaranteed by
/// NotificationCenter rather than assumed of GameController, and the per frame poll is called from
/// the display link, which is also the main thread. So every entry point is main thread by
/// construction, not by hope.
@MainActor
final class PhysicalControllers: ObservableObject {

    /// `MAX_PORTS` in crates/emulator-bridge/src/input/mod.rs.
    ///
    /// Restated here rather than exported, because a constant crossing the FFI for this would be
    /// more machinery than it saves. The engine ignores a poll for a port above its own limit
    /// silently, which is the failure this number exists to avoid: a fifth pad is REPORTED as
    /// having nowhere to go rather than quietly doing nothing.
    static let maxPorts = 4

    // MARK: Published state

    /// How many attached controllers have been given an engine port, which is what the auto hide
    /// decision and the read-outs are built on.
    ///
    /// Not the same as the number of attached devices. A Siri Remote is a `GCController` with no
    /// `extendedGamepad`, and a fifth pad arrives after the ports have run out, and neither can send
    /// the engine anything.
    @Published private(set) var playablePads = 0

    /// The Settings screen's read-out: one line per attached device, or the pairing hint when
    /// there are none. Multi-line on purpose, because each device may have something to say about
    /// itself, such as a missing Options button.
    @Published private(set) var summary = PhysicalControllers.noneAttachedText

    /// The single line the diagnostics HUD carries, kept short because it shares a strip.
    @Published private(set) var diagnosticLine = "controllers: none attached"

    /// Whether the on-screen pad gets out of the way while a usable controller is attached.
    ///
    /// OFF by default, and that default is the whole design of this setting. A controller can be
    /// connected and not in the player's hands: it can be charging on a desk, or paired from
    /// yesterday and across the room, and in both cases an overlay that vanished on its own with
    /// no way back would leave the game unplayable with nothing on screen to explain why. So this
    /// is opt in, it is reversible from the same switch, it is scoped to controllers the app can
    /// actually read, and it is scoped again to controllers that can reach every control the overlay
    /// offers. See `hidesOnScreenPadNow` for that second limit, which is not an edge case: it is the
    /// difference between this switch being a convenience and it being a way to lose SELECT.
    ///
    /// The session controls, pause, reset, save state, diagnostics and the way back to the
    /// library, are NOT part of what this hides. They live on the player screen's top bar, which
    /// stays exactly where it was. Hiding the game pad cannot strand anyone.
    @Published var autoHidesOnScreenPad = false {
        didSet {
            guard oldValue != autoHidesOnScreenPad else { return }
            UserDefaults.standard.set(autoHidesOnScreenPad, forKey: Self.autoHideKey)
            // Before the status line, which reads the result. The read-outs do not mention this
            // setting, so they are not rebuilt; the status line is still told, because flipping this
            // with a controller attached changes what is on screen, and because the case where it
            // deliberately does NOT act has to be explainable.
            recomputeHiding()
            onChange?(statusSentence)
        }
    }

    /// True when the on-screen pad should be off screen right now.
    ///
    /// `PlayerScreen` mounts the overlay on this, and watches it to give the picture back the space
    /// the controls were holding.
    ///
    /// STORED AND PUBLISHED rather than computed on demand, and that is about SwiftUI rather than
    /// about speed. A computed property would be right every time it was read, but SwiftUI only
    /// re-reads this object when a `@Published` property changes, and two of the three inputs to
    /// this decision are not published: which controller holds port 0, and whether that controller
    /// has an Options button. Published, the value cannot be read stale, and `onChange` has
    /// something real to fire on.
    @Published private(set) var hidesOnScreenPadNow = false

    /// Recomputes the hiding decision. Called from everything that can change any input to it.
    private func recomputeHiding() {
        // `portZeroPadIsComplete` is the condition that stops this setting taking a button off the
        // screen that nothing else can press. See that property.
        let next = autoHidesOnScreenPad && portZeroPadIsComplete
        if hidesOnScreenPadNow != next { hidesOnScreenPadNow = next }
    }

    /// Whether the pad on port 0 can reach everything the on-screen pad can.
    ///
    /// Port 0 specifically, because that is the port the overlay writes to: hiding the overlay only
    /// takes controls away from player 1, so player 1's pad is the one that has to be able to
    /// replace them.
    ///
    /// "Complete" means it has an Options button, which is SELECT. `buttonOptions` is optional on
    /// the framework side and genuinely missing on some pads, and without it a hidden overlay would
    /// leave SELECT reachable from nothing at all, which on this library is how a Game Boy or NES
    /// game opens its menu. A pad like that keeps the overlay on screen even with the setting on,
    /// and the Settings read-out says so against that pad rather than leaving the switch looking
    /// broken. Nothing else the overlay offers is ever absent: the face buttons, the shoulders, the
    /// triggers, the D-pad and Menu are all non-optional on `GCExtendedGamepad`.
    private var portZeroPadIsComplete: Bool {
        guard let pad = pads.first?.extendedGamepad else { return false }
        return pad.buttonOptions != nil
    }

    /// Called after anything changes that the rest of the app should hear about, with a sentence
    /// for the always-visible status line.
    ///
    /// A closure rather than a reference to the host, because this object needs nothing else from
    /// it and a one way dependency is easier to reason about. Wired by `EngineHost.init`, and nil
    /// until then, which is why the first scan below cannot write a status line over the launch
    /// message.
    var onChange: ((String) -> Void)?

    // MARK: Private state

    /// Everything the system says is attached, in the order it was first seen.
    ///
    /// Order is preserved across rescans deliberately: the pad that was player 1 stays player 1 when
    /// a second one joins or leaves, which is why `refresh` filters and appends instead of adopting
    /// `GCController.controllers()` order wholesale. That order is not documented to be stable, and
    /// having players swap sides mid session because the OS reordered an array would be a genuinely
    /// baffling bug.
    ///
    /// Includes the devices that cannot play, because "I plugged something in and nothing happened"
    /// is a question the read-out has to be able to answer.
    private var attached: [GCController] = []

    /// The controllers that were given a port, INDEX IS THE ENGINE PORT.
    ///
    /// DERIVED FROM `attached` BY FILTERING, NOT BY INDEXING INTO IT, and that distinction is a bug
    /// that was in here until it was reviewed. Ports used to be the index into the full attached
    /// list, so a Siri Remote arriving first took port 0 and pushed a real controller to port 1,
    /// where most single-player cores never look: the pad was listed, lit with a player number, and
    /// completely inert. Numbering over the playable ones only is what makes "first pad to connect
    /// is player 1" true rather than nearly true.
    private var pads: [GCController] = []

    /// Held only for `releaseInputSource`. The per frame writes happen in `MetalCanvas.tick`, next
    /// to the touch write and the step they both feed, because that ordering is the thing a reader
    /// of the tick needs to be able to see. A disconnect is not a frame event: it must take effect
    /// even when no display link is running, which is why this one call lives here.
    private let engine: ContinuumEngine

    private var observers: [NSObjectProtocol] = []

    private static let autoHideKey = "continuum.controls.autoHidePad.v1"

    private static let noneAttachedText =
        "No controller attached. Pair one in iOS Settings, then Bluetooth, and it appears here "
        + "without restarting the app."

    // MARK: Life cycle

    init(engine: ContinuumEngine) {
        self.engine = engine

        if let stored = UserDefaults.standard.object(forKey: Self.autoHideKey) as? Bool {
            // Read through `object(forKey:)` rather than `bool(forKey:)` because the latter
            // answers false for a key that was never written, which is indistinguishable from a
            // user who turned it off. It happens to agree with the default here, and relying on
            // that agreement is how a default becomes impossible to change later.
            autoHidesOnScreenPad = stored
        }

        observe()

        // Scanned immediately, because a pad paired before launch is already connected and its
        // `GCControllerDidConnect` fired before this object existed. Waiting for a notification
        // that already happened is the classic way to ship a feature that only works if you
        // unplug and replug.
        refresh()
    }

    deinit {
        // Block based observers are not removed when the object holding the token dies, so
        // without this the block outlives the app's use of it. It never runs today, since this
        // object lives for the app's lifetime, and leaving the leak in place on that basis is how
        // the next owner of this file inherits a puzzle.
        for observer in observers {
            NotificationCenter.default.removeObserver(observer)
        }
    }

    private func observe() {
        let centre = NotificationCenter.default

        // TWO SEPARATE GUARANTEES ARE BEING BOUGHT HERE, AND NEITHER IS PARANOIA.
        //
        // `queue: .main` is the thread. GameController is documented to post these on the main
        // queue, but "documented to" and "guaranteed to" are different things to hang published
        // state on, and handing NotificationCenter an operation queue makes the delivery queue our
        // guarantee rather than a framework's promise. It costs nothing, because they already
        // arrive there.
        //
        // `Task { @MainActor in }` is the ISOLATION, which is a different thing from the thread and
        // is the one that has to be written out. Whether the block NotificationCenter takes is
        // `@Sendable` varies by SDK, and that decides whether this closure inherits the enclosing
        // main-actor isolation or is nonisolated. Calling a main-actor method straight from the
        // nonisolated case is not a warning, it is a compile error, so the hop is explicit and the
        // code compiles either way. Verified by compiling this file against stubs on a toolchain
        // where the block IS `@Sendable`, which is how the error was found in the first place.
        //
        // Being one main-actor turn late costs nothing real: these are connect and disconnect
        // events, not a frame path, and `refresh` re-reads the system's list rather than trusting
        // the notification, so two events arriving together cannot land out of order.
        observers = [
            centre.addObserver(forName: .GCControllerDidConnect,
                               object: nil, queue: .main) { [weak self] _ in
                Task { @MainActor in self?.refresh() }
            },
            centre.addObserver(forName: .GCControllerDidDisconnect,
                               object: nil, queue: .main) { [weak self] _ in
                Task { @MainActor in self?.refresh() }
            },
            // A pad can be turned on, or run out of battery, while the app is in the background.
            // A notification posted to a suspended app is not something to rely on, so the set is
            // rescanned on the way back in. Cheap, idempotent, and it closes the one case the two
            // notifications above cannot cover.
            centre.addObserver(forName: UIApplication.didBecomeActiveNotification,
                               object: nil, queue: .main) { [weak self] _ in
                Task { @MainActor in self?.refresh() }
            },
        ]
    }

    // MARK: Connections

    /// Rebuilds the port assignment from what the system says is attached.
    ///
    /// One path for connect, disconnect and foregrounding, rather than an add and a remove that
    /// have to agree with each other. Rescanning is a handful of pointer comparisons over at most
    /// a few objects and happens only on those three events, never per frame.
    private func refresh() {
        let live = GCController.controllers()

        // Survivors first, in their existing order, then whatever is new on the end. Identity
        // comparison rather than `==`, because two pads of the same model are two players and only
        // object identity says which is which.
        var nextAttached = attached.filter { known in live.contains { $0 === known } }
        for controller in live where !nextAttached.contains(where: { $0 === controller }) {
            nextAttached.append(controller)
        }

        // The ports go to the devices that can use one, in attached order, and stop at the engine's
        // last port. Anything else stays in `attached` for the read-out and is never polled.
        let nextPads = Array(nextAttached.filter { $0.extendedGamepad != nil }
            .prefix(Self.maxPorts))

        // Identity again, pairwise rather than `==` on the arrays, and tracked separately for the
        // two lists because they answer different questions. A Siri Remote arriving changes what
        // the read-out should say and changes no port at all, so it must not be allowed to release
        // a layer a real pad is holding.
        let attachedChanged = Self.differ(nextAttached, attached)
        let padsChanged = Self.differ(nextPads, pads)
        attached = nextAttached
        pads = nextPads

        if padsChanged {
            // THE WHOLE GAMEPAD LAYER, on any change to the ported pads, including a pure connect.
            // Deliberately blunter than "release the port that went away", and the blunt version is
            // the correct one: ports are reassigned above, so a pad leaving can shift another pad's
            // port, and the bits left on the old port would then be held by nobody with no poll
            // coming to clear them. Releasing everything costs one call and is provably complete,
            // and the next frame's poll re-establishes whatever is genuinely held. Only the
            // `.gamepad` layer is touched, so a finger on the on-screen pad at that moment keeps
            // its press.
            engine.releaseInputSource(source: .gamepad)
        }

        // Unconditional, because a pad can lose its port without the ported list changing shape,
        // and a light left on for a player number nobody has any more is a lie the hardware tells.
        // Each write is guarded inside, so this costs nothing when nothing moved.
        assignPlayerIndices()
        rebuildReadouts()

        if attachedChanged {
            onChange?(statusSentence)
        }
    }

    /// Whether two ordered lists of controllers differ, by identity and by position.
    ///
    /// Position matters as much as membership: for `pads` the index IS the engine port, so the same
    /// two pads in the other order is a different assignment and has to count as a change.
    private static func differ(_ lhs: [GCController], _ rhs: [GCController]) -> Bool {
        lhs.count != rhs.count || zip(lhs, rhs).contains { $0.0 !== $0.1 }
    }

    /// Lights the player number on the controller itself.
    ///
    /// Not cosmetic. On an Xbox or DualSense pad this is the ring or bar that tells a player which
    /// side of the couch they are, and it is the only feedback the hardware can give that the app
    /// saw it at all. A device with no port gets `.indexUnset`, so its light goes out rather than
    /// claiming a player number the engine will never read, which matches what the read-out says
    /// about it.
    ///
    /// Walks `attached` rather than `pads` so that a device which LOST its port is turned off too.
    /// Each write is compared first, because assigning the same index again is a request to the
    /// hardware, and some pads answer it by replaying their light animation.
    private func assignPlayerIndices() {
        for controller in attached {
            let port = pads.firstIndex { $0 === controller }
            let index = port.flatMap { GCControllerPlayerIndex(rawValue: $0) } ?? .indexUnset
            if controller.playerIndex != index {
                controller.playerIndex = index
            }
        }
    }

    // MARK: The frame loop

    /// The state of every playable controller, for the frame about to run.
    ///
    /// Called once per frame from `MetalCanvas.tick`, immediately before `engine.tick`. An empty
    /// array means "write nothing to the gamepad layer" rather than "write a released frame", and
    /// that is safe only because of the release in `refresh`: a pad leaving empties that layer
    /// there and then, so with nothing attached there is nothing left to state and re-stating it
    /// sixty times a second would be work with no reader. Remove that release and this shortcut
    /// becomes the bug where an unplugged controller holds a button forever.
    ///
    /// Allocating two arrays per pad per frame is on purpose rather than overlooked. The UniFFI
    /// call lowers each array into a `RustBuffer` regardless, so the Swift array is not the cost
    /// that matters, and a preallocated buffer would have to be mutated in place while the render
    /// loop holds it. `TouchControlsView.currentFrame` makes the same trade.
    func poll() -> [ControllerFrame] {
        guard !pads.isEmpty else { return [] }

        var frames: [ControllerFrame] = []
        frames.reserveCapacity(pads.count)
        for (index, controller) in pads.enumerated() {
            // Re-read rather than cached at connect time. A profile that appeared late would
            // otherwise be missed until the next rescan, and a cached object outliving its device
            // is a worse failure than one frame of nothing.
            guard let pad = controller.extendedGamepad else { continue }
            frames.append(Self.frame(from: pad, port: UInt32(index)))
        }
        return frames
    }

    /// One controller's state, translated into the engine's wire format.
    ///
    /// ## The face buttons, which look transposed and are not
    ///
    /// Two independent naming schemes meet on these four lines, and both of them are POSITIONAL,
    /// which is what makes the result correct despite how it reads:
    ///
    ///   - GameController names by position in the diamond, following the Xbox and MFi layout:
    ///     `buttonA` is south, `buttonB` is east, `buttonX` is west, `buttonY` is north. On a
    ///     DualSense that means Cross, Circle, Square, Triangle in that order.
    ///   - The W3C standard gamepad numbers by position too: 0 south, 1 east, 2 west, 3 north.
    ///     `PadSlot` is that table transcribed, and its names are the RETRO button each index ends
    ///     up as, which is why slot 0 is called `b`.
    ///
    /// So south to 0, east to 1, west to 2, north to 3 is an exact positional pairing, and the
    /// apparent transposition is only the two schemes disagreeing about names for the same places.
    /// The engine then maps index 0 to retro B, because retro follows the Nintendo arrangement
    /// where A sits to the RIGHT of B, so the bottom button of a diamond is B. The full reasoning
    /// is on `STANDARD_GAMEPAD_MAP` in crates/emulator-bridge/src/input/gamepad.rs and on
    /// `PadSlot` in TouchControls.swift. Getting this wrong does not look broken, it looks like a
    /// game whose buttons are swapped, which is the kind of bug that gets blamed on the core.
    ///
    /// ## Select and Start
    ///
    /// `buttonOptions` is the small left hand button, View on an Xbox pad and Share on a
    /// DualShock, and `buttonMenu` is the right hand one, Menu on an Xbox pad and Options on a
    /// DualShock. Those are physically the Select and Start of a modern controller, and W3C
    /// numbers them 8 and 9 in that order, so that is where they go.
    ///
    /// `buttonMenu` therefore produces retro START and nothing else, and that is the deliberate
    /// answer to "what should the menu button do". The alternative, spending it on the app's own
    /// pause, would leave a player with the on-screen pad hidden no way to press Start at all,
    /// which rules out most of the library: Start is how a Mega Drive game begins and how a
    /// PlayStation menu is confirmed. The app's pause has not been given up to buy that, because
    /// it was never on this button: pause, reset, save state and the way out live on the player
    /// screen's top bar, which auto hide does not touch, so they stay one tap away with a
    /// controller in hand.
    ///
    /// `buttonOptions` is optional on the framework side and genuinely absent on some pads. Such a
    /// pad simply has no Select, which the Settings read-out says out loud, because a player who
    /// cannot find Select should be told where it went rather than left hunting for it.
    ///
    /// ## The sticks, and the sign of Y
    ///
    /// GameController reports `yAxis` POSITIVE for up. The engine reads the opposite: in
    /// `apply_standard_gamepad_from` it is `y <= -AXIS_DEADZONE` that means Up, with
    /// `AXIS_DEADZONE` at 0.35. So Y is negated here, once, at the boundary. Without the negation
    /// every stick in the app is inverted, and inverted in a way that half the players who meet it
    /// will assume is a setting they have to find.
    ///
    /// The engine also synthesises D-pad presses from the left stick past that same deadzone,
    /// which is why nothing here turns a stick into directions: doing it in both places would mean
    /// two thresholds to keep in step, and the one that matters is the one the core reads.
    /// Clamping is the engine's job too, for the same reason.
    private static func frame(from pad: GCExtendedGamepad, port: UInt32) -> ControllerFrame {
        var buttons = [Bool](repeating: false, count: PadSlot.arrayLength)

        // Positional, not nominal. See the comment above before "fixing" these four lines.
        buttons[PadSlot.b.rawValue] = pad.buttonA.isPressed      // south
        buttons[PadSlot.a.rawValue] = pad.buttonB.isPressed      // east
        buttons[PadSlot.y.rawValue] = pad.buttonX.isPressed      // west
        buttons[PadSlot.x.rawValue] = pad.buttonY.isPressed      // north

        buttons[PadSlot.l.rawValue] = pad.leftShoulder.isPressed
        buttons[PadSlot.r.rawValue] = pad.rightShoulder.isPressed

        // The triggers are analog, and `isPressed` is GameController's own threshold on that
        // value. Used as-is rather than compared against a number chosen here, so there is one
        // definition of "pressed" for a trigger and it is Apple's, which is the one calibrated
        // per device.
        buttons[PadSlot.l2.rawValue] = pad.leftTrigger.isPressed
        buttons[PadSlot.r2.rawValue] = pad.rightTrigger.isPressed

        buttons[PadSlot.select.rawValue] = pad.buttonOptions?.isPressed ?? false
        buttons[PadSlot.start.rawValue] = pad.buttonMenu.isPressed

        // Optional on the framework side because plenty of pads cannot click their sticks. A pad
        // that cannot reports nothing rather than a false press.
        buttons[PadSlot.l3.rawValue] = pad.leftThumbstickButton?.isPressed ?? false
        buttons[PadSlot.r3.rawValue] = pad.rightThumbstickButton?.isPressed ?? false

        // Read as four buttons rather than as the pad's own axes, because that is what a D-pad is
        // to a core, and because the two-axis form would need the same deadzone decision the
        // engine already makes for sticks.
        buttons[PadSlot.up.rawValue] = pad.dpad.up.isPressed
        buttons[PadSlot.down.rawValue] = pad.dpad.down.isPressed
        buttons[PadSlot.left.rawValue] = pad.dpad.left.isPressed
        buttons[PadSlot.right.rawValue] = pad.dpad.right.isPressed

        let axes: [Float] = [
            pad.leftThumbstick.xAxis.value,
            -pad.leftThumbstick.yAxis.value,
            pad.rightThumbstick.xAxis.value,
            -pad.rightThumbstick.yAxis.value,
        ]

        return ControllerFrame(port: port, buttons: buttons, axes: axes)
    }

    // MARK: The read-outs

    /// Rebuilds both read-out strings from `pads`.
    ///
    /// Each assignment is guarded, because assigning an identical string to a `@Published` still
    /// invalidates every view observing this object. `EngineHost.refreshAudioReadout` guards for
    /// the same reason.
    private func rebuildReadouts() {
        var lines: [String] = []

        for controller in attached {
            let name = Self.name(of: controller)

            // No port means one of exactly two things, and they are worth telling apart: the device
            // is not a game pad at all, or it is one and arrived after the ports ran out. Both are
            // named rather than hidden, because "it is plugged in and nothing happens" is the
            // question this read-out exists to answer.
            guard let port = pads.firstIndex(where: { $0 === controller }) else {
                if controller.extendedGamepad == nil {
                    lines.append("\(name): attached, but it is not a game pad, so there is nothing "
                                 + "here to map. A Siri Remote reads this way.")
                } else {
                    lines.append("\(name): attached, but the engine has only \(Self.maxPorts) "
                                 + "ports and they are taken, so it is not being read.")
                }
                continue
            }

            var line = "\(name): player \(port + 1), engine port \(port)."
            // Only ever true of the pad on port 0, since that is the only port the on-screen pad
            // shares. See `portZeroPadIsComplete` for why this keeps the overlay on screen.
            if port == 0, controller.extendedGamepad?.buttonOptions == nil {
                line += " This pad has no View or Share button, so SELECT is only on the on-screen "
                    + "pad, which is why the pad stays on screen even with hiding turned on."
            }
            lines.append(line)
        }

        let nextSummary = lines.isEmpty ? Self.noneAttachedText : lines.joined(separator: "\n")
        if summary != nextSummary { summary = nextSummary }

        let nextDiagnostic = Self.diagnosticText(attached: attached, pads: pads)
        if diagnosticLine != nextDiagnostic { diagnosticLine = nextDiagnostic }

        if playablePads != pads.count { playablePads = pads.count }

        // Last, because it is the one the player screen acts on: by the time a view is told the
        // overlay is going away, everything it might read alongside that is already true.
        recomputeHiding()
    }

    /// The HUD's line. One line, kept short: it shares a strip with the frame, audio and core
    /// lines, and a diagnostic that gets truncated is not a diagnostic.
    private static func diagnosticText(attached: [GCController], pads: [GCController]) -> String {
        guard !attached.isEmpty else { return "controllers: none attached" }
        let named = pads.enumerated().map { index, controller in
            "port \(index) \(name(of: controller))"
        }
        var line = "controllers: \(attached.count) attached, \(pads.count) on a port"
        if !named.isEmpty {
            line += " (" + named.joined(separator: ", ") + ")"
        }
        return line
    }

    /// The sentence written to the always-visible status line when something changes.
    ///
    /// Derived from the new state rather than from a diff, so connect and disconnect share one
    /// path. What a player needs from this line is what is true now, not which event caused it.
    private var statusSentence: String {
        guard let first = pads.first else {
            guard attached.isEmpty else {
                return "something is attached but it cannot be read as a game pad; "
                    + "see Settings, game controllers"
            }
            return "no controller attached; the on-screen pad has port 0 to itself"
        }
        let name = Self.name(of: first)
        var sentence = pads.count == 1
            ? "\(name) is connected as player 1"
            : "\(pads.count) controllers connected; \(name) is player 1"
        if autoHidesOnScreenPad {
            // Which of these it is matters: the second one is the case where the switch is on and
            // deliberately not acting, and a player who has just flipped it deserves to be told
            // that rather than left thinking it did nothing.
            sentence += hidesOnScreenPadNow
                ? ", so the on-screen pad is hidden"
                : ", and the on-screen pad stays because that pad has no SELECT of its own"
        }
        return sentence
    }

    /// The best name the framework will give for a controller.
    ///
    /// `vendorName` is what the manufacturer wrote and is what a player recognises, but it is
    /// optional and has been seen empty rather than nil. `productCategory` is never either, and
    /// says something useful on its own ("Xbox One", "DualShock 4", "MFi"), so it is the fallback
    /// rather than a placeholder string invented here.
    private static func name(of controller: GCController) -> String {
        let vendor = controller.vendorName?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        return vendor.isEmpty ? controller.productCategory : vendor
    }
}
