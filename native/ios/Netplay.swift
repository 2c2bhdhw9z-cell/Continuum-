// Continuum - online play for two players, the transport half.
//
// The protocol is entirely in Rust (`crates/emulator-bridge/src/netplay`): deterministic lockstep
// with a small input delay, the starting-state handshake, the per-frame input exchange, desync
// checksums, keepalive and every status line. This file only moves the bytes the engine produces
// over a TCP connection from the Network framework and hands back the bytes that arrive. It never
// looks inside them, so an Android build can carry the same bytes over its own sockets.
//
// THE NETWORK HALF RUNS OFF THE MAIN THREAD. `NetplayLink` owns the listener, the connection and
// the browser on its own serial queue and calls the engine directly from there (the engine is a
// Mutex, and every netplay call is a few microseconds). That keeps an arriving input from waiting
// for the next main run-loop turn. The main actor only reads the status for the HUD, throttled,
// and asks the link to flush after each display-link tick.
//
// Host: listens on `netplayDefaultPort()` (or any free port if that one is taken), shows its
// Wi-Fi address and port, and advertises `_continuum._tcp` over Bonjour. Join: type the address,
// or tap a host found on the local network. The same game must be loaded on both phones; the
// engine refuses a mismatch by name. Player 2's input goes to port 1 on both phones.
//
// If player 2's connection drops, the host listens and advertises again on the same port, and its
// game runs on alone meanwhile. When player 2 connects again the host opens a fresh engine session
// (a lost one is over for good) and the handshake sends the host's game as it is by then.

import Darwin
import Foundation
import Network
import SwiftUI
import UIKit

// MARK: - Transport

final class NetplayLink: @unchecked Sendable {
    private let engine: ContinuumEngine
    private let queue = DispatchQueue(label: "dev.continuum.netplay", qos: .userInteractive)
    private var listener: NWListener?
    private var connection: NWConnection?
    private var browser: NWBrowser?

    // The host's side. Like the three above, only ever touched on `queue`.

    /// From `listen` until `close`. While it is set, a guest whose connection drops is listened
    /// for again rather than being the end of online play.
    private var hosting = false
    private var serviceName = ""
    /// Whether the current listener ever reached `.ready`. Only one that never opened may move to
    /// another port: once open, its port is the address player 2 has been given.
    private var listenerReady = false
    /// The port the last listener was open on, so a returning player 2 finds the address they know.
    private var hostPort: UInt16 = 0
    /// Player 2 left and this phone is listening for them again.
    private var awaitingRejoin = false
    /// A phone that came back, accepted but NOT started, until the controller has opened a fresh
    /// engine session for it. See `admitReturningGuest`.
    private var returningGuest: NWConnection?

    /// Written on the link queue, read on main by the controller through `snapshot`.
    private let lock = NSLock()
    private var _listeningPort: UInt16 = 0
    private var _transportNote = ""
    private var _nearby: [NetplayNearbyHost] = []
    private var _browseNote = ""
    private var _awaitingRejoin = false
    /// Set on the link queue when `returningGuest` is parked, cleared on main by
    /// `claimReturningGuest`, so each return is handled exactly once.
    private var _guestReturned = false

    init(engine: ContinuumEngine) {
        self.engine = engine
    }

    struct Snapshot {
        let listeningPort: UInt16
        let transportNote: String
        let nearby: [NetplayNearbyHost]
        let browseNote: String
        /// The host's player 2 left and this phone is listening for them to come back.
        let awaitingRejoin: Bool
    }

    func snapshot() -> Snapshot {
        lock.lock(); defer { lock.unlock() }
        return Snapshot(listeningPort: _listeningPort, transportNote: _transportNote,
                        nearby: _nearby, browseNote: _browseNote,
                        awaitingRejoin: _awaitingRejoin)
    }

    private func set(_ body: () -> Void) {
        lock.lock(); body(); lock.unlock()
    }

