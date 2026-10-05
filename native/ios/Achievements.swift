// Continuum - RetroAchievements: the account, the network half of rcheevos, and what the player
// and the game card show.
//
// ## Who does what
//
// The engine owns rcheevos (vendored C, `crates/emulator-bridge/vendor/rcheevos`) and runs it: it
// hashes the game, evaluates every achievement after every frame against the core's memory, and
// decides when one unlocks. rcheevos never touches the network. When it wants the server it queues
// a request in the engine; THIS FILE takes those requests, performs them with URLSession, and hands
// each answer back by id. That is the whole of the Swift side of the protocol, and it is deliberately
// thin: Android will do the same loop with its own HTTP client.
//
// ## The password is never stored
//
// A password login is one request. The engine passes the password to rcheevos for that request and
// keeps nothing; the server answers with a TOKEN, and the token is what goes into the Keychain
// (`kSecClassGenericPassword`, this device only, readable after first unlock). The next launch logs
// in with the username and the token. Logging out deletes the token.
//
// ## Hardcore is off
//
// Hardcore mode forbids save states, rewind, cheats and slow motion, which are features of this app,
// so rcheevos is created in softcore and nothing here switches it on.

import Foundation
import Security
import SwiftUI

// MARK: - Keychain

/// The token, in the Keychain, keyed by username.
enum AchievementsKeychain {
    private static let service = "app.continuum.retroachievements"

    static func saveToken(_ token: String, for username: String) -> Bool {
        deleteToken(for: username)
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: username,
            kSecValueData as String: Data(token.utf8),
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
        ]
        return SecItemAdd(query as CFDictionary, nil) == errSecSuccess
    }

    static func token(for username: String) -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: username,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var item: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &item) == errSecSuccess,
              let data = item as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }

    @discardableResult
    static func deleteToken(for username: String) -> Bool {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: username,
        ]
        let status = SecItemDelete(query as CFDictionary)
        return status == errSecSuccess || status == errSecItemNotFound
    }
}

// MARK: - What the card shows for a game that is not running

/// One achievement as last seen, kept on disk so a game's card can list them without a game
/// running. Refreshed every time the game is played.
struct CachedAchievement: Codable, Identifiable, Hashable {
    let id: UInt32
    let title: String
    let detail: String
    let points: UInt32
    let unlocked: Bool
    let badgeURL: String

    init(_ entry: AchievementEntry) {
        id = entry.id
        title = entry.title
        detail = entry.description
        points = entry.points
        unlocked = entry.unlocked
        badgeURL = entry.unlocked ? entry.badgeUrl : entry.badgeLockedUrl
    }

    private enum CodingKeys: String, CodingKey { case id, title, detail, points, unlocked, badgeURL }

    /// Field by field, for the reason `SaveStateRecord.init(from:)` gives.
    init(from decoder: Decoder) throws {
        let box = try decoder.container(keyedBy: CodingKeys.self)
        id = try box.decodeIfPresent(UInt32.self, forKey: .id) ?? 0
        title = try box.decodeIfPresent(String.self, forKey: .title) ?? ""
        detail = try box.decodeIfPresent(String.self, forKey: .detail) ?? ""
        points = try box.decodeIfPresent(UInt32.self, forKey: .points) ?? 0
        unlocked = try box.decodeIfPresent(Bool.self, forKey: .unlocked) ?? false
        badgeURL = try box.decodeIfPresent(String.self, forKey: .badgeURL) ?? ""
    }
}

struct CachedAchievementSet: Codable {
    var title: String
    var seenAt: Double
    var achievements: [CachedAchievement]
}

enum AchievementsDisk {
    /// Where a game's list lives. Only a path: the folder is made by `write`, the one caller that
    /// needs it, so finding out whether a list exists never creates anything.
    static func url(gameId: String) -> URL? {
        guard let support = FileManager.default.urls(for: .applicationSupportDirectory,
                                                     in: .userDomainMask).first else { return nil }
        return support.appendingPathComponent("Achievements", isDirectory: true)
            .appendingPathComponent("\(ArtworkDisk.key(forPath: gameId)).json")
    }

