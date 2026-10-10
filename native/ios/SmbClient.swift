import Foundation

struct SmbFailure: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

enum SmbClient {
    struct Entry {
        var name: String
        var isDirectory: Bool
        var size: UInt64
    }

    private static let cap = 2048

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
        if rc != 0 {
            throw SmbFailure(message: message(err))
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
            throw SmbFailure(message: message(err))
        }
        return (0..<max(0, min(Int(count), cap))).map { index in
            let entry = raw[index]
            let name = withUnsafePointer(to: entry.name) {
                $0.withMemoryRebound(to: CChar.self, capacity: 256) { String(cString: $0) }
            }
            return Entry(name: name, isDirectory: entry.is_dir != 0, size: entry.size)
        }
    }

    private static func message(_ err: [CChar]) -> String {
        let text = err.withUnsafeBufferPointer { String(cString: $0.baseAddress!) }
        return text.isEmpty ? "SMB failed" : text
    }
}
