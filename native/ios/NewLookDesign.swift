// Continuum - how the New look's pieces LOOK, drawn from plain values (text, pictures, colours).
//
// The design is the owner's Home mockup, `.kiro/notes/docs/new-look/mockup-home.jpg`. Measure
// against the picture, not against a description of it. Sizes below are the mockup's, in points
// (the mockup is drawn at about one pixel per point on a 440-point-wide phone).
//
// WHY THIS FILE EXISTS. Builds 197 to 204 changed the New look eight times without anyone seeing
// it with games on screen: the app cannot be built for the iOS Simulator (its engine is built for
// iPhone hardware only). Everything here uses only SwiftUI, UIKit and NewLookColors.swift, so the
// simulator preview (tools/newlook-preview) compiles THESE views, fills them with sample games and
// takes a screenshot to put beside the mockup. The app's own views (NewLookShell.swift) hand
// these the real games, covers and actions. Keep it that way: nothing from the engine in here.

import SwiftUI
import UIKit

/// The mockup's measurements, shared by the app and the preview.
enum NewLookMetrics {
    /// The page's side margin for titles and the hero text.
    static let side: CGFloat = 20
    /// The hero's box art frame.
    static let heroArt = CGSize(width: 196, height: 196)
    /// Space between the hero's buttons and SYSTEMS, SYSTEMS's tray and Jump Back In, and so on.
    static let sectionGap: CGFloat = 34
    /// A SYSTEMS tile.
    static let tileWidth: CGFloat = 84
    /// Home's cover cards at the Medium card size, and their frame's shape (width over height).
    static let cardWidth: CGFloat = 138
    static let cardAspect: CGFloat = 0.97
}

// MARK: - The page

/// Behind every New-look page, as Settings, LOOK, Background says: colours that swirl and mix
/// (the default, NewLookSwirl), one soft glow from the top (NewLookAura), or the plain colour.
/// It moves unless "Moving" is off; `allowsMotion` false holds it still whatever the setting
/// (a sheet, which can be over a game).
struct NewLookPage: View {
    var strength: Double = 1
    var allowsMotion = true
    @ObservedObject private var colors = NewLookColors.shared

    var body: some View {
        let moving = allowsMotion && colors.backgroundMoves
        switch colors.backgroundStyle {
        case .swirl:
            NewLookSwirl(moving: moving, strength: strength)
        case .glow:
            NewLookAura(moving: moving, strength: strength)
        case .plain:
            colors.background
        }
    }
}

/// What the pages are seen through: nothing at the top, under CONTINUUM and its buttons, then
/// fully visible from just below the header. A title scrolling up fades into the background as it
/// reaches the header instead of showing through the wordmark, and nothing is painted behind the
/// header: the background runs right to the top of the screen.
///
/// Build 205 to 209 had a frosted glass band behind the header for this; the owner circled it on
/// build 209 ("Get rid of that weird crap at the top").
struct NewLookHeaderFade: View {
    /// From the top of the screen to the bottom of the header row.
    let headerBottom: CGFloat

    var body: some View {
        // Long and eased, not a short step: the glow behind the hero's box art reaches up under
        // the header at rest, and a short fade cut it off in a straight line.
        VStack(spacing: 0) {
            Color.clear.frame(height: max(headerBottom - 30, 0))
            LinearGradient(stops: [.init(color: .clear, location: 0),
                                   .init(color: .black.opacity(0.25), location: 0.55),
                                   .init(color: .black, location: 1)],
                           startPoint: .top, endPoint: .bottom)
                .frame(height: 40)
            Color.black
        }
        .ignoresSafeArea()
    }

    /// The fade as a mask. Only on Home: it is the one page that scrolls under the header (the
    /// others start below it), and a mask costs a drawing pass.
    struct Mask: ViewModifier {
        let enabled: Bool
        let headerBottom: CGFloat

