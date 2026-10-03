// Continuum - extra buttons the player places anywhere on the screen.
//
// The built-in pad is a fixed set of controls that can be moved. These are ADDITIONAL controls,
// each one a floating circle that presses:
//
//   - one or more of the system's buttons at once (a combo such as A+B),
//   - one or more buttons as TURBO, pulsed by the engine while held (`ContinuumEngine.applyTurbo`),
//   - or an app action: quick save, quick load, fast forward, rewind, screenshot, menu.
//
// Stored PER SYSTEM AND PER ORIENTATION, because a button placed for a portrait GBA layout has
// nowhere sensible to go on a landscape one, and a PS1 turbo button has no meaning on an NES.
// Decoding is forgiving field by field, the same rule as `TouchLayout`: one renamed key in a
// future build must not wipe someone's buttons.
//
// The pad (`TouchControlsView`) owns one `FloatingButtonLayer`, which draws the buttons, hit-tests
// them, and in the editor drags and pinches them. The layer reads `FloatingButtonStore.shared`
// rather than being handed the list through SwiftUI, so the player screen did not need a new
// parameter and the editor and the player cannot disagree about what is stored.

import Combine
import Foundation
import SwiftUI
import UIKit

// MARK: - Model

/// Something an extra button can do that is not a game input.
enum PadAppAction: String, Codable, CaseIterable, Identifiable, Sendable {
    case quickSave
    case quickLoad
    case fastForward
    case rewind
    case screenshot
    case menu

    var id: String { rawValue }

    var title: String {
        switch self {
        case .quickSave: return "Quick save"
        case .quickLoad: return "Quick load"
        case .fastForward: return "Fast forward (hold)"
        case .rewind: return "Rewind (hold)"
        case .screenshot: return "Screenshot"
        case .menu: return "Menu (pause)"
        }
    }

    /// What the circle says. Short, because the button may be small.
    var caption: String {
        switch self {
        case .quickSave: return "SAVE"
        case .quickLoad: return "LOAD"
        case .fastForward: return "FF"
        case .rewind: return "REW"
        case .screenshot: return "SHOT"
        case .menu: return "MENU"
        }
    }

    /// Held actions act for as long as the finger is down; the rest act once, on the press.
    var isHold: Bool {
        self == .fastForward || self == .rewind
    }
}

/// One extra button.
struct FloatingButton: Equatable, Identifiable, Sendable {
    enum Kind: String, Codable, Sendable, CaseIterable {
        /// Holds every slot in `slots` while the finger is down. Two or more is a combo.
        case press
        /// Pulses every slot in `slots` while held. The engine does the pulsing.
        case turbo
        /// Runs `action`.
        case action
    }

    static let minSize = 36.0
    static let maxSize = 140.0
    static let defaultSize = 62.0
    static let minOpacity = 0.15
    static let maxOpacity = 1.0

    var id: UUID
    var kind: Kind
    /// `PadSlot.layoutKey` values. Strings rather than raw integers for the reason `TouchLayout`
    /// keys its frees by name: a stored layout stays readable and a reordered enum cannot remap it.
    var slots: [String]
    var action: PadAppAction?
    /// Centre, as a fraction of the pad view (the whole window), origin top left.
    var x: Double
    var y: Double
    /// Diameter in points.
    var size: Double
    var opacity: Double

    init(id: UUID = UUID(), kind: Kind, slots: [String] = [], action: PadAppAction? = nil,
         x: Double = 0.5, y: Double = 0.5, size: Double = FloatingButton.defaultSize,
         opacity: Double = 0.6) {
        self.id = id
        self.kind = kind
        self.slots = slots
        self.action = action
        self.x = x
        self.y = y
        self.size = size
        self.opacity = opacity
    }

    /// The engine slots this button names, dropping any key no `PadSlot` has.
    var padSlots: [PadSlot] {
        slots.compactMap { key in PadSlot.allCases.first { $0.layoutKey == key } }
    }

