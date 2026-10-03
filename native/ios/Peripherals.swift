// Continuum - the phone's own hardware handed to a game: microphone, camera, and Amiibo files.
//
// THIN ON PURPOSE. Everything with a decision in it is in Rust (crates/emulator-bridge/src/
// peripherals): the libretro interfaces, the ring, the resampling, the camera scaling, the Amiibo
// checks. This file only owns what only iOS can do: the permission prompts, the AVAudioSession
// category, the AVAudioEngine input tap, the AVCaptureSession, and the Files picker.
//
// HOW IT IS DRIVEN. `Peripherals.poll()` runs once per display-link frame and reads
// `engine.peripheralRequests()`, which is a handful of atomic loads in Rust and takes no lock. When
// a core opens a microphone (and "Allow microphone in games" is on) the input tap starts; when it
// closes it, the tap stops. The camera follows the core's `start` and `stop` the same way. Nothing
// is captured while no core asks, so the orange and green privacy dots only ever appear while a
// game is genuinely listening or looking.
//
// THE THREAD RULE. The tap and the capture delegate call `pushMicrophoneSamples` and
// `pushCameraFrame` from their own threads. Those two engine methods never take the engine lock
// (see uniffi_peripherals.rs), which is what keeps a capture thread from waiting a whole tick on
// the display link.

import AVFoundation
import SwiftUI
import UIKit
import UniformTypeIdentifiers

// MARK: - The audio session category

/// Which category the shared AVAudioSession should be in.
///
/// `.playback` normally. `.playAndRecord` with `.defaultToSpeaker` and `.allowBluetooth` only while
/// a game holds a microphone, because that category routes game sound to the earpiece without
/// `.defaultToSpeaker` and lowers Bluetooth to call quality, neither of which anyone wants while
/// not blowing into the phone. `AudioOutput.startGraph` asks this rather than hardcoding
/// `.playback`, so a route-change rebuild of the output graph while the mic is open does not drop
/// the session back out of record mode underneath the input tap.
enum AudioSessionPolicy {
    /// Set only by `MicrophoneCapture`, on the main thread.
    static var recording = false

    static func apply(to session: AVAudioSession) throws {
        let category: AVAudioSession.Category = recording ? .playAndRecord : .playback
        let options: AVAudioSession.CategoryOptions = recording
            ? [.defaultToSpeaker, .allowBluetooth]
            : []
        // Skipped when nothing would change. Setting the same category again can still post a
        // route change, and both audio graphs rebuild on one, so a redundant set here is how two
        // engines end up restarting each other.
        if session.category == category, session.mode == .default,
           session.categoryOptions == options {
            return
        }
        try session.setCategory(category, mode: .default, options: options)
    }

    static var categoryName: String {
        recording ? "playAndRecord" : "playback"
    }
}

// MARK: - The microphone

/// The AVAudioEngine input tap. A SEPARATE engine from `AudioOutput`'s, so starting and stopping
/// the microphone never touches the output graph beyond the category change iOS forces on it.
final class MicrophoneCapture {
    private let engine: ContinuumEngine
    private var audioEngine: AVAudioEngine?
    private var observer: NSObjectProtocol?
    private(set) var running = false
    private(set) var status = "mic: not started"

    init(engine: ContinuumEngine) {
        self.engine = engine
    }

    deinit {
        if let observer { NotificationCenter.default.removeObserver(observer) }
    }

    /// Asks for permission if needed, then starts. `done` runs on the main thread with the result.
    func start(done: @escaping (String) -> Void) {
        let session = AVAudioSession.sharedInstance()
        switch session.recordPermission {
        case .granted:
            done(startGranted())
        case .denied:
            status = "mic: iOS microphone access is off for Continuum (Settings, Privacy, "
                + "Microphone), so the game hears silence"
            done(status)
        case .undetermined:
            status = "mic: asking iOS for microphone access"
            session.requestRecordPermission { granted in
                DispatchQueue.main.async {
                    if granted {
                        done(self.startGranted())
                    } else {
                        self.status = "mic: microphone access was refused, so the game hears "
                            + "silence"
                        done(self.status)
                    }
                }
            }
        @unknown default:
            status = "mic: iOS reported an unknown microphone permission state"
            done(status)
        }
    }

