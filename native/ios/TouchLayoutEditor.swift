// Continuum - the on-screen control layout editor.
//
// Shows the REAL pad (`TouchControlsHost` with `isEditing` true) so what you drag is what you get.
// Every face button, shoulder (L/R/L1/L2/R1/R2), and SELECT/START has its own outline and drag.
// The D-pad stays one surface. Preview remounts on console change so labels match that system.
// Every value goes through `TouchLayout.sanitised` before commit. Landscape still overrides D-pad
// y (thumbs sit mid-edge); free buttons and SELECT/START stay free in both orientations.
//
// Import .deltaskin maps Delta info.json item frames onto that same free-drag layout. Skin art
// is not drawn yet; cancel / empty pick fails with a panel message (see DeltaSkinImport.swift).

import SwiftUI

/// The full-screen layout editor.
///
/// Holds the in-progress layout in local `@State` rather than binding the host's `@Published`
/// `touchLayout`, so a drag does not republish the whole library shell sixty times a second.
struct TouchLayoutEditor: View {

    let onCommit: (TouchLayout) -> Void
    let onClose: () -> Void

    @State private var draft: TouchLayout
    @State private var committed: TouchLayout
    @State private var previewSystem: GameSystem = .ps1
    @State private var pictureArea: CGRect?
    @State private var overlapWarning: String?
    @State private var panelExpanded = true
    @State private var input = PadInputSource()
    @State private var skinMessage: String?
    @State private var skinMessageIsError = false
    @State private var skinPicker = DeltaSkinPicker()

    init(initialLayout: TouchLayout,
         onCommit: @escaping (TouchLayout) -> Void,
         onClose: @escaping () -> Void) {
        self.onCommit = onCommit
        self.onClose = onClose
        let start = initialLayout.sanitised
        _draft = State(initialValue: start)
        _committed = State(initialValue: start)
    }

    var body: some View {
        ZStack(alignment: .top) {
            Color.black.ignoresSafeArea()
            pictureStandIn
            pad
            panel
        }
        .onDisappear {
            let finished = draft
            DispatchQueue.main.async { onCommit(finished) }
        }
    }

    // MARK: Picture stand-in

    private var pictureStandIn: some View {
        GeometryReader { proxy in
            let region = pictureArea ?? CGRect(origin: .zero, size: proxy.size)
            let width = max(1, region.width)
            let height = max(1, region.height)
            RoundedRectangle(cornerRadius: 6)
                .fill(ShellPalette.surface)
                .overlay(
                    RoundedRectangle(cornerRadius: 6)
                        .strokeBorder(ShellPalette.hairline, lineWidth: 1)
                )
                .overlay(
                    Text("GAME")
                        .font(.system(size: 11, weight: .bold))
                        .tracking(1.4)
                        .foregroundStyle(Color.white.opacity(0.28))
                )
                .frame(width: width, height: height)
                .position(x: region.midX, y: region.midY)
        }
        .ignoresSafeArea()
        .allowsHitTesting(false)
    }

    // MARK: Pad

    private var pad: some View {
        TouchControlsHost(
            system: previewSystem,
            layout: draft,
            input: input,
            onDiagnostic: { _ in },
            onPictureArea: { rect in
                DispatchQueue.main.async {
                    guard self.pictureArea != rect else { return }
                    self.pictureArea = rect
                }
            },
            isEditing: true,
            onLayoutEdited: { layout, settled in
                self.draft = layout
                if settled { self.commit(layout) }
            },
            onOverlapState: { line in
                DispatchQueue.main.async { self.overlapWarning = line }
            }
        )
        // Remount when the preview console changes so chip labels and outlines cannot keep a
        // previous system's names (e.g. Triangle/Square stuck after switching off PS1).
        .id(previewSystem)
        .ignoresSafeArea()
    }

    // MARK: Panel