        @ViewBuilder
        func body(content: Content) -> some View {
            if enabled {
                content.mask { NewLookHeaderFade(headerBottom: headerBottom) }
            } else {
                content
            }
        }
    }
}

// MARK: - The header

/// CONTINUUM in light capitals, with the ring from the mockup.
struct NewLookWordmark: View {
    /// iOS Settings, Accessibility, Reduce Motion: the ring stays still.
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var spinning = false
    /// Settings, LOOK: the ring's colour, and whether it spins.
    @ObservedObject private var colors = NewLookColors.shared
    private var shouldSpin: Bool { colors.ringSpins && !reduceMotion }

    var body: some View {
        HStack(spacing: 8) {
            Text("CONTINUUM")
                .font(.system(size: 21, weight: .light))
                .tracking(2.2)
                .foregroundStyle(.white)
                .lineLimit(1)
                .minimumScaleFactor(0.7)
            Circle()
                .strokeBorder(
                    AngularGradient(colors: colors.ringColors, center: .center),
                    lineWidth: 4
                )
                .frame(width: 24, height: 24)
                // One slow, even turn every 3.5 seconds, for ever. The animation is tied to this
                // one value, so nothing else in the header moves with it.
                .rotationEffect(.degrees(spinning ? 360 : 0))
                .animation(spinning ? .linear(duration: 3.5).repeatForever(autoreverses: false)
                                    : .default,
                           value: spinning)
        }
        .onAppear { spinning = shouldSpin }
        .onChange(of: reduceMotion) { _ in spinning = shouldSpin }
        .onChange(of: colors.ringSpins) { _ in spinning = shouldSpin }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Continuum")
        .accessibilityAddTraits(.isHeader)
    }
}

/// The mockup's glass capsule on the right of the header: each button a round glass bead.
struct NewLookHeaderCapsule<Content: View>: View {
    @ViewBuilder let content: () -> Content

    var body: some View {
        HStack(spacing: 6, content: content)
            .padding(4)
            .background(.ultraThinMaterial, in: Capsule())
            .background(Color.white.opacity(0.06), in: Capsule())
            .overlay(Capsule().strokeBorder(Color.white.opacity(0.12), lineWidth: 0.5))
    }
}

/// One round bead in the header capsule.
struct NewLookHeaderBead: View {
    let systemImage: String

    var body: some View {
        Image(systemName: systemImage)
            .font(.system(size: 16, weight: .semibold))
            .foregroundStyle(.white)
            .frame(width: 34, height: 34)
            .background(Color.white.opacity(0.12), in: Circle())
            .contentShape(Circle())
    }
}

/// The small controller mark under the header capsule while a controller is connected, as in the
/// mockup, with its battery when the controller reports one.
struct NewLookControllerMark: View {
    /// 0 to 1, or nil when the controller does not say.
    let battery: Float?

    var body: some View {
        HStack(spacing: 4) {
            Image(systemName: "gamecontroller.fill")
                .font(.system(size: 12, weight: .semibold))
            if let battery {
                Capsule()
                    .fill(Color.white.opacity(0.25))
                    .frame(width: 4, height: 11)
                    .overlay(alignment: .bottom) {
                        Capsule()
                            .fill(battery < 0.2 ? Color.red : Color.white.opacity(0.85))
                            .frame(width: 4, height: max(2, 11 * CGFloat(battery)))
                    }
            }
        }
        .foregroundStyle(Color.white.opacity(0.7))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Controller connected")
    }
}

// MARK: - Buttons

/// The mockup's gold Resume button: a capsule in the accent, dark text.
struct NewLookPrimaryButton: View {
    let title: String
    var systemImage: String?
    let action: () -> Void
    @ObservedObject private var colors = NewLookColors.shared