    static func parameters() -> NWParameters {
        let tcp = NWProtocolTCP.Options()
        // Inputs are a few dozen bytes each frame; Nagle would hold them back for an ACK.
        tcp.noDelay = true
        tcp.enableKeepalive = true
        let parameters = NWParameters(tls: nil, tcp: tcp)
        parameters.includePeerToPeer = true
        return parameters
    }

    // ---------------------------------------------------------------- host

    func listen(serviceName: String) {
        queue.async { [self] in
            hosting = true
            self.serviceName = serviceName
            hostPort = 0
            endRejoinWait()
            startListener(port: netplayDefaultPort(), fallBackToAnyPort: true)
        }
    }

    /// Opens the listener and its Bonjour advert on `port`, or on any free port when it is nil.
    /// The fixed port comes first, so the address can be typed from memory. With
    /// `fallBackToAnyPort`, a port already in use moves to any free one: here when the listener
    /// cannot be made, and in the state handler when it fails before it ever opened, because
    /// Network.framework reports a busy port later as `.failed` rather than throwing.
    private func startListener(port: UInt16?, fallBackToAnyPort: Bool) {
        stopListening()
        listenerReady = false
        let parameters = Self.parameters()
        let made: NWListener?
        if let port, let fixed = NWEndpoint.Port(rawValue: port) {
            made = try? NWListener(using: parameters, on: fixed)
        } else {
            made = try? NWListener(using: parameters)
        }
        guard let listener = made else {
            if fallBackToAnyPort {
                startListener(port: nil, fallBackToAnyPort: false)
                return
            }
            hosting = false
            endRejoinWait()
            set { _transportNote = "could not open a port to listen on" }
            engine.netplayTransportLost(reason: "this phone could not open a port to listen on")
            return
        }
        // Bonjour names are capped at 63 bytes.
        var name = serviceName
        while name.utf8.count > 63 { name.removeLast() }
        listener.service = NWListener.Service(name: name, type: netplayBonjourType())
        listener.stateUpdateHandler = { [weak self, weak listener] state in
            guard let self, let listener, self.listener === listener else { return }
            switch state {
            case .ready:
                self.listenerReady = true
                let bound = listener.port?.rawValue ?? 0
                if bound != 0 { self.hostPort = bound }
                self.set {
                    self._listeningPort = bound
                    self._transportNote = ""
                }
            case .failed(let error):
                if fallBackToAnyPort, !self.listenerReady {
                    let busy = port ?? netplayDefaultPort()
                    self.set { self._transportNote = "port \(busy) is busy, using another" }
                    self.startListener(port: nil, fallBackToAnyPort: false)
                } else {
                    // A listener that was open keeps its port, even failing: moving it would
                    // silently change the address player 2 was given. It is reported instead.
                    self.stopListening()
                    self.hosting = false
                    self.endRejoinWait()
                    self.set { self._transportNote = "listening failed: \(error)" }
                    self.engine.netplayTransportLost(reason: "listening failed (\(error))")
                }
            case .waiting(let error):
                self.set { self._transportNote = "waiting for the network: \(error)" }
            default:
                break
            }
        }
        listener.newConnectionHandler = { [weak self] incoming in
            guard let self else { return }
            if self.connection != nil || self.returningGuest != nil {
                // One guest only. A third phone is turned away rather than replacing player 2.
                incoming.cancel()
                return
            }
            // Stop advertising once player 2 is here.
            self.stopListening()
            if self.awaitingRejoin {
                // Player 2 coming back. The engine ended its session for good when they left,
                // so this connection waits, not started, until the controller has opened a
                // fresh host session for it on the main thread.
                self.returningGuest = incoming
                self.set { self._guestReturned = true }
            } else {
                self.adopt(incoming)
            }
        }
        self.listener = listener
        listener.start(queue: queue)
    }

    /// Not waiting for a returning player 2 any more: they were let in, or hosting ended.
    private func endRejoinWait() {
        awaitingRejoin = false
        set {
            _awaitingRejoin = false
            _guestReturned = false
        }
    }