    static func write(_ set: CachedAchievementSet, gameId: String) {
        guard let url = url(gameId: gameId),
              let data = try? JSONEncoder().encode(set) else { return }
        try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(),
                                                 withIntermediateDirectories: true)
        try? data.write(to: url, options: .atomic)
    }

    static func read(gameId: String) -> CachedAchievementSet? {
        guard let url = url(gameId: gameId), let data = try? Data(contentsOf: url) else {
            return nil
        }
        return try? JSONDecoder().decode(CachedAchievementSet.self, from: data)
    }
}

// MARK: - The store

/// A pop-up in the player when something unlocks.
struct AchievementToast: Identifiable, Equatable {
    let id = UUID()
    let title: String
    let detail: String
    let badgeURL: String
}

@MainActor
final class AchievementsStore: ObservableObject {

    /// The logged-in account's name, or nil.
    @Published private(set) var username: String?
    @Published private(set) var displayName = ""
    @Published private(set) var score: UInt32 = 0
    /// What the store last did, for Settings and the diagnostics. Never empty.
    @Published private(set) var line = "achievements: not logged in"
    /// The running game's set, as rcheevos reports it.
    @Published private(set) var gameTitle = ""
    @Published private(set) var gameProgress = ""
    /// The toast on screen in the player, if any.
    @Published var toast: AchievementToast?
    /// True while a login is waiting for the server: a password typed in Settings, or the stored
    /// token at launch.
    @Published private(set) var loggingIn = false
    /// The running game's list as the card draws it. Refreshed by the notices that change it (the
    /// set loading, an unlock), so drawing the card never takes the engine's lock.
    @Published private(set) var liveRows: [CachedAchievement] = []
    /// The last list seen for each game whose card has appeared, or that was played, this run,
    /// keyed by game id, so drawing a card never reads or decodes a file.
    @Published private(set) var savedSets: [String: CachedAchievementSet] = [:]

    private let engine: ContinuumEngine
    private weak var host: EngineHost?
    private var timer: Timer?
    private var toastQueue: [AchievementToast] = []
    /// The game the running set belongs to, so its list can be cached under the right id.
    private var currentGameId: String?
    /// The games whose list has been looked for on disk this run, found or not.
    private var savedSetsRead = Set<String>()
    /// The name the launch's TOKEN login is for, while it waits. Only that login's failure may
    /// forget the stored token: a mistyped password says nothing about it.
    private var tokenLoginName: String?
    /// Whether the server's answer to the login waiting now turned the credentials down, as
    /// opposed to never arriving. Set from the answer itself, because the engine's LoginFailed
    /// notice carries only a message, and "offline" must not cost a phone its stored login.
    private var loginRejected = false

    private static let usernameKey = "continuum.achievements.username.v1"

    /// One session for every request, with a timeout short enough that a dead network becomes a
    /// retry rather than a hang.
    private lazy var session: URLSession = {
        let configuration = URLSessionConfiguration.default
        configuration.timeoutIntervalForRequest = 30
        configuration.httpAdditionalHeaders = ["User-Agent": self.userAgent]
        return URLSession(configuration: configuration)
    }()

    /// RetroAchievements asks every client to say what it is: the app, then rcheevos' own clause.
    private lazy var userAgent: String = {
        let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString")
            as? String ?? "0"
        let clause = self.engine.achievementsUserAgentClause()
        return "Continuum/\(version) (iOS) \(clause)".trimmingCharacters(in: .whitespaces)
    }()

    init(engine: ContinuumEngine) {
        self.engine = engine
        let status = engine.achievementsStatus()
        if !status.available {
            line = "achievements: this build has no RetroAchievements support"
            return
        }
        // Back in with the stored token, if there is one. The password is never stored, so there
        // is nothing else this could do without asking.
        if let stored = UserDefaults.standard.string(forKey: Self.usernameKey),
           let token = AchievementsKeychain.token(for: stored) {
            do {
                try engine.achievementsLoginToken(username: stored, token: token)
                line = "achievements: logging in as \(stored)"
                tokenLoginName = stored
                loggingIn = true
                updatePumping()
            } catch {
                line = "achievements: could not log in as \(stored): \(error)"
            }
        }
    }

    func attach(host: EngineHost) {
        self.host = host
    }

    // MARK: Account

