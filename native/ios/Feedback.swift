// Continuum - feedback from testers.
//
// Three ways in:
//   - Settings, the first card: a problem, an idea, or anything else, about the app.
//   - A game's ⋯ menu: the same form about that game, opening on "How it runs" (a rating and what
//     is wrong), with a picture of the game the tester can draw on to point at the problem.
//   - By itself, after a crash: when the app closed unexpectedly last time, the next start asks
//     whether to send a report, with what the app was doing just before it closed.
//
// Every report carries, unless switched off: the app's version, the iPhone model and iOS version,
// the game, its system and emulator, and the activity log, which is every line the app's small
// status text showed, with its time (kept on disk by the engine, feedback.rs, so it survives a
// crash). The log goes as a text file, the picture as an image.
//
// NOTHING LEAVES THE PHONE BY ITSELF. The form opens Apple's Mail screen addressed to
// `FeedbackDestination.email`, or, when no address is set or Mail is not set up, the share menu.
// The tester sees the whole message and taps send. There is no server.

import MessageUI
import PencilKit
import SwiftUI
import UIKit

/// Where feedback goes.
enum FeedbackDestination {
    /// The address the Mail screen is addressed to, given by the owner on 6 October 2026. Empty
    /// would mean no address, and the share menu with the tester choosing who it goes to.
    static let email = "idkplswrk@gmail.com"

    static var hasEmail: Bool { !email.isEmpty }
}

/// What a report is about.
enum FeedbackKind: String, CaseIterable, Hashable {
    case gameReport
    case problem
    case idea
    case other
    case crash

    /// On the selector.
    var title: String {
        switch self {
        case .gameReport: return "How it runs"
        case .problem: return "Problem"
        case .idea: return "Idea"
        case .other: return "Other"
        case .crash: return "Crash"
        }
    }

    /// In the subject line.
    var reportName: String {
        switch self {
        case .gameReport: return "Game report"
        case .problem: return "Problem"
        case .idea: return "Idea"
        case .other: return "Feedback"
        case .crash: return "Crash"
        }
    }
}

/// How well a game runs.
enum GameRating: String, CaseIterable, Hashable {
    case perfect
    case playable
    case problems
    case wontRun

    var title: String {
        switch self {
        case .perfect: return "Perfect"
        case .playable: return "Playable"
        case .problems: return "Problems"
        case .wontRun: return "Won't run"
        }
    }

    /// In the message, where there is room for the whole thought.
    var reportText: String {
        switch self {
        case .perfect: return "Runs perfectly"
        case .playable: return "Playable, a few small issues"
        case .problems: return "Runs, but it's rough"
        case .wontRun: return "Won't start or can't be played"
        }
    }
}

/// What can be wrong with a game.
enum GameIssue: String, CaseIterable, Hashable, Identifiable {
    case picture
    case sound
    case speed
    case controls
    case saves
    case skin
    case freezes
    case cheats

    var id: String { rawValue }

    var title: String {
        switch self {
        case .picture: return "Picture"
        case .sound: return "Sound"
        case .speed: return "Speed"
        case .controls: return "Controls"
        case .saves: return "Saves"
        case .skin: return "Skin"
        case .freezes: return "Freezes or closes"
        case .cheats: return "Cheats"
        }
    }
}

// MARK: - The form

struct FeedbackSheet: View {
    @ObservedObject var host: EngineHost
    /// The game it is about, when opened from a game. Nil from Settings and for a crash.
    let entry: LibraryEntry?
    /// The last session, when this is a crash report.
    let crash: FeedbackUnexpectedClose?
    var onDone: () -> Void

    @ObservedObject private var center = FeedbackCenter.shared

    @State private var kind: FeedbackKind
    @State private var rating: GameRating = .playable
    @State private var issues: Set<GameIssue> = []
    @State private var message = ""
    /// Remembered on this phone, so a tester types it once.
    @AppStorage("continuum.feedback.tester.v1") private var tester = ""
    @State private var includeDetails = true
    @State private var includeLog = true
    @State private var includePicture = true
    /// The game's picture, taken as the form opens, before anything else changes it.
    @State private var picture: UIImage? = nil
    /// The same picture with the tester's drawing on it, once they have drawn.
    @State private var drawn: UIImage? = nil
    @State private var showMarkup = false
    /// What happened to the last send, under the button.
    @State private var resultLine = ""

