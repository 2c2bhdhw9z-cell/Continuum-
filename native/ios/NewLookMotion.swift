// Everything that moves in the New look, in one place.
//
// The owner's brief (22 October 2026): the New look is the Home mockup
// (.kiro/notes/docs/new-look/mockup-home.jpg), and its background is COLOURS THAT SWIRL AND MIX
// TOGETHER, like Google's Gemini app and Manic EMU (NewLookSwirl): smooth, rich, slow, never
// blotchy. The light running round the main buttons stays. Nothing decorative the mockup does not
// have (no sparks, no shine sweeping over titles, no sparkle mark: the owner rejected those).
//
// EVERYTHING HERE IS THE PLAYER'S CHOICE (owner: "always customisable"), in Settings, LOOK: the
// background's style (swirl, glow or plain), its colours, speed and strength, whether it moves;
// the light round buttons and its speed; the glow behind the box art; the box floating. Every
// colour comes from `NewLookColors`. iOS Settings, Accessibility, Reduce Motion holds ALL of it
// still.
//
// Built only from TimelineView, Canvas and ordinary SwiftUI animation, which all run on iOS 16,
// the app's deployment target. Nothing here runs during a game: the library (and with it every
// page this decorates) is off screen then. Only SwiftUI, UIKit and NewLookColors.swift are used,
// so this file also compiles into the simulator preview (tools/newlook-preview).

import SwiftUI

/// The fraction part of a number, 0 up to 1.
@inline(__always) func newLookFrac(_ value: Double) -> Double { value - value.rounded(.down) }

// MARK: - The clock

/// Hands its content the time in seconds, about `fps` times a second. With Reduce Motion on, or
/// `running` false, it hands over one fixed time, once, so whatever it draws holds still.
struct NewLookClock<Content: View>: View {
    var fps: Double = 30
    /// The moment shown while things hold still.
    var stillAt: Double = 0
    var running = true
    @ViewBuilder let content: (Double) -> Content
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        if reduceMotion || !running {
            content(stillAt)
        } else {
            TimelineView(.animation(minimumInterval: 1.0 / fps)) { context in
                // Kept small so the sines below never lose precision.
                content(context.date.timeIntervalSince1970.truncatingRemainder(dividingBy: 100_000))
            }
        }
    }
}

// MARK: - The swirl

/// Colours that swirl and mix behind the page, like Gemini's: a mesh of the swirl colours whose
/// inner points turn slowly round the middle and wander, so the colours flow into one another and
/// never visibly repeat, over the background colour. On iOS 18 and later it is Apple's own
/// MeshGradient (smooth, drawn by the GPU); before that, soft shapes of the same colours blurred
/// into each other.
struct NewLookSwirl: View {
    var moving = true
    /// 1 on the library pages; a sheet uses a little less.
    var strength: Double = 1
    @ObservedObject private var colors = NewLookColors.shared