    /// The text on the circle, using the system's own names for its buttons.
    func caption(for system: GameSystem) -> String {
        switch kind {
        case .action:
            return action?.caption ?? "?"
        case .press, .turbo:
            let names = padSlots.map { FloatingButton.label(of: $0, on: system) }
            let joined = names.isEmpty ? "?" : names.joined(separator: "+")
            return kind == .turbo ? "T\u{00B7}" + joined : joined
        }
    }

    /// The label the system's pad prints on `slot`, or a plain name for one it does not draw.
    static func label(of slot: PadSlot, on system: GameSystem) -> String {
        if let control = system.controls.first(where: { $0.slot == slot }) {
            return control.label
        }
        switch slot {
        case .up: return "\u{2191}"
        case .down: return "\u{2193}"
        case .left: return "\u{2190}"
        case .right: return "\u{2192}"
        default: return slot.layoutKey.uppercased()
        }
    }

    /// The slots an extra button may send on `system`: everything its pad draws, plus the four
    /// directions. "Any button the system has" is what the pad says it has, because a slot the
    /// core never reads would be a button that does nothing.
    static func availableSlots(on system: GameSystem) -> [PadSlot] {
        var out: [PadSlot] = []
        for control in system.controls where !out.contains(control.slot) {
            out.append(control.slot)
        }
        for slot in [PadSlot.up, .down, .left, .right] where !out.contains(slot) {
            out.append(slot)
        }
        return out
    }

    /// Forced into range. Applied on every read, like `TouchLayout.sanitised`.
    var sanitised: FloatingButton {
        var out = self
        out.x = Self.clamp(x, 0.02, 0.98, 0.5)
        out.y = Self.clamp(y, 0.02, 0.98, 0.5)
        out.size = Self.clamp(size, Self.minSize, Self.maxSize, Self.defaultSize)
        out.opacity = Self.clamp(opacity, Self.minOpacity, Self.maxOpacity, 0.6)
        out.slots = padSlots.map(\.layoutKey)
        if kind == .action, action == nil { out.action = .quickSave }
        return out
    }

    private static func clamp(_ value: Double, _ low: Double, _ high: Double,
                              _ fallback: Double) -> Double {
        guard value.isFinite else { return fallback }
        return min(high, max(low, value))
    }
}

extension FloatingButton: Codable {
    private enum CodingKeys: String, CodingKey {
        case id, kind, slots, action, x, y, size, opacity
    }

    /// Field by field, each one falling back rather than throwing. See `TouchLayout.init(from:)`.
    init(from decoder: Decoder) throws {
        let box = try decoder.container(keyedBy: CodingKeys.self)
        id = (try? box.decodeIfPresent(UUID.self, forKey: .id)) ?? UUID()
        let rawKind = (try? box.decodeIfPresent(String.self, forKey: .kind)) ?? nil
        kind = rawKind.flatMap(Kind.init(rawValue:)) ?? .press
        slots = (try? box.decodeIfPresent([String].self, forKey: .slots)) ?? []
        let rawAction = (try? box.decodeIfPresent(String.self, forKey: .action)) ?? nil
        action = rawAction.flatMap(PadAppAction.init(rawValue:))
        x = (try? box.decodeIfPresent(Double.self, forKey: .x)) ?? 0.5
        y = (try? box.decodeIfPresent(Double.self, forKey: .y)) ?? 0.5
        size = (try? box.decodeIfPresent(Double.self, forKey: .size)) ?? Self.defaultSize
        opacity = (try? box.decodeIfPresent(Double.self, forKey: .opacity)) ?? 0.6
    }

    func encode(to encoder: Encoder) throws {
        var box = encoder.container(keyedBy: CodingKeys.self)
        try box.encode(id, forKey: .id)
        try box.encode(kind.rawValue, forKey: .kind)
        try box.encode(slots, forKey: .slots)
        try box.encodeIfPresent(action?.rawValue, forKey: .action)
        try box.encode(x, forKey: .x)
        try box.encode(y, forKey: .y)
        try box.encode(size, forKey: .size)
        try box.encode(opacity, forKey: .opacity)
    }
}

/// One system's extra buttons, one list per orientation.
struct FloatingButtonSet: Equatable, Sendable {
    var portrait: [FloatingButton] = []
    var landscape: [FloatingButton] = []

