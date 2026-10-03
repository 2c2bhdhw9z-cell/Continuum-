// Continuum - the on-screen control layout editor.
//
// Shows the REAL pad (`TouchControlsHost` with `isEditing` true) so what you drag is what you get.
// Every face button, shoulder (L/R/L1/L2/R1/R2), and SELECT/START has its own outline and drag.
// The D-pad stays one surface. Preview remounts on console change so labels match that system.
// Every value goes through `TouchLayout.sanitised` before commit. Without a skin, landscape
// still overrides D-pad y (thumbs sit mid-edge). An imported skin keeps portrait and landscape
// and swaps them when the phone rotates; that skin's stored y is not overridden.
//
// Layouts are PER SYSTEM: switching the preview console loads that console's saved arrangement.
// Import .deltaskin maps Delta info.json item frames onto the inferred (or current preview)
// system's layout. Cancel / empty pick fails with a panel message.
//
// The system menu is NOT inside the scrolling part of the panel. On build 98 a tap on another
// system did not change the layout: the menu lived in that scroller, so the row's action never
// ran, and a row drawn over a pad outline was also claimed by the pad. `selectPreviewSystem`
// is unchanged. The menu stays the first row under the header, where it already was.

import SwiftUI

/// The full-screen layout editor.
///
/// Holds the in-progress layout in local `@State` rather than binding the host, so a drag does
/// not republish the whole library shell sixty times a second. Commits carry the preview
/// `GameSystem` so each console keeps its own arrangement.
struct TouchLayoutEditor: View {

    let layoutFor: (GameSystem) -> TouchLayout
    let onCommit: (GameSystem, TouchLayout) -> Void
    /// Persists imported Delta art + screens under the target console.
    let onSkinImported: (GameSystem, DeltaSkinImportResult) -> Void
    let skinImageFor: (GameSystem) -> UIImage?
    let skinScreenFor: (GameSystem) -> DeltaSkinNormalizedRect?
    let skinMappingFor: (GameSystem) -> CGSize
    let landscapeImageFor: (GameSystem) -> UIImage?
    let landscapeScreenFor: (GameSystem) -> DeltaSkinNormalizedRect?
    let landscapeMappingFor: (GameSystem) -> CGSize
    let landscapeLayoutFor: (GameSystem) -> TouchLayout?
    let onLandscapeLayout: (GameSystem, TouchLayout) -> Void
    let onClearSkin: (GameSystem) -> Void
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
    /// Bumps when import/clear changes art so the pad remounts with new UIImage.
    @State private var skinEpoch: UInt = 0
    /// True from the moment the system list appears until it is gone, including a tap that
    /// dismisses it without choosing a console. While this is set the pad claims no touches,
    /// so a row that lands on a button underneath still reaches `selectPreviewSystem`.
    @State private var systemMenuOpen = false