    var body: some View {
        Button(action: action) {
            HStack(spacing: 9) {
                if let systemImage {
                    Image(systemName: systemImage)
                        .font(.system(size: 15, weight: .bold))
                }
                Text(title)
                    .font(.system(size: 17, weight: .semibold))
            }
            .foregroundStyle(colors.onAccent)
            .padding(.horizontal, 22)
            .padding(.vertical, 11)
            .frame(minWidth: 130)
            .background(colors.accentFill, in: Capsule())
            // The light running round it (Settings, LOOK, "Light round buttons").
            .newLookButtonLight(Capsule())
            // A soft top light, so the gold reads as a lit button, as in the mockup.
            .overlay(
                Capsule().fill(LinearGradient(colors: [.white.opacity(0.22), .clear],
                                              startPoint: .top, endPoint: .center))
                    .allowsHitTesting(false)
            )
            .shadow(color: colors.accent.opacity(0.35), radius: 10, y: 3)
        }
        .buttonStyle(NewLookPressStyle())
    }
}

/// The mockup's Game Hub button: a glass capsule, white text.
struct NewLookGlassButton: View {
    let title: String
    var systemImage: String?
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                if let systemImage {
                    Image(systemName: systemImage)
                        .font(.system(size: 17, weight: .semibold))
                        .foregroundStyle(Color.white.opacity(0.85))
                }
                Text(title)
                    .font(.system(size: 17, weight: .medium))
                    .foregroundStyle(Color.white.opacity(0.92))
            }
            .padding(.horizontal, 18)
            .padding(.vertical, 11)
            .background(.ultraThinMaterial, in: Capsule())
            .background(Color.white.opacity(0.08), in: Capsule())
            .overlay(Capsule().strokeBorder(Color.white.opacity(0.14), lineWidth: 0.5))
        }
        .buttonStyle(NewLookPressStyle())
    }
}

/// A round glass button with one glyph.
struct NewLookRoundButton: View {
    let systemImage: String
    var tint: Color = .white
    let label: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: systemImage)
                .font(.system(size: 17, weight: .semibold))
                .foregroundStyle(tint)
                .frame(width: 44, height: 44)
                .background(.ultraThinMaterial, in: Circle())
                .background(Color.white.opacity(0.08), in: Circle())
                .overlay(Circle().strokeBorder(Color.white.opacity(0.14), lineWidth: 0.5))
        }
        .buttonStyle(NewLookPressStyle())
        .accessibilityLabel(label)
    }
}

// MARK: - Section titles

/// A section's title, as in the mockup: "SYSTEMS" in grey capitals with "View all" on the right,
/// or "Jump Back In" in bold white.
struct NewLookSectionTitle: View {
    let title: String
    var caps = false
    var trailing: String?
    var onTrailing: (() -> Void)?

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            if caps {
                Text(title.uppercased())
                    .font(.system(size: 17, weight: .medium))
                    .tracking(1.2)
                    .foregroundStyle(Color.white.opacity(0.6))
            } else {
                Text(title)
                    .font(.system(size: 22, weight: .bold))
                    .foregroundStyle(.white)
            }
            Spacer(minLength: 8)
            if let trailing, let onTrailing {
                Button(trailing, action: onTrailing)
                    .font(.system(size: 16))
                    .foregroundStyle(Color.white.opacity(0.5))
                    .buttonStyle(NewLookPressStyle())
            }
        }
        .padding(.horizontal, NewLookMetrics.side)
        .accessibilityAddTraits(.isHeader)
    }
}

/// A page's own big title (Library, Favorites, Settings) and the small line under it.
struct NewLookPageTitle: View {
    let title: String
    var subtitle: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(title)
                .font(.system(size: 32, weight: .bold))
                .foregroundStyle(.white)
            if let subtitle {
                Text(subtitle)
                    .font(.system(size: 14))
                    .foregroundStyle(Color.white.opacity(0.5))
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isHeader)
    }
}

// MARK: - The hero