    var body: some View {
        let speed = colors.swirlSpeed
        NewLookClock(fps: 30, stillAt: 37, running: moving) { t in
            let time = t * speed
            if #available(iOS 18.0, *) {
                MeshGradient(width: 4, height: 4, points: Self.points(time), colors: meshColors(time),
                             background: Self.color(colors.backgroundRGB), smoothsColors: true)
            } else {
                blobs(time)
            }
        }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }

    /// The mesh's 16 points. The corners stay put, the edge points slide along their edges, and the
    /// four inner points turn together round the middle (the swirl) while each wanders on its own.
    static func points(_ t: Double) -> [SIMD2<Float>] {
        let turn = t * 2 * Double.pi / 26
        var out: [SIMD2<Float>] = []
        for row in 0..<4 {
            for col in 0..<4 {
                var x = Double(col) / 3
                var y = Double(row) / 3
                let i = Double(row * 4 + col)
                let edgeX = col == 0 || col == 3
                let edgeY = row == 0 || row == 3
                if edgeX && edgeY {
                    // A corner.
                } else if edgeY {
                    x += 0.10 * sin(t * 0.21 + i * 1.7)
                } else if edgeX {
                    y += 0.10 * sin(t * 0.19 + i * 1.3)
                } else {
                    let dx = x - 0.5, dy = y - 0.5
                    let wobble = 1 + 0.35 * sin(t * 0.17 + i)
                    x = 0.5 + (dx * cos(turn) - dy * sin(turn)) * wobble + 0.06 * sin(t * 0.31 + i * 2.1)
                    y = 0.5 + (dx * sin(turn) + dy * cos(turn)) * wobble + 0.06 * cos(t * 0.27 + i * 1.6)
                }
                out.append(SIMD2<Float>(Float(min(max(x, 0), 1)), Float(min(max(y, 0), 1))))
            }
        }
        return out
    }

    /// The mesh's 16 colours: the swirl colours drifting from point to point over time, strongest
    /// in the middle and softer at the edges, each mixed into the background by the strength.
    private func meshColors(_ t: Double) -> [Color] {
        let palette = colors.swirlRGB
        let base = colors.backgroundRGB
        let k = colors.swirlStrength * strength
        var out: [Color] = []
        for row in 0..<4 {
            for col in 0..<4 {
                let i = row * 4 + col
                let edgeX = col == 0 || col == 3
                let edgeY = row == 0 || row == 3
                let weight = edgeX && edgeY ? 0.45 : (edgeX || edgeY ? 0.75 : 1.0)
                // Where along the palette this point is: each point drifts at its own pace.
                let phase = t * (0.045 + 0.012 * Double(i % 5)) + Double(i) * 0.61
                let a = palette[Int(phase.rounded(.down)) % palette.count]
                let b = palette[(Int(phase.rounded(.down)) + 1) % palette.count]
                let f = newLookFrac(phase)
                let smooth = f * f * (3 - 2 * f)
                let c = a + (b - a) * smooth
                out.append(Self.color(base + (c - base) * min(k * weight, 1)))
            }
        }
        return out
    }

    /// Before iOS 18: soft shapes of the swirl colours turning round the middle, blurred together.
    private func blobs(_ t: Double) -> some View {
        let palette = colors.swirlRGB
        let k = colors.swirlStrength * strength
        return GeometryReader { geo in
            let size = geo.size
            Canvas(rendersAsynchronously: true) { context, canvas in
                let w = Double(canvas.width), h = Double(canvas.height)
                context.addFilter(.blur(radius: CGFloat(max(w, h) * 0.16)))
                let turn = t * 2 * Double.pi / 26
                for i in 0..<6 {
                    let fi = Double(i)
                    let angle = turn + fi * 1.047
                    let r = 0.28 + 0.08 * sin(t * 0.2 + fi)
                    let x = w * (0.5 + r * cos(angle) + 0.08 * sin(t * 0.3 + fi * 2))
                    let y = h * (0.5 + r * 1.2 * sin(angle) + 0.06 * cos(t * 0.25 + fi))
                    let radius = max(w, h) * (0.26 + 0.05 * sin(t * 0.23 + fi * 1.3))
                    let c = palette[i % palette.count]
                    context.fill(Path(ellipseIn: CGRect(x: x - radius, y: y - radius,
                                                        width: 2 * radius, height: 2 * radius)),
                                 with: .color(Self.color(c).opacity(min(k, 1))))
                }
            }
            .frame(width: max(size.width / 4, 1), height: max(size.height / 4, 1))
            .scaleEffect(4, anchor: .topLeading)
            .frame(width: size.width, height: size.height, alignment: .topLeading)
        }
        .background(Self.color(colors.backgroundRGB))
        .clipped()
    }

    static func color(_ v: SIMD3<Double>) -> Color { Color(red: v.x, green: v.y, blue: v.z) }
}

// MARK: - The page glow

/// The light behind every New-look page: the charcoal of the mockup with a soft glow of the
/// accent colour coming from the top, where the mockup's hero sits, and (while it moves) a fainter
/// one travelling along the bottom edge behind the tab bar, bright at its leading edge and fading
/// out behind, the way Gemini's glow moves. The middle of the page stays dark, so text and covers
/// always read. Drawn at a third of the screen's resolution and scaled up: it is nothing but soft
/// gradients, so nothing is lost and it costs a fraction.
struct NewLookAura: View {
    /// False holds one still picture (Settings, LOOK, "Moving glow" off).
    var moving = true
    /// 1 on the library pages; a sheet uses a little less.
    var strength: Double = 1
    @ObservedObject private var colors = NewLookColors.shared

    var body: some View {
        GeometryReader { geo in
            let size = geo.size
            NewLookClock(fps: 24, stillAt: 12, running: moving) { t in
                Canvas(rendersAsynchronously: true) { context, canvasSize in
                    draw(&context, size: canvasSize, t: t)
                }
                .frame(width: max(size.width / 3, 1), height: max(size.height / 3, 1))
                .scaleEffect(3, anchor: .topLeading)
            }
            .frame(width: size.width, height: size.height, alignment: .topLeading)
        }
        .background(colors.background)
        .clipped()
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }

