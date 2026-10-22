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

/// The page's colours as ribbons that swirl round each other, like Google's Gemini and Manic
/// EMU (the owner's recording: .kiro/notes/docs/new-look/reference-video-gemini-manic-frames.png).
///
/// THE OWNER'S BRIEF, after build 207 (22 October 2026): every colour stays visible on screen at
/// the same time, in tighter ribbon-like waves, with the dark background showing between them,
/// swirling round each other continuously, never one colour swallowing the whole screen and never
/// turning muddy where they overlap.
///
/// HOW:
/// - Each colour is two ribbons, half a page apart: soft wavy bands across the whole page with a
///   faint glow round them and the dark page between bands.
/// - The ribbons are spaced evenly across the page and all drift the same way, wrapping round, so
///   every colour is always on screen (measured in a prototype: each of three colours covers 9 to
///   21% of the screen in every frame; on the simulator the ribbons were then narrowed until the
///   dark between them stays well in view).
/// - Each ribbon leans its own way and swings slowly, so ribbons cross and weave; each waves on
///   its own rhythm. Where two cross, one passes over the other with a thin line of dark between
///   (the lower one is cut away round the upper), so two colours are never averaged into grey.
/// - Builds 206 and 207 were a single colour field (MeshGradient): it folded (206), then washed
///   one colour over the whole screen at a time and went grey where colours met (207).
/// Drawn at a third of the screen's resolution and scaled up: the ribbons are soft.
struct NewLookSwirl: View {
    var moving = true
    /// 1 on the library pages; a sheet uses a little less.
    var strength: Double = 1
    @ObservedObject private var colors = NewLookColors.shared