/// The LAST PLAYED hero, laid out as in the mockup: no card round it; the gold eyebrow, the big
/// title, the console line on the left; the box art standing on the right with the warm glow
/// behind it; Resume and Game Hub under the text. `art` is the box (NewLookBoxFace), framed to
/// `NewLookMetrics.heroArt`.
struct NewLookHeroLayout<Art: View>: View {
    let eyebrow: String
    let title: String
    let meta: String
    let primaryTitle: String
    var primaryIcon = "play.fill"
    var secondaryTitle: String? = "Game Hub"
    var secondaryIcon = "info.circle.fill"
    let onPrimary: () -> Void
    var onSecondary: () -> Void = {}
    @ViewBuilder let art: () -> Art
    @ObservedObject private var colors = NewLookColors.shared

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .top, spacing: 4) {
                VStack(alignment: .leading, spacing: 0) {
                    Text(eyebrow)
                        .font(.system(size: 15, weight: .semibold))
                        .tracking(0.6)
                        .foregroundStyle(colors.accent)
                    Text(title)
                        .font(.system(size: 30, weight: .bold))
                        .foregroundStyle(.white)
                        .lineLimit(3)
                        .minimumScaleFactor(0.7)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(.top, 12)
                    Text(meta)
                        .font(.system(size: 15))
                        .foregroundStyle(Color.white.opacity(0.55))
                        .lineLimit(3)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(.top, 10)
                }
                .padding(.top, 20)
                .frame(maxWidth: .infinity, alignment: .leading)

                art()
                    .frame(width: NewLookMetrics.heroArt.width, height: NewLookMetrics.heroArt.height)
                    .background(NewLookHeroGlow())
            }

            HStack(spacing: 10) {
                NewLookPrimaryButton(title: primaryTitle, systemImage: primaryIcon, action: onPrimary)
                if let secondaryTitle {
                    NewLookGlassButton(title: secondaryTitle, systemImage: secondaryIcon,
                                       action: onSecondary)
                }
                Spacer(minLength: 0)
            }
            .padding(.top, 2)
        }
        .padding(.leading, NewLookMetrics.side)
        .padding(.trailing, 12)
    }
}

/// Home with nothing in it yet: the hero's place, in the hero's type, and the gold button.
struct NewLookEmptyHero: View {
    var eyebrow: String? = "WELCOME"
    let title: String
    let line: String
    var actionTitle: String?
    var action: () -> Void = {}
    @ObservedObject private var colors = NewLookColors.shared

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let eyebrow {
                Text(eyebrow)
                    .font(.system(size: 15, weight: .semibold))
                    .tracking(0.6)
                    .foregroundStyle(colors.accent)
                    .padding(.bottom, 12)
            }
            Text(title)
                .font(.system(size: eyebrow == nil ? 24 : 30, weight: .bold))
                .foregroundStyle(.white)
                .fixedSize(horizontal: false, vertical: true)
            Text(line)
                .font(.system(size: 15))
                .foregroundStyle(Color.white.opacity(0.55))
                .fixedSize(horizontal: false, vertical: true)
                .padding(.top, 10)
            if let actionTitle {
                NewLookPrimaryButton(title: actionTitle, systemImage: "plus", action: action)
                    .padding(.top, 24)
            }
        }
        .padding(.top, 20)
        .padding(.horizontal, NewLookMetrics.side)
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// A box standing on a shelf, as in the mockup: the cover's front, a thin side going back on
/// its right, a slight turn, and a deep shadow. The WHOLE cover at its own shape, never cropped.
/// `placeholder` stands in until there is a picture.
struct NewLookBoxFace<Placeholder: View>: View {
    let image: UIImage?
    /// Width over height of the front while there is no picture.
    var fallbackAspect: CGFloat = 0.75
    let maxWidth: CGFloat
    let maxHeight: CGFloat
    var depth: CGFloat = 9
    var turn: Double = 2.5
    @ViewBuilder var placeholder: () -> Placeholder

    private var aspect: CGFloat {
        if let image, image.size.width > 0, image.size.height > 0 {
            return min(max(image.size.width / image.size.height, 0.45), 2.2)
        }
        return fallbackAspect
    }

