// Continuum - the bundled players: Flash in Ruffle and J2ME in J2meJS, each in a WKWebView that
// loads LOCAL FILES ONLY.
//
// docs/MANIC_PARITY.md decided this: these run the way Manic EMU runs them, in a player view inside
// the .ipa. That is a player inside the app and not a web build of Continuum. The rest of the app
// stays around them: the Library row, the on-screen pad and controllers (sending keys), skins and
// their function buttons, the status line, pause, quit, screenshots, and saves in Manic's formats.
//
// HOW IT STAYS OFF THE LIBRETRO PATH. CoreCatalog routes .swf and .jar to systems `flash` and
// `j2me` whose route carries a PLAYER id ("ruffle", "j2mejs") in the core id slot. `byId` does not
// know those, so nothing libretro can pick them up, and `EngineHost.launch` branches to
// `launchWebPlayer` BEFORE its core lookup. The one `engine.launch` call stays the only one, and it
// is never reached for these. `running` stays false (it means "an engine session is running");
// `webPlayer` is the session here, and the player screen is shown because `activeEntry` is set.
//
// WHAT THE WEB VIEW MAY TOUCH. Pages, scripts and the game come from a WKURLSchemeHandler on
// `continuum-player://`, which serves the player's folder in the app bundle (support/players/<kind>,
// fetched and pinned by scripts/fetch-players.sh), the one game file, and the two scripts this app
// writes, and nothing else (WebPlayerAddress.classify). The handler stands in for loadFileURL on
// purpose: WebKit refuses fetch() and module workers on file:// pages, both engines fetch their own
// files, and Ruffle needs application/wasm to compile; the handler also never exposes a directory,
// where loadFileURL's read access is a whole folder. The network is fenced three ways: a content
// rule list blocking every load that is not this scheme, blob: or data:; a Content-Security-Policy
// on every page; and a navigation policy that cancels any navigation off the player's own pages.
// Storage is a non-persistent website data store per session: the save FILE is the truth, seeded
// into the page before the engine starts and read back while it runs and when it stops.

#if canImport(UIKit)
import AVFoundation
import Combine
import SwiftUI
import UIKit
import WebKit

// MARK: - The scheme handler

/// Serves the player's address space. Main-thread callbacks; file reads on a background queue,
/// answered only if WebKit has not stopped the task meanwhile (answering a stopped task raises).
final class WebPlayerSchemeHandler: NSObject, WKURLSchemeHandler {
    let kind: WebPlayerKind
    let root: URL
    let gameURL: URL
    let page: Data
    let bridge: Data
    let seed: Data
    /// Told about every refused request, for the status line.
    var onRefused: ((String) -> Void)?

    private var live = Set<ObjectIdentifier>()
    private let queue = DispatchQueue(label: "dev.continuum.player.files", qos: .userInitiated)

    init(kind: WebPlayerKind, root: URL, gameURL: URL, page: Data, bridge: Data, seed: Data) {
        self.kind = kind
        self.root = root.standardizedFileURL
        self.gameURL = gameURL
        self.page = page
        self.bridge = bridge
        self.seed = seed
    }

    func webView(_ webView: WKWebView, start task: WKURLSchemeTask) {
        let id = ObjectIdentifier(task)
        live.insert(id)
        let url = task.request.url
        switch WebPlayerAddress.classify(host: url?.host, path: url?.path ?? "", kind: kind) {
        case .page:
            respond(task, data: page, mime: "text/html; charset=utf-8")
        case .bridge:
            respond(task, data: bridge, mime: "text/javascript; charset=utf-8")
        case .seed:
            respond(task, data: seed, mime: "text/javascript; charset=utf-8")
        case .game:
            read(gameURL, for: task, mime: WebPlayerAddress.mimeType(for: gameURL.path))
        case .bundle(let relative):
            let file = root.appendingPathComponent(relative).standardizedFileURL
            // classify already refused every `..`; this is the belt to that brace.
            guard file.path.hasPrefix(root.path + "/") else {
                notFound(task, reason: "\(relative) is outside the player's folder")
                return
            }
            read(file, for: task, mime: WebPlayerAddress.mimeType(for: file.path))
        case .refused(let reason):
            notFound(task, reason: reason)
        }
    }

    func webView(_ webView: WKWebView, stop task: WKURLSchemeTask) {
        live.remove(ObjectIdentifier(task))
    }

    private func read(_ file: URL, for task: WKURLSchemeTask, mime: String) {
        let id = ObjectIdentifier(task)
        queue.async { [weak self] in
            let data = try? Data(contentsOf: file, options: .mappedIfSafe)
            DispatchQueue.main.async {
                guard let self, self.live.contains(id) else { return }
                if let data {
                    self.respond(task, data: data, mime: mime)
                } else {
                    self.notFound(task, reason: "\(file.lastPathComponent) is not in this build")
                }
            }
        }
    }

    private func respond(_ task: WKURLSchemeTask, data: Data, mime: String) {
        let id = ObjectIdentifier(task)
        guard live.contains(id), let url = task.request.url else { return }
        var headers = [
            "Content-Type": mime,
            "Content-Length": String(data.count),
            "Cache-Control": "no-store",
        ]
        if mime.hasPrefix("text/html") {
            headers["Content-Security-Policy"] = WebPlayerAddress.contentSecurityPolicy
        }
        let response = HTTPURLResponse(url: url, statusCode: 200, httpVersion: "HTTP/1.1",
                                       headerFields: headers)
            ?? URLResponse(url: url, mimeType: mime, expectedContentLength: data.count,
                           textEncodingName: nil)
        task.didReceive(response)
        task.didReceive(data)
        task.didFinish()
        live.remove(id)
    }

