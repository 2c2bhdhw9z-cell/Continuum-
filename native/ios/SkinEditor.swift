// Continuum - edit an imported Delta skin inside the app.
//
// Shows the skin's own art with every button hit area, analog stick and screen hole outlined.
// Drag an outline to move it, drag its corner to resize it, pick a button to change which input it
// sends, fade the whole skin, or put everything back the way the file had it.
//
// NOTHING HERE WRITES THE IMPORTED FILE. Every change is a `SkinFaceEdits` overlay stored beside
// it (SkinEdits.swift), per system and per orientation, and `EngineHost.skinPadFace` lays that
// overlay over the file whenever the player asks for a face. So the player uses the edited values
// with no change of its own, and "Reset to the file" is simply an empty overlay.
//
// Outlines are drawn from the same fractions the pad reads and placed with the same aspect-fit
// (`DeltaSkinNormalizedRect.aspectFitCanvas`), so what is outlined here is where a thumb lands.

import SwiftUI
import UIKit

/// What the skin editor needs from the host, as closures, the way `TouchLayoutEditor` is wired.
struct SkinEditAccess {
    /// (system, landscape, edited) -> the face, or nil when there is none.
    let face: (GameSystem, Bool, Bool) -> SkinEditableFace?
    let edits: (GameSystem, Bool) -> SkinFaceEdits
    let setEdits: (SkinFaceEdits, GameSystem, Bool) -> Void
    /// (system, landscape) -> the skin art for that orientation.
    let image: (GameSystem, Bool) -> UIImage?
}

extension EngineHost {
    var skinEditAccess: SkinEditAccess {
        SkinEditAccess(
            face: { [weak self] system, landscape, edited in
                self?.skinEditableFace(for: system, landscape: landscape, edited: edited)
            },
            edits: { [weak self] system, landscape in
                self?.skinEdits(for: system, landscape: landscape) ?? SkinFaceEdits()
            },
            setEdits: { [weak self] edits, system, landscape in
                self?.setSkinEdits(edits, for: system, landscape: landscape)
            },
            image: { [weak self] system, landscape in
                landscape ? self?.skinLandscapeImage(for: system) : self?.skinImage(for: system)
            }
        )
    }
}

struct SkinEditor: View {
    let system: GameSystem
    let access: SkinEditAccess
    let onClose: () -> Void

    private enum Item: Hashable {
        case button(Int)
        case stick(Int)
        case screen(Int)
    }

    @State private var landscape = false
    @State private var edits = SkinFaceEdits()
    @State private var selection: Item?
    /// The item's rect when the current drag began, so a drag is relative to where it started.
    @State private var dragStart: DeltaSkinNormalizedRect?
    @State private var message: String?

    init(system: GameSystem, access: SkinEditAccess, onClose: @escaping () -> Void) {
        self.system = system
        self.access = access
        self.onClose = onClose
        let startLandscape = access.face(system, false, false) == nil
            && access.face(system, true, false) != nil
        _landscape = State(initialValue: startLandscape)
        _edits = State(initialValue: access.edits(system, startLandscape))
    }

    private var original: SkinEditableFace? { access.face(system, landscape, false) }
    private var hasPortrait: Bool { access.face(system, false, false) != nil }
    private var hasLandscape: Bool { access.face(system, true, false) != nil }

    var body: some View {
        VStack(spacing: 0) {
            header
            if let original {
                let face = edits.applied(to: original)
                canvas(face: face)
                panel(face: face, original: original)
            } else {
                Spacer()
                Text("This skin has no \(landscape ? "landscape" : "portrait") layout to edit.")
                    .font(.system(size: 14))
                    .foregroundStyle(ShellPalette.secondaryText)
                    .padding()
                Spacer()
            }
        }
        .background(Color.black.ignoresSafeArea())
    }

    // MARK: Header

