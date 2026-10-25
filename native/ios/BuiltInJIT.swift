// JIT, every way it can be turned on (owner, 25 October 2026: "It should be as simple as Manic, if not
// simpler", and "any way JIT can be set up, Continuum can do that method").
//
// THE WAYS, in Settings, Technical details, JIT method:
//
// - Built in (the default once a pairing file is in): Continuum turns JIT on by itself, from the phone,
//   with the pairing file and LocalDevVPN. Import the pairing file once; from then on JIT comes on
//   when a game starts. This is what Manic and DolphiniOS call their built-in debugger. It works
//   through Continuum's own small helper (JITHelper/ContinuumJITHelper.swift, a separate process,
//   because a process that attaches a debugger to itself deadlocks) and StikJIT, the StikDebug team's
//   library inside it.
// - StikDebug: Continuum opens StikDebug and asks it to attach (the old button).
// - Another JIT app: Continuum waits for whatever attaches (StikDebug's own list, SideStore, a
//   computer), and finishes the job itself.
//
// Whichever way it attaches, on an iPhone with TXM the code memory then has to be prepared through
// the debugger with the universal script; the engine does that (`EngineHost.refreshJit`,
// crates/emulator-bridge/src/jit26.c) once it knows the right script is attached.
//
// The pairing file is kept in Documents/StikJIT/pairingFile.plist, where StikJIT's guide says apps
// keep it (so it can also be dropped in from Files), and a copy in the Keychain, which iOS keeps when
// the app is deleted: the owner deletes the app before every install, and must not have to import the
// pairing file every time. It is never logged and never synced.

import Foundation
import Security
import SwiftUI
import UIKit
import UniformTypeIdentifiers

/// How JIT is turned on. Stored as its raw value.
enum JitMethod: String, CaseIterable, Identifiable {
    case builtIn
    case stikDebug
    case otherApp

    var id: String { rawValue }

    var label: String {
        switch self {
        case .builtIn: return "Built in"
        case .stikDebug: return "StikDebug"
        case .otherApp: return "Another JIT app"
        }
    }

    var note: String {
        switch self {
        case .builtIn: return "Continuum turns JIT on by itself. Needs the pairing file and LocalDevVPN."
        case .stikDebug: return "Continuum asks StikDebug to turn JIT on."
        case .otherApp: return "Turn JIT on with any JIT app. Continuum finishes the rest."
        }
    }
}

/// The pairing file: Documents/StikJIT/pairingFile.plist, with a Keychain copy that outlives the app.
enum JitPairingFile {
    static let keychainService = "dev.continuum.jit"
    static let keychainAccount = "pairingFile"

    static var url: URL? {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first?
            .appendingPathComponent("StikJIT", isDirectory: true)
            .appendingPathComponent("pairingFile.plist")
    }

    /// The pairing file's bytes: the file, or the Keychain copy put back as the file.
    static func data() -> Data? {
        if let url, let data = try? Data(contentsOf: url), !data.isEmpty {
            if keychainData() == nil { saveToKeychain(data) }
            return data
        }
        guard let kept = keychainData() else { return nil }
        writeFile(kept)
        return kept
    }

    static var isPresent: Bool { data() != nil }

    /// Stores a file the player picked (a security-scoped URL from the file picker).
    static func importFile(from source: URL) throws {
        let scoped = source.startAccessingSecurityScopedResource()
        defer { if scoped { source.stopAccessingSecurityScopedResource() } }
        let data = try Data(contentsOf: source)
        guard looksLikePairingFile(data) else {
            throw NSError(domain: "Continuum", code: 1, userInfo: [NSLocalizedDescriptionKey:
                "That isn't a pairing file. Export one from StikPair (or idevice_pair) and pick that."])
        }
        writeFile(data)
        saveToKeychain(data)
    }

    static func remove() {
        if let url { try? FileManager.default.removeItem(at: url) }
        SecItemDelete(query() as CFDictionary)
    }

