// Everything that moves in the New look, in one place.
//
// Continuum's own motion, in the language of the owner's Home mockup: a fluid field of soft colour
// behind the pages and inside the hero cards, covers that float with sparks rising behind them,
// a light sweep across titles, borders with a highlight running round them, breathing glows,
// springy presses, toggles and pickers. Every colour comes from the player's own New-look colours
// (`NewLookColors`), so a custom theme drives all of it.
//
// iOS Settings, Accessibility, Reduce Motion holds ALL of it still: the clock below stops at a
// fixed moment, and the repeating animations are never started.
//
// Built only from TimelineView, Canvas and ordinary SwiftUI animation, which all run on iOS 16,
// the app's deployment target, so nothing here needs a version check or a fallback. Nothing here
// runs during a game: the library (and with it every page this decorates) is off screen then.

import SwiftUI

/// The fraction part of a number, 0 up to 1.
@inline(__always) func newLookFrac(_ value: Double) -> Double { value - value.rounded(.down) }

// MARK: - The clock

/// Hands its content the time in seconds, about `fps` times a second. With Reduce Motion on it
/// hands over one fixed time, once, so whatever it draws holds still.
struct NewLookClock<Content: View>: View {
    var fps: Double = 30
    /// The moment shown while Reduce Motion holds things still.
    var stillAt: Double = 0
    @ViewBuilder let content: (Double) -> Content
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        if reduceMotion {
            content(stillAt)
        } else {
            TimelineView(.animation(minimumInterval: 1.0 / fps)) { context in
                // Kept small so the sines below never lose precision.
                content(context.date.timeIntervalSince1970.truncatingRemainder(dividingBy: 100_000))
            }
        }
    }
}

// MARK: - The fluid field

/// Soft masses of colour that drift, swirl, stretch, turn and melt into one another, each on its
/// own path at speeds that never line up, so the picture never visibly repeats. Their colours
/// drift between the theme's colours. Drawn at a fraction of the screen's resolution and scaled
/// up: it is nothing but soft gradients, so nothing is lost and it costs a fraction.
struct NewLookFluid: View {
    let palette: [SIMD3<Double>]
    let base: Color
    /// Roughly how many seconds one big swirl takes.
    var period: Double = 24
    var masses: Int = 10
    /// How strongly each mass shows, 0 to 1.
    var intensity: Double = 0.55
    var downscale: CGFloat = 3

    var body: some View {
        GeometryReader { geo in
            let size = geo.size
            NewLookClock(fps: 30, stillAt: 41) { t in
                Canvas(rendersAsynchronously: true) { context, canvasSize in
                    draw(&context, size: canvasSize, t: t)
                }
                .frame(width: max(size.width / downscale, 1), height: max(size.height / downscale, 1))
                .scaleEffect(downscale, anchor: .topLeading)
            }
            .frame(width: size.width, height: size.height, alignment: .topLeading)
        }
        .background(base)
        .clipped()
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }

    private func draw(_ context: inout GraphicsContext, size: CGSize, t: Double) {
        guard !palette.isEmpty, size.width > 0, size.height > 0 else { return }
        let span = Double(max(size.width, size.height))
        let w = 2 * Double.pi / period
        context.blendMode = .plusLighter
        for i in 0..<masses {
            let fi = Double(i)
            // Speeds from irrational steps, so no two masses ever fall back into step.
            let k1 = 1 + 0.6 * newLookFrac(fi * 0.618_034)
            let k2 = 1 + 0.6 * newLookFrac(fi * 0.414_214 + 0.3)
            let k3 = 1 + 0.6 * newLookFrac(fi * 0.732_051 + 0.6)
            let x: Double = 0.5 + 0.36 * sin(t * w * k1 + fi * 1.9) + 0.14 * sin(t * w * k2 * 2.2 + fi * 0.7)
            let y: Double = 0.52 + 0.34 * cos(t * w * k2 + fi * 2.4) + 0.14 * sin(t * w * k3 * 1.7 + fi)
            // Stretch and turn, so each mass changes shape as it moves: smoke, not balls.
            let sx: Double = max(1 + 0.55 * sin(t * w * k3 * 1.3 + fi * 2.1), 0.35)
            let sy: Double = max(1 + 0.55 * cos(t * w * k1 * 1.1 + fi * 1.3), 0.35)
            let angle: Double = t * w * 0.35 * (i.isMultiple(of: 2) ? 1 : -1) + fi
            let radius: Double = span * (0.20 + 0.11 * newLookFrac(fi * 0.381_966))
            // The colour drifts between two of the theme's colours.
            let mix: Double = 0.5 + 0.5 * sin(t * w * 0.6 * k2 + fi * 1.1)
            let a = palette[i % palette.count]
            let b = palette[(i + 1) % palette.count]
            let c = a + (b - a) * mix
            let color = Color(red: c.x, green: c.y, blue: c.z)

            var layer = context
            layer.translateBy(x: CGFloat(x) * size.width, y: CGFloat(y) * size.height)
            layer.rotate(by: .radians(angle))
            layer.scaleBy(x: CGFloat(sx), y: CGFloat(sy))
            let r = CGFloat(radius)
            layer.fill(
                Path(ellipseIn: CGRect(x: -r, y: -r, width: 2 * r, height: 2 * r)),
                with: .radialGradient(
                    Gradient(colors: [color.opacity(intensity), color.opacity(intensity * 0.45),
                                      color.opacity(0)]),
                    center: .zero, startRadius: 0, endRadius: r)
            )
        }
    }
}