    private var panel: some View {
        GeometryReader { proxy in
            VStack(spacing: 0) {
                card(maxBodyHeight: max(100, proxy.size.height * 0.36))
                Spacer(minLength: 0)
            }
        }
    }

    private func card(maxBodyHeight: CGFloat) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            header
            if panelExpanded {
                Rectangle()
                    .fill(ShellPalette.hairline)
                    .frame(height: 1)
                ScrollView {
                    VStack(alignment: .leading, spacing: 14) {
                        systemRow
                        sizeSlider
                        opacitySlider
                        HStack(spacing: 10) {
                            SettingsButton(title: "Swap sides", role: .normal) {
                                let swapped = draft.mirrored
                                draft = swapped
                                commit(swapped)
                            }
                            SettingsButton(title: "Reset", role: .destructive) {
                                let standard = TouchLayout.standard
                                draft = standard
                                commit(standard)
                            }
                            .disabled(draft.isStandard)
                            .opacity(draft.isStandard ? 0.45 : 1)
                        }
                        SettingsButton(title: "Import .deltaskin", role: .normal) {
                            beginSkinImport()
                        }
                        if let skinMessage {
                            skinBanner(skinMessage, isError: skinMessageIsError)
                        }
                        if let overlapWarning {
                            warning(overlapWarning)
                        }
                    }
                    .padding(14)
                }
                .frame(maxHeight: maxBodyHeight)
            }
        }
        .background(
            RoundedRectangle(cornerRadius: 14)
                .fill(Color.black.opacity(0.82))
                .overlay(
                    RoundedRectangle(cornerRadius: 14)
                        .strokeBorder(ShellPalette.hairline, lineWidth: 1)
                )
        )
        .padding(.horizontal, 12)
        .padding(.top, 8)
    }

    private var header: some View {
        HStack(spacing: 10) {
            VStack(alignment: .leading, spacing: 2) {
                Text("ON-SCREEN CONTROLS")
                    .font(.system(size: 11, weight: .bold))
                    .tracking(1.6)
                    .foregroundStyle(ShellPalette.secondaryText)
                Text(summaryLine)
                    .font(.system(.caption2, design: .monospaced))
                    .foregroundStyle(Color.white.opacity(0.85))
                    .lineLimit(1)
                    .minimumScaleFactor(0.7)
            }

            Spacer(minLength: 4)

            Button {
                panelExpanded.toggle()
            } label: {
                Image(systemName: panelExpanded ? "chevron.up" : "chevron.down")
                    .font(.system(size: 13, weight: .bold))
                    .foregroundStyle(.white)
                    .frame(width: 34, height: 34)
                    .background(ShellPalette.surface, in: RoundedRectangle(cornerRadius: 9))
            }
            .buttonStyle(.plain)
            .accessibilityLabel(panelExpanded
                                ? "Hide the controls panel"
                                : "Show the controls panel")

            Button {
                commit(draft)
                onClose()
            } label: {
                Text("Done")
                    .font(.system(size: 14, weight: .bold))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 8)
                    .background(ShellPalette.accent, in: Capsule())
            }
            .buttonStyle(.plain)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
    }

    private var systemRow: some View {
        HStack(spacing: 10) {
            Text("Preview")
                .font(.system(size: 14, weight: .medium))
                .foregroundStyle(.white)
            Spacer(minLength: 8)
            Menu {
                ForEach(GameSystem.allCases, id: \.self) { system in
                    Button(system.displayName) { previewSystem = system }
                }
            } label: {
                HStack(spacing: 6) {
                    Text(previewSystem.badge)
                        .font(.system(size: 14, weight: .bold))
                    Image(systemName: "chevron.up.chevron.down")
                        .font(.system(size: 11, weight: .bold))
                }
                .foregroundStyle(ShellPalette.metadata)
                .padding(.horizontal, 14)
                .frame(minWidth: 96, minHeight: 44)
                .background(ShellPalette.surfaceStrong, in: Capsule())
                .contentShape(Capsule())
            }
            .accessibilityLabel("Preview another system's controls")
            .accessibilityValue(previewSystem.displayName)
        }
    }

    private var sizeSlider: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("SIZE \(percent(draft.scale))")
                .font(.system(size: 10, weight: .bold))
                .tracking(1.1)
                .foregroundStyle(Color.white.opacity(0.45))
            Slider(
                value: number(\.scale),
                in: TouchLayout.minScale...TouchLayout.maxScale,
                onEditingChanged: { editing in
                    if !editing { commit(draft) }
                }
            )
            .tint(ShellPalette.accent)
            .accessibilityLabel("Control size")
            .accessibilityValue(percent(draft.scale))
        }
    }

    private var opacitySlider: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("OPACITY \(percent(draft.opacity))")
                .font(.system(size: 10, weight: .bold))
                .tracking(1.1)
                .foregroundStyle(Color.white.opacity(0.45))
            Slider(
                value: number(\.opacity),
                in: TouchLayout.minOpacity...TouchLayout.maxOpacity,
                onEditingChanged: { editing in
                    if !editing { commit(draft) }
                }
            )
            .tint(ShellPalette.accent)
            .accessibilityLabel("Control opacity")
            .accessibilityValue(percent(draft.opacity))
        }
    }

    private func warning(_ text: String) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("OVERLAP")
                .font(.system(size: 10, weight: .bold))
                .tracking(1.1)
                .foregroundStyle(ShellPalette.accent)
            Text(text)
                .font(.system(size: 12))
                .foregroundStyle(Color.white.opacity(0.9))
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            RoundedRectangle(cornerRadius: 10)
                .fill(ShellPalette.accent.opacity(0.14))
                .overlay(
                    RoundedRectangle(cornerRadius: 10)
                        .strokeBorder(ShellPalette.accent.opacity(0.55), lineWidth: 1)
                )
        )
    }

    private func number(_ field: WritableKeyPath<TouchLayout, Double>) -> Binding<Double> {
        Binding(
            get: { draft.sanitised[keyPath: field] },
            set: { next in
                var updated = draft
                updated[keyPath: field] = next
                draft = updated.sanitised
            }
        )
    }

    private func commit(_ layout: TouchLayout) {
        let next = layout.sanitised
        guard committed != next else { return }
        committed = next
        onCommit(next)
    }

    private var summaryLine: String {
        "size \(percent(draft.scale))  opacity \(percent(draft.opacity))"
    }

    private func skinBanner(_ text: String, isError: Bool) -> some View {
        let tint = isError ? Color.red.opacity(0.85) : ShellPalette.accent
        return VStack(alignment: .leading, spacing: 6) {
            Text(isError ? "SKIN IMPORT" : "SKIN")
                .font(.system(size: 10, weight: .bold))
                .tracking(1.1)
                .foregroundStyle(tint)
            Text(text)
                .font(.system(size: 12))
                .foregroundStyle(Color.white.opacity(0.9))
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            RoundedRectangle(cornerRadius: 10)
                .fill(tint.opacity(0.14))
                .overlay(
                    RoundedRectangle(cornerRadius: 10)
                        .strokeBorder(tint.opacity(0.55), lineWidth: 1)
                )
        )
    }

    private func beginSkinImport() {
        skinMessage = "Pick a .deltaskin, .zip, or info.json…"
        skinMessageIsError = false
        skinPicker.present { result in
            switch result {
            case .success(let imported):
                draft = imported.layout
                commit(imported.layout)
                if let system = imported.previewSystem {
                    previewSystem = system
                }
                skinMessage = imported.summary
                skinMessageIsError = false
            case .failure(let error):
                skinMessage = error.localizedDescription
                skinMessageIsError = true
            }
        }
    }

    private func percent(_ value: Double) -> String {
        "\(Int((value * 100).rounded()))%"
    }
}
