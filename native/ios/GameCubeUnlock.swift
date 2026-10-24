import SwiftUI
import UIKit

// THE SECRET WAY IN TO THE GAMECUBE CORE, while it is experimental (owner, 24 October 2026: "put
// it somewhere secret, make it kinda cool too, but easily removable so we can put it the normal
// way when it's essentially done").
//
// How it works: in Settings, ABOUT, tap the Version line seven times in a row. A spinning glass
// cube says GameCube is unlocked, and a switch appears right under Version to turn it off again
// (which hides it again). Until then nothing anywhere says GameCube can be played: its file types
// are refused at import exactly as before, a GameCube disc lists as it did before, and GameCube
// appears in no picker.
//
// TO TAKE THIS OUT when GameCube is ready for everyone: delete this file and its line in
// project.yml, then follow the compiler. Every place that asks this file something is marked
// `// GameCube unlock`: answer "unlocked" there (or move `GameCubeSwitchRow` into Settings, CORES,
// as a normal switch), and put GameCube's file types into `CoreCatalog.routeTable` like any other
// system's.

/// Whether GameCube is unlocked, and what that changes. Readable from anywhere.
enum GameCubeUnlock {
    static let defaultsKey = "continuum.gamecube.unlocked"

    static var isOn: Bool { UserDefaults.standard.bool(forKey: defaultsKey) }

    /// File types only a GameCube game has. `.iso` is shared with the PlayStation and the PSP: a
    /// GameCube disc is told apart by its header (the engine's detector), not here.
    static let fileTypes: Set<String> = ["gcm", "gcz", "rvz", "wia", "ciso", "tgc", "dol"]

    /// Where a GameCube-only file goes while unlocked; nil otherwise (so it is not a game, as before).
    static func route(forExtension ext: String) -> CoreCatalog.Route? {
        guard isOn, fileTypes.contains(ext) else { return nil }
        return CoreCatalog.Route(coreId: CoreCatalog.continuumGc.coreId, system: .gamecube)
    }

    /// Whether the import accepts this file type because GameCube is unlocked.
    static func importable(_ ext: String) -> Bool {
        isOn && fileTypes.contains(ext)
    }

    /// Whether a system may be shown or routed to: everything but GameCube, and GameCube once
    /// unlocked.
    static func allows(_ system: GameSystem) -> Bool {
        system != .gamecube || isOn
    }

    /// Whether a core may be listed in Settings (the GameCube core only once unlocked).
    static func lists(coreId: String) -> Bool {
        coreId != CoreCatalog.continuumGc.coreId || isOn
    }
}

/// The switch side, for views: published so Settings redraws when it flips.
@MainActor
final class GameCubeUnlockState: ObservableObject {
    static let shared = GameCubeUnlockState()

    @Published private(set) var on: Bool = GameCubeUnlock.isOn

    func set(_ value: Bool, host: EngineHost) {
        UserDefaults.standard.set(value, forKey: GameCubeUnlock.defaultsKey)
        on = value
        // GameCube games already in the folder appear (or go) at once.
        host.refreshLibrary()
    }
}

/// Settings, ABOUT, the Version line. Seven taps in a row unlock GameCube; from the third tap on
/// it counts down, as a little hint that something is happening.
struct GameCubeVersionReadout: View {
    @ObservedObject var host: EngineHost
    let value: String
    @ObservedObject private var unlock = GameCubeUnlockState.shared
    @State private var taps = 0
    @State private var lastTap = Date.distantPast
    @State private var hint = ""
    @State private var celebrating = false