    /// A pairing file is a property list (XML or binary). Nothing more is checked here: StikJIT
    /// reads it and says what is wrong with it if anything is.
    static func looksLikePairingFile(_ data: Data) -> Bool {
        (try? PropertyListSerialization.propertyList(from: data, format: nil)) != nil
    }

    private static func writeFile(_ data: Data) {
        guard let url else { return }
        try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? data.write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
    }

    private static func query() -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: keychainService,
         kSecAttrAccount as String: keychainAccount]
    }

    private static func keychainData() -> Data? {
        var q = query()
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var out: CFTypeRef?
        guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess else { return nil }
        return out as? Data
    }

    private static func saveToKeychain(_ data: Data) {
        SecItemDelete(query() as CFDictionary)
        var q = query()
        q[kSecValueData as String] = data
        q[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        SecItemAdd(q as CFDictionary, nil)
    }
}

// MARK: - Starting the helper

/// The private NSExtension calls that start an app extension of the app's own and talk to it (the
/// ExtensionFoundation API LiveContainer and others use for the same job).
@objc private protocol ExtensionClassCalls {
    @objc(extensionWithIdentifier:error:)
    func `extension`(withIdentifier identifier: String) throws -> AnyObject
}

@objc private protocol ExtensionCalls {
    @objc(beginExtensionRequestWithInputItems:completion:)
    func beginExtensionRequest(withInputItems items: [Any], completion: @escaping (NSUUID?) -> Void)
    @objc(setRequestCompletionBlock:)
    func setRequestCompletionBlock(_ block: @escaping (NSUUID?, [Any]?) -> Void)
    @objc(setRequestCancellationBlock:)
    func setRequestCancellationBlock(_ block: @escaping (NSUUID?, NSError?) -> Void)
    @objc(setRequestInterruptionBlock:)
    func setRequestInterruptionBlock(_ block: @escaping (NSUUID?) -> Void)
}

/// The helper's answer (the same JSON the helper writes; ContinuumJITHelper.swift).
struct JitHelperAnswer: Codable {
    let ok: Bool
    let message: String
    let log: [String]
}

@MainActor
private enum JitHelper {
    static let file = "ContinuumJITHelper.appex"
    static let requestKey = "continuum-jit-request"
    static let answerKey = "continuum-jit-response"

    /// Requests on their way, kept alive until they answer.
    private static var running: [Request] = []

    /// Why the helper can't be used here, or nil.
    static var unavailable: String? {
        guard #available(iOS 17.4, *) else { return "Built-in JIT needs iOS 17.4 or newer." }
        if getenv("LC_HOME_PATH") != nil {
            return "Built-in JIT doesn't work inside LiveContainer. Use StikDebug instead."
        }
        if identifier == nil { return "The JIT helper is missing from this copy. Install it again and keep its app extensions." }
        return nil
    }

    /// The helper's bundle id in this install (a signing app may have renamed it with the app's).
    static var identifier: String? {
        guard let url = Bundle.main.builtInPlugInsURL?.appendingPathComponent(file) else { return nil }
        return Bundle(url: url)?.bundleIdentifier
    }

    static func send(_ request: [String: Any], done: @escaping (JitHelperAnswer) -> Void) {
        let fail = { (line: String) in done(JitHelperAnswer(ok: false, message: line, log: [])) }
        if let why = unavailable { return fail(why) }
        guard let identifier else { return fail("The JIT helper is missing from this copy.") }
        if NSClassFromString("NSExtension") == nil {
            dlopen("/System/Library/Frameworks/ExtensionFoundation.framework/ExtensionFoundation", RTLD_NOW)
        }
        guard let cls = NSClassFromString("NSExtension") else { return fail("iOS didn't let Continuum start its JIT helper.") }
        let factory = unsafeBitCast(cls as AnyObject, to: ExtensionClassCalls.self)
        let found: AnyObject
        do {
            found = try factory.`extension`(withIdentifier: identifier)
        } catch {
            return fail("Couldn't find the JIT helper (\(identifier)): \(error.localizedDescription)")
        }
        guard let body = try? JSONSerialization.data(withJSONObject: request) else { return fail("Couldn't write the request.") }
        let r = Request(found, done: done)
        running.append(r)
        r.begin(body)
    }

