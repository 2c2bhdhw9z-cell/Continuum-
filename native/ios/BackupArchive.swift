// Continuum - the backup as ONE .zip file.
//
// WHY A ZIP (build 167): "Restore from a backup" crashed on the phone (build 166). It picked the
// backup FOLDER with UIDocumentPickerViewController(forOpeningContentTypes: [.folder],
// asCopy: true). Copy mode is not supported for folders (Apple only documents it for files), and
// on top of that the whole copied tree was then copied again and walked ON THE MAIN THREAD, which
// with games in the backup is easily long enough for iOS's watchdog to kill the app. A single
// file in copy mode is the one picker path that works on this sideloaded install (every import
// uses it), so Back up to Files now exports `Continuum Backup <date>.zip`, and Restore picks a
// .zip. A folder backup from builds 164-166 still works: long-press it in Files, Compress, and
// pick the .zip. All packing and unpacking runs off the main thread; every step goes to the
// durable log (FeedbackCenter.step) and every failure is a thrown error shown on screen.
//
// The writer stores files uncompressed (saves are small, games are already compressed, and it
// keeps a multi-gigabyte backup fast). The reader takes stored and deflated entries, ZIP64
// included, so a zip made by Files' own Compress restores too.

import Compression
import Foundation

struct BackupArchiveError: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}

enum BackupArchive {
    private static let chunk = 1 << 20

    // ------------------------------------------------------------------ CRC-32

    private static let crcTable: [UInt32] = (0..<256).map { n -> UInt32 in
        var c = UInt32(n)
        for _ in 0..<8 { c = (c & 1) != 0 ? 0xEDB8_8320 ^ (c >> 1) : c >> 1 }
        return c
    }