    /// How far the box is turned to show its right side, as in the mockup. (Computed: a generic
    /// view cannot hold a stored static.)
    private static var yaw: Double { 18 }
    /// How strong the perspective is: the far edge a few percent shorter, as in the mockup.
    private static var perspective: CGFloat { 0.2 }

    var body: some View {
        // Fit the front and its side inside the frame, a little smaller than the frame so the
        // turn never pushes a corner out of it.
        let room = CGSize(width: maxWidth * 0.94 - depth, height: maxHeight * 0.94)
        let frontWidth = min(room.width, room.height * aspect).rounded()
        let frontHeight = (frontWidth / aspect).rounded()
        let yaw = Self.yaw * Double.pi / 180
        // The side's real depth, so that turned it shows `depth` points.
        let sideDepth = depth / CGFloat(sin(yaw))
        let shape = NewLookBoxFrontShape(radius: 3)
        // A REAL BOX, turned (owner, build 212, circled the side twice): the front turns on its
        // right edge so its left edge goes back, and the side folds back from that same edge,
        // square to the front, both in the same perspective. They meet in one sharp edge, the
        // nearest and tallest part of the box, as in the mockup. Build 205 to 212 drew a flat
        // front with a flat strip stuck on its right: a slab of another colour, not a side.
        ZStack(alignment: .leading) {
            front
                .frame(width: frontWidth, height: frontHeight)
                .clipShape(shape)
                .overlay(
                    // A soft sheen from the top left, so the front reads as printed card.
                    LinearGradient(colors: [.white.opacity(0.16), .clear, .black.opacity(0.10)],
                                   startPoint: .topLeading, endPoint: .bottomTrailing)
                        .clipShape(shape)
                )
                .rotation3DEffect(.radians(-yaw), axis: (x: 0, y: 1, z: 0),
                                  anchor: .trailing, perspective: Self.perspective)
            side
                .frame(width: sideDepth, height: frontHeight)
                .rotation3DEffect(.radians(Double.pi / 2 - yaw), axis: (x: 0, y: 1, z: 0),
                                  anchor: .leading, perspective: Self.perspective)
                .offset(x: frontWidth)
        }
        .frame(width: frontWidth, height: frontHeight, alignment: .leading)
        // Centred on what shows: the turned front is narrower, and the side sticks out right.
        .offset(x: -(frontWidth * CGFloat(1 - cos(yaw)) + depth) / 2)
        .rotationEffect(.degrees(turn))
        .shadow(color: .black.opacity(0.6), radius: 14, x: -4, y: 12)
        .frame(width: maxWidth, height: maxHeight)
        .accessibilityHidden(true)
    }

    @ViewBuilder
    private var front: some View {
        if let image {
            Image(uiImage: image)
                .interpolation(.high)
                .antialiased(true)
                .resizable()
                .aspectRatio(contentMode: .fill)
        } else {
            placeholder()
        }
    }

    /// The box's right side, square to the front (the perspective narrows it going back): one
    /// colour, the cover's own right edge, a little in shadow and darker towards the back, with a
    /// faint light on the edge it shares with the front. (Build 205 drew a squeezed copy of the
    /// picture there; build 206 to 212 a strip darkened by up to 60%, a slab of another colour.)
    private var side: some View {
        let edge = Color(uiColor: image.map(NewLookBoxEdge.edgeColor(of:)) ?? UIColor(white: 0.3, alpha: 1))
        return edge
            .overlay(LinearGradient(colors: [.black.opacity(0.16), .black.opacity(0.34)],
                                    startPoint: .leading, endPoint: .trailing))
            .overlay(alignment: .leading) {
                LinearGradient(colors: [.white.opacity(0.22), .clear],
                               startPoint: .leading, endPoint: .trailing)
                    .frame(width: 3)
            }
    }
}

/// The front of the box: its two outer corners rounded a little, its two corners on the edge it
/// shares with the side square, so the side meets it with no notch. (iOS 16 has no shape with
/// different corners.)
struct NewLookBoxFrontShape: Shape {
    var radius: CGFloat