    func login(username: String, password: String) {
        // One at a time: rcheevos refuses a second login while one waits ("Login already in
        // progress"), and that refusal would end the first one's "Logging in..." early.
        guard !loggingIn else { return }
        let name = username.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty, !password.isEmpty else {
            report("achievements: type a username and a password")
            return
        }
        do {
            loginRejected = false
            try engine.achievementsLoginPassword(username: name, password: password)
            loggingIn = true
            report("achievements: logging in as \(name)")
            updatePumping()
        } catch {
            report("achievements: the login did not start: \(error)")
        }
    }

    func logout() {
        if let name = username ?? UserDefaults.standard.string(forKey: Self.usernameKey) {
            AchievementsKeychain.deleteToken(for: name)
        }
        UserDefaults.standard.removeObject(forKey: Self.usernameKey)
        engine.achievementsLogout()
        username = nil
        loggingIn = false
        tokenLoginName = nil
        loginRejected = false
        displayName = ""
        score = 0
        gameTitle = ""
        gameProgress = ""
        liveRows = []
        // One last poll, so anything rcheevos queued before the logout is still sent and said,
        // and then nothing until the next login: a logged-out store has nothing to poll for.
        pump()
        updatePumping()
        report("achievements: logged out; the stored token was deleted")
    }

    // MARK: Games

    /// Called by the launch path once a game is running.
    func gameStarted(entry: LibraryEntry, system: GameSystem?) {
        gameTitle = ""
        gameProgress = ""
        liveRows = []
        currentGameId = SaveStates.gameId(for: entry)
        guard username != nil else { return }
        // Before the load, so the requests it queues are collected whatever happens below.
        startPumping()
        guard let system else {
            line = "achievements: no system is mapped for \(entry.name)"
            return
        }
        do {
            try engine.achievementsLoadGame(system: system.rawValue, path: entry.path)
            line = "achievements: identifying \(entry.name)"
        } catch {
            line = "achievements: \(entry.name) was not identified: \(error)"
        }
    }

    /// The running game's achievements, live, from the engine (it takes the engine's lock; the
    /// card draws `liveRows` instead).
    func liveList() -> [AchievementEntry] {
        engine.achievementsList()
    }

    /// What the card shows for a game that is not running: the last list seen for it. Only a
    /// lookup; `loadCachedSet(for:)` fills it.
    func cachedSet(for entry: LibraryEntry) -> CachedAchievementSet? {
        savedSets[SaveStates.gameId(for: entry)]
    }

    /// Reads a game's last-seen list from disk the first time its card appears this run. Called
    /// from the card's onAppear, never from a body, which SwiftUI runs on every redraw.
    func loadCachedSet(for entry: LibraryEntry) {
        let gameId = SaveStates.gameId(for: entry)
        guard savedSetsRead.insert(gameId).inserted else { return }
        if let set = AchievementsDisk.read(gameId: gameId) {
            savedSets[gameId] = set
        }
    }

    // MARK: The network loop

    /// Polls the engine for requests and notices four times a second, but only while there can be
    /// something to poll for: while logged in, and while a login waits for the server. Logged out,
    /// the timer is gone, so it does not wake the phone four times a second for nothing.
    ///
    /// A timer rather than a hook in the display link, so this file needs nothing from the render
    /// loop: unlock notices are raised inside the engine's own tick and simply wait in its queue
    /// until the next poll, a quarter of a second at most.
    private func updatePumping() {
        if username != nil || loggingIn {
            startPumping()
        } else {
            stopPumping()
        }
    }

