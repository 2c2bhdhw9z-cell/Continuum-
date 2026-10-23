// The New look's Home, drawn by the REAL New-look pieces (native/ios/NewLookDesign.swift,
// NewLookMotion.swift and NewLookColors.swift) and filled with the games in the owner's mockup,
// so CI can screenshot it in the iOS Simulator and put it beside the mockup
// (.kiro/notes/docs/new-look/mockup-home.jpg). The app itself cannot be built for the simulator.
//
// The page below is laid out exactly as NewLookHome and LibraryShell lay it out (same pieces,
// same NewLookMetrics, same paddings). If you change the spacing there, change it here too.
//
// Launch arguments: -page home | empty | scrolled, and -accent HEX-HEX for a colour (the owner
// uses A66BFF-FF5FB0, purple to pink). The covers are cut out of the mockup itself, so the
// screenshot compares like with like.

import SwiftUI
import UIKit

@main
struct NewLookPreviewApp: App {
    var body: some Scene {
        WindowGroup { PreviewRoot() }
    }
}

private enum Page: String {
    case home, empty, scrolled
    /// The page background alone, full screen, to film how its colours mix.
    case bg
    /// The standing box (NewLookBoxFace) close up, with real covers run.sh downloads.
    case box
}

private func argument(_ name: String) -> String? {
    let args = ProcessInfo.processInfo.arguments
    guard let i = args.firstIndex(of: name), i + 1 < args.count else { return nil }
    return args[i + 1]
}

private let page: Page = {
    let args = ProcessInfo.processInfo.arguments
    if let i = args.firstIndex(of: "-page"), i + 1 < args.count, let p = Page(rawValue: args[i + 1]) {
        return p
    }
    return .home
}()

/// Pictures from the app bundle (run.sh copies them in).
private enum Art {
    static func file(_ name: String) -> UIImage? {
        guard let path = Bundle.main.path(forResource: name, ofType: nil) else { return nil }
        return UIImage(contentsOfFile: path)
    }

    static let mockup = file("mockup-home.jpg")

    /// A piece of the mockup, in its own pixels.
    static func cut(_ x: CGFloat, _ y: CGFloat, _ w: CGFloat, _ h: CGFloat) -> UIImage? {
        guard let cg = mockup?.cgImage?.cropping(to: CGRect(x: x, y: y, width: w, height: h))
        else { return nil }
        return UIImage(cgImage: cg)
    }

    static let yellow = cut(266, 152, 166, 176)
    static let sapphire = cut(40, 695, 130, 134)
    static let dragonBall = cut(190, 695, 130, 134)
    static let kirby = cut(340, 695, 115, 134)

    static func console(_ name: String) -> Image? {
        file("console-\(name).png").map { Image(uiImage: $0) }
    }
}

struct PreviewRoot: View {
    init() {
        NewLookColors.shared.accentHex = argument("-accent").flatMap { NewLookColors.validated($0) }
        if let source = argument("-swirl").flatMap(NewLookColors.SwirlSource.init(rawValue:)) {
            NewLookColors.shared.swirlSource = source
        }
        if let own = argument("-own").flatMap({ NewLookColors.validated($0, maxParts: 3) }) {
            NewLookColors.shared.swirlHex = own
            NewLookColors.shared.swirlSource = .own
        }
    }

    var body: some View {
        GeometryReader { geo in
            if page == .bg {
                NewLookPage().ignoresSafeArea()
            } else if page == .box {
                PreviewBoxes()
            } else {
                PreviewHome(insets: geo.safeAreaInsets, empty: page == .empty)
            }
        }
        .preferredColorScheme(.dark)
        .statusBarHidden(false)
    }
}

/// Two real covers as standing boxes at Home's hero size, then twice that, to see the side.
private struct PreviewBoxes: View {
    private let covers = ["box-doki.png", "box-silver.png"].compactMap { Art.file($0) }

    var body: some View {
        VStack(spacing: 30) {
            HStack(spacing: 0) {
                ForEach(covers.indices, id: \.self) { i in
                    NewLookBoxFace(image: covers[i], maxWidth: NewLookMetrics.heroArt.width,
                                   maxHeight: NewLookMetrics.heroArt.height) { Color.gray }
                }
            }
            ForEach(covers.indices, id: \.self) { i in
                NewLookBoxFace(image: covers[i], maxWidth: NewLookMetrics.heroArt.width,
                               maxHeight: NewLookMetrics.heroArt.height) { Color.gray }
                    .scaleEffect(1.8)
                    .frame(height: 340)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color(red: 0.94, green: 0.47, blue: 0.12))
    }
}

private struct Card: Identifiable {
    let id: Int
    let image: UIImage?
    let title: String
    let line: String
    let glow: Color
}

/// Console colours from NewLook.consoleColor (NewLookShell.swift).
private let gbaGlow = Color(red: 0.36, green: 0.45, blue: 1.0)
private let gbcGlow = Color(red: 1.0, green: 0.70, blue: 0.20)

private struct PreviewHome: View {
    let insets: EdgeInsets
    let empty: Bool

    /// LibraryShell: topBarHeight 52, plus the top inset, plus 6.
    private var topPadding: CGFloat { 52 + max(insets.top, 8) + 6 }

    private let cards = [
        Card(id: 0, image: Art.sapphire, title: "Pokémon Sapphire", line: "Last played: 2d ago", glow: gbaGlow),
        Card(id: 1, image: Art.dragonBall, title: "Dragon Ball Z: Legendary Super Warriors",
             line: "Last played: 3d ago", glow: gbcGlow),
        Card(id: 2, image: Art.kirby, title: "Kirby Tilt 'n' Tumble", line: "Last played: 5d ago", glow: gbcGlow),
    ]