    func path(in rect: CGRect) -> Path {
        let r = min(radius, rect.width / 2, rect.height / 2)
        var path = Path()
        path.move(to: CGPoint(x: rect.maxX, y: rect.minY))
        path.addLine(to: CGPoint(x: rect.maxX, y: rect.maxY))
        path.addLine(to: CGPoint(x: rect.minX + r, y: rect.maxY))
        path.addQuadCurve(to: CGPoint(x: rect.minX, y: rect.maxY - r),
                          control: CGPoint(x: rect.minX, y: rect.maxY))
        path.addLine(to: CGPoint(x: rect.minX, y: rect.minY + r))
        path.addQuadCurve(to: CGPoint(x: rect.minX + r, y: rect.minY),
                          control: CGPoint(x: rect.minX, y: rect.minY))
        path.closeSubpath()
        return path
    }
}

/// The average colour of a cover's right-hand edge, for the box's side (NewLookBoxFace).
/// Its own type because a generic view cannot hold the cache.
enum NewLookBoxEdge {
    private static let edgeColors = NSCache<UIImage, UIColor>()

    /// The average colour of the picture's right-hand edge, worked out once per picture.
    static func edgeColor(of image: UIImage) -> UIColor {
        if let known = edgeColors.object(forKey: image) { return known }
        var result = UIColor(white: 0.3, alpha: 1)
        if let cg = image.cgImage, cg.width > 0, cg.height > 0 {
            let x = Int(Double(cg.width) * 0.94)
            if let strip = cg.cropping(to: CGRect(x: x, y: 0, width: max(cg.width - x, 1),
                                                  height: cg.height)) {
                var pixel = [UInt8](repeating: 0, count: 4)
                pixel.withUnsafeMutableBytes { buffer in
                    guard let context = CGContext(
                        data: buffer.baseAddress, width: 1, height: 1, bitsPerComponent: 8,
                        bytesPerRow: 4, space: CGColorSpaceCreateDeviceRGB(),
                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return }
                    context.interpolationQuality = .medium
                    context.draw(strip, in: CGRect(x: 0, y: 0, width: 1, height: 1))
                }
                result = UIColor(red: CGFloat(pixel[0]) / 255, green: CGFloat(pixel[1]) / 255,
                                 blue: CGFloat(pixel[2]) / 255, alpha: 1)
            }
        }
        edgeColors.setObject(result, forKey: image)
        return result
    }
}

// MARK: - The SYSTEMS tray

/// The mockup's tray: one dark rounded panel, tiles side by side, running off the right edge of
/// the screen so it reads as something to scroll.
struct NewLookTray<Content: View>: View {
    @ViewBuilder let content: () -> Content

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 22, style: .continuous)
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 4, content: content)
                .padding(.horizontal, 4)
                .padding(.vertical, 12)
                .padding(.trailing, 24)
        }
        .background(
            shape.fill(Color.white.opacity(0.065))
                .overlay(shape.strokeBorder(Color.white.opacity(0.08), lineWidth: 0.5))
                // On past the right edge of the screen, as in the mockup.
                .padding(.trailing, -40)
        )
        .padding(.leading, 12)
    }
}