// MARK: - Sparks

/// Tiny points of light rising and fading, each on its own short life.
struct NewLookSparks: View {
    let color: Color
    var count = 14

    var body: some View {
        NewLookClock(fps: 30, stillAt: 3) { t in
            Canvas { context, size in
                for i in 0..<count {
                    let fi = Double(i)
                    let life = 2.6 + 1.8 * newLookFrac(fi * 0.618)
                    let p = newLookFrac(t / life + newLookFrac(fi * 0.371))
                    let x = size.width * CGFloat(0.12 + 0.76 * newLookFrac(fi * 0.537))
                        + CGFloat(sin(t * 1.3 + fi) * 6)
                    let y = size.height * CGFloat(1 - p)
                    let alpha = sin(p * .pi) * 0.9
                    let s = CGFloat(1.2 + 1.6 * newLookFrac(fi * 0.29))
                    context.fill(Path(ellipseIn: CGRect(x: x - s * 3, y: y - s * 3, width: s * 6, height: s * 6)),
                                 with: .color(color.opacity(alpha * 0.18)))
                    context.fill(Path(ellipseIn: CGRect(x: x - s, y: y - s, width: s * 2, height: s * 2)),
                                 with: .color(color.opacity(alpha)))
                }
            }
        }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }
}

// MARK: - Float, with sparks behind

/// A gentle, slow bob up and down, with sparks rising behind.
struct NewLookFloat: ViewModifier {
    let sparkColor: Color
    var amplitude: CGFloat = 5
    var period: Double = 4.2
    @State private var up = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func body(content: Content) -> some View {
        content
            .offset(y: up ? -amplitude : amplitude)
            .background(
                NewLookSparks(color: sparkColor)
                    .padding(.horizontal, -18)
                    .padding(.vertical, -12)
            )
            .onAppear {
                guard !reduceMotion else { return }
                withAnimation(.easeInOut(duration: period / 2).repeatForever(autoreverses: true)) {
                    up = true
                }
            }
    }
}

// MARK: - Breathing glow

/// A glow that slowly swells and settles.
struct NewLookBreathe: ViewModifier {
    let color: Color
    var radius: CGFloat = 10
    var low: Double = 0.35
    var high: Double = 0.8
    var period: Double = 3.0
    var delay: Double = 0
    @State private var on = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func body(content: Content) -> some View {
        content
            .shadow(color: color.opacity(on ? high : low), radius: on ? radius * 1.35 : radius)
            .onAppear {
                guard !reduceMotion else { return }
                withAnimation(.easeInOut(duration: period / 2).repeatForever(autoreverses: true).delay(delay)) {
                    on = true
                }
            }
    }
}

// MARK: - Light sweep

/// A soft metallic band of light that passes left to right across the text now and then.
struct NewLookSweep: ViewModifier {
    var period: Double = 4.0
    /// Where in its cycle this one starts, 0 to 1, so titles on one page do not all flash at once.
    var phase: Double = 0
    /// False draws the text plain, for views drawn in both looks.
    var enabled = true

    @ViewBuilder
    func body(content: Content) -> some View {
        if enabled {
            sweeping(content)
        } else {
            content
        }
    }

    private func sweeping(_ content: Content) -> some View {
        content.overlay(
            NewLookClock(fps: 30, stillAt: 0) { t in
                GeometryReader { geo in
                    let width = geo.size.width
                    let band = max(width * 0.35, 44)
                    let p = newLookFrac(t / period + phase)
                    // Crosses in the first 40% of the cycle, then rests.
                    let travel = CGFloat(min(p / 0.4, 1))
                    // White titles cannot get any whiter, so the band is a metallic glint: the
                    // accent's light colour at its edges and pure white at its core, which reads
                    // as a bright sheen passing over the letters.
                    let tint = NewLookColors.shared.accentStops.first ?? .white
                    LinearGradient(colors: [tint.opacity(0), tint.opacity(0.85), .white,
                                            tint.opacity(0.85), tint.opacity(0)],
                                   startPoint: .leading, endPoint: .trailing)
                        .frame(width: band, height: geo.size.height)
                        .offset(x: -band + (width + band) * travel)
                        .opacity(p < 0.4 && t != 0 ? 1 : 0)
                }
            }
            .mask(content)
            .allowsHitTesting(false)
            .accessibilityHidden(true)
        )
    }
}

