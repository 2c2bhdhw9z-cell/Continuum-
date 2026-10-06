// Continuum - feedback from players: one form, reached from two places.
//
//   - Settings, at the top: "Send feedback", about the app in general.
//   - A game's ⋯ menu: "Send feedback about this game...", which also attaches the game, its system,
//     its emulator and a picture of the screen, because that is where a tester notices a problem.
//
// NOTHING LEAVES THE PHONE BY ITSELF. The form writes a message and hands it to Apple's Mail
// screen, addressed to `FeedbackDestination.email`, or, when no address is set or Mail is not set
// up on this phone, to the share sheet, where the player picks Mail, Messages or any other app.
// Either way the player sees the whole message and taps send themselves. There is no server.

import MessageUI
import SwiftUI
import UIKit

/// Where feedback goes.
enum FeedbackDestination {
    /// The address the Mail screen is addressed to. Empty: no address yet, so the share sheet is
    /// used and the player chooses who it goes to.
    static let email = ""

    static var hasEmail: Bool { !email.isEmpty }
}

/// What the message is about. Becomes the subject line.
enum FeedbackKind: String, CaseIterable, Hashable {
    case problem
    case idea
    case other

    var title: String {
        switch self {
        case .problem: return "A problem"
        case .idea: return "An idea"
        case .other: return "Something else"
        }
    }
}

/// The form.
struct FeedbackSheet: View {
    @ObservedObject var host: EngineHost
    /// The game it is about, when it was opened from a game. Nil from Settings.
    let entry: LibraryEntry?
    var onDone: () -> Void

    @State private var kind: FeedbackKind = .problem
    @State private var message = ""
    @State private var includeDetails = true
    @State private var includePicture = true
    /// The game's screen, taken when the form opened, before anything else changes it.
    @State private var picture: Data? = nil
    @State private var preview: UIImage? = nil
    /// What happened to the last send, under the button.
    @State private var resultLine = ""

