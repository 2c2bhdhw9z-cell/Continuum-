// Continuum - the player screen, and the diagnostics panel both screens share.
//
// Before this file the app had one screen: the Metal canvas full bleed, with the debug HUD and the
// Library stacked on top of it. That was correct for bring-up and wrong as a product. There was
// also no way OUT of a running game, which matters more than it sounds: leaving has to call
// `stopSession()` so the core is handed back to the registry and unloaded under the Drop retention
// policy, and nothing was calling it.
//
// Two layout decisions in here are deliberate and are the reason the bug in docs/mobile-player.png
// is not reproduced:
//
//  1. SESSION controls (pause, reset, save state, diagnostics) live in the TOP bar. GAMEPLAY
//     controls own the bottom. In the browser layout those two sets shared one band, and the
//     screenshot shows the result: the D-pad sitting on the aspect selector, B sitting on START,
//     and SCALE disappearing under the D-pad. Separating them by region means they cannot collide
//     no matter what either one is sized at.
//  2. The status line sits at the TOP, under the telemetry strip, and not along the bottom, where
//     it would land on SELECT and START.

import SwiftUI

// MARK: - The player

/// A running game: the picture, the telemetry, the touch controls and the way back out.
struct PlayerScreen: View {
    @ObservedObject var host: EngineHost

    /// Which pad to draw. Nil when the launched file's extension has no system mapped, which the
    /// launch path should already have refused, so it is reported rather than silently ignored.
    let system: GameSystem?

    var body: some View {
        ZStack(alignment: .top) {
            // Controls first, so the chrome's buttons sit above them in the z-order. They only
            // claim the touches that land on an actual control (see
            // `TouchControlsView.point(inside:with:)`), so the picture and the chrome stay live.
            if let system {
                TouchControlsHost(
                    system: system,
                    layout: host.touchLayout,
                    input: host.padInput,
                    onDiagnostic: { line in host.noteControlLayout(line) },
                    onPictureArea: { rect in host.updatePictureArea(rect) }
                )
                .ignoresSafeArea()
            }

            VStack(alignment: .leading, spacing: 6) {
                topBar
                telemetryStrip
                statusLine
                if host.showDiagnostics {
                    DiagnosticsPanel(host: host)
                }
                // Claims the rest of the height without claiming any touches, so everything below
                // reaches the controls.
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 12)
            .padding(.top, 6)
        }
    }

    // MARK: Top bar

    private var topBar: some View {
        HStack(spacing: 10) {
            Button {
                host.leavePlayer()
            } label: {
                Image(systemName: "chevron.left")
                    .font(.system(size: 17, weight: .semibold))
                    .frame(width: 40, height: 40)
                    .background(Color.white.opacity(0.12), in: Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Back to the library")

            Text(host.activeEntry?.name ?? "no game")
                .font(.system(.footnote, design: .monospaced))
                .lineLimit(1)
                .truncationMode(.middle)
                .foregroundStyle(Color.white.opacity(0.9))

            Spacer(minLength: 4)

            sessionButton(host.paused ? "play.fill" : "pause.fill",
                          label: host.paused ? "Resume" : "Pause") {
                host.togglePause()
            }
            sessionButton("arrow.clockwise", label: "Reset") {
                host.resetGame()
            }
            sessionButton("square.and.arrow.down", label: "Save state") {
                host.saveStateToDisk()
            }
            sessionButton(host.showDiagnostics ? "info.circle.fill" : "info.circle",
                          label: "Diagnostics") {
                host.showDiagnostics.toggle()
            }
        }
        .foregroundStyle(.white)
    }

    private func sessionButton(_ symbol: String, label: String,
                               action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(.system(size: 15, weight: .semibold))
                .frame(width: 38, height: 38)
                .background(Color.white.opacity(0.12), in: Circle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
    }

    // MARK: Telemetry

    /// The thin strip from the design reference: fps, frames, audio, core.
    ///
    /// Two figures from that screenshot are deliberately not here, because inventing them would be
    /// worse than leaving them out:
    ///
    ///   - core memory. There is no memory query anywhere in the engine, so the core id takes that
    ///     slot instead, which is at least load-bearing: it is how a routing mistake is spotted.
    ///   - a real audio latency for audio that is being heard. `drain_audio` is not exported
    ///     through UniFFI, so nothing is played out yet. The queued figure below is real, it is
    ///     just measuring a ring nobody is draining.
    private var telemetryStrip: some View {
        Text(host.telemetryLine)
            .font(.system(.caption2, design: .monospaced))
            .foregroundStyle(Color.white.opacity(0.75))
            .lineLimit(1)
            .minimumScaleFactor(0.7)
    }

    /// Always on screen, never behind the toggle.
    ///
    /// On a sideloaded build with no debugger this line is the only thing that explains a failure,
    /// so the player screen keeps it even when the full panel is hidden.
    private var statusLine: some View {
        Text(host.status)
            .font(.system(.caption2, design: .monospaced))
            .foregroundStyle(Color.white.opacity(0.6))
            .lineLimit(2)
            .fixedSize(horizontal: false, vertical: true)
    }
}

// MARK: - The diagnostics, in one place both screens use

/// The full diagnostic block: every line the HUD has ever carried.
///
/// NOTHING HERE MAY BE DELETED, AND THAT IS NOT CAUTION. On a sideloaded build there is no
/// debugger, no console and no crash log, so each of these lines is the only way to tell one
/// failure from another:
///
///   - no GPU line          -> attachMetal failed; the reason is in `status`
///   - GPU but 0 frames     -> renderer up, core or gate not producing
///   - frames but no colour -> compositor or pixel-format problem
///   - frozen counter       -> the frame gate is not releasing
///   - a core line naming a missing dylib -> that system's core never reached the bundle
///   - a running line naming the wrong core -> the extension route is wrong
///   - a control layout line -> two controls were laid out on top of each other
///
/// It moved out of the always-on HUD into a toggle, and moved nowhere else: the same strings, in
/// the same order, reachable from the library and from the player.
struct DiagnosticsPanel: View {
    @ObservedObject var host: EngineHost

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Continuum \(host.buildLine)")
                .font(.system(.caption2, design: .monospaced)).bold()
            Text(host.status)
                .fixedSize(horizontal: false, vertical: true)
            if !host.cores.isEmpty {
                Text(host.cores)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if !host.bios.isEmpty {
                Text(host.bios)
            }
            if !host.gpu.isEmpty {
                Text(host.gpu)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Text(host.frameLine)
            Text(host.audioLine)
            Text(host.inputLine)
            if !host.controlNote.isEmpty {
                Text(host.controlNote)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if !host.libraryStatus.isEmpty {
                Text(host.libraryStatus)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .font(.system(.caption2, design: .monospaced))
        .foregroundStyle(.white)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(10)
        .background(.black.opacity(0.62), in: RoundedRectangle(cornerRadius: 8))
    }
}
