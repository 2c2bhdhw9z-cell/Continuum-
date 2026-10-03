// Continuum - the skin's button sound (`sound.caf`), played on every press.
//
// LOW LATENCY: a small pool of prepared `AVAudioPlayer`s, so a fast run of presses overlaps rather
// than cutting itself off, and the first press does not wait for a decode.
//
// THE APP VOLUME: each play takes the volume the game's audio uses (and nothing at all while the
// app is muted), read through `volume`, which `EngineHost.wireSkinFunctions` points at
// `EmulationSettings`.
//
// THE MUTE SWITCH: the app's audio session is `.playback` so games are heard with the switch on
// silent, and `.playback` ignores the switch for every sound the app makes. iOS has no API that
// reports the switch, so the one reliable signal is used: a system sound, which iOS itself
// silences on mute, finishes almost at once when it was silenced. A short silent system sound is
// played now and then; when it comes back early the switch is on silent and button clicks are
// skipped. The game's own audio is not affected.

import AudioToolbox
import AVFoundation
import Foundation
import QuartzCore

@MainActor
final class SkinButtonSound {
    static let shared = SkinButtonSound()

    /// 0...1, the app's game volume, 0 while muted. Set once by the host.
    var volume: () -> Float = { 1 }

    private var loadedURL: URL?
    private var players: [AVAudioPlayer] = []
    private var nextPlayer = 0
    private let silentSwitch = SilentSwitchProbe()

    /// Loads the skin's sound ahead of the first press. Nil unloads.
    func prepare(_ url: URL?) {
        guard url != loadedURL else { return }
        loadedURL = url
        players.removeAll()
        nextPlayer = 0
        guard let url else { return }
        for _ in 0..<4 {
            guard let player = try? AVAudioPlayer(contentsOf: url) else { break }
            player.prepareToPlay()
            players.append(player)
        }
        silentSwitch.refresh(force: true)
    }

    /// One click, when Settings allows it, the app is not muted and the switch is not on silent.
    func play(_ url: URL?) {
        guard let url, SkinRuntime.shared.buttonSoundEnabled else { return }
        let level = max(0, min(1, volume()))
        guard level > 0.001 else { return }
        silentSwitch.refresh(force: false)
        guard !silentSwitch.isSilent else { return }
        prepare(url)
        guard !players.isEmpty else { return }
        let player = players[nextPlayer % players.count]
        nextPlayer &+= 1
        player.volume = level
        player.currentTime = 0
        player.play()
    }
}

/// Reads the ring/silent switch the only way iOS allows: a silenced system sound ends at once.
@MainActor
final class SilentSwitchProbe {
    private(set) var isSilent = false
    private var soundID: SystemSoundID = 0
    private var lastProbe: CFTimeInterval = 0
    private var probing = false

    /// The probe sound is a quarter second long; one that "finished" in under a tenth of a
    /// second was not played.
    private static let length = 0.25
    private static let silentBelow = 0.1

    init() {
        guard let url = Self.writeSilence() else { return }
        AudioServicesCreateSystemSoundID(url as CFURL, &soundID)
    }

    /// Probes again when the last probe is more than two seconds old (or now, when forced).
    func refresh(force: Bool) {
        guard soundID != 0, !probing else { return }
        let now = CACurrentMediaTime()
        guard force || now - lastProbe > 2 else { return }
        probing = true
        lastProbe = now
        let threshold = Self.silentBelow
        AudioServicesPlaySystemSoundWithCompletion(soundID) { [weak self] in
            let elapsed = CACurrentMediaTime() - now
            Task { @MainActor in
                self?.isSilent = elapsed < threshold
                self?.probing = false
            }
        }
    }

    /// A quarter second of silence as a 16-bit mono WAV in Caches, written once.
    private static func writeSilence() -> URL? {
        guard let caches = FileManager.default.urls(for: .cachesDirectory,
                                                    in: .userDomainMask).first else { return nil }
        let url = caches.appendingPathComponent("continuum-silence.wav")
        if FileManager.default.fileExists(atPath: url.path) { return url }
        let rate: UInt32 = 22_050
        let samples = UInt32(Double(rate) * length)
        let dataBytes = samples * 2
        var wav = Data()
        func put(_ text: String) { wav.append(contentsOf: Array(text.utf8)) }
        func put32(_ value: UInt32) { withUnsafeBytes(of: value.littleEndian) { wav.append(contentsOf: $0) } }
        func put16(_ value: UInt16) { withUnsafeBytes(of: value.littleEndian) { wav.append(contentsOf: $0) } }
        put("RIFF"); put32(36 + dataBytes); put("WAVE")
        put("fmt "); put32(16); put16(1); put16(1); put32(rate); put32(rate * 2); put16(2); put16(16)
        put("data"); put32(dataBytes)
        wav.append(Data(count: Int(dataBytes)))
        do {
            try wav.write(to: url, options: .atomic)
            return url
        } catch {
            return nil
        }
    }
}