    private func notFound(_ task: WKURLSchemeTask, reason: String) {
        let id = ObjectIdentifier(task)
        guard live.contains(id), let url = task.request.url else { return }
        onRefused?(reason)
        let response = HTTPURLResponse(url: url, statusCode: 404, httpVersion: "HTTP/1.1",
                                       headerFields: ["Content-Type": "text/plain", "Content-Length": "0"])
        if let response { task.didReceive(response) }
        task.didReceive(Data())
        task.didFinish()
        live.remove(id)
    }
}

/// WKUserContentController holds its handlers strongly; this breaks the cycle back to the session.
private final class WeakMessageProxy: NSObject, WKScriptMessageHandler {
    weak var target: WebPlayerSession?
    init(_ target: WebPlayerSession) { self.target = target }
    func userContentController(_ controller: WKUserContentController,
                               didReceive message: WKScriptMessage) {
        MainActor.assumeIsolated { target?.receive(message.body) }
    }
}

// MARK: - The session

/// One game running in a bundled player.
@MainActor
final class WebPlayerSession: NSObject, ObservableObject, WKNavigationDelegate, WKUIDelegate {
    let kind: WebPlayerKind
    let entry: LibraryEntry
    let webView: WKWebView
    private let handler: WebPlayerSchemeHandler
    private let pageURL: URL
    private let saveURL: URL
    private(set) var settings: WebPlayerGameSettings
    /// The phone screen (J2ME) or nil until the movie says (Flash).
    private(set) var screen: CGSize?
    @Published private(set) var paused = false
    private(set) var isReady = false

    /// The buttons held this frame, W3C order, touch and controllers merged. Set by the host.
    var inputSource: () -> [Bool] = { [] }
    /// A line for the status strip.
    var report: (String) -> Void = { _ in }
    /// The picture's aspect changed.
    var onAspect: (CGFloat) -> Void = { _ in }

    private var table: [Int: String] = [:]
    private var flashKeys: [String: FlashKeyEvent] = [:]
    private var held = Set<String>()
    private var link: CADisplayLink?
    private var observers: [NSObjectProtocol] = []
    private var finished = false
    private var savedOnce = false

    /// Sessions that have left the screen but are still handing their save back.
    private static var closing: [String: WebPlayerSession] = [:]
    private static var afterClose: [String: [() -> Void]] = [:]
    /// The compiled offline rule list, built once per launch of the app.
    private static var ruleList: WKContentRuleList?
    private static var ruleListError: String?