    /// Main thread. True once for each phone that came back while this host was waiting for it,
    /// so the controller opens exactly one fresh engine session per return.
    func claimReturningGuest() -> Bool {
        lock.lock(); defer { lock.unlock() }
        guard _guestReturned else { return false }
        _guestReturned = false
        return true
    }

    /// Starts the returning phone's connection, once the engine has a fresh host session. From
    /// here it is a first join again: `.ready` says `netplayTransportConnected`, the guest says
    /// hello, and the host's current state goes over as the new starting state.
    func admitReturningGuest() {
        queue.async { [self] in
            guard let incoming = returningGuest else { return }
            returningGuest = nil
            endRejoinWait()
            adopt(incoming)
        }
    }

    private func stopListening() {
        listener?.cancel()
        listener = nil
        set { _listeningPort = 0 }
    }

    // ---------------------------------------------------------------- join

    func connect(to endpoint: NWEndpoint) {
        queue.async { [self] in
            adopt(NWConnection(to: endpoint, using: Self.parameters()))
        }
    }

    private func adopt(_ connection: NWConnection) {
        self.connection?.cancel()
        self.connection = connection
        connection.stateUpdateHandler = { [weak self, weak connection] state in
            guard let self, let connection, self.connection === connection else { return }
            switch state {
            case .ready:
                self.set { self._transportNote = "" }
                self.engine.netplayTransportConnected()
                self.flushOnQueue()
                self.receive(on: connection)
            case .waiting(let error):
                self.set { self._transportNote = "waiting for the network: \(error)" }
            case .failed(let error):
                self.lost("\(error)")
                // A failed connection still holds its resources until cancelled. `lost` has
                // already let go of it, so the `.cancelled` this causes is ignored above.
                connection.cancel()
            case .cancelled:
                self.lost("the connection was closed")
            default:
                break
            }
        }
        connection.start(queue: queue)
    }

    private func lost(_ reason: String) {
        connection = nil
        engine.netplayTransportLost(reason: reason)
        set { _transportNote = reason }
        // A host's player 2 dropped. Listen and advertise again on the same port, so they can
        // come back from Nearby or by the address they already have. The game is not touched:
        // the engine's session has just ended, so the host's game runs on alone until they do.
        if hosting {
            awaitingRejoin = true
            set { _awaitingRejoin = true }
            startListener(port: hostPort == 0 ? netplayDefaultPort() : hostPort,
                          fallBackToAnyPort: true)
        }
    }

