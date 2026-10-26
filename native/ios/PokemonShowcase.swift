// Continuum - the Pokémon Box's Showcase (PokemonBox.swift's Tools menu), after G1R Deluxe's:
// stages to show Box Pokémon off on, each with a background, a pattern and the Pokémon placed
// where the player drags them. Kept in Application Support/PokemonBox/showcase.json. The
// pictures are each generation's own sprites, from PokeAPI's sprite collection on GitHub (needs
// the internet the first time; the phone keeps them after).

import SwiftUI

struct ShowcasePiece: Codable, Identifiable, Equatable {
    var id = UUID()
    var species: Int
    var generation: Int
    var shiny: Bool
    var name: String
    var x: Double
    var y: Double
    var scale: Double = 1
}

struct ShowcaseStage: Codable, Identifiable, Equatable {
    var id = UUID()
    var name: String
    var background: Int = 0
    var pattern: Int = 0
    var pieces: [ShowcasePiece] = []
}

enum ShowcaseLook {
    static let backgrounds: [(String, Color)] = [
        ("Forest", Color(red: 0.18, green: 0.36, blue: 0.24)), ("Sky", Color(red: 0.30, green: 0.50, blue: 0.68)),
        ("Brick", Color(red: 0.44, green: 0.30, blue: 0.34)), ("Sunset", Color(red: 0.60, green: 0.32, blue: 0.25)),
        ("Ocean", Color(red: 0.14, green: 0.42, blue: 0.50)), ("Midnight", Color(red: 0.12, green: 0.14, blue: 0.26)),
        ("Meadow", Color(red: 0.36, green: 0.50, blue: 0.24)), ("Lavender", Color(red: 0.43, green: 0.34, blue: 0.58)),
        ("Sand", Color(red: 0.65, green: 0.56, blue: 0.38)), ("Snow", Color(red: 0.60, green: 0.67, blue: 0.72)),
    ]
    static let patterns = ["Plain", "Checks", "Dots", "Stripes"]

    /// The sprite of the generation the Pokémon is kept as: Red/Blue, Crystal or Emerald's.
    static func sprite(_ p: ShowcasePiece) -> URL? {
        let base = "https://raw.githubusercontent.com/PokeAPI/sprites/master/sprites/pokemon/versions/"
        let path: String
        switch p.generation {
        case 1: path = "generation-i/red-blue/transparent/\(p.species).png"
        case 2: path = "generation-ii/crystal/transparent/\(p.shiny ? "shiny/" : "")\(p.species).png"
        default: path = "generation-iii/emerald/\(p.shiny ? "shiny/" : "")\(p.species).png"
        }
        return URL(string: base + path)
    }
}

@MainActor
enum ShowcaseStore {
    static var url: URL { URL(fileURLWithPath: PokemonGames.folder).appendingPathComponent("showcase.json") }

    static func load() -> [ShowcaseStage] {
        guard let data = try? Data(contentsOf: url),
              let stages = try? JSONDecoder().decode([ShowcaseStage].self, from: data) else { return [] }
        return stages
    }

    static func save(_ stages: [ShowcaseStage]) {
        if let data = try? JSONEncoder().encode(stages) {
            try? data.write(to: url, options: .atomic)
        }
    }
}

struct PokemonShowcasePane: View {
    let storage: [PokeBoxPage]
    @State private var stages: [ShowcaseStage] = []
    @State private var current = 0
    @State private var picked: UUID?