    var body: some View {
        NavigationView {
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    SettingsSection(title: "WHAT IS IT ABOUT") {
                        SegmentedChoice(options: FeedbackKind.allCases,
                                        title: { $0.title },
                                        selection: $kind)
                        TextField(placeholder, text: $message, axis: .vertical)
                            .lineLimit(4...12)
                            .font(.system(size: 15))
                            .foregroundStyle(.white)
                            .padding(10)
                            .background(ShellPalette.surfaceStrong,
                                        in: RoundedRectangle(cornerRadius: 9))
                    }

                    SettingsSection(title: "ADDED TO THE MESSAGE") {
                        Toggle(isOn: $includeDetails) {
                            VStack(alignment: .leading, spacing: 3) {
                                Text("The app's details")
                                    .font(.system(size: 15, weight: .semibold))
                                    .foregroundStyle(.white)
                                Text(detailsNote)
                                    .font(.system(size: 12))
                                    .foregroundStyle(ShellPalette.secondaryText)
                            }
                        }
                        .tint(ShellPalette.accent)

                        if let preview {
                            Toggle(isOn: $includePicture) {
                                Text("A picture of the game")
                                    .font(.system(size: 15, weight: .semibold))
                                    .foregroundStyle(.white)
                            }
                            .tint(ShellPalette.accent)
                            if includePicture {
                                Image(uiImage: preview)
                                    .resizable()
                                    .aspectRatio(contentMode: .fit)
                                    .frame(maxHeight: 160)
                                    .frame(maxWidth: .infinity)
                                    .clipShape(RoundedRectangle(cornerRadius: 8))
                            }
                        }
                    }

                    VStack(alignment: .leading, spacing: 8) {
                        SettingsButton(title: FeedbackDestination.hasEmail ? "Send" : "Send...",
                                       role: .normal) {
                            send()
                        }
                        SettingsNote(sendNote)
                        if !resultLine.isEmpty {
                            Text(resultLine)
                                .font(.system(size: 13, weight: .semibold))
                                .foregroundStyle(.white)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }
                    .padding(.horizontal, 16)
                }
                .padding(.vertical, 16)
            }
            .background(Color.black.ignoresSafeArea())
            .navigationTitle(entry == nil ? "Feedback" : "Feedback about this game")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close") { onDone() }
                }
            }
        }
        .navigationViewStyle(.stack)
        .preferredColorScheme(.dark)
        .task {
            await takePicture()
        }
    }

    /// Out of the view body on purpose: a long concatenation inside a view builder is the kind of
    /// expression the compiler gives up on, and CI is the only compiler.
    private var detailsNote: String {
        if entry == nil {
            return "The app's version, the iPhone model and iOS version, and the app's status text."
        }
        return "The app's version, the iPhone model and iOS version, the game, its system and "
            + "emulator, and the app's status text."
    }

    private var sendNote: String {
        if FeedbackDestination.hasEmail {
            return "Opens Mail with the message ready. You see all of it before it goes. If Mail is "
                + "not set up on this iPhone, you can pick another app instead."
        }
        return "Opens the share menu with the message ready: pick Mail, Messages or another app, "
            + "and send it to whoever gave you Continuum. Nothing is sent until you do."
    }

    private var placeholder: String {
        switch kind {
        case .problem: return "What happened, and what were you doing just before?"
        case .idea: return "What would you like Continuum to do?"
        case .other: return "Your message"
        }
    }

    /// The game's picture, through the same capture the save slots use: the game alone, without
    /// the skin, in its own shape.
    private func takePicture() async {
        guard entry != nil, host.running,
              let frame = try? host.engine.captureFrame(width: 0, height: 480) else { return }
        let (png, _) = await CapturedCover.pngData(width: frame.width, height: frame.height,
                                                   rgba: frame.rgba)
        guard let png, let image = UIImage(data: png) else { return }
        picture = png
        preview = image
    }

    private var subject: String {
        var line = "Continuum feedback: \(kind.title)"
        if let entry {
            line += " - \(entry.name)"
        }
        return line
    }

    /// The message as the player will see it in Mail or the app they pick.
    private var composedMessage: String {
        var parts: [String] = []
        let typed = message.trimmingCharacters(in: .whitespacesAndNewlines)
        parts.append(typed.isEmpty ? "(no message typed)" : typed)
        if includeDetails {
            parts.append("--\n" + FeedbackDetails.text(host: host, entry: entry))
        }
        return parts.joined(separator: "\n\n")
    }

    private func send() {
        let attachment = (includePicture && preview != nil) ? picture : nil
        if FeedbackDestination.hasEmail,
           FeedbackMail.shared.present(to: FeedbackDestination.email, subject: subject,
                                       body: composedMessage, png: attachment,
                                       onFinish: { resultLine = $0 }) {
            return
        }
        FeedbackShare.present(text: subject + "\n\n" + composedMessage, png: attachment)
        resultLine = "Pick how to send it. Thank you."
    }
}

// MARK: - The details

/// The facts a message carries when "The app's details" is on: plain text, one fact per line, so a
/// reader can see at once which build, phone and game it came from.
enum FeedbackDetails {
    @MainActor
    static func text(host: EngineHost, entry: LibraryEntry?) -> String {
        var lines: [String] = []
        lines.append(EngineHost.versionLabel)
        lines.append("iPhone: \(deviceModel), iOS \(UIDevice.current.systemVersion)")
        if let entry {
            var game = "Game: \(entry.name)"
            if let system = CoreCatalog.system(for: entry) {
                game += ", \(system.displayName)"
            }
            if !host.activeCoreId.isEmpty {
                game += ", emulator \(host.activeCoreId)"
            }
            lines.append(game)
        }
        lines.append("Status: \(host.status)")
        if !host.cores.isEmpty { lines.append(host.cores) }
        if entry != nil {
            if !host.bios.isEmpty { lines.append(host.bios) }
            lines.append(host.frameLine)
        }
        if !host.gpu.isEmpty { lines.append(host.gpu) }
        return lines.joined(separator: "\n")
    }