    private func receive(on connection: NWConnection) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 256 * 1024) {
            [weak self, weak connection] data, _, isComplete, error in
            guard let self, let connection, self.connection === connection else { return }
            if let data, !data.isEmpty {
                self.engine.netplayReceive(data: data)
                // A ping or the handshake can need an answer before the next display-link tick.
                self.flushOnQueue()
            }
            if let error {
                self.lost("\(error)")
                connection.cancel()
            } else if isComplete {
                self.lost("the other phone closed the connection")
                connection.cancel()
            } else {
                self.receive(on: connection)
            }
        }
    }

    // ---------------------------------------------------------------- sending

    /// Sends whatever the engine has queued. Called after every display-link tick.
    func flush() {
        queue.async { [self] in flushOnQueue() }
    }

    private func flushOnQueue() {
        guard let connection else { return }
        let bytes = engine.netplayTakeOutgoing()
        guard !bytes.isEmpty else { return }
        connection.send(content: bytes, completion: .contentProcessed { [weak self] error in
            if let error { self?.set { self?._transportNote = "send failed: \(error)" } }
        })
    }

    /// Sends the goodbye, then closes everything, including the wait for a returning player 2.
    func close(sendingLast bytes: Data) {
        queue.async { [self] in
            hosting = false
            endRejoinWait()
            returningGuest?.cancel()
            returningGuest = nil
            stopListening()
            guard let connection else { return }
            self.connection = nil
            if bytes.isEmpty {
                connection.cancel()
            } else {
                connection.send(content: bytes, completion: .contentProcessed { _ in
                    connection.cancel()
                })
            }
        }
    }

    // ---------------------------------------------------------------- Bonjour

    func startBrowsing() {
        queue.async { [self] in
            guard browser == nil else { return }
            let browser = NWBrowser(for: .bonjour(type: netplayBonjourType(), domain: nil),
                                    using: Self.parameters())
            browser.browseResultsChangedHandler = { [weak self] results, _ in
                let hosts = results.compactMap { result -> NetplayNearbyHost? in
                    if case let .service(name, _, _, _) = result.endpoint {
                        return NetplayNearbyHost(id: name, name: name, endpoint: result.endpoint)
                    }
                    return nil
                }.sorted { $0.name < $1.name }
                self?.set { self?._nearby = hosts }
            }
            browser.stateUpdateHandler = { [weak self] state in
                switch state {
                case .ready:
                    self?.set { self?._browseNote = "searching the local network" }
                case .failed(let error):
                    self?.set {
                        self?._browseNote = "nearby search failed (\(error)); check Settings > "
                            + "Privacy > Local Network for Continuum"
                    }
                case .waiting(let error):
                    self?.set { self?._browseNote = "nearby search waiting: \(error)" }
                default:
                    break
                }
            }
            self.browser = browser
            browser.start(queue: queue)
        }
    }

    func stopBrowsing() {
        queue.async { [self] in
            browser?.cancel()
            browser = nil
            set {
                _nearby = []
                _browseNote = ""
            }
        }
    }

    // ---------------------------------------------------------------- addresses

    /// This phone's IPv4 addresses on Wi-Fi (en0) and Personal Hotspot (bridge100), best first.
    static func localAddresses() -> [String] {
        var head: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&head) == 0, let first = head else { return [] }
        defer { freeifaddrs(head) }
        var found: [(rank: Int, address: String)] = []
        var cursor: UnsafeMutablePointer<ifaddrs>? = first
        while let entry = cursor {
            defer { cursor = entry.pointee.ifa_next }
            guard let address = entry.pointee.ifa_addr,
                  address.pointee.sa_family == UInt8(AF_INET) else { continue }
            let name = String(cString: entry.pointee.ifa_name)
            let rank: Int
            switch name {
            case "en0": rank = 0
            case let other where other.hasPrefix("bridge"): rank = 1
            case let other where other.hasPrefix("en"): rank = 2
            default: continue
            }
            var host = [CChar](repeating: 0, count: Int(NI_MAXHOST))
            if getnameinfo(address, socklen_t(address.pointee.sa_len), &host, socklen_t(host.count),
                           nil, 0, NI_NUMERICHOST) == 0 {
                found.append((rank, String(cString: host)))
            }
        }
        return found.sorted { $0.rank < $1.rank }.map(\.address)
    }

    /// `host`, `host:port` or `[v6]:port`, defaulting the port.
    static func parse(_ text: String) -> (host: String, port: UInt16)? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return nil }
        if trimmed.hasPrefix("["), let close = trimmed.firstIndex(of: "]") {
            let host = String(trimmed[trimmed.index(after: trimmed.startIndex)..<close])
            let rest = trimmed[trimmed.index(after: close)...]
            if rest.hasPrefix(":"), let port = UInt16(rest.dropFirst()) { return (host, port) }
            return rest.isEmpty ? (host, netplayDefaultPort()) : nil
        }
        let parts = trimmed.split(separator: ":", omittingEmptySubsequences: false)
        if parts.count == 2 {
            guard let port = UInt16(parts[1]), !parts[0].isEmpty else { return nil }
            return (String(parts[0]), port)
        }
        // One part is a bare host; more than two is an unbracketed IPv6 address.
        return (trimmed, netplayDefaultPort())
    }
}

struct NetplayNearbyHost: Identifiable, Hashable {
    let id: String
    let name: String
    let endpoint: NWEndpoint
}

// MARK: - The app-facing object

@MainActor
final class NetplayController: ObservableObject {
    private static let delayKey = "continuum.netplay.inputDelay.v1"
    private static let addressKey = "continuum.netplay.lastAddress.v1"