    func buttons(landscape wide: Bool) -> [FloatingButton] {
        (wide ? landscape : portrait).map(\.sanitised)
    }

    var isEmpty: Bool { portrait.isEmpty && landscape.isEmpty }
}

extension FloatingButtonSet: Codable {
    private enum CodingKeys: String, CodingKey { case portrait, landscape }

    init(from decoder: Decoder) throws {
        let box = try decoder.container(keyedBy: CodingKeys.self)
        portrait = Self.forgiving(box, .portrait)
        landscape = Self.forgiving(box, .landscape)
    }

    func encode(to encoder: Encoder) throws {
        var box = encoder.container(keyedBy: CodingKeys.self)
        try box.encode(portrait, forKey: .portrait)
        try box.encode(landscape, forKey: .landscape)
    }

    /// A list where one unreadable element is dropped rather than failing the whole list.
    private static func forgiving(_ box: KeyedDecodingContainer<CodingKeys>,
                                  _ key: CodingKeys) -> [FloatingButton] {
        guard let raw = try? box.decodeIfPresent([LossyButton].self, forKey: key) else {
            return []
        }
        return raw.compactMap(\.value)
    }

    private struct LossyButton: Decodable {
        let value: FloatingButton?
        init(from decoder: Decoder) throws {
            value = try? FloatingButton(from: decoder)
        }
    }
}

// MARK: - Store

/// Every system's extra buttons, persisted in UserDefaults.
///
/// A singleton because two unrelated views need it: the pad, which is a UIKit view the player
/// mounts with no knowledge of this feature, and the editor panel. `didChange` tells a mounted pad
/// to redraw; `@Published` drives the editor.
@MainActor
final class FloatingButtonStore: ObservableObject {
    static let shared = FloatingButtonStore()

    /// Posted after any change, so a live pad re-reads its list.
    static let didChange = Notification.Name("continuum.floatingButtons.didChange")

    @Published private(set) var sets: [String: FloatingButtonSet] = [:]
    /// The button the editor is working on. Not persisted: a selection is a moment, not a setting.
    @Published var selectedID: UUID? {
        // Posted so a live pad redraws its highlight when the selection changes from the panel.
        didSet {
            if oldValue != selectedID {
                NotificationCenter.default.post(name: Self.didChange, object: nil)
            }
        }
    }
    /// Last thing that went wrong saving, so the editor can say it rather than lose it.
    @Published private(set) var lastError: String?

    private static let key = "continuum.controls.floatingButtons.v1"

    private init() {
        if let data = UserDefaults.standard.data(forKey: Self.key) {
            sets = Self.decode(data)
        }
    }

    /// Forgiving at the top level too: one system's unreadable entry does not cost the others.
    static func decode(_ data: Data) -> [String: FloatingButtonSet] {
        guard let raw = try? JSONDecoder().decode([String: LossySet].self, from: data) else {
            return [:]
        }
        var out: [String: FloatingButtonSet] = [:]
        for (key, value) in raw {
            if let set = value.value { out[key] = set }
        }
        return out
    }

    private struct LossySet: Decodable {
        let value: FloatingButtonSet?
        init(from decoder: Decoder) throws {
            value = try? FloatingButtonSet(from: decoder)
        }
    }

    func buttons(for system: GameSystem, landscape: Bool) -> [FloatingButton] {
        sets[system.rawValue]?.buttons(landscape: landscape) ?? []
    }

    func button(id: UUID, system: GameSystem, landscape: Bool) -> FloatingButton? {
        buttons(for: system, landscape: landscape).first { $0.id == id }
    }

    /// Adds a button at the centre of the screen and selects it, so the editor's controls apply
    /// to it straight away.
    @discardableResult
    func add(_ button: FloatingButton, for system: GameSystem, landscape: Bool) -> FloatingButton {
        let clean = button.sanitised
        mutate(system) { set in
            if landscape { set.landscape.append(clean) } else { set.portrait.append(clean) }
        }
        selectedID = clean.id
        return clean
    }

