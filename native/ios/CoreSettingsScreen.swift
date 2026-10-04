// Core settings, filters, palettes, speeds, discs, rotation, the TV's own choices and the Atari
// 2600 switches: the in-game actions, and the screens two of them open.
//
// Every behaviour is in the engine (crates/emulator-bridge: cores/options.rs, bridge_actions.rs,
// gfx/renderer.rs). This file names actions, remembers a few choices in UserDefaults, and draws.
//
// The EngineHost methods below are called BY NAME by the skins worker's button dispatcher, so their
// names, labels and return types are a contract: each returns the plain line it also put on the
// status bar.

import SwiftUI

/// Which sheet the actions want on screen. A shared object rather than a property on EngineHost,
/// so the player screen and Settings can both observe it without EngineHost growing stored state.
/// Only ever written from the main thread (the actions are main-actor methods).
final class CoreActionsModel: ObservableObject {
    static let shared = CoreActionsModel()

    enum Sheet: Identifiable, Equatable {
        case coreSettings(coreId: String)
        case filters
        case discs

        var id: String {
            switch self {
            case .coreSettings(let coreId): return "core-" + coreId
            case .filters: return "filters"
            case .discs: return "discs"
            }
        }
    }

    @Published var sheet: Sheet?
}

// MARK: Stored choices

/// The small set of choices this file keeps in UserDefaults: the look per system, and the TV's own
/// fit and layout. Everything else is the engine's (core settings are `.opt` files it writes).
enum CoreActionPrefs {
    private static let filterPrefix = "continuum.filter."
    private static let brightnessPrefix = "continuum.brightness."
    private static let tvScaleKey = "continuum.tv.scale"
    private static let tvLayoutKey = "continuum.tv.layout"

    static func effectName(_ effect: PostEffectOption) -> String {
        switch effect {
        case .none: return "none"
        case .smooth: return "smooth"
        case .sharpBilinear: return "sharpBilinear"
        case .scanlines: return "scanlines"
        case .crt: return "crt"
        case .lcdGrid: return "lcdGrid"
        case .dotMatrix: return "dotMatrix"
        }
    }

    static func effect(named name: String) -> PostEffectOption? {
        let all: [PostEffectOption] = [.none, .smooth, .sharpBilinear, .scanlines, .crt, .lcdGrid, .dotMatrix]
        return all.first { effectName($0) == name }
    }

    static func effect(for system: GameSystem) -> PostEffectOption {
        let name = UserDefaults.standard.string(forKey: filterPrefix + system.rawValue) ?? ""
        return effect(named: name) ?? .none
    }

    static func setEffect(_ effect: PostEffectOption, for system: GameSystem) {
        UserDefaults.standard.set(effectName(effect), forKey: filterPrefix + system.rawValue)
    }

    static func brightness(for system: GameSystem) -> Float {
        let stored = UserDefaults.standard.object(forKey: brightnessPrefix + system.rawValue) as? Double
        return Float(stored ?? 1.0)
    }

    static func setBrightness(_ value: Float, for system: GameSystem) {
        UserDefaults.standard.set(Double(value), forKey: brightnessPrefix + system.rawValue)
    }

    static func scaleName(_ mode: ScaleModeOption) -> String {
        switch mode {
        case .aspectFit: return "fit"
        case .integerScale: return "integer"
        case .stretch: return "stretch"
        }
    }

    static func scaleLabel(_ mode: ScaleModeOption?) -> String {
        guard let mode else { return "same as the phone" }
        switch mode {
        case .aspectFit: return "Fit"
        case .integerScale: return "Pixel perfect"
        case .stretch: return "Fill"
        }
    }

    static var tvScale: ScaleModeOption? {
        get {
            switch UserDefaults.standard.string(forKey: tvScaleKey) {
            case "fit": return .aspectFit
            case "integer": return .integerScale
            case "stretch": return .stretch
            default: return nil
            }
        }
        set {
            if let newValue {
                UserDefaults.standard.set(scaleName(newValue), forKey: tvScaleKey)
            } else {
                UserDefaults.standard.removeObject(forKey: tvScaleKey)
            }
        }
    }

    static var tvLayout: ScreenModes.Layout? {
        get { UserDefaults.standard.string(forKey: tvLayoutKey).flatMap(ScreenModes.Layout.init(rawValue:)) }
        set {
            if let newValue {
                UserDefaults.standard.set(newValue.rawValue, forKey: tvLayoutKey)
            } else {
                UserDefaults.standard.removeObject(forKey: tvLayoutKey)
            }
        }
    }
}