// MARK: - Flowing border

/// A thin border in the accent's colours with a bright highlight running round it.
struct NewLookFlowRim<S: InsettableShape>: View {
    let shape: S
    var lineWidth: CGFloat = 1.5
    var period: Double = 2.2
    @ObservedObject private var colors = NewLookColors.shared

    var body: some View {
        let stops = colors.accentStops
        let first = stops.first ?? colors.accent
        let second = stops.count > 1 ? stops[1] : first
        NewLookClock(fps: 30, stillAt: 0) { t in
            shape.strokeBorder(
                AngularGradient(
                    gradient: Gradient(stops: [
                        .init(color: first.opacity(0.35), location: 0),
                        .init(color: second.opacity(0.7), location: 0.35),
                        .init(color: .white.opacity(0.95), location: 0.5),
                        .init(color: first.opacity(0.7), location: 0.65),
                        .init(color: first.opacity(0.35), location: 1),
                    ]),
                    center: .center,
                    angle: .degrees(newLookFrac(t / period) * 360)
                ),
                lineWidth: lineWidth
            )
        }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }
}

extension View {
    func newLookFlowingBorder<S: InsettableShape>(_ shape: S, lineWidth: CGFloat = 1.5,
                                                 period: Double = 2.2) -> some View {
        overlay(NewLookFlowRim(shape: shape, lineWidth: lineWidth, period: period))
    }
}

// MARK: - Press

/// Scales down a little while pressed and springs back. `enabled` false is a plain button, for
/// views drawn in both looks.
struct NewLookPressStyle: ButtonStyle {
    var enabled = true
    var scale: CGFloat = 0.95

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .scaleEffect(enabled && configuration.isPressed ? scale : 1)
            .animation(.spring(response: 0.28, dampingFraction: 0.6), value: configuration.isPressed)
    }
}

/// The New look's press style for a button drawn in both looks; the plain style it always had
/// in the Current look.
struct NewLookButtons: ViewModifier {
    let enabled: Bool
    var scale: CGFloat = 0.95

    @ViewBuilder
    func body(content: Content) -> some View {
        if enabled {
            content.buttonStyle(NewLookPressStyle(scale: scale))
        } else {
            content.buttonStyle(.plain)
        }
    }
}

// MARK: - Toggles

/// The New look's switch: the track fills with the accent (its gradient, for a blend), and the
/// knob springs across.
struct NewLookToggleStyle: ToggleStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 10) {
            configuration.label
            Spacer(minLength: 8)
            ZStack(alignment: configuration.isOn ? .trailing : .leading) {
                Capsule(style: .continuous)
                    .fill(configuration.isOn ? ShellPalette.accentFill : AnyShapeStyle(Color.white.opacity(0.16)))
                    .frame(width: 51, height: 31)
                Circle()
                    .fill(Color.white)
                    .shadow(color: .black.opacity(0.3), radius: 2, y: 1)
                    .padding(2)
                    .frame(width: 31, height: 31)
            }
            .animation(.spring(response: 0.32, dampingFraction: 0.62), value: configuration.isOn)
            .contentShape(Capsule())
            .onTapGesture { configuration.isOn.toggle() }
        }
        .accessibilityRepresentation {
            Toggle(isOn: configuration.$isOn) { configuration.label }
        }
    }
}

/// The New look's switches for everything under it; nothing at all in the Current look.
struct NewLookToggles: ViewModifier {
    let enabled: Bool

    @ViewBuilder
    func body(content: Content) -> some View {
        if enabled {
            content.toggleStyle(NewLookToggleStyle())
        } else {
            content
        }
    }
}

// MARK: - Appearing

/// Content rises a little and fades in when it appears, on a spring. `enabled` false does
/// nothing, for views drawn in both looks.
struct NewLookAppear: ViewModifier {
    var enabled = true
    var delay: Double = 0
    @State private var shown = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func body(content: Content) -> some View {
        let still = !enabled || reduceMotion || shown
        return content
            .opacity(still ? 1 : 0)
            .offset(y: still ? 0 : 18)
            .onAppear {
                guard enabled, !reduceMotion, !shown else { return }
                withAnimation(.spring(response: 0.55, dampingFraction: 0.82).delay(delay)) {
                    shown = true
                }
            }
    }
}

// MARK: - The sparkle mark

