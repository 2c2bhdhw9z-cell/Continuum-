// Continuum player check: the pure-Foundation half of the bundled players (WebPlayerCore.swift).
import Foundation
var failures = 0
func expect(_ ok: Bool, _ what: String) { if !ok { failures += 1; print("FAIL: \(what)") } }
let out = CommandLine.arguments[1]

expect(WebPlayerKind(engineId: "ruffle") == .flash && WebPlayerKind(engineId: "j2mejs") == .j2me, "engine ids")
expect(WebPlayerKind.flash.saveFileName(forGame: "Bloons TD.swf") == "Bloons TD.json", "flash save name")
expect(WebPlayerKind.j2me.saveFileName(forGame: "Game.jar") == "Game.J2meJS.srm", "j2me save name")
let can: Set<SkinFunction> = [.flex, .quit, .restart, .screenshot, .volume, .toggleControlls, .skins,
                              .haptics, .orientation, .functionLayout, .gameplayManuals]
for kind in WebPlayerKind.allCases {
    for f in SkinFunction.allCases {
        let r = kind.refusal(for: f)
        let allowed = can.contains(f) || (f == .j2meSettings && kind == .j2me)
        expect((r == nil) == allowed, "\(kind) \(f) support")
        if let r { expect(!r.contains("\u{2014}") && r.contains(kind.gameKind) || r.hasPrefix("J2ME settings"), "\(kind) \(f) line") }
    }
}
typealias A = WebPlayerAddress
expect(A.classify(host: "localhost", path: "/index.html", kind: .flash) == .page, "flash page")
expect(A.classify(host: "localhost", path: "/ruffle/ruffle.js", kind: .flash) == .bundle("ruffle.js"), "ruffle file")
expect(A.classify(host: "localhost", path: "/ruffle/../x", kind: .flash) != .bundle("../x"), "dotdot")
expect(A.classify(host: "localhost", path: "/etc/passwd", kind: .flash) != .bundle("etc/passwd"), "flash only ruffle")
expect(A.classify(host: "evil.com", path: "/index.html", kind: .flash) != .page, "host")
expect(A.classify(host: "j2me", path: "/main.html", kind: .j2me) == .page, "j2me page")
expect(A.classify(host: "j2me", path: "/bld/main-all.js", kind: .j2me) == .bundle("bld/main-all.js"), "j2me file")
expect(A.classify(host: "j2me", path: "/a/../../b", kind: .j2me) != .bundle("a/../../b"), "j2me dotdot")
expect(A.classify(host: "j2me", path: "/__continuum__/game.jar", kind: .j2me) == .game, "jar")
expect(A.classify(host: "j2me", path: "/__continuum__/game.swf", kind: .j2me) != .game, "no swf on j2me")
expect(A.classify(host: "j2me", path: "/__continuum__/seed.js", kind: .j2me) == .seed, "seed")
expect(A.mimeType(for: "x.wasm") == "application/wasm", "wasm mime")
expect(!A.contentSecurityPolicy.contains("http"), "csp has no http")
let rules = try! JSONSerialization.jsonObject(with: Data(A.contentRuleList.utf8)) as! [[String: Any]]
expect(rules.count == 5, "rule list parses")
expect(A.allowsNavigation(to: URL(string: "continuum-player://j2me/main.html"), kind: .j2me), "nav ok")
expect(!A.allowsNavigation(to: URL(string: "https://example.com"), kind: .j2me), "nav blocked")
let page = A.pageURL(kind: .j2me, midletClass: "a.B", width: 176, height: 208)!.absoluteString
expect(page.contains("main.html") && page.contains("midletClassName=a.B") && page.contains("size-176x208"), "j2me url \(page)")
let html = "<html><head>\n" + WebPlayerPages.j2meViewportMarker + "\n<script src=\"x.js\" defer></script></head></html>"
let rw = WebPlayerPages.rewriteJ2MEPage(html, width: 240, height: 320)!
expect(rw.contains("width=240, user-scalable=no"), "viewport")
let seedAt = rw.range(of: "seed.js")!.lowerBound, bridgeAt = rw.range(of: "bridge.js")!.lowerBound, xAt = rw.range(of: "x.js")!.lowerBound
expect(seedAt < bridgeAt && bridgeAt < xAt, "seed, bridge, then the engine")
expect(WebPlayerPages.rewriteJ2MEPage("<head></head>", width: 1, height: 1) == nil, "unknown page refused")
let fp = WebPlayerPages.flashPage
expect(fp.range(of: "bridge.js")!.lowerBound < fp.range(of: "ruffle.js")!.lowerBound, "flash order")
expect(WebPlayerScripts.jsString("a\"b\n\u{2028}") == "\"a\\\"b\\n\u{2028}\"", "jsString \(WebPlayerScripts.jsString("a\"b\n\u{2028}"))")
let seed = WebPlayerScripts.j2meSeed(records: [.init(pathname: "/", isDir: true, parentDir: nil, mtime: 1, data: Data()),
                                               .init(pathname: "/a", isDir: false, parentDir: "/", mtime: 2, data: Data([1, 2]))])
expect(seed.hasPrefix("window.__continuumSeed = {") && seed.contains("\"AQI=\""), "j2me seed")
expect(WebPlayerEvent(message: ["type": "ready"]) == .ready, "ready")
expect(WebPlayerEvent(message: ["type": "flashSave", "items": [["k", "v"], ["n", NSNull()]]]) == .flashSave([("k", "v")]), "flash save event")
expect(WebPlayerEvent(message: ["type": "j2meSave", "files": [["path": "/a", "mtime": 3, "data": "AQI="]]])
       == .j2meSave([WebPlayerFile(path: "/a", mtime: 3, data: Data([1, 2]))]), "j2me save event")
expect(WebPlayerEvent(message: ["type": "j2meSave", "files": [["path": 1]]]) == nil, "malformed dropped")
expect(WebPlayerEvent(message: "x") == nil, "not a dict")
let d = UserDefaults(suiteName: "player-check")!
var s = WebPlayerGameSettings(); s.phoneType = "standard"
s.save(kind: .j2me, gameName: "G.jar", to: d)
expect(WebPlayerGameSettings.load(kind: .j2me, gameName: "G.jar", from: d) == s, "settings round trip")
WebPlayerGameSettings().save(kind: .j2me, gameName: "G.jar", to: d)
expect(d.object(forKey: WebPlayerGameSettings.storageKey(kind: .j2me, gameName: "G.jar")) == nil, "defaults stored as nothing")
let held = WebPlayerKeys.held(slots: [0, 1, 5], table: [0: "Space", 1: "Space", 5: "none"])
expect(held == ["Space"], "two buttons, one key")
let ch = WebPlayerKeys.changes(from: ["A", "B"], to: ["B", "C"])
expect(ch.released == ["A"] && ch.pressed == ["C"], "changes")
expect(WebPlayerKeys.slots(from: [true, false, true]) == [0, 2], "slots")
expect(WebPlayerScripts.j2meBridge.contains("done(null); }, \(WebPlayerTimeouts.j2meExportMilliseconds));"), "j2me export wait in the bridge")
expect(WebPlayerTimeouts.finishSeconds >= 2 * Double(WebPlayerTimeouts.j2meExportMilliseconds) / 1000, "the app waits clearly longer than the page")
try! WebPlayerScripts.flashBridge.write(toFile: out + "/flash-bridge.js", atomically: true, encoding: .utf8)
try! WebPlayerScripts.j2meBridge.write(toFile: out + "/j2me-bridge.js", atomically: true, encoding: .utf8)
print("player checks: \(failures) failed")
exit(failures == 0 ? 0 : 1)