// MARK: The actions

extension EngineHost {
    /// Once, from `init`: where the engine keeps core settings, and the TV's stored choices.
    func configureCoreActions() {
        if let support = FileManager.default.urls(for: .applicationSupportDirectory,
                                                  in: .userDomainMask).first {
            let dir = support.appendingPathComponent("CoreOptions", isDirectory: true)
            try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            engine.setCoreOptionsDirectory(path: dir.path)
        }
        engine.setTvScaleMode(mode: CoreActionPrefs.tvScale)
        engine.setTvLayout(layout: CoreActionPrefs.tvLayout?.engineValue)
    }

    /// On every launch: the system's own look.
    func applyCoreActionPreferences() {
        guard let system = activeSystem else { return }
        var settings = engine.postEffectDefaults(effect: CoreActionPrefs.effect(for: system))
        settings.brightness = CoreActionPrefs.brightness(for: system)
        engine.setPostEffect(settings: settings)
    }

    @discardableResult
    private func coreActionReport(_ line: String) -> String {
        status = line
        return line
    }

    @MainActor @discardableResult
    func showCoreSettings() -> String {
        guard !activeCoreId.isEmpty else { return coreActionReport("core settings: no game is running") }
        CoreActionsModel.shared.sheet = .coreSettings(coreId: activeCoreId)
        return coreActionReport("core settings for \(activeCoreId)")
    }

    @MainActor @discardableResult
    func showFilters() -> String {
        guard running else { return coreActionReport("filters: no game is running") }
        CoreActionsModel.shared.sheet = .filters
        return coreActionReport("filters")
    }

    @MainActor @discardableResult
    func cyclePalette() -> String {
        guard running, let system = activeSystem else { return coreActionReport("palette: no game is running") }
        return coreActionReport(engine.cyclePalette(system: system.rawValue))
    }

    @MainActor @discardableResult
    func cycleFastForward() -> String {
        guard running else { return coreActionReport("fast forward: no game is running") }
        return coreActionReport(engine.cycleFastForward())
    }

    @MainActor @discardableResult
    func setHoldSpeed(_ multiplier: Double, held: Bool) -> String {
        guard running else { return coreActionReport("fast forward: no game is running") }
        return coreActionReport(engine.setHoldSpeed(multiplier: multiplier, held: held))
    }

    @MainActor @discardableResult
    func toggleSlowMotion() -> String {
        guard running else { return coreActionReport("slow motion: no game is running") }
        return coreActionReport(engine.toggleSlowMotion())
    }

    @MainActor @discardableResult
    func swapDisc() -> String {
        guard running else { return coreActionReport("discs: no game is running") }
        return coreActionReport(engine.swapDisc())
    }

    /// Opens the disc list, or for a Famicom Disk System game ejects or inserts the disk.
    @MainActor @discardableResult
    func insertDisc() -> String {
        guard running else { return coreActionReport("discs: no game is running") }
        guard let discs = engine.discStatus() else {
            return coreActionReport("discs: this game's core has no disc control")
        }
        if discs.fds {
            return coreActionReport(engine.insertDisc(index: 0))
        }
        CoreActionsModel.shared.sheet = .discs
        return coreActionReport("discs: \(discs.count) in this game, disc \(discs.index + 1) is in")
    }

    @MainActor @discardableResult
    func cycleResolution() -> String {
        guard running else { return coreActionReport("resolution: no game is running") }
        return coreActionReport(engine.cycleResolution())
    }

    /// Fit, pixel perfect, fill. Stored as the same preference Settings shows.
    @MainActor @discardableResult
    func cycleScreenScaling() -> String {
        let all = EmulationSettings.ScreenFit.allCases
        let index = all.firstIndex(of: emulation.screenFit) ?? 0
        emulation.screenFit = all[(index + 1) % all.count]
        screenLayoutVersion += 1
        return coreActionReport("screen scaling: \(emulation.screenFit.label)")
    }

    /// The TV's own fit: same as the phone, fit, pixel perfect, fill.
    @MainActor @discardableResult
    func cycleAirPlayScaling() -> String {
        let order: [ScaleModeOption?] = [nil, .aspectFit, .integerScale, .stretch]
        let now = CoreActionPrefs.tvScale
        let index = order.firstIndex(where: { $0 == now }) ?? 0
        let next = order[(index + 1) % order.count]
        CoreActionPrefs.tvScale = next
        engine.setTvScaleMode(mode: next)
        let tv = externalDisplayActive ? "" : " (no TV connected now)"
        return coreActionReport("TV scaling: \(CoreActionPrefs.scaleLabel(next))\(tv)")
    }