    private func startGranted() -> String {
        guard !running else { return status }
        let session = AVAudioSession.sharedInstance()
        AudioSessionPolicy.recording = true
        do {
            try AudioSessionPolicy.apply(to: session)
            try session.setActive(true)
        } catch {
            AudioSessionPolicy.recording = false
            try? AudioSessionPolicy.apply(to: session)
            status = "mic: the audio session would not switch to record mode: \(error)"
            return status
        }

        let graph = AVAudioEngine()
        let input = graph.inputNode
        let format = input.outputFormat(forBus: 0)
        guard format.channelCount > 0, format.sampleRate > 0 else {
            status = "mic: this device reports no microphone input "
                + "(\(format.channelCount) channels at \(Int(format.sampleRate)) Hz)"
            restorePlayback()
            return status
        }
        let rust = engine
        let rate = UInt32(format.sampleRate.rounded())
        input.installTap(onBus: 0, bufferSize: 1024, format: format) { buffer, _ in
            // Channel 0 only: libretro microphones are mono.
            guard let channel = buffer.floatChannelData?[0] else { return }
            let frames = Int(buffer.frameLength)
            guard frames > 0 else { return }
            let samples = Array(UnsafeBufferPointer(start: channel, count: frames))
            _ = rust.pushMicrophoneSamples(samples: samples, sampleRate: rate)
        }
        graph.prepare()
        do {
            try graph.start()
        } catch {
            input.removeTap(onBus: 0)
            status = "mic: the input engine would not start: \(error)"
            restorePlayback()
            return status
        }
        audioEngine = graph
        running = true
        engine.setMicrophoneCapturing(capturing: true)
        // A route change (headset plugged in) reconfigures this engine and stops it. Rebuilt
        // rather than restarted, because the input format may have changed with the route.
        observer = NotificationCenter.default.addObserver(
            forName: NSNotification.Name.AVAudioEngineConfigurationChange,
            object: graph,
            queue: .main
        ) { [weak self] _ in
            guard let self, self.running else { return }
            self.teardown()
            _ = self.startGranted()
        }
        status = "mic: listening at \(Int(format.sampleRate)) Hz"
        return status
    }

    /// Stops the tap and puts the session back to `.playback`.
    func stop() {
        guard running || audioEngine != nil else { return }
        teardown()
        restorePlayback()
        status = "mic: stopped"
    }

    private func teardown() {
        engine.setMicrophoneCapturing(capturing: false)
        if let observer {
            NotificationCenter.default.removeObserver(observer)
            self.observer = nil
        }
        if let graph = audioEngine {
            graph.inputNode.removeTap(onBus: 0)
            graph.stop()
        }
        audioEngine = nil
        running = false
    }

    private func restorePlayback() {
        AudioSessionPolicy.recording = false
        do {
            try AudioSessionPolicy.apply(to: AVAudioSession.sharedInstance())
        } catch {
            status += " (and the session would not go back to playback: \(error))"
        }
    }
}

// MARK: - The camera

/// The AVCaptureSession. Frames are BGRA, which is libretro's XRGB8888 in memory; Rust scales them
/// to the size the core asked for and hands them over on the core's own thread.
final class CameraCapture: NSObject, AVCaptureVideoDataOutputSampleBufferDelegate {
    private let engine: ContinuumEngine
    private let queue = DispatchQueue(label: "app.continuum.camera")
    private var session: AVCaptureSession?
    private(set) var running = false

    init(engine: ContinuumEngine) {
        self.engine = engine
    }

    /// Asks for permission if needed, then starts. `done` runs on the main thread.
    func start(front: Bool, done: @escaping (String) -> Void) {
        switch AVCaptureDevice.authorizationStatus(for: .video) {
        case .authorized:
            startAuthorised(front: front, done: done)
        case .denied, .restricted:
            done("camera: iOS camera access is off for Continuum (Settings, Privacy, Camera), "
                 + "so the game sees nothing")
        case .notDetermined:
            AVCaptureDevice.requestAccess(for: .video) { granted in
                DispatchQueue.main.async {
                    if granted {
                        self.startAuthorised(front: front, done: done)
                    } else {
                        done("camera: camera access was refused, so the game sees nothing")
                    }
                }
            }
        @unknown default:
            done("camera: iOS reported an unknown camera permission state")
        }
    }