    static func finished(_ r: Request) { running.removeAll { $0 === r } }

    /// Main-actor like the rest of this: a nested type does not take its enclosing type's actor.
    @MainActor
    final class Request {
        private let found: AnyObject
        private let calls: ExtensionCalls
        private var done: ((JitHelperAnswer) -> Void)?

        init(_ found: AnyObject, done: @escaping (JitHelperAnswer) -> Void) {
            self.found = found
            self.calls = unsafeBitCast(found, to: ExtensionCalls.self)
            self.done = done
        }

        func begin(_ body: Data) {
            calls.setRequestCompletionBlock { [weak self] _, items in
                let data = (items?.first as? NSExtensionItem)?.userInfo?[JitHelper.answerKey] as? Data
                let answer = data.flatMap { try? JSONDecoder().decode(JitHelperAnswer.self, from: $0) }
                    ?? JitHelperAnswer(ok: false, message: "The JIT helper finished without an answer.", log: [])
                Task { @MainActor in self?.end(answer) }
            }
            calls.setRequestCancellationBlock { [weak self] _, error in
                let why = error?.localizedDescription ?? "it was cancelled"
                Task { @MainActor in self?.end(JitHelperAnswer(ok: false, message: "The JIT helper stopped: \(why)", log: [])) }
            }
            calls.setRequestInterruptionBlock { [weak self] _ in
                Task { @MainActor in self?.end(JitHelperAnswer(ok: false, message: "The JIT helper stopped. Try again.", log: [])) }
            }
            let item = NSExtensionItem()
            item.userInfo = [JitHelper.requestKey: body]
            calls.beginExtensionRequest(withInputItems: [item]) { [weak self] uuid in
                if uuid == nil {
                    Task { @MainActor in
                        self?.end(JitHelperAnswer(ok: false, message: "The JIT helper didn't start. Install Continuum again and keep its app extensions.", log: []))
                    }
                }
            }
        }

        private func end(_ answer: JitHelperAnswer) {
            guard let done else { return }
            self.done = nil
            JitHelper.finished(self)
            done(answer)
        }
    }
}

// MARK: - Turning JIT on

/// Turns JIT on the chosen way, and says how it went.
@MainActor
final class BuiltInJIT: ObservableObject {
    static let shared = BuiltInJIT()

    static let methodKey = "continuum.jit.method"
    static let onLaunchKey = "continuum.jit.onGameStart"

    /// What happened last, for Settings (one short line).
    @Published private(set) var line = ""
    /// The helper's steps from the last try, for the technical line.
    @Published private(set) var steps: [String] = []
    @Published private(set) var busy = false
    @Published private(set) var pairingPresent = JitPairingFile.isPresent

    @Published var method: JitMethod = JitMethod(rawValue: UserDefaults.standard.string(forKey: BuiltInJIT.methodKey) ?? "") ?? .builtIn {
        didSet { UserDefaults.standard.set(method.rawValue, forKey: Self.methodKey) }
    }

    /// Turn JIT on by itself when a game starts (Built in).
    @Published var onGameStart: Bool = UserDefaults.standard.object(forKey: BuiltInJIT.onLaunchKey) as? Bool ?? true {
        didSet { UserDefaults.standard.set(onGameStart, forKey: Self.onLaunchKey) }
    }

    /// Waiters for the try in progress.
    private var waiting: [(Bool) -> Void] = []
    /// Tries that already failed this run, so a game does not wait on every start for LocalDevVPN.
    private var failedThisRun = 0