/// Continuum's own sparkle: a four-pointed star with curved sides and a small one beside it.
struct NewLookSparkleShape: Shape {
    func path(in rect: CGRect) -> Path {
        var path = Path()
        let unit = min(rect.width, rect.height)
        star(&path, center: CGPoint(x: rect.minX + rect.width * 0.42, y: rect.minY + rect.height * 0.58),
             radius: unit * 0.42)
        star(&path, center: CGPoint(x: rect.minX + rect.width * 0.82, y: rect.minY + rect.height * 0.2),
             radius: unit * 0.16)
        return path
    }

    private func star(_ path: inout Path, center c: CGPoint, radius r: CGFloat) {
        let tips = [CGPoint(x: c.x, y: c.y - r), CGPoint(x: c.x + r, y: c.y),
                    CGPoint(x: c.x, y: c.y + r), CGPoint(x: c.x - r, y: c.y)]
        path.move(to: tips[0])
        for i in 0..<4 {
            let next = tips[(i + 1) % 4]
            let mid = CGPoint(x: (tips[i].x + next.x) / 2, y: (tips[i].y + next.y) / 2)
            // Pulled toward the centre, so the sides curve in.
            let control = CGPoint(x: c.x + (mid.x - c.x) * 0.22, y: c.y + (mid.y - c.y) * 0.22)
            path.addQuadCurve(to: next, control: control)
        }
        path.closeSubpath()
    }
}

/// The sparkle in the theme's colours: the shape holds still, the colours inside it turn. It
/// floats, with sparks rising behind it.
struct NewLookSparkleMark: View {
    var size: CGFloat = 64
    @ObservedObject private var colors = NewLookColors.shared

    var body: some View {
        let ring = colors.motionColors
        NewLookClock(fps: 30, stillAt: 0) { t in
            AngularGradient(colors: ring + [ring.first ?? .white],
                            center: UnitPoint(x: 0.42, y: 0.58),
                            angle: .degrees(newLookFrac(t / 2.5) * 360))
        }
        .mask(NewLookSparkleShape())
        .frame(width: size, height: size)
        .shadow(color: colors.accent.opacity(0.55), radius: 14)
        .modifier(NewLookFloat(sparkColor: colors.sparkColor))
        .accessibilityHidden(true)
    }
}

// MARK: - The tab bar's pill

/// The selected tab's pill: a faint gradient of the accent that slowly turns inside it, and a
/// soft glow that breathes.
struct NewLookTabPill: View {
    @ObservedObject private var colors = NewLookColors.shared

    var body: some View {
        let stops = colors.accentStops
        let first = stops.first ?? colors.accent
        let second = stops.count > 1 ? stops[1] : first
        NewLookClock(fps: 20, stillAt: 0) { t in
            let a = t * 2 * Double.pi / 12
            RoundedRectangle(cornerRadius: 18, style: .continuous)
                .fill(LinearGradient(
                    colors: [first.opacity(0.20), second.opacity(0.08), first.opacity(0.16)],
                    startPoint: UnitPoint(x: 0.5 + 0.5 * cos(a), y: 0.5 + 0.5 * sin(a)),
                    endPoint: UnitPoint(x: 0.5 - 0.5 * cos(a), y: 0.5 - 0.5 * sin(a))))
        }
        .modifier(NewLookBreathe(color: colors.accent, radius: 7, low: 0.10, high: 0.28, period: 3.6))
    }
}

// MARK: - One even frame for a cover

/// A game card's cover frame: one size for every card in a row (from Settings, Card size, and
/// Square covers and Corner roundness in LOOK), whatever shape each cover is. `cover` is drawn
/// whole and centred, scaled to fit; `backdrop` fills the frame behind it and is dimmed, so the
/// space a tall or wide cover leaves is a soft copy of the same art. The glow and outline follow
/// the frame, not the cover, and the soft glow breathes (`NewLookBreathe`).
struct NewLookCoverFrame<Backdrop: View, Cover: View>: View {
    let width: CGFloat
    let height: CGFloat
    let radius: CGFloat
    /// Nil: no glow, just a faint hairline.
    let glow: Color?
    var breathDelay: Double = 0
    @ViewBuilder var backdrop: () -> Backdrop
    @ViewBuilder var cover: () -> Cover

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)
        ZStack {
            backdrop()
                .frame(width: width, height: height)
                .clipped()
                .overlay(Color.black.opacity(0.42))
            cover()
                .frame(width: width, height: height)
        }
        .frame(width: width, height: height)
        .clipShape(shape)
        .overlay(shape.strokeBorder(glow.map { $0.opacity(0.75) } ?? Color.white.opacity(0.14),
                                    lineWidth: glow == nil ? 0.5 : 1))
        // A tight shadow of the glow colour that reads as a thin bright edge, then the soft
        // breathing one that is the glow.
        .shadow(color: glow ?? .clear, radius: 1.2)
        .modifier(NewLookBreathe(color: glow ?? .clear, radius: 9, low: 0.45, high: 0.9,
                                 period: 3.4, delay: breathDelay))
    }
}