    init(kind: WebPlayerKind, entry: LibraryEntry, settings: WebPlayerGameSettings,
         handler: WebPlayerSchemeHandler, pageURL: URL, saveURL: URL, screen: CGSize?) {
        self.kind = kind
        self.entry = entry
        self.settings = settings
        self.handler = handler
        self.pageURL = pageURL
        self.saveURL = saveURL
        self.screen = screen

        let config = WKWebViewConfiguration()
        // A fresh, in-memory store per session: one game can never see another's storage, and
        // nothing survives except what this app writes to the save file.
        config.websiteDataStore = .nonPersistent()
        config.setURLSchemeHandler(handler, forURLScheme: WebPlayerAddress.scheme)
        config.allowsInlineMediaPlayback = true
        config.mediaTypesRequiringUserActionForPlayback = []
        config.preferences.javaScriptCanOpenWindowsAutomatically = false
        config.suppressesIncrementalRendering = false
        if #available(iOS 15.4, *) {
            config.preferences.isElementFullscreenEnabled = false
        }
        let view = WKWebView(frame: .zero, configuration: config)
        view.isOpaque = true
        view.backgroundColor = .black
        view.scrollView.backgroundColor = .black
        view.scrollView.isScrollEnabled = false
        view.scrollView.bounces = false
        view.scrollView.contentInsetAdjustmentBehavior = .never
        view.allowsBackForwardNavigationGestures = false
        view.allowsLinkPreview = false
        webView = view
        super.init()
        config.userContentController.add(WeakMessageProxy(self), name: WebPlayerScripts.handlerName)
        view.navigationDelegate = self
        view.uiDelegate = self
        handler.onRefused = { [weak self] reason in
            self?.report("\(kind.playerName) asked for something that is not served: \(reason)")
        }
        rebuildKeyTable()
    }

    // MARK: Start and stop

    /// Builds the offline fence, then loads the page.
    func start() {
        Self.withRuleList { [weak self] list in
            guard let self, !self.finished else { return }
            if let list {
                self.webView.configuration.userContentController.add(list)
            } else {
                // Not silent: the CSP and the navigation policy still hold, and the line says the
                // third fence is missing.
                self.report("\(self.kind.playerName): the offline rule list could not be built "
                            + "(\(Self.ruleListError ?? "no reason given")); the page's own "
                            + "security policy still blocks the network")
            }
            self.webView.load(URLRequest(url: self.pageURL, cachePolicy: .reloadIgnoringLocalCacheData))
            self.startInput()
        }
        observeLifecycle()
    }

    private static func withRuleList(_ done: @escaping (WKContentRuleList?) -> Void) {
        if let ruleList { done(ruleList); return }
        guard let store = WKContentRuleListStore.default() else {
            ruleListError = "there is no rule list store"
            done(nil)
            return
        }
        store.compileContentRuleList(forIdentifier: "continuum-player-offline-v1",
                                     encodedContentRuleList: WebPlayerAddress.contentRuleList) { list, error in
            DispatchQueue.main.async {
                if let list { ruleList = list } else { ruleListError = error?.localizedDescription }
                done(list)
            }
        }
    }

    /// Hands the save back, writes it, and closes the page. `then` runs once the save is on disk
    /// (or after three seconds at most, with a line saying the last automatic save is kept).
    func finish(then: (() -> Void)? = nil) {
        guard !finished else { then?(); return }
        finished = true
        stopInput()
        for observer in observers { NotificationCenter.default.removeObserver(observer) }
        observers = []
        let key = entry.path
        Self.closing[key] = self
        if let then { Self.afterClose[key, default: []].append(then) }
        var settled = false
        let settle: (String?) -> Void = { [weak self] line in
            guard !settled else { return }
            settled = true
            if let line { self?.report(line) }
            self?.tearDown()
            Self.closing[key] = nil
            let waiting = Self.afterClose.removeValue(forKey: key) ?? []
            for run in waiting { run() }
        }
        guard isReady else {
            settle(nil)
            return
        }
        webView.callAsyncJavaScript("return await window.continuumPlayer.finish();",
                                    arguments: [:], in: nil, in: .page) { [weak self] result in
            guard let self else { settle(nil); return }
            switch result {
            case .success(let value):
                settle(self.write(result: value, reason: "left"))
            case .failure(let error):
                settle("\(self.entry.name): the save could not be read back as it closed "
                       + "(\(error.localizedDescription)); the last automatic save is kept")
            }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 3) {
            settle("\(key.split(separator: "/").last.map(String.init) ?? "the game"): the save "
                   + "took too long to read back as it closed; the last automatic save is kept")
        }
    }

    /// Runs `then` once any closing session for this game has written its save.
    static func whenClosed(path: String, then: @escaping () -> Void) {
        if closing[path] != nil {
            afterClose[path, default: []].append(then)
        } else {
            then()
        }
    }

    private func tearDown() {
        webView.stopLoading()
        webView.configuration.userContentController.removeAllScriptMessageHandlers()
        webView.configuration.userContentController.removeAllContentRuleLists()
        webView.navigationDelegate = nil
        webView.uiDelegate = nil
        webView.removeFromSuperview()
    }

    // MARK: Saves

    /// Asks the page for its storage now and writes it. Used when the app leaves the foreground.
    func persistNow(reason: String) {
        guard isReady, !finished else { return }
        var task = UIBackgroundTaskIdentifier.invalid
        task = UIApplication.shared.beginBackgroundTask(withName: "continuum.player.save") {
            UIApplication.shared.endBackgroundTask(task)
        }
        webView.callAsyncJavaScript("return await window.continuumPlayer.flushNow();",
                                    arguments: [:], in: nil, in: .page) { [weak self] result in
            if case .success(let value) = result, let line = self?.write(result: value, reason: reason) {
                self?.report(line)
            }
            UIApplication.shared.endBackgroundTask(task)
        }
    }

    /// Writes what the page handed back. Returns a line only for a failure.
    private func write(result: Any?, reason: String) -> String? {
        guard let dict = result as? [String: Any] else {
            return "\(entry.name): the player handed back no save when the game was \(reason)"
        }
        switch kind {
        case .flash:
            guard let items = WebPlayerEvent.flashItems(dict["items"]) else {
                return "\(entry.name): the player's storage came back unreadable; the last save is kept"
            }
            return writeFlash(items)
        case .j2me:
            guard let files = WebPlayerEvent.j2meFiles(dict["files"]) else {
                return "\(entry.name): the phone's files could not be read back; the last save is kept"
            }
            return writeJ2ME(files)
        }
    }

    private func writeFlash(_ items: [(String, String)]) -> String? {
        // Nothing stored and no save yet: nothing to write, and no empty file to leave behind.
        if items.isEmpty && !FileManager.default.fileExists(atPath: saveURL.path) { return nil }
        let text = playerFlashSaveEncode(items: items.map { FlashSaveItem(key: $0.0, value: $0.1) })
        return writeSaveData(Data(text.utf8))
    }

    private func writeJ2ME(_ files: [WebPlayerFile]) -> String? {
        if files.isEmpty && !FileManager.default.fileExists(atPath: saveURL.path) { return nil }
        let stem = (entry.name as NSString).deletingPathExtension
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        do {
            let data = try playerJ2meSaveEncode(
                files: files.map { J2meFile(path: $0.path, mtime: $0.mtime, data: $0.data) },
                gameName: stem, exportDate: formatter.string(from: Date()))
            return writeSaveData(data)
        } catch {
            return "\(entry.name): the J2ME save could not be built: \(error)"
        }
    }

    private func writeSaveData(_ data: Data) -> String? {
        do {
            try FileManager.default.createDirectory(at: saveURL.deletingLastPathComponent(),
                                                    withIntermediateDirectories: true)
            try data.write(to: saveURL, options: .atomic)
            savedOnce = true
            return nil
        } catch {
            return "\(entry.name): the save could not be written: \(error.localizedDescription)"
        }
    }

    // MARK: The page

    func receive(_ body: Any) {
        guard let event = WebPlayerEvent(message: body) else { return }
        switch event {
        case .ready:
            isReady = true
            report("running: \(entry.name) in \(kind.playerName)")
        case .metadata(let width, let height):
            if width > 0, height > 0 {
                screen = CGSize(width: width, height: height)
                onAspect(CGFloat(width / height))
            }
        case .flashSave(let items):
            if let line = writeFlash(items) { report(line) }
        case .j2meSave(let files):
            if let line = writeJ2ME(files) { report(line) }
        case .failed(let message):
            report("\(entry.name) in \(kind.playerName) failed: \(message)")
        case .log:
            break
        }
    }

    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction,
                 decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        if WebPlayerAddress.allowsNavigation(to: action.request.url, kind: kind) {
            decisionHandler(.allow)
        } else {
            report("\(kind.playerName) tried to open \(action.request.url?.host ?? "a page"); "
                   + "the player has no network, so it was blocked")
            decisionHandler(.cancel)
        }
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        report("\(entry.name): the player page failed: \(error.localizedDescription)")
    }

    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!,
                 withError error: Error) {
        report("\(entry.name): the player page did not open: \(error.localizedDescription)")
    }

    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
        isReady = false
        report("\(entry.name): the player stopped (WebKit's process ended, usually for memory); "
               + "quit and open the game again. The last automatic save is kept")
    }

    func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration,
                 for navigationAction: WKNavigationAction,
                 windowFeatures: WKWindowFeatures) -> WKWebView? {
        report("\(kind.playerName) tried to open a new window; the player allows none")
        return nil
    }

    // MARK: Controls

    func setPaused(_ pause: Bool) {
        guard isReady else {
            paused = pause
            return
        }
        if pause { releaseAllKeys() }
        paused = pause
        webView.evaluateJavaScript(pause ? "window.continuumPlayer.pause();"
                                         : "window.continuumPlayer.resume();")
    }

    func setVolume(muted: Bool, volume: Double) {
        let level = muted ? 0 : max(0, min(1, volume))
        webView.evaluateJavaScript("window.continuumPlayer && window.continuumPlayer.setVolume(\(level));")
    }

    /// New per-game settings: keys apply now; a J2ME screen size applies on the next start.
    func apply(settings new: WebPlayerGameSettings) {
        releaseAllKeys()
        settings = new
        rebuildKeyTable()
    }

    private func rebuildKeyTable() {
        let bindings: [PlayerBinding]
        do {
            bindings = try playerBindings(system: kind.systemId, overrides: settings.keyOverrides)
        } catch {
            report("\(entry.name): its key remap was unreadable (\(error)); the default keys are used")
            bindings = (try? playerBindings(system: kind.systemId, overrides: "")) ?? []
        }
        table = Dictionary(uniqueKeysWithValues: bindings.map { (Int($0.slot), $0.token) })
        flashKeys = [:]
        if kind == .flash {
            for token in Set(table.values) where token != WebPlayerKeys.unbound {
                if let event = playerFlashKey(token: token) { flashKeys[token] = event }
            }
        }
    }

    private func startInput() {
        guard link == nil else { return }
        let proxy = DisplayLinkProxy(self)
        let link = CADisplayLink(target: proxy, selector: #selector(DisplayLinkProxy.tick))
        link.add(to: .main, forMode: .common)
        self.link = link
    }

    private func stopInput() {
        link?.invalidate()
        link = nil
        releaseAllKeys()
    }

    fileprivate func tick() {
        guard isReady, !paused, !finished else { return }
        let now = WebPlayerKeys.held(slots: WebPlayerKeys.slots(from: inputSource()), table: table)
        guard now != held else { return }
        let change = WebPlayerKeys.changes(from: held, to: now)
        held = now
        send(released: change.released, pressed: change.pressed)
    }

    private func releaseAllKeys() {
        guard !held.isEmpty else { return }
        let released = held.sorted()
        held = []
        send(released: released, pressed: [])
    }

    private func send(released: [String], pressed: [String]) {
        var lines: [String] = []
        for (tokens, down) in [(released, false), (pressed, true)] {
            for token in tokens {
                switch kind {
                case .flash:
                    if let key = flashKeys[token] {
                        lines.append(WebPlayerScripts.flashKey(code: key.code, key: key.key,
                                                               keyCode: key.keyCode, down: down))
                    }
                case .j2me:
                    if let code = playerJ2meKeyCode(token: token, phoneType: settings.phoneType) {
                        lines.append(WebPlayerScripts.j2meKey(code: code, down: down))
                    }
                }
            }
        }
        guard !lines.isEmpty else { return }
        webView.evaluateJavaScript(lines.joined(separator: "\n"))
    }

    private func observeLifecycle() {
        let centre = NotificationCenter.default
        observers = [
            centre.addObserver(forName: UIApplication.willResignActiveNotification, object: nil,
                               queue: .main) { [weak self] _ in
                MainActor.assumeIsolated {
                    guard let self else { return }
                    self.releaseAllKeys()
                    self.persistNow(reason: "put in the background")
                }
            },
        ]
    }

    /// The picture as an image, for the screenshot button.
    func snapshot(_ done: @escaping (UIImage?, String?) -> Void) {
        let config = WKSnapshotConfiguration()
        config.afterScreenUpdates = true
        webView.takeSnapshot(with: config) { image, error in
            done(image, error?.localizedDescription)
        }
    }
}