    var body: some View {
        ZStack(alignment: .top) {
            NewLookBackdropPreview()
                .ignoresSafeArea()

            ScrollViewReader { proxy in
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    if empty {
                        NewLookEmptyHero(title: "Nothing to play yet",
                                         line: "Add your games and they'll show up here. Zips work too.",
                                         actionTitle: "Import games")
                    } else {
                        NewLookHeroLayout(
                            eyebrow: "LAST PLAYED",
                            title: "Pokémon - Yellow Version",
                            meta: "GAME BOY COLOR \u{2022} mGBA \u{2022} 1h 45m played",
                            primaryTitle: "Resume",
                            onPrimary: {}
                        ) {
                            NewLookBoxFace(image: Art.yellow, maxWidth: NewLookMetrics.heroArt.width,
                                           maxHeight: NewLookMetrics.heroArt.height) {
                                Color.gray
                            }
                            .modifier(NewLookFloat())
                        }

                        VStack(alignment: .leading, spacing: 12) {
                            NewLookSectionTitle(title: "Systems", caps: true, trailing: "View all",
                                                onTrailing: {})
                            NewLookTray {
                                tile("gb", "GAME BOY", selected: false)
                                tile("gbc", "GAME BOY COLOR", selected: false)
                                tile("gba", "GAME BOY ADVANCE", selected: false)
                                tile("snes", "SUPER NINTENDO", selected: false)
                                tile("n64", "NINTENDO 64", selected: false)
                            }
                        }
                        .padding(.top, NewLookMetrics.sectionGap + 3)
                        .id("systems")

                        VStack(alignment: .leading, spacing: 2) {
                            NewLookSectionTitle(title: "Jump Back In")
                            ScrollView(.horizontal, showsIndicators: false) {
                                HStack(alignment: .top, spacing: 12) {
                                    ForEach(cards) { card in cardView(card) }
                                }
                                .padding(.horizontal, NewLookMetrics.side - 2)
                                .padding(.vertical, 10)
                            }
                        }
                        .padding(.top, NewLookMetrics.sectionGap - 5)
                    }
                    Spacer(minLength: 700)
                }
                .padding(.top, topPadding + 4)
            }
            .ignoresSafeArea(edges: .top)
            // NewLookHome: what scrolls up fades out under the header; nothing behind it.
            .modifier(NewLookHeaderFade.Mask(enabled: true, headerBottom: topPadding - 6))
            .onAppear {
                guard page == .scrolled else { return }
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                    proxy.scrollTo("systems", anchor: .top)
                }
            }
            }

            header
            VStack(spacing: 0) {
                Spacer(minLength: 0)
                NewLookTabBarFace(
                    items: [
                        .init(id: 0, icon: "house.fill", title: "Home"),
                        .init(id: 1, icon: "books.vertical.fill", title: "Library"),
                        .init(id: 2, icon: "heart.fill", title: "Favorites"),
                        .init(id: 3, icon: "gearshape.fill", title: "Settings"),
                    ],
                    selected: 0,
                    bottomPadding: max(insets.bottom - 12, 10)
                ) { _ in }
            }
            .ignoresSafeArea(edges: .bottom)
        }
    }

    /// LibraryShell.newTopBar, without the search field.
    private var header: some View {
        HStack(spacing: 10) {
            NewLookWordmark()
            Spacer(minLength: 8)
            NewLookHeaderCapsule {
                NewLookHeaderBead(systemImage: "magnifyingglass")
                NewLookHeaderBead(systemImage: "plus")
                NewLookHeaderBead(systemImage: "person.fill")
            }
            .overlay(alignment: .bottomTrailing) {
                NewLookControllerMark(battery: 0.8)
                    .offset(x: -10, y: 20)
            }
        }
        .padding(.horizontal, 18)
        .frame(height: 52)
        .padding(.top, max(insets.top, 8))
        .ignoresSafeArea(edges: .top)
    }

    private func tile(_ id: String, _ name: String, selected: Bool) -> some View {
        NewLookSystemTileFace(image: Art.console(id), name: name, count: 13, selected: selected)
    }

    private func cardView(_ card: Card) -> some View {
        // NewLookGlowCard on a shelf: the row's height, as wide as the art.
        let base = NewLookMetrics.cardWidth
        let height = (base / NewLookMetrics.cardAspect).rounded()
        let aspect = card.image.map { $0.size.width / max($0.size.height, 1) } ?? 0.75
        let width = min(max(height * aspect, base * 0.6), base * 1.6).rounded()
        let radius = NewLookCoverStyle.radius(width: min(width, height))
        return NewLookCardFace(width: width, height: height, radius: radius, glow: card.glow,
                               title: card.title, line: card.line) {
            if let image = card.image {
                Image(uiImage: image).resizable().aspectRatio(contentMode: .fill)
                    .blur(radius: 28, opaque: true).scaleEffect(1.15)
            }
        } cover: {
            if let image = card.image {
                Image(uiImage: image).resizable().aspectRatio(contentMode: .fit)
                    .clipShape(RoundedRectangle(cornerRadius: radius, style: .continuous))
            }
        }
    }
}

/// NewLookBackdrop (NewLookShell.swift) is just NewLookPage.
private struct NewLookBackdropPreview: View {
    var body: some View { NewLookPage() }
}
