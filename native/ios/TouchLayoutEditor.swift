// Continuum - the on-screen control layout editor: the screen that writes the pad's six numbers.
//
// This is the last of the controls the design reference showed that needed no engine change, and it
// only needed no engine change because `TouchLayout` was built as a VALUE with limits and clamping
// long before anything could edit it. So this file adds no geometry of its own. It shows the real
// pad, lets two clusters be dragged, moves two sliders, and every number it produces has already
// been through `TouchLayout.sanitised` before it gets here.
//
// THE PAD ON THIS SCREEN IS THE REAL ONE, and that is the whole design. A drawing of the pad would
// have to reimplement the unit arithmetic, the cluster templates, the orientation rules and the
// clamping, and the second copy would eventually disagree with the first. The symptom of that
// disagreement would be the worst possible one: an editor that shows you an arrangement you do not
// get. So `TouchControlsHost` is mounted here exactly as the player screen mounts it, with
// `isEditing` true, and what you drag is the thing that will be under your thumb.
//
// Three honest limits are stated on the screen itself rather than buried here, because each one is
// a place where a control would otherwise look like it was doing nothing:
//
//   1. SELECT and START cannot be dragged. Neither position comes from these six numbers: in
//      portrait the row is pinned to the bottom centre of the play area, and in landscape each pill
//      is stacked under the column it belongs to.
//   2. Vertical position applies in PORTRAIT only. The pad centres both clusters vertically in
//      landscape on purpose, because a sideways grip puts the thumbs at the middle of the long edge.
//   3. The clamping can refuse part of a drag. A cluster pulled into a corner stops at the limit
//      that keeps it clear of the system edge gestures, and the outline stops with it rather than
//      pretending otherwise.

import SwiftUI

/// The full-screen layout editor.
///
/// Takes the layout it should start from and a closure to write the result through, rather than
/// observing `EngineHost`. That is a deliberate narrowing, for a performance reason that is easy to
/// create and hard to see: a drag produces a new layout on every touch move, and `touchLayout` is
/// `@Published` on the host, so binding straight to it would republish the host sixty times a second
/// and re-evaluate every view that observes it, including the whole library shell sitting under this
/// screen. Holding the in-progress value in local state means a drag invalidates this screen and
/// nothing else, and the host is written once per gesture instead.
struct TouchLayoutEditor: View {

    /// Called with a finished arrangement, at the end of a gesture rather than during one. The
    /// caller is expected to persist it; see `EngineHost.touchLayout`.
    let onCommit: (TouchLayout) -> Void

    /// Dismisses this screen. Owned by the presenter, because the presenter owns the flag the
    /// `fullScreenCover` is bound to.
    let onClose: () -> Void

    /// The arrangement being edited, which is what the pad on screen is drawing.
    @State private var draft: TouchLayout

    /// The last value handed to `onCommit`, so a gesture that changed nothing writes nothing.
    @State private var committed: TouchLayout

    /// Which system's pad is on screen.
    ///
    /// The control SET is per system while the layout is shared, so the layout has to be checked
    /// against a pad, and which pad matters: a PlayStation has four face buttons and four shoulders
    /// where a Game Boy has two buttons and no shoulders. Defaults to the fullest one for that
    /// reason, and it is a real choice rather than a preview toggle, because it is the only way to
    /// see whether an arrangement fits the busiest console in this build.
    @State private var previewSystem: GameSystem = .ps1

    /// The room the pad says is left for the picture, published after each of its layout passes.
    @State private var pictureArea: CGRect?

    /// The live overlap verdict: non-nil while two controls are laid out on top of each other.
    @State private var overlapWarning: String?

    /// The last thing the pad had to say for itself, which is a layout fault other than an overlap:
    /// no room at all, or no clear band for the picture.
    @State private var padNote: String = ""

    /// Whether the panel is showing more than its header.
    ///
    /// Collapsible because the panel has to sit somewhere, and anywhere it sits is somewhere a
    /// cluster can be dragged. The panel is above the pad in the z-order, so a cluster parked under
    /// it would otherwise be unreachable: collapsing gets it back. That is the reason this exists,
    /// and it is why the control says what it does rather than being a decorative chevron.
    @State private var panelExpanded = true

