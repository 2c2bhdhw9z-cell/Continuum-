// Wi-Fi transfer: a switch starts a small web server on the local network, and any browser on the
// same Wi-Fi opens the shown address to upload games, disc sets, skins, cheats and saves straight
// into the normal import path, see the library, and download battery saves.
//
// The socket is NWListener (Network framework). The protocol is in Rust (import/http.rs): the
// request head is parsed there, and the page itself is compiled into the engine, so Android serves
// the same page. Bodies never go through Rust or memory: an upload is `PUT /<code>/upload?name=<file>`
// and its bytes are streamed to a scratch file, then handed to EngineHost.importFiles exactly like
// a pick from the Files app.
//
// It stops when switched off and whenever the app leaves the foreground: iOS suspends a
// background app's sockets anyway, and a server left listening with nobody looking at the
// phone is not something to leave on by accident.
//
// Access: each time it is switched on, a fresh random code (alphabet and length from Rust) goes
// into the shown address as a path segment, `http://192.168.1.20:8080/k7m2qx/`. Every request
// must start with `/<code>/` (checked in Rust, wifiStripAccessCode, in constant time); anything
// else gets a bare 404, so nobody else on the Wi-Fi can upload files or download saves.

import Combine
import Foundation
import Network
import UIKit

@MainActor
final class WiFiTransferServer: ObservableObject {
    @Published private(set) var isOn = false
    /// `http://192.168.1.20:8080/k7m2qx/` (the access code last, then a slash), or empty while off.
    @Published private(set) var address = ""
    @Published private(set) var line = "Wi-Fi transfer is off"

    /// This session's access code, or empty while off.
    private var accessCode = ""
    private var listener: NWListener?
    private let core = WiFiServerCore()
    private weak var host: EngineHost?
    private var observers: [NSObjectProtocol] = []
    private var librarySink: AnyCancellable?

    func attach(host: EngineHost) {
        self.host = host
        core.importer = { [weak self] url, reply in
            Task { @MainActor in
                guard let self, let host = self.host else {
                    reply(500, "the app is not ready")
                    return
                }
                host.importFiles([url])
                try? FileManager.default.removeItem(at: url.deletingLastPathComponent())
                self.line = "received \(url.lastPathComponent): \(host.status)"
                reply(201, host.status)
            }
        }
        librarySink = host.$library.sink { [weak self] entries in
            self?.core.setLibrary(entries.map { ($0.name, max(0, $0.byteCount)) })
        }
        let centre = NotificationCenter.default
        observers.append(centre.addObserver(forName: UIApplication.didEnterBackgroundNotification,
                                            object: nil, queue: .main) { [weak self] _ in
            Task { @MainActor in
                guard let self, self.isOn else { return }
                self.stop(reason: "Wi-Fi transfer stopped: the app left the foreground")
            }
        })
    }

    func setOn(_ on: Bool) {
        if on { start() } else { stop(reason: "Wi-Fi transfer is off") }
    }

    /// True while trying the easy-to-type port 8080; a failure there retries on any free port.
    private var triedFixedPort = false

    private func start(anyPort: Bool = false) {
        guard listener == nil else { return }
        // A fresh code every time it is switched on, so an address seen before stops working.
        let code = Self.newAccessCode()
        guard wifiIsAccessCode(code: code) else {
            line = "Wi-Fi transfer could not start: no access code could be made"
            host?.status = line
            return
        }
        let parameters = NWParameters.tcp
        parameters.allowLocalEndpointReuse = true
        let made: NWListener
        do {
            if !anyPort, let port = NWEndpoint.Port(rawValue: 8080),
               let fixed = try? NWListener(using: parameters, on: port) {
                made = fixed
                triedFixedPort = true
            } else {
                made = try NWListener(using: parameters)
                triedFixedPort = false
            }
        } catch {
            line = "Wi-Fi transfer could not start: \(error.localizedDescription)"
            host?.status = line
            return
        }
        listener = made
        accessCode = code
        // `self.` on purpose: a `let core` further down this function shadows the property, and
        // Swift refuses a bare `core` used before that local's declaration.
        self.core.setAccessCode(code)
        isOn = true
        line = "starting..."
        let core = self.core
        made.newConnectionHandler = { connection in
            WiFiConnection(connection: connection, core: core).start()
        }
        made.stateUpdateHandler = { [weak self] state in
            Task { @MainActor in self?.listenerChanged(state) }
        }
        made.start(queue: core.queue)
    }

    private func listenerChanged(_ state: NWListener.State) {
        switch state {
        case .ready:
            let port = listener?.port?.rawValue ?? 0
            if let ip = Self.wifiAddress() {
                address = wifiTransferAddress(ip: ip, port: port, code: accessCode)
                line = "open \(address) in a browser on the same Wi-Fi"
            } else {
                address = ""
                line = "listening on port \(port), but this phone has no Wi-Fi address. Join a Wi-Fi network"
            }
            host?.status = line
        case .failed(let error):
            if triedFixedPort {
                // Port 8080 is taken (another app, or a listener not yet released): any port will do.
                listener?.cancel()
                listener = nil
                start(anyPort: true)
                return
            }
            stop(reason: "Wi-Fi transfer failed: \(error.localizedDescription). If iOS asked about the "
                 + "local network, allow it in Settings, Privacy, Local Network")
        case .waiting(let error):
            line = "Wi-Fi transfer is waiting: \(error.localizedDescription)"
        default:
            break
        }
    }

