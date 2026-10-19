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