    private func startAuthorised(front: Bool, done: @escaping (String) -> Void) {
        guard !running else {
            done("camera: already running")
            return
        }
        running = true
        // Read on the main thread, used on the capture queue: the picture is turned to match the
        // way the phone is being held, so "up" in the game is up in the room.
        let orientation = Self.videoOrientation()
        let side = front ? "front" : "back"
        queue.async {
            let capture = AVCaptureSession()
            capture.beginConfiguration()
            if capture.canSetSessionPreset(.vga640x480) {
                capture.sessionPreset = .vga640x480
            }
            guard let device = AVCaptureDevice.default(.builtInWideAngleCamera, for: .video,
                                                       position: front ? .front : .back),
                  let input = try? AVCaptureDeviceInput(device: device),
                  capture.canAddInput(input) else {
                capture.commitConfiguration()
                DispatchQueue.main.async {
                    self.running = false
                    done("camera: the \(side) camera could not be opened")
                }
                return
            }
            capture.addInput(input)
            let output = AVCaptureVideoDataOutput()
            output.videoSettings = [
                kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
            ]
            output.alwaysDiscardsLateVideoFrames = true
            output.setSampleBufferDelegate(self, queue: self.queue)
            guard capture.canAddOutput(output) else {
                capture.commitConfiguration()
                DispatchQueue.main.async {
                    self.running = false
                    done("camera: the capture output could not be added")
                }
                return
            }
            capture.addOutput(output)
            if let connection = output.connection(with: .video),
               connection.isVideoOrientationSupported {
                connection.videoOrientation = orientation
            }
            capture.commitConfiguration()
            capture.startRunning()
            let started = capture.isRunning
            DispatchQueue.main.async {
                self.session = capture
                self.running = started
                done(started
                     ? "camera: \(side) camera running"
                     : "camera: the \(side) camera session would not start")
            }
        }
    }

    func stop() {
        guard running || session != nil else { return }
        running = false
        let capture = session
        session = nil
        queue.async {
            capture?.stopRunning()
        }
    }

    func captureOutput(_ output: AVCaptureOutput, didOutput sampleBuffer: CMSampleBuffer,
                       from connection: AVCaptureConnection) {
        guard let pixels = CMSampleBufferGetImageBuffer(sampleBuffer) else { return }
        CVPixelBufferLockBaseAddress(pixels, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(pixels, .readOnly) }
        guard let base = CVPixelBufferGetBaseAddress(pixels) else { return }
        let width = CVPixelBufferGetWidth(pixels)
        let height = CVPixelBufferGetHeight(pixels)
        let stride = CVPixelBufferGetBytesPerRow(pixels)
        let data = Data(bytes: base, count: stride * height)
        _ = engine.pushCameraFrame(bgra: data, width: UInt32(width), height: UInt32(height),
                                   stride: UInt32(stride), mirror: false)
    }

    private static func videoOrientation() -> AVCaptureVideoOrientation {
        let scene = UIApplication.shared.connectedScenes.first as? UIWindowScene
        switch scene?.interfaceOrientation ?? .portrait {
        case .landscapeLeft: return .landscapeLeft
        case .landscapeRight: return .landscapeRight
        case .portraitUpsideDown: return .portraitUpsideDown
        default: return .portrait
        }
    }
}

// MARK: - The owner

/// The microphone and camera switches, the capture objects, and the Amiibo folder.
///
/// Owned by `EngineHost` for the app's lifetime, like `audio` and `controllers`: it holds capture
/// sessions and observers that a SwiftUI rebuild must never replace underneath the display link.
@MainActor
final class Peripherals: ObservableObject {
    enum CameraSide: String, CaseIterable, Identifiable {
        case back
        case front

        var id: String { rawValue }
        var title: String { self == .back ? "Back" : "Front" }
    }

    private static let microphoneKey = "continuum.peripherals.microphoneAllowed"
    private static let cameraKey = "continuum.peripherals.cameraAllowed"
    private static let cameraSideKey = "continuum.peripherals.cameraSide"

    /// "Allow microphone in games". Off until the user turns it on.
    @Published var microphoneAllowed: Bool {
        didSet {
            UserDefaults.standard.set(microphoneAllowed, forKey: Self.microphoneKey)
            engine.setMicrophoneAllowed(allowed: microphoneAllowed)
        }
    }

    /// "Allow camera in games". Off until the user turns it on.
    @Published var cameraAllowed: Bool {
        didSet {
            UserDefaults.standard.set(cameraAllowed, forKey: Self.cameraKey)
            pushCameraAllowed()
        }
    }