    init(layoutFor: @escaping (GameSystem) -> TouchLayout,
         onCommit: @escaping (GameSystem, TouchLayout) -> Void,
         onSkinImported: @escaping (GameSystem, DeltaSkinImportResult) -> Void,
         skinImageFor: @escaping (GameSystem) -> UIImage?,
         skinScreenFor: @escaping (GameSystem) -> DeltaSkinNormalizedRect?,
         skinMappingFor: @escaping (GameSystem) -> CGSize = { _ in .zero },
         landscapeImageFor: @escaping (GameSystem) -> UIImage? = { _ in nil },
         landscapeScreenFor: @escaping (GameSystem) -> DeltaSkinNormalizedRect? = { _ in nil },
         landscapeMappingFor: @escaping (GameSystem) -> CGSize = { _ in .zero },
         landscapeLayoutFor: @escaping (GameSystem) -> TouchLayout? = { _ in nil },
         onLandscapeLayout: @escaping (GameSystem, TouchLayout) -> Void = { _, _ in },
         onClearSkin: @escaping (GameSystem) -> Void,
         onClose: @escaping () -> Void) {
        self.layoutFor = layoutFor
        self.onCommit = onCommit
        self.onSkinImported = onSkinImported
        self.skinImageFor = skinImageFor
        self.skinScreenFor = skinScreenFor
        self.skinMappingFor = skinMappingFor
        self.landscapeImageFor = landscapeImageFor
        self.landscapeScreenFor = landscapeScreenFor
        self.landscapeMappingFor = landscapeMappingFor
        self.landscapeLayoutFor = landscapeLayoutFor
        self.onLandscapeLayout = onLandscapeLayout
        self.onClearSkin = onClearSkin
        self.onClose = onClose
        let start = layoutFor(.ps1).sanitised
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
            let system = previewSystem
            DispatchQueue.main.async { onCommit(system, finished) }
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
            },
            skinArtwork: skinImageFor(previewSystem),
            skinScreenNormalized: skinScreenFor(previewSystem),
            skinMapping: skinMappingFor(previewSystem),
            landscapeArtwork: landscapeImageFor(previewSystem),
            landscapeScreen: landscapeScreenFor(previewSystem),
            landscapeMapping: landscapeMappingFor(previewSystem),
            landscapeLayout: landscapeLayoutFor(previewSystem),
            onLandscapeLayoutEdited: { layout, settled in
                if settled { self.onLandscapeLayout(self.previewSystem, layout) }
            },
            editingHitsSuspended: systemMenuOpen
        )
        // Remount when the preview console or imported art changes so chip labels and skin
        // UIImage cannot keep a previous system's names or a stale texture.
        .id("\(previewSystem.rawValue)-\(skinEpoch)")
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
                // The menu used to be the first child of the ScrollView below. A menu inside
                // that scroller does not deliver its row action, which is why tapping another
                // system left the layout where it was. It stays the first row: 14pt under the
                // hairline, 14pt above the size slider, inset 14pt from the card edges, the
                // same insets the scrolling stack used. It is not moved to another part of the
                // screen. The scroller's max height gives those 72pt back so the card does not
                // grow by a row.
                systemRow
                    .padding(.horizontal, 14)
                    .padding(.top, 14)
                    .padding(.bottom, 14)
                ScrollView {
                    VStack(alignment: .leading, spacing: 14) {
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
                        if skinImageFor(previewSystem) != nil || skinScreenFor(previewSystem) != nil || landscapeMappingFor(previewSystem).width > 0 {
                            SettingsButton(title: "Clear this system's skin art", role: .destructive) {
                                onClearSkin(previewSystem)
                                skinEpoch &+= 1
                                skinMessage = "Cleared skin art for \(previewSystem.displayName)."
                                skinMessageIsError = false
                            }
                        }
                        if let skinMessage {
                            skinBanner(skinMessage, isError: skinMessageIsError)
                        }
                        if let overlapWarning {
                            warning(overlapWarning)
                        }
                    }
                    .padding(.horizontal, 14)
                    .padding(.bottom, 14)
                }
                .frame(maxHeight: max(80, maxBodyHeight - Self.previewMenuBlock))
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

    /// 14pt top inset + 44pt capsule + 14pt gap under it. Taken back off the scroller's max
    /// height so pulling the menu out of the scroller does not lengthen the card.
    private static let previewMenuBlock: CGFloat = 72

    /// The system list. Same names, same capsule, same place on the card as the old menu.
    ///
    /// Tap path, which is the whole bug: a tap on a row has to reach `selectPreviewSystem`.
    /// Two things were stopping it. The list was inside the panel `ScrollView`, and a menu
    /// there does not run its row action. And when the open list overlaps a pad outline, the
    /// pad's `point(inside:)` answered yes and took the touch (`claimsEditingHit`). This row
    /// is a sibling of the scroller, not a child of it, and `onMenuVisible` tells the pad to
    /// claim nothing until the list closes. Choosing a row still only calls
    /// `selectPreviewSystem`; that function was not the fault and is not changed here.
    private var systemRow: some View {
        HStack(spacing: 10) {
            Text("Preview")
                .font(.system(size: 14, weight: .medium))
                .foregroundStyle(.white)
            Spacer(minLength: 8)
            PreviewSystemMenu(
                current: previewSystem,
                onSelect: { selectPreviewSystem($0) },
                onMenuVisible: { open in
                    if systemMenuOpen != open { systemMenuOpen = open }
                }
            )
            .fixedSize()
        }
        .frame(maxWidth: .infinity)
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
        onCommit(previewSystem, next)
    }

    /// Switch preview console (iOS 16-safe; no two-parameter onChange).
    private func selectPreviewSystem(_ system: GameSystem) {
        guard system != previewSystem else { return }
        onCommit(previewSystem, draft.sanitised)
        previewSystem = system
        let loaded = layoutFor(system).sanitised
        draft = loaded
        committed = loaded
        skinMessage = nil
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
                let target = imported.previewSystem ?? previewSystem
                let clean = imported.layout.sanitised
                // Save under the skin's console, then show that console without re-loading an
                // older layout for it (selectPreviewSystem would layoutFor and wipe the import).
                onCommit(target, clean)
                onSkinImported(target, imported)
                if target != previewSystem {
                    onCommit(previewSystem, draft.sanitised)
                    previewSystem = target
                }
                draft = clean
                committed = clean
                skinEpoch &+= 1
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

// MARK: - System menu

/// The preview-console menu. A system `UIButton` menu, not a new control.
///
/// SwiftUI `Menu` cannot say when its list is open, and the pad has to ignore taps for that
/// whole time: a row drawn over an outline is otherwise delivered to the pad and
/// `selectPreviewSystem` never runs. The closed control is the same capsule the editor already
/// showed (badge, chevron, metadata colour, 44pt tall, at least 96pt wide). The open list is
/// the system menu of `GameSystem.displayName`, in the same order, and each row calls the same
/// select function. It is placed by the editor as the first row of the panel, outside the
/// scroller, which is where that capsule already sat.
private struct PreviewSystemMenu: UIViewRepresentable {
    let current: GameSystem
    let onSelect: (GameSystem) -> Void
    let onMenuVisible: (Bool) -> Void

    func makeUIView(context: Context) -> PreviewSystemMenuButton {
        let button = PreviewSystemMenuButton()
        button.onSelect = onSelect
        button.onMenuVisible = onMenuVisible
        button.setCurrent(current)
        return button
    }

    func updateUIView(_ button: PreviewSystemMenuButton, context: Context) {
        // Refresh the closures. Do not rebuild the menu here: this runs because the list
        // opened (the visibility flag changed), and assigning `menu` again would dismiss the
        // list the user just asked for.
        button.onSelect = onSelect
        button.onMenuVisible = onMenuVisible
        button.setCurrent(current)
    }

    func sizeThatFits(_ proposal: ProposedViewSize,
                      uiView: PreviewSystemMenuButton,
                      context: Context) -> CGSize? {
        uiView.intrinsicContentSize
    }
}

/// Tap opens the system list. `showsMenuAsPrimaryAction` is what makes it a tap rather than a
/// long press, which is how the previous menu opened. The two context-menu callbacks are the
/// only reason this is a `UIButton` subclass: the button is the menu's delegate, and these are
/// how it reports the list appearing and going away, including a tap that picks nothing.
private final class PreviewSystemMenuButton: UIButton {
    var onSelect: ((GameSystem) -> Void)?
    var onMenuVisible: ((Bool) -> Void)?
    private var displayed: GameSystem?

    override init(frame: CGRect) {
        super.init(frame: frame)
        showsMenuAsPrimaryAction = true
        menu = UIMenu(children: GameSystem.allCases.map { system in
            UIAction(title: system.displayName) { [weak self] _ in
                self?.onSelect?(system)
            }
        })
        accessibilityLabel = "Preview another system's controls"
        setContentHuggingPriority(.required, for: .horizontal)
        setContentHuggingPriority(.required, for: .vertical)
        setContentCompressionResistancePriority(.required, for: .horizontal)
        setContentCompressionResistancePriority(.required, for: .vertical)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not supported") }

    /// Badge text only. Skips work when the console has not changed, so the list opening
    /// (which re-renders the editor) does not rebuild the capsule under the open menu.
    func setCurrent(_ system: GameSystem) {
        guard displayed != system else { return }
        displayed = system
        var config = UIButton.Configuration.plain()
        config.title = system.badge
        config.image = UIImage(systemName: "chevron.up.chevron.down")
        config.preferredSymbolConfigurationForImage = UIImage.SymbolConfiguration(pointSize: 11, weight: .bold)
        config.imagePlacement = .trailing
        config.imagePadding = 6
        config.baseForegroundColor = UIColor(ShellPalette.metadata)
        config.background.backgroundColor = UIColor(ShellPalette.surfaceStrong)
        config.cornerStyle = .capsule
        config.contentInsets = NSDirectionalEdgeInsets(top: 0, leading: 14, bottom: 0, trailing: 14)
        config.titleTextAttributesTransformer = UIConfigurationTextAttributesTransformer { incoming in
            var outgoing = incoming
            outgoing.font = .systemFont(ofSize: 14, weight: .bold)
            return outgoing
        }
        configuration = config
        accessibilityValue = system.displayName
    }

    override var intrinsicContentSize: CGSize {
        let fitted = super.intrinsicContentSize
        return CGSize(width: max(96, ceil(fitted.width)), height: 44)
    }

    override func contextMenuInteraction(
        _ interaction: UIContextMenuInteraction,
        willDisplayMenuFor configuration: UIContextMenuConfiguration,
        animator: UIContextMenuInteractionAnimating?
    ) {
        super.contextMenuInteraction(interaction, willDisplayMenuFor: configuration, animator: animator)
        onMenuVisible?(true)
    }

    override func contextMenuInteraction(
        _ interaction: UIContextMenuInteraction,
        willEndFor configuration: UIContextMenuConfiguration,
        animator: UIContextMenuInteractionAnimating?
    ) {
        super.contextMenuInteraction(interaction, willEndFor: configuration, animator: animator)
        onMenuVisible?(false)
    }
}

