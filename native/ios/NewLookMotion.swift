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

/// The page's background: the WHOLE screen in colour, the colours swirling round each other like
/// stirred paint, like Google's Gemini and Manic EMU (the owner's recording:
/// .kiro/notes/docs/new-look/reference-video-gemini-manic-frames.png).
///
/// THE OWNER, 22 October 2026: every colour on screen at the same time, swirling round each other
/// continuously, never one colour over the whole screen, never muddy or grey where they meet, and
/// NO GAPS: no dark showing between colours, no strips (build 208's ribbons: "all it is is
/// strips"). Before that: 206's MeshGradient folded into sharp sheets; 207 washed one colour over
/// the whole screen at a time and went grey where colours met.
///
/// HOW (tested in a prototype first, .scratch/rib/field4.py, and in the simulator preview):
/// - Each colour owns two soft clouds. All the clouds sit evenly round a slowly turning oval, so
///   the colours swirl round each other and each always has its own share of the screen
///   (measured: each of three colours is the strongest on 21 to 51% of it). Each cloud also
///   drifts in and out on its own rhythm, so they pass through the middle at different times.
/// - Two slow whirlpools wander over the page, turning opposite ways, and twist it; a gentle
///   wobble on top. The edges between colours curl like stirred paint, in different places.
/// - Colours mix the way the eye sees colour (Oklab), by brightness, richness and hue, so a mix
///   is always as rich as the colours that made it: never grey. The hue goes the way round the
///   colour wheel that stays among the player's colours, and round the oval the clouds follow the
///   colour wheel there and back, so only neighbours meet (purple into pink, purple into green).
/// - Worked out on a coarse grid every frame (about 3,600 points), drawn smoothly scaled up and
///   softened, at a third of the screen's resolution.
struct NewLookSwirl: View {
    var moving = true
    /// 1 on the library pages; a sheet uses a little less.
    var strength: Double = 1
    @ObservedObject private var colors = NewLookColors.shared