    private static func crc(_ data: Data, _ start: UInt32) -> UInt32 {
        var c = ~start
        crcTable.withUnsafeBufferPointer { table in
            data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
                for b in raw { c = table[Int((c ^ UInt32(b)) & 0xFF)] ^ (c >> 8) }
            }
        }
        return ~c
    }

    // ------------------------------------------------------------------ bytes

    private static func le16(_ v: UInt16) -> [UInt8] { [UInt8(v & 0xFF), UInt8(v >> 8)] }
    private static func le32(_ v: UInt32) -> [UInt8] { (0..<4).map { UInt8((v >> (8 * $0)) & 0xFF) } }
    private static func le64(_ v: UInt64) -> [UInt8] { (0..<8).map { UInt8((v >> (8 * UInt64($0))) & 0xFF) } }

    private static func u16(_ d: Data, _ at: Int) -> UInt16 {
        let b = d.startIndex + at
        return UInt16(d[b]) | UInt16(d[b + 1]) << 8
    }
    private static func u32(_ d: Data, _ at: Int) -> UInt32 {
        UInt32(u16(d, at)) | UInt32(u16(d, at + 2)) << 16
    }
    private static func u64(_ d: Data, _ at: Int) -> UInt64 {
        UInt64(u32(d, at)) | UInt64(u32(d, at + 4)) << 32
    }

    // ------------------------------------------------------------------ write

    /// Packs `folder` into a new zip at `zip`; entries are named `<folder name>/<relative path>`.
    /// Returns the number of files written.
    @discardableResult
    static func pack(folder: URL, into zip: URL, log: @escaping (String) -> Void) throws -> Int {
        let fm = FileManager.default
        try? fm.removeItem(at: zip)
        guard fm.createFile(atPath: zip.path, contents: nil) else {
            throw BackupArchiveError("could not create \(zip.lastPathComponent)")
        }
        let out = try FileHandle(forWritingTo: zip)
        defer { try? out.close() }
        let prefix = folder.lastPathComponent
        let basePath = folder.standardizedFileURL.resolvingSymlinksInPath().path
        guard let walker = fm.enumerator(at: folder, includingPropertiesForKeys: [.isRegularFileKey],
                                         options: [], errorHandler: { url, error in
            log("zip: skipped unreadable \(url.lastPathComponent): \(error.localizedDescription)")
            return true
        }) else { throw BackupArchiveError("could not read the backup folder") }

        var central = Data()
        var count = 0
        var offset: UInt64 = 0
        for case let file as URL in walker {
            guard (try? file.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile) == true else { continue }
            let full = file.standardizedFileURL.resolvingSymlinksInPath().path
            guard full.hasPrefix(basePath + "/") else { continue }
            let relative = String(full.dropFirst(basePath.count + 1))
            let name = Array((prefix + "/" + relative).utf8)
            let input = try FileHandle(forReadingFrom: file)
            defer { try? input.close() }
            let size = (try? fm.attributesOfItem(atPath: file.path)[.size] as? NSNumber)?.uint64Value ?? 0
            let big = size >= 0xFFFF_FFFF || offset >= 0xFFFF_FFFF
            let headerOffset = offset

            // Local header. CRC is not known yet: bit 3 (data descriptor) follows the data.
            var local: [UInt8] = le32(0x0403_4B50) + le16(big ? 45 : 20) + le16(0x0808) + le16(0)
                + le16(0) + le16(0x21) + le32(0)
            local += big ? le32(0xFFFF_FFFF) + le32(0xFFFF_FFFF) : le32(0) + le32(0)
            let localExtra: [UInt8] = big ? le16(1) + le16(16) + le64(0) + le64(0) : []
            local += le16(UInt16(name.count)) + le16(UInt16(localExtra.count)) + name + localExtra
            try out.write(contentsOf: Data(local))
            offset += UInt64(local.count)

            var crcValue: UInt32 = 0
            var written: UInt64 = 0
            while true {
                let piece = try autoreleasepool { try input.read(upToCount: chunk) ?? Data() }
                if piece.isEmpty { break }
                crcValue = crc(piece, crcValue)
                try out.write(contentsOf: piece)
                written += UInt64(piece.count)
            }
            offset += written
            let descriptor: [UInt8] = big
                ? le32(0x0807_4B50) + le32(crcValue) + le64(written) + le64(written)
                : le32(0x0807_4B50) + le32(crcValue) + le32(UInt32(written)) + le32(UInt32(written))
            try out.write(contentsOf: Data(descriptor))
            offset += UInt64(descriptor.count)

            let needs64 = written >= 0xFFFF_FFFF || headerOffset >= 0xFFFF_FFFF
            var extra: [UInt8] = []
            if needs64 { extra = le16(1) + le16(24) + le64(written) + le64(written) + le64(headerOffset) }
            var entry: [UInt8] = le32(0x0201_4B50) + le16(0x031E) + le16(needs64 ? 45 : 20)
                + le16(0x0808) + le16(0) + le16(0) + le16(0x21) + le32(crcValue)
            entry += needs64 ? le32(0xFFFF_FFFF) + le32(0xFFFF_FFFF)
                             : le32(UInt32(written)) + le32(UInt32(written))
            entry += le16(UInt16(name.count)) + le16(UInt16(extra.count)) + le16(0) + le16(0) + le16(0)
                + le32(0o100644 << 16)
                + (needs64 ? le32(0xFFFF_FFFF) : le32(UInt32(headerOffset)))
            entry += name + extra
            central.append(contentsOf: entry)
            count += 1
        }

        let centralStart = offset
        try out.write(contentsOf: central)
        offset += UInt64(central.count)
        let need64 = centralStart >= 0xFFFF_FFFF || count >= 0xFFFF
        if need64 {
            let eocd64Start = offset
            let rec: [UInt8] = le32(0x0606_4B50) + le64(44) + le16(45) + le16(45) + le32(0) + le32(0)
                + le64(UInt64(count)) + le64(UInt64(count)) + le64(UInt64(central.count)) + le64(centralStart)
            let locator: [UInt8] = le32(0x0706_4B50) + le32(0) + le64(eocd64Start) + le32(1)
            try out.write(contentsOf: Data(rec + locator))
        }
        let n16 = UInt16(min(count, 0xFFFF))
        let eocd: [UInt8] = le32(0x0605_4B50) + le16(0) + le16(0) + le16(n16) + le16(n16)
            + le32(UInt32(min(UInt64(central.count), 0xFFFF_FFFF)))
            + le32(need64 ? 0xFFFF_FFFF : UInt32(centralStart)) + le16(0)
        try out.write(contentsOf: Data(eocd))
        log("zip: packed \(count) file(s), \(offset + UInt64(eocd.count)) bytes")
        return count
    }

    // ------------------------------------------------------------------ read

    private struct Entry {
        let name: String
        let method: UInt16
        let compressed: UInt64
        let size: UInt64
        let localOffset: UInt64
    }

    private static func read(_ h: FileHandle, at: UInt64, count: Int) throws -> Data {
        try h.seek(toOffset: at)
        let d = try h.read(upToCount: count) ?? Data()
        guard d.count == count else { throw BackupArchiveError("the zip is cut short") }
        return d
    }

    private static func entries(_ h: FileHandle) throws -> [Entry] {
        let length = try h.seekToEnd()
        guard length >= 22 else { throw BackupArchiveError("this is not a zip file") }
        let tailLength = Int(min(length, 65_557))
        let tail = try read(h, at: length - UInt64(tailLength), count: tailLength)
        var eocd = -1
        var i = tailLength - 22
        while i >= 0 {
            if u32(tail, i) == 0x0605_4B50 { eocd = i; break }
            i -= 1
        }
        guard eocd >= 0 else { throw BackupArchiveError("this is not a zip file (no directory found)") }
        var total = UInt64(u16(tail, eocd + 10))
        var dirSize = UInt64(u32(tail, eocd + 12))
        var dirStart = UInt64(u32(tail, eocd + 16))
        if eocd >= 20, u32(tail, eocd - 20) == 0x0706_4B50 {
            let recAt = u64(tail, eocd - 20 + 8)
            let rec = try read(h, at: recAt, count: 56)
            guard u32(rec, 0) == 0x0606_4B50 else { throw BackupArchiveError("damaged ZIP64 directory") }
            total = u64(rec, 32)
            dirSize = u64(rec, 40)
            dirStart = u64(rec, 48)
        }
        guard dirStart + dirSize <= length, dirSize < 512 << 20 else {
            throw BackupArchiveError("damaged zip directory")
        }
        let dir = try read(h, at: dirStart, count: Int(dirSize))
        var list: [Entry] = []
        var p = 0
        while list.count < total, p + 46 <= dir.count {
            guard u32(dir, p) == 0x0201_4B50 else { throw BackupArchiveError("damaged zip entry") }
            let flags = u16(dir, p + 8)
            let method = u16(dir, p + 10)
            var compressed = UInt64(u32(dir, p + 20))
            var size = UInt64(u32(dir, p + 24))
            let nameLen = Int(u16(dir, p + 28))
            let extraLen = Int(u16(dir, p + 30))
            let commentLen = Int(u16(dir, p + 32))
            var localOffset = UInt64(u32(dir, p + 42))
            guard p + 46 + nameLen + extraLen + commentLen <= dir.count else {
                throw BackupArchiveError("damaged zip entry")
            }
            let nameData = dir.subdata(in: (dir.startIndex + p + 46)..<(dir.startIndex + p + 46 + nameLen))
            let name = String(data: nameData, encoding: (flags & 0x0800) != 0 ? .utf8 : .utf8)
                ?? String(decoding: nameData, as: UTF8.self)
            var e = p + 46 + nameLen
            let extraEnd = e + extraLen
            while e + 4 <= extraEnd {
                let id = u16(dir, e), len = Int(u16(dir, e + 2))
                if id == 1 {
                    var q = e + 4
                    if size == 0xFFFF_FFFF, q + 8 <= e + 4 + len { size = u64(dir, q); q += 8 }
                    if compressed == 0xFFFF_FFFF, q + 8 <= e + 4 + len { compressed = u64(dir, q); q += 8 }
                    if localOffset == 0xFFFF_FFFF, q + 8 <= e + 4 + len { localOffset = u64(dir, q) }
                }
                e += 4 + len
            }
            if (flags & 1) != 0 { throw BackupArchiveError("the zip is password protected") }
            list.append(Entry(name: name, method: method, compressed: compressed, size: size,
                              localOffset: localOffset))
            p = extraEnd + commentLen
        }
        return list
    }

    /// Unpacks `zip` into the empty folder `destination`. Returns the number of files written.
    @discardableResult
    static func unpack(zip: URL, into destination: URL, log: (String) -> Void) throws -> Int {
        let fm = FileManager.default
        let h = try FileHandle(forReadingFrom: zip)
        defer { try? h.close() }
        let list = try entries(h)
        log("unzip: \(list.count) entr(ies) in \(zip.lastPathComponent)")
        try fm.createDirectory(at: destination, withIntermediateDirectories: true)
        var files = 0
        for entry in list {
            let parts = entry.name.replacingOccurrences(of: "\\", with: "/")
                .split(separator: "/").map(String.init)
            // Never write outside the destination; skip macOS junk.
            if parts.isEmpty || parts.contains("..") || parts.first == "__MACOSX"
                || parts.last == ".DS_Store" { continue }
            let target = parts.reduce(destination) { $0.appendingPathComponent($1) }
            if entry.name.hasSuffix("/") {
                try fm.createDirectory(at: target, withIntermediateDirectories: true)
                continue
            }
            try fm.createDirectory(at: target.deletingLastPathComponent(), withIntermediateDirectories: true)
            let header = try read(h, at: entry.localOffset, count: 30)
            guard u32(header, 0) == 0x0403_4B50 else { throw BackupArchiveError("damaged zip entry \(entry.name)") }
            let dataStart = entry.localOffset + 30 + UInt64(u16(header, 26)) + UInt64(u16(header, 28))
            try? fm.removeItem(at: target)
            guard fm.createFile(atPath: target.path, contents: nil) else {
                throw BackupArchiveError("could not write \(entry.name)")
            }
            let out = try FileHandle(forWritingTo: target)
            defer { try? out.close() }
            try h.seek(toOffset: dataStart)
            switch entry.method {
            case 0:
                var left = entry.compressed
                while left > 0 {
                    let piece = try autoreleasepool { try h.read(upToCount: Int(min(left, UInt64(chunk)))) ?? Data() }
                    if piece.isEmpty { throw BackupArchiveError("the zip is cut short at \(entry.name)") }
                    try out.write(contentsOf: piece)
                    left -= UInt64(piece.count)
                }
            case 8:
                try inflate(from: h, compressed: entry.compressed, into: out, name: entry.name)
            default:
                throw BackupArchiveError("\(entry.name) uses a zip compression Continuum cannot read (\(entry.method))")
            }
            files += 1
        }
        log("unzip: wrote \(files) file(s)")
        return files
    }

    private static func inflate(from h: FileHandle, compressed: UInt64, into out: FileHandle, name: String) throws {
        let dst = UnsafeMutablePointer<UInt8>.allocate(capacity: chunk)
        defer { dst.deallocate() }
        var stream = compression_stream(dst_ptr: dst, dst_size: 0, src_ptr: UnsafePointer(dst), src_size: 0, state: nil)
        guard compression_stream_init(&stream, COMPRESSION_STREAM_DECODE, COMPRESSION_ZLIB) == COMPRESSION_STATUS_OK else {
            throw BackupArchiveError("could not start unzipping \(name)")
        }
        defer { compression_stream_destroy(&stream) }
        var left = compressed
        var finished = false
        while !finished {
            let piece = left > 0 ? (try h.read(upToCount: Int(min(left, UInt64(chunk)))) ?? Data()) : Data()
            if left > 0 && piece.isEmpty { throw BackupArchiveError("the zip is cut short at \(name)") }
            left -= UInt64(piece.count)
            let last = left == 0
            try piece.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
                stream.src_ptr = raw.bindMemory(to: UInt8.self).baseAddress ?? UnsafePointer(dst)
                stream.src_size = raw.count
                repeat {
                    stream.dst_ptr = dst
                    stream.dst_size = chunk
                    let status = compression_stream_process(&stream, last ? Int32(COMPRESSION_STREAM_FINALIZE.rawValue) : 0)
                    let produced = chunk - stream.dst_size
                    if produced > 0 { try out.write(contentsOf: Data(bytes: dst, count: produced)) }
                    if status == COMPRESSION_STATUS_END { finished = true; break }
                    if status == COMPRESSION_STATUS_ERROR { throw BackupArchiveError("\(name) is damaged in the zip") }
                    if stream.src_size == 0 && stream.dst_size > 0 { break }
                } while true
            }
            if last && !finished && piece.isEmpty { finished = true }
        }
    }
}
