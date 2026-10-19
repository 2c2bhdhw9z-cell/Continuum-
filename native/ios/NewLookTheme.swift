// The New look's Theme sheet (Settings, LOOK, Theme), its colour wheel, and the moving
// background.
//
// Everything here only shows while the New look is on. The Current look is untouched.
// The settings live in `NewLookColors` (LookTheme.swift). The sheet is presented by `RootView`,
// outside the library, so the library redrawing in a new colour never closes or resets it.

import SwiftUI
import UIKit

// MARK: - The row in Settings

/// Under the Look switch while New is on: opens the Theme sheet.
struct ThemeRow: View {
    @ObservedObject private var colors = NewLookColors.shared

    var body: some View {
        Button {
            colors.showingTheme = true
        } label: {
            HStack(spacing: 12) {
                Circle()
                    .fill(colors.accentFill)
                    .frame(width: 30, height: 30)
                    .overlay(Circle().strokeBorder(Color.white.opacity(0.25), lineWidth: 1))
                VStack(alignment: .leading, spacing: 2) {
                    Text("Theme")
                        .font(.system(size: 16, weight: .semibold))
                        .foregroundStyle(.white)
                    Text("Colours, background, box art and console order")
                        .font(.system(size: 12))
                        .foregroundStyle(Color.white.opacity(0.6))
                        .lineLimit(2)
                }
                Spacer(minLength: 0)
                Image(systemName: "chevron.right")
                    .font(.system(size: 13, weight: .bold))
                    .foregroundStyle(ShellPalette.gold)
            }
            .padding(12)
            .background(Color.white.opacity(0.06),
                        in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

// MARK: - The Theme sheet

struct ThemeSheet: View {
    @ObservedObject var host: EngineHost
    @ObservedObject private var colors = NewLookColors.shared
    @Environment(\.dismiss) private var dismiss

    /// What is open over the sheet: the colour wheel for one setting, or the console order.
    enum Sub: Identifiable {
        case accent, glow, ring, background, order
        var id: Self { self }
    }
    @State private var sub: Sub?

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Theme")
                    .font(.system(size: 28, weight: .bold))
                    .foregroundStyle(.white)
                Spacer(minLength: 0)
                Button {
                    dismiss()
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 14, weight: .bold))
                        .foregroundStyle(.white)
                        .frame(width: 34, height: 34)
                        .background(Color.white.opacity(0.12), in: Circle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Close")
            }
            .padding(.horizontal, 20)
            .padding(.top, 20)
            .padding(.bottom, 12)

            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    card("Colours") { accentSection }
                    card("Cover glow") { glowSection }
                    card("Ring") { ringSection }
                    card("Background") { backgroundSection }
                    card("Box art") { boxArtSection }
                    card("Console order") { orderSection }
                    if !colors.isDefault {
                        Button {
                            colors.resetToDefaults()
                        } label: {
                            Text("Reset theme")
                                .font(.system(size: 16, weight: .semibold))
                                .foregroundStyle(.white)
                                .frame(maxWidth: .infinity)
                                .padding(.vertical, 13)
                                .background(Color.white.opacity(0.08), in: Capsule())
                        }
                        .buttonStyle(.plain)
                        .padding(.horizontal, 16)
                    }
                }
                .padding(.bottom, 30)
            }
        }
        .background(NewLookBackdrop().ignoresSafeArea())
        .preferredColorScheme(.dark)
        .tint(colors.accent)
        .sheet(item: $sub) { which in
            switch which {
            case .order:
                ConsoleOrderSheet(host: host) { sub = nil }
                    .preferredColorScheme(.dark)
            default:
                picker(for: which)
            }
        }
    }

    // MARK: Sections

    private var accentSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 12) {
                    ForEach(NewLookColors.accentPresets, id: \.self) { value in
                        swatch(value, current: colors.accentHex ?? NewLookColors.defaultAccent) {
                            colors.accentHex = value == NewLookColors.defaultAccent ? nil : value
                        }
                    }
                    ForEach(colors.savedSwatches, id: \.self) { value in
                        swatch(value, current: colors.accentHex ?? NewLookColors.defaultAccent) {
                            colors.accentHex = value
                        }
                        .contextMenu {
                            Button(role: .destructive) {
                                colors.deleteSwatch(value)
                            } label: {
                                Label("Delete this colour", systemImage: "trash")
                            }
                        }
                    }
                    addButton { sub = .accent }
                }
                .padding(6)
            }
            note(colors.savedSwatches.isEmpty
                 ? "Tap a colour to use it. + makes your own."
                 : "Tap a colour to use it. + makes your own. Press and hold one of yours to delete it.")
        }
    }

    private var glowSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            chips(NewLookColors.GlowMode.allCases, current: colors.glowMode, title: { $0.title }) { mode in
                if mode == .custom { sub = .glow } else { colors.glowMode = mode }
            }
            note("The thin light round each cover.")
        }
    }

    private var ringSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            chips(NewLookColors.RingMode.allCases, current: colors.ringMode, title: { $0.title }) { mode in
                if mode == .custom { sub = .ring } else { colors.ringMode = mode }
            }
            Toggle(isOn: $colors.ringSpins) { label("Spin the ring") }
        }
    }

    private var backgroundSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 12) {
                    ForEach(NewLookColors.backgroundPresets, id: \.self) { value in
                        swatch(value, current: colors.backgroundHex ?? NewLookColors.defaultBackground) {
                            colors.backgroundHex = value == NewLookColors.defaultBackground ? nil : value
                        }
                    }
                    addButton { sub = .background }
                }
                .padding(6)
            }
            Toggle(isOn: $colors.backgroundMoves) { label("Moving gradient") }
            note("A slow, soft gradient made from your colours. It holds still when Reduce Motion "
                 + "is on, and it stops while you play.")
        }
    }

    private var boxArtSection: some View {
        VStack(alignment: .leading, spacing: 14) {
            Picker("Style", selection: $colors.coverStyle) {
                Text("Style 1").tag(1)
                Text("Style 2").tag(2)
                Text("Style 3").tag(3)
            }
            .pickerStyle(.segmented)

            HStack {
                Spacer(minLength: 0)
                boxArtPreview
                Spacer(minLength: 0)
            }

            VStack(alignment: .leading, spacing: 6) {
                HStack {
                    label("Corners")
                    Spacer(minLength: 0)
                    Text("\(Int((colors.coverCorner * 100).rounded()))%")
                        .font(.system(size: 14, weight: .semibold).monospacedDigit())
                        .foregroundStyle(Color.white.opacity(0.7))
                }
                Slider(value: $colors.coverCorner, in: 0...1, step: 0.05)
            }
            Toggle(isOn: $colors.forceSquare) { label("Square covers") }
            Toggle(isOn: $colors.hideTitles) { label("Hide game titles") }
            note("For the covers on Home, in the Library and in Favorites. Covers are never cut "
                 + "off: a square cover sits whole inside its square.")
        }
    }

    private var orderSection: some View {
        Button {
            sub = .order
        } label: {
            HStack {
                label("Change the console order")
                Spacer(minLength: 0)
                Image(systemName: "chevron.right")
                    .font(.system(size: 13, weight: .bold))
                    .foregroundStyle(ShellPalette.gold)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    /// One real game from the library drawn the way its cards will be, or a blank card when
    /// there are no games yet.
    @ViewBuilder
    private var boxArtPreview: some View {
        if let entry = host.library.first {
            NewLookGlowCard(entry: entry, system: CoreCatalog.system(for: entry),
                            store: host.artwork, generation: host.artwork.generation,
                            isFavourite: false, width: 120, cardSize: .medium, host: host,
                            line: GameMetadata.cardLine(for: entry, system: CoreCatalog.system(for: entry)),
                            frameAspect: 0.8)
                .allowsHitTesting(false)
                // Drawn again on every change, so the preview always shows the current style.
                .id(colors.version)
                .padding(.vertical, 8)
        } else {
            RoundedRectangle(cornerRadius: NewLookCoverStyle.radius(width: 120), style: .continuous)
                .fill(Color.white.opacity(0.08))
                .frame(width: 120, height: colors.forceSquare ? 120 : 150)
                .overlay(Image(systemName: "photo").foregroundStyle(Color.white.opacity(0.4)))
                .padding(.vertical, 8)
        }
    }

    // MARK: Pieces

    private func card<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title)
                .font(.system(size: 17, weight: .bold))
                .foregroundStyle(.white)
            content()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(16)
        .background(Color.white.opacity(0.06), in: RoundedRectangle(cornerRadius: 20, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 20, style: .continuous)
                    .strokeBorder(Color.white.opacity(0.09), lineWidth: 0.5))
        .padding(.horizontal, 16)
    }

    private func label(_ text: String) -> some View {
        Text(text)
            .font(.system(size: 15, weight: .semibold))
            .foregroundStyle(.white)
    }

    private func note(_ text: String) -> some View {
        Text(text)
            .font(.system(size: 12))
            .foregroundStyle(Color.white.opacity(0.55))
            .fixedSize(horizontal: false, vertical: true)
    }

    private func swatch(_ value: String, current: String, pick: @escaping () -> Void) -> some View {
        let selected = value.caseInsensitiveCompare(current) == .orderedSame
        return Button(action: pick) {
            ThemeSwatch(value: value)
                .frame(width: 40, height: 40)
                .overlay(
                    Circle()
                        .strokeBorder(selected ? Color.white : Color.clear, lineWidth: 2.5)
                        .padding(-5)
                )
                .padding(2)
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Colour \(value)")
        .accessibilityAddTraits(selected ? [.isButton, .isSelected] : .isButton)
    }

    private func addButton(_ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: "plus")
                .font(.system(size: 16, weight: .bold))
                .foregroundStyle(.white)
                .frame(width: 40, height: 40)
                .background(Color.white.opacity(0.10), in: Circle())
                .overlay(Circle().strokeBorder(Color.white.opacity(0.25),
                                               style: StrokeStyle(lineWidth: 1, dash: [3, 3])))
                .padding(2)
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Make your own colour")
    }

    private func chips<Mode: Identifiable & Equatable>(
        _ modes: [Mode], current: Mode, title: @escaping (Mode) -> String,
        pick: @escaping (Mode) -> Void
    ) -> some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(modes) { mode in
                    let selected = mode == current
                    Button {
                        pick(mode)
                    } label: {
                        Text(title(mode))
                            .font(.system(size: 13, weight: .semibold))
                            .foregroundStyle(selected ? ShellPalette.onAccent : Color.white)
                            .padding(.horizontal, 14)
                            .padding(.vertical, 8)
                            .background(selected ? AnyShapeStyle(colors.accentFill)
                                                 : AnyShapeStyle(Color.white.opacity(0.08)),
                                        in: Capsule())
                    }
                    .buttonStyle(.plain)
                    .accessibilityAddTraits(selected ? [.isButton, .isSelected] : .isButton)
                }
            }
        }
    }

    // MARK: The colour wheel, per setting

    @ViewBuilder
    private func picker(for which: Sub) -> some View {
        switch which {
        case .accent:
            ThemeColorPicker(title: "Your colour", start: colors.accentHex ?? NewLookColors.defaultAccent,
                             defaultValue: NewLookColors.defaultAccent, allowsGradient: true,
                             onClose: { sub = nil }) { value in
                colors.accentHex = value == NewLookColors.defaultAccent ? nil : value
            }
        case .glow:
            ThemeColorPicker(title: "Cover glow colour", start: colors.glowHex ?? NewLookColors.defaultSingle,
                             defaultValue: NewLookColors.defaultSingle, allowsGradient: false,
                             onClose: { sub = nil }) { value in
                colors.glowHex = value
                colors.glowMode = .custom
            }
        case .ring:
            ThemeColorPicker(title: "Ring colour", start: colors.ringHex ?? NewLookColors.defaultSingle,
                             defaultValue: NewLookColors.defaultSingle, allowsGradient: false,
                             onClose: { sub = nil }) { value in
                colors.ringHex = value
                colors.ringMode = .custom
            }
        case .background, .order:
            ThemeColorPicker(title: "Background colour",
                             start: colors.backgroundHex ?? NewLookColors.defaultBackground,
                             defaultValue: NewLookColors.defaultBackground, allowsGradient: false,
                             darkOnly: true, onClose: { sub = nil }) { value in
                let dark = NewLookColors.darkened(hex: value)
                colors.backgroundHex = dark == NewLookColors.defaultBackground ? nil : dark
            }
        }
    }
}

