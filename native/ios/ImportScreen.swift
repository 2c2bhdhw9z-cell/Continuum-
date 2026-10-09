// The Import screen, opened from the library's + button. Every way in, on one page:
// Files (the device-verified picker, unchanged), Wi-Fi transfer, the clipboard, network servers
// (WebDAV and SMB), and a note for drag and drop, Open in
// and the cloud drives.
//
// Google Drive, Dropbox and OneDrive are NOT logged into directly: a direct login needs an OAuth
// app id that only the owner can register, and a fake one would fail at the first sign-in. Their
// own apps put them in the Files picker under Browse, which is the route this screen points to.

import SwiftUI
import UIKit

struct ImportScreen: View {
    @ObservedObject var host: EngineHost
    @ObservedObject var center: ImportCenter
    @ObservedObject var wifi: WiFiTransferServer
    @ObservedObject var remotes: RemoteSources
    let onDone: () -> Void

    @State private var adding: RemoteServer.Kind?

    init(host: EngineHost, onDone: @escaping () -> Void) {
        self.host = host
        self.center = host.importCenter
        self.wifi = host.importCenter.wifi
        self.remotes = host.importCenter.remotes
        self.onDone = onDone
    }

    var body: some View {
        NavigationView {
            List {
                Section {
                    Button {
                        onDone()
                        // After the sheet is gone, so the picker is presented from the library.
                        Task { @MainActor in
                            try? await Task.sleep(nanoseconds: 400_000_000)
                            host.presentImportPicker()
                        }
                    } label: {
                        Label("Files", systemImage: "folder")
                    }
                    Button {
                        center.importClipboard()
                    } label: {
                        Label("Paste from the clipboard", systemImage: "doc.on.clipboard")
                    }
                } header: {
                    Text("On this phone")
                } footer: {
                    Text("Files also reaches iCloud Drive, Google Drive, Dropbox and OneDrive: install "
                         + "their app and they appear under Browse in the Files picker. The clipboard "
                         + "takes files copied in Files, or on a Mac with Handoff.")
                }

                Section {
                    Toggle(isOn: Binding(get: { wifi.isOn }, set: { wifi.setOn($0) })) {
                        Label("Wi-Fi transfer", systemImage: "wifi")
                    }
                    if !wifi.address.isEmpty {
                        Text(wifi.address)
                            .font(.system(.title3, design: .monospaced))
                            .textSelection(.enabled)
                        Text("Type this whole address, including the last part. It changes each time "
                             + "you turn Wi-Fi transfer on.")
                            .font(.footnote)
                    }
                    Text(wifi.line).font(.footnote).foregroundStyle(.secondary)
                } header: {
                    Text("From a computer")
                } footer: {
                    Text("Type the address into a browser on the same Wi-Fi to send games and saves, "
                         + "see the library, and download battery saves. It stops when you leave the app.")
                }

                Section {
                    ForEach(remotes.servers) { server in
                        NavigationLink {
                            RemoteFolderView(sources: remotes, server: server, path: remotes.rootPath(server))
                        } label: {
                            Label(server.name, systemImage: server.kind == .smb ? "externaldrive.connected.to.line.below" : "network")
                        }
                    }
                    .onDelete { offsets in
                        for index in offsets { remotes.remove(remotes.servers[index]) }
                    }
                    Button("Add a WebDAV server") { adding = .webdav }
                    Button("Add an SMB share (NAS)") { adding = .smb }
                        .disabled(!RemoteSources.smbAvailable)
                    Text(remotes.line).font(.footnote).foregroundStyle(.secondary)
                } header: {
                    Text("Network")
                } footer: {
                    Text("Browse a NAS or router share and pick files, or import a whole folder. WebDAV works the same way.")
                }

                Section {
                    Text("Drag files from another app onto the library, or use Share or Open in from "
                         + "Files, Mail and Safari. Games, .deltaskin and .manicskin skins, .cht cheats, "
                         + "save files and PDF manuals are all recognised.")
                        .font(.footnote)
                    Text(center.line).font(.footnote).foregroundStyle(.secondary)
                } header: {
                    Text("Also")
                }
            }
            .navigationTitle("Import")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button("Done", action: onDone) }
            }
            .sheet(item: $adding) { kind in
                AddRemoteServerView(sources: remotes, kind: kind) { adding = nil }
            }
        }
        .navigationViewStyle(.stack)
        .preferredColorScheme(.dark)
    }
}

extension RemoteServer.Kind: Identifiable {
    var id: String { rawValue }
}