/// CADisplayLink retains its target; this keeps the session releasable.
private final class DisplayLinkProxy: NSObject {
    weak var target: WebPlayerSession?
    init(_ target: WebPlayerSession) { self.target = target }
    @objc func tick() { MainActor.assumeIsolated { target?.tick() } }
}

// MARK: - On screen

/// The player's web view, where the picture goes. Real touches reach it directly: taps are
/// mouse clicks in Ruffle and the touch screen in J2meJS.
struct WebPlayerSurface: UIViewRepresentable {
    @ObservedObject var session: WebPlayerSession

    func makeUIView(context: Context) -> UIView {
        let container = UIView()
        container.backgroundColor = .black
        container.clipsToBounds = true
        attach(to: container)
        return container
    }

    func updateUIView(_ container: UIView, context: Context) {
        if session.webView.superview !== container { attach(to: container) }
    }

    private func attach(to container: UIView) {
        session.webView.removeFromSuperview()
        session.webView.frame = container.bounds
        session.webView.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        container.addSubview(session.webView)
    }
}

// MARK: - The host's side

extension EngineHost {
    /// The app's one host, for code that has none at hand (the save-format buttons).
    static weak var shared: EngineHost?

    /// The player's folder in the bundle, when this build has it.
    static func webPlayerRoot(_ kind: WebPlayerKind) -> URL? {
        guard let root = Bundle.main.resourceURL?
            .appendingPathComponent("support/players/\(kind.bundleFolder)", isDirectory: true) else {
            return nil
        }
        let probe = root.appendingPathComponent(kind.bundleProbe)
        return FileManager.default.fileExists(atPath: probe.path) ? root : nil
    }