/// A round swatch: one colour, or a gradient of two.
struct ThemeSwatch: View {
    let value: String

    var body: some View {
        let stops = NewLookColors.stops(value).map(NewLookColors.color(hex:))
        Circle()
            .fill(LinearGradient(colors: stops.count == 1 ? [stops[0], stops[0]] : stops,
                                 startPoint: .topLeading, endPoint: .bottomTrailing))
            .overlay(Circle().strokeBorder(Color.white.opacity(0.22), lineWidth: 1))
    }
}

// MARK: - The colour wheel

/// Pick any colour: a wheel (hue round it, strength out from the middle) with a handle to drag,
/// a house mark at the default colour (tap it to go back), a brightness slider, a preview, a hex
/// box, + to keep the colour in your saved row, and Save. With `allowsGradient`, a second colour
/// can be added for a gradient; the wheel then edits whichever end is picked.
struct ThemeColorPicker: View {
    let title: String
    let start: String
    let defaultValue: String
    var allowsGradient = false
    /// The background: the preview shows it dimmed as it will be used.
    var darkOnly = false
    let onClose: () -> Void
    let onSave: (String) -> Void

    @ObservedObject private var colors = NewLookColors.shared
    @State private var ends: [String] = []
    @State private var editingEnd = 0
    @State private var hue = 0.0
    @State private var saturation = 0.0
    @State private var brightness = 1.0
    @State private var hexText = ""
    @State private var hexInvalid = false
    @State private var savedFlash = false
    @FocusState private var hexFocused: Bool