    var body: some View {
        let palette = NewLookSwirlPalette(colors.swirlRGB)
        let k = min(colors.swirlStrength * strength, 1)
        let speed = colors.swirlSpeed
        let base = colors.backgroundRGB
        GeometryReader { geo in
            let size = geo.size
            NewLookClock(fps: 30, stillAt: 37, running: moving) { t in
                Canvas(rendersAsynchronously: true) { context, canvas in
                    Self.draw(&context, size: canvas, t: t * speed, palette: palette, k: k, base: base)
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

    private static let sRGB = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()

    static func draw(_ context: inout GraphicsContext, size: CGSize, t: Double,
                     palette: NewLookSwirlPalette, k: Double, base: SIMD3<Double>) {
        guard size.width > 0, size.height > 0 else { return }
        let over = NewLookSwirlField.over
        let (pixels, width, height) = NewLookSwirlField.pixels(
            t: t, tall: Double(size.height / size.width), palette: palette, k: k, base: base)
        guard width > 0 else { return }
        guard let provider = CGDataProvider(data: Data(pixels) as CFData),
              let image = CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32,
                                  bytesPerRow: width * 4, space: sRGB,
                                  bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
                                  provider: provider, decode: nil, shouldInterpolate: true,
                                  intent: .defaultIntent)
        else { return }
        var soft = context
        soft.addFilter(.blur(radius: max(size.width / 44, 1)))
        let w = Double(size.width), h = Double(size.height)
        soft.draw(Image(decorative: image, scale: 1).interpolation(.high),
                  in: CGRect(x: -over * w, y: -over * h, width: (1 + 2 * over) * w,
                             height: (1 + 2 * over) * h))
    }

    static func color(_ v: SIMD3<Double>) -> Color {
        Color(red: min(max(v.x, 0), 1), green: min(max(v.y, 0), 1), blue: min(max(v.z, 0), 1))
    }
}

/// The swirl's colour maths, apart from the drawing so it can be checked on its own (plain
/// Foundation, no SwiftUI).
enum NewLookSwirlField {
    /// The grid runs this far past every edge (a fraction of the screen), so the softening
    /// never pulls in an edge.
    static let over = 0.08

    /// One frame of colour: a small picture (`width` by `height`, red, green, blue and one spare
    /// byte per point) covering the screen and `over` past each edge. `tall` is the screen's
    /// height over its width; positions below are in screen widths (x 0 to 1, y 0 to `tall`).
    static func pixels(t time: Double, tall: Double, palette: NewLookSwirlPalette, k: Double,
                       base: SIMD3<Double>) -> (pixels: [UInt8], width: Int, height: Int) {
        // The usual pace (Speed 1): on the simulator at 1x the colours barely moved in 10 seconds.
        let t = time * 1.5
        let n = palette.lightness.count
        guard n > 0, tall > 0 else { return ([], 0, 0) }
        let cols = 40
        let rows = max(Int((Double(cols) * tall).rounded()), 1)
        let sigma = 0.22, sharp = 1.8
        let spin = 0.08, wobble = 0.13
        let mix = 0.35 + 0.6 * k

        // Two whirlpools, turning opposite ways, wandering slowly.
        let whirls: [(x: Double, y: Double, turn: Double)] = (0..<2).map { v in
            let fv = Double(v)
            return (0.5 + 0.28 * sin(t * 0.051 + fv * 2.6),
                    tall * (0.5 + 0.28 * sin(t * 0.039 + fv * 1.9 + 0.8)),
                    (v == 0 ? 3.0 : -3.0) * (0.75 + 0.25 * sin(t * 0.11 + fv)))
        }
        // The clouds, evenly round a slowly turning oval, each drifting in and out.
        let cx = 0.5 + 0.05 * sin(t * 0.07), cy = tall * (0.5 + 0.04 * sin(t * 0.053 + 1))
        let m = palette.ring.count
        let clouds: [(x: Double, y: Double, colour: Int)] = palette.ring.enumerated().map { item in
            let fs = Double(item.offset), colour = item.element
            let angle = t * spin + 2 * Double.pi * fs / Double(m)
            let reach = 1 + wobble / 0.3 * sin(t * (0.13 + 0.02 * fs) + fs * 2.1)
            return (cx + 0.30 * reach * cos(angle), cy + 0.31 * tall * reach * sin(angle), colour)
        }
        let falloff = 1 / (2 * sigma * sigma)

        let width = cols + 1, height = rows + 1
        var pixels = [UInt8](repeating: 255, count: width * height * 4)
        var weight = [Double](repeating: 0, count: n)
        for j in 0..<height {
            let y0 = (-over + (1 + 2 * over) * Double(j) / Double(rows)) * tall
            for i in 0..<width {
                var qx = -over + (1 + 2 * over) * Double(i) / Double(cols)
                var qy = y0
                for whirl in whirls {
                    let dx = qx - whirl.x, dy = qy - whirl.y
                    let a = whirl.turn * exp(-(dx * dx + dy * dy) / 0.12)
                    let ca = cos(a), sa = sin(a)
                    qx = whirl.x + dx * ca - dy * sa
                    qy = whirl.y + dx * sa + dy * ca
                }
                let px = qx + 0.05 * sin(qy * 3.1 + t * 0.37) + 0.025 * sin(qy * 5.3 - t * 0.29 + 1.3)
                let py = qy + 0.05 * sin(qx * 2.7 - t * 0.31 + 2.1) + 0.025 * sin(qx * 4.9 + t * 0.23)
                for c in 0..<n { weight[c] = 0 }
                for cloud in clouds {
                    let dx = px - cloud.x, dy = py - cloud.y
                    weight[cloud.colour] += exp(-(dx * dx + dy * dy) * falloff)
                }
                var total = 0.0, l = 0.0, chroma = 0.0, hue = 0.0
                for c in 0..<n {
                    let v = pow(weight[c] + 1e-4, sharp)
                    total += v
                    l += v * palette.lightness[c]
                    chroma += v * palette.chroma[c]
                    hue += v * palette.hue[c]
                }
                l /= total; chroma /= total; hue /= total
                let rgb = NewLookOklab.rgb(SIMD3(l, chroma * cos(hue), chroma * sin(hue)))
                let out = base * (1 - mix) + rgb * mix
                let p = (j * width + i) * 4
                pixels[p] = UInt8(min(max(out.x, 0), 1) * 255 + 0.5)
                pixels[p + 1] = UInt8(min(max(out.y, 0), 1) * 255 + 0.5)
                pixels[p + 2] = UInt8(min(max(out.z, 0), 1) * 255 + 0.5)
            }
        }
        return (pixels, width, height)
    }
}

/// The swirl's colours, ready to mix: each one's lightness, richness (chroma) and hue in Oklab,
/// and the order of the clouds round the oval.
struct NewLookSwirlPalette {
    var lightness: [Double] = []
    var chroma: [Double] = []
    /// In radians, laid out so that mixing two never crosses the widest empty stretch of the
    /// colour wheel: a mix stays among the player's colours (pink and green meet through purple
    /// when purple is one of them, not through yellow).
    var hue: [Double] = []
    /// The colour of each cloud, in order round the oval: round the colour wheel and back
    /// (green, purple, pink, pink, purple, green), so colours far apart are never neighbours.
    var ring: [Int] = []

    init(_ colours: [SIMD3<Double>]) {
        guard !colours.isEmpty else { return }
        let labs = colours.map(NewLookOklab.lab)
        lightness = labs.map(\.x)
        chroma = labs.map { ($0.y * $0.y + $0.z * $0.z).squareRoot() }
        let raw = labs.map { atan2($0.z, $0.y) }
        let order = raw.indices.sorted { raw[$0] < raw[$1] }
        var cut = 0, widest = -1.0
        for k in order.indices {
            var gap = raw[order[(k + 1) % order.count]] - raw[order[k]]
            if k == order.count - 1 { gap += 2 * Double.pi }
            if gap > widest { widest = gap; cut = k }
        }
        let start = raw[order[(cut + 1) % order.count]]
        hue = raw.map { value in
            let d = (value - start).truncatingRemainder(dividingBy: 2 * Double.pi)
            return start + (d < 0 ? d + 2 * Double.pi : d)
        }
        let wheel = hue.indices.sorted { hue[$0] < hue[$1] }
        ring = wheel + wheel.reversed()
    }
}

/// Oklab: colour as the eye sees it (lightness, then two colour axes). Mixing here keeps colours
/// rich where plain red, green and blue mixing goes grey. (Björn Ottosson's published matrices.)
enum NewLookOklab {
    private static func linear(_ c: Double) -> Double {
        c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4)
    }

    private static func gamma(_ c: Double) -> Double {
        let c = min(max(c, 0), 1)
        return c <= 0.0031308 ? c * 12.92 : 1.055 * pow(c, 1 / 2.4) - 0.055
    }

    /// sRGB (0 to 1 each) to Oklab.
    static func lab(_ rgb: SIMD3<Double>) -> SIMD3<Double> {
        let r = linear(rgb.x), g = linear(rgb.y), b = linear(rgb.z)
        let l = cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b)
        let m = cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b)
        let s = cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b)
        return SIMD3(0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
                     1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
                     0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s)
    }

    /// Oklab to sRGB (0 to 1 each, colours outside the screen's range clipped).
    static func rgb(_ lab: SIMD3<Double>) -> SIMD3<Double> {
        let l0 = lab.x + 0.3963377774 * lab.y + 0.2158037573 * lab.z
        let m0 = lab.x - 0.1055613458 * lab.y - 0.0638541728 * lab.z
        let s0 = lab.x - 0.0894841775 * lab.y - 1.2914855480 * lab.z
        let l = l0 * l0 * l0, m = m0 * m0 * m0, s = s0 * s0 * s0
        return SIMD3(gamma(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
                     gamma(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
                     gamma(-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s))
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