    private var header: some View {
        HStack(spacing: 10) {
            VStack(alignment: .leading, spacing: 2) {
                Text("EDIT SKIN")
                    .font(.system(size: 11, weight: .bold))
                    .tracking(1.6)
                    .foregroundStyle(ShellPalette.secondaryText)
                Text(system.displayName)
                    .font(.system(.caption, design: .monospaced))
                    .foregroundStyle(.white)
                    .lineLimit(1)
            }
            Spacer(minLength: 4)
            if hasPortrait && hasLandscape {
                orientationButton("Portrait", wide: false)
                orientationButton("Landscape", wide: true)
            }
            Button {
                commit()
                onClose()
            } label: {
                Text("Done")
                    .font(.system(size: 14, weight: .bold))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 8)
                    .background(ShellPalette.accent, in: Capsule())
            }
            .buttonStyle(.plain)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
    }

    private func orientationButton(_ title: String, wide: Bool) -> some View {
        Button {
            guard landscape != wide else { return }
            commit()
            landscape = wide
            edits = access.edits(system, wide)
            selection = nil
        } label: {
            Text(title)
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(.white)
                .padding(.horizontal, 10)
                .padding(.vertical, 7)
                .background(Capsule().fill(landscape == wide ? ShellPalette.accent.opacity(0.8)
                                           : Color.white.opacity(0.10)))
        }
        .buttonStyle(.plain)
    }

    // MARK: Canvas

    private func canvas(face: SkinEditableFace) -> some View {
        GeometryReader { proxy in
            let bounds = CGRect(origin: .zero, size: proxy.size).insetBy(dx: 8, dy: 8)
            let fitted = DeltaSkinNormalizedRect.aspectFitCanvas(mapping: face.mapping, in: bounds)
            ZStack(alignment: .topLeading) {
                if let image = access.image(system, landscape) {
                    Image(uiImage: image)
                        .resizable()
                        .frame(width: fitted.width, height: fitted.height)
                        .opacity(face.opacity)
                        .position(x: fitted.midX, y: fitted.midY)
                } else {
                    Rectangle()
                        .strokeBorder(ShellPalette.hairline, lineWidth: 1)
                        .frame(width: fitted.width, height: fitted.height)
                        .position(x: fitted.midX, y: fitted.midY)
                }
                ForEach(Array(face.screens.enumerated()), id: \.offset) { index, screen in
                    outline(.screen(index), rect: screen.output, canvas: fitted,
                            colour: .red, caption: "SCREEN \(index + 1)")
                }
                ForEach(Array(face.buttons.enumerated()), id: \.offset) { index, button in
                    outline(.button(index), rect: button.frame, canvas: fitted,
                            colour: .cyan, caption: caption(forSlot: button.slot))
                }
                ForEach(Array(face.sticks.enumerated()), id: \.offset) { index, stick in
                    outline(.stick(index), rect: stick.frame, canvas: fitted,
                            colour: .green, caption: stick.side == "right" ? "R STICK" : "STICK")
                }
            }
            .contentShape(Rectangle())
            .onTapGesture { selection = nil }
        }
        .frame(maxHeight: .infinity)
    }

    private func outline(_ item: Item, rect: DeltaSkinNormalizedRect, canvas: CGRect,
                         colour: Color, caption: String) -> some View {
        let placed = rect.cgRect(in: canvas)
        let selected = selection == item
        return ZStack(alignment: .bottomTrailing) {
            Rectangle()
                .fill(colour.opacity(selected ? 0.28 : 0.12))
                .overlay(Rectangle().strokeBorder(colour, lineWidth: selected ? 3 : 1.5))
                .overlay(
                    Text(caption)
                        .font(.system(size: 9, weight: .bold))
                        .foregroundStyle(.white)
                        .lineLimit(1)
                        .minimumScaleFactor(0.5)
                        .padding(2),
                    alignment: .topLeading
                )
                .gesture(moveGesture(item, canvas: canvas))
            if selected {
                // The resize handle. Generous, because a corner a few points wide is not a target.
                Circle()
                    .fill(colour)
                    .frame(width: 26, height: 26)
                    .overlay(Image(systemName: "arrow.up.left.and.arrow.down.right")
                        .font(.system(size: 11, weight: .bold))
                        .foregroundStyle(.black))
                    .offset(x: 13, y: 13)
                    .gesture(resizeGesture(item, canvas: canvas))
            }
        }
        .frame(width: max(placed.width, 8), height: max(placed.height, 8))
        .position(x: placed.midX, y: placed.midY)
    }