    private func startPumping() {
        guard timer == nil else { return }
        let timer = Timer(timeInterval: 0.25, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.pump() }
        }
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer
    }

    private func stopPumping() {
        timer?.invalidate()
        timer = nil
    }

    private func pump() {
        for request in engine.achievementsTakeRequests() {
            send(request)
        }
        for notice in engine.achievementsTakeNotices() {
            handle(notice)
        }
    }

    private func send(_ request: AchievementRequest) {
        let isLogin = Self.isLogin(request)
        guard let url = URL(string: request.url) else {
            try? engine.achievementsCompleteRequest(id: request.id, httpStatus: -1, body: Data())
            return
        }
        var urlRequest = URLRequest(url: url)
        if let body = request.postData {
            urlRequest.httpMethod = "POST"
            urlRequest.httpBody = Data(body.utf8)
            urlRequest.setValue(request.contentType ?? "application/x-www-form-urlencoded",
                                forHTTPHeaderField: "Content-Type")
        }
        let id = request.id
        session.dataTask(with: urlRequest) { [weak self] data, response, _ in
            // -2 is rcheevos' "no answer at all, retry it" (RC_API_SERVER_RESPONSE_RETRYABLE_CLIENT_ERROR).
            let status = (response as? HTTPURLResponse).map { Int32($0.statusCode) } ?? -2
            let body = data ?? Data()
            Task { @MainActor in
                guard let self else { return }
                // Before the engine sees it: completing the login raises its LoginFailed, which the
                // poll below handles, and that needs to know what the server said.
                if isLogin { self.loginRejected = Self.rejectsCredentials(body) }
                do {
                    try self.engine.achievementsCompleteRequest(id: id, httpStatus: status,
                                                                body: body)
                } catch {
                    self.line = "achievements: a server answer was not accepted: \(error)"
                    if isLogin { self.loginAnswerLost(error) }
                }
                self.pump()
            }
        }.resume()
    }

    /// Whether a request is rcheevos' login call (`r=login2`, by password or by token).
    private static func isLogin(_ request: AchievementRequest) -> Bool {
        guard let post = request.postData else { return false }
        return post.split(separator: "&").contains { $0 == "r=login2" || $0 == "r=login" }
    }

    /// Whether a login answer turned the credentials down: the server's own codes for a wrong or
    /// expired password or token, the two rcheevos reports as RC_INVALID_CREDENTIALS and
    /// RC_EXPIRED_TOKEN. No answer at all, an error page or any other code is not, so a phone
    /// that is offline at launch keeps its stored login.
    private static func rejectsCredentials(_ body: Data) -> Bool {
        guard let object = (try? JSONSerialization.jsonObject(with: body)) as? [String: Any],
              let code = object["Code"] as? String else { return false }
        return code == "invalid_credentials" || code == "expired_token"
    }

    /// A login whose answer the engine would not take: no LoginFailed will ever come for it, so
    /// it ends here, and the engine is logged out of it so the next try is not refused as
    /// "already in progress".
    private func loginAnswerLost(_ error: Error) {
        guard loggingIn, username == nil else { return }
        engine.achievementsLogout()
        loggingIn = false
        tokenLoginName = nil
        loginRejected = false
        updatePumping()
        report("achievements: the login's answer was not accepted (\(error)); log in again")
    }

    private func handle(_ notice: AchievementNotice) {
        switch notice {
        case let .loginSucceeded(name, display, token, total, softcore):
            loggingIn = false
            tokenLoginName = nil
            loginRejected = false
            username = name
            // Logged in: the poll runs from now until the logout.
            updatePumping()
            displayName = display.isEmpty ? name : display
            score = total + softcore
            UserDefaults.standard.set(name, forKey: Self.usernameKey)
            let kept = AchievementsKeychain.saveToken(token, for: name)
            report(kept
                   ? "achievements: logged in as \(displayName), \(score) points"
                   : "achievements: logged in as \(displayName), but the Keychain refused the "
                     + "token, so you will be asked again next launch")
            // A game already running when the login lands gets identified now.
            if let host, host.running, let entry = host.activeEntry {
                gameStarted(entry: entry, system: host.activeSystem)
            }
        case let .loginFailed(message):
            loggingIn = false
            let tokenName = tokenLoginName
            tokenLoginName = nil
            let rejected = loginRejected
            loginRejected = false
            updatePumping()
            if let tokenName, rejected {
                // The server turned the stored token itself down (wrong or expired), which no
                // retry changes: forget it and the name, so the next launch does not try the dead
                // token again, and ask for a login. A login that got no answer keeps both.
                AchievementsKeychain.deleteToken(for: tokenName)
                UserDefaults.standard.removeObject(forKey: Self.usernameKey)
                report("RetroAchievements login expired, log in again in Settings")
            } else {
                report("achievements: login failed: \(message)")
            }
        case let .gameLoaded(_, title, _, _, total, unlocked, pointsTotal, pointsUnlocked):
            gameTitle = title
            gameProgress = total == 0
                ? "no achievements in this set yet"
                : "\(unlocked) of \(total) unlocked, \(pointsUnlocked) of \(pointsTotal) points"
            report("achievements: \(title), \(gameProgress)")
            cacheList()
        case let .gameLoadFailed(message):
            report("achievements: this game was not identified (\(message)). A hack, a "
                   + "translation or a bad dump usually has no set.")
        case let .unlocked(_, title, description, points, badgeUrl):
            enqueueToast(AchievementToast(title: "Unlocked: \(title)",
                                          detail: "\(description) (\(points) points)",
                                          badgeURL: badgeUrl))
            host?.status = "achievement unlocked: \(title)"
            cacheList()
        case .gameCompleted:
            enqueueToast(AchievementToast(title: "Set complete",
                                          detail: "Every achievement in \(gameTitle) is unlocked.",
                                          badgeURL: ""))
        case let .progress(_, title, progress):
            line = "achievements: \(title), \(progress)"
        case let .leaderboardStarted(title):
            line = "achievements: leaderboard attempt started: \(title)"
        case let .leaderboardFailed(title):
            line = "achievements: leaderboard attempt failed: \(title)"
        case let .leaderboardSubmitted(title, result):
            enqueueToast(AchievementToast(title: "Leaderboard: \(title)", detail: result,
                                          badgeURL: ""))
        case let .serverError(api, message):
            report("achievements: the server refused \(api): \(message)")
        case .disconnected:
            report("achievements: offline; unlocks are kept and sent when the network is back")
        case .reconnected:
            report("achievements: back online; queued unlocks were sent")
        }
    }

    /// Takes the running set's list from the engine (on the two notices that change it) for the
    /// card, and keeps it on disk and in `savedSets` for when the game is not running.
    private func cacheList() {
        let list = engine.achievementsList()
        liveRows = list.map { CachedAchievement($0) }
        guard let gameId = currentGameId, !list.isEmpty else { return }
        let set = CachedAchievementSet(title: gameTitle, seenAt: Date().timeIntervalSince1970,
                                       achievements: liveRows)
        AchievementsDisk.write(set, gameId: gameId)
        savedSets[gameId] = set
        savedSetsRead.insert(gameId)
    }

    // MARK: Toasts

    private func enqueueToast(_ toast: AchievementToast) {
        toastQueue.append(toast)
        if self.toast == nil { showNextToast() }
    }

    private func showNextToast() {
        guard !toastQueue.isEmpty else {
            toast = nil
            return
        }
        toast = toastQueue.removeFirst()
        let shown = toast?.id
        DispatchQueue.main.asyncAfter(deadline: .now() + 4) { [weak self] in
            Task { @MainActor in
                guard let self, self.toast?.id == shown else { return }
                self.showNextToast()
            }
        }
    }

    private func report(_ text: String) {
        line = text
        host?.status = text
    }
}