    private func draw(_ context: inout GraphicsContext, size: CGSize, t: Double) {
        guard size.width > 0, size.height > 0 else { return }
        let stops = colors.accentStops
        let first = stops.first ?? colors.accent
        let second = stops.count > 1 ? stops[1] : first
        let w = Double(size.width)
        let h = Double(size.height)
        let tau = 2 * Double.pi

        func glow(x: Double, y: Double, radius: Double, squash: Double, color: Color, alpha: Double) {
            var layer = context
            layer.translateBy(x: CGFloat(x), y: CGFloat(y))
            layer.scaleBy(x: 1, y: CGFloat(squash))
            let r = CGFloat(radius)
            let a = alpha * strength
            layer.fill(
                Path(ellipseIn: CGRect(x: -r, y: -r, width: 2 * r, height: 2 * r)),
                with: .radialGradient(
                    Gradient(stops: [
                        .init(color: color.opacity(a), location: 0),
                        .init(color: color.opacity(a * 0.45), location: 0.45),
                        .init(color: color.opacity(0), location: 1),
                    ]),
                    center: .zero, startRadius: 0, endRadius: r)
            )
        }

        // The top glow, round where the hero's box art stands (the mockup's warm light):
        // breathes over about 7 seconds and drifts a little sideways over about 19.
        let breath = 0.5 + 0.5 * sin(t * tau / 7)
        glow(x: w * (0.72 + 0.05 * sin(t * tau / 19)),
             y: h * (0.22 + 0.015 * sin(t * tau / 23)),
             radius: w * (0.66 + 0.04 * breath), squash: 0.8,
             color: second, alpha: 0.15 + 0.05 * breath)
        // A faint one from the other side, so the light is never a single spot.
        glow(x: w * (0.25 - 0.06 * sin(t * tau / 29)), y: h * 0.06,
             radius: w * 0.55, squash: 0.6, color: first, alpha: 0.04 + 0.02 * (1 - breath))

        // Along the bottom, only while it moves: a glow that travels left to right every 14
        // seconds, fading in at the left and out at the right, over a faint steady one.
        if moving {
            glow(x: w * 0.5, y: h * 1.03, radius: w * 0.75, squash: 0.30,
                 color: second, alpha: 0.10)
            let p = newLookFrac(t / 14)
            glow(x: w * (-0.25 + 1.5 * p), y: h * 1.0, radius: w * 0.42, squash: 0.36,
                 color: first, alpha: 0.20 * sin(p * .pi))
        }
    }
}

/// The warm glow standing behind a hero's box art (Home, the Game Hub, the pause menu), as in the
/// mockup: strongest right behind the box, fading out smoothly. It breathes slowly; with Reduce
/// Motion it holds still. Only its opacity and size animate, so it costs next to nothing.
/// Settings, LOOK, "Glow behind the box" turns it off.
struct NewLookHeroGlow: View {
    var size: CGFloat = 540
    @ObservedObject private var colors = NewLookColors.shared
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var lit = false

    var body: some View {
        // The accent's deeper colour: the mockup's light is a saturated gold, not a pale one.
        let c = colors.accentStops.last ?? colors.accent
        RadialGradient(
            gradient: Gradient(stops: [
                .init(color: c.opacity(0.85), location: 0),
                .init(color: c.opacity(0.45), location: 0.3),
                .init(color: c.opacity(0.16), location: 0.6),
                .init(color: c.opacity(0.06), location: 0.8),
                .init(color: c.opacity(0), location: 1),
            ]),
            center: .center, startRadius: 0, endRadius: size / 2
        )
        .frame(width: size, height: size)
        .opacity(colors.heroGlow ? (lit ? 1 : 0.82) : 0)
        .scaleEffect(lit ? 1.04 : 0.97)
        .allowsHitTesting(false)
        .accessibilityHidden(true)
        .onAppear {
            guard !reduceMotion else { return }
            withAnimation(.easeInOut(duration: 3.2).repeatForever(autoreverses: true)) {
                lit = true
            }
        }
    }
}

// MARK: - Float

/// A gentle, slow bob up and down: the hero's box art (Settings, LOOK, "Box art floats").
struct NewLookFloat: ViewModifier {
    var amplitude: CGFloat = 4
    var period: Double = 4.6
    @State private var up = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @ObservedObject private var colors = NewLookColors.shared