    private func moveGesture(_ item: Item, canvas: CGRect) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { value in
                if dragStart == nil || selection != item {
                    selection = item
                    dragStart = currentRect(of: item)
                }
                guard let start = dragStart, canvas.width > 0, canvas.height > 0 else { return }
                var next = start
                next.x = start.x + Double(value.translation.width / canvas.width)
                next.y = start.y + Double(value.translation.height / canvas.height)
                setRect(next.clampedToCanvas, of: item)
            }
            .onEnded { _ in
                dragStart = nil
                commit()
            }
    }

    private func resizeGesture(_ item: Item, canvas: CGRect) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { value in
                if dragStart == nil { dragStart = currentRect(of: item) }
                guard let start = dragStart, canvas.width > 0, canvas.height > 0 else { return }
                var next = start
                next.width = start.width + Double(value.translation.width / canvas.width)
                next.height = start.height + Double(value.translation.height / canvas.height)
                // Grows from the top-left corner, so it is clamped against the right and bottom
                // edges by shrinking rather than by sliding the rect.
                next.width = min(next.width, 1 - next.x)
                next.height = min(next.height, 1 - next.y)
                setRect(next.clampedToCanvas, of: item)
            }
            .onEnded { _ in
                dragStart = nil
                commit()
            }
    }

    // MARK: Panel

    private func panel(face: SkinEditableFace, original: SkinEditableFace) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                if let selection {
                    selectedPanel(selection, face: face)
                } else {
                    Text("Tap an outline to select it. Drag it to move, drag its corner to resize. "
                         + "Blue is a button, green a stick, red a screen hole.")
                        .font(.system(size: 12))
                        .foregroundStyle(ShellPalette.secondaryText)
                        .fixedSize(horizontal: false, vertical: true)
                }

                VStack(alignment: .leading, spacing: 4) {
                    Text("SKIN OPACITY \(Int((edits.effectiveOpacity * 100).rounded()))%")
                        .font(.system(size: 10, weight: .bold))
                        .tracking(1.1)
                        .foregroundStyle(Color.white.opacity(0.45))
                    Slider(
                        value: Binding(
                            get: { edits.effectiveOpacity },
                            set: { edits.opacity = $0 >= 0.999 ? nil : $0 }
                        ),
                        in: SkinFaceEdits.minOpacity...1,
                        onEditingChanged: { editing in if !editing { commit() } }
                    )
                    .tint(ShellPalette.accent)
                }

                HStack(spacing: 10) {
                    SettingsButton(title: "Reset \(landscape ? "landscape" : "portrait") to the file",
                                   role: .destructive) {
                        edits = SkinFaceEdits()
                        selection = nil
                        commit()
                        message = "Back to the imported file's \(landscape ? "landscape" : "portrait") layout."
                    }
                    .disabled(edits.isEmpty)
                    .opacity(edits.isEmpty ? 0.45 : 1)
                }

                if let message {
                    Text(message)
                        .font(.system(size: 12))
                        .foregroundStyle(Color.white.opacity(0.85))
                }

                Text("Edits are kept on top of the imported skin, which is never changed. "
                     + "Importing the skin again starts from the file.")
                    .font(.system(size: 11))
                    .foregroundStyle(ShellPalette.secondaryText)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(14)
        }
        .frame(maxHeight: 260)
        .background(Color.black.opacity(0.85))
    }

    @ViewBuilder
    private func selectedPanel(_ item: Item, face: SkinEditableFace) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title(of: item, face: face))
                .font(.system(size: 10, weight: .bold))
                .tracking(1.1)
                .foregroundStyle(ShellPalette.accent)

            if case .button(let index) = item, face.buttons.indices.contains(index),
               face.buttons[index].slot != "dpad" {
                Text("SENDS")
                    .font(.system(size: 10, weight: .bold))
                    .tracking(1.1)
                    .foregroundStyle(Color.white.opacity(0.45))
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 58), spacing: 6)],
                          alignment: .leading, spacing: 6) {
                    ForEach(FloatingButton.availableSlots(on: system), id: \.rawValue) { slot in
                        let chosen = face.buttons[index].slot == slot.layoutKey
                        Button {
                            setSlot(slot, forButton: index)
                        } label: {
                            Text(FloatingButton.label(of: slot, on: system))
                                .font(.system(size: 12, weight: .semibold))
                                .lineLimit(1)
                                .minimumScaleFactor(0.6)
                                .foregroundStyle(.white)
                                .frame(maxWidth: .infinity, minHeight: 30)
                                .background(Capsule().fill(chosen ? ShellPalette.accent.opacity(0.85)
                                                           : Color.white.opacity(0.10)))
                        }
                        .buttonStyle(.plain)
                    }
                }
            }

            SettingsButton(title: "Reset this one", role: .normal) {
                resetItem(item)
                commit()
            }
        }
    }

    // MARK: Edits

    private func currentRect(of item: Item) -> DeltaSkinNormalizedRect? {
        guard let original else { return nil }
        let face = edits.applied(to: original)
        switch item {
        case .button(let index):
            return face.buttons.indices.contains(index) ? face.buttons[index].frame : nil
        case .stick(let index):
            return face.sticks.indices.contains(index) ? face.sticks[index].frame : nil
        case .screen(let index):
            return face.screens.indices.contains(index) ? face.screens[index].output : nil
        }
    }

    private func setRect(_ rect: DeltaSkinNormalizedRect, of item: Item) {
        switch item {
        case .button(let index): edits.buttonFrames[String(index)] = rect
        case .stick(let index): edits.stickFrames[String(index)] = rect
        case .screen(let index): edits.screenFrames[String(index)] = rect
        }
    }

    private func setSlot(_ slot: PadSlot, forButton index: Int) {
        guard let original, original.buttons.indices.contains(index) else { return }
        if original.buttons[index].slot == slot.layoutKey {
            edits.buttonSlots.removeValue(forKey: String(index))
        } else {
            edits.buttonSlots[String(index)] = slot.layoutKey
        }
        commit()
    }

    private func resetItem(_ item: Item) {
        switch item {
        case .button(let index):
            edits.buttonFrames.removeValue(forKey: String(index))
            edits.buttonSlots.removeValue(forKey: String(index))
        case .stick(let index):
            edits.stickFrames.removeValue(forKey: String(index))
        case .screen(let index):
            edits.screenFrames.removeValue(forKey: String(index))
        }
    }

    private func commit() {
        access.setEdits(edits, system, landscape)
    }

    private func title(of item: Item, face: SkinEditableFace) -> String {
        switch item {
        case .button(let index):
            guard face.buttons.indices.contains(index) else { return "BUTTON" }
            return "BUTTON: " + caption(forSlot: face.buttons[index].slot)
        case .stick(let index):
            return index < face.sticks.count && face.sticks[index].side == "right"
                ? "RIGHT STICK" : "ANALOG STICK"
        case .screen(let index):
            return "SCREEN HOLE \(index + 1)"
        }
    }

    private func caption(forSlot key: String) -> String {
        if key == "dpad" { return "D-PAD" }
        guard let slot = PadSlot.allCases.first(where: { $0.layoutKey == key }) else {
            return key.uppercased()
        }
        return FloatingButton.label(of: slot, on: system)
    }
}