    private static let tapsNeeded = 7

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            SettingsReadout(label: "Version", value: value)
                .contentShape(Rectangle())
                .onTapGesture { tapped() }
            if !hint.isEmpty {
                Text(hint)
                    .font(.system(size: 12, weight: .semibold, design: .rounded))
                    .foregroundStyle(GameCubeLook.glow)
                    .transition(.opacity.combined(with: .move(edge: .top)))
            }
            if unlock.on {
                GameCubeSwitchRow(host: host)
            }
        }
        .animation(.easeOut(duration: 0.2), value: hint)
        .fullScreenCover(isPresented: $celebrating) {
            GameCubeUnlockedView { celebrating = false }
        }
    }

    private func tapped() {
        let now = Date()
        // Seven taps IN A ROW: a pause of more than a second and a half starts over.
        taps = now.timeIntervalSince(lastTap) < 1.5 ? taps + 1 : 1
        lastTap = now
        let left = Self.tapsNeeded - taps
        if taps >= 3 && left > 0 {
            UIImpactFeedbackGenerator(style: .light).impactOccurred()
            hint = left == 1 ? "1 more..." : "\(left) more..."
            let shown = hint
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.6) {
                if hint == shown { hint = "" }
            }
        }
        guard left <= 0 else { return }
        taps = 0
        hint = ""
        if !unlock.on {
            unlock.set(true, host: host)
        }
        // No slide-up: the cube's own entrance is the show.
        var quiet = Transaction()
        quiet.disablesAnimations = true
        withTransaction(quiet) { celebrating = true }
    }
}

/// The switch under Version once GameCube is unlocked. Off hides GameCube (and this switch)
/// again; seven taps on Version bring both back.
struct GameCubeSwitchRow: View {
    @ObservedObject var host: EngineHost
    @ObservedObject private var unlock = GameCubeUnlockState.shared

    var body: some View {
        Toggle(isOn: Binding(get: { unlock.on }, set: { unlock.set($0, host: host) })) {
            VStack(alignment: .leading, spacing: 3) {
                Text("GameCube (experimental)")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(ShellPalette.text)
                Text("Our own GameCube emulator. It's early, so games may be slow or not start. "
                     + "Turning this off hides it again.")
                    .font(.system(size: 12))
                    .foregroundStyle(ShellPalette.secondaryText)
            }
        }
        .tint(GameCubeLook.glow)
    }
}

/// The unlock colours: GameCube indigo.
enum GameCubeLook {
    static let core = Color(red: 0.60, green: 0.48, blue: 1.0)
    static let glow = Color(red: 0.66, green: 0.56, blue: 1.0)
    static let deep = Color(red: 0.10, green: 0.05, blue: 0.24)
}

/// The celebration: a glass cube with a glowing core drops in spinning, lands with a thud, and
/// a burst of little cubes flies out. Tap anywhere to close; it closes by itself after a few
/// seconds.
struct GameCubeUnlockedView: View {
    let onDone: () -> Void
    @State private var start = Date()

    /// Seconds from the start.
    private static let landsAt = 1.15
    private static let closesAt = 4.2

    var body: some View {
        TimelineView(.animation) { timeline in
            let t = timeline.date.timeIntervalSince(start)
            ZStack {
                RadialGradient(colors: [GameCubeLook.deep.opacity(0.98), Color.black],
                               center: .center, startRadius: 10, endRadius: 520)
                    .ignoresSafeArea()
                Canvas { context, size in
                    let centre = CGPoint(x: size.width / 2, y: size.height * 0.42)
                    GameCubeScene.draw(in: &context, centre: centre, t: t, landsAt: Self.landsAt)
                }
                .ignoresSafeArea()
                VStack(spacing: 8) {
                    Spacer()
                    Text("GAMECUBE")
                        .font(.system(size: 34, weight: .heavy, design: .rounded))
                        .tracking(6)
                        .foregroundStyle(
                            LinearGradient(colors: [.white, GameCubeLook.glow],
                                           startPoint: .top, endPoint: .bottom)
                        )
                    Text("unlocked")
                        .font(.system(size: 17, weight: .semibold, design: .rounded))
                        .tracking(3)
                        .foregroundStyle(.white.opacity(0.85))
                    Text("Experimental. Have fun.")
                        .font(.system(size: 13))
                        .foregroundStyle(.white.opacity(0.55))
                        .padding(.top, 6)
                }
                .padding(.bottom, 120)
                .opacity(GameCubeScene.ease(t, from: Self.landsAt + 0.05, over: 0.5))
                .offset(y: 14 * (1 - GameCubeScene.ease(t, from: Self.landsAt + 0.05, over: 0.5)))
            }
        }
        .contentShape(Rectangle())
        .onTapGesture { onDone() }
        .onAppear {
            start = Date()
            UIImpactFeedbackGenerator(style: .soft).impactOccurred()
            // The thud when it lands.
            DispatchQueue.main.asyncAfter(deadline: .now() + Self.landsAt) {
                UINotificationFeedbackGenerator().notificationOccurred(.success)
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + Self.closesAt) { onDone() }
        }
        .statusBarHidden()
    }
}