    /// The TV's own two-screen layout: same as the phone, then each layout.
    @MainActor @discardableResult
    func cycleAirPlayLayout() -> String {
        guard activeSystemHasTwoScreens || !running else {
            return coreActionReport("TV layout: only for the DS and 3DS")
        }
        let order: [ScreenModes.Layout?] = [nil] + ScreenModes.Layout.allCases.map { Optional($0) }
        let now = CoreActionPrefs.tvLayout
        let index = order.firstIndex(where: { $0 == now }) ?? 0
        let next = order[(index + 1) % order.count]
        CoreActionPrefs.tvLayout = next
        engine.setTvLayout(layout: next?.engineValue)
        screenLayoutVersion += 1
        let label = next?.label ?? "same as the phone"
        return coreActionReport("TV layout: \(label)")
    }

    @MainActor @discardableResult
    func rotateScreen() -> String {
        guard running else { return coreActionReport("rotate: no game is running") }
        let line = engine.rotateScreen()
        screenLayoutVersion += 1
        return coreActionReport(line)
    }

    @MainActor @discardableResult
    func toggleTVType() -> String {
        guard running else { return coreActionReport("TV type: no game is running") }
        return coreActionReport(engine.toggleTvType())
    }

    @MainActor @discardableResult
    func toggleDifficulty(left: Bool) -> String {
        guard running else { return coreActionReport("difficulty: no game is running") }
        return coreActionReport(engine.toggleDifficulty(left: left))
    }

    @MainActor
    func currentTVTypeIsColor() -> Bool {
        engine.tvTypeIsColor()
    }

    @MainActor
    func difficultyIsA(left: Bool) -> Bool {
        engine.difficultyIsA(left: left)
    }

    /// Quits and relaunches the running game so settings read only at start take effect. The auto
    /// save written on the way out is what it resumes from.
    @MainActor
    func restartForCoreSettings() {
        guard let entry = activeEntry else {
            status = "restart: no game is running"
            return
        }
        CoreActionsModel.shared.sheet = nil
        launch(entry: entry, resumingAuto: true)
    }

    /// The system the running game is on, for the in-game menu.
    var activeSystemHasPalette: Bool {
        guard let system = activeSystem else { return false }
        return [.gb, .gbc, .nes, .fds].contains(system) || system.rawValue == "vb"
    }
}

// MARK: The sheets

/// What the player screen and Settings present for `CoreActionsModel.Sheet`.
struct CoreActionSheet: View {
    @ObservedObject var host: EngineHost
    let sheet: CoreActionsModel.Sheet

    var body: some View {
        switch sheet {
        case .coreSettings(let coreId):
            CoreSettingsScreen(host: host, coreId: coreId)
        case .filters:
            FiltersScreen(host: host)
        case .discs:
            DiscPickerScreen(host: host)
        }
    }
}

/// One core's settings, grouped by the core's own categories.
struct CoreSettingsScreen: View {
    @ObservedObject var host: EngineHost
    let coreId: String

    @Environment(\.dismiss) private var dismiss
    @State private var entries: [CoreOptionEntry] = []
    @State private var categories: [CoreOptionCategoryRecord] = []
    @State private var forGame = false
    @State private var showHidden = false
    @State private var line = ""

    private var game: String? { host.engine.coreOptionsGame(coreId: coreId) }
    private var coreName: String { CoreCatalog.core(id: coreId)?.displayName ?? coreId }