    /// Replaces one button. `persist` false is for a slider mid-drag: the pad redraws, nothing is
    /// written, and the release writes once.
    func update(_ button: FloatingButton, for system: GameSystem, landscape: Bool,
                persist: Bool = true) {
        let clean = button.sanitised
        mutate(system, persist: persist) { set in
            if landscape {
                if let index = set.landscape.firstIndex(where: { $0.id == clean.id }) {
                    set.landscape[index] = clean
                }
            } else if let index = set.portrait.firstIndex(where: { $0.id == clean.id }) {
                set.portrait[index] = clean
            }
        }
    }

    func remove(id: UUID, for system: GameSystem, landscape: Bool) {
        mutate(system) { set in
            if landscape {
                set.landscape.removeAll { $0.id == id }
            } else {
                set.portrait.removeAll { $0.id == id }
            }
        }
        if selectedID == id { selectedID = nil }
    }

    func removeAll(for system: GameSystem, landscape: Bool) {
        mutate(system) { set in
            if landscape { set.landscape.removeAll() } else { set.portrait.removeAll() }
        }
        selectedID = nil
    }

    private func mutate(_ system: GameSystem, persist: Bool = true,
                        _ change: (inout FloatingButtonSet) -> Void) {
        var set = sets[system.rawValue] ?? FloatingButtonSet()
        change(&set)
        if set.isEmpty {
            sets.removeValue(forKey: system.rawValue)
        } else {
            sets[system.rawValue] = set
        }
        if persist { save() }
        NotificationCenter.default.post(name: Self.didChange, object: nil)
    }

    private func save() {
        if sets.isEmpty {
            UserDefaults.standard.removeObject(forKey: Self.key)
            lastError = nil
            return
        }
        do {
            let data = try JSONEncoder().encode(sets)
            UserDefaults.standard.set(data, forKey: Self.key)
            lastError = nil
        } catch {
            lastError = "extra buttons could not be saved: \(error.localizedDescription). "
                + "They still apply until the app quits."
        }
    }
}

// MARK: - Drawing

/// One extra button on screen. Never touchable itself; the pad reads every finger, for the same
/// multi-touch reason `ControlChip` is not touchable.
final class FloatingChipView: UIView {
    private let title = UILabel()
    private(set) var button: FloatingButton

    private static let accent = UIColor(red: 0.93, green: 0.16, blue: 0.29, alpha: 1)

    init(button: FloatingButton) {
        self.button = button
        super.init(frame: .zero)
        isUserInteractionEnabled = false
        layer.borderWidth = 1.5
        title.textAlignment = .center
        title.adjustsFontSizeToFitWidth = true
        title.minimumScaleFactor = 0.4
        title.numberOfLines = 1
        title.textColor = UIColor.white.withAlphaComponent(0.92)
        title.isUserInteractionEnabled = false
        addSubview(title)
        setPressed(false)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not supported") }

    func configure(_ button: FloatingButton, system: GameSystem, selected: Bool, editing: Bool) {
        self.button = button
        title.text = button.caption(for: system)
        alpha = editing ? max(CGFloat(button.opacity), 0.45) : CGFloat(button.opacity)
        layer.borderColor = (selected ? Self.accent : UIColor.white.withAlphaComponent(0.4)).cgColor
        layer.borderWidth = selected ? 3 : 1.5
        switch button.kind {
        case .press: backgroundColor = UIColor.white.withAlphaComponent(0.14)
        case .turbo: backgroundColor = UIColor.systemOrange.withAlphaComponent(0.28)
        case .action: backgroundColor = UIColor.systemTeal.withAlphaComponent(0.26)
        }
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        layer.cornerRadius = bounds.height / 2
        title.frame = bounds.insetBy(dx: 5, dy: 5)
        title.font = .systemFont(ofSize: max(9, bounds.height * 0.28), weight: .bold)
    }

    func setPressed(_ pressed: Bool) {
        layer.shadowOpacity = pressed ? 0.6 : 0
        layer.shadowColor = UIColor.white.cgColor
        layer.shadowRadius = pressed ? 8 : 0
        transform = pressed ? CGAffineTransform(scaleX: 0.94, y: 0.94) : .identity
    }
}

// MARK: - The layer inside the pad

/// Everything the pad needs for extra buttons, kept out of TouchControls.swift so that file only
/// carries the hooks.
@MainActor
final class FloatingButtonLayer {
    private weak var host: UIView?
    private var chips: [FloatingChipView] = []
    private(set) var buttons: [FloatingButton] = []
    private var rects: [CGRect] = []
    private var system: GameSystem = .nes
    private var landscape = false
    private var editing = false
    private var observer: NSObjectProtocol?