    /// The model identifier, for example "iPhone18,2", which names the exact phone where
    /// `UIDevice.model` only says "iPhone".
    static var deviceModel: String {
        var info = utsname()
        uname(&info)
        let machine = Mirror(reflecting: info.machine).children.compactMap { child -> Character? in
            guard let value = child.value as? Int8, value != 0 else { return nil }
            return Character(UnicodeScalar(UInt8(bitPattern: value)))
        }
        return machine.isEmpty ? "unknown" : String(machine)
    }
}

// MARK: - Sending

/// Apple's Mail screen, addressed and filled in. Retained here, because its delegate is weak.
final class FeedbackMail: NSObject, MFMailComposeViewControllerDelegate {
    static let shared = FeedbackMail()

    private var onFinish: ((String) -> Void)?

    /// False when Mail is not set up on this phone or there is nothing to show it on, so the
    /// caller can use the share sheet instead.
    @MainActor
    func present(to address: String, subject: String, body: String, png: Data?,
                 onFinish: @escaping (String) -> Void) -> Bool {
        guard MFMailComposeViewController.canSendMail(),
              let presenter = EngineHost.topmostViewController() else { return false }
        let mail = MFMailComposeViewController()
        mail.mailComposeDelegate = self
        mail.setToRecipients([address])
        mail.setSubject(subject)
        mail.setMessageBody(body, isHTML: false)
        if let png {
            mail.addAttachmentData(png, mimeType: "image/png", fileName: "Continuum screen.png")
        }
        self.onFinish = onFinish
        presenter.present(mail, animated: true)
        return true
    }

    func mailComposeController(_ controller: MFMailComposeViewController,
                               didFinishWith result: MFMailComposeResult, error: Error?) {
        let line: String
        switch result {
        case .sent: line = "Sent. Thank you."
        case .saved: line = "Saved in Mail's drafts, not sent yet."
        case .cancelled: line = "Not sent."
        case .failed: line = "Mail could not send it: "
            + (error?.localizedDescription ?? "it gave no reason") + "."
        @unknown default: line = "Mail closed."
        }
        Task { @MainActor in
            controller.dismiss(animated: true)
            self.onFinish?(line)
            self.onFinish = nil
        }
    }
}

/// The share sheet with the message and the picture, for any app the player picks.
@MainActor
enum FeedbackShare {
    static func present(text: String, png: Data?) {
        guard let presenter = EngineHost.topmostViewController() else { return }
        var items: [Any] = [text]
        if let png, let image = UIImage(data: png) {
            items.append(image)
        }
        let sheet = UIActivityViewController(activityItems: items, applicationActivities: nil)
        // iPad needs an anchor or it traps; the centre of the presenting view is honest enough.
        if let popover = sheet.popoverPresentationController {
            popover.sourceView = presenter.view
            popover.sourceRect = CGRect(x: presenter.view.bounds.midX,
                                        y: presenter.view.bounds.midY, width: 1, height: 1)
            popover.permittedArrowDirections = []
        }
        presenter.present(sheet, animated: true)
    }
}

// MARK: - In Settings

/// The first card in Settings.
struct FeedbackSettingsSection: View {
    @ObservedObject var host: EngineHost
    @State private var showForm = false

    var body: some View {
        SettingsSection(title: "FEEDBACK") {
            SettingsNote("Found a problem, or have an idea? Send it from here. From inside a game, "
                         + "the ⋯ menu has the same form with that game and a picture of it "
                         + "attached.")
            SettingsButton(title: "Send feedback", role: .normal) {
                showForm = true
            }
        }
        .sheet(isPresented: $showForm) {
            FeedbackSheet(host: host, entry: nil) {
                showForm = false
            }
        }
    }
}