    var body: some View {
        NavigationView {
            List {
                header
                if entries.isEmpty {
                    Section {
                        Text(host.engine.coreOptionsKnown(coreId: coreId)
                             ? "This core has no settings that can be changed here."
                             : "This core's settings are learned the first time a game runs on it. Start any game for it once, then come back.")
                            .foregroundStyle(.secondary)
                    }
                }
                ForEach(groups, id: \.key) { group in
                    Section(header: Text(group.label), footer: Text(group.info)) {
                        ForEach(group.options, id: \.key) { option in
                            row(option)
                        }
                    }
                }
                if entries.contains(where: { !$0.visible }) {
                    Section {
                        Toggle("Show settings the core hid for now", isOn: $showHidden)
                    }
                }
                Section {
                    Button("Reset every \(coreName) setting", role: .destructive) {
                        apply { try host.engine.resetCoreOptions(coreId: coreId) }
                    }
                    if game != nil {
                        Button("Clear this game's own settings", role: .destructive) {
                            apply { try host.engine.resetGameOptions(coreId: coreId) }
                        }
                    }
                }
            }
            .navigationTitle(coreName)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
        .onAppear { reload() }
    }

    @ViewBuilder
    private var header: some View {
        Section {
            if let game {
                Picker("Change for", selection: $forGame) {
                    Text("Every game").tag(false)
                    Text("This game only").tag(true)
                }
                .pickerStyle(.segmented)
                Text(forGame ? "Changes apply to \(game) only." : "Changes apply to every \(coreName) game.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
            if !line.isEmpty {
                Text(line).font(.footnote)
            }
            if game != nil, entries.contains(where: { $0.needsRestart }) {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Some changes only take effect when the game starts again.")
                        .font(.footnote)
                    Button("Restart the game now") { host.restartForCoreSettings() }
                        .font(.footnote.weight(.semibold))
                }
            }
        }
    }

    private struct OptionGroup {
        let key: String
        let label: String
        let info: String
        let options: [CoreOptionEntry]
    }

    private var groups: [OptionGroup] {
        let shown = entries.filter { $0.visible || showHidden }
        var out: [OptionGroup] = []
        let general = shown.filter { $0.category.isEmpty }
        if !general.isEmpty {
            out.append(OptionGroup(key: "", label: categories.isEmpty ? "Settings" : "General", info: "", options: general))
        }
        for category in categories {
            let options = shown.filter { $0.category == category.key }
            if !options.isEmpty {
                out.append(OptionGroup(key: category.key, label: category.label, info: category.info, options: options))
            }
        }
        return out
    }

    private func label(of value: String, in option: CoreOptionEntry) -> String {
        option.values.first(where: { $0.value == value })?.label ?? value
    }

    private func row(_ option: CoreOptionEntry) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(option.label)
                Spacer(minLength: 8)
                Menu {
                    ForEach(option.values, id: \.value) { choice in
                        Button {
                            set(option, to: choice.value)
                        } label: {
                            if choice.value == option.current {
                                Label(choice.label, systemImage: "checkmark")
                            } else {
                                Text(choice.value == option.defaultValue ? choice.label + " (default)" : choice.label)
                            }
                        }
                    }
                } label: {
                    Text(label(of: option.current, in: option))
                        .lineLimit(1)
                }
            }
            if !option.info.isEmpty {
                Text(option.info).font(.caption).foregroundStyle(.secondary)
            }
            HStack(spacing: 8) {
                if option.gameOverride {
                    Text("this game").font(.caption2.weight(.semibold)).foregroundStyle(.blue)
                }
                if option.needsRestart {
                    Text("restart needed").font(.caption2.weight(.semibold)).foregroundStyle(.orange)
                }
                if !option.visible {
                    Text("hidden by the core").font(.caption2).foregroundStyle(.secondary)
                }
            }
        }
        .opacity(option.visible ? 1 : 0.55)
    }

    private func set(_ option: CoreOptionEntry, to value: String) {
        apply {
            try host.engine.setCoreOptionValue(coreId: coreId, key: option.key, value: value, forGame: forGame && game != nil)
        }
    }

    private func apply(_ action: () throws -> String) {
        do {
            line = try action()
        } catch {
            line = "could not change it: \(error)"
        }
        host.status = line
        reload()
        // The restart flag needs a few frames to tell "the core re-read it" from "it only reads at
        // start", so the list is read once more shortly after.
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { reload() }
    }

    private func reload() {
        categories = host.engine.coreOptionCategories(coreId: coreId)
        entries = host.engine.coreOptionEntries(coreId: coreId)
    }
}

/// The post-process look for the running system.
struct FiltersScreen: View {
    @ObservedObject var host: EngineHost
    @Environment(\.dismiss) private var dismiss
    @State private var settings: PostEffectSettings?

    private var system: GameSystem? { host.activeSystem }

