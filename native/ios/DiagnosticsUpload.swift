import Foundation
import SwiftUI
import UIKit

/// "Upload a diagnostic file" (Settings, Technical details): packs the telemetry (every
/// emulator's log lines, the crash catcher's report, the frame watchdog's samples, per-second
/// frame and sound figures, the activity logs, and the last 30 seconds of the game's sound as a
/// WAV; crates/emulator-bridge/src/telemetry.rs) into one .zip, uploads it to a temporary file
/// host, and shows the link to copy. The owner pastes the link into the chat and it is read and
/// listened to off the phone (owner, 25 October 2026).
///
/// litterbox.catbox.moe: no account, keeps a file 72 hours, then deletes it.
@MainActor
final class DiagnosticsUpload: ObservableObject {
    static let shared = DiagnosticsUpload()

    @Published private(set) var busy = false
    @Published private(set) var line = ""
    @Published private(set) var link = UserDefaults.standard.string(forKey: DiagnosticsUpload.linkKey) ?? ""

    private static let linkKey = "continuum.diagnostics.link.v1"
    private static let endpoint = URL(string: "https://litterbox.catbox.moe/resources/internals/api.php")!

    /// "iPhone18,2, iOS 27.0.1": for the first line of the core log and for about.txt.
    nonisolated static func phoneLine() -> String {
        var info = utsname()
        uname(&info)
        let machine = withUnsafeBytes(of: &info.machine) { raw in
            String(decoding: raw.prefix(while: { $0 != 0 }), as: UTF8.self)
        }
        let v = ProcessInfo.processInfo.operatingSystemVersion
        return "\(machine), iOS \(v.majorVersion).\(v.minorVersion).\(v.patchVersion)"
    }

    func upload(about: String) {
        guard !busy else { return }
        busy = true
        line = "Packing it up..."
        let out = FileManager.default.temporaryDirectory.path
        Task.detached(priority: .userInitiated) {
            let path = telemetryPackage(outDir: out, about: about)
            if path.hasPrefix("error: ") {
                await self.finish(link: nil, problem: "Couldn't pack it: " + path.dropFirst(7))
                return
            }
            await self.uploading()
            do {
                let url = try await Self.send(URL(fileURLWithPath: path))
                await self.finish(link: url, problem: nil)
            } catch {
                await self.finish(link: nil, problem: "The upload didn't work: \(error.localizedDescription)")
            }
            try? FileManager.default.removeItem(atPath: path)
        }
    }

    func copied() {
        UIPasteboard.general.string = link
        line = "Copied. Paste it into the chat."
    }

    private func uploading() {
        line = "Uploading..."
    }

    private func finish(link url: String?, problem: String?) {
        busy = false
        if let url {
            link = url
            UserDefaults.standard.set(url, forKey: Self.linkKey)
            UIPasteboard.general.string = url
            line = "Done, and the link is copied. Paste it into the chat."
            FeedbackCenter.record("diagnostics uploaded: \(url)")
        } else {
            line = problem ?? "Something went wrong."
            FeedbackCenter.record("diagnostics upload failed: \(line)")
        }
    }

    nonisolated private static func send(_ file: URL) async throws -> String {
        var request = URLRequest(url: endpoint)
        request.httpMethod = "POST"
        request.timeoutInterval = 180
        let boundary = "continuum-\(UUID().uuidString)"
        request.setValue("multipart/form-data; boundary=\(boundary)", forHTTPHeaderField: "Content-Type")
        var body = Data()
        func text(_ s: String) { body.append(Data(s.utf8)) }
        for (name, value) in [("reqtype", "fileupload"), ("time", "72h")] {
            text("--\(boundary)\r\nContent-Disposition: form-data; name=\"\(name)\"\r\n\r\n\(value)\r\n")
        }
        text("--\(boundary)\r\nContent-Disposition: form-data; name=\"fileToUpload\"; "
            + "filename=\"continuum-diagnostics.zip\"\r\nContent-Type: application/zip\r\n\r\n")
        body.append(try Data(contentsOf: file))
        text("\r\n--\(boundary)--\r\n")
        let (data, response) = try await URLSession.shared.upload(for: request, from: body)
        let answer = String(decoding: data, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
        guard (response as? HTTPURLResponse)?.statusCode == 200, answer.hasPrefix("https://") else {
            throw NSError(domain: "Continuum", code: 1, userInfo: [
                NSLocalizedDescriptionKey: answer.isEmpty ? "the upload site didn't answer" : String(answer.prefix(120)),
            ])
        }
        return answer
    }
}

/// The rows under Technical details.
struct DiagnosticsUploadRows: View {
    @ObservedObject var host: EngineHost
    @ObservedObject private var upload = DiagnosticsUpload.shared

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            SettingsButton(title: upload.busy ? "Uploading..." : "Upload a diagnostic file", role: .normal) {
                let jit = BuiltInJIT.shared
                upload.upload(about: "\(EngineHost.versionLabel)\n\(DiagnosticsUpload.phoneLine())\n"
                    + "status: \(host.status)\n"
                    + "JIT method: \(jit.method.label), pairing file \(jit.pairingPresent ? "imported" : "missing"), "
                    + "turn on at game start \(jit.onGameStart ? "on" : "off"), "
                    + "use JIT when available \(host.useJitWhenAvailable ? "on" : "off")\n"
                    + "thermal \(ProcessInfo.processInfo.thermalState.rawValue) (0 cool .. 3 critical), "
                    + "Low Power Mode \(ProcessInfo.processInfo.isLowPowerModeEnabled ? "on" : "off")")
            }
            .disabled(upload.busy)
            SettingsNote("Sends the logs, any crash and the last 30 seconds of game sound. The link lasts 3 days.")
            if !upload.line.isEmpty {
                SettingsNote(upload.line)
            }
            if !upload.link.isEmpty {
                SettingsReadout(label: "Last link", value: upload.link)
                SettingsButton(title: "Copy the link", role: .normal) {
                    upload.copied()
                }
            }
        }
    }
}