    func importPairingFile(_ url: URL) {
        do {
            try JitPairingFile.importFile(from: url)
            pairingPresent = true
            failedThisRun = 0
            line = "Pairing file imported. JIT turns on when a game starts."
        } catch {
            line = error.localizedDescription
        }
    }

    func forgetPairingFile() {
        JitPairingFile.remove()
        pairingPresent = false
        line = "Pairing file removed."
    }

    /// Whether a game about to start should wait for JIT first: Built in is chosen with a pairing
    /// file, the switch is on, the copy can be debugged and nothing has attached yet.
    func shouldTurnOnBeforeGame(_ host: EngineHost) -> Bool {
        guard method == .builtIn, onGameStart, host.useJitWhenAvailable, pairingPresent, failedThisRun < 2 else { return false }
        host.refreshJit()
        guard let report = host.jitReport else { return false }
        return report.canAskEnabler || report.needsSetup || busy
    }

    /// Turns JIT on the chosen way. `done` says whether it is on.
    func turnOn(_ host: EngineHost, done: @escaping (Bool) -> Void = { _ in }) {
        host.refreshJit()
        if host.jitReport?.on == true {
            line = "JIT is on."
            return done(true)
        }
        waiting.append(done)
        guard !busy else { return }
        switch method {
        case .builtIn:
            turnOnBuiltIn(host)
        case .stikDebug:
            // What happens to the tap ("Opening StikDebug...", or that it didn't open) is the
            // host's `jitAskLine`, shown under the button.
            busy = true
            line = ""
            host.askStikDebugForJit()
            waitForAttach(host, seconds: 90, attachLine: "StikDebug attached. Setting JIT up...")
        case .otherApp:
            busy = true
            host.engine.noteJitEnablerAsked()
            line = "Waiting for your JIT app to attach..."
            waitForAttach(host, seconds: 180, attachLine: "Attached. Setting JIT up...")
        }
    }

    private func turnOnBuiltIn(_ host: EngineHost) {
        guard let pairing = JitPairingFile.data() else {
            line = "Import the pairing file first (from StikPair)."
            return finish(false)
        }
        if let why = JitHelper.unavailable {
            line = why
            return finish(false)
        }
        busy = true
        steps = []
        line = "Turning JIT on..."
        // The engine prepares the code memory as soon as the helper's script has attached.
        host.engine.noteJitEnablerAsked()
        var helperDone = false
        JitHelper.send(["operation": "enable", "targetPID": Int(getpid()), "pairing": pairing.base64EncodedString()]) { [weak self] answer in
            guard let self else { return }
            helperDone = true
            self.steps = answer.log
            host.refreshJit()
            if host.jitReport?.on == true {
                self.line = "JIT is on."
                self.failedThisRun = 0
                self.finish(true)
            } else if !answer.ok {
                self.line = Self.plain(answer.message)
                self.failedThisRun += 1
                self.finish(false)
            }
        }
        // While the helper works: the moment its script is attached, prepare the code memory (the
        // engine does nothing until a debugger is really there).
        poll(host, deadline: Date().addingTimeInterval(120)) { [weak self] on in
            guard let self, self.busy else { return }
            if on {
                self.line = "JIT is on."
                self.failedThisRun = 0
                self.finish(true)
            } else {
                // Never leaves `busy` stuck on: also when the helper said it worked and JIT still
                // isn't on by the deadline.
                self.line = helperDone
                    ? "The helper finished but JIT isn't on. Try again."
                    : "JIT didn't come on in time. Is LocalDevVPN connected?"
                self.failedThisRun += 1
                self.finish(false)
            }
        }
    }

    /// StikDebug or another app: wait for it to attach; the engine finishes the job.
    private func waitForAttach(_ host: EngineHost, seconds: TimeInterval, attachLine: String) {
        poll(host, deadline: Date().addingTimeInterval(seconds)) { [weak self] on in
            guard let self else { return }
            self.line = on ? "JIT is on." : "Nothing attached. Try again once your JIT app is ready."
            self.finish(on)
        } attached: { [weak self] in
            self?.line = attachLine
        }
    }