    var body: some View {
        NavigationView {
            List {
                if let settings {
                    Section(footer: Text(footer)) {
                        ForEach(host.engine.postEffects(), id: \.label) { info in
                            Button {
                                choose(info.effect)
                            } label: {
                                HStack {
                                    Text(info.label).foregroundStyle(.primary)
                                    if info.effect == suggested {
                                        Text("suggested").font(.caption2).foregroundStyle(.secondary)
                                    }
                                    Spacer()
                                    if info.effect == settings.effect {
                                        Image(systemName: "checkmark")
                                    }
                                }
                            }
                        }
                    }
                    Section(header: Text("Adjust")) {
                        slider("Brightness", value: settings.brightness, range: 0.5...1.5) { $0.brightness = $1 }
                        if settings.effect == .scanlines || settings.effect == .crt
                            || settings.effect == .lcdGrid || settings.effect == .dotMatrix {
                            slider(settings.effect == .scanlines || settings.effect == .crt ? "Scanlines" : "Grid",
                                   value: settings.lineStrength, range: 0...1) { $0.lineStrength = $1 }
                        }
                        if settings.effect == .crt {
                            slider("Curvature", value: settings.curvature, range: 0...0.3) { $0.curvature = $1 }
                            slider("Mask", value: settings.maskStrength, range: 0...1) { $0.maskStrength = $1 }
                            slider("Vignette", value: settings.vignette, range: 0...1) { $0.vignette = $1 }
                        }
                    }
                }
            }
            .navigationTitle("Filters")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
        .onAppear { settings = host.engine.postEffect() }
    }

    private var suggested: PostEffectOption? {
        system.map { host.engine.suggestedPostEffect(system: $0.rawValue) }
    }

    private var footer: String {
        guard let system else { return "Applies to the game on screen, on the phone and on a TV." }
        return "Remembered for every \(system.displayName) game. Shown on the phone and on a TV."
    }

    private func choose(_ effect: PostEffectOption) {
        var next = host.engine.postEffectDefaults(effect: effect)
        next.brightness = settings?.brightness ?? 1.0
        push(next)
        if let system { CoreActionPrefs.setEffect(effect, for: system) }
        host.status = "filter: " + (host.engine.postEffects().first { $0.effect == effect }?.label ?? "set")
    }

    private func push(_ next: PostEffectSettings) {
        host.engine.setPostEffect(settings: next)
        settings = host.engine.postEffect()
        if let system, let settings { CoreActionPrefs.setBrightness(settings.brightness, for: system) }
    }

    private func slider(_ title: String, value: Float, range: ClosedRange<Float>,
                        change: @escaping (inout PostEffectSettings, Float) -> Void) -> some View {
        VStack(alignment: .leading) {
            Text(title)
            Slider(value: Binding(get: { value }, set: { new in
                guard var next = settings else { return }
                change(&next, new)
                push(next)
            }), in: range)
        }
    }
}

/// Pick a disc by its label.
struct DiscPickerScreen: View {
    @ObservedObject var host: EngineHost
    @Environment(\.dismiss) private var dismiss
    @State private var discs: DiscStatusRecord?
    @State private var line = ""

    var body: some View {
        NavigationView {
            List {
                if let discs, !discs.labels.isEmpty {
                    Section(footer: Text(line.isEmpty ? "The tray is opened, the disc changed and the tray closed for you." : line)) {
                        ForEach(Array(discs.labels.enumerated()), id: \.offset) { index, label in
                            Button {
                                line = host.engine.insertDisc(index: UInt32(index))
                                host.status = line
                                self.discs = host.engine.discStatus()
                            } label: {
                                HStack {
                                    Text(label).foregroundStyle(.primary)
                                    Spacer()
                                    if UInt32(index) == discs.index {
                                        Image(systemName: "opticaldisc")
                                    }
                                }
                            }
                        }
                    }
                } else {
                    Text("This game's core lists no discs.").foregroundStyle(.secondary)
                }
            }
            .navigationTitle("Discs")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
        .onAppear { discs = host.engine.discStatus() }
    }
}

// MARK: Player menu and Settings

/// The in-game menu section: every action this file adds, for the system on screen.
struct CoreActionsMenuSection: View {
    /// A plain reference, not observed: observing the host re-ran this body (and its engine
    /// queries) on every telemetry tick while the menu was open. `PlayerActionsMenu` decides
    /// when the menu is rebuilt.
    let host: EngineHost
    let system: GameSystem?