    /// Where a game's player save lives: Application Support/PlayerSaves/<stem>.<Manic's extension>.
    static func webPlayerSaveURL(kind: WebPlayerKind, entry: LibraryEntry) -> URL? {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first?
            .appendingPathComponent("PlayerSaves", isDirectory: true)
            .appendingPathComponent(kind.saveFileName(forGame: entry.name))
    }

    /// Opens a Flash or J2ME game in its bundled player. Every failure leaves its own line.
    func launchWebPlayer(entry: LibraryEntry, kind: WebPlayerKind) {
        status = "opening \(entry.name) in \(kind.playerName)..."
        guard let root = Self.webPlayerRoot(kind) else {
            status = "\(entry.name): the \(kind.gameKind) player (\(kind.playerName)) is not in this "
                + "build; the download on the build machine failed, so \(kind.gameKind) games "
                + "cannot open until the next build has it"
            return
        }
        guard FileManager.default.fileExists(atPath: entry.path) else {
            status = "missing on disk: \(entry.name); it was removed since the last scan"
            refreshLibrary()
            return
        }
        guard let saveURL = Self.webPlayerSaveURL(kind: kind, entry: entry) else {
            status = "\(entry.name): there is no Application Support folder for its save"
            return
        }
        // Anything running goes first: an engine session through the one stop, or another player.
        if running || webPlayer != nil || activeEntry != nil {
            stopSession()
        }
        // A session of this same game that is still handing its save back must finish writing
        // before this one reads it.
        WebPlayerSession.whenClosed(path: entry.path) { [weak self] in
            self?.startWebPlayerSession(entry: entry, kind: kind, root: root, saveURL: saveURL)
        }
    }

