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

    /// Written on the link queue, read on main by the controller through `snapshot`.
    private let lock = NSLock()
    private var _listeningPort: UInt16 = 0
    private var _transportNote = ""
    private var _nearby: [NetplayNearbyHost] = []
    private var _browseNote = ""

    init(engine: ContinuumEngine) {
        self.engine = engine
    }

    struct Snapshot {
        let listeningPort: UInt16
        let transportNote: String
        let nearby: [NetplayNearbyHost]
        let browseNote: String
    }

    func snapshot() -> Snapshot {
        lock.lock(); defer { lock.unlock() }
        return Snapshot(listeningPort: _listeningPort, transportNote: _transportNote,
                        nearby: _nearby, browseNote: _browseNote)
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
            startListener(serviceName: serviceName, fixedPort: true)
        }
    }

    /// Tries the fixed port first, so the address can be typed from memory. A port already in use
    /// does not throw here: Network.framework reports it later as `.failed`, which is where the
    /// retry on any free port happens.
    private func startListener(serviceName: String, fixedPort: Bool) {
        stopListening()
        let parameters = Self.parameters()
        let made: NWListener?
        if fixedPort, let port = NWEndpoint.Port(rawValue: netplayDefaultPort()) {
            made = try? NWListener(using: parameters, on: port)
        } else {
            made = try? NWListener(using: parameters)
        }
        guard let listener = made else {
            if fixedPort {
                startListener(serviceName: serviceName, fixedPort: false)
                return
            }
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
                self.set {
                    self._listeningPort = listener.port?.rawValue ?? 0
                    self._transportNote = ""
                }
            case .failed(let error):
                if fixedPort {
                    self.set { self._transportNote = "port \(netplayDefaultPort()) is busy, using another" }
                    self.startListener(serviceName: serviceName, fixedPort: false)
                } else {
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
            if self.connection != nil {
                // One guest only. A third phone is turned away rather than replacing player 2.
                incoming.cancel()
                return
            }
            // Stop advertising once player 2 is here.
            self.stopListening()
            self.adopt(incoming)
        }
        self.listener = listener
        listener.start(queue: queue)
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

    /// Sends the goodbye, then closes everything.
    func close(sendingLast bytes: Data) {
        queue.async { [self] in
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
        kind = status.kind
        isHost = status.role == .host
        var text = status.line
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
        } else if status.kind == .waiting, status.role == .host {
            addressLine = "Opening a port..."
        } else {
            addressLine = ""
        }
        let ping = status.pingMs >= 0 ? String(format: "%.0f ms", status.pingMs) : "--"
        detailLine = "frame \(status.frame), delay \(status.inputDelay), ping \(ping), "
            + "\(status.checksCompared) desync checks passed"
            + (status.desyncFrame.map { ", DESYNC at frame \($0)" } ?? "")
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
            try engine.netplayHost(contentPath: entry.path, inputDelay: UInt32(max(0, inputDelay)),
                                   checksumInterval: 60)
            host?.emulation.releaseHeldControls()
            // Pause is hidden while online, so a paused game is resumed rather than stranded.
            if host?.paused == true { host?.togglePause() }
            link.listen(serviceName: "Continuum \(UIDevice.current.name): \(entry.name)")
            host?.status = "online: hosting \(entry.name), waiting for player 2"
        } catch {
            line = "online: could not host: \(error)"
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