    init(host: EngineHost, entry: LibraryEntry?, crash: FeedbackUnexpectedClose? = nil,
         onDone: @escaping () -> Void) {
        _host = ObservedObject(wrappedValue: host)
        self.entry = entry
        self.crash = crash
        self.onDone = onDone
        let first: FeedbackKind = crash != nil ? .crash : (entry != nil ? .gameReport : .problem)
        _kind = State(initialValue: first)
    }

    /// The choices on the selector. A crash report has none.
    private var kinds: [FeedbackKind] {
        entry != nil ? [.gameReport, .problem, .idea] : [.problem, .idea, .other]
    }

    private var asksHowItRuns: Bool { kind == .gameReport }
    private var asksWhatIsWrong: Bool {
        entry != nil && (kind == .gameReport || kind == .problem)
    }

    var body: some View {
        NavigationView {
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    if let crash {
                        crashCard(crash)
                    } else {
                        SettingsSection(title: "WHAT'S THIS ABOUT?") {
                            SegmentedChoice(options: kinds, title: { $0.title }, selection: $kind)
                        }
                    }
                    if asksHowItRuns {
                        ratingCard
                    }
                    if asksWhatIsWrong {
                        issuesCard
                    }
                    messageCard
                    attachmentsCard
                    sendBlock
                }
                .padding(.vertical, 16)
            }
            .background(Color.black.ignoresSafeArea())
            .navigationTitle(title)
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
        .sheet(isPresented: $showMarkup) {
            if let picture {
                FeedbackMarkup(image: picture, onDone: { marked in
                    drawn = marked
                    showMarkup = false
                }, onCancel: {
                    showMarkup = false
                })
            }
        }
    }

    private var title: String {
        if crash != nil { return "Crash report" }
        return entry == nil ? "Feedback" : "Feedback about this game"
    }

    // MARK: Cards

    private func crashCard(_ crash: FeedbackUnexpectedClose) -> some View {
        SettingsSection(title: "WHAT HAPPENED") {
            Text(FeedbackCenter.crashSentence(crash))
                .font(.system(size: 14))
                .foregroundStyle(.white)
                .fixedSize(horizontal: false, vertical: true)
            SettingsNote("If you remember what you were doing right before, tell me below. The "
                         + "app's own record of its last few minutes gets attached too.")
        }
    }

    private var ratingCard: some View {
        SettingsSection(title: "HOW'S IT RUNNING?") {
            if let entry {
                Text(entry.name)
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(.white)
                    .lineLimit(2)
            }
            SegmentedChoice(options: GameRating.allCases, title: { $0.title },
                            selection: $rating)
            SettingsNote(rating.reportText + ".")
        }
    }

    private var issuesCard: some View {
        SettingsSection(title: "ANYTHING WRONG? (TAP ALL THAT APPLY)") {
            LazyVGrid(columns: [GridItem(.flexible(), spacing: 8),
                                GridItem(.flexible(), spacing: 8)], spacing: 8) {
                ForEach(GameIssue.allCases) { issue in
                    issueChip(issue)
                }
            }
        }
    }

    private func issueChip(_ issue: GameIssue) -> some View {
        let on = issues.contains(issue)
        return Button {
            if on {
                issues.remove(issue)
            } else {
                issues.insert(issue)
            }
        } label: {
            HStack(spacing: 6) {
                Image(systemName: on ? "checkmark.circle.fill" : "circle")
                Text(issue.title)
                    .lineLimit(1)
                    .minimumScaleFactor(0.8)
                Spacer(minLength: 0)
            }
            .font(.system(size: 14, weight: on ? .semibold : .regular))
            .foregroundStyle(on ? Color.white : ShellPalette.secondaryText)
            .padding(.horizontal, 10)
            .padding(.vertical, 10)
            .background(on ? ShellPalette.accent.opacity(0.35) : ShellPalette.surfaceStrong,
                        in: RoundedRectangle(cornerRadius: 9))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private var messageCard: some View {
        SettingsSection(title: "YOUR MESSAGE") {
            TextField(placeholder, text: $message, axis: .vertical)
                .lineLimit(4...12)
                .font(.system(size: 15))
                .foregroundStyle(.white)
                .padding(10)
                .background(ShellPalette.surfaceStrong, in: RoundedRectangle(cornerRadius: 9))
            TextField("Your name or username (optional)", text: $tester)
                .font(.system(size: 14))
                .foregroundStyle(.white)
                .padding(10)
                .background(ShellPalette.surfaceStrong, in: RoundedRectangle(cornerRadius: 9))
        }
    }

    private var attachmentsCard: some View {
        SettingsSection(title: "SENT WITH IT") {
            toggleRow("Phone and app info", note: detailsNote, isOn: $includeDetails)
            toggleRow("What the app was doing", note: logNote, isOn: $includeLog)
            if let picture {
                toggleRow("A screenshot of the game", note: pictureNote, isOn: $includePicture)
                if includePicture {
                    Image(uiImage: drawn ?? picture)
                        .resizable()
                        .aspectRatio(contentMode: .fit)
                        .frame(maxHeight: 180)
                        .frame(maxWidth: .infinity)
                        .clipShape(RoundedRectangle(cornerRadius: 8))
                    SettingsButton(title: drawn == nil ? "Draw on it" : "Draw on it again",
                                   role: .normal) {
                        showMarkup = true
                    }
                }
            }
        }
    }

    private func toggleRow(_ title: String, note: String, isOn: Binding<Bool>) -> some View {
        Toggle(isOn: isOn) {
            VStack(alignment: .leading, spacing: 3) {
                Text(title)
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(.white)
                Text(note)
                    .font(.system(size: 12))
                    .foregroundStyle(ShellPalette.secondaryText)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .tint(ShellPalette.accent)
    }

    private var sendBlock: some View {
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
            if center.sentCount > 0 {
                SettingsNote(center.sentLine)
            }
        }
        .padding(.horizontal, 16)
    }

    // MARK: Words

    /// Out of the view body on purpose: a long concatenation inside a view builder is the kind of
    /// expression the compiler gives up on, and CI is the only compiler.
    private var detailsNote: String {
        if entry == nil {
            return "Which iPhone and iOS you're on, and the app version. Helps me track it down."
        }
        return "Which iPhone and iOS you're on, the app version, and which game and emulator. "
            + "Helps me track it down."
    }

    private var logNote: String {
        if crash != nil {
            return "The app's own notes from right before it closed. Nothing personal, just what it "
                + "was doing."
        }
        return "The app's own notes on what it's been doing since you opened it. Nothing personal."
    }

    private var pictureNote: String {
        "What the game looked like when you opened this. Draw on it to circle the problem."
    }

    private var sendNote: String {
        if FeedbackDestination.hasEmail {
            return "Opens Mail with everything filled in, going to \(FeedbackDestination.email). "
                + "You'll see it all before it sends. No Mail app? Pick another way to send it to "
                + "that address."
        }
        return "Opens the share menu with everything filled in. Nothing sends until you tap send."
    }

    private var placeholder: String {
        switch kind {
        case .gameReport: return "Anything else? Slowdown, glitches, what you were doing... (optional)"
        case .problem: return "What happened? What were you doing right before?"
        case .idea: return "What would make Continuum better?"
        case .other: return "Your message"
        case .crash: return "What were you doing when it closed? (optional)"
        }
    }

    // MARK: Picture

    /// The game's picture, through the same capture the save slots use: the game alone, without
    /// the skin, in its own shape.
    private func takePicture() async {
        guard entry != nil, host.running,
              let frame = try? host.engine.captureFrame(width: 0, height: 480) else { return }
        let (png, _) = await CapturedCover.pngData(width: frame.width, height: frame.height,
                                                   rgba: frame.rgba)
        guard let png, let image = UIImage(data: png) else { return }
        picture = image
    }

    // MARK: Sending

    private var systemName: String {
        guard let entry, let system = CoreCatalog.system(for: entry) else { return "" }
        return system.displayName
    }

    private func composed() -> FeedbackMessage {
        let record = FeedbackReportRecord(
            kind: kind.reportName,
            message: message,
            tester: tester,
            game: entry?.name ?? crash?.game ?? "",
            system: systemName,
            rating: asksHowItRuns ? rating.reportText : "",
            issues: asksWhatIsWrong ? GameIssue.allCases.filter { issues.contains($0) }.map(\.title)
                : [],
            details: includeDetails ? FeedbackDetails.lines(host: host, entry: entry, crash: crash)
                : []
        )
        return feedbackCompose(report: record)
    }

    private func send() {
        let message = composed()
        let image = includePicture ? (drawn ?? picture) : nil
        let png = image?.pngData()
        let log = includeLog ? FeedbackCenter.logText(crash: crash != nil) : ""
        let sentKind = kind
        let game = entry?.name ?? crash?.game ?? ""
        let finished: (Bool, String) -> Void = { sent, line in
            resultLine = line
            if sent {
                FeedbackCenter.shared.noteSent(kind: sentKind.reportName, game: game)
            }
        }
        if FeedbackDestination.hasEmail,
           FeedbackMail.shared.present(to: FeedbackDestination.email, message: message, png: png,
                                       log: log, onFinish: finished) {
            return
        }
        FeedbackShare.present(message: message, png: png, log: log, onFinish: finished)
    }
}

// MARK: - The details

/// The facts a report carries when "The app's details" is on: plain text, one fact per line, so a
/// reader can see at once which build, phone and game it came from.
enum FeedbackDetails {
    @MainActor
    static func lines(host: EngineHost, entry: LibraryEntry?,
                      crash: FeedbackUnexpectedClose?) -> [String] {
        var lines: [String] = []
        lines.append(EngineHost.versionLabel)
        lines.append("iPhone: \(deviceDescription), iOS \(UIDevice.current.systemVersion)")
        if let crash {
            lines.append("Closed unexpectedly: \(crash.build), session started \(crash.started)")
            if !crash.game.isEmpty { lines.append("Game at the time: \(crash.game)") }
            if !crash.loading.isEmpty { lines.append("Loading at the time: \(crash.loading)") }
        }
        if entry != nil, !host.activeCoreId.isEmpty {
            lines.append("Emulator: \(host.activeCoreId)")
        }
        lines.append("Status: \(host.status)")
        if !host.jitLine.isEmpty { lines.append(host.jitLine) }
        if !host.cores.isEmpty { lines.append(host.cores) }
        if entry != nil {
            if !host.bios.isEmpty { lines.append(host.bios) }
            lines.append(host.frameLine)
        }
        if !host.gpu.isEmpty { lines.append(host.gpu) }
        return lines
    }

    /// The phone's actual name AND its identifier, for example
    /// "iPhone 17 Pro Max (iPhone18,2)".
    ///
    /// The report used to carry only the identifier, and the owner's reaction to reading
    /// "iPhone: iPhone18,2" was reasonable: it is a part number, not an answer to "which phone is
    /// this". But the identifier cannot simply be replaced by the name, for two reasons that both
    /// bite. It is what decides JIT behaviour — `device_has_txm` matches on `iPhone14,2` and up —
    /// so a bug report without it is a bug report that cannot be diagnosed. And the table below
    /// is a hand-written list that goes out of date every September, so a phone released after
    /// this build would otherwise report a wrong name or none.
    ///
    /// Both, therefore: the name when it is known, the identifier always. A phone missing from the
    /// table reports exactly what it used to, which is the worst case and is no worse than before.
    static var deviceDescription: String {
        let id = deviceModel
        guard let name = Self.deviceNames[id] else { return id }
        return "\(name) (\(id))"
    }

    /// Identifier to marketing name. Verified against theapplewiki's per-model pages rather than
    /// guessed, because a confidently wrong phone name in a bug report is worse than a part
    /// number. Anything absent falls back to the identifier; see `deviceDescription`.
    private static let deviceNames: [String: String] = [
        // iPhone 18 family
        "iPhone19,2": "iPhone 18 Pro",
        "iPhone19,3": "iPhone 18 Pro Max",
        "iPhone19,7": "iPhone 18 Pro Max",
        // iPhone 17 family and the Air
        "iPhone18,1": "iPhone 17 Pro",
        "iPhone18,2": "iPhone 17 Pro Max",
        "iPhone18,3": "iPhone 17",
        "iPhone18,4": "iPhone Air",
        // iPhone 16 family
        "iPhone17,1": "iPhone 16 Pro",
        "iPhone17,2": "iPhone 16 Pro Max",
        "iPhone17,3": "iPhone 16",
        "iPhone17,4": "iPhone 16 Plus",
        "iPhone17,5": "iPhone 16e",
        // iPhone 15 family
        "iPhone16,1": "iPhone 15 Pro",
        "iPhone16,2": "iPhone 15 Pro Max",
        "iPhone15,4": "iPhone 15",
        "iPhone15,5": "iPhone 15 Plus",
        // iPhone 14 family
        "iPhone15,2": "iPhone 14 Pro",
        "iPhone15,3": "iPhone 14 Pro Max",
        "iPhone14,7": "iPhone 14",
        "iPhone14,8": "iPhone 14 Plus",
        // iPhone 13 family, and the SE that shipped alongside it
        "iPhone14,2": "iPhone 13 Pro",
        "iPhone14,3": "iPhone 13 Pro Max",
        "iPhone14,4": "iPhone 13 mini",
        "iPhone14,5": "iPhone 13",
        "iPhone14,6": "iPhone SE (3rd generation)",
        // iPhone 12 family — the oldest generation that matters here, since this is also roughly
        // where the JIT rules start caring about the model.
        "iPhone13,1": "iPhone 12 mini",
        "iPhone13,2": "iPhone 12",
        "iPhone13,3": "iPhone 12 Pro",
        "iPhone13,4": "iPhone 12 Pro Max",
    ]

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

// MARK: - The log, the crash marker and what has been sent

/// The time on each log line.
enum FeedbackStamp {
    private static let clockFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "HH:mm:ss"
        return formatter
    }()

    private static let dayFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        return formatter
    }()

    static func now() -> String { clockFormatter.string(from: Date()) }
    static func longNow() -> String { dayFormatter.string(from: Date()) }
    static func day(_ date: Date) -> String { dayFormatter.string(from: date) }
}

/// The app's side of feedback.rs: starts the log and the session marker, follows the app on and
/// off screen, offers the crash report, and counts what has been sent from this phone.
@MainActor
final class FeedbackCenter: ObservableObject {
    static let shared = FeedbackCenter()

    /// The last session, when it closed unexpectedly.
    @Published private(set) var unexpectedClose: FeedbackUnexpectedClose?
    /// The "send a report?" question on screen.
    @Published var crashPromptShown = false
    /// The crash report form on screen.
    @Published var crashReportOpen = false
    @Published private(set) var sentCount: Int
    @Published private(set) var lastSent: Date?

    private var observers: [NSObjectProtocol] = []
    private var started = false

    private static let sentCountKey = "continuum.feedback.sentCount.v1"
    private static let lastSentKey = "continuum.feedback.lastSent.v1"

    private init() {
        let defaults = UserDefaults.standard
        sentCount = defaults.integer(forKey: Self.sentCountKey)
        lastSent = defaults.object(forKey: Self.lastSentKey) as? Date
    }

    /// Once, as the app starts, before anything else can happen.
    func start() {
        guard !started else { return }
        started = true
        guard let dir = Self.directory() else { return }
        feedbackLogOpen(dir: dir.path)
        let closed = feedbackSessionOpen(dir: dir.path, build: EngineHost.versionLabel,
                                         started: FeedbackStamp.longNow())
        unexpectedClose = closed
        crashPromptShown = closed != nil
        if let closed {
            Self.record("the last session (\(closed.build)) closed unexpectedly")
        }
        // Off screen as soon as the app is not in front: the app switcher, a call, Control
        // Centre. Being killed after that is not a crash; stopping while on screen is.
        let centre = NotificationCenter.default
        observers = [
            centre.addObserver(forName: UIApplication.willResignActiveNotification,
                               object: nil, queue: .main) { _ in
                feedbackSessionOnScreen(onScreen: false)
            },
            centre.addObserver(forName: UIApplication.didBecomeActiveNotification,
                               object: nil, queue: .main) { _ in
                feedbackSessionOnScreen(onScreen: true)
            },
        ]
    }

    /// One status line into the log. Called for every change of the status text.
    nonisolated static func record(_ line: String) {
        feedbackLogAdd(stamp: FeedbackStamp.now(), line: line)
    }

    /// The log to send: this session's, or for a crash report the one that ended in the crash.
    nonisolated static func logText(crash: Bool) -> String {
        crash ? feedbackLogPreviousText(limit: 300) : feedbackLogText(limit: 300)
    }

    func noteSent(kind: String, game: String) {
        sentCount += 1
        lastSent = Date()
        UserDefaults.standard.set(sentCount, forKey: Self.sentCountKey)
        UserDefaults.standard.set(lastSent, forKey: Self.lastSentKey)
        Self.record("feedback sent: \(kind)\(game.isEmpty ? "" : " about \(game)")")
    }

    var sentLine: String {
        let times = sentCount == 1 ? "1 report" : "\(sentCount) reports"
        guard let lastSent else { return "Sent from this iPhone: \(times)." }
        return "Sent from this iPhone: \(times), the last on \(FeedbackStamp.day(lastSent))."
    }

    /// One sentence about how the last session ended, for the question and the report form.
    nonisolated static func crashSentence(_ crash: FeedbackUnexpectedClose) -> String {
        var line = crash.game.isEmpty ? "Looks like Continuum crashed last time."
            : "Looks like Continuum crashed last time, while you were playing \(crash.game)."
        if !crash.loading.isEmpty {
            line += " It was loading a save right then, so that save won't load by itself again "
                + "(it's still there if you want to try it)."
        }
        return line
    }

    var crashPromptText: String {
        guard let unexpectedClose else { return "" }
        return Self.crashSentence(unexpectedClose)
            + " Sorry about that. Want to send a quick report? It really helps me fix it."
    }

    private static func directory() -> URL? {
        guard let base = FileManager.default.urls(for: .applicationSupportDirectory,
                                                  in: .userDomainMask).first else { return nil }
        let dir = base.appendingPathComponent("Feedback", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    /// The log as a file to attach, in a folder of its own that the system clears.
    nonisolated static func logFile(_ text: String) -> URL? {
        guard !text.isEmpty else { return nil }
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("ContinuumFeedback-\(UUID().uuidString)", isDirectory: true)
        do {
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            let url = dir.appendingPathComponent("Continuum activity.txt")
            try text.write(to: url, atomically: true, encoding: .utf8)
            return url
        } catch {
            return nil
        }
    }
}

/// The question after a crash, and the crash report form, over the library.
struct CrashReportPrompt: ViewModifier {
    @ObservedObject var host: EngineHost
    @ObservedObject private var center = FeedbackCenter.shared

    func body(content: Content) -> some View {
        content
            .alert("Continuum crashed", isPresented: $center.crashPromptShown) {
                Button("Send a report") {
                    // After the question has gone, so the form is not presented over it.
                    Task { @MainActor in
                        try? await Task.sleep(nanoseconds: 400_000_000)
                        center.crashReportOpen = true
                    }
                }
                Button("No thanks", role: .cancel) {}
            } message: {
                Text(center.crashPromptText)
            }
            .sheet(isPresented: $center.crashReportOpen) {
                FeedbackSheet(host: host, entry: nil, crash: center.unexpectedClose) {
                    center.crashReportOpen = false
                }
            }
    }
}

// MARK: - Sending

/// Apple's Mail screen, addressed and filled in. Retained here, because its delegate is weak.
final class FeedbackMail: NSObject, MFMailComposeViewControllerDelegate {
    static let shared = FeedbackMail()

    private var onFinish: ((Bool, String) -> Void)?

    /// False when Mail is not set up on this phone or there is nothing to show it on, so the
    /// caller can use the share menu instead.
    @MainActor
    func present(to address: String, message: FeedbackMessage, png: Data?, log: String,
                 onFinish: @escaping (Bool, String) -> Void) -> Bool {
        guard MFMailComposeViewController.canSendMail(),
              let presenter = EngineHost.topmostViewController() else { return false }
        let mail = MFMailComposeViewController()
        mail.mailComposeDelegate = self
        mail.setToRecipients([address])
        mail.setSubject(message.subject)
        mail.setMessageBody(message.body, isHTML: false)
        if let png {
            mail.addAttachmentData(png, mimeType: "image/png", fileName: "Continuum screen.png")
        }
        if !log.isEmpty, let data = log.data(using: .utf8) {
            mail.addAttachmentData(data, mimeType: "text/plain",
                                   fileName: "Continuum activity.txt")
        }
        self.onFinish = onFinish
        presenter.present(mail, animated: true)
        return true
    }

    func mailComposeController(_ controller: MFMailComposeViewController,
                               didFinishWith result: MFMailComposeResult, error: Error?) {
        let sent: Bool
        let line: String
        switch result {
        case .sent:
            sent = true
            line = "Sent. Thanks, seriously, this helps a lot."
        case .saved:
            sent = false
            line = "Saved to your Mail drafts. It hasn't been sent yet."
        case .cancelled:
            sent = false
            line = "Not sent."
        case .failed:
            sent = false
            line = "Mail could not send it: " + (error?.localizedDescription ?? "no reason given")
        @unknown default:
            sent = false
            line = "Mail closed."
        }
        Task { @MainActor in
            controller.dismiss(animated: true)
            self.onFinish?(sent, line)
            self.onFinish = nil
        }
    }
}

/// The share menu with the message, the picture and the log, for any app the tester picks.
@MainActor
enum FeedbackShare {
    static func present(message: FeedbackMessage, png: Data?, log: String,
                        onFinish: @escaping (Bool, String) -> Void) {
        guard let presenter = EngineHost.topmostViewController() else {
            onFinish(false, "There is no screen to show the share menu on.")
            return
        }
        // Mail is not set up on this phone, so the tester picks another app: the address goes at
        // the top, so they know where to send it.
        let to = FeedbackDestination.hasEmail
            ? "Please send this to \(FeedbackDestination.email)\n\n" : ""
        var items: [Any] = [to + message.subject + "\n\n" + message.body]
        if let png, let image = UIImage(data: png) {
            items.append(image)
        }
        if let file = FeedbackCenter.logFile(log) {
            items.append(file)
        }
        let sheet = UIActivityViewController(activityItems: items, applicationActivities: nil)
        sheet.completionWithItemsHandler = { _, completed, _, _ in
            Task { @MainActor in
                onFinish(completed, completed ? "Sent. Thanks, seriously, this helps a lot." : "Not sent.")
            }
        }
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

// MARK: - Drawing on the picture

/// The game's picture with a red pen over it, to circle or point at the problem.
struct FeedbackMarkup: View {
    let image: UIImage
    var onDone: (UIImage) -> Void
    var onCancel: () -> Void

    @State private var canvas = PKCanvasView()

    var body: some View {
        NavigationView {
            VStack(spacing: 14) {
                SettingsNote("Draw on it with your finger to show where the problem is.")
                    .padding(.horizontal, 16)
                Image(uiImage: image)
                    .resizable()
                    .aspectRatio(contentMode: .fit)
                    // The canvas sits exactly on the picture, so a line lands where it was drawn.
                    .overlay(MarkupCanvas(canvas: canvas))
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .padding(.horizontal, 16)
                SettingsButton(title: "Clear the drawing", role: .normal) {
                    canvas.drawing = PKDrawing()
                }
                .padding(.horizontal, 16)
            }
            .padding(.vertical, 14)
            .background(Color.black.ignoresSafeArea())
            .navigationTitle("Draw on it")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { onCancel() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { onDone(flattened()) }
                }
            }
        }
        .navigationViewStyle(.stack)
        .preferredColorScheme(.dark)
    }

    /// The picture with the drawing on it, at the picture's own size.
    private func flattened() -> UIImage {
        let bounds = canvas.bounds
        guard bounds.width > 0, bounds.height > 0, !canvas.drawing.bounds.isEmpty else {
            return image
        }
        let ink = canvas.drawing.image(from: bounds, scale: UIScreen.main.scale)
        let format = UIGraphicsImageRendererFormat()
        format.scale = image.scale
        let rect = CGRect(origin: .zero, size: image.size)
        return UIGraphicsImageRenderer(size: image.size, format: format).image { _ in
            image.draw(in: rect)
            ink.draw(in: rect)
        }
    }
}

/// PencilKit's canvas, finger drawing on, a thick red pen, see-through.
struct MarkupCanvas: UIViewRepresentable {
    let canvas: PKCanvasView

    func makeUIView(context: Context) -> PKCanvasView {
        canvas.drawingPolicy = .anyInput
        canvas.tool = PKInkingTool(.pen, color: .systemRed, width: 8)
        canvas.backgroundColor = .clear
        canvas.isOpaque = false
        return canvas
    }

    func updateUIView(_ uiView: PKCanvasView, context: Context) {}
}

// MARK: - In Settings

/// The first card in Settings.
struct FeedbackSettingsSection: View {
    @ObservedObject var host: EngineHost
    @ObservedObject private var center = FeedbackCenter.shared
    @State private var showForm = false

    var body: some View {
        SettingsSection(title: "FEEDBACK") {
            SettingsNote("Something broken, or got an idea? Let me know here. If it's about a "
                         + "specific game, open the game and tap ⋯ then Send feedback about this "
                         + "game: you can rate it and draw on a screenshot. And if the app ever "
                         + "crashes, it'll offer to send a report next time you open it.")
            SettingsButton(title: "Send feedback", role: .normal) {
                showForm = true
            }
            if center.sentCount > 0 {
                SettingsNote(center.sentLine)
            }
        }
        .sheet(isPresented: $showForm) {
            FeedbackSheet(host: host, entry: nil) {
                showForm = false
            }
        }
    }
}
