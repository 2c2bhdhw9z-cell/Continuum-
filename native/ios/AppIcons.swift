// Home screen icon switcher.
//
// Every name in `AppIconCatalog.choices` except Default is an alternate app icon.
// The asset catalogue set is `Name.appiconset`, the Info.plist key is the same
// string, and `setAlternateIconName` is passed that string. Nil restores the
// primary icon, which is the one the app shipped with.
//
// iOS always presents “You have changed the icon for Continuum.” There is no
// public way to skip that alert. The icons still have to be inside the app;
// a picture downloaded later cannot become the home screen icon.

import SwiftUI
import UIKit

struct AppIconChoice: Identifiable, Hashable {
    let name: String
    /// Passed to `setAlternateIconName`. Nil restores the primary icon.
    let alternateName: String?
    var id: String { alternateName ?? "primary" }
}

enum AppIconCatalog {
    static let choices: [AppIconChoice] = [
        AppIconChoice(name: "Default", alternateName: nil),
        AppIconChoice(name: "Halo", alternateName: "Halo"),
        AppIconChoice(name: "Prism", alternateName: "Prism"),
        AppIconChoice(name: "Signal", alternateName: "Signal"),
        AppIconChoice(name: "Plate", alternateName: "Plate"),
        AppIconChoice(name: "Bitmap", alternateName: "Bitmap"),
        AppIconChoice(name: "Circuit", alternateName: "Circuit"),
        AppIconChoice(name: "Quiet", alternateName: "Quiet"),
        AppIconChoice(name: "Cartridge", alternateName: "Cartridge"),
        AppIconChoice(name: "Nebula", alternateName: "Nebula"),
        AppIconChoice(name: "Glacier", alternateName: "Glacier"),
        AppIconChoice(name: "Cabinet", alternateName: "Cabinet"),
        AppIconChoice(name: "Dream", alternateName: "Dream"),
        AppIconChoice(name: "Forge", alternateName: "Forge"),
        AppIconChoice(name: "Orbit", alternateName: "Orbit"),
        AppIconChoice(name: "Rain", alternateName: "Rain"),
        AppIconChoice(name: "Trace", alternateName: "Trace"),
        AppIconChoice(name: "Dotmatrix", alternateName: "Dotmatrix"),
        AppIconChoice(name: "Wire", alternateName: "Wire"),
        AppIconChoice(name: "Wash", alternateName: "Wash"),
        AppIconChoice(name: "Lens", alternateName: "Lens"),
        AppIconChoice(name: "Reactor", alternateName: "Reactor"),
        AppIconChoice(name: "Knockout", alternateName: "Knockout"),
        AppIconChoice(name: "Terminal", alternateName: "Terminal"),
        AppIconChoice(name: "Gem", alternateName: "Gem"),
        AppIconChoice(name: "Storm", alternateName: "Storm"),
        AppIconChoice(name: "Clay", alternateName: "Clay"),
        AppIconChoice(name: "Saturn", alternateName: "Saturn"),
        AppIconChoice(name: "Mark", alternateName: "Mark"),
        AppIconChoice(name: "Sunset", alternateName: "Sunset"),
        AppIconChoice(name: "Fold", alternateName: "Fold"),
        AppIconChoice(name: "Glow", alternateName: "Glow"),
        AppIconChoice(name: "Velvet", alternateName: "Velvet"),
        AppIconChoice(name: "Badge", alternateName: "Badge"),
        AppIconChoice(name: "Line", alternateName: "Line"),
        AppIconChoice(name: "Fracture", alternateName: "Fracture"),
        AppIconChoice(name: "Chrome", alternateName: "Chrome"),
        AppIconChoice(name: "Cloud", alternateName: "Cloud"),
        AppIconChoice(name: "Violet", alternateName: "Violet"),
        AppIconChoice(name: "Slate", alternateName: "Slate"),
        AppIconChoice(name: "Grid", alternateName: "Grid"),
        AppIconChoice(name: "Leather", alternateName: "Leather"),
        AppIconChoice(name: "Ivory", alternateName: "Ivory"),
        AppIconChoice(name: "Ember", alternateName: "Ember"),
        AppIconChoice(name: "Spectrum", alternateName: "Spectrum"),
        AppIconChoice(name: "Navy", alternateName: "Navy"),
        AppIconChoice(name: "Emboss", alternateName: "Emboss"),
        AppIconChoice(name: "Mint", alternateName: "Mint"),
        AppIconChoice(name: "Tube", alternateName: "Tube"),
        AppIconChoice(name: "Deck", alternateName: "Deck"),
        AppIconChoice(name: "Copper", alternateName: "Copper"),
        AppIconChoice(name: "Graphite", alternateName: "Graphite"),
        AppIconChoice(name: "Lilac", alternateName: "Lilac"),
        AppIconChoice(name: "Cyan", alternateName: "Cyan"),
        AppIconChoice(name: "Sand", alternateName: "Sand"),
        AppIconChoice(name: "Filament", alternateName: "Filament"),
    ]