// MARK: - The toast, in the player

/// The unlock banner. Mounted by the player screen; draws nothing when there is no toast, and
/// never takes a touch, so a banner over the controls cannot eat a button press.
struct AchievementToastOverlay: View {
    @ObservedObject var store: AchievementsStore

    var body: some View {
        VStack {
            if let toast = store.toast {
                HStack(spacing: 10) {
                    BadgeImage(url: toast.badgeURL, size: 40)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(toast.title)
                            .font(.system(size: 14, weight: .bold))
                            .foregroundStyle(.white)
                        Text(toast.detail)
                            .font(.system(size: 12))
                            .foregroundStyle(Color.white.opacity(0.8))
                            .lineLimit(2)
                    }
                    Spacer(minLength: 0)
                }
                .padding(10)
                .background(.black.opacity(0.8), in: RoundedRectangle(cornerRadius: 12))
                .overlay(RoundedRectangle(cornerRadius: 12)
                    .strokeBorder(Color.yellow.opacity(0.7), lineWidth: 1))
                .padding(.horizontal, 16)
                .transition(.move(edge: .top).combined(with: .opacity))
            }
            Spacer()
        }
        .padding(.top, 54)
        .animation(.easeInOut(duration: 0.25), value: store.toast)
        .allowsHitTesting(false)
    }
}

/// An achievement badge from the RetroAchievements media server, or a trophy while it loads.
struct BadgeImage: View {
    let url: String
    let size: CGFloat

    var body: some View {
        Group {
            if let parsed = URL(string: url), !url.isEmpty {
                AsyncImage(url: parsed) { image in
                    image.resizable().interpolation(.none)
                } placeholder: {
                    Image(systemName: "trophy.fill").foregroundStyle(.yellow)
                }
            } else {
                Image(systemName: "trophy.fill").foregroundStyle(.yellow)
            }
        }
        .frame(width: size, height: size)
        .clipShape(RoundedRectangle(cornerRadius: 6))
    }
}