    /// Reads the JIT state every quarter second until it is on or the deadline passes. Each read
    /// lets the engine run the region protocol once the script is attached (it asked for it above).
    /// A main-actor task rather than a Timer: a Timer's block is @Sendable, and this one has to
    /// change `said` and touch the engine host.
    private func poll(_ host: EngineHost, deadline: Date, done: @escaping (Bool) -> Void, attached: (() -> Void)? = nil) {
        Task { @MainActor in
            var said = false
            while self.busy {
                let report = host.engine.prepareJit(askedForIt: false)
                host.jitReport = report
                if report.needsSetup, !said {
                    said = true
                    attached?()
                }
                if report.on || Date() >= deadline {
                    host.refreshJit()
                    done(report.on)
                    return
                }
                try? await Task.sleep(nanoseconds: 250_000_000)
            }
        }
    }

    private func finish(_ on: Bool) {
        busy = false
        let all = waiting
        waiting = []
        all.forEach { $0(on) }
    }

    func resetDiskImage() {
        JitHelper.send(["operation": "reset"]) { [weak self] answer in
            self?.line = answer.ok ? "Done. It downloads again next time." : Self.plain(answer.message)
        }
    }

    /// The helper's errors, as a person would say them.
    static func plain(_ message: String) -> String {
        let m = message.lowercased()
        if m.contains("unreachable") || m.contains("connection refused") || m.contains("timed out")
            || m.contains("10.7.0.1") || m.contains("tunnel") {
            return "Couldn't reach the phone. Connect LocalDevVPN (with Wi-Fi on) and try again."
        }
        if m.contains("pairing") {
            return "The pairing file didn't work. Make a new one in StikPair and import it."
        }
        if m.contains("developer mode") {
            return "Turn on Developer Mode (Settings, Privacy & Security) and try again."
        }
        return "JIT didn't turn on: \(message)"
    }
}

// MARK: - Settings rows

/// The JIT method, the pairing file and the buttons, under Technical details' JIT line.
struct BuiltInJITRows: View {
    @ObservedObject var host: EngineHost
    @ObservedObject private var jit = BuiltInJIT.shared
    @State private var picking = false

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Picker("JIT method", selection: $jit.method) {
                ForEach(JitMethod.allCases) { Text($0.label).tag($0) }
            }
            .pickerStyle(.segmented)
            SettingsNote(jit.method.note)

            if jit.method == .builtIn {
                SettingsReadout(label: "Pairing file", value: jit.pairingPresent ? "Imported" : "Not imported")
                SettingsButton(title: jit.pairingPresent ? "Import a new pairing file" : "Import pairing file", role: .normal) {
                    picking = true
                }
                Toggle(isOn: $jit.onGameStart) {
                    Text("Turn JIT on when a game starts")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(ShellPalette.text)
                }
                .tint(ShellPalette.accent)
            }
            if host.jitReport?.on != true {
                SettingsButton(title: jit.busy ? "Turning JIT on..." : "Turn on JIT now", role: .normal) {
                    jit.turnOn(host)
                }
                .disabled(jit.busy)
            }
            if jit.method == .stikDebug, !host.jitAskLine.isEmpty {
                SettingsNote(host.jitAskLine)
            }
            if !jit.line.isEmpty {
                SettingsNote(jit.line)
            }
            if jit.method == .builtIn, !jit.steps.isEmpty {
                SettingsNote(jit.steps.suffix(3).joined(separator: " · "))
            }
            if jit.method == .builtIn {
                SettingsButton(title: "Download the developer disk image again", role: .normal) {
                    jit.resetDiskImage()
                }
            }
        }
        .fileImporter(isPresented: $picking, allowedContentTypes: [.item]) { result in
            if case .success(let url) = result { jit.importPairingFile(url) }
        }
    }
}
