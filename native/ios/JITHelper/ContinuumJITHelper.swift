// Continuum's JIT helper: a small app extension that turns JIT on for Continuum from the phone itself,
// with the pairing file and LocalDevVPN, the way Manic and DolphiniOS do with their built-in debugger.
//
// WHY A SEPARATE PROCESS. Turning JIT on means attaching a debugger to Continuum, and a process that
// attaches a debugger to itself deadlocks. So Continuum starts this extension by its bundle id
// (BuiltInJIT.swift), hands it its own process id and the pairing file, and the extension does the
// attaching through StikJIT (the StikDebug team's library, MPL-2.0, compiled in from source by
// scripts/fetch-stikjit.sh). On an iPhone with TXM the universal script stays attached until
// Continuum has had its code memory prepared and says it is done (crates/emulator-bridge/src/jit26.c),
// and only then does the request finish.
//
// One request per extension request: a small JSON object in the input item's userInfo, answered
// with JSON in the item the request completes with. Nothing here is ever logged with the pairing file.

import Foundation

/// What Continuum asks for.
struct ContinuumJITRequest: Codable {
    static let itemKey = "continuum-jit-request"
    /// "enable", "prepare" or "reset".
    let operation: String
    let targetPID: Int32?
    let pairing: Data?
}

/// What the helper answers.
struct ContinuumJITResponse: Codable {
    static let itemKey = "continuum-jit-response"
    let ok: Bool
    let message: String
    /// The steps the helper went through, for Settings' technical line.
    let log: [String]
}

private enum Work {
    /// StikJIT's calls block and must run one after another (its guide says so).
    static let queue = DispatchQueue(label: "dev.continuum.jit-helper")

    static func handle(_ request: ContinuumJITRequest) -> ContinuumJITResponse {
        var steps: [String] = []
        let note: (String) -> Void = { line in
            steps.append(line)
            if steps.count > 40 { steps.removeFirst() }
        }
        let fm = FileManager.default
        let root = fm.urls(for: .libraryDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("StikJIT", isDirectory: true)
        let paths = DDIPaths.default(in: root)
        do {
            try fm.createDirectory(at: root, withIntermediateDirectories: true)
            if request.operation == "reset" {
                try StikJIT.resetCachedDDI(at: paths)
                return ContinuumJITResponse(ok: true, message: "The developer disk image will be downloaded again next time.", log: steps)
            }
            guard let pairing = request.pairing, !pairing.isEmpty else {
                return ContinuumJITResponse(ok: false, message: "No pairing file was given.", log: steps)
            }
            // A file of the helper's own, gone as soon as the request is.
            let file = root.appendingPathComponent("pairing-\(UUID().uuidString).plist")
            try pairing.write(to: file, options: .atomic)
            defer { try? fm.removeItem(at: file) }
            let stage: (StikJIT.PreparationStage) -> Void = { note(describe($0)) }
            switch request.operation {
            case "prepare":
                switch StikJIT.prepareDevice(pairingFile: file, paths: paths, progress: stage) {
                case .ready:
                    return ContinuumJITResponse(ok: true, message: "Ready.", log: steps)
                case .unreachable(let reason):
                    return ContinuumJITResponse(ok: false, message: "Can't reach the phone: \(reason)", log: steps)
                case .preparationFailed(let reason):
                    return ContinuumJITResponse(ok: false, message: reason, log: steps)
                }
            case "enable":
                guard let pid = request.targetPID, pid > 0 else {
                    return ContinuumJITResponse(ok: false, message: "No process to turn JIT on for.", log: steps)
                }
                try StikJIT.enableJIT(targetPID: pid, pairingFile: file, ddiPaths: paths,
                                      script: .universal, forceScript: false,
                                      preparationProgress: stage,
                                      progress: { note($0) })
                return ContinuumJITResponse(ok: true, message: "JIT is on.", log: steps)
            default:
                return ContinuumJITResponse(ok: false, message: "Unknown request \(request.operation).", log: steps)
            }
        } catch {
            return ContinuumJITResponse(ok: false, message: error.localizedDescription, log: steps)
        }
    }

    static func describe(_ stage: StikJIT.PreparationStage) -> String {
        switch stage {
        case .checkingReachability: return "checking LocalDevVPN"
        case .checkingDDI: return "checking the developer disk image"
        case .downloadingDDI(let fraction, let status): return "downloading the developer disk image \(Int(fraction * 100))% \(status)"
        case .mountingDDI(let fraction): return "mounting the developer disk image \(Int(fraction * 100))%"
        case .verifyingDDI: return "checking the mount"
        case .ready: return "ready"
        }
    }
}

/// The extension's principal class (Info.plist NSExtensionPrincipalClass).
@objc(ContinuumJITHelperHandler)
final class ContinuumJITHelperHandler: NSObject, NSExtensionRequestHandling {
    func beginRequest(with context: NSExtensionContext) {
        let info = (context.inputItems.first as? NSExtensionItem)?.userInfo
        let data = info?[ContinuumJITRequest.itemKey] as? Data
        Work.queue.async {
            let response: ContinuumJITResponse
            if let data, let request = try? JSONDecoder().decode(ContinuumJITRequest.self, from: data) {
                response = Work.handle(request)
            } else {
                response = ContinuumJITResponse(ok: false, message: "The JIT helper got no request.", log: [])
            }
            let item = NSExtensionItem()
            item.userInfo = [ContinuumJITResponse.itemKey: (try? JSONEncoder().encode(response)) ?? Data()]
            context.completeRequest(returningItems: [item], completionHandler: nil)
        }
    }
}
