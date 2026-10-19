// A page laid out like the New look's Home, built from the real motion pieces in
// native/ios/NewLookMotion.swift, so CI can screenshot it a few times and the frames can be
// compared to prove each piece moves. Each piece sits in a fixed place (see the frame notes) so
// the comparison can look at each one separately.

import SwiftUI

@main
struct NewLookPreviewApp: App {
    var body: some Scene {
        WindowGroup { PreviewPage() }
    }
}

struct PreviewPage: View {
    @State private var toggle = true
    @State private var tab = 0

    var body: some View {
        ZStack(alignment: .top) {
            // 1. The page background: the fluid field, full screen.
            NewLookFluid(palette: NewLookColors.shared.motionRGB, base: ShellPalette.charcoal,
                         period: 26, masses: 11, intensity: 0.55)
                .ignoresSafeArea()
            ShellPalette.charcoal.opacity(0.18).ignoresSafeArea()

            VStack(alignment: .leading, spacing: 22) {
                // 2. The hero card, y about 70 to 330 points.
                hero
                // 3. A section title with the light sweep.
                Text("Jump Back In")
                    .font(.system(size: 23, weight: .bold))
                    .foregroundStyle(.white)
                    .modifier(NewLookSweep())
                    .padding(.horizontal, 20)
                HStack(spacing: 18) {
                    // 4. A breathing glow card.
                    RoundedRectangle(cornerRadius: 10)
                        .fill(Color(white: 0.2))
                        .frame(width: 90, height: 110)
                        .modifier(NewLookBreathe(color: .blue, radius: 9, low: 0.2, high: 0.95, period: 2.0))
                    // 5. The sparkle mark (still shape, turning colours, floating, sparks).
                    NewLookSparkleMark(size: 68)
                    // 6. A switch in the New look's style.
                    Toggle("", isOn: $toggle).labelsHidden().toggleStyle(NewLookToggleStyle())
                }
                .padding(.horizontal, 20)
                // 8. A row of covers of five different shapes in one even frame each
                // (NewLookCoverFrame): tall PSP, square 3DS, wide SNES, a 16:9 screenshot, Game
                // Boy. Same frame, titles on one line.
                HStack(alignment: .top, spacing: 12) {
                    ForEach(Array(PreviewPage.mixed.enumerated()), id: \.offset) { _, item in
                        VStack(alignment: .leading, spacing: 5) {
                            NewLookCoverFrame(width: 62, height: 78, radius: 9, glow: item.glow) {
                                fakeArt(item.colors)
                            } cover: {
                                fakeArt(item.colors)
                                    .aspectRatio(item.aspect, contentMode: .fit)
                                    .overlay(RoundedRectangle(cornerRadius: 3).stroke(.white, lineWidth: 1.5))
                            }
                            Text(item.title)
                                .font(.system(size: 10, weight: .semibold))
                                .foregroundStyle(.white)
                                .lineLimit(1)
                        }
                        .frame(width: 62)
                    }
                }
                .padding(.horizontal, 20)
                Spacer(minLength: 0)
                // 7. The tab bar's pill.
                HStack(spacing: 4) {
                    ForEach(0..<4) { i in
                        Text(["Home", "Library", "Favorites", "Settings"][i])
                            .font(.system(size: 11, weight: .semibold))
                            .foregroundStyle(.white)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 16)
                            .background { if i == tab { NewLookTabPill() } }
                    }
                }
                .padding(6)
                .background(Color.black.opacity(0.4), in: RoundedRectangle(cornerRadius: 26))
                .padding(.horizontal, 16)
                .padding(.bottom, 20)
            }
            .padding(.top, 70)
            .modifier(NewLookAppear())
        }
        .preferredColorScheme(.dark)
    }

    struct Mixed { let title: String; let aspect: CGFloat; let colors: [Color]; let glow: Color? }
    static let mixed: [Mixed] = [
        Mixed(title: "PSP box", aspect: 0.58, colors: [.purple, .indigo], glow: .blue),
        Mixed(title: "3DS box", aspect: 1.0, colors: [.red, .orange], glow: .red),
        Mixed(title: "SNES box", aspect: 1.4, colors: [.gray, .white], glow: .purple),
        Mixed(title: "Screenshot", aspect: 1.78, colors: [.cyan, .blue], glow: nil),
        Mixed(title: "Game Boy", aspect: 0.9, colors: [.green, .yellow], glow: .green),
    ]

    private func fakeArt(_ colors: [Color]) -> some View {
        LinearGradient(colors: colors, startPoint: .topLeading, endPoint: .bottomTrailing)
    }

    private var hero: some View {
        let shape = RoundedRectangle(cornerRadius: 26, style: .continuous)
        return HStack(alignment: .center, spacing: 8) {
            VStack(alignment: .leading, spacing: 14) {
                Text("LAST PLAYED")
                    .font(.system(size: 15, weight: .bold))
                    .foregroundStyle(ShellPalette.gold)
                Text("Pokemon Emerald Version")
                    .font(.system(size: 28, weight: .bold))
                    .foregroundStyle(.white)
                    .modifier(NewLookSweep())
                // The primary button with its flowing border.
                Text("Resume")
                    .font(.system(size: 17, weight: .semibold))
                    .foregroundStyle(.black)
                    .padding(.horizontal, 22)
                    .padding(.vertical, 12)
                    .background(ShellPalette.accentFill, in: Capsule())
                    .newLookFlowingBorder(Capsule(), lineWidth: 1.2)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            // The floating box with sparks behind it.
            RoundedRectangle(cornerRadius: 6)
                .fill(LinearGradient(colors: [.green, .teal], startPoint: .top, endPoint: .bottom))
                .frame(width: 120, height: 150)
                .modifier(NewLookFloat(sparkColor: ShellPalette.gold))
                .frame(width: 140, height: 180)
        }
        .padding(20)
        .background(
            NewLookFluid(palette: NewLookColors.shared.motionRGB, base: .clear, period: 9, masses: 6,
                         intensity: 0.40, downscale: 2)
                .clipShape(shape)
        )
        .overlay(shape.strokeBorder(Color.white.opacity(0.08), lineWidth: 0.5))
        .padding(.horizontal, 12)
    }
}