    /// This editor's OWN read-through box, and it must not be the one the app owns.
    ///
    /// `TouchControlsHost` points whichever box it is given at its live view, and the display link
    /// reads the app's box every frame. Handing over `EngineHost.padInput` would therefore aim the
    /// render loop at a pad in edit mode. It would happen to be harmless today, because an editing
    /// pad reports a released frame and no game can be running while Settings is on screen, but it
    /// would be harmless by luck rather than by design. A private box nothing reads makes it true by
    /// construction.
    @State private var input = PadInputSource()

    /// `initialLayout` is a seed and is deliberately not stored.
    ///
    /// Not stored because it would be a lie after the first edit: the presenter re-creates this
    /// struct on every commit, since it reads the layout out of an object it observes, so a stored
    /// copy would track the live value rather than the value the editor opened on. `@State` keeps
    /// the seed it was given the first time and ignores it afterwards, which is the behaviour
    /// wanted, and nothing else on this screen has any use for where it started.
    init(initialLayout: TouchLayout,
         onCommit: @escaping (TouchLayout) -> Void,
         onClose: @escaping () -> Void) {
        self.onCommit = onCommit
        self.onClose = onClose
        // Seeded here rather than in `onAppear`, so the first frame already draws the arrangement
        // the user arrived with. Seeding on appearance would show one frame of the default layout
        // and read as the editor having forgotten it.
        let start = initialLayout.sanitised
        _draft = State(initialValue: start)
        _committed = State(initialValue: start)
    }

    // MARK: - The screen

    var body: some View {
        ZStack(alignment: .top) {
            // Opaque. The Metal canvas is still mounted under Settings, and an editor you can see a
            // game through would make the previewed opacity impossible to judge.
            Color.black.ignoresSafeArea()

            pictureStandIn
            pad
            panel
        }
        // The backstop for anything the per-gesture commits missed, and it should never be the
        // thing that saves an arrangement: Done commits before it closes.
        //
        // Hopped off the teardown, and straight to `onCommit` rather than through `commit`, for two
        // separate reasons. Writing observable state while SwiftUI is mid-update is how the
        // "publishing changes from within view updates" fault starts, and `commit` would be touching
        // this screen's own `@State` after the screen has gone. The receiving setter ignores a value
        // equal to the one it already holds, so the normal case costs nothing.
        .onDisappear {
            let finished = draft
            DispatchQueue.main.async { onCommit(finished) }
        }
    }

    // MARK: Where the picture would be

    /// A stand-in for the game, drawn in the rect the pad says is free.
    ///
    /// Positioned from `pictureArea` with the same technique `RootView` positions the real canvas
    /// with, and from the same source: the control surface reports the region after every layout
    /// pass, so this is not a guess about how much room the controls leave. It shrinks as the
    /// clusters are dragged inward, which is the thing a layout editor most needs to show and the
    /// thing a picture of a pad on a white background cannot.
    private var pictureStandIn: some View {
        GeometryReader { proxy in
            let region = pictureArea ?? CGRect(origin: .zero, size: proxy.size)
            // Floored at one point before anything else touches it, which also happens to be what
            // makes the `Int` conversion below safe: converting a non-finite CGFloat to `Int` traps
            // rather than returning something odd, and `max` answers a comparison against a NaN with
            // its first argument. The pad cannot currently report one, and a crash is too expensive
            // a way to find out that it can.
            let width = max(1, region.width)
            let height = max(1, region.height)
            ZStack {
                RoundedRectangle(cornerRadius: 6)
                    .fill(ShellPalette.surface)
                RoundedRectangle(cornerRadius: 6)
                    .strokeBorder(ShellPalette.hairline, lineWidth: 1)
                VStack(spacing: 4) {
                    Text("THE GAME GOES HERE")
                        .font(.system(size: 11, weight: .bold))
                        .tracking(1.4)
                    Text("\(Int(width)) by \(Int(height)) points")
                        .font(.system(.caption2, design: .monospaced))
                }
                .foregroundStyle(Color.white.opacity(0.35))
            }
            .frame(width: width, height: height)
            .position(x: region.midX, y: region.midY)
        }
        .ignoresSafeArea()
        // Never takes a touch. It is a label, and a cluster dragged over it must still be grabbable.
        .allowsHitTesting(false)
    }

    // MARK: The pad