/// The drawing: plain arithmetic on a cube's corners, so it needs nothing but a Canvas.
enum GameCubeScene {
    /// 0 before `from`, rising smoothly to 1 over `over` seconds.
    static func ease(_ t: Double, from: Double, over: Double) -> Double {
        let x = min(max((t - from) / over, 0), 1)
        return 1 - pow(1 - x, 3)
    }

    /// Overshoots a little before settling, for the landing.
    static func easeOutBack(_ x: Double) -> Double {
        let x = min(max(x, 0), 1)
        let c1 = 1.70158
        let c3 = c1 + 1
        return 1 + c3 * pow(x - 1, 3) + c1 * pow(x - 1, 2)
    }

    private struct Burst {
        let angle: Double
        let speed: Double
        let size: Double
        let spin: Double
        let shade: Double
    }

    /// Twenty-eight little cubes, the same every time (a fixed sequence, not random).
    private static let bursts: [Burst] = (0..<28).map { k in
        func r(_ n: Int) -> Double {
            let v = sin(Double(k * 7919 + n * 104_729) * 12.9898) * 43758.5453
            return v - v.rounded(.down)
        }
        return Burst(angle: Double(k) / 28 * 2 * .pi + r(1) * 0.4, speed: 170 + r(2) * 240,
                     size: 5 + r(3) * 9, spin: (r(4) - 0.5) * 9, shade: 0.55 + r(5) * 0.45)
    }

    static func draw(in context: inout GraphicsContext, centre: CGPoint, t: Double, landsAt: Double) {
        let (cx, cy) = (Double(centre.x), Double(centre.y))
        // The glow behind the cube, pulsing out once it lands.
        let pulse = ease(t, from: landsAt, over: 1.1)
        if t > landsAt {
            let radius: Double = 90 + 260 * pulse
            let ring = Path(ellipseIn: CGRect(x: cx - radius, y: cy - radius,
                                              width: radius * 2, height: radius * 2))
            context.fill(ring, with: .radialGradient(
                Gradient(colors: [GameCubeLook.glow.opacity(0.45 * (1 - pulse)), .clear]),
                center: centre, startRadius: 0, endRadius: CGFloat(radius)))
            context.stroke(ring, with: .color(GameCubeLook.glow.opacity(0.6 * (1 - pulse))),
                           lineWidth: 2)
        }
        let steady = Path(ellipseIn: CGRect(x: cx - 150, y: cy - 150, width: 300, height: 300))
        context.fill(steady, with: .radialGradient(
            Gradient(colors: [GameCubeLook.core.opacity(0.28 * ease(t, from: 0.2, over: 1.0)), .clear]),
            center: centre, startRadius: 0, endRadius: 150))

        // The burst of little cubes.
        if t > landsAt {
            let dt = t - landsAt
            for b in bursts {
                let fly = 1 - pow(max(1 - dt / 1.4, 0), 2)
                let fade = max(1 - dt / 1.6, 0)
                guard fade > 0 else { continue }
                let px: Double = cx + cos(b.angle) * b.speed * fly
                let py: Double = cy + sin(b.angle) * b.speed * fly + 60 * dt * dt
                let p = CGPoint(x: px, y: py)
                drawCube(in: &context, centre: p, size: b.size, yaw: b.spin * dt + b.angle,
                         alpha: fade, glass: false, shade: b.shade)
            }
        }

        // The cube: drops in spinning twice, lands at the classic three-faces view with a bounce,
        // then turns slowly.
        let fall = min(max(t / landsAt, 0), 1)
        let scale = easeOutBack(fall)
        let settle = 1 - pow(1 - fall, 3)
        let yaw = Double.pi / 4 + 4 * Double.pi * (1 - settle) + max(t - landsAt, 0) * 0.35
        let drop: Double = -260 * (1 - settle)
        let size: Double = 70 * scale
        guard size > 0.5 else { return }
        let c = CGPoint(x: cx, y: cy + drop)
        drawCube(in: &context, centre: c, size: size * 0.48, yaw: yaw, alpha: 1, glass: false, shade: 1)
        drawCube(in: &context, centre: c, size: size, yaw: yaw, alpha: 1, glass: true, shade: 1)
    }