    private var value: String { ends.joined(separator: "-") }
    private var shown: String { darkOnly ? NewLookColors.darkened(hex: value) : value }

    var body: some View {
        VStack(spacing: 18) {
            HStack {
                Text(title)
                    .font(.system(size: 20, weight: .bold))
                    .foregroundStyle(.white)
                Spacer(minLength: 0)
                Button(action: onClose) {
                    Image(systemName: "xmark")
                        .font(.system(size: 14, weight: .bold))
                        .foregroundStyle(.white)
                        .frame(width: 34, height: 34)
                        .background(Color.white.opacity(0.12), in: Circle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Close without saving")
            }

            ScrollView {
                VStack(spacing: 18) {
                    if allowsGradient {
                        Picker("Kind", selection: gradientBinding) {
                            Text("One colour").tag(false)
                            Text("Gradient").tag(true)
                        }
                        .pickerStyle(.segmented)
                        if ends.count == 2 {
                            Picker("End", selection: $editingEnd) {
                                Text("First colour").tag(0)
                                Text("Second colour").tag(1)
                            }
                            .pickerStyle(.segmented)
                        }
                    }

                    ThemeWheel(hue: $hue, saturation: $saturation, brightness: brightness,
                               home: NewLookColors.hsb(hex: NewLookColors.stops(defaultValue)[0])) {
                        resetToDefault()
                    }
                    .frame(width: 250, height: 250)

                    ThemeBrightnessSlider(value: $brightness, hue: hue, saturation: saturation)
                        .frame(height: 40)

                    HStack(spacing: 14) {
                        ThemeSwatch(value: shown)
                            .frame(width: 52, height: 52)
                        VStack(alignment: .leading, spacing: 4) {
                            HStack(spacing: 4) {
                                Text("#")
                                    .foregroundStyle(Color.white.opacity(0.6))
                                TextField("93255F", text: $hexText)
                                    .textInputAutocapitalization(.characters)
                                    .autocorrectionDisabled()
                                    .keyboardType(.asciiCapable)
                                    .focused($hexFocused)
                                    .submitLabel(.done)
                                    .onSubmit { applyHex() }
                                    .onChange(of: hexText) { text in
                                        let clean = String(text.uppercased().filter { $0.isHexDigit }.prefix(6))
                                        if clean != text { hexText = clean }
                                        hexInvalid = false
                                        if clean.count == 6 { applyHex() }
                                    }
                            }
                            .font(.system(size: 18, weight: .semibold, design: .monospaced))
                            .foregroundStyle(.white)
                            .padding(.horizontal, 12)
                            .padding(.vertical, 9)
                            .background(Color.white.opacity(0.08), in: RoundedRectangle(cornerRadius: 10))
                            .overlay(RoundedRectangle(cornerRadius: 10)
                                        .strokeBorder(hexInvalid ? ShellPalette.danger : Color.clear, lineWidth: 1))
                            if hexInvalid {
                                Text("Use six characters, 0 to 9 and A to F.")
                                    .font(.system(size: 11))
                                    .foregroundStyle(ShellPalette.danger)
                            }
                        }
                        Button {
                            colors.saveSwatch(value)
                            savedFlash = true
                        } label: {
                            Image(systemName: savedFlash ? "checkmark" : "plus")
                                .font(.system(size: 16, weight: .bold))
                                .foregroundStyle(.white)
                                .frame(width: 44, height: 44)
                                .background(Color.white.opacity(0.12), in: Circle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel("Keep this colour in your saved colours")
                    }

                    Button {
                        applyHex()
                        guard !hexInvalid else { return }
                        onSave(value)
                        onClose()
                    } label: {
                        Text("Save")
                            .font(.system(size: 17, weight: .bold))
                            .foregroundStyle(NewLookColors.isLight(hex: NewLookColors.middle(shown))
                                             ? Color.black : Color.white)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 15)
                            .background(ThemeSwatch(value: shown).clipShape(Capsule()))
                            .clipShape(Capsule())
                    }
                    .buttonStyle(.plain)
                }
                .padding(.bottom, 20)
            }
        }
        .padding(20)
        .background(ShellPalette.charcoal.ignoresSafeArea())
        .preferredColorScheme(.dark)
        .presentationDetents([.large])
        .onAppear(perform: load)
        .onChange(of: editingEnd) { _ in loadEnd() }
        .onChange(of: hue) { _ in commitWheel() }
        .onChange(of: saturation) { _ in commitWheel() }
        .onChange(of: brightness) { _ in commitWheel() }
    }

    private var gradientBinding: Binding<Bool> {
        Binding(
            get: { ends.count == 2 },
            set: { on in
                if on && ends.count == 1 {
                    ends.append(NewLookColors.shiftedHue(ends[0], by: 0.08))
                    editingEnd = 1
                } else if !on && ends.count == 2 {
                    ends = [ends[0]]
                    editingEnd = 0
                }
                loadEnd()
            }
        )
    }

    private func load() {
        var start = NewLookColors.validated(self.start) ?? NewLookColors.validated(defaultValue) ?? "FAC442"
        if !allowsGradient { start = NewLookColors.middle(start) }
        ends = NewLookColors.stops(start)
        editingEnd = 0
        loadEnd()
    }

    private func loadEnd() {
        guard ends.indices.contains(editingEnd) else { return }
        let (h, s, b) = NewLookColors.hsb(hex: ends[editingEnd])
        hue = h
        saturation = s
        brightness = b
        hexText = ends[editingEnd]
        hexInvalid = false
    }

    /// The wheel or the slider moved: the colour being edited follows.
    private func commitWheel() {
        guard ends.indices.contains(editingEnd) else { return }
        let hex = NewLookColors.hex(h: hue, s: saturation, b: brightness)
        guard hex != ends[editingEnd] else { return }
        // Converting back and forth can wobble by one step; only a real move changes the colour.
        let (h0, s0, b0) = NewLookColors.hsb(hex: ends[editingEnd])
        if abs(h0 - hue) < 0.002 && abs(s0 - saturation) < 0.002 && abs(b0 - brightness) < 0.002 { return }
        ends[editingEnd] = hex
        if !hexFocused { hexText = hex }
        savedFlash = false
    }

    private func applyHex() {
        guard let clean = NewLookColors.validated(hexText), !clean.contains("-") else {
            hexInvalid = true
            return
        }
        hexInvalid = false
        guard ends.indices.contains(editingEnd), clean != ends[editingEnd] else { return }
        ends[editingEnd] = clean
        let (h, s, b) = NewLookColors.hsb(hex: clean)
        hue = h
        saturation = s
        brightness = b
        savedFlash = false
    }

    private func resetToDefault() {
        let def = NewLookColors.validated(defaultValue) ?? "FAC442"
        ends = allowsGradient ? NewLookColors.stops(def) : [NewLookColors.middle(def)]
        editingEnd = 0
        loadEnd()
        savedFlash = false
    }
}

/// The wheel itself: hue round the rim, from grey in the middle to full colour at the edge,
/// dimmed by the brightness slider. Drag anywhere on it.
struct ThemeWheel: View {
    @Binding var hue: Double
    @Binding var saturation: Double
    let brightness: Double
    /// Where the default colour sits, for the house mark.
    let home: (Double, Double, Double)
    let onHome: () -> Void

    var body: some View {
        GeometryReader { geo in
            let size = min(geo.size.width, geo.size.height)
            let r = size / 2
            let center = CGPoint(x: geo.size.width / 2, y: geo.size.height / 2)
            ZStack {
                Circle()
                    .fill(AngularGradient(
                        colors: stride(from: 0.0, through: 1.0, by: 1.0 / 12).map {
                            Color(hue: $0, saturation: 1, brightness: 1)
                        },
                        center: .center))
                Circle()
                    .fill(RadialGradient(colors: [.white, .white.opacity(0)], center: .center,
                                         startRadius: 0, endRadius: r))
                Circle()
                    .fill(Color.black.opacity(1 - brightness))

                // The house mark at the default colour. Tap it to go back to the default.
                Image(systemName: "house.fill")
                    .font(.system(size: 11, weight: .bold))
                    .foregroundStyle(.black)
                    .frame(width: 24, height: 24)
                    .background(Color.white.opacity(0.9), in: Circle())
                    .position(point(hue: home.0, saturation: home.1, r: r - 4, center: center))
                    .onTapGesture { onHome() }
                    .accessibilityLabel("Back to the default colour")
                    .accessibilityAddTraits(.isButton)

                // The handle.
                Circle()
                    .fill(Color(hue: hue, saturation: saturation, brightness: brightness))
                    .frame(width: 30, height: 30)
                    .overlay(Circle().strokeBorder(Color.white, lineWidth: 3))
                    .shadow(color: .black.opacity(0.5), radius: 3)
                    .position(point(hue: hue, saturation: saturation, r: r, center: center))
                    .allowsHitTesting(false)
            }
            .frame(width: geo.size.width, height: geo.size.height)
            .contentShape(Circle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { drag in
                        let dx = drag.location.x - center.x
                        let dy = drag.location.y - center.y
                        var angle = atan2(Double(dy), Double(dx)) / (2 * .pi)
                        if angle < 0 { angle += 1 }
                        hue = angle
                        saturation = min(Double(hypot(dx, dy) / r), 1)
                    }
            )
            .accessibilityElement(children: .contain)
            .accessibilityLabel("Colour wheel")
        }
    }

    private func point(hue: Double, saturation: Double, r: CGFloat, center: CGPoint) -> CGPoint {
        let angle = hue * 2 * .pi
        let distance = CGFloat(saturation) * r
        return CGPoint(x: center.x + CGFloat(cos(angle)) * distance,
                       y: center.y + CGFloat(sin(angle)) * distance)
    }
}

/// Brightness, dark to full, with a small triangle under the track to drag.
struct ThemeBrightnessSlider: View {
    @Binding var value: Double
    let hue: Double
    let saturation: Double

    var body: some View {
        GeometryReader { geo in
            let width = geo.size.width
            ZStack(alignment: .topLeading) {
                Capsule()
                    .fill(LinearGradient(colors: [.black, Color(hue: hue, saturation: saturation, brightness: 1)],
                                         startPoint: .leading, endPoint: .trailing))
                    .overlay(Capsule().strokeBorder(Color.white.opacity(0.2), lineWidth: 1))
                    .frame(height: 22)
                ThemeTriangle()
                    .fill(Color.white)
                    .frame(width: 16, height: 12)
                    .offset(x: CGFloat(value) * (width - 16), y: 26)
            }
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { drag in
                        value = min(max(Double((drag.location.x - 8) / max(width - 16, 1)), 0), 1)
                    }
            )
        }
        .accessibilityElement()
        .accessibilityLabel("Brightness")
        .accessibilityValue("\(Int((value * 100).rounded())) percent")
        .accessibilityAdjustableAction { direction in
            switch direction {
            case .increment: value = min(value + 0.05, 1)
            case .decrement: value = max(value - 0.05, 0)
            @unknown default: break
            }
        }
    }
}

/// Points up.
struct ThemeTriangle: Shape {
    func path(in rect: CGRect) -> Path {
        var path = Path()
        path.move(to: CGPoint(x: rect.midX, y: rect.minY))
        path.addLine(to: CGPoint(x: rect.maxX, y: rect.maxY))
        path.addLine(to: CGPoint(x: rect.minX, y: rect.maxY))
        path.closeSubpath()
        return path
    }
}

// MARK: - Box art style for the New look's cards

enum NewLookCoverStyle {
    /// The corner radius for a cover this wide: Theme, Box art, Corners. 0% is square, 100% very
    /// round; the default is about what the cards always had.
    static func radius(width: CGFloat) -> CGFloat {
        CGFloat(NewLookColors.shared.coverCorner) * 0.18 * max(width, 40)
    }
}

// MARK: - The moving background

/// Theme, Background, "Moving gradient": a slow dark gradient from the theme's colours that
/// drifts all the time. A mesh gradient on iOS 18 and later, soft moving glows before that. It
/// holds still with Reduce Motion on. The library (and so this) is not on screen while a game
/// runs, so it never costs anything during play; it also draws at most 20 times a second.
struct NewLookMovingBackdrop: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let colors: [Color]

    var body: some View {
        if reduceMotion {
            frame(t: 0)
        } else {
            TimelineView(.animation(minimumInterval: 1.0 / 20)) { context in
                frame(t: context.date.timeIntervalSinceReferenceDate)
            }
        }
    }

    @ViewBuilder
    private func frame(t: Double) -> some View {
        let base = colors.first ?? .black
        let a = colors.count > 1 ? colors[1] : base
        let b = colors.count > 2 ? colors[2] : a
        let c = colors.count > 3 ? colors[3] : b
        ZStack {
            if #available(iOS 18.0, *) {
                MeshGradient(
                    width: 3, height: 3,
                    points: meshPoints(t),
                    colors: [base, a, base,
                             c, b, a,
                             base, c, base],
                    smoothsColors: true
                )
            } else {
                base
                glow(a, t: t, speed: 0.050, phase: 0.0)
                glow(b, t: t, speed: 0.037, phase: 2.1)
                glow(c, t: t, speed: 0.043, phase: 4.2)
            }
            // A dark wash over it, so white text always reads.
            base.opacity(0.30)
        }
    }