    @Published var cameraSide: CameraSide {
        didSet {
            UserDefaults.standard.set(cameraSide.rawValue, forKey: Self.cameraSideKey)
            // A running camera is restarted on the other side at the next poll.
            if camera.running { camera.stop() }
        }
    }

    /// The HUD line: the engine's sentence, then the iOS side's. Never empty.
    @Published private(set) var statusLine = "mic: not asked; camera: not asked"

    /// The Amiibo files in Documents/Amiibo, sorted by name.
    @Published private(set) var amiiboFiles: [URL] = []

    private let engine: ContinuumEngine
    private let microphone: MicrophoneCapture
    private let camera: CameraCapture
    private var micStarting = false
    private var cameraStarting = false
    /// Set when a start failed (no permission, no device), so the poll does not retry sixty times
    /// a second. Cleared when the core stops asking, so the next request tries again.
    private var micGaveUp = false
    private var cameraGaveUp = false
    private var iosLine = ""
    private var statusCountdown = 0
    private var picker: AmiiboPicker?

    init(engine: ContinuumEngine) {
        self.engine = engine
        microphone = MicrophoneCapture(engine: engine)
        camera = CameraCapture(engine: engine)
        let defaults = UserDefaults.standard
        microphoneAllowed = defaults.bool(forKey: Self.microphoneKey)
        cameraAllowed = defaults.bool(forKey: Self.cameraKey)
        cameraSide = CameraSide(rawValue: defaults.string(forKey: Self.cameraSideKey) ?? "")
            ?? .back
        // `didSet` does not run inside init, so the engine is told here.
        engine.setMicrophoneAllowed(allowed: microphoneAllowed)
        pushCameraAllowed()
        refreshAmiibo()
    }

    /// The switch ANDed with what iOS has decided, so the core's `start` gets an honest false when
    /// the camera has been refused in iOS Settings. Undetermined counts as allowed: the prompt is
    /// shown when the game starts the camera, and a refusal there is pushed back in.
    private func pushCameraAllowed() {
        let status = AVCaptureDevice.authorizationStatus(for: .video)
        let refused = status == .denied || status == .restricted
        engine.setCameraAllowed(allowed: cameraAllowed && !refused)
    }

    // MARK: Per frame

    /// Called from the display link after each tick. Atomic reads in Rust; starts and stops the
    /// capture objects only when what the core wants changes.
    func poll() {
        let wanted = engine.peripheralRequests()

        if wanted.microphoneWanted, !microphone.running, !micStarting, !micGaveUp {
            micStarting = true
            microphone.start { [weak self] line in
                guard let self else { return }
                self.micStarting = false
                self.micGaveUp = !self.microphone.running
                self.iosLine = line
            }
        } else if !wanted.microphoneWanted {
            micGaveUp = false
            if microphone.running {
                microphone.stop()
                iosLine = microphone.status
            }
        }

        if wanted.cameraWanted, !camera.running, !cameraStarting, !cameraGaveUp {
            cameraStarting = true
            camera.start(front: cameraSide == .front) { [weak self] line in
                guard let self else { return }
                self.cameraStarting = false
                self.cameraGaveUp = !self.camera.running
                self.iosLine = line
                self.pushCameraAllowed()
            }
        } else if !wanted.cameraWanted {
            cameraGaveUp = false
            if camera.running {
                camera.stop()
                iosLine = "camera: stopped"
            }
        }

        // The sentence is rebuilt twice a second, not every frame: it is a String allocation and a
        // published change, and the HUD is read by a person.
        statusCountdown -= 1
        if statusCountdown <= 0 {
            statusCountdown = 30
            let engineLine = engine.peripheralStatus()
            let line = iosLine.isEmpty
                ? engineLine
                : "\(engineLine) [\(iosLine); session \(AudioSessionPolicy.categoryName)]"
            if line != statusLine { statusLine = line }
        }
    }

    /// Called when a session stops, BEFORE `audio.stop()`, so the session leaves record mode while
    /// it is still active and the output graph can be torn down cleanly afterwards.
    func stopCapture() {
        micStarting = false
        cameraStarting = false
        micGaveUp = false
        cameraGaveUp = false
        if microphone.running { microphone.stop() }
        if camera.running { camera.stop() }
        iosLine = ""
    }

    // MARK: Amiibo