    private func startWebPlayerSession(entry: LibraryEntry, kind: WebPlayerKind, root: URL,
                                       saveURL: URL) {
        let settings = WebPlayerGameSettings.load(kind: kind, gameName: entry.name)
        let page: Data
        let seed: String
        let pageURL: URL?
        var screen: CGSize?
        var notes: [String] = []
        let bridge: String
        switch kind {
        case .flash:
            bridge = WebPlayerScripts.flashBridge
            page = Data(WebPlayerPages.flashPage.utf8)
            var items: [(String, String)] = []
            if let text = try? String(contentsOf: saveURL, encoding: .utf8) {
                do {
                    let read = try playerFlashSaveDecode(text: text, swfFileName: entry.name)
                    items = read.items.map { ($0.key, $0.value) }
                } catch {
                    notes.append("its save could not be read (\(error)), so it starts without it")
                }
            }
            seed = WebPlayerScripts.flashSeed(items: items, swfName: entry.name)
            pageURL = WebPlayerAddress.pageURL(kind: .flash)
        case .j2me:
            bridge = WebPlayerScripts.j2meBridge
            let manifest: J2meManifest
            do {
                manifest = try playerJ2meManifest(path: entry.path)
            } catch {
                status = "\(entry.name) cannot open as a J2ME game: \(error)"
                return
            }
            let size = playerJ2meParseScreenSize(text: settings.screenSize)
                ?? manifest.screen ?? playerJ2meDefaultScreenSize()
            screen = CGSize(width: CGFloat(size.width), height: CGFloat(size.height))
            guard let mainURL = Optional(root.appendingPathComponent("main.html")),
                  let html = try? String(contentsOf: mainURL, encoding: .utf8),
                  let rewritten = WebPlayerPages.rewriteJ2MEPage(html, width: Int(size.width),
                                                                 height: Int(size.height)) else {
                status = "\(entry.name): the J2ME player's page is not the one this build was "
                    + "written for, so it was not run"
                return
            }
            page = Data(rewritten.utf8)
            var files: [J2meFile] = []
            if let data = try? Data(contentsOf: saveURL) {
                do {
                    files = try playerJ2meSaveDecode(data: data)
                } catch {
                    notes.append("its save could not be read (\(error)), so it starts without it")
                }
            }
            let records = playerJ2meSeed(files: files).map {
                WebPlayerScripts.SeedRecord(pathname: $0.pathname, isDir: $0.isDir,
                                            parentDir: $0.parentDir, mtime: $0.mtime, data: $0.data)
            }
            seed = WebPlayerScripts.j2meSeed(records: records)
            pageURL = WebPlayerAddress.pageURL(kind: .j2me, midletClass: manifest.midletClass,
                                               width: Int(size.width), height: Int(size.height))
        }
        guard let pageURL else {
            status = "\(entry.name): the player's address could not be built"
            return
        }
        let handler = WebPlayerSchemeHandler(kind: kind, root: root,
                                             gameURL: URL(fileURLWithPath: entry.path), page: page,
                                             bridge: Data(bridge.utf8), seed: Data(seed.utf8))
        let session = WebPlayerSession(kind: kind, entry: entry, settings: settings,
                                       handler: handler, pageURL: pageURL, saveURL: saveURL,
                                       screen: screen)
        let padInput = self.padInput
        let controllers = self.controllers
        session.inputSource = {
            var buttons = padInput.currentFrame().buttons
            for pad in controllers.poll() {
                for (index, down) in pad.buttons.enumerated() where down {
                    if index < buttons.count { buttons[index] = true } else { buttons.append(true) }
                }
            }
            return buttons
        }
        session.report = { [weak self] line in self?.status = line }
        session.onAspect = { [weak self] aspect in
            self?.webPlayerAspect = aspect
            self?.screenLayoutVersion &+= 1
        }

        // The audio session the app's own player uses, so a game is heard with the silent switch
        // on, like every other system.
        let audioSession = AVAudioSession.sharedInstance()
        do {
            try AudioSessionPolicy.apply(to: audioSession)
            try audioSession.setActive(true)
        } catch {
            notes.append("sound may be silent: the audio session refused (\(error.localizedDescription))")
        }

        webPlayer = session
        webPlayerAspect = screen.map { $0.width / $0.height } ?? (4.0 / 3.0)
        activeEntry = entry
        activeCoreId = kind.engineId
        paused = false
        controllers.noteRunningSystem(activeSystem)
        screenLayoutVersion &+= 1
        session.setVolume(muted: emulation.muted, volume: emulation.volume)
        webPlayerVolumeWatch = emulation.objectWillChange.sink { [weak self, weak session] _ in
            DispatchQueue.main.async {
                guard let self, let session, self.webPlayer === session else { return }
                session.setVolume(muted: self.emulation.muted, volume: self.emulation.volume)
            }
        }
        var line = "opening \(entry.name) in \(kind.playerName)"
        if let screen, kind == .j2me { line += " at \(Int(screen.width))x\(Int(screen.height))" }
        if !notes.isEmpty { line += "; " + notes.joined(separator: "; ") }
        status = line
        session.start()
    }

    /// Ends the bundled player's session, if any: hands the save back and closes the page. Called
    /// from `stopSession`, so leaving, quitting and a new launch all end it the same way.
    func stopWebPlayer() {
        guard let session = webPlayer else { return }
        webPlayer = nil
        webPlayerAspect = 0
        webPlayerSettingsOpen = false
        webPlayerVolumeWatch = nil
        session.finish()
    }

    /// The restart button for a bundled player: the save is handed back and written, then the
    /// game starts again from it in a fresh page.
    func restartWebPlayer() {
        guard let session = webPlayer, let entry = activeEntry else { return }
        let kind = session.kind
        webPlayer = nil
        webPlayerVolumeWatch = nil
        session.finish { [weak self] in
            guard let self, self.activeEntry == entry, self.webPlayer == nil,
                  let root = Self.webPlayerRoot(kind),
                  let saveURL = Self.webPlayerSaveURL(kind: kind, entry: entry) else { return }
            self.startWebPlayerSession(entry: entry, kind: kind, root: root, saveURL: saveURL)
        }
    }

    /// The screenshot button for a bundled player: the web view's picture, saved where the engine's
    /// screenshots go.
    func captureWebPlayerScreenshot(_ session: WebPlayerSession, of entry: LibraryEntry) {
        session.snapshot { [weak self] image, failure in
            guard let self else { return }
            guard let png = image?.pngData() else {
                self.status = "screenshot failed: \(failure ?? "the player's picture could not be read")"
                return
            }
            guard let documents = FileManager.default.urls(for: .documentDirectory,
                                                           in: .userDomainMask).first else {
                self.status = "screenshot failed: there is no Documents folder"
                return
            }
            let folder = documents.appendingPathComponent("Screenshots", isDirectory: true)
            let formatter = DateFormatter()
            formatter.dateFormat = "yyyy-MM-dd HH.mm.ss"
            let safe = entry.name.replacingOccurrences(of: "/", with: "-")
            let url = folder.appendingPathComponent("\(safe) \(formatter.string(from: Date())).png")
            do {
                try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
                try png.write(to: url, options: .atomic)
                self.status = "screenshot saved to Files, Continuum/Screenshots/\(url.lastPathComponent)"
            } catch {
                self.status = "screenshot failed: \(error.localizedDescription)"
            }
        }
    }

