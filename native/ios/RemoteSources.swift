// Network sources: WebDAV servers (a NAS, router storage, a computer's shared folder). Add a server
// once (address, user, password; the password goes in the Keychain), browse its folders, and pick
// files or a whole folder: they download into scratch and go through the SAME import path as the
// Files picker, so a folder holding a .cue and its .bin tracks arrives as one batch.
//
// WebDAV is URLSession with PROPFIND (Depth: 1); the XML answer is parsed in Rust
// (import/webdav.rs).
//
// SMB is not in this build. It used AMSMB2, a Swift package over libsmb2, and was taken out after
// build 117 stopped the app opening (Xcode linked AMSMB2.framework without embedding it; see the
// note in project.yml). The SMB code stays here behind `#if canImport(AMSMB2)` for when it
// returns; without the package the rest of the app builds and the SMB row says it is unavailable.

import Foundation
import Security
import SwiftUI
#if canImport(AMSMB2)
import AMSMB2
#endif

// MARK: - Model

struct RemoteServer: Codable, Identifiable, Hashable {
    enum Kind: String, Codable { case webdav, smb }
    var id = UUID()
    var kind: Kind
    var name: String
    /// `https://nas.local:5006/dav` for WebDAV, `smb://nas.local` for SMB.
    var url: String
    var user: String
}

struct RemoteItem: Identifiable, Hashable {
    var id: String { path }
    let name: String
    /// Absolute on the server. For SMB, `/<share>/<path in share>`.
    let path: String
    let isFolder: Bool
    let size: UInt64
}

/// Server passwords, in the Keychain under one service, keyed by server id.
enum RemoteKeychain {
    private static let service = "dev.continuum.remote-sources"

    static func set(_ password: String, for id: UUID) {
        let base: [String: Any] = [kSecClass as String: kSecClassGenericPassword,
                                   kSecAttrService as String: service,
                                   kSecAttrAccount as String: id.uuidString]
        SecItemDelete(base as CFDictionary)
        var add = base
        add[kSecValueData as String] = Data(password.utf8)
        add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock
        SecItemAdd(add as CFDictionary, nil)
    }

    static func get(_ id: UUID) -> String? {
        let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword,
                                    kSecAttrService as String: service,
                                    kSecAttrAccount as String: id.uuidString,
                                    kSecReturnData as String: true,
                                    kSecMatchLimit as String: kSecMatchLimitOne]
        var out: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &out) == errSecSuccess,
              let data = out as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }

    static func remove(_ id: UUID) {
        let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword,
                                    kSecAttrService as String: service,
                                    kSecAttrAccount as String: id.uuidString]
        SecItemDelete(query as CFDictionary)
    }
}

struct RemoteError: LocalizedError {
    let text: String
    var errorDescription: String? { text }
}

@MainActor
final class RemoteSources: ObservableObject {
    @Published private(set) var servers: [RemoteServer] = []
    @Published var line = "no network servers added yet"
    @Published private(set) var busy = false

    private weak var host: EngineHost?
    private static let key = "remote.servers.v1"

    static var smbAvailable: Bool {
        #if canImport(AMSMB2)
        return true
        #else
        return false
        #endif
    }

    func attach(host: EngineHost) {
        self.host = host
        if let data = UserDefaults.standard.data(forKey: Self.key),
           let list = try? JSONDecoder().decode([RemoteServer].self, from: data) {
            servers = list
        }
        if !servers.isEmpty { line = "\(servers.count) network server(s)" }
    }

    private func save() {
        if let data = try? JSONEncoder().encode(servers) {
            UserDefaults.standard.set(data, forKey: Self.key)
        }
    }