    /// Documents/Amiibo, created if absent. Inside Documents so the folder is visible in the Files
    /// app, and in a subfolder so the Library scan (which reads Documents flat) never mistakes a
    /// .bin tag for a PlayStation disc track.
    var amiiboFolder: URL? {
        guard let documents = FileManager.default.urls(for: .documentDirectory,
                                                       in: .userDomainMask).first else {
            return nil
        }
        let folder = documents.appendingPathComponent("Amiibo", isDirectory: true)
        if !FileManager.default.fileExists(atPath: folder.path) {
            try? FileManager.default.createDirectory(at: folder,
                                                     withIntermediateDirectories: true)
        }
        return folder
    }

    func refreshAmiibo() {
        guard let folder = amiiboFolder,
              let contents = try? FileManager.default.contentsOfDirectory(
                at: folder, includingPropertiesForKeys: nil, options: [.skipsHiddenFiles]) else {
            amiiboFiles = []
            return
        }
        amiiboFiles = contents
            .filter { $0.pathExtension.lowercased() == "bin" }
            .sorted { $0.lastPathComponent.localizedStandardCompare($1.lastPathComponent)
                == .orderedAscending }
    }

    /// Whether the running core can take an Amiibo, in words.
    var amiiboSupport: AmiiboSupport {
        engine.amiiboSupport()
    }

    /// Opens the Files picker, checks every chosen file, copies the good ones into the Amiibo
    /// folder under their original names, and reports one sentence.
    func importAmiibo(done: @escaping (String) -> Void) {
        let picker = AmiiboPicker()
        self.picker = picker
        picker.present { [weak self] urls in
            guard let self else { return }
            self.picker = nil
            done(self.importFiles(urls))
        }
    }

    private func importFiles(_ urls: [URL]) -> String {
        guard !urls.isEmpty else { return "amiibo import: no files chosen" }
        guard let folder = amiiboFolder else {
            return "amiibo import failed: no Documents directory"
        }
        var imported: [String] = []
        var refused: [String] = []
        for url in urls {
            let name = url.lastPathComponent
            let scoped = url.startAccessingSecurityScopedResource()
            defer { if scoped { url.stopAccessingSecurityScopedResource() } }
            guard let data = try? Data(contentsOf: url) else {
                refused.append("\(name) could not be read")
                continue
            }
            let check = engine.inspectAmiibo(data: data)
            guard check.ok else {
                refused.append("\(name): \(check.message)")
                continue
            }
            // Always named .bin in the folder, so the picker finds it; the original name is kept.
            let fileName = url.pathExtension.lowercased() == "bin" ? name : name + ".bin"
            let destination = folder.appendingPathComponent(fileName)
            do {
                if FileManager.default.fileExists(atPath: destination.path) {
                    try FileManager.default.removeItem(at: destination)
                }
                try data.write(to: destination, options: .atomic)
                imported.append(fileName)
            } catch {
                refused.append("\(name) could not be saved: \(error.localizedDescription)")
            }
        }
        refreshAmiibo()
        var line = "amiibo import: \(imported.count) saved to the Amiibo folder"
        if !refused.isEmpty {
            line += ", \(refused.count) refused (\(refused.joined(separator: "; ")))"
        }
        return line
    }

    /// "Taps" one Amiibo on the running game. Returns the status line, always.
    func tap(_ url: URL) -> String {
        guard let data = try? Data(contentsOf: url) else {
            return "amiibo \(url.lastPathComponent) could not be read"
        }
        return engine.tapAmiibo(fileName: url.lastPathComponent, data: data)
    }

    func deleteAmiibo(_ url: URL) -> String {
        do {
            try FileManager.default.removeItem(at: url)
            refreshAmiibo()
            return "amiibo \(url.lastPathComponent) deleted"
        } catch {
            return "amiibo \(url.lastPathComponent) could not be deleted: "
                + error.localizedDescription
        }
    }
}

// MARK: - The Files picker for Amiibo dumps

/// A `UIDocumentPickerViewController`, for the reason every other import in this app uses one:
/// SwiftUI's `.fileImporter` was proven on device not to call its completion.
final class AmiiboPicker: NSObject, UIDocumentPickerDelegate {
    private var onFinish: (([URL]) -> Void)?

    @MainActor
    func present(onFinish: @escaping ([URL]) -> Void) {
        self.onFinish = onFinish
        guard let presenter = EngineHost.topmostViewController() else {
            finish([])
            return
        }
        // `.data` rather than a .bin type: there is no registered type for Amiibo dumps, and
        // `.bin` is claimed by several unrelated formats. Rust checks every file anyway.
        let picker = UIDocumentPickerViewController(forOpeningContentTypes: [.data, .item],
                                                    asCopy: true)
        picker.delegate = self
        picker.allowsMultipleSelection = true
        picker.shouldShowFileExtensions = true
        presenter.present(picker, animated: true)
    }