    /// The editor drag in progress, by touch identity.
    private var drag: (touch: ObjectIdentifier, id: UUID, offset: CGSize)?
    /// The size when a pinch began, so the pinch scales from it rather than compounding.
    private var pinchStartSize: Double?
    private var pinchID: UUID?

    /// True while an extra button is being dragged in the editor.
    var isDragging: Bool { drag != nil }

    /// Called when the stored list changed, so the pad can lay out again.
    var onChange: (() -> Void)?

    init() {
        observer = NotificationCenter.default.addObserver(
            forName: FloatingButtonStore.didChange, object: nil, queue: .main
        ) { [weak self] _ in
            // Hopped explicitly rather than assumed: the queue is main, but the closure is not
            // typed as main-actor isolated, and this keeps iOS 16 without `assumeIsolated`.
            Task { @MainActor in self?.onChange?() }
        }
    }

    deinit {
        if let observer { NotificationCenter.default.removeObserver(observer) }
    }

    func attach(to view: UIView) {
        host = view
    }

    /// Re-reads the store and places every button. Called at the end of the pad's layout pass, so
    /// the buttons sit above everything the pass placed.
    func layout(in bounds: CGRect, system: GameSystem, editing: Bool) {
        guard let host else { return }
        self.system = system
        self.editing = editing
        landscape = bounds.width > bounds.height
        let store = FloatingButtonStore.shared
        var next = store.buttons(for: system, landscape: landscape)
        // A drag in progress keeps the dragged button where the finger is rather than snapping
        // back to the stored value on an unrelated relayout.
        if let drag, let live = buttons.first(where: { $0.id == drag.id }),
           let index = next.firstIndex(where: { $0.id == drag.id }) {
            next[index].x = live.x
            next[index].y = live.y
        }
        buttons = next

        while chips.count > buttons.count {
            chips.removeLast().removeFromSuperview()
        }
        while chips.count < buttons.count {
            let chip = FloatingChipView(button: buttons[chips.count])
            host.addSubview(chip)
            chips.append(chip)
        }
        rects = []
        let selected = store.selectedID
        for (index, button) in buttons.enumerated() {
            let rect = Self.rect(for: button, in: bounds)
            rects.append(rect)
            let chip = chips[index]
            chip.frame = rect
            chip.configure(button, system: system, selected: editing && button.id == selected,
                           editing: editing)
            host.bringSubviewToFront(chip)
        }
    }

    static func rect(for button: FloatingButton, in bounds: CGRect) -> CGRect {
        let side = CGFloat(button.size)
        var centre = CGPoint(x: bounds.minX + CGFloat(button.x) * bounds.width,
                             y: bounds.minY + CGFloat(button.y) * bounds.height)
        // Kept fully on screen, so a button placed near an edge on a wider phone is not half off
        // a narrower one.
        centre.x = min(max(centre.x, bounds.minX + side / 2), bounds.maxX - side / 2)
        centre.y = min(max(centre.y, bounds.minY + side / 2), bounds.maxY - side / 2)
        return CGRect(x: centre.x - side / 2, y: centre.y - side / 2, width: side, height: side)
    }

    /// The button under a point, tested as a circle with a little slop. Last drawn wins, because
    /// it is the one on top.
    func index(at point: CGPoint, slop: CGFloat = 4) -> Int? {
        for index in rects.indices.reversed() {
            let rect = rects[index]
            let dx = point.x - rect.midX
            let dy = point.y - rect.midY
            if (dx * dx + dy * dy).squareRoot() <= rect.width / 2 + slop {
                return index
            }
        }
        return nil
    }

