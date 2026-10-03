// Continuum - the editor's panel for extra buttons: add, retype, resize, fade and delete.
//
// Lives inside the scrolling body of `TouchLayoutEditor`'s card. NO SwiftUI `Menu` in here, on
// purpose: the editor's own notes record that a Menu inside that scroller never ran its row
// action on device (build 98). Every choice below is a plain button instead.
//
// Placement is done on the pad itself: tap an extra button to select it, drag it to move it,
// pinch around the selected one to resize it. This panel does the rest.

import SwiftUI

struct FloatingButtonsPanel: View {
    let system: GameSystem
    let landscape: Bool

    @ObservedObject private var store = FloatingButtonStore.shared

    private var buttons: [FloatingButton] {
        store.buttons(for: system, landscape: landscape)
    }

    private var selected: FloatingButton? {
        guard let id = store.selectedID else { return nil }
        return store.button(id: id, system: system, landscape: landscape)
    }

    private var orientationWord: String { landscape ? "landscape" : "portrait" }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("EXTRA BUTTONS, \(system.badge) \(orientationWord.uppercased())")
                .font(.system(size: 10, weight: .bold))
                .tracking(1.1)
                .foregroundStyle(Color.white.opacity(0.45))

            HStack(spacing: 8) {
                addButton("+ Button") {
                    FloatingButton(kind: .press, slots: [defaultSlot.layoutKey])
                }
                addButton("+ Turbo") {
                    FloatingButton(kind: .turbo, slots: [defaultSlot.layoutKey])
                }
                addButton("+ Action") {
                    FloatingButton(kind: .action, action: .quickSave)
                }
            }

            if buttons.isEmpty {
                note("None yet for \(system.displayName) in \(orientationWord). Added buttons "
                     + "appear in the middle of the screen: drag one to place it, pinch around it or use the size slider to resize.")
            } else {
                chipGrid(items: buttons.map { ($0.id.uuidString, $0.caption(for: system),
                                               $0.id == store.selectedID) }) { key in
                    store.selectedID = UUID(uuidString: key)
                }
            }

            if let selected {
                editor(for: selected)
            }

