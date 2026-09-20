//
//  TestLaunchView.swift
//  Continuum - Simple test to launch one NES ROM
//

import SwiftUI

struct TestLaunchView: View {
    @StateObject private var host = TestEngineHost()
    
    var body: some View {
        ZStack {
            // Metal canvas (game display)
            MetalCanvasView(
                engine: host.engine,
                onAttach: { host.surfaceAttached($0) },
                onTelemetry: { telemetry in
                    host.frameCount = telemetry.frameCount
                    host.displayFps = telemetry.displayFps
                    host.dropped = telemetry.dropped
                }
            )
            .ignoresSafeArea()
            
            // Status HUD
            VStack {
                HStack {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Continuum - NES Test")
                            .font(.system(.caption, design: .monospaced)).bold()
                        Text(host.status)
                        if !host.gpu.isEmpty {
                            Text(host.gpu)
                        }
                        if host.sessionStarted {
                            Text("\(host.frameCount) frames · \(String(format: "%.0f", host.displayFps)) fps")
                        }
                    }
                    .font(.system(.caption2, design: .monospaced))
                    .foregroundStyle(.white)
                    .padding(10)
                    .background(.black.opacity(0.7), in: RoundedRectangle(cornerRadius: 8))
                    
                    Spacer()
                }
                .padding()
                
                Spacer()
                
                // Launch button - only enabled when GPU is ready
                if !host.sessionStarted {
                    Button(action: host.launchTestROM) {
                        Text("Launch Test NES ROM")
                            .font(.headline)
                            .padding(.horizontal, 24)
                            .padding(.vertical, 12)
                            .background(host.gpuReady ? Color.blue : Color.gray)
                            .foregroundColor(.white)
                            .cornerRadius(12)
                    }
                    .disabled(!host.gpuReady)
                    .padding(.bottom, 50)
                }
            }
        }
        .background(.black)
        .preferredColorScheme(.dark)
        .statusBarHidden(true)
    }
}

@MainActor
final class TestEngineHost: ObservableObject {
    let engine: ContinuumEngine
    
    @Published var frameCount: UInt64 = 0
    @Published var displayFps: Double = 0
    @Published var dropped: UInt32 = 0
    @Published var status: String = "Initializing..."
    @Published var gpu: String = ""
    @Published var sessionStarted = false
    @Published var gpuReady = false
    
    init() {
        engine = ContinuumEngine()
        
        // Declare NES core at startup
        do {
            try engine.declareCore(
                declaration: CoreDeclaration(
                    id: "fceumm",
                    displayName: "FCEUmm",
                    systems: ["nes"],
                    modulePath: "", // Set when loading
                    baseWidth: 256,
                    baseHeight: 240,
                    maxWidth: 256,
                    maxHeight: 240,
                    aspectRatio: 256.0 / 240.0,
                    targetFps: 60.0988,
                    audioSampleRate: 48000,
                    pixelFormat: 0, // RGB565
                    priority: 100
                )
            )
            status = "NES core declared. Waiting for GPU..."
        } catch {
            status = "Failed to declare core: \(error)"
        }
    }
    
    func surfaceAttached(_ result: Result<String, Error>) {
        switch result {
        case .success(let summary):
            gpu = summary
            gpuReady = true
            status = "GPU ready. Tap button to launch."
        case .failure(let error):
            status = "GPU failed: \(error)"
        }
    }
    
    func launchTestROM() {
        guard gpuReady else {
            status = "Error: GPU not ready"
            return
        }
        
        status = "Loading NES core..."
        
        // Create a minimal NES test ROM (iNES header + minimal data)
        let testROM = createMinimalNESROM()
        
        // Save to temp location
        let tempURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("test-nes.nes")
        
        do {
            try testROM.write(to: tempURL)
            
            status = "Checking core state..."
            
            // Check if core needs loading
            let state = engine.coreState(coreId: "fceumm")
            
            if state == "declared" {
                status = "Loading fceumm dylib..."
                
                // Find the dylib
                guard let dylibPath = Bundle.main.privateFrameworksURL?
                    .appendingPathComponent("libretro_fceumm.dylib") else {
                    status = "Error: No Frameworks directory"
                    return
                }
                
                guard FileManager.default.fileExists(atPath: dylibPath.path) else {
                    status = "Error: libretro_fceumm.dylib not found in bundle"
                    return
                }
                
                let systemDir = FileManager.default.urls(
                    for: .applicationSupportDirectory,
                    in: .userDomainMask
                ).first
                
                // THIS IS THE CRITICAL STEP
                try engine.loadNativeCore(
                    coreId: "fceumm",
                    libraryPath: dylibPath.path,
                    systemDir: systemDir?.path,
                    saveDir: systemDir?.path
                )
                
                status = "Core loaded. Launching ROM..."
            }
            
            // Now launch
            try engine.launch(
                coreId: "fceumm",
                contentId: "test-nes",
                rom: testROM,
                filename: tempURL.path
            )
            
            sessionStarted = true
            status = "NES game running!"
            
        } catch {
            status = "Launch failed: \(error.localizedDescription)"
        }
    }
    
    /// Creates a minimal valid NES ROM with proper reset vector
    private func createMinimalNESROM() -> Data {
        var rom = Data()

        // iNES header (16 bytes)
        rom.append(contentsOf: [
            0x4E, 0x45, 0x53, 0x1A,  // "NES" + EOF
            0x01,                     // 1 x 16KB PRG ROM
            0x01,                     // 1 x 8KB CHR ROM
            0x00,                     // Mapper 0 (NROM), horizontal mirroring
            0x00,                     // Mapper 0 upper nibble
            0x00, 0x00, 0x00, 0x00,  // Padding
            0x00, 0x00, 0x00, 0x00
        ])

        // 16KB PRG ROM with valid reset vector
        var prg = Data(count: 16384)

        // Write a simple infinite loop at $C000
        // LDA #$00 / STA $2001 / JMP $C000
        prg[0x0000] = 0xA9  // LDA immediate
        prg[0x0001] = 0x00  // #$00
        prg[0x0002] = 0x8D  // STA absolute
        prg[0x0003] = 0x01  // $2001 (PPU mask register)
        prg[0x0004] = 0x20
        prg[0x0005] = 0x4C  // JMP absolute
        prg[0x0006] = 0x00  // $C000
        prg[0x0007] = 0xC0

        // Set reset vector at $FFFC-$FFFD to point to $C000
        prg[0x3FFC] = 0x00  // Low byte of $C000
        prg[0x3FFD] = 0xC0  // High byte of $C000

        rom.append(prg)

        // 8KB CHR ROM (graphics)
        var chr = Data(count: 8192)
        // Add a simple pattern so it's not all zeros
        for i in 0..<256 {
            chr[i] = UInt8(i % 256)
        }
        rom.append(chr)

        return rom
    }
}