    private var pad: some View {
        TouchControlsHost(
            system: previewSystem,
            layout: draft,
            input: input,
            onDiagnostic: { line in
                // Hopped to the next run loop turn for the reason `EngineHost.updatePictureArea`
                // hops: this arrives from the middle of a UIKit layout pass, and writing SwiftUI
                // state there starts an update inside an update.
                DispatchQueue.main.async { self.padNote = line }
            },
            onPictureArea: { rect in
                DispatchQueue.main.async {
                    guard self.pictureArea != rect else { return }
                    self.pictureArea = rect
                }
            },
            isEditing: true,
            onLayoutEdited: { layout, settled in
                // Not hopped, because this comes from a touch callback rather than a layout pass,
                // and the preview has to follow the finger within the same turn to feel attached.
                self.draft = layout
                // `layout` is passed on rather than `draft` re-read: a `@State` property written
                // a line earlier is not guaranteed to read back as the new value inside the same
                // event, and committing the previous one would persist the second-to-last drag.
                if settled { self.commit(layout) }
            },
            onOverlapState: { line in
                DispatchQueue.main.async { self.overlapWarning = line }
            }
        )
        .ignoresSafeArea()
    }

    // MARK: The panel

    /// The controls, in a card at the top with the thumb clusters left clear below it.
    ///
    /// At the top because the clusters default to the bottom, which is where a thumb reaches, so a
    /// panel along the bottom would cover the thing being edited before the user had touched
    /// anything.
    private var panel: some View {
        GeometryReader { proxy in
            VStack(spacing: 0) {
                card(maxBodyHeight: max(120, proxy.size.height * 0.56))
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
                // Scrollable and bounded, because this is a lot of words on a phone in landscape,
                // where the whole screen is shorter than this panel would like to be.
                ScrollView {
                    VStack(alignment: .leading, spacing: 14) {
                        panelControls
                        panelNotes
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

    /// The controls themselves.
    ///
    /// Split from the notes below it for a mundane reason worth stating, because the error it
    /// prevents is confusing: a view builder takes at most ten children, and this panel has more
    /// than ten things in it. Two properties rather than one keeps both under the limit without a
    /// `Group` wrapper that would mean nothing to a reader.
    @ViewBuilder
    private var panelControls: some View {
        SettingsNote(
            "Drag either outlined group to move it, or use the position sliders below if you would "
            + "rather set an exact number. The two groups are the direction pad with its left "
            + "shoulder buttons, and the face buttons with their right shoulder buttons: shoulders "
            + "are anchored above the cluster they belong to, so they travel with it and cannot "
            + "land on top of it."
        )

        systemRow
        sizeSlider
        opacitySlider
        positionSliders
        SettingsReadout(label: "Positions", value: positionLine)

        HStack(spacing: 10) {
            SettingsButton(title: "Swap sides", role: .normal) {
                // Mirrored into a local first, then used twice. Reading `draft` straight back after
                // writing it is the one thing not to do with `@State` inside a single event.
                let swapped = draft.mirrored
                draft = swapped
                commit(swapped)
            }
            // Disabled rather than hidden when there is nothing to reset, so the control stays
            // where the user left it and still says what it would do. A Reset that quietly did
            // nothing would be the dead control this app's settings screen has a rule against.
            SettingsButton(title: "Reset to default", role: .destructive) {
                let standard = TouchLayout.standard
                draft = standard
                commit(standard)
            }
            .disabled(draft.isStandard)
            .opacity(draft.isStandard ? 0.45 : 1)
        }

        if let overlapWarning {
            warning(overlapWarning)
        }
    }

    /// What this screen cannot do, and why, in the same plain language Settings uses.
    @ViewBuilder
    private var panelNotes: some View {
        SettingsNote(
            "Changes apply as you make them and are remembered, so Done just closes this. Every "
            + "position is pulled back inside a limit that keeps a control on screen and clear of "
            + "the few millimetres at each edge that belong to the system's own swipe gestures, so "
            + "a cluster dragged into a corner stops short of it on purpose."
        )

        SettingsNote(
            "Up and down apply in portrait only. Held sideways, the pad centres both groups "
            + "vertically instead, because a landscape grip puts your thumbs at the middle of the "
            + "long edge rather than at the bottom corners, and the column has to fit a shoulder "
            + "above and SELECT or START below. Dragging sideways works in both."
        )

        SettingsNote(
            "SELECT and START cannot be moved. Their positions are not part of this layout: in "
            + "portrait they sit in their own row along the bottom, which is what keeps them from "
            + "ever colliding with a thumb cluster, and in landscape each one is stacked under the "
            + "group it belongs with so the middle of the screen stays clear for the picture."
        )

        if !padNote.isEmpty {
            SettingsReadout(label: "Last note from the pad", value: padNote)
        }
    }

    private var header: some View {
        HStack(spacing: 10) {
            VStack(alignment: .leading, spacing: 2) {
                Text("ON-SCREEN CONTROLS")
                    .font(.system(size: 11, weight: .bold))
                    .tracking(1.6)
                    .foregroundStyle(ShellPalette.secondaryText)
                // Kept visible while collapsed, so hiding the panel to reach a cluster underneath
                // it does not also hide what the numbers currently are.
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
                                ? "Hide the controls panel to reach a group underneath it"
                                : "Show the controls panel")

            Button {
                // Committed before closing as well as on disappear, because a commit is the only
                // thing that writes the arrangement out and Done is the path the user believes
                // saves it.
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
            VStack(alignment: .leading, spacing: 2) {
                Text("Check it against")
                    .font(.system(size: 14, weight: .medium))
                    .foregroundStyle(.white)
                Text("\(previewSystem.controlCount) buttons plus the direction pad")
                    .font(.system(.caption2, design: .monospaced))
                    .foregroundStyle(ShellPalette.secondaryText)
            }
            Spacer(minLength: 8)
            Menu {
                ForEach(GameSystem.allCases, id: \.self) { system in
                    Button(system.displayName) { previewSystem = system }
                }
            } label: {
                HStack(spacing: 4) {
                    Text(previewSystem.badge)
                        .font(.system(size: 13, weight: .semibold))
                    Image(systemName: "chevron.down")
                        .font(.system(size: 10, weight: .bold))
                }
                .foregroundStyle(ShellPalette.metadata)
                .padding(.horizontal, 10)
                .padding(.vertical, 6)
                .background(ShellPalette.surface, in: Capsule())
            }
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
                // The release is the moment to write, not each step of the drag. See `commit`.
                onEditingChanged: { editing in
                    if !editing { commit(draft) }
                }
            )
            .tint(ShellPalette.accent)
            .accessibilityLabel("Control size")
            .accessibilityValue(percent(draft.scale))
            Text("A multiplier on a size the pad works out from the room available, so it is "
                 + "relative rather than absolute, and it cannot grow past what fits: the pad "
                 + "refuses the surplus rather than running a control off the screen.")
                .font(.system(size: 12))
                .foregroundStyle(ShellPalette.secondaryText)
                .fixedSize(horizontal: false, vertical: true)
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
            Text("The controls below are drawn at exactly this, so what you see is what a game "
                 + "gets. The red outlines are not: they stay solid so a nearly invisible pad is "
                 + "still something you can take hold of. The lowest setting is a faint 15 percent "
                 + "rather than nothing, because a pad you cannot see is a pad you cannot aim at.")
                .font(.system(size: 12))
                .foregroundStyle(ShellPalette.secondaryText)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    /// The same four numbers the drag writes, as sliders.
    ///
    /// NOT A DUPLICATE OF THE DRAG, which is why they are here rather than left out as clutter.
    /// Dragging is the better way to place a control and it is the only one of the two that cannot
    /// be operated with VoiceOver on, or by someone whose hands will not make a steady drag, or by
    /// anyone who wants a number matched exactly between two devices. A slider is adjustable by
    /// assistive technology and lands on a value rather than near it. Both write through the same
    /// `number(_:)` binding, so neither can produce something the other would refuse.
    ///
    /// The vertical pair is labelled "portrait" because that is where it applies: the pad centres
    /// both groups vertically when the screen is on its side. Saying so on the control is the
    /// honest version of a slider that appears to do nothing while the phone is sideways, and the
    /// value is real either way, because the phone gets turned back.
    @ViewBuilder
    private var positionSliders: some View {
        Text("POSITION")
            .font(.system(size: 10, weight: .bold))
            .tracking(1.1)
            .foregroundStyle(Color.white.opacity(0.45))
        positionSlider("D-pad across", \.dpadX,
                       range: TouchLayout.minX...TouchLayout.maxX, value: draft.dpadX)
        positionSlider("D-pad up and down (portrait)", \.dpadY,
                       range: TouchLayout.minY...TouchLayout.maxY, value: draft.dpadY)
        positionSlider("Buttons across", \.faceX,
                       range: TouchLayout.minX...TouchLayout.maxX, value: draft.faceX)
        positionSlider("Buttons up and down (portrait)", \.faceY,
                       range: TouchLayout.minY...TouchLayout.maxY, value: draft.faceY)
    }

    /// One labelled position slider. `value` is passed in only so the caption can show it without
    /// reading the key path back out, which would be the same number by a longer route.
    private func positionSlider(_ label: String,
                                _ field: WritableKeyPath<TouchLayout, Double>,
                                range: ClosedRange<Double>,
                                value: Double) -> some View {
        HStack(spacing: 10) {
            Text(label)
                .font(.system(size: 12))
                .foregroundStyle(ShellPalette.secondaryText)
                .frame(width: 118, alignment: .leading)
                .fixedSize(horizontal: false, vertical: true)
            Slider(
                value: number(field),
                in: range,
                onEditingChanged: { editing in
                    if !editing { commit(draft) }
                }
            )
            .tint(ShellPalette.accent)
            // The label goes ON the slider rather than the row being merged into one element with
            // `accessibilityElement(children: .combine)`. Merging reads better and would have
            // thrown away the thing these sliders exist for: a combined element is not adjustable,
            // so VoiceOver could announce the value and not change it.
            .accessibilityLabel(label)
            .accessibilityValue(twoPlaces(value))
            Text(twoPlaces(value))
                .font(.system(.caption2, design: .monospaced))
                .foregroundStyle(Color.white.opacity(0.85))
                .frame(width: 34, alignment: .trailing)
        }
    }

    private func warning(_ text: String) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            Text("TWO CONTROLS ARE OVERLAPPING")
                .font(.system(size: 10, weight: .bold))
                .tracking(1.1)
                .foregroundStyle(ShellPalette.accent)
            Text(text)
                .font(.system(.caption2, design: .monospaced))
                .foregroundStyle(Color.white.opacity(0.9))
                .fixedSize(horizontal: false, vertical: true)
            Text("This is the pad's own check on the arrangement it just laid out, not a guess. "
                 + "Move the groups apart or make them smaller: a button under another button is "
                 + "one you cannot press.")
                .font(.system(size: 12))
                .foregroundStyle(ShellPalette.secondaryText)
                .fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(10)
        .background(
            RoundedRectangle(cornerRadius: 10)
                .fill(ShellPalette.accent.opacity(0.14))
                .overlay(
                    RoundedRectangle(cornerRadius: 10)
                        .strokeBorder(ShellPalette.accent.opacity(0.55), lineWidth: 1)
                )
        )
    }

    // MARK: - Editing

    /// A binding onto one of the layout's numbers that cannot produce an illegal value.
    ///
    /// Every write goes through `sanitised`, so a slider is held to the same limits a drag is. The
    /// slider's own range is already the limit for that field, which makes this redundant for the
    /// two sliders on this screen today, and it stays anyway: the redundancy is what keeps a third
    /// slider added later from being the one control that can write out of range.
    private func number(_ field: WritableKeyPath<TouchLayout, Double>) -> Binding<Double> {
        Binding(
            get: { draft[keyPath: field] },
            set: { newValue in
                var next = draft
                next[keyPath: field] = newValue
                draft = next.sanitised
            }
        )
    }

    /// Hands a finished arrangement to the caller, once.
    ///
    /// Called at the END of a gesture and not during it, which is the whole reason this screen
    /// holds a draft; see the note on the type. The comparison against the last committed value is
    /// what makes calling it from a drag, from either slider, from Reset, from Swap and from Done
    /// safe: five callers, and at most one write each time the arrangement actually changed.
    private func commit(_ layout: TouchLayout) {
        let next = layout.sanitised
        guard committed != next else { return }
        committed = next
        onCommit(next)
    }

    // MARK: - Read-outs

    /// The whole arrangement in one line, for the collapsed header.
    private var summaryLine: String {
        "size \(percent(draft.scale))  opacity \(percent(draft.opacity))  \(positionLine)"
    }

    /// The four position numbers, to two decimal places.
    ///
    /// Shown at all, rather than only the visual result, because they are the values that persist
    /// and because a user comparing two arrangements needs something to compare. Two decimals is
    /// what the limits are expressed to.
    private var positionLine: String {
        let live = draft.sanitised
        return "d-pad \(twoPlaces(live.dpadX)), \(twoPlaces(live.dpadY))"
            + "  buttons \(twoPlaces(live.faceX)), \(twoPlaces(live.faceY))"
    }

    private func percent(_ value: Double) -> String {
        "\(Int((value * 100).rounded()))%"
    }

    private func twoPlaces(_ value: Double) -> String {
        String(format: "%.2f", value)
    }
}