    /// Adds a server. Returns nil on success or the reason it was refused.
    func add(kind: RemoteServer.Kind, name: String, url: String, user: String, password: String) -> String? {
        var address = url.trimmingCharacters(in: .whitespacesAndNewlines)
        if kind == .smb && !address.lowercased().hasPrefix("smb://") { address = "smb://" + address }
        if kind == .webdav && !address.lowercased().hasPrefix("http") { address = "https://" + address }
        guard let parsed = URL(string: address), parsed.host != nil else {
            return "\(address) is not an address (want \(kind == .smb ? "smb://nas.local" : "https://nas.local:5006/dav"))"
        }
        if kind == .smb && !Self.smbAvailable {
            return "SMB is not in this build yet; it was taken out after it stopped the app opening. WebDAV works"
        }
        let server = RemoteServer(kind: kind, name: name.isEmpty ? (parsed.host ?? address) : name,
                                  url: address, user: user)
        RemoteKeychain.set(password, for: server.id)
        servers.append(server)
        save()
        line = "added \(server.name)"
        return nil
    }

    func remove(_ server: RemoteServer) {
        RemoteKeychain.remove(server.id)
        servers.removeAll { $0.id == server.id }
        save()
        line = "removed \(server.name)"
    }

    /// The folder to start in.
    func rootPath(_ server: RemoteServer) -> String {
        switch server.kind {
        case .webdav:
            let path = URL(string: server.url)?.path ?? "/"
            return path.isEmpty ? "/" : (path.hasSuffix("/") ? path : path + "/")
        case .smb:
            return "/"
        }
    }

    // MARK: Listing

    func list(_ server: RemoteServer, path: String) async throws -> [RemoteItem] {
        switch server.kind {
        case .webdav: return try await davList(server, path: path)
        case .smb: return try await smbList(server, path: path)
        }
    }

    private func davURL(_ server: RemoteServer, path: String) throws -> URL {
        guard var parts = URLComponents(string: server.url) else {
            throw RemoteError(text: "\(server.url) is not an address")
        }
        parts.path = path
        guard let url = parts.url else { throw RemoteError(text: "\(path) is not a usable path") }
        return url
    }

    private func authorise(_ request: inout URLRequest, _ server: RemoteServer) {
        guard !server.user.isEmpty else { return }
        let pair = "\(server.user):\(RemoteKeychain.get(server.id) ?? "")"
        request.setValue("Basic \(Data(pair.utf8).base64EncodedString())", forHTTPHeaderField: "Authorization")
    }

    private func davList(_ server: RemoteServer, path: String) async throws -> [RemoteItem] {
        var request = URLRequest(url: try davURL(server, path: path))
        request.httpMethod = "PROPFIND"
        request.setValue("1", forHTTPHeaderField: "Depth")
        request.setValue("application/xml; charset=utf-8", forHTTPHeaderField: "Content-Type")
        request.httpBody = Data(webdavPropfindBody().utf8)
        authorise(&request, server)
        let (data, response) = try await URLSession.shared.data(for: request)
        let code = (response as? HTTPURLResponse)?.statusCode ?? 0
        if code == 401 || code == 403 {
            throw RemoteError(text: "\(server.name) refused the user name or password (HTTP \(code))")
        }
        guard code == 207 || code == 200 else {
            throw RemoteError(text: "\(server.name) answered HTTP \(code) to the folder listing")
        }
        let xml = String(decoding: data, as: UTF8.self)
        return webdavParseListing(xml: xml, requestPath: path).map {
            RemoteItem(name: $0.name, path: $0.path, isFolder: $0.isFolder, size: $0.size)
        }
    }

    // MARK: SMB

    #if canImport(AMSMB2)
    private var smbClients: [UUID: (share: String, client: SMB2Manager)] = [:]