    func body(content: Content) -> some View {
        content
            .offset(y: colors.boxFloats ? (up ? -amplitude : amplitude) : 0)
            .onAppear {
                guard !reduceMotion, colors.boxFloats else { return }
                withAnimation(.easeInOut(duration: period / 2).repeatForever(autoreverses: true)) {
                    up = true
                }
            }
    }
}

// MARK: - Breathing glow

/// A glow that slowly swells and settles. For ONE thing on a page at a time (the picked console):
/// an animated shadow is redrawn every frame, so never put this on every card in a row.
struct NewLookBreathe: ViewModifier {
    let color: Color
    var radius: CGFloat = 10
    var low: Double = 0.35
    var high: Double = 0.8
    var period: Double = 3.0
    @State private var on = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func body(content: Content) -> some View {
        content
            .shadow(color: color.opacity(on ? high : low), radius: on ? radius * 1.3 : radius)
            .onAppear {
                guard !reduceMotion else { return }
                withAnimation(.easeInOut(duration: period / 2).repeatForever(autoreverses: true)) {
                    on = true
                }
            }
    }
}

// MARK: - Flowing border

/// A thin border in the accent's colours with a bright highlight running round it (Settings,
/// LOOK, "Light round buttons", and its speed).
struct NewLookFlowRim<S: InsettableShape>: View {
    let shape: S
    var lineWidth: CGFloat = 1.5
    var period: Double = 2.4
    @ObservedObject private var colors = NewLookColors.shared

    var body: some View {
        let stops = colors.accentStops
        let first = stops.first ?? colors.accent
        let second = stops.count > 1 ? stops[1] : first
        let period = self.period / max(colors.lightSpeed, 0.1)
        NewLookClock(fps: 30, stillAt: 0) { t in
            shape.strokeBorder(
                AngularGradient(
                    gradient: Gradient(stops: [
                        .init(color: first.opacity(0.75), location: 0),
                        .init(color: second.opacity(0.9), location: 0.35),
                        .init(color: .white.opacity(0.95), location: 0.5),
                        .init(color: first.opacity(0.9), location: 0.65),
                        .init(color: first.opacity(0.75), location: 1),
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
    /// The light running round a button, when Settings, LOOK, "Light round buttons" is on.
    func newLookButtonLight<S: InsettableShape>(_ shape: S, lineWidth: CGFloat = 1.4) -> some View {
        modifier(NewLookButtonLight(shape: shape, lineWidth: lineWidth))
    }
}

struct NewLookButtonLight<S: InsettableShape>: ViewModifier {
    let shape: S
    var lineWidth: CGFloat = 1.4
    @ObservedObject private var colors = NewLookColors.shared

    func body(content: Content) -> some View {
        content.overlay {
            if colors.buttonLight {
                NewLookFlowRim(shape: shape, lineWidth: lineWidth)
            }
        }
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
                    .fill(configuration.isOn ? AnyShapeStyle(NewLookColors.shared.accentFill)
                                             : AnyShapeStyle(Color.white.opacity(0.16)))
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
            .offset(y: still ? 0 : 14)
            .onAppear {
                guard enabled, !reduceMotion, !shown else { return }
                withAnimation(.spring(response: 0.5, dampingFraction: 0.85).delay(delay)) {
                    shown = true
                }
            }
    }
}

// MARK: - One even frame for a cover

/// A game card's cover frame, as in the mockup's Jump Back In row: one size for every card in a
/// row (from Settings, Card size, and Square covers and Corner roundness in LOOK), whatever shape
/// each cover is, with a bright rim and a soft glow in the console's colour. `cover` is drawn
/// whole and centred, scaled to fit; `backdrop` fills the frame behind it and is dimmed, so the
/// space a tall or wide cover leaves is a soft copy of the same art.
///
/// The glow is STILL, on purpose: a moving glow on every card in a row is redrawn every frame
/// for every card, which is what made the phone warm in builds 202 to 204.
struct NewLookCoverFrame<Backdrop: View, Cover: View>: View {
    let width: CGFloat
    let height: CGFloat
    let radius: CGFloat
    /// Nil: no glow, just a faint hairline.
    let glow: Color?
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
        .overlay(shape.strokeBorder(glow.map { $0.opacity(0.95) } ?? Color.white.opacity(0.14),
                                    lineWidth: glow == nil ? 0.5 : 2))
        .background {
            if let glow {
                // The soft glow: a blurred rim behind the card. Drawn once, not every frame.
                shape.stroke(glow.opacity(0.75), lineWidth: 5)
                    .blur(radius: 7)
            }
        }
    }
}