    /// One cube, seen from slightly above (the tilt that shows three faces), turned by `yaw`.
    /// A glass cube is see-through with bright edges; a solid one is shaded by its faces' light.
    private static func drawCube(in context: inout GraphicsContext, centre: CGPoint, size: Double,
                                 yaw: Double, alpha: Double, glass: Bool, shade: Double) {
        let tilt = 0.6155 // atan(1 / sqrt(2)): the isometric angle
        let (cy, sy, ct, st) = (cos(yaw), sin(yaw), cos(tilt), sin(tilt))
        let (ox, oy) = (Double(centre.x), Double(centre.y))
        func project(_ v: (Double, Double, Double)) -> (CGPoint, Double) {
            // Turn around the vertical axis, then tip towards the viewer.
            let x1: Double = v.0 * cy + v.2 * sy
            let z1: Double = -v.0 * sy + v.2 * cy
            let y2: Double = v.1 * ct - z1 * st
            let z2: Double = v.1 * st + z1 * ct
            let sx: Double = ox + x1 * size
            let syy: Double = oy - y2 * size
            return (CGPoint(x: sx, y: syy), z2)
        }
        let corners: [(Double, Double, Double)] = [
            (-1, -1, -1), (1, -1, -1), (1, 1, -1), (-1, 1, -1),
            (-1, -1, 1), (1, -1, 1), (1, 1, 1), (-1, 1, 1),
        ]
        let faces: [([Int], (Double, Double, Double))] = [
            ([4, 5, 6, 7], (0, 0, 1)), ([1, 0, 3, 2], (0, 0, -1)),
            ([5, 1, 2, 6], (1, 0, 0)), ([0, 4, 7, 3], (-1, 0, 0)),
            ([7, 6, 2, 3], (0, 1, 0)), ([0, 1, 5, 4], (0, -1, 0)),
        ]
        let projected = corners.map(project)
        let light = (0.35, 0.8, 0.5)
        for (indices, normal) in faces {
            // Facing the viewer?
            let n = project(normal)
            let facing = n.1 - project((0, 0, 0)).1
            guard facing > 0.001 else { continue }
            var path = Path()
            path.move(to: projected[indices[0]].0)
            for i in indices.dropFirst() { path.addLine(to: projected[i].0) }
            path.closeSubpath()
            // Light from above and in front.
            let nx: Double = normal.0 * cy + normal.2 * sy
            let nz: Double = -normal.0 * sy + normal.2 * cy
            let ny2: Double = normal.1 * ct - nz * st
            let nz2: Double = normal.1 * st + nz * ct
            let dot: Double = nx * light.0 + ny2 * light.1 + nz2 * light.2
            let lit: Double = max(0, dot)
            let brightness: Double = (0.45 + 0.55 * lit) * shade
            let colour = Color(red: 0.42 * brightness + 0.1, green: 0.32 * brightness + 0.06,
                               blue: 0.95 * brightness + 0.05)
            if glass {
                context.fill(path, with: .color(colour.opacity(0.32 * alpha)))
                context.stroke(path, with: .color(Color.white.opacity(0.75 * alpha)), lineWidth: 1.6)
            } else {
                context.fill(path, with: .color(colour.opacity(alpha)))
                context.stroke(path, with: .color(GameCubeLook.glow.opacity(0.9 * alpha)), lineWidth: 0.8)
            }
        }
    }
}