    private func smbClient(_ server: RemoteServer, share: String) async throws -> SMB2Manager {
        if let cached = smbClients[server.id], cached.share == share { return cached.client }
        guard let url = URL(string: server.url) else { throw RemoteError(text: "\(server.url) is not an address") }
        let credential = URLCredential(user: server.user.isEmpty ? "guest" : server.user,
                                       password: RemoteKeychain.get(server.id) ?? "",
                                       persistence: .forSession)
        guard let client = SMB2Manager(url: url, credential: credential) else {
            throw RemoteError(text: "\(server.url) is not an SMB address")
        }
        if !share.isEmpty {
            try await client.connectShare(name: share)
            smbClients[server.id] = (share, client)
        }
        return client
    }

    private static func split(_ path: String) -> (share: String, rest: String) {
        let parts = path.split(separator: "/", omittingEmptySubsequences: true)
        guard let first = parts.first else { return ("", "/") }
        return (String(first), "/" + parts.dropFirst().joined(separator: "/"))
    }
    #endif

    private func smbList(_ server: RemoteServer, path: String) async throws -> [RemoteItem] {
        #if canImport(AMSMB2)
        let (share, rest) = Self.split(path)
        if share.isEmpty {
            let client = try await smbClient(server, share: "")
            let shares = try await client.listShares()
            return shares.map { RemoteItem(name: $0.name, path: "/\($0.name)/", isFolder: true, size: 0) }
        }
        let client = try await smbClient(server, share: share)
        let entries = try await client.contentsOfDirectory(atPath: rest)
        let base = path.hasSuffix("/") ? path : path + "/"
        return entries.compactMap { entry -> RemoteItem? in
            guard let name = entry[.nameKey] as? String, name != ".", name != "..", !name.hasPrefix(".") else {
                return nil
            }
            let folder = (entry[.isDirectoryKey] as? Bool) ?? false
            let size = (entry[.fileSizeKey] as? NSNumber)?.uint64Value ?? 0
            return RemoteItem(name: name, path: base + name + (folder ? "/" : ""), isFolder: folder, size: size)
        }
        .sorted { ($0.isFolder ? 0 : 1, $0.name.lowercased()) < ($1.isFolder ? 0 : 1, $1.name.lowercased()) }
        #else
        throw RemoteError(text: "SMB is not in this build yet; it was taken out after it stopped the app opening. WebDAV works")
        #endif
    }

    // MARK: Download and import

    /// Downloads the files (folders are skipped) into scratch and imports them as one batch.
    func importItems(_ items: [RemoteItem], from server: RemoteServer) async {
        let files = items.filter { !$0.isFolder }
        guard !files.isEmpty else {
            line = "nothing to import: pick files, or open a folder and import it"
            return
        }
        busy = true
        defer { busy = false }
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("ContinuumRemote", isDirectory: true)
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        var urls: [URL] = []
        var failures: [String] = []
        for (index, item) in files.enumerated() {
            line = "downloading \(index + 1) of \(files.count): \(item.name)"
            let target = dir.appendingPathComponent(wifiSafeFileName(name: item.name) ?? "file-\(index)")
            do {
                switch server.kind {
                case .webdav: try await davDownload(server, item, to: target)
                case .smb: try await smbDownload(server, item, to: target)
                }
                urls.append(target)
            } catch {
                failures.append("\(item.name): \(error.localizedDescription)")
            }
        }
        if !urls.isEmpty { host?.importFiles(urls) }
        let summary = host?.status ?? ""
        line = failures.isEmpty
            ? "from \(server.name): \(summary)"
            : "from \(server.name): \(summary) | failed: \(failures.joined(separator: ", "))"
        host?.status = line
    }

    private func davDownload(_ server: RemoteServer, _ item: RemoteItem, to target: URL) async throws {
        var request = URLRequest(url: try davURL(server, path: item.path))
        authorise(&request, server)
        let (temp, response) = try await URLSession.shared.download(for: request)
        let code = (response as? HTTPURLResponse)?.statusCode ?? 0
        guard (200..<300).contains(code) else {
            try? FileManager.default.removeItem(at: temp)
            throw RemoteError(text: "HTTP \(code)")
        }
        try? FileManager.default.removeItem(at: target)
        try FileManager.default.moveItem(at: temp, to: target)
    }