    /// The `j2meSettings` skin function: the running J2ME game's settings sheet.
    @MainActor func showJ2MESettings() -> String {
        guard activeEntry != nil else { return "J2ME settings: no game is running" }
        guard let session = webPlayer else {
            return "J2ME settings: the running game is not a J2ME game"
        }
        guard session.kind == .j2me else {
            return "J2ME settings: this is a Flash game; its keys are under the player menu, Flash settings"
        }
        webPlayerSettingsOpen = true
        return "J2ME settings: screen size, phone type, keys and the save file"
    }

    /// The player menu's settings row, for either player.
    func showWebPlayerSettings() {
        guard let session = webPlayer else {
            status = "player settings: no Flash or J2ME game is running"
            return
        }
        webPlayerSettingsOpen = true
        status = "\(session.kind.gameKind) settings: keys and the save file"
    }

    /// Saves new per-game settings and hands them to the running session.
    func setWebPlayerSettings(_ settings: WebPlayerGameSettings, restartIfNeeded: Bool) {
        guard let session = webPlayer else { return }
        let old = session.settings
        settings.save(kind: session.kind, gameName: session.entry.name)
        session.apply(settings: settings)
        if restartIfNeeded, session.kind == .j2me, old.screenSize != settings.screenSize {
            status = "J2ME screen size set to \(settings.screenSize.isEmpty ? "the game's own" : settings.screenSize); restarting"
            restartWebPlayer()
        }
    }

    // MARK: Save files in Manic's formats

    /// The formats a player system's save exports as, or nil for a libretro system.
    static func webPlayerSaveFormats(system: String) -> [String]? {
        WebPlayerKind(systemId: system).map { [$0.saveExtension] }
    }

    /// Imports a Manic `.json` (Flash) or `.J2meJS.srm` (J2ME) save for `entry`. The old save is
    /// kept as .bak; a running copy of the game restarts from the new one.
    func importWebPlayerSave(_ data: Data, named name: String, for entry: LibraryEntry,
                             kind: WebPlayerKind) -> String {
        guard let saveURL = Self.webPlayerSaveURL(kind: kind, entry: entry) else {
            return "save import failed: there is no Application Support folder"
        }
        // Checked before anything is replaced, so a wrong file changes nothing.
        let note: String
        switch kind {
        case .flash:
            guard let text = String(data: data, encoding: .utf8) else {
                return "save import refused for \(entry.name): \(name) is not text, so not a Flash .json save"
            }
            do {
                let read = try playerFlashSaveDecode(text: text, swfFileName: entry.name)
                note = "\(read.items.count) stored item(s)"
                    + (read.rehosted > 0 ? ", \(read.rehosted) moved onto this game's file name" : "")
            } catch {
                return "save import refused for \(entry.name): \(error)"
            }
        case .j2me:
            do {
                let files = try playerJ2meSaveDecode(data: data)
                note = "\(files.count) phone file(s)"
            } catch {
                return "save import refused for \(entry.name): \(error)"
            }
        }
        let apply = { () -> String in
            let manager = FileManager.default
            do {
                try manager.createDirectory(at: saveURL.deletingLastPathComponent(),
                                            withIntermediateDirectories: true)
                if manager.fileExists(atPath: saveURL.path) {
                    let backup = saveURL.appendingPathExtension("bak")
                    try? manager.removeItem(at: backup)
                    try? manager.copyItem(at: saveURL, to: backup)
                }
                try data.write(to: saveURL, options: .atomic)
            } catch {
                return "save import failed for \(entry.name): \(error.localizedDescription)"
            }
            return "imported \(name) for \(entry.name) (\(note)); the old save is kept as .bak"
        }
        // A running copy would write its own storage over the import as it closed, so it is
        // closed first and started again from the imported save.
        if let session = webPlayer, session.entry == entry {
            webPlayer = nil
            webPlayerVolumeWatch = nil
            session.finish { [weak self] in
                guard let self else { return }
                self.status = apply() + "; restarting from it"
                if self.activeEntry == entry, let root = Self.webPlayerRoot(kind) {
                    self.startWebPlayerSession(entry: entry, kind: kind, root: root, saveURL: saveURL)
                }
            }
            return "importing \(name) for \(entry.name): closing the running game first"
        }
        return apply()
    }