    @Published private(set) var kind: NetplayStatusKind = .idle
    @Published private(set) var line = "online: off"
    @Published private(set) var detailLine = ""
    @Published private(set) var addressLine = ""
    @Published private(set) var nearby: [NetplayNearbyHost] = []
    @Published private(set) var browseNote = ""
    @Published private(set) var isHost = false
    @Published var inputDelay: Int {
        didSet { UserDefaults.standard.set(inputDelay, forKey: Self.delayKey) }
    }
    @Published var joinAddress: String {
        didSet { UserDefaults.standard.set(joinAddress, forKey: Self.addressKey) }
    }

    var isActive: Bool { kind != .idle }

    private let engine: ContinuumEngine
    private let link: NetplayLink
    private weak var host: EngineHost?
    private var lastRefresh: CFTimeInterval = 0

    init(engine: ContinuumEngine) {
        self.engine = engine
        link = NetplayLink(engine: engine)
        let stored = UserDefaults.standard.object(forKey: Self.delayKey) as? Int
        inputDelay = stored ?? 2
        joinAddress = UserDefaults.standard.string(forKey: Self.addressKey) ?? ""
    }

    func attach(host: EngineHost) {
        self.host = host
    }

    /// Called after every display-link tick: send what the tick produced, and refresh the HUD a
    /// few times a second (publishing sixty times a second would rebuild the player for nothing).
    func afterTick() {
        guard isActive || engine.netplayIsLive() else { return }
        // Before the flush, so a player 2 who came back is let in on this tick.
        takeBackReturningGuest()
        link.flush()
        let now = CACurrentMediaTime()
        if now - lastRefresh > 0.25 {
            lastRefresh = now
            refresh()
        }
    }

    func refresh() {
        let status = engine.netplayStatus()
        let snap = link.snapshot()
        // The engine calls this "disconnected", which reads as the end of online play. It is not:
        // this phone is listening again (see `NetplayLink.lost`) and lets player 2 back in as soon
        // as they reconnect, so it is shown as a wait, with the reason it ended in the detail.
        let awaitingRejoin = snap.awaitingRejoin && status.role == .host
            && status.kind == .disconnected
        kind = awaitingRejoin ? .waiting : status.kind
        isHost = status.role == .host
        var text = awaitingRejoin
            ? "online: the other phone left; waiting for it to rejoin (your game carries on)"
            : status.line
        if !snap.transportNote.isEmpty, status.kind != .running {
            text += " (\(snap.transportNote))"
        }
        line = text
        nearby = snap.nearby
        browseNote = snap.browseNote
        if status.role == .host, snap.listeningPort != 0 {
            let addresses = NetplayLink.localAddresses()
            addressLine = addresses.isEmpty
                ? "Hosting on port \(snap.listeningPort), but this phone has no Wi-Fi address"
                : "Hosting at " + addresses.map { "\($0):\(snap.listeningPort)" }.joined(separator: " or ")
        } else if status.role == .host, status.kind == .waiting || awaitingRejoin {
            addressLine = "Opening a port..."
        } else {
            addressLine = ""
        }
        if awaitingRejoin {
            // Why the last session ended ("the other player left (...)", "refused player 2: ...").
            detailLine = status.line
        } else {
            let ping = status.pingMs >= 0 ? String(format: "%.0f ms", status.pingMs) : "--"
            detailLine = "frame \(status.frame), delay \(status.inputDelay), ping \(ping), "
                + "\(status.checksCompared) desync checks passed"
                + (status.desyncFrame.map { ", DESYNC at frame \($0)" } ?? "")
        }
        if host?.netplayLive != status.live {
            host?.netplayLive = status.live
        }
    }

    private func gameEntry() -> LibraryEntry? {
        guard let entry = host?.activeEntry else {
            line = "online: load a game first; online play starts from a running game"
            return nil
        }
        return entry
    }