    var body: some View {
        Section("Game") {
            Button { host.showCoreSettings() } label: {
                Label("Core settings...", systemImage: "slider.horizontal.3")
            }
            Button { host.showFilters() } label: {
                Label("Filters...", systemImage: "camera.filters")
            }
            if host.engine.discStatus() != nil {
                Button { host.swapDisc() } label: {
                    Label("Next disc", systemImage: "opticaldisc")
                }
                Button { host.insertDisc() } label: {
                    Label("Insert disc...", systemImage: "eject")
                }
            }
            if host.activeSystemHasPalette {
                Button { host.cyclePalette() } label: {
                    Label("Next palette", systemImage: "paintpalette")
                }
            }
            Button { host.cycleResolution() } label: {
                Label("Next resolution", systemImage: "square.resize")
            }
            if system == .atari2600 {
                Button { host.toggleTVType() } label: {
                    Label(host.currentTVTypeIsColor() ? "TV type: colour" : "TV type: black and white",
                          systemImage: "tv")
                }
                Button { host.toggleDifficulty(left: true) } label: {
                    Label(host.difficultyIsA(left: true) ? "Left difficulty: A" : "Left difficulty: B",
                          systemImage: "l.square")
                }
                Button { host.toggleDifficulty(left: false) } label: {
                    Label(host.difficultyIsA(left: false) ? "Right difficulty: A" : "Right difficulty: B",
                          systemImage: "r.square")
                }
            }
        }
        Section("Picture and speed") {
            Button { host.cycleFastForward() } label: {
                Label("Speed: " + speedLabel(host.engine.speedPreset()), systemImage: "forward")
            }
            Button { host.toggleSlowMotion() } label: {
                let slow = host.engine.slowMotionSpeed()
                Label(slow > 0 ? "Slow motion: " + speedLabel(slow) : "Slow motion: off", systemImage: "tortoise")
            }
            Button { host.cycleScreenScaling() } label: {
                Label("Scaling: " + host.emulation.screenFit.label, systemImage: "aspectratio")
            }
            if !host.activeSystemHasTwoScreens {
                Button { host.rotateScreen() } label: {
                    Label("Rotate picture", systemImage: "rotate.left")
                }
            }
            Button { host.cycleAirPlayScaling() } label: {
                Label("TV scaling: " + CoreActionPrefs.scaleLabel(CoreActionPrefs.tvScale), systemImage: "tv")
            }
            if host.activeSystemHasTwoScreens {
                Button { host.cycleAirPlayLayout() } label: {
                    Label("TV layout: " + (CoreActionPrefs.tvLayout?.label ?? "same as the phone"),
                          systemImage: "rectangle.split.2x1")
                }
            }
        }
    }

    private func speedLabel(_ speed: Double) -> String {
        speed == speed.rounded() ? "\(Int(speed))x" : "\(speed)x"
    }
}

/// Settings: every core, each opening its settings screen, and the TV's own choices.
struct CoreSettingsSettingsSection: View {
    @ObservedObject var host: EngineHost
    @State private var openCore: String?
    @State private var tvScale = CoreActionPrefs.tvScale
    @State private var tvLayout = CoreActionPrefs.tvLayout

    var body: some View {
        SettingsSection(title: "CORE SETTINGS") {
            SettingsNote(
                "Each emulator core has its own settings: resolution, renderer, palettes and more. "
                + "A core's list appears after a game has run on it once. In a game, the menu also "
                + "offers settings for that game only."
            )
            ForEach(CoreCatalog.all, id: \.coreId) { spec in
                SettingsButton(title: spec.displayName + (host.engine.coreOptionsKnown(coreId: spec.coreId)
                                                          ? "" : " (start a game first)"),
                               role: .normal) {
                    openCore = spec.coreId
                }
            }
            SettingsReadout(label: "TV scaling", value: CoreActionPrefs.scaleLabel(tvScale))
            SettingsButton(title: "Change TV scaling", role: .normal) {
                host.cycleAirPlayScaling()
                tvScale = CoreActionPrefs.tvScale
            }
            SettingsReadout(label: "TV layout (DS, 3DS)", value: tvLayout?.label ?? "same as the phone")
            SettingsButton(title: "Change TV layout", role: .normal) {
                host.cycleAirPlayLayout()
                tvLayout = CoreActionPrefs.tvLayout
            }
        }
        .sheet(item: Binding(get: { openCore.map { IdentifiedCore(id: $0) } },
                             set: { openCore = $0?.id })) { core in
            CoreSettingsScreen(host: host, coreId: core.id)
        }
    }

    private struct IdentifiedCore: Identifiable {
        let id: String
    }
}