    private func finish(_ urls: [URL]) {
        let callback = onFinish
        onFinish = nil
        DispatchQueue.main.async { callback?(urls) }
    }

    @objc func documentPicker(_ controller: UIDocumentPickerViewController,
                              didPickDocumentsAt urls: [URL]) {
        finish(urls)
    }

    @objc func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
        finish([])
    }
}

// MARK: - Settings

/// The MICROPHONE, CAMERA AND AMIIBO card in Settings. Its own view so it can observe
/// `Peripherals` directly without the Settings screen growing another stored property.
struct PeripheralsSettingsSection: View {
    @ObservedObject var peripherals: Peripherals
    let report: (String) -> Void

    var body: some View {
        SettingsSection(title: "MICROPHONE, CAMERA AND AMIIBO") {
            Toggle(isOn: $peripherals.microphoneAllowed) {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Allow microphone in games")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(.white)
                    Text(peripherals.microphoneAllowed
                         ? "A game that asks for the microphone hears the iPhone's."
                         : "Games that ask for the microphone hear silence.")
                        .font(.system(size: 12))
                        .foregroundStyle(ShellPalette.secondaryText)
                }
            }
            .tint(ShellPalette.accent)

            SettingsNote(
                "The microphone is only switched on while a game holds it open, and iOS shows its "
                + "orange dot while it is. 3DS games on Azahar ask for it. The DS core used here "
                + "does not read a microphone at all: it fakes a blow with a held button instead."
            )

            Toggle(isOn: $peripherals.cameraAllowed) {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Allow camera in games")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(.white)
                    Text(peripherals.cameraAllowed
                         ? "A game that starts the camera sees the iPhone's."
                         : "Games see no camera.")
                        .font(.system(size: 12))
                        .foregroundStyle(ShellPalette.secondaryText)
                }
            }
            .tint(ShellPalette.accent)

            SettingsLabel("Camera")
            SegmentedChoice(options: Peripherals.CameraSide.allCases,
                            title: { $0.title },
                            selection: $peripherals.cameraSide)

            SettingsNote(
                "No core in this build asks for a camera yet: Azahar's libretro version does not "
                + "request one. The camera is wired so a core that does gets it straight away."
            )

            SettingsReadout(label: "Now", value: peripherals.statusLine)

            SettingsLabel("Amiibo")
            SettingsReadout(
                label: "Amiibo folder",
                value: peripherals.amiiboFiles.isEmpty
                    ? "empty (Files app, Continuum, Amiibo)"
                    : peripherals.amiiboFiles.map { $0.lastPathComponent }
                        .joined(separator: ", ")
            )
            SettingsButton(title: "Import Amiibo files", role: .normal) {
                peripherals.importAmiibo { line in report(line) }
            }
            SettingsNote(
                "Amiibo dumps are .bin files of 540 or 572 bytes. Each file is checked and copied "
                + "into the Amiibo folder under its own name. While a 3DS game runs, the ... menu "
                + "lists them to tap. Azahar's libretro build cannot receive an Amiibo yet, so a "
                + "tap says that instead of reaching the game."
            )
        }
        .onAppear { peripherals.refreshAmiibo() }
    }
}

// MARK: - The in-game Amiibo picker

/// The Amiibo rows in the player's ... menu, shown for the 3DS only.
struct AmiiboMenuSection: View {
    @ObservedObject var peripherals: Peripherals
    let report: (String) -> Void

    var body: some View {
        Section("Amiibo") {
            if peripherals.amiiboFiles.isEmpty {
                Button("No Amiibo files yet") {}
                    .disabled(true)
            } else {
                ForEach(peripherals.amiiboFiles, id: \.self) { url in
                    Button {
                        report(peripherals.tap(url))
                    } label: {
                        Label("Tap \(url.deletingPathExtension().lastPathComponent)",
                              systemImage: "wave.3.right")
                    }
                }
            }
            Button {
                peripherals.importAmiibo { line in report(line) }
            } label: {
                Label("Import Amiibo files", systemImage: "square.and.arrow.down.on.square")
            }
        }
    }
}