    func hostGame() {
        guard let entry = gameEntry() else { return }
        do {
            try openHostSession(for: entry)
            link.listen(serviceName: "Continuum \(UIDevice.current.name): \(entry.name)")
            host?.status = "online: hosting \(entry.name), waiting for player 2"
        } catch {
            line = "online: could not host: \(error)"
        }
        refresh()
    }

    /// A fresh host session in the engine for the running game. Shared by `hostGame` and by
    /// taking a returning player 2 back, so both start from exactly the same place.
    private func openHostSession(for entry: LibraryEntry) throws {
        try engine.netplayHost(contentPath: entry.path, inputDelay: UInt32(max(0, inputDelay)),
                               checksumInterval: 60)
        host?.emulation.releaseHeldControls()
        // Pause is hidden while online, so a paused game is resumed rather than stranded.
        if host?.paused == true { host?.togglePause() }
    }

    /// Player 2 reconnected to this host after leaving. A lost connection ends the engine's
    /// session for good (`NetplaySession::transport_lost`), so a fresh one is opened on the same
    /// game, exactly as tapping Host does, and only then is their connection started: the
    /// handshake and the starting state (the host's game as it is now) follow as the first time.
    /// On the main thread, like `leave`, so a phone coming back and the host leaving can never
    /// interleave.
    private func takeBackReturningGuest() {
        guard link.claimReturningGuest() else { return }
        guard let entry = host?.activeEntry else {
            link.close(sendingLast: Data())
            return
        }
        do {
            try openHostSession(for: entry)
            link.admitReturningGuest()
            host?.status = "online: the other phone is back; sending it this game as it is now"
        } catch {
            link.close(sendingLast: Data())
            host?.status = "online: the other phone came back, but hosting could not restart: "
                + "\(error)"
        }
        refresh()
    }

    func join(address text: String) {
        guard let parsed = NetplayLink.parse(text),
              let nwPort = NWEndpoint.Port(rawValue: parsed.port) else {
            line = "online: \"\(text)\" is not an address; type it like 192.168.1.20:\(netplayDefaultPort())"
            return
        }
        joinAddress = text
        join(endpoint: .hostPort(host: NWEndpoint.Host(parsed.host), port: nwPort),
             label: "\(parsed.host):\(parsed.port)")
    }

    func join(nearby: NetplayNearbyHost) {
        join(endpoint: nearby.endpoint, label: nearby.name)
    }

    private func join(endpoint: NWEndpoint, label: String) {
        guard let entry = gameEntry() else { return }
        do {
            // This phone's own progress is kept first: the host's state is about to replace the
            // running game, and from then until the session stops the auto-save stays off.
            host?.saveStates.writeAutoSave(reason: "before joining online play")
            try engine.netplayJoin(contentPath: entry.path)
            host?.suppressAutoSave = true
            host?.emulation.releaseHeldControls()
            if host?.paused == true { host?.togglePause() }
            link.connect(to: endpoint)
            host?.status = "online: joining \(label) as player 2"
        } catch {
            line = "online: could not join: \(error)"
        }
        refresh()
    }

    /// Ends online play from this phone and closes the connection. The game carries on locally.
    func leave(reason: String = "the other player ended online play") {
        guard isActive || engine.netplayIsLive() else { return }
        engine.netplayLeave(reason: reason)
        let last = engine.netplayStop()
        link.close(sendingLast: last)
        refresh()
        line = "online: off (you ended online play)"
        host?.status = "online play ended"
    }

    /// Called before the session stops, so the other phone hears why rather than timing out.
    func endForLeavingGame() {
        leave(reason: "the other player went back to the library")
    }

    func startBrowsing() { link.startBrowsing() }
    func stopBrowsing() { link.stopBrowsing() }
}

// MARK: - HUD and sheet

/// One plain line on the player screen for every online state.
struct NetplayHUDLine: View {
    @ObservedObject var netplay: NetplayController

    var body: some View {
        if netplay.isActive {
            Text(netplay.line)
                .font(.system(.caption2, design: .monospaced))
                .foregroundStyle(tint)
                .lineLimit(2)
                .fixedSize(horizontal: false, vertical: true)
                .allowsHitTesting(false)
        }
    }