    private func meshPoints(_ t: Double) -> [SIMD2<Float>] {
        func wave(_ speed: Double, _ phase: Double, _ amount: Double) -> Float {
            Float(sin(t * speed + phase) * amount)
        }
        func p(_ x: Float, _ y: Float) -> SIMD2<Float> { SIMD2<Float>(x, y) }
        let top = 0.5 + wave(0.21, 0.0, 0.25)
        let left = 0.5 + wave(0.17, 1.3, 0.25)
        let midX = 0.5 + wave(0.13, 2.0, 0.22)
        let midY = 0.5 + wave(0.19, 0.7, 0.22)
        let right = 0.5 + wave(0.15, 3.1, 0.25)
        let bottom = 0.5 + wave(0.18, 4.4, 0.25)
        return [p(0, 0), p(top, 0), p(1, 0),
                p(0, left), p(midX, midY), p(1, right),
                p(0, 1), p(bottom, 1), p(1, 1)]
    }

    private func glow(_ color: Color, t: Double, speed: Double, phase: Double) -> some View {
        RadialGradient(
            colors: [color, color.opacity(0)],
            center: UnitPoint(x: 0.5 + 0.38 * sin(t * speed * 2 * .pi + phase),
                              y: 0.5 + 0.38 * cos(t * speed * 1.6 * .pi + phase * 0.7)),
            startRadius: 0, endRadius: 420
        )
    }
}