            if let error = store.lastError {
                note(error)
            }
        }
    }

    private var defaultSlot: PadSlot {
        FloatingButton.availableSlots(on: system).first ?? .a
    }

    private func addButton(_ title: String, make: @escaping () -> FloatingButton) -> some View {
        Button {
            var made = make()
            made.x = 0.5
            made.y = landscape ? 0.5 : 0.45
            store.add(made, for: system, landscape: landscape)
        } label: {
            Text(title)
                .font(.system(size: 13, weight: .bold))
                .foregroundStyle(.white)
                .frame(maxWidth: .infinity, minHeight: 36)
                .background(ShellPalette.surfaceStrong, in: Capsule())
        }
        .buttonStyle(.plain)
    }

    // MARK: The selected button

    @ViewBuilder
    private func editor(for button: FloatingButton) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("SELECTED: \(button.caption(for: system))")
                .font(.system(size: 10, weight: .bold))
                .tracking(1.1)
                .foregroundStyle(ShellPalette.accent)

            chipGrid(items: FloatingButton.Kind.allCases.map {
                ($0.rawValue, kindTitle($0), $0 == button.kind)
            }) { key in
                guard let kind = FloatingButton.Kind(rawValue: key) else { return }
                var next = button
                next.kind = kind
                if kind == .action, next.action == nil { next.action = .quickSave }
                if kind != .action, next.padSlots.isEmpty { next.slots = [defaultSlot.layoutKey] }
                store.update(next, for: system, landscape: landscape)
            }

            switch button.kind {
            case .press, .turbo:
                note(button.kind == .turbo
                     ? "Pressed on and off while held, at the Turbo speed in Settings. Pick one or more."
                     : "Held while the finger is down. Pick two or more for a combo such as A+B.")
                chipGrid(items: FloatingButton.availableSlots(on: system).map { slot in
                    (slot.layoutKey, FloatingButton.label(of: slot, on: system),
                     button.slots.contains(slot.layoutKey))
                }) { key in
                    var next = button
                    if let index = next.slots.firstIndex(of: key) {
                        // At least one input stays, so the button never does nothing.
                        if next.slots.count > 1 { next.slots.remove(at: index) }
                    } else {
                        next.slots.append(key)
                    }
                    store.update(next, for: system, landscape: landscape)
                }
            case .action:
                chipGrid(items: PadAppAction.allCases.map {
                    ($0.rawValue, $0.title, $0 == button.action)
                }) { key in
                    var next = button
                    next.action = PadAppAction(rawValue: key)
                    store.update(next, for: system, landscape: landscape)
                }
            }

            slider(title: "SIZE \(Int(button.size)) PT", value: button.size,
                   range: FloatingButton.minSize...FloatingButton.maxSize) { value, done in
                var next = button
                next.size = value
                store.update(next, for: system, landscape: landscape, persist: done)
            }
            slider(title: "OPACITY \(Int((button.opacity * 100).rounded()))%",
                   value: button.opacity,
                   range: FloatingButton.minOpacity...FloatingButton.maxOpacity) { value, done in
                var next = button
                next.opacity = value
                store.update(next, for: system, landscape: landscape, persist: done)
            }

            HStack(spacing: 10) {
                SettingsButton(title: "Delete", role: .destructive) {
                    store.remove(id: button.id, for: system, landscape: landscape)
                }
                SettingsButton(title: "Deselect", role: .normal) {
                    store.selectedID = nil
                }
            }
        }
        .padding(10)
        .background(
            RoundedRectangle(cornerRadius: 10)
                .fill(Color.white.opacity(0.05))
                .overlay(
                    RoundedRectangle(cornerRadius: 10)
                        .strokeBorder(ShellPalette.hairline, lineWidth: 1)
                )
        )
    }

    private func kindTitle(_ kind: FloatingButton.Kind) -> String {
        switch kind {
        case .press: return "Button"
        case .turbo: return "Turbo"
        case .action: return "Action"
        }
    }

    // MARK: Pieces

    private func note(_ text: String) -> some View {
        Text(text)
            .font(.system(size: 12))
            .foregroundStyle(ShellPalette.secondaryText)
            .fixedSize(horizontal: false, vertical: true)
    }

    /// One tappable chip: a stable key, what it says, and whether it is lit.
    private struct ChipItem: Identifiable {
        let id: String
        let title: String
        let selected: Bool
    }

    /// A wrap of tappable chips. `items` is (key, title, selected).
    private func chipGrid(items: [(String, String, Bool)],
                          onTap: @escaping (String) -> Void) -> some View {
        let chips = items.map { ChipItem(id: $0.0, title: $0.1, selected: $0.2) }
        return LazyVGrid(columns: [GridItem(.adaptive(minimum: 64), spacing: 6)],
                         alignment: .leading, spacing: 6) {
            ForEach(chips) { item in
                Button {
                    onTap(item.id)
                } label: {
                    Text(item.title)
                        .font(.system(size: 12, weight: .semibold))
                        .lineLimit(1)
                        .minimumScaleFactor(0.6)
                        .foregroundStyle(.white)
                        .frame(maxWidth: .infinity, minHeight: 32)
                        .background(
                            Capsule().fill(item.selected ? ShellPalette.accent.opacity(0.85)
                                           : Color.white.opacity(0.10))
                        )
                }
                .buttonStyle(.plain)
            }
        }
    }

    /// A slider that redraws the pad while dragged and writes once, on release.
    private func slider(title: String, value: Double, range: ClosedRange<Double>,
                        onChange: @escaping (Double, Bool) -> Void) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title)
                .font(.system(size: 10, weight: .bold))
                .tracking(1.1)
                .foregroundStyle(Color.white.opacity(0.45))
            Slider(
                value: Binding(get: { value }, set: { onChange($0, false) }),
                in: range,
                onEditingChanged: { editing in
                    if !editing { onChange(value, true) }
                }
            )
            .tint(ShellPalette.accent)
        }
    }
}