    var body: some View {
        let palette = colors.swirlRGB.map { Self.color($0) }
        let k = min(colors.swirlStrength * strength, 1)
        let speed = colors.swirlSpeed
        GeometryReader { geo in
            let size = geo.size
            NewLookClock(fps: 30, stillAt: 37, running: moving) { t in
                Canvas(rendersAsynchronously: true) { context, canvas in
                    Self.draw(&context, size: canvas, t: t * speed, palette: palette, k: k)
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

    static func draw(_ context: inout GraphicsContext, size: CGSize, t: Double,
                     palette: [Color], k: Double) {
        let n = palette.count
        guard n > 0, size.width > 0, size.height > 0 else { return }
        let w = Double(size.width), h = Double(size.height)
        // Two ribbons of every colour, half a page apart, so while one wraps round off the edge
        // the other is on screen: no colour ever leaves. (Rainbow had one of each, and in the
        // prototype each of its four colours vanished for seconds at a time.)
        let bands = n * 2
        let unit = min(w, h)
        let baseAngle = -0.6 + 0.5 * sin(t * 2 * Double.pi / 50)
        let cx = w / 2, cy = h / 2
        // Narrower and less blurred than the first prototype: on the phone the blur spreads
        // further, and at 0.17 / 0.075 the dark between ribbons fell to about 5% of the screen.
        // More ribbons (Rainbow's eight) are narrower still, so the dark stays about the same.
        let half0 = unit * 0.12 * min(1, 6 / Double(bands))
        let blur = unit * 0.045
        let margin = half0 * 1.15 + unit * 0.24 + 2.5 * blur
        let opacity = 0.35 + 0.6 * k
        let flow = t / 30
        let samples = 48

        // Each ribbon's outline, and a slightly wider one round it for the dark seam.
        var ribbons: [(path: Path, seam: Path, color: Color)] = []
        ribbons.reserveCapacity(bands)
        for j in 0..<bands {
            let fj = Double(j)
            // Its own lean, swinging slowly, so the ribbons weave across each other.
            let angle = baseAngle + 0.42 * sin(t * 0.07 + fj * 2.4)
            let dx = cos(angle), dy = sin(angle)
            let nx = -dy, ny = dx
            let across = abs(w * nx) + abs(h * ny)
            let along = abs(w * dx) + abs(h * dy)
            let span = across + 2 * margin
            // Evenly spaced across the page, drifting, wrapping round.
            let offset = (newLookFrac(fj / Double(bands) + flow) - 0.5) * span

            var left: [CGPoint] = [], right: [CGPoint] = []
            var seamLeft: [CGPoint] = [], seamRight: [CGPoint] = []
            for i in 0...samples {
                let s = Double(i) / Double(samples)
                let u = (s - 0.5) * (along + 2 * margin)
                let ph = 2 * Double.pi * s
                let wave = unit * (0.17 * sin(ph * 1.15 + t * (0.45 + 0.09 * fj) + fj * 1.7)
                                   + 0.07 * sin(ph * 2.4 - t * (0.33 + 0.06 * fj) + fj * 0.9))
                let bow = unit * 0.25 * sin(Double.pi * s) * sin(t * 0.09 + 0.5)
                let half = half0 * (0.7 + 0.45 * (0.5 + 0.5 * sin(ph * 1.9 + t * 0.35 + fj * 2.3)))
                let px = cx + dx * u + nx * (offset + wave + bow)
                let py = cy + dy * u + ny * (offset + wave + bow)
                left.append(CGPoint(x: px + nx * half, y: py + ny * half))
                right.append(CGPoint(x: px - nx * half, y: py - ny * half))
                let wide = half + blur
                seamLeft.append(CGPoint(x: px + nx * wide, y: py + ny * wide))
                seamRight.append(CGPoint(x: px - nx * wide, y: py - ny * wide))
            }
            ribbons.append((Self.outline(left, right), Self.outline(seamLeft, seamRight),
                            palette[j % n]))
        }

        // The faint glows go on the page itself, under every ribbon.
        for r in ribbons {
            var halo = context
            halo.addFilter(.blur(radius: CGFloat(blur * 2.0)))
            halo.fill(r.path, with: .color(r.color.opacity(opacity * 0.22)))
        }
        // The ribbons go on their own layer above. Before each is drawn, the ribbons already there
        // are softly cut away just round it, so where two cross, one passes cleanly over the other
        // with a thin line of dark page between: the two colours are never averaged (pink over
        // green went grey-mauve on the simulator). The cut only touches ribbons, never the page.
        context.drawLayer { layer in
            for r in ribbons {
                var seam = layer
                seam.blendMode = .destinationOut
                seam.addFilter(.blur(radius: CGFloat(blur * 0.6)))
                seam.fill(r.seam, with: .color(.black))
                var ribbon = layer
                ribbon.addFilter(.blur(radius: CGFloat(blur)))
                ribbon.fill(r.path, with: .color(r.color.opacity(opacity)))
            }
        }
    }

    /// A closed shape: along one edge, back along the other.
    private static func outline(_ left: [CGPoint], _ right: [CGPoint]) -> Path {
        var path = Path()
        guard let first = left.first else { return path }
        path.move(to: first)
        for point in left.dropFirst() { path.addLine(to: point) }
        for point in right.reversed() { path.addLine(to: point) }
        path.closeSubpath()
        return path
    }

    static func color(_ v: SIMD3<Double>) -> Color {
        Color(red: min(max(v.x, 0), 1), green: min(max(v.y, 0), 1), blue: min(max(v.z, 0), 1))
    }
}

/// A colour as hue, saturation and brightness (each 0 to 1), for mixing two colours round the
/// colour wheel the way the eye expects, instead of averaging red, green and blue (which turns
/// blue and yellow into grey).
struct NewLookHSV {
    var h: Double
    var s: Double
    var v: Double

    init(h: Double, s: Double, v: Double) {
        self.h = h
        self.s = s
        self.v = v
    }

    init(rgb c: SIMD3<Double>) {
        let mx = max(c.x, c.y, c.z), mn = min(c.x, c.y, c.z)
        let d = mx - mn
        v = mx
        s = mx > 0 ? d / mx : 0
        if d == 0 {
            h = 0
        } else if mx == c.x {
            h = newLookFrac(((c.y - c.z) / d) / 6 + 1)
        } else if mx == c.y {
            h = ((c.z - c.x) / d + 2) / 6
        } else {
            h = ((c.x - c.y) / d + 4) / 6
        }
    }

    var rgb: SIMD3<Double> {
        let hh = newLookFrac(h) * 6
        let sector = Int(hh.rounded(.down)) % 6
        let f = hh - hh.rounded(.down)
        let p = v * (1 - s), q = v * (1 - s * f), u = v * (1 - s * (1 - f))
        switch sector {
        case 0: return SIMD3(v, u, p)
        case 1: return SIMD3(q, v, p)
        case 2: return SIMD3(p, v, u)
        case 3: return SIMD3(p, q, v)
        case 4: return SIMD3(u, p, v)
        default: return SIMD3(v, p, q)
        }
    }

    /// `a` to `b` by `f` (0 to 1), the short way round the wheel. A grey (no saturation) takes
    /// the other colour's hue, so a colour fades into grey without passing through red.
    static func mix(_ a: NewLookHSV, _ b: NewLookHSV, _ f: Double) -> NewLookHSV {
        var ha = a.h, hb = b.h
        if a.s < 0.05 { ha = hb }
        if b.s < 0.05 { hb = ha }
        var d = hb - ha
        if d > 0.5 { d -= 1 }
        if d < -0.5 { d += 1 }
        return NewLookHSV(h: newLookFrac(ha + d * f + 1), s: a.s + (b.s - a.s) * f,
                          v: a.v + (b.v - a.v) * f)
    }
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