/// One console in the tray: its picture, the count in the top corner, its name in heavy italic
/// capitals underneath like a logo. The picked one has a gold rim and a glow that breathes.
/// A nil picture is the Library's All tile.
struct NewLookSystemTileFace: View {
    let image: Image?
    let name: String
    let count: Int
    let selected: Bool
    @ObservedObject private var colors = NewLookColors.shared

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 15, style: .continuous)
        VStack(spacing: 6) {
            Group {
                if let image {
                    image
                        .resizable()
                        .interpolation(.high)
                        .scaledToFit()
                } else {
                    Image(systemName: "square.grid.2x2.fill")
                        .font(.system(size: 34, weight: .semibold))
                        .foregroundStyle(colors.accent)
                }
            }
            .frame(width: 72, height: 78)
            // A soft light round each console, as in the mockup, over a little shadow.
            .shadow(color: .white.opacity(0.32), radius: 5)
            .shadow(color: .black.opacity(0.45), radius: 4, y: 3)

            Text(name)
                .font(.system(size: 13, weight: .heavy).italic())
                .tracking(-0.3)
                .foregroundStyle(Color.white.opacity(selected ? 0.98 : 0.82))
                .multilineTextAlignment(.center)
                .lineLimit(2)
                .minimumScaleFactor(0.7)
                .frame(width: NewLookMetrics.tileWidth - 4, height: 32, alignment: .top)
        }
        .padding(.top, 14)
        .padding(.bottom, 8)
        .frame(width: NewLookMetrics.tileWidth)
        // The count, in the tile's top corner and inside its frame, as in the mockup.
        .overlay(alignment: .topTrailing) {
            Text("\(count)")
                .font(.system(size: 11, weight: .bold))
                .foregroundStyle(.white)
                .padding(.horizontal, 5)
                .frame(minWidth: 22, minHeight: 22)
                .background(Color(white: 0.33), in: Capsule())
                .overlay(Capsule().strokeBorder(Color.white.opacity(0.12), lineWidth: 0.5))
                .padding(.top, 5)
                .padding(.trailing, 5)
        }
        .background {
            if selected {
                ZStack {
                    shape.fill(colors.accent.opacity(0.07))
                    // The glow: a blurred rim behind the tile, breathing (one tile at a time).
                    shape.stroke(colors.accent.opacity(0.85), lineWidth: 5)
                        .blur(radius: 8)
                        .modifier(NewLookPulse())
                }
            }
        }
        .overlay {
            if selected {
                shape.strokeBorder(colors.accent, lineWidth: 2)
                    .newLookButtonLight(shape, lineWidth: 2)
            }
        }
        .contentShape(shape)
    }
}

/// Fades a view gently in and out, slowly. Only opacity animates, so it is cheap.
struct NewLookPulse: ViewModifier {
    var low: Double = 0.55
    var period: Double = 3.0
    @State private var on = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func body(content: Content) -> some View {
        content
            .opacity(on ? 1 : low)
            .onAppear {
                guard !reduceMotion else { return }
                withAnimation(.easeInOut(duration: period / 2).repeatForever(autoreverses: true)) {
                    on = true
                }
            }
    }
}

// MARK: - A cover card

enum NewLookCoverStyle {
    /// The corner radius for a cover this wide: Settings, LOOK, "Corner roundness". 0% is square,
    /// 100% very round; the default is about what the cards always had.
    static func radius(width: CGFloat) -> CGFloat {
        CGFloat(NewLookColors.shared.coverCorner) * 0.18 * max(width, 40)
    }
}


/// A card in a row, as in the mockup's Jump Back In: the framed cover with its glow, the title,
/// and one grey line ("Last played: 2d ago").
struct NewLookCardFace<Backdrop: View, Cover: View>: View {
    /// The card's width: the titles are this wide.
    let width: CGFloat
    /// The frame's height.
    let height: CGFloat
    /// The frame's width when it is narrower than the card (it fits the art). Nil: the card's.
    var frameWidth: CGFloat?
    /// The tallest frame in the row. A shorter frame sits at the bottom of it, so the titles of
    /// a row still line up. Nil: the frame's own height.
    var slotHeight: CGFloat?
    let radius: CGFloat
    let glow: Color?
    let title: String?
    let line: String?
    var titleFont: CGFloat = 16
    var lineFont: CGFloat = 13
    @ViewBuilder var backdrop: () -> Backdrop
    @ViewBuilder var cover: () -> Cover

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            NewLookCoverFrame(width: frameWidth ?? width, height: height, radius: radius, glow: glow,
                              backdrop: backdrop, cover: cover)
                .frame(width: width, height: slotHeight ?? height, alignment: .bottomLeading)
            if let title {
                Text(title)
                    .font(.system(size: titleFont, weight: .medium))
                    .foregroundStyle(.white)
                    .lineLimit(1)
                    .padding(.top, 10)
            }
            if let line {
                Text(line)
                    .font(.system(size: lineFont))
                    .foregroundStyle(Color.white.opacity(0.5))
                    .lineLimit(1)
                    .minimumScaleFactor(0.85)
                    .padding(.top, 3)
            }
        }
        .frame(width: width, alignment: .leading)
    }
}

