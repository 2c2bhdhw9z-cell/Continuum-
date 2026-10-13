import Foundation

struct SmbFailure: LocalizedError {
    /// One short sentence, for the screen.
    let message: String
    /// libsmb2's own words, for the activity log only. Never shown.
    let detail: String
    var errorDescription: String? { message }
}

enum SmbClient {
    struct Entry {
        var name: String
        var isDirectory: Bool
        var size: UInt64
    }

    private static let cap = 2048

    /// The blocking calls get this queue, never Swift's shared pool. See `offMainThread`.
    private static let queue = DispatchQueue(label: "dev.continuum.smb", qos: .userInitiated)

    static func listShares(host: String, user: String, password: String) throws -> [Entry] {
        try run { err, count, buf in
            smb_list_shares(host, user, password, buf, Int32(cap), count, err, 512)
        }
    }

    static func list(host: String, share: String, path: String, user: String,
                     password: String) throws -> [Entry] {
        try run { err, count, buf in
            smb_list_dir(host, share, path, user, password, buf, Int32(cap), count, err, 512)
        }
    }

    static func download(host: String, share: String, path: String, to local: String,
                         user: String, password: String) throws {
        var err = [CChar](repeating: 0, count: 512)
        let rc = err.withUnsafeMutableBufferPointer { buf in
            smb_download(host, share, path, local, user, password, buf.baseAddress, 512)
        }
        // Same rule as `run`: never trust the C side to have terminated the message.
        err[err.count - 1] = 0
        if rc != 0 {
            throw failure(err)
        }
    }

    /// Runs one call off the main thread, on a queue of its own.
    ///
    /// libsmb2 blocks until the server answers, and its own timeout is 20 seconds. These calls
    /// used to run in `Task.detached`, on the pool Swift shares across the app: that pool has
    /// about one thread per core, so a NAS that had gone to sleep could sit on a thread the rest
    /// of the app wanted.
    ///
    /// `what` names the call for the activity log: when it fails, libsmb2's own words go there
    /// and only the short sentence reaches the screen.
    static func offMainThread<T>(_ what: String,
                                 _ work: @escaping @Sendable () throws -> T) async throws -> T {
        try await withCheckedThrowingContinuation { continuation in
            queue.async {
                do {
                    let value = try work()
                    continuation.resume(returning: value)
                } catch {
                    if let refusal = error as? SmbFailure {
                        FeedbackCenter.step("smb: \(what): \(refusal.detail)")
                    }
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    private static func run(
        _ body: (UnsafeMutablePointer<CChar>, UnsafeMutablePointer<Int32>,
                 UnsafeMutablePointer<SmbEntry>) -> Int32
    ) throws -> [Entry] {
        let raw = UnsafeMutablePointer<SmbEntry>.allocate(capacity: cap)
        defer { raw.deallocate() }
        var count: Int32 = 0
        var err = [CChar](repeating: 0, count: 512)
        let rc = err.withUnsafeMutableBufferPointer { buf -> Int32 in
            body(buf.baseAddress!, &count, raw)
        }
        // Never trust the C side's lengths further than the buffers this side allocated.
        err[err.count - 1] = 0
        if rc != 0 {
            throw failure(err)
        }
        return (0..<max(0, min(Int(count), cap))).map { index in
            let entry = raw[index]
            let name = withUnsafePointer(to: entry.name) {
                $0.withMemoryRebound(to: CChar.self, capacity: 256) { String(cString: $0) }
            }
            return Entry(name: name, isDirectory: entry.is_dir != 0, size: entry.size)
        }
    }

    private static func failure(_ err: [CChar]) -> SmbFailure {
        let raw = err.withUnsafeBufferPointer { String(cString: $0.baseAddress!) }
        let detail = raw.isEmpty ? "SMB failed and said nothing" : raw
        return SmbFailure(message: plainLine(detail), detail: detail)
    }

    /// One short sentence for what a NAS really hands back.
    ///
    /// libsmb2 answers in protocol words ("Session setup failed with (0xc000006d)
    /// STATUS_LOGON_FAILURE"), and that used to go straight on screen. The raw line is kept, but
    /// only for the activity log: see `offMainThread`.
    static func plainLine(_ raw: String) -> String {
        let text = raw.lowercased()
        func says(_ parts: String...) -> Bool { parts.contains { text.contains($0) } }

        if says("logon_failure", "access_denied", "wrong_password", "no_such_user",
                "account_disabled", "account_locked_out", "account_restriction",
                "password_expired", "password_must_change", "logon_type_not_granted") {
            return "Wrong user name or password for that server."
        }
        if says("bad_network_name", "bad_network_path", "network_name_deleted") {
            return "That share isn't on the server any more."
        }
        // A file for a download, a folder for a listing: one line covers both.
        if says("object_name_not_found", "object_path_not_found", "no_such_file",
                "delete_pending", "file_closed") {
            return "That's not on the server any more."
        }
        if says("sharing_violation", "file_lock_conflict", "lock_not_granted") {
            return "Something else on the server is using that file."
        }
        if says("can not resolve", "invalid address") {
            return "Can't find that server. Check the address."
        }
        if says("socket connect failed", "not connected", "read from socket failed",
                "error when writing to") {
            return "Nothing answered at that address. Check the server is on."
        }
        if says("timeout", "timed out") {
            return "The server stopped answering. Try again."
        }
        if says("could not write the download") {
            return "This phone wouldn't save the download. Check there's room on it."
        }
        return "That server wouldn't do it. Check it's on, and the user name and password."
    }
}