    private func smbDownload(_ server: RemoteServer, _ item: RemoteItem, to target: URL) async throws {
        #if canImport(AMSMB2)
        let (share, rest) = Self.split(item.path)
        let client = try await smbClient(server, share: share)
        try? FileManager.default.removeItem(at: target)
        try await client.downloadItem(atPath: rest, to: target, progress: nil)
        #else
        throw RemoteError(text: "SMB is not in this build")
        #endif
    }
}

// MARK: - Screens

/// Add a server: kind, address, user, password.
struct AddRemoteServerView: View {
    @ObservedObject var sources: RemoteSources
    let kind: RemoteServer.Kind
    let onDone: () -> Void
    @State private var name = ""
    @State private var url = ""
    @State private var user = ""
    @State private var password = ""
    @State private var problem = ""

    var body: some View {
        NavigationView {
            Form {
                Section {
                    TextField("Name (optional)", text: $name)
                    TextField(kind == .smb ? "smb://nas.local" : "https://nas.local:5006/dav", text: $url)
                        .keyboardType(.URL)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled(true)
                    TextField("User name", text: $user)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled(true)
                    SecureField("Password", text: $password)
                } footer: {
                    Text(problem.isEmpty
                         ? "The password is kept in this phone's Keychain."
                         : problem)
                }
            }
            .navigationTitle(kind == .smb ? "Add an SMB share" : "Add a WebDAV server")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel", action: onDone) }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Add") {
                        if let reason = sources.add(kind: kind, name: name, url: url, user: user, password: password) {
                            problem = reason
                        } else {
                            onDone()
                        }
                    }
                }
            }
        }
        .preferredColorScheme(.dark)
    }
}

/// One folder on a server. Folders push another one of these; files are picked with a tap.
struct RemoteFolderView: View {
    @ObservedObject var sources: RemoteSources
    let server: RemoteServer
    let path: String
    @State private var items: [RemoteItem] = []
    @State private var picked: Set<String> = []
    @State private var state = "loading..."

    var body: some View {
        List {
            if !state.isEmpty {
                Text(state).font(.footnote).foregroundStyle(.secondary)
            }
            ForEach(items) { item in
                if item.isFolder {
                    NavigationLink {
                        RemoteFolderView(sources: sources, server: server, path: item.path)
                    } label: {
                        Label(item.name, systemImage: "folder")
                    }
                } else {
                    Button {
                        if picked.contains(item.id) { picked.remove(item.id) } else { picked.insert(item.id) }
                    } label: {
                        HStack {
                            Image(systemName: picked.contains(item.id) ? "checkmark.circle.fill" : "circle")
                            Text(item.name)
                            Spacer()
                            Text(SaveStates.byteText(Int64(item.size))).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                }
            }
        }
        .navigationTitle(path == "/" ? server.name : (path as NSString).lastPathComponent)
        .toolbar {
            ToolbarItem(placement: .confirmationAction) {
                Menu("Import") {
                    Button("Import \(picked.count) picked") {
                        let chosen = items.filter { picked.contains($0.id) }
                        Task { await sources.importItems(chosen, from: server); picked.removeAll() }
                    }
                    .disabled(picked.isEmpty)
                    Button("Import every file in this folder") {
                        let all = items
                        Task { await sources.importItems(all, from: server) }
                    }
                }
                .disabled(sources.busy)
            }
        }
        .safeAreaInset(edge: .bottom) {
            if sources.busy {
                Text(sources.line).font(.footnote).padding(8).frame(maxWidth: .infinity)
                    .background(.ultraThinMaterial)
            }
        }
        .task(id: path) {
            do {
                items = try await sources.list(server, path: path)
                state = items.isEmpty ? "this folder is empty" : ""
            } catch {
                state = "could not list \(path): \(error.localizedDescription)"
                sources.line = state
            }
        }
    }
}
