//
//  CoreManager.swift
//  Continuum
//
//  Manages core declarations, loading, and launch lifecycle.
//

import Foundation

/// Manages emulator cores: declaration, loading, and launch.
final class CoreManager {
    private let engine: ContinuumEngine

    init(engine: ContinuumEngine) {
        self.engine = engine
    }

    // MARK: - Core Registry

    /// Declares all cores the app supports. Call this at app startup.
    func declareAllCores() throws {
        // PS1
        try declareCore(
            id: "pcsx_rearmed",
            name: "PCSX ReARMed",
            systems: ["ps1"],
            baseWidth: 320,
            baseHeight: 240,
            maxWidth: 700,
            maxHeight: 576,
            aspectRatio: 4.0 / 3.0,
            fps: 59.94,
            sampleRate: 44100,
            pixelFormat: 1, // XRGB8888
            priority: 100
        )

        // NES
        try declareCore(
            id: "fceumm",
            name: "FCEUmm",
            systems: ["nes"],
            baseWidth: 256,
            baseHeight: 240,
            maxWidth: 256,
            maxHeight: 240,
            aspectRatio: 256.0 / 240.0,
            fps: 60.0988,
            sampleRate: 48000,
            pixelFormat: 0, // RGB565
            priority: 100
        )

        // GBA
        try declareCore(
            id: "mgba",
            name: "mGBA",
            systems: ["gba", "gb", "gbc"],
            baseWidth: 240,
            baseHeight: 160,
            maxWidth: 240,
            maxHeight: 160,
            aspectRatio: 240.0 / 160.0,
            fps: 59.7275,
            sampleRate: 65536,
            pixelFormat: 0, // RGB565
            priority: 100
        )

        // SNES
        try declareCore(
            id: "snes9x",
            name: "Snes9x",
            systems: ["snes"],
            baseWidth: 256,
            baseHeight: 224,
            maxWidth: 1024,
            maxHeight: 478,
            aspectRatio: 4.0 / 3.0,
            fps: 60.0988,
            sampleRate: 32040,
            pixelFormat: 0, // RGB565
            priority: 100
        )

        // Genesis / Mega Drive / Master System
        try declareCore(
            id: "genesis_plus_gx",
            name: "Genesis Plus GX",
            systems: ["genesis", "megadrive", "sms"],
            baseWidth: 320,
            baseHeight: 224,
            maxWidth: 348,
            maxHeight: 240,
            aspectRatio: 4.0 / 3.0,
            fps: 59.9227,
            sampleRate: 44100,
            pixelFormat: 0, // RGB565
            priority: 100
        )
    }

    private func declareCore(
        id: String,
        name: String,
        systems: [String],
        baseWidth: UInt32,
        baseHeight: UInt32,
        maxWidth: UInt32,
        maxHeight: UInt32,
        aspectRatio: Float,
        fps: Double,
        sampleRate: UInt32,
        pixelFormat: UInt32,
        priority: Int32
    ) throws {
        try engine.declareCore(
            declaration: CoreDeclaration(
                id: id,
                displayName: name,
                systems: systems,
                modulePath: "", // Set when loading
                baseWidth: baseWidth,
                baseHeight: baseHeight,
                maxWidth: maxWidth,
                maxHeight: maxHeight,
                aspectRatio: aspectRatio,
                targetFps: fps,
                audioSampleRate: sampleRate,
                pixelFormat: pixelFormat,
                priority: priority
            )
        )
        print("✓ Declared core: \(id)")
    }

    // MARK: - Core Loading

    /// Loads a core if not already loaded. Safe to call multiple times.
    func ensureCoreLoaded(coreId: String) throws {
        guard let state = engine.coreState(coreId: coreId) else {
            throw CoreError.notDeclared(coreId)
        }

        switch state {
        case "loaded", "bound":
            // Already loaded, nothing to do
            return

        case "declared":
            // Need to load
            try loadCore(coreId: coreId)

        case "failed":
            throw CoreError.loadFailed(coreId, "Core is in failed state")

        default:
            throw CoreError.unknownState(coreId, state)
        }
    }

    private func loadCore(coreId: String) throws {
        // Find the dylib in the app bundle
        guard let frameworksURL = Bundle.main.privateFrameworksURL else {
            throw CoreError.bundleError("No Frameworks directory in bundle")
        }

        let dylibName = "libretro_\(coreId).dylib"
        let dylibPath = frameworksURL.appendingPathComponent(dylibName)

        guard FileManager.default.fileExists(atPath: dylibPath.path) else {
            throw CoreError.dylibNotFound(dylibName)
        }

        // Get system/save directories
        let systemDir = FileManager.default.urls(
            for: .applicationSupportDirectory,
            in: .userDomainMask
        ).first

        let saveDir = FileManager.default.urls(
            for: .documentDirectory,
            in: .userDomainMask
        ).first

        // Load the native core
        try engine.loadNativeCore(
            coreId: coreId,
            libraryPath: dylibPath.path,
            systemDir: systemDir?.path,
            saveDir: saveDir?.path
        )

        print("✓ Loaded core: \(coreId) from \(dylibPath.lastPathComponent)")
    }

    // MARK: - Game Launch

    /// Launches a game with automatic core loading.
    func launchGame(
        coreId: String,
        contentId: String,
        romPath: String,
        needsFullPath: Bool = false
    ) throws {
        // 1. Ensure core is loaded
        try ensureCoreLoaded(coreId: coreId)

        // 2. Load ROM data (or empty for need_fullpath cores)
        let romData: Data
        if needsFullPath {
            romData = Data() // Core reads from filename
        } else {
            romData = try Data(contentsOf: URL(fileURLWithPath: romPath))
        }

        // 3. Launch
        try engine.launch(
            coreId: coreId,
            contentId: contentId,
            rom: romData,
            filename: romPath
        )

        print("✓ Launched: \(contentId) on \(coreId)")
    }

    /// Launches a PS1 game (handles .cue + .bin)
    func launchPS1Game(cuePath: String, contentId: String? = nil) throws {
        let id = contentId ?? (cuePath as NSString).lastPathComponent

        // PS1 cores use need_fullpath (they read .bin from .cue)
        try launchGame(
            coreId: "pcsx_rearmed",
            contentId: id,
            romPath: cuePath,
            needsFullPath: true
        )
    }
}

// MARK: - Errors

enum CoreError: LocalizedError {
    case notDeclared(String)
    case dylibNotFound(String)
    case loadFailed(String, String)
    case bundleError(String)
    case unknownState(String, String)

    var errorDescription: String? {
        switch self {
        case .notDeclared(let coreId):
            return "Core '\(coreId)' not declared. Call declareAllCores() first."
        case .dylibNotFound(let name):
            return "Core library '\(name)' not found in app bundle."
        case .loadFailed(let coreId, let reason):
            return "Core '\(coreId)' failed to load: \(reason)"
        case .bundleError(let reason):
            return "Bundle error: \(reason)"
        case .unknownState(let coreId, let state):
            return "Core '\(coreId)' is in unknown state: \(state)"
        }
    }
}