    /// The save of `entry` as a file for the share sheet, in Manic's format (it is stored in it).
    func exportWebPlayerSave(for entry: LibraryEntry, kind: WebPlayerKind) -> (URL?, String) {
        guard let saveURL = Self.webPlayerSaveURL(kind: kind, entry: entry),
              FileManager.default.fileExists(atPath: saveURL.path) else {
            return (nil, "\(entry.name) has no save yet. A save is made from the game's own menu, "
                    + "and kept automatically while it runs")
        }
        guard let outDir = SaveStateDisk.exportDirectory() else {
            return (nil, "save export failed: no temporary directory")
        }
        let url = outDir.appendingPathComponent(saveURL.lastPathComponent)
        do {
            try? FileManager.default.removeItem(at: url)
            try FileManager.default.copyItem(at: saveURL, to: url)
            let size = (try? FileManager.default.attributesOfItem(atPath: url.path)[.size] as? Int64) ?? 0
            return (url, "exported the save for \(entry.name) as .\(kind.saveExtension), "
                    + "\(SaveStates.byteText(size))")
        } catch {
            return (nil, "save export failed: \(error.localizedDescription)")
        }
    }
}

// MARK: - The settings sheet

/// Flash settings and J2ME settings: what each pad button sends, the J2ME phone type and screen
/// size, and the save file in Manic's format.
struct WebPlayerSettingsSheet: View {
    @ObservedObject var host: EngineHost
    let session: WebPlayerSession
    @State private var settings: WebPlayerGameSettings
    @State private var table: [Int: String]
    @State private var line = ""
    private let choices: [PlayerKeyChoice]
    private let sizes: [String]
    private let phoneTypes: [String]

    init(host: EngineHost, session: WebPlayerSession) {
        self.host = host
        self.session = session
        let settings = session.settings
        _settings = State(initialValue: settings)
        let bindings = (try? playerBindings(system: session.kind.systemId,
                                            overrides: settings.keyOverrides)) ?? []
        _table = State(initialValue: Dictionary(uniqueKeysWithValues: bindings.map {
            (Int($0.slot), $0.token)
        }))
        choices = playerKeyChoices(system: session.kind.systemId)
        sizes = playerJ2meScreenSizes()
        phoneTypes = playerJ2mePhoneTypes()
    }

    private var slotNames: [Int: String] {
        session.kind == .j2me ? WebPlayerKeys.j2meSlotNames : WebPlayerKeys.slotNames
    }

    var body: some View {
        NavigationView {
            Form {
                if session.kind == .j2me {
                    Section {
                        Picker("Screen size", selection: $settings.screenSize) {
                            Text("The game's own (or 240x320)").tag("")
                            ForEach(sizes, id: \.self) { Text($0).tag($0) }
                        }
                        Picker("Phone type", selection: $settings.phoneType) {
                            ForEach(phoneTypes, id: \.self) { type in
                                Text(type == "nokia" ? "Nokia (arrows and OK are their own keys)"
                                                     : "Standard (the D-pad is 2, 4, 6, 8 and OK is 5)")
                                    .tag(type)
                            }
                        }
                    } header: {
                        Text("Phone")
                    } footer: {
                        Text("A new screen size restarts the game; its save is kept.")
                    }
                }
                Section {
                    ForEach(0..<16, id: \.self) { slot in
                        Picker(slotNames[slot] ?? "Button \(slot)", selection: binding(for: slot)) {
                            Text("Nothing").tag(WebPlayerKeys.unbound)
                            ForEach(choices, id: \.token) { Text($0.label).tag($0.token) }
                        }
                    }
                    Button("Back to the default keys") {
                        let defaults = (try? playerBindings(system: session.kind.systemId,
                                                            overrides: "")) ?? []
                        table = Dictionary(uniqueKeysWithValues: defaults.map { (Int($0.slot), $0.token) })
                        commit()
                    }
                } header: {
                    Text(session.kind == .flash ? "What each button presses" : "What each button sends")
                } footer: {
                    Text("For this game only. Controllers use the same buttons.")
                }
                Section {
                    Button("Import a save (.\(session.kind.saveExtension))") { importSave() }
                    Button("Export the save as .\(session.kind.saveExtension)") { exportSave() }
                } header: {
                    Text("Save file, in Manic EMU's format")
                } footer: {
                    Text(session.kind.saveStatesUnavailable)
                }
                if !line.isEmpty {
                    Section { Text(line).font(.footnote) }
                }
            }
            .navigationTitle("\(session.kind.gameKind) settings")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") {
                        commit(restart: true)
                        host.webPlayerSettingsOpen = false
                    }
                }
            }
            .onChange(of: settings.phoneType) { _ in commit() }
        }
    }

    private func binding(for slot: Int) -> Binding<String> {
        Binding(get: { table[slot] ?? WebPlayerKeys.unbound },
                set: { table[slot] = $0; commit() })
    }

    private func commit(restart: Bool = false) {
        let bindings = (0..<16).map { PlayerBinding(slot: UInt8($0), token: table[$0] ?? WebPlayerKeys.unbound) }
        do {
            settings.keyOverrides = try playerBindingOverrides(system: session.kind.systemId,
                                                               bindings: bindings)
        } catch {
            line = "the remap was not saved: \(error)"
            return
        }
        host.setWebPlayerSettings(settings, restartIfNeeded: restart)
    }

    private func importSave() {
        let entry = session.entry
        let kind = session.kind
        FilePicker.shared.present(onPick: { data, name in
            let text = host.importWebPlayerSave(data, named: name, for: entry, kind: kind)
            line = text
            host.status = text
        }, onFailure: { reason in
            line = "save import failed: \(reason)"
        })
    }

    private func exportSave() {
        let (url, text) = host.exportWebPlayerSave(for: session.entry, kind: session.kind)
        line = text
        host.status = text
        if let url { FileShare.present(url) }
    }
}
#endif