    func stop(reason: String) {
        listener?.cancel()
        listener = nil
        // An empty code matches nothing, so a connection still open after this gets 404s.
        accessCode = ""
        core.setAccessCode("")
        isOn = false
        address = ""
        line = reason
        host?.status = reason
    }

    /// A new access code: `wifiAccessCodeLength()` characters from the engine's alphabet, each
    /// drawn with SystemRandomNumberGenerator (the system's cryptographically secure source).
    private static func newAccessCode() -> String {
        let alphabet = Array(wifiAccessCodeAlphabet())
        var generator = SystemRandomNumberGenerator()
        var code = ""
        for _ in 0..<Int(wifiAccessCodeLength()) {
            if let character = alphabet.randomElement(using: &generator) {
                code.append(character)
            }
        }
        return code
    }

    /// The phone's IPv4 address on Wi-Fi (en0), or any non-loopback IPv4 as a fallback.
    static func wifiAddress() -> String? {
        var list: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&list) == 0, let first = list else { return nil }
        defer { freeifaddrs(list) }
        var fallback: String?
        var cursor: UnsafeMutablePointer<ifaddrs>? = first
        while let item = cursor {
            defer { cursor = item.pointee.ifa_next }
            guard let sa = item.pointee.ifa_addr, sa.pointee.sa_family == UInt8(AF_INET) else { continue }
            let name = String(cString: item.pointee.ifa_name)
            var buffer = [CChar](repeating: 0, count: Int(NI_MAXHOST))
            guard getnameinfo(sa, socklen_t(sa.pointee.sa_len), &buffer, socklen_t(buffer.count),
                              nil, 0, NI_NUMERICHOST) == 0 else { continue }
            let ip = String(cString: buffer)
            if name == "en0" { return ip }
            if !name.hasPrefix("lo") && !name.hasPrefix("pdp_ip") && fallback == nil { fallback = ip }
        }
        return fallback
    }
}

/// What the connections share, used on the server's own queue.
final class WiFiServerCore: @unchecked Sendable {
    let queue = DispatchQueue(label: "dev.continuum.wifi-transfer")
    /// Imports one received file on the main actor, then replies (status, text).
    var importer: (@Sendable (URL, @escaping @Sendable (Int, String) -> Void) -> Void)?
    private let lock = NSLock()
    private var library: [(String, Int64)] = []
    private var code = ""

    func setLibrary(_ entries: [(String, Int64)]) {
        lock.lock(); library = entries; lock.unlock()
    }

    /// The access code every request has to start with; empty (nothing matches) while off.
    func setAccessCode(_ newCode: String) {
        lock.lock(); code = newCode; lock.unlock()
    }

    func accessCode() -> String {
        lock.lock()
        let current = code
        lock.unlock()
        return current
    }

    func libraryJSON() -> Data {
        lock.lock()
        let games = library
        lock.unlock()
        let gameItems = games.map { "{\"name\":\"\(wifiJsonEscape(text: $0.0))\",\"size\":\($0.1)}" }
        let saveItems = batterySaves().map { "{\"name\":\"\(wifiJsonEscape(text: $0.0))\",\"size\":\($0.1)}" }
        let json = "{\"games\":[\(gameItems.joined(separator: ","))],\"saves\":[\(saveItems.joined(separator: ","))]}"
        return Data(json.utf8)
    }

    static func batteryDirectory() -> URL? {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first?
            .appendingPathComponent("BatterySaves", isDirectory: true)
    }

    func batterySaves() -> [(String, Int64)] {
        guard let dir = Self.batteryDirectory(),
              let files = try? FileManager.default.contentsOfDirectory(
                at: dir, includingPropertiesForKeys: [.fileSizeKey], options: [.skipsHiddenFiles])
        else { return [] }
        return files
            .filter { $0.pathExtension.lowercased() == "srm" }
            .map { ($0.lastPathComponent, Int64((try? $0.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0)) }
            .sorted { $0.0.localizedStandardCompare($1.0) == .orderedAscending }
    }
}

/// One browser connection: read the head, answer it, close.
final class WiFiConnection: @unchecked Sendable {
    private let connection: NWConnection
    private let core: WiFiServerCore
    private var buffer = Data()
    private var upload: FileHandle?
    private var uploadURL: URL?
    private var remaining: UInt64 = 0
    private var uploadName = ""

    init(connection: NWConnection, core: WiFiServerCore) {
        self.connection = connection
        self.core = core
    }

    func start() {
        connection.start(queue: core.queue)
        readHead()
    }