// MARK: - Settings

/// The account: log in, see who is logged in, log out.
struct AchievementsSettingsSection: View {
    @ObservedObject var store: AchievementsStore
    @State private var username = ""
    @State private var password = ""

    var body: some View {
        SettingsSection(title: "RETROACHIEVEMENTS") {
            if let name = store.username {
                SettingsReadout(label: "Logged in as",
                                value: "\(store.displayName.isEmpty ? name : store.displayName), "
                                    + "\(store.score) points")
                SettingsButton(title: "Log out", role: .destructive) { store.logout() }
            } else {
                TextField("Username", text: $username)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled(true)
                    .padding(10)
                    .background(ShellPalette.surfaceStrong, in: RoundedRectangle(cornerRadius: 9))
                    .foregroundStyle(.white)
                SecureField("Password", text: $password)
                    .padding(10)
                    .background(ShellPalette.surfaceStrong, in: RoundedRectangle(cornerRadius: 9))
                    .foregroundStyle(.white)
                SettingsButton(title: store.loggingIn ? "Logging in..." : "Log in",
                               role: .normal) {
                    store.login(username: username, password: password)
                    // Cleared at once: the password is not kept anywhere, including this field.
                    password = ""
                }
                // Off while a login waits: a second tap would start a second login, which
                // rcheevos refuses, and that refusal would end the first one's wait early.
                .disabled(store.loggingIn)
                .opacity(store.loggingIn ? 0.5 : 1)
            }
            SettingsReadout(label: "Status", value: store.line)
            SettingsNote(
                "Uses your retroachievements.org account. The password is sent once, to log in, and "
                + "is never stored: the server returns a token, and only the token is kept, in the "
                + "Keychain on this device. Hardcore mode is off, because it forbids save states, "
                + "rewind and cheats. A game is identified by its file when it starts."
            )
        }
    }
}

// MARK: - The game card

/// The achievements block on a game's card: the live list when it is running, otherwise the list
/// as it was the last time it was played.
struct AchievementsCardBlock: View {
    let entry: LibraryEntry
    @ObservedObject var host: EngineHost
    @ObservedObject var store: AchievementsStore

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("ACHIEVEMENTS")
                .font(.system(size: 11, weight: .bold))
                .tracking(1.6)
                .foregroundStyle(ShellPalette.secondaryText)
            content
        }
        .padding(14)
        .background(ShellPalette.surface, in: RoundedRectangle(cornerRadius: 12))
        // The saved list is read here, once per game, and not in `content`: SwiftUI runs that on
        // every change the store or the host publishes, and it used to take the engine's lock or
        // read and decode a file each time.
        .onAppear { store.loadCachedSet(for: entry) }
        .onChange(of: entry) { shown in store.loadCachedSet(for: shown) }
    }

    @ViewBuilder
    private var content: some View {
        let running = host.running && host.activeEntry?.id == entry.id
        let rows: [CachedAchievement] = running
            ? store.liveRows
            : (store.cachedSet(for: entry)?.achievements ?? [])
        if store.username == nil {
            SettingsNote("Log in to RetroAchievements in Settings to see this game's achievements.")
        } else if rows.isEmpty {
            SettingsNote(running
                         ? "No achievements are loaded for this game. \(store.line)"
                         : "Play the game once while logged in and its achievements are listed "
                           + "here.")
        } else {
            let unlocked = rows.filter { $0.unlocked }.count
            SettingsReadout(label: running ? "This session" : "Last seen",
                            value: "\(unlocked) of \(rows.count) unlocked")
            ForEach(rows) { row in
                HStack(spacing: 10) {
                    BadgeImage(url: row.badgeURL, size: 36)
                        .opacity(row.unlocked ? 1 : 0.5)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(row.title)
                            .font(.system(size: 13, weight: .semibold))
                            .foregroundStyle(row.unlocked ? Color.white : ShellPalette.secondaryText)
                        Text(row.detail)
                            .font(.system(size: 11))
                            .foregroundStyle(ShellPalette.secondaryText)
                            .lineLimit(2)
                    }
                    Spacer(minLength: 4)
                    Text("\(row.points)")
                        .font(.system(.caption, design: .monospaced))
                        .foregroundStyle(ShellPalette.metadata)
                }
            }
        }
    }
}