// MARK: - The tab bar

/// The mockup's floating glass tab bar: icon over label, the picked one in the accent with a
/// soft glow, the rest grey. The whole strip it sits in, its margins included, takes the touch,
/// so a tap beside the bar never reaches a cover scrolling underneath it.
struct NewLookTabBarFace: View {
    struct Item: Identifiable {
        let id: Int
        let icon: String
        let title: String
    }

    let items: [Item]
    let selected: Int
    var bottomPadding: CGFloat = 24
    let onSelect: (Int) -> Void
    @ObservedObject private var colors = NewLookColors.shared

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 30, style: .continuous)
        HStack(spacing: 0) {
            ForEach(items) { item in
                let isOn = item.id == selected
                Button {
                    onSelect(item.id)
                } label: {
                    VStack(spacing: 5) {
                        Image(systemName: item.icon)
                            .font(.system(size: 22, weight: .semibold))
                            .frame(height: 26)
                            .shadow(color: isOn ? colors.accent.opacity(0.6) : .clear, radius: 8)
                            .scaleEffect(isOn ? 1 : 0.94)
                        Text(item.title)
                            .font(.system(size: 15, weight: .medium))
                            .lineLimit(1)
                            .minimumScaleFactor(0.8)
                    }
                    .foregroundStyle(isOn ? AnyShapeStyle(colors.accentFill)
                                          : AnyShapeStyle(Color.white.opacity(0.6)))
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 12)
                    .contentShape(Rectangle())
                }
                .buttonStyle(NewLookPressStyle(scale: 0.9))
                .accessibilityLabel(item.title)
                .accessibilityAddTraits(isOn ? [.isButton, .isSelected] : .isButton)
            }
        }
        .animation(.spring(response: 0.35, dampingFraction: 0.6), value: selected)
        .padding(.horizontal, 6)
        .padding(.vertical, 2)
        .background(colors.background.opacity(0.2), in: shape)
        .background(.ultraThinMaterial, in: shape)
        .overlay(shape.strokeBorder(Color.white.opacity(0.10), lineWidth: 0.5))
        .shadow(color: .black.opacity(0.45), radius: 18, y: 8)
        .padding(.horizontal, 12)
        .padding(.top, 8)
        .padding(.bottom, bottomPadding)
        .contentShape(Rectangle())
    }
}

// MARK: - A glass card (sheets, Settings, the Game Hub's blocks)

/// The New look's card for a block of controls: a rounded glass panel with a hairline.
struct NewLookCardBackground: ViewModifier {
    var radius: CGFloat = 20
    var padding: CGFloat = 14

    func body(content: Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)
        content
            .padding(padding)
            .background(Color.white.opacity(0.055), in: shape)
            .overlay(shape.strokeBorder(Color.white.opacity(0.09), lineWidth: 0.5))
    }
}

/// A card's heading in the New look: the gold icon on its soft gold tile, and the title.
struct NewLookCardHeading: View {
    let icon: String
    let title: String
    @ObservedObject private var colors = NewLookColors.shared

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: icon)
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(colors.accent)
                .frame(width: 34, height: 34)
                .background(colors.accent.opacity(0.16),
                            in: RoundedRectangle(cornerRadius: 10, style: .continuous))
            Text(title)
                .font(.system(size: 17, weight: .bold))
                .foregroundStyle(.white)
            Spacer(minLength: 0)
        }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isHeader)
    }
}