    private func readHead() {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 64 * 1024) { [self] data, _, done, error in
            if let data { buffer.append(data) }
            do {
                if let head = try wifiParseRequestHead(data: buffer) {
                    handle(head)
                } else if done || error != nil {
                    connection.cancel()
                } else {
                    readHead()
                }
            } catch {
                send(400, "text/plain; charset=utf-8", Data("\(error)".utf8))
            }
        }
    }

    private func handle(_ head: WifiRequestHead) {
        let method = head.method
        let code = core.accessCode()
        // Every request starts with `/<code>`. Anything else is a bare 404 with no detail, so a
        // guess is not even told there is something to guess. The rest routes as it always has.
        guard let path = wifiStripAccessCode(path: head.path, code: code) else {
            send(404, "text/plain; charset=utf-8", Data())
            return
        }
        if path.isEmpty {
            // `/<code>` without the slash: send the browser to `/<code>/`, or the page's relative
            // URLs would resolve against `/` and lose the code.
            send(302, "text/plain; charset=utf-8", Data(), extra: "Location: /\(code)/\r\n")
            return
        }
        if method == "GET" && (path == "/" || path == "/index.html") {
            send(200, "text/html; charset=utf-8", Data(wifiPageHtml().utf8))
        } else if method == "GET" && path == "/api/library" {
            send(200, "application/json", core.libraryJSON())
        } else if method == "GET" && path.hasPrefix("/saves/") {
            let raw = String(path.dropFirst("/saves/".count))
            guard let name = wifiSafeFileName(name: raw),
                  let dir = WiFiServerCore.batteryDirectory(),
                  let data = try? Data(contentsOf: dir.appendingPathComponent(name)) else {
                send(404, "text/plain; charset=utf-8", Data("no such save".utf8))
                return
            }
            send(200, "application/octet-stream", data,
                 extra: "Content-Disposition: attachment; filename=\"\(name)\"\r\n")
        } else if (method == "PUT" || method == "POST") && path == "/upload" {
            beginUpload(head)
        } else {
            send(404, "text/plain; charset=utf-8", Data("not found".utf8))
        }
    }

    private func beginUpload(_ head: WifiRequestHead) {
        guard let name = head.query["name"].flatMap({ wifiSafeFileName(name: $0) }) else {
            send(400, "text/plain; charset=utf-8", Data("the upload has no usable file name".utf8))
            return
        }
        guard head.contentLength > 0 else {
            send(400, "text/plain; charset=utf-8", Data("\(name) is empty".utf8))
            return
        }
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("ContinuumWiFi", isDirectory: true)
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        let url = dir.appendingPathComponent(name)
        do {
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            FileManager.default.createFile(atPath: url.path, contents: nil)
            upload = try FileHandle(forWritingTo: url)
        } catch {
            send(500, "text/plain; charset=utf-8", Data("could not store \(name): \(error.localizedDescription)".utf8))
            return
        }
        uploadURL = url
        uploadName = name
        remaining = head.contentLength
        let start = Int(head.headLen)
        let body = buffer.count > start ? buffer.subdata(in: start..<buffer.count) : Data()
        buffer = Data()
        write(body)
    }

    private func write(_ chunk: Data) {
        guard let handle = upload else { return }
        var piece = chunk
        if UInt64(piece.count) > remaining { piece = piece.prefix(Int(remaining)) }
        if !piece.isEmpty {
            do {
                try handle.write(contentsOf: piece)
            } catch {
                failUpload(507, "the phone could not store \(uploadName): \(error.localizedDescription)")
                return
            }
            remaining -= UInt64(piece.count)
        }
        if remaining == 0 {
            finishUpload()
            return
        }
        connection.receive(minimumIncompleteLength: 1, maximumLength: 1 << 20) { [self] data, _, done, error in
            if let data, !data.isEmpty {
                write(data)
            } else if done || error != nil {
                failUpload(400, "the upload of \(uploadName) stopped before it finished")
            } else {
                write(Data())
            }
        }
    }

    private func failUpload(_ status: Int, _ text: String) {
        try? upload?.close()
        upload = nil
        if let url = uploadURL { try? FileManager.default.removeItem(at: url.deletingLastPathComponent()) }
        send(UInt16(status), "text/plain; charset=utf-8", Data(text.utf8))
    }

    private func finishUpload() {
        try? upload?.close()
        upload = nil
        guard let url = uploadURL, let importer = core.importer else {
            failUpload(500, "the app is not ready")
            return
        }
        importer(url) { [self] status, text in
            core.queue.async { self.send(UInt16(status), "text/plain; charset=utf-8", Data(text.utf8)) }
        }
    }

    private func send(_ status: UInt16, _ type: String, _ body: Data, extra: String = "") {
        var head = wifiResponseHead(status: status, contentType: type, length: UInt64(body.count))
        if !extra.isEmpty, head.count >= 2 {
            // Insert the extra header line before the blank line that ends the head.
            head.removeLast(2)
            head.append(Data(extra.utf8))
            head.append(Data("\r\n".utf8))
        }
        connection.send(content: head + body, contentContext: .finalMessage, isComplete: true,
                        completion: .contentProcessed { [connection] _ in connection.cancel() })
    }
}