    private var tint: Color {
        switch netplay.kind {
        case .desynced, .disconnected: return ShellPalette.accent
        case .stalled: return Color.yellow
        case .running: return Color.green.opacity(0.9)
        default: return Color.white.opacity(0.8)
        }
    }
}

struct NetplaySheet: View {
    @ObservedObject var netplay: NetplayController
    let gameName: String
    let onClose: () -> Void

    var body: some View {
        NavigationView {
            Form {
                Section {
                    Text(netplay.line)
                        .font(.system(.footnote, design: .monospaced))
                    if !netplay.addressLine.isEmpty {
                        Text(netplay.addressLine)
                            .font(.system(.footnote, design: .monospaced))
                            .textSelection(.enabled)
                    }
                    if netplay.isActive {
                        Text(netplay.detailLine)
                            .font(.system(.caption, design: .monospaced))
                            .foregroundStyle(.secondary)
                    }
                } header: {
                    Text(gameName)
                }

                if netplay.isActive {
                    Section {
                        Button(role: .destructive) {
                            netplay.leave()
                        } label: {
                            Text(netplay.kind == .disconnected ? "Close online play" : "Leave online play")
                        }
                    }
                } else {
                    Section {
                        Stepper("Input delay: \(netplay.inputDelay) frame\(netplay.inputDelay == 1 ? "" : "s")",
                                value: $netplay.inputDelay, in: 0...8)
                        Button("Host this game (you are player 1)") {
                            netplay.hostGame()
                        }
                    } header: {
                        Text("Host")
                    } footer: {
                        Text("The other phone joins with the address shown here, or finds this phone "
                             + "under Nearby. More delay hides more network lag; 2 suits home Wi-Fi.")
                    }

                    Section {
                        TextField("192.168.1.20:\(netplayDefaultPort())", text: $netplay.joinAddress)
                            .keyboardType(.numbersAndPunctuation)
                            .autocorrectionDisabled(true)
                            .textInputAutocapitalization(.never)
                        Button("Join (you are player 2)") {
                            netplay.join(address: netplay.joinAddress)
                        }
                    } header: {
                        Text("Join by address")
                    }

                    Section {
                        if netplay.nearby.isEmpty {
                            Text(netplay.browseNote.isEmpty ? "No hosts found yet" : netplay.browseNote)
                                .foregroundStyle(.secondary)
                        }
                        ForEach(netplay.nearby) { nearby in
                            Button("Join \(nearby.name)") {
                                netplay.join(nearby: nearby)
                            }
                        }
                    } header: {
                        Text("Nearby")
                    }
                }

                Section {
                    Text("Both phones need the same ROM file and the same Continuum build, with "
                         + "cheats off. The host's game is copied to the other phone when you connect. "
                         + "Rewind, fast forward, reset and loading a state are off while you play online.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
            .navigationTitle("Online play")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { onClose() }
                }
            }
        }
        .onAppear {
            netplay.refresh()
            netplay.startBrowsing()
        }
        .onDisappear { netplay.stopBrowsing() }
    }
}

/// Settings entry for online play. The session itself starts from a running game, because the
/// game is what both phones have to agree on.
struct OnlinePlaySettingsSection: View {
    @ObservedObject var netplay: NetplayController

    var body: some View {
        SettingsSection(title: "ONLINE PLAY") {
            SettingsReadout(label: "Now", value: netplay.isActive ? netplay.line : "off")
            SettingsReadout(label: "Input delay", value: "\(netplay.inputDelay) frames")
            SettingsNote("Two players on two phones. Load the same game on both, then open the "
                         + "\u{2026} menu in the player and choose Play online. One phone hosts "
                         + "and shows its address; the other types it or picks it under Nearby. "
                         + "Same Wi-Fi works out of the box; over the internet the host's router "
                         + "must forward TCP port \(netplayDefaultPort()).")
        }
    }
}
