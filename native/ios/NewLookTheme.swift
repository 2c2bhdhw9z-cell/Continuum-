// The New look's colour controls (Settings, LOOK, under the Current / New switch), and the
// moving background.
//
// Everything here only shows while the New look is on. The Current look is untouched.
// The settings live in `NewLookColors` (LookTheme.swift). Any colour is picked with Apple's own
// system colour picker (SwiftUI `ColorPicker`). Reference apps were looked at for what a player
// can change, never for how a screen is laid out: this is Continuum's own design, in the
// language of the owner's Home mockup (charcoal glass cards, the accent, bold titles, glow).

import SwiftUI
import UIKit

// MARK: - The colour panels

struct NewLookColorPanels: View {
    @ObservedObject private var colors = NewLookColors.shared
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            preview
            panel("Accent", icon: "paintpalette.fill",
                  blurb: "Buttons, the selected tab, switches, sliders and glows.") { accent }
            panel("Covers", icon: "rectangle.portrait.on.rectangle.portrait.fill",
                  blurb: "How game covers look on Home, in the Library and in Favorites.") { covers }
            panel("Ring", icon: "circle.dashed",
                  blurb: "The ring next to CONTINUUM at the top.") { ring }
            panel("Background", icon: "square.fill.on.square.fill",
                  blurb: "Behind Home, Library, Favorites and Settings.") { background }
            if !colors.isDefault {
                Button {
                    colors.resetToDefaults()
                } label: {
                    Label("Back to gold on charcoal", systemImage: "arrow.uturn.backward")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(.white)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 13)
                        .background(Color.white.opacity(0.07), in: Capsule())
                        .overlay(Capsule().strokeBorder(Color.white.opacity(0.12), lineWidth: 0.5))
                }
                .buttonStyle(.plain)
                .padding(.horizontal, 16)
            }
        }
    }

    // MARK: Live preview

    /// A small piece of Home in the chosen colours, so a pick shows at once right here.
    private var preview: some View {
        let shape = RoundedRectangle(cornerRadius: 24, style: .continuous)
        return HStack(spacing: 14) {
            VStack(alignment: .leading, spacing: 10) {
                Text("PREVIEW")
                    .font(.system(size: 12, weight: .bold))
                    .tracking(1.2)
                    .foregroundStyle(ShellPalette.gold)
                HStack(spacing: 6) {
                    Image(systemName: "play.fill")
                        .font(.system(size: 12, weight: .bold))
                    Text("Resume")
                        .font(.system(size: 14, weight: .semibold))
                }
                .foregroundStyle(ShellPalette.onAccent)
                .padding(.horizontal, 16)
                .padding(.vertical, 9)
                .background(colors.accentFill, in: Capsule())
                .shadow(color: colors.accent.opacity(0.4), radius: 8, y: 2)
                Toggle("", isOn: .constant(true))
                    .labelsHidden()
                    .tint(colors.accent)
                    .allowsHitTesting(false)
            }
            Spacer(minLength: 0)
            // A blank box with the glow and corners the covers will have.
            RoundedRectangle(cornerRadius: NewLookCoverStyle.radius(width: 64), style: .continuous)
                .fill(LinearGradient(colors: [Color(white: 0.22), Color(white: 0.12)],
                                     startPoint: .top, endPoint: .bottom))
                .frame(width: 64, height: colors.forceSquare ? 64 : 84)
                .overlay(Image(systemName: "gamecontroller.fill")
                            .foregroundStyle(Color.white.opacity(0.35)))
                .shadow(color: colors.glow(for: .gba) ?? .clear, radius: 1.2)
                .shadow(color: (colors.glow(for: .gba) ?? .clear).opacity(0.65), radius: 9)
            RoundedRectangle(cornerRadius: NewLookCoverStyle.radius(width: 64), style: .continuous)
                .fill(LinearGradient(colors: [Color(white: 0.22), Color(white: 0.12)],
                                     startPoint: .top, endPoint: .bottom))
                .frame(width: 64, height: colors.forceSquare ? 64 : 84)
                .overlay(Image(systemName: "gamecontroller.fill")
                            .foregroundStyle(Color.white.opacity(0.35)))
                .shadow(color: colors.glow(for: .snes) ?? .clear, radius: 1.2)
                .shadow(color: (colors.glow(for: .snes) ?? .clear).opacity(0.65), radius: 9)
        }
        .padding(18)
        .background(NewLookGlow().clipShape(shape))
        .overlay(shape.strokeBorder(Color.white.opacity(0.08), lineWidth: 0.5))
        .padding(.horizontal, 16)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Preview of the chosen colours")
    }

    // MARK: Accent

    private var accent: some View {
        VStack(alignment: .leading, spacing: 14) {
            swatchRow {
                ForEach(NewLookColors.accentPresets, id: \.self) { value in
                    swatch(value, picked: isCurrentAccent(value)) {
                        colors.accentHex = value == NewLookColors.defaultAccent ? nil : value
                    }
                }
            }

            if !colors.savedSwatches.isEmpty {
                subhead("My colours")
                swatchRow {
                    ForEach(colors.savedSwatches, id: \.self) { value in
                        swatch(value, picked: isCurrentAccent(value)) {
                            colors.accentHex = value
                        }
                        .contextMenu {
                            Button(role: .destructive) {
                                colors.deleteSwatch(value)
                            } label: {
                                Label("Remove from my colours", systemImage: "trash")
                            }
                        }
                    }
                }
                hint("Press and hold one of your colours to remove it.")
            }

            divider
            ColorPicker(selection: accentBinding(0), supportsOpacity: false) {
                rowLabel("Any colour", detail: "Opens the full colour picker")
            }
            Toggle(isOn: blendBinding) {
                rowLabel("Blend two colours", detail: "A gradient from one colour to another")
            }
            if accentStops.count == 2 {
                ColorPicker(selection: accentBinding(1), supportsOpacity: false) {
                    rowLabel("Second colour", detail: nil)
                }
            }
            Button {
                colors.saveSwatch(colors.accentHex ?? NewLookColors.defaultAccent)
            } label: {
                Label(isSaved ? "In my colours" : "Add to my colours",
                      systemImage: isSaved ? "checkmark.circle.fill" : "plus.circle.fill")
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(isSaved ? Color.white.opacity(0.6) : ShellPalette.gold)
            }
            .buttonStyle(.plain)
            .disabled(isSaved)
        }
    }

    private var accentStops: [String] { NewLookColors.stops(colors.accentHex ?? NewLookColors.defaultAccent) }

    private var isSaved: Bool {
        let current = colors.accentHex ?? NewLookColors.defaultAccent
        return colors.savedSwatches.contains { $0.caseInsensitiveCompare(current) == .orderedSame }
            || NewLookColors.accentPresets.contains { $0.caseInsensitiveCompare(current) == .orderedSame }
    }

    private func isCurrentAccent(_ value: String) -> Bool {
        value.caseInsensitiveCompare(colors.accentHex ?? NewLookColors.defaultAccent) == .orderedSame
    }

    /// One end of the accent, for the system colour picker. Setting it keeps the other end.
    private func accentBinding(_ index: Int) -> Binding<Color> {
        Binding(
            get: {
                let stops = accentStops
                return NewLookColors.color(hex: stops.indices.contains(index) ? stops[index] : stops[0])
            },
            set: { color in
                guard let hex = NewLookColors.hex(from: color) else { return }
                var stops = accentStops
                guard stops.indices.contains(index), stops[index] != hex else { return }
                stops[index] = hex
                colors.accentHex = stops.joined(separator: "-")
            }
        )
    }

    private var blendBinding: Binding<Bool> {
        Binding(
            get: { accentStops.count == 2 },
            set: { on in
                let stops = accentStops
                if on, stops.count == 1 {
                    colors.accentHex = stops[0] + "-" + NewLookColors.shiftedHue(stops[0], by: 0.08)
                } else if !on, stops.count == 2 {
                    colors.accentHex = NewLookColors.middle(stops.joined(separator: "-"))
                }
            }
        )
    }

    // MARK: Covers

    private var covers: some View {
        VStack(alignment: .leading, spacing: 14) {
            subhead("Glow round each cover")
            chips(NewLookColors.GlowMode.allCases, current: colors.glowMode, title: { $0.title }) {
                colors.glowMode = $0
            }
            if colors.glowMode == .custom {
                ColorPicker(selection: singleBinding(\.glowHex), supportsOpacity: false) {
                    rowLabel("Glow colour", detail: nil)
                }
            }
            divider
            VStack(alignment: .leading, spacing: 6) {
                HStack {
                    rowLabel("Corner roundness", detail: nil)
                    Spacer(minLength: 0)
                    Text("\(Int((colors.coverCorner * 100).rounded()))%")
                        .font(.system(size: 14, weight: .semibold).monospacedDigit())
                        .foregroundStyle(Color.white.opacity(0.65))
                }
                Slider(value: $colors.coverCorner, in: 0...1, step: 0.05)
                    .tint(colors.accent)
            }
            Toggle(isOn: $colors.forceSquare) {
                rowLabel("Square covers", detail: "Each cover sits whole inside a square")
            }
            Toggle(isOn: $colors.hideTitles) {
                rowLabel("Hide titles", detail: "Covers only, no names under them")
            }
        }
    }

    // MARK: Ring

    private var ring: some View {
        VStack(alignment: .leading, spacing: 14) {
            chips(NewLookColors.RingMode.allCases, current: colors.ringMode, title: { $0.title }) {
                colors.ringMode = $0
            }
            if colors.ringMode == .custom {
                ColorPicker(selection: singleBinding(\.ringHex), supportsOpacity: false) {
                    rowLabel("Ring colour", detail: nil)
                }
            }
            Toggle(isOn: $colors.ringSpins) {
                rowLabel("Spin", detail: nil)
            }
        }
    }

    // MARK: Background

    private var background: some View {
        VStack(alignment: .leading, spacing: 14) {
            swatchRow {
                ForEach(NewLookColors.backgroundPresets, id: \.self) { value in
                    swatch(value, picked: value.caseInsensitiveCompare(
                        colors.backgroundHex ?? NewLookColors.defaultBackground) == .orderedSame) {
                        colors.backgroundHex = value == NewLookColors.defaultBackground ? nil : value
                    }
                }
            }
            ColorPicker(selection: backgroundBinding, supportsOpacity: false) {
                rowLabel("Any colour", detail: "Bright colours are dimmed so words stay readable")
            }
            divider
            Toggle(isOn: $colors.backgroundMoves) {
                rowLabel("Moving gradient", detail: "Soft glows in your accent colours drift slowly behind everything")
            }
            if colors.backgroundMoves && reduceMotion {
                Label("Reduce Motion is on in iOS Settings, so the gradient is keeping still.",
                      systemImage: "pause.circle.fill")
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(ShellPalette.gold)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private var backgroundBinding: Binding<Color> {
        Binding(
            get: { colors.background },
            set: { color in
                guard let hex = NewLookColors.hex(from: color) else { return }
                let dark = NewLookColors.darkened(hex: hex)
                let current = colors.backgroundHex ?? NewLookColors.defaultBackground
                guard dark != current else { return }
                colors.backgroundHex = dark == NewLookColors.defaultBackground ? nil : dark
            }
        )
    }

    /// The custom glow or ring colour, for the system colour picker.
    private func singleBinding(_ key: ReferenceWritableKeyPath<NewLookColors, String?>) -> Binding<Color> {
        Binding(
            get: { NewLookColors.color(hex: colors[keyPath: key] ?? NewLookColors.defaultSingle) },
            set: { color in
                guard let hex = NewLookColors.hex(from: color),
                      hex != colors[keyPath: key] else { return }
                colors[keyPath: key] = hex
            }
        )
    }

    // MARK: Pieces

    /// A glass card in the Home mockup's style: the accent icon tile, a bold title, a line
    /// saying what it changes, then its controls.
    private func panel<Content: View>(_ title: String, icon: String, blurb: String,
                                      @ViewBuilder content: () -> Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: 20, style: .continuous)
        return VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 12) {
                Image(systemName: icon)
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(ShellPalette.gold)
                    .frame(width: 34, height: 34)
                    .background(ShellPalette.gold.opacity(0.16),
                                in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                VStack(alignment: .leading, spacing: 2) {
                    Text(title)
                        .font(.system(size: 17, weight: .bold))
                        .foregroundStyle(.white)
                    Text(blurb)
                        .font(.system(size: 12))
                        .foregroundStyle(Color.white.opacity(0.55))
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            content()
        }
        .tint(colors.accent)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(16)
        .background(shape.fill(Color.white.opacity(0.055)))
        .overlay(shape.strokeBorder(Color.white.opacity(0.09), lineWidth: 0.5))
        .padding(.horizontal, 16)
    }

    private func rowLabel(_ title: String, detail: String?) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title)
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(.white)
            if let detail {
                Text(detail)
                    .font(.system(size: 12))
                    .foregroundStyle(Color.white.opacity(0.55))
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private func subhead(_ text: String) -> some View {
        Text(text)
            .font(.system(size: 13, weight: .semibold))
            .foregroundStyle(Color.white.opacity(0.7))
    }

    private func hint(_ text: String) -> some View {
        Text(text)
            .font(.system(size: 12))
            .foregroundStyle(Color.white.opacity(0.5))
    }

    private var divider: some View {
        Rectangle().fill(Color.white.opacity(0.08)).frame(height: 0.5)
    }

    private func swatchRow<Content: View>(@ViewBuilder content: () -> Content) -> some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 10) { content() }
                .padding(.vertical, 6)
                .padding(.horizontal, 4)
        }
    }

    /// A rounded-square tile of the colour (or its gradient). The picked one glows in its own
    /// colour and carries a tick.
    private func swatch(_ value: String, picked: Bool, pick: @escaping () -> Void) -> some View {
        let shape = RoundedRectangle(cornerRadius: 12, style: .continuous)
        return Button(action: pick) {
            ThemeSwatch(value: value, shape: shape)
                .frame(width: 44, height: 44)
                .overlay(shape.strokeBorder(picked ? Color.white.opacity(0.9) : Color.clear,
                                            lineWidth: 2))
                .overlay {
                    if picked {
                        Image(systemName: "checkmark")
                            .font(.system(size: 14, weight: .heavy))
                            .foregroundStyle(NewLookColors.isLight(hex: NewLookColors.middle(value))
                                             ? Color.black : Color.white)
                    }
                }
                .shadow(color: picked ? NewLookColors.color(hex: value).opacity(0.7) : .clear,
                        radius: 8)
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Colour \(value)")
        .accessibilityAddTraits(picked ? [.isButton, .isSelected] : .isButton)
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
            .padding(.vertical, 2)
        }
    }
}