    /// In the editor, the area around the SELECTED button that a pinch may start in.
    ///
    /// A gesture recogniser only sees touches that hit-test to the pad, and the editing pad
    /// otherwise claims only its outlines, so two fingers spread across a 60 point button would
    /// rarely both land on it. Claiming a ring around the selected one makes the pinch reachable
    /// without the pad swallowing the rest of the editor.
    func claimsPinch(at point: CGPoint) -> Bool {
        guard editing, let id = FloatingButtonStore.shared.selectedID,
              let index = buttons.firstIndex(where: { $0.id == id }), index < rects.count
        else { return false }
        let rect = rects[index]
        let reach = max(110, rect.width * 1.6)
        let dx = point.x - rect.midX
        let dy = point.y - rect.midY
        return (dx * dx + dy * dy).squareRoot() <= reach
    }

    func button(at index: Int) -> FloatingButton? {
        index >= 0 && index < buttons.count ? buttons[index] : nil
    }

    func setPressed(_ pressed: Set<Int>) {
        for (index, chip) in chips.enumerated() {
            chip.setPressed(!editing && pressed.contains(index))
        }
    }

    // MARK: Editing

    /// Starts a drag if the touch lands on a button, selecting it. True when it did.
    func beginEdit(_ touch: UITouch, in view: UIView) -> Bool {
        let point = touch.location(in: view)
        guard drag == nil, let index = index(at: point, slop: 8) else { return false }
        let button = buttons[index]
        let rect = rects[index]
        drag = (ObjectIdentifier(touch), button.id,
                CGSize(width: point.x - rect.midX, height: point.y - rect.midY))
        if FloatingButtonStore.shared.selectedID != button.id {
            FloatingButtonStore.shared.selectedID = button.id
        }
        return true
    }

    /// True when the touch belongs to a floating drag (so the pad should not treat it as a
    /// cluster drag).
    func continueEdit(_ touches: Set<UITouch>, in view: UIView) -> Bool {
        guard let drag,
              let touch = touches.first(where: { ObjectIdentifier($0) == drag.touch }),
              let index = buttons.firstIndex(where: { $0.id == drag.id }) else { return false }
        let bounds = view.bounds
        guard bounds.width > 0, bounds.height > 0 else { return true }
        let point = touch.location(in: view)
        let centre = CGPoint(x: point.x - drag.offset.width, y: point.y - drag.offset.height)
        buttons[index].x = Double((centre.x - bounds.minX) / bounds.width)
        buttons[index].y = Double((centre.y - bounds.minY) / bounds.height)
        buttons[index] = buttons[index].sanitised
        let rect = Self.rect(for: buttons[index], in: bounds)
        if index < rects.count { rects[index] = rect }
        if index < chips.count { chips[index].frame = rect }
        return true
    }

    /// Ends a floating drag and writes where it landed. True when the touch was one.
    func endEdit(_ touches: Set<UITouch>) -> Bool {
        guard let drag, touches.contains(where: { ObjectIdentifier($0) == drag.touch }) else {
            return false
        }
        self.drag = nil
        if let button = buttons.first(where: { $0.id == drag.id }) {
            FloatingButtonStore.shared.update(button, for: system, landscape: landscape)
        }
        return true
    }

    func cancelEdit() {
        drag = nil
        pinchStartSize = nil
        pinchID = nil
    }

    /// A pinch anywhere resizes the selected button. Persisted when the pinch ends.
    func pinch(scale: CGFloat, state: UIGestureRecognizer.State) {
        let store = FloatingButtonStore.shared
        guard editing, let id = pinchID ?? store.selectedID,
              let index = buttons.firstIndex(where: { $0.id == id }) else { return }
        switch state {
        case .began:
            pinchID = id
            pinchStartSize = buttons[index].size
        case .changed:
            guard let start = pinchStartSize else { return }
            buttons[index].size = start * Double(scale)
            store.update(buttons[index], for: system, landscape: landscape, persist: false)
        case .ended, .cancelled, .failed:
            if let start = pinchStartSize {
                buttons[index].size = start * Double(scale)
            }
            store.update(buttons[index], for: system, landscape: landscape, persist: true)
            pinchStartSize = nil
            pinchID = nil
        default:
            break
        }
    }
}