    private var stage: ShowcaseStage? { stages.indices.contains(current) ? stages[current] : nil }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            SettingsLabel("Showcase")
            HStack(spacing: 8) {
                Menu {
                    ForEach(Array(stages.enumerated()), id: \.element.id) { i, s in
                        Button(s.name) { current = i; picked = nil }
                    }
                    Button("New stage") { addStage() }
                } label: {
                    Text(stage?.name ?? "No stage yet")
                        .font(.system(size: 14, weight: .semibold))
                        .padding(.horizontal, 12).padding(.vertical, 8)
                        .background(ShellPalette.surfaceStrong, in: RoundedRectangle(cornerRadius: 10))
                }
                if stage == nil {
                    SettingsButton(title: "Make a stage", role: .normal) { addStage() }
                }
            }
            if let stage {
                HStack(spacing: 8) {
                    Menu {
                        ForEach(Array(ShowcaseLook.backgrounds.enumerated()), id: \.offset) { i, b in
                            Button(b.0) { edit { $0.background = i } }
                        }
                    } label: { chip("Background: \(ShowcaseLook.backgrounds[stage.background % 10].0)") }
                    Menu {
                        ForEach(Array(ShowcaseLook.patterns.enumerated()), id: \.offset) { i, n in
                            Button(n) { edit { $0.pattern = i } }
                        }
                    } label: { chip("Pattern: \(ShowcaseLook.patterns[stage.pattern % 4])") }
                }
                canvas(stage)
                    .frame(height: 300)
                    .clipShape(RoundedRectangle(cornerRadius: 12))
                HStack(spacing: 8) {
                    Menu {
                        ForEach(boxPokemon) { entry in
                            let m = entry.mon
                            Button("\(m.name)\(m.shiny ? " ★" : "") (Lv \(m.level))") { add(m) }
                        }
                    } label: { chip(boxPokemon.isEmpty ? "The Box is empty" : "Add a Box Pokémon") }
                    .disabled(boxPokemon.isEmpty || stage.pieces.count >= 30)
                    if let picked, stage.pieces.contains(where: { $0.id == picked }) {
                        Button("Bigger") { edit { s in if let i = s.pieces.firstIndex(where: { $0.id == picked }) { s.pieces[i].scale = min(3, s.pieces[i].scale * 1.25) } } }
                            .buttonStyle(.bordered)
                        Button("Smaller") { edit { s in if let i = s.pieces.firstIndex(where: { $0.id == picked }) { s.pieces[i].scale = max(0.4, s.pieces[i].scale / 1.25) } } }
                            .buttonStyle(.bordered)
                        Button("Remove", role: .destructive) {
                            edit { stage in stage.pieces.removeAll { piece in piece.id == picked } }
                            self.picked = nil
                        }
                            .buttonStyle(.bordered)
                    }
                }
                HStack {
                    SettingsButton(title: "Delete this stage", role: .destructive) {
                        stages.remove(at: current)
                        current = max(0, current - 1)
                        ShowcaseStore.save(stages)
                    }
                }
                SettingsNote("Drag a Pokémon to place it, tap it to make it bigger or smaller. Stages are kept on this phone.")
            }
        }
        .padding(12)
        .modifier(LookBlockCard(radius: 12))
        .onAppear { stages = ShowcaseStore.load() }
    }

    private struct BoxEntry: Identifiable {
        let id: String
        let mon: PokeMon
    }

    private var boxPokemon: [BoxEntry] {
        var out: [BoxEntry] = []
        for (b, page) in storage.enumerated() {
            for (i, m) in page.slots.enumerated() {
                if let m, m.readable, !m.egg { out.append(BoxEntry(id: "\(b)/\(i)", mon: m)) }
            }
        }
        return out
    }

    private func chip(_ t: String) -> some View {
        Text(t).font(.system(size: 12, weight: .semibold))
            .padding(.horizontal, 10).padding(.vertical, 7)
            .background(ShellPalette.surfaceStrong, in: RoundedRectangle(cornerRadius: 9))
    }

    private func addStage() {
        stages.append(ShowcaseStage(name: "Stage \(stages.count + 1)"))
        current = stages.count - 1
        ShowcaseStore.save(stages)
    }

    private func edit(_ change: (inout ShowcaseStage) -> Void) {
        guard stages.indices.contains(current) else { return }
        change(&stages[current])
        ShowcaseStore.save(stages)
    }

    private func add(_ m: PokeMon) {
        let n = stage?.pieces.count ?? 0
        let piece = ShowcasePiece(species: Int(m.species), generation: Int(m.generation), shiny: m.shiny, name: m.name,
                                  x: 0.15 + Double(n % 5) * 0.17, y: 0.25 + Double(n / 5 % 3) * 0.25)
        edit { $0.pieces.append(piece) }
        picked = piece.id
    }

    private func canvas(_ stage: ShowcaseStage) -> some View {
        GeometryReader { geo in
            ZStack {
                ShowcaseLook.backgrounds[stage.background % 10].1
                pattern(stage.pattern % 4, in: geo.size)
                ForEach(stage.pieces) { p in
                    VStack(spacing: 0) {
                        AsyncImage(url: ShowcaseLook.sprite(p)) { img in
                            img.resizable().interpolation(.none).scaledToFit()
                        } placeholder: {
                            Text(p.name).font(.system(size: 10)).foregroundStyle(.white)
                        }
                        .frame(width: 64 * p.scale, height: 64 * p.scale)
                        Text(p.name)
                            .font(.system(size: 9, weight: .bold))
                            .foregroundStyle(.white)
                            .shadow(radius: 1)
                    }
                    .padding(2)
                    .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(picked == p.id ? Color.yellow : .clear, lineWidth: 2))
                    .position(x: p.x * geo.size.width, y: p.y * geo.size.height)
                    .onTapGesture { picked = p.id }
                    .gesture(DragGesture().onChanged { v in
                        let x = min(max(v.location.x / geo.size.width, 0), 1)
                        let y = min(max(v.location.y / geo.size.height, 0), 1)
                        if let i = stages[current].pieces.firstIndex(where: { $0.id == p.id }) {
                            stages[current].pieces[i].x = x
                            stages[current].pieces[i].y = y
                        }
                        picked = p.id
                    }.onEnded { _ in ShowcaseStore.save(stages) })
                }
            }
        }
    }

    @ViewBuilder
    private func pattern(_ kind: Int, in size: CGSize) -> some View {
        Canvas { ctx, sz in
            let shade = GraphicsContext.Shading.color(.white.opacity(0.10))
            let step: CGFloat = 24
            switch kind {
            case 1:
                for yi in 0..<Int(sz.height / step) + 1 {
                    for xi in 0..<Int(sz.width / step) + 1 where (xi + yi) % 2 == 0 {
                        ctx.fill(Path(CGRect(x: CGFloat(xi) * step, y: CGFloat(yi) * step, width: step, height: step)), with: shade)
                    }
                }
            case 2:
                for yi in 0..<Int(sz.height / step) + 1 {
                    for xi in 0..<Int(sz.width / step) + 1 {
                        ctx.fill(Path(ellipseIn: CGRect(x: CGFloat(xi) * step + 9, y: CGFloat(yi) * step + 9, width: 6, height: 6)), with: shade)
                    }
                }
            case 3:
                for yi in stride(from: 0, to: sz.height, by: step) {
                    ctx.fill(Path(CGRect(x: 0, y: yi, width: sz.width, height: step / 2)), with: shade)
                }
            default:
                break
            }
        }
    }
}