/// A colour tile: one colour, or a gradient of two.
struct ThemeSwatch<S: InsettableShape>: View {
    let value: String
    let shape: S

    var body: some View {
        let stops = NewLookColors.stops(value).map(NewLookColors.color(hex:))
        shape
            .fill(LinearGradient(colors: stops.count == 1 ? [stops[0], stops[0]] : stops,
                                 startPoint: .topLeading, endPoint: .bottomTrailing))
            .overlay(shape.strokeBorder(Color.white.opacity(0.18), lineWidth: 1))
    }
}

// MARK: - Cover corners

enum NewLookCoverStyle {
    /// The corner radius for a cover this wide: Settings, LOOK, "Corner roundness". 0% is square,
    /// 100% very round; the default is about what the cards always had.
    static func radius(width: CGFloat) -> CGFloat {
        CGFloat(NewLookColors.shared.coverCorner) * 0.18 * max(width, 40)
    }
}

// MARK: - The moving background

/// Settings, LOOK, Background, "Moving gradient": large soft glows in the accent's colours that
/// drift, grow and shrink across the page all the time, each on its own slow path, so the
/// colours keep flowing into one another. Still only when iOS Reduce Motion is on. The library
/// (and so this) is not on screen while a game runs, so it costs nothing during play.
struct NewLookMovingBackdrop: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let base: Color
    let blobs: [Color]

    /// One glow's drift: seconds per lap across and down, and where on the path it starts.
    private struct Drift { let across: Double; let down: Double; let pulse: Double; let phase: Double }
    private static let drifts: [Drift] = [
        Drift(across: 11, down: 14, pulse: 9, phase: 0.0),
        Drift(across: 15, down: 10, pulse: 12, phase: 2.2),
        Drift(across: 13, down: 17, pulse: 10, phase: 4.1),
        Drift(across: 18, down: 12, pulse: 14, phase: 1.3),
    ]

    var body: some View {
        if reduceMotion {
            scene(t: 20)
        } else {
            TimelineView(.animation(minimumInterval: 1.0 / 30)) { context in
                scene(t: context.date.timeIntervalSinceReferenceDate)
            }
        }
    }

    private func scene(t: Double) -> some View {
        GeometryReader { geo in
            let w = geo.size.width
            let h = geo.size.height
            let size = max(w, h) * 0.95
            ZStack {
                base
                ForEach(Array(blobs.prefix(Self.drifts.count).enumerated()), id: \.offset) { item in
                    let p: Drift = Self.drifts[item.offset]
                    let x: Double = 0.5 + 0.45 * sin(2 * Double.pi * t / p.across + p.phase)
                    let y: Double = 0.5 + 0.42 * cos(2 * Double.pi * t / p.down + p.phase * 1.7)
                    let scale: Double = 0.8 + 0.3 * sin(2 * Double.pi * t / p.pulse + p.phase)
                    Circle()
                        .fill(RadialGradient(colors: [item.element, item.element.opacity(0.55),
                                                      item.element.opacity(0)],
                                             center: .center, startRadius: 0, endRadius: size / 2))
                        .frame(width: size, height: size)
                        .scaleEffect(scale)
                        .position(x: w * CGFloat(x), y: h * CGFloat(y))
                        .blendMode(.screen)
                }
                // A light dark wash, so white text always reads over the brightest spot.
                base.opacity(0.22)
            }
            .frame(width: w, height: h)
            .clipped()
            .drawingGroup()
        }
    }
}