    /// The settings grid cannot read an app-icon set through `UIImage(named:)`.
    /// Each choice has a matching 180px PNG in AppIconPreviews/.
    static func preview(_ name: String) -> UIImage? {
        guard let url = Bundle.main.url(
            forResource: name, withExtension: "png", subdirectory: "AppIconPreviews"
        ) else { return nil }
        return UIImage(contentsOfFile: url.path)
    }
}

struct AppIconSettingsSection: View {
    @State private var selected = UIApplication.shared.alternateIconName
    @State private var note = "iPhone asks every time the icon changes. You can switch again whenever you want."

    private let columns = [GridItem(.adaptive(minimum: 68), spacing: 10)]

    var body: some View {
        SettingsSection(title: "APP ICON") {
            SettingsNote(note)
            LazyVGrid(columns: columns, spacing: 12) {
                ForEach(AppIconCatalog.choices) { choice in
                    Button {
                        apply(choice)
                    } label: {
                        VStack(spacing: 4) {
                            preview(choice)
                                .frame(width: 60, height: 60)
                                .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                                .overlay(
                                    RoundedRectangle(cornerRadius: 14, style: .continuous)
                                        .strokeBorder(
                                            isCurrent(choice) ? ShellPalette.accent : Color.white.opacity(0.16),
                                            lineWidth: isCurrent(choice) ? 2 : 1
                                        )
                                )
                            Text(choice.name)
                                .font(.system(size: 10, weight: .semibold))
                                .foregroundStyle(.white)
                                .lineLimit(1)
                        }
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Use \(choice.name) as the Continuum icon")
                }
            }
        }
    }

    private func isCurrent(_ choice: AppIconChoice) -> Bool {
        selected == choice.alternateName
    }

    @ViewBuilder
    private func preview(_ choice: AppIconChoice) -> some View {
        if let name = choice.alternateName, let image = AppIconCatalog.preview(name) {
            Image(uiImage: image)
                .resizable()
                .scaledToFill()
        } else {
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .fill(ShellPalette.surfaceStrong)
                .overlay(
                    Text(String(choice.name.prefix(1)))
                        .font(.system(size: 18, weight: .bold))
                        .foregroundStyle(.white)
                )
        }
    }

    private func apply(_ choice: AppIconChoice) {
        guard UIApplication.shared.supportsAlternateIcons else {
            note = "This install cannot change its icon."
            return
        }
        if UIApplication.shared.alternateIconName == choice.alternateName {
            return
        }
        UIApplication.shared.setAlternateIconName(choice.alternateName) { error in
            DispatchQueue.main.async {
                if let error {
                    note = error.localizedDescription
                } else {
                    selected = UIApplication.shared.alternateIconName
                    if choice.alternateName == nil {
                        note = "Primary icon restored. iPhone asks because it has to."
                    } else {
                        note = "\(choice.name) is set. iPhone asks each time, then it stays."
                    }
                }
            }
        }
    }
}
