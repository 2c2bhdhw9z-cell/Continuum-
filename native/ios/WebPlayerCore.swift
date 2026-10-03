// Continuum - the bundled players' pure logic: Flash (Ruffle) and J2ME (J2meJS).
//
// docs/MANIC_PARITY.md decided these run the way Manic EMU runs them: in a player view inside the
// .ipa, a WKWebView that loads LOCAL FILES ONLY. That is a player inside the app, not a web build
// of Continuum. WebPlayers.swift is the UIKit half (the web view, its scheme handler, the input
// poll, the settings sheet); this file is everything that can be checked off a Mac with real
// swiftc (scripts/check-players.sh):
//
//   * which player a system opens in, and what each is called;
//   * which skin function buttons a player can do, and the plain line for the ones it cannot;
//   * the local address space the web view is allowed (`continuum-player://`), how each request
//     path maps onto a file, and what is refused;
//   * the J2ME page rewrite (viewport, the injected bridge) and the Flash page, both written here;
//   * the bridge scripts the pages run, which seed storage BEFORE the engine starts and hand the
//     saves back;
//   * per-game settings, held keys, and decoding what the page posts.
//
// Key tables and the save formats are Rust (crates/emulator-bridge/src/players), because Android
// will want them too.

import Foundation

// MARK: - The two players

enum WebPlayerKind: String, CaseIterable, Sendable {
    case flash
    case j2me

    /// The id CoreCatalog's routing table carries in a route's core id slot. NOT a libretro core
    /// id: `CoreCatalog.byId` does not know it, which is what keeps these off the libretro path.
    var engineId: String {
        switch self {
        case .flash: return "ruffle"
        case .j2me: return "j2mejs"
        }
    }

    init?(engineId: String) {
        guard let kind = Self.allCases.first(where: { $0.engineId == engineId }) else { return nil }
        self = kind
    }

    /// The shared system id, which is also the raw value.
    var systemId: String { rawValue }

    /// "Flash games", "J2ME games".
    var gameKind: String {
        switch self {
        case .flash: return "Flash"
        case .j2me: return "J2ME"
        }
    }

    /// The player, with the version scripts/fetch-players.sh pins.
    var playerName: String {
        switch self {
        case .flash: return "Ruffle 0.6.0"
        case .j2me: return "J2meJS"
        }
    }

    /// The folder under the bundle's support/players/.
    var bundleFolder: String {
        switch self {
        case .flash: return "ruffle"
        case .j2me: return "j2me"
        }
    }

    /// A file that must be in that folder for the player to be in this build.
    var bundleProbe: String {
        switch self {
        case .flash: return "ruffle.js"
        case .j2me: return "bld/main-all.js"
        }
    }

    /// The host of the page's address. Flash's is `localhost` on purpose: Ruffle keys every
    /// SharedObject by the movie's host, and Manic's page is on localhost, so a save moves
    /// between the two apps unchanged (see players/flash_save.rs).
    var host: String {
        switch self {
        case .flash: return "localhost"
        case .j2me: return "j2me"
        }
    }

    /// The save file's extension, Manic's own: `<game>.json` and `<game>.J2meJS.srm`.
    var saveExtension: String {
        switch self {
        case .flash: return "json"
        case .j2me: return "J2meJS.srm"
        }
    }

    /// The save file's name for a game file name ("Bloons.swf" -> "Bloons.json").
    func saveFileName(forGame name: String) -> String {
        let stem = (name as NSString).deletingPathExtension
        return "\(stem.isEmpty ? name : stem).\(saveExtension)"
    }

    /// Why the save-state buttons do nothing here, said plainly.
    var saveStatesUnavailable: String {
        switch self {
        case .flash:
            return "save states are not available for Flash games: Ruffle cannot freeze a running "
                + "movie. The game's own saves are kept automatically"
        case .j2me:
            return "save states are not available for J2ME games: the J2ME engine cannot freeze a "
                + "running phone. The game's own saves are kept automatically"
        }
    }

    /// The line for a skin function this player cannot do, or nil when it can.
    ///
    /// CAN: quit, restart, screenshot, volume, hide the controls, the skin and function sheets,
    /// haptics, the orientation lock and the layout editor, game manuals, and the J2ME settings.
    /// Everything else needs a libretro session (fast forward, rewind, cheats, core options,
    /// discs, palettes...) or is another system's hardware, and says so instead of failing.
    func refusal(for function: SkinFunction) -> String? {
        switch function {
        case .flex, .quit, .restart, .screenshot, .volume, .toggleControlls, .skins, .haptics,
             .orientation, .functionLayout, .gameplayManuals:
            return nil
        case .j2meSettings:
            return self == .j2me ? nil
                : "J2ME settings: this is a Flash game; its keys are under the player menu, Flash settings"
        case .quickSave, .quickLoad, .saveStates:
            return saveStatesUnavailable
        case .coreSettings, .dosSettings:
            return "\(gameKind) games have no core settings: they run in \(playerName), not a "
                + "libretro core. Keys and save files are under the player menu, "
                + "\(gameKind) settings"
        case .fastForward, .toggleFastForward, .fastForward2x, .fastForward3x, .fastForward4x,
             .rewind, .slowMotion:
            return "\(function.title) is not available for \(gameKind) games: \(playerName) runs "
                + "at the game's own speed"
        case .cheatCodes:
            return "cheats are not available for \(gameKind) games: there is no memory to search "
                + "in \(playerName)"
        default:
            return "\(function.title) is not available for \(gameKind) games, which run in "
                + "\(playerName) rather than a libretro core"
        }
    }

    init?(systemId: String) {
        self.init(rawValue: systemId)
    }
}

// MARK: - The local address space

enum WebPlayerAddress {
    /// The only scheme the player's web view may load, served by the app itself from the bundle
    /// and the one game file. Never the network.
    static let scheme = "continuum-player"

    /// The folder of app-made resources (bridge, seed, the game) inside the address space.
    static let reservedFolder = "__continuum__"

    /// The page's address. J2ME's carries the engine's own options as its query, which is how
    /// J2meJS takes them (config/urlparams.js).
    static func pageURL(kind: WebPlayerKind, midletClass: String = "", width: Int = 240,
                        height: Int = 320) -> URL? {
        var parts = URLComponents()
        parts.scheme = scheme
        parts.host = kind.host
        switch kind {
        case .flash:
            parts.path = "/index.html"
        case .j2me:
            // The page MUST be called main.html: J2meJS decides it is the launcher, and does
            // nothing, when "main.html" is not in its address.
            parts.path = "/main.html"
            parts.queryItems = [
                URLQueryItem(name: "jars", value: "\(reservedFolder)/game.jar"),
                URLQueryItem(name: "jad", value: ""),
                // Given up front: with no class the engine waits for a phone-information answer
                // nothing here gives. The engine also re-reads it from the manifest.
                URLQueryItem(name: "midletClassName", value: midletClass),
                URLQueryItem(name: "canvasSize", value: "size-\(width)x\(height)"),
            ]
        }
        return parts.url
    }

    /// What a request is for.
    enum Request: Equatable {
        /// A file of the player, relative to its bundle folder.
        case bundle(String)
        /// The game the user opened.
        case game
        /// The page (generated for Flash, rewritten from main.html for J2ME).
        case page
        /// The bridge script.
        case bridge
        /// The seed script: this game's saves and settings.
        case seed
        /// Anything else, with the reason, which is answered 404 and never read.
        case refused(String)
    }

    /// Maps a request onto what serves it. `host` must be the kind's host and every path segment
    /// a plain name: no `..`, no `.`, no empty segment, no backslash. The path is the URL's
    /// decoded path, so `%2e%2e` arrives here as `..` and is refused like it.
    static func classify(host: String?, path: String, kind: WebPlayerKind) -> Request {
        guard host == kind.host else {
            return .refused("the host \(host ?? "none") is not this player's")
        }
        guard path.hasPrefix("/") else { return .refused("not an absolute path") }
        let body = String(path.dropFirst())
        if body.isEmpty { return kind == .flash ? .page : .refused("no page at /") }
        let segments = body.split(separator: "/", omittingEmptySubsequences: false).map(String.init)
        for segment in segments {
            if segment.isEmpty || segment == "." || segment == ".." || segment.contains("\\")
                || segment.contains("\0") {
                return .refused("the path \(path) is not a plain file path")
            }
        }
        if segments.first == reservedFolder {
            guard segments.count == 2 else { return .refused("no such app file \(path)") }
            switch segments[1] {
            case "bridge.js": return .bridge
            case "seed.js": return .seed
            case "game.swf" where kind == .flash: return .game
            case "game.jar" where kind == .j2me: return .game
            default: return .refused("no such app file \(path)")
            }
        }
        switch kind {
        case .flash:
            if segments == ["index.html"] { return .page }
            // Only Ruffle's own folder, one level: ruffle.js, its chunks and its two .wasm.
            if segments.count == 2, segments[0] == "ruffle" { return .bundle(segments[1]) }
            return .refused("the Flash page has no file \(path)")
        case .j2me:
            if segments == ["main.html"] { return .page }
            return .bundle(segments.joined(separator: "/"))
        }
    }

    /// The Content-Type for a file. `application/wasm` matters: WebKit only streams a module
    /// compile for that type.
    static func mimeType(for path: String) -> String {
        switch (path as NSString).pathExtension.lowercased() {
        case "html", "htm": return "text/html; charset=utf-8"
        case "js": return "text/javascript; charset=utf-8"
        case "wasm": return "application/wasm"
        case "css": return "text/css; charset=utf-8"
        case "json": return "application/json"
        case "png": return "image/png"
        case "gif": return "image/gif"
        case "jpg", "jpeg": return "image/jpeg"
        case "jar": return "application/java-archive"
        case "swf": return "application/x-shockwave-flash"
        case "txt": return "text/plain; charset=utf-8"
        default: return "application/octet-stream"
        }
    }

    /// The Content-Security-Policy every page is served with. Only this app's own scheme, and the
    /// blob: and data: URLs the engines build in memory; no http, https, ws or anything else. The
    /// eval allowances are what the engines need: Ruffle compiles WebAssembly, and J2meJS compiles
    /// Java methods to JavaScript at run time. None of this is the app's process; it is WebKit's.
    static let contentSecurityPolicy =
        "default-src \(scheme): blob: data: 'unsafe-inline' 'unsafe-eval' 'wasm-unsafe-eval'; "
        + "connect-src \(scheme): blob: data:; "
        + "img-src \(scheme): blob: data:; "
        + "media-src \(scheme): blob: data:; "
        + "worker-src \(scheme): blob:; "
        + "frame-src 'none'; object-src 'none'; form-action 'none'; base-uri 'none'"

    /// The second fence, a WebKit content rule list: block every load, then let this scheme and
    /// in-memory URLs through. It covers fetch, XHR and WebSocket, which navigation policy never
    /// sees.
    static let contentRuleList = """
    [{"trigger":{"url-filter":".*"},"action":{"type":"block"}},\
    {"trigger":{"url-filter":"^\(scheme):"},"action":{"type":"ignore-previous-rules"}},\
    {"trigger":{"url-filter":"^blob:"},"action":{"type":"ignore-previous-rules"}},\
    {"trigger":{"url-filter":"^data:"},"action":{"type":"ignore-previous-rules"}},\
    {"trigger":{"url-filter":"^about:"},"action":{"type":"ignore-previous-rules"}}]
    """

    /// Whether a navigation may happen: only this player's own pages.
    static func allowsNavigation(to url: URL?, kind: WebPlayerKind) -> Bool {
        guard let url else { return false }
        if url.absoluteString == "about:blank" { return true }
        return url.scheme?.lowercased() == scheme && url.host == kind.host
    }
}

// MARK: - The pages

enum WebPlayerPages {
    /// The tags put at the top of J2meJS's main.html: our CSP, the seed and the bridge (which must
    /// run before the engine's deferred scripts open storage), and the style that makes the phone
    /// screen fill the view.
    static func j2meHeadInjection(width: Int, height: Int) -> String {
        """
        <meta http-equiv="Content-Security-Policy" content="\(WebPlayerAddress.contentSecurityPolicy)">
        <script src="/\(WebPlayerAddress.reservedFolder)/seed.js"></script>
        <script src="/\(WebPlayerAddress.reservedFolder)/bridge.js"></script>
        <style>
        html, body { margin: 0 !important; padding: 0 !important; background: #000 !important;
          overflow: hidden !important; width: \(width)px !important; height: \(height)px !important; }
        #display-container, #display, #drawer, #main { width: \(width)px !important;
          height: \(height)px !important; margin: 0 !important; padding: 0 !important; }
        #canvas { position: absolute !important; left: 0 !important; top: 0 !important; }
        #gamepad, #sidebar, #drawer > header, #back-button { display: none !important; }
        </style>
        """
    }

    /// The marker the rewrite looks for, from J2meJS main.html at the pinned commit.
    static let j2meViewportMarker =
        #"<meta name="viewport" content="width=device-width, user-scalable=no, initial-scale=1">"#

    /// J2meJS's main.html with the viewport set to the phone screen's width, so WebKit scales the
    /// screen to fill the view and touches arrive in phone pixels, and the injection above. Nil
    /// when the page is not the one this was written against, which the player reports rather
    /// than running a page it does not understand.
    static func rewriteJ2MEPage(_ html: String, width: Int, height: Int) -> String? {
        guard html.contains(j2meViewportMarker), let head = html.range(of: "<head>") else {
            return nil
        }
        var out = html
        out.replaceSubrange(head, with: "<head>\n" + j2meHeadInjection(width: width, height: height))
        out = out.replacingOccurrences(
            of: j2meViewportMarker,
            with: #"<meta name="viewport" content="width=\#(width), user-scalable=no">"#)
        return out
    }

    /// The Flash page, written here: Ruffle filling the view, nothing else.
    static let flashPage = """
    <!doctype html>
    <html>
    <head>
    <meta charset="utf-8">
    <meta http-equiv="Content-Security-Policy" content="\(WebPlayerAddress.contentSecurityPolicy)">
    <meta name="viewport" content="width=device-width, initial-scale=1, user-scalable=no">
    <style>
    html, body { margin: 0; padding: 0; width: 100%; height: 100%; background: #000;
      overflow: hidden; -webkit-user-select: none; -webkit-touch-callout: none; }
    #stage { position: absolute; inset: 0; }
    #stage > * { width: 100%; height: 100%; display: block; }
    </style>
    <script src="/\(WebPlayerAddress.reservedFolder)/seed.js"></script>
    <script src="/\(WebPlayerAddress.reservedFolder)/bridge.js"></script>
    <script src="/ruffle/ruffle.js"></script>
    </head>
    <body><div id="stage"></div>
    <script>window.continuumPlayer && window.continuumPlayer.start();</script>
    </body>
    </html>
    """
}

// MARK: - The bridge scripts

enum WebPlayerScripts {
    /// The message handler name both bridges post to.
    static let handlerName = "continuum"

    /// The Flash bridge. Seeds localStorage from the save BEFORE Ruffle exists, starts Ruffle with
    /// networking off, sends keys, and hands localStorage back whenever it changes and on finish.
    static let flashBridge = #"""
    (function () {
      'use strict';
      function post(message) {
        try { window.webkit.messageHandlers.continuum.postMessage(message); } catch (e) {}
      }
      var seed = window.__continuumSeed || { items: [], swfName: 'movie.swf' };
      try {
        localStorage.clear();
        (seed.items || []).forEach(function (kv) { localStorage.setItem(kv[0], kv[1]); });
      } catch (e) {
        post({ type: 'failed', message: 'the player page could not use its storage: ' + e });
      }
      function dump() {
        var out = [];
        try {
          for (var i = 0; i < localStorage.length; i++) {
            var k = localStorage.key(i);
            out.push([k, localStorage.getItem(k)]);
          }
        } catch (e) {}
        out.sort(function (a, b) { return a[0] < b[0] ? -1 : (a[0] > b[0] ? 1 : 0); });
        return out;
      }
      var last = JSON.stringify(dump());
      function pushIfChanged() {
        var now = dump();
        var text = JSON.stringify(now);
        if (text !== last) { last = text; post({ type: 'flashSave', items: now }); }
      }
      window.addEventListener('error', function (e) {
        post({ type: 'log', message: String(e.message || e) });
      });
      window.RufflePlayer = window.RufflePlayer || {};
      window.RufflePlayer.config = {
        publicPath: '/ruffle/', polyfills: false, autoplay: 'on', unmuteOverlay: 'hidden',
        splashScreen: false, contextMenu: 'off', letterbox: 'on', allowNetworking: 'none',
        openUrlMode: 'deny', upgradeToHttps: false, warnOnUnsupportedContent: false,
        showSwfDownload: false, allowScriptAccess: false, backgroundColor: '#000000',
        logLevel: 'warn', favorFlash: false, allowFullscreen: false
      };
      var element = null;
      var api = null;
      var paused = false;
      var volume = 1;
      function focusPlayer() {
        if (element && document.activeElement !== element) {
          try { element.focus({ preventScroll: true }); } catch (e) {}
        }
      }
      window.continuumPlayer = {
        start: function () {
          if (!window.RufflePlayer || typeof window.RufflePlayer.newest !== 'function') {
            post({ type: 'failed', message: 'Ruffle did not load (ruffle.js is missing from this build)' });
            return;
          }
          var ruffle = window.RufflePlayer.newest();
          if (!ruffle) { post({ type: 'failed', message: 'Ruffle did not start' }); return; }
          element = ruffle.createPlayer();
          element.tabIndex = 0;
          document.getElementById('stage').appendChild(element);
          api = element.ruffle();
          element.addEventListener('loadedmetadata', function () {
            var m = api.metadata || {};
            post({ type: 'metadata', width: m.width || 0, height: m.height || 0,
                   frameRate: m.frameRate || 0, swfVersion: m.swfVersion || 0 });
          });
          element.addEventListener('pointerdown', focusPlayer);
          fetch('/__continuum__/game.swf').then(function (r) {
            if (!r.ok) { throw new Error('the game file could not be read (' + r.status + ')'); }
            return r.arrayBuffer();
          }).then(function (data) {
            return api.load({ data: data, swfFileName: seed.swfName || 'movie.swf' });
          }).then(function () {
            focusPlayer();
            api.volume = volume;
            post({ type: 'ready' });
          }).catch(function (e) {
            post({ type: 'failed', message: String(e && e.message || e) });
          });
          setInterval(pushIfChanged, 2000);
        },
        key: function (code, key, keyCode, down) {
          if (!element || paused) { return; }
          focusPlayer();
          var event = new KeyboardEvent(down ? 'keydown' : 'keyup',
            { code: code, key: key, bubbles: true, cancelable: true });
          try {
            Object.defineProperty(event, 'keyCode', { get: function () { return keyCode; } });
            Object.defineProperty(event, 'which', { get: function () { return keyCode; } });
          } catch (e) {}
          // Once, on the player; it bubbles to the window, where Ruffle listens.
          element.dispatchEvent(event);
        },
        pause: function () { paused = true; if (api) { api.pause(); } },
        resume: function () { paused = false; if (api) { api.play(); focusPlayer(); } },
        setVolume: function (v) { volume = v; if (api) { api.volume = v; } },
        flushNow: function () {
          // Ruffle writes its SharedObjects out on pagehide, the same as a tab closing.
          try { window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: false })); } catch (e) {}
          var items = dump();
          last = JSON.stringify(items);
          return { items: items };
        },
        finish: function () {
          // Removing the player destroys it, and Ruffle flushes every SharedObject as it goes.
          try { if (element && element.parentNode) { element.parentNode.removeChild(element); } } catch (e) {}
          element = null;
          api = null;
          var items = dump();
          last = JSON.stringify(items);
          return Promise.resolve({ items: items });
        }
      };
    })();
    """#

    /// The J2ME bridge. Runs before J2meJS's deferred scripts: seeds the engine's IndexedDB file
    /// system from the save (the engine's open waits behind this one, per the IndexedDB spec's
    /// open queue), tracks audio contexts so pause and mute can reach them, sends keys, and hands
    /// the phone's files back whenever they change and on finish.
    static let j2meBridge = #"""
    (function () {
      'use strict';
      function post(message) {
        try { window.webkit.messageHandlers.continuum.postMessage(message); } catch (e) {}
      }
      var seed = window.__continuumSeed || { records: [] };
      function fromBase64(text) {
        var raw = atob(text || '');
        var bytes = new Uint8Array(raw.length);
        for (var i = 0; i < raw.length; i++) { bytes[i] = raw.charCodeAt(i); }
        return bytes;
      }
      function toBase64(bytes) {
        var parts = [];
        for (var i = 0; i < bytes.length; i += 0x8000) {
          parts.push(String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000)));
        }
        return btoa(parts.join(''));
      }
      // 1. The phone's storage, exactly as J2meJS's Store.init would create it, holding the save.
      try {
        var open = indexedDB.open('asyncStorage', 4);
        open.onupgradeneeded = function () {
          var store = open.result.createObjectStore('fs4', { keyPath: 'pathname' });
          store.createIndex('parentDir', 'parentDir', { unique: false });
          (seed.records || []).forEach(function (r) {
            var record = { pathname: r.pathname, isDir: !!r.isDir, mtime: r.mtime || 0,
                           parentDir: r.parentDir === undefined ? null : r.parentDir };
            if (!r.isDir) {
              var bytes = fromBase64(r.data);
              record.data = new Blob([bytes]);
              record.size = bytes.length;
            }
            store.put(record);
          });
        };
        open.onsuccess = function () { try { open.result.close(); } catch (e) {} };
        open.onerror = function () {
          post({ type: 'failed', message: 'the phone storage could not be opened: ' + open.error });
        };
      } catch (e) {
        post({ type: 'failed', message: 'the phone storage could not be opened: ' + e });
      }
      // 2. Audio contexts, so pause and mute reach the game's sound.
      var contexts = [];
      ['AudioContext', 'webkitAudioContext'].forEach(function (name) {
        var Original = window[name];
        if (!Original) { return; }
        var Tracked = function () {
          var ctx = new (Function.prototype.bind.apply(Original, [null].concat([].slice.call(arguments))))();
          contexts.push(ctx);
          return ctx;
        };
        Tracked.prototype = Original.prototype;
        window[name] = Tracked;
      });
      var paused = false;
      var muted = false;
      function applySound() {
        contexts.forEach(function (ctx) {
          try { (paused || muted) ? ctx.suspend() : ctx.resume(); } catch (e) {}
        });
        Array.prototype.forEach.call(document.querySelectorAll('audio, video'), function (m) {
          m.muted = paused || muted;
        });
      }
      // 3. Pause: the engine's scheduler stops taking work; it is handed what queued on resume.
      var originalRun = null;
      function hookScheduler() {
        if (originalRun || typeof J2ME === 'undefined' || !J2ME.Scheduler) { return !!originalRun; }
        originalRun = J2ME.Scheduler.processRunningQueue;
        J2ME.Scheduler.processRunningQueue = function () {
          if (paused) { return; }
          return originalRun.apply(J2ME.Scheduler, arguments);
        };
        return true;
      }
      // 4. The phone's files, read back through the engine's own export.
      function exportFiles() {
        return new Promise(function (resolve) {
          var settled = false;
          function done(value) { if (!settled) { settled = true; resolve(value); } }
          setTimeout(function () { done(null); }, 3000);
          try {
            if (typeof fs === 'undefined' || typeof fs.exportStore !== 'function') { done(null); return; }
            try { if (typeof myflushAll === 'function') { myflushAll(); } } catch (e) {}
            fs.exportStore(function (blob) {
              var reader = new FileReader();
              reader.onload = function () {
                try {
                  var all = JSON.parse(reader.result);
                  var files = [];
                  Object.keys(all).forEach(function (path) {
                    var r = all[path];
                    if (!r || r.isDir) { return; }
                    var source = r.data || [];
                    var bytes = new Uint8Array(source.length);
                    for (var i = 0; i < source.length; i++) { bytes[i] = source[i] & 255; }
                    files.push({ path: path, mtime: r.mtime || 0, data: toBase64(bytes) });
                  });
                  files.sort(function (a, b) { return a.path < b.path ? -1 : (a.path > b.path ? 1 : 0); });
                  done(files);
                } catch (e) { done(null); }
              };
              reader.onerror = function () { done(null); };
              reader.readAsText(blob);
            });
          } catch (e) { done(null); }
        });
      }
      var last = null;
      var started = false;
      function pushIfChanged() {
        if (!started) { return; }
        exportFiles().then(function (files) {
          if (!files) { return; }
          var text = JSON.stringify(files);
          if (last === null) { last = text; return; }
          if (text !== last) { last = text; post({ type: 'j2meSave', files: files }); }
        });
      }
      var readyTimer = setInterval(function () {
        if (typeof isLoadJarFinished !== 'undefined' && isLoadJarFinished && typeof J2ME !== 'undefined') {
          clearInterval(readyTimer);
          started = true;
          hookScheduler();
          post({ type: 'ready' });
          setInterval(pushIfChanged, 3000);
        }
      }, 250);
      window.addEventListener('error', function (e) {
        post({ type: 'log', message: String(e.message || e) });
      });
      window.continuumPlayer = {
        key: function (code, down) {
          if (paused || typeof MIDP === 'undefined') { return; }
          try { down ? MIDP.sendKeyPress(code) : MIDP.sendKeyRelease(code); } catch (e) {}
        },
        pause: function () {
          paused = true;
          hookScheduler();
          applySound();
        },
        resume: function () {
          paused = false;
          applySound();
          if (originalRun) {
            try { originalRun.call(J2ME.Scheduler, false); } catch (e) {}
          }
        },
        setVolume: function (v) { muted = v <= 0; applySound(); },
        flushNow: function () {
          return exportFiles().then(function (files) {
            if (files) { last = JSON.stringify(files); }
            return { files: files };
          });
        },
        finish: function () {
          return exportFiles().then(function (files) {
            paused = true;
            applySound();
            return { files: files };
          });
        }
      };
    })();
    """#

    /// A JavaScript string literal for any text (JSON's escaping is valid JavaScript).
    static func jsString(_ text: String) -> String {
        guard let data = try? JSONSerialization.data(withJSONObject: [text]),
              let array = String(data: data, encoding: .utf8) else { return "\"\"" }
        // ["..."] -> "..."
        return String(array.dropFirst().dropLast())
    }

    static func flashKey(code: String, key: String, keyCode: UInt32, down: Bool) -> String {
        "window.continuumPlayer && window.continuumPlayer.key(\(jsString(code)), "
            + "\(jsString(key)), \(keyCode), \(down));"
    }

    static func j2meKey(code: Int32, down: Bool) -> String {
        "window.continuumPlayer && window.continuumPlayer.key(\(code), \(down));"
    }

    /// The seed script for Flash: the save's storage items and the movie's file name.
    static func flashSeed(items: [(String, String)], swfName: String) -> String {
        let object: [String: Any] = [
            "kind": "flash",
            "swfName": swfName,
            "items": items.map { [$0.0, $0.1] },
        ]
        return seedScript(object)
    }

    /// One record for the J2ME seed.
    struct SeedRecord: Equatable {
        var pathname: String
        var isDir: Bool
        var parentDir: String?
        var mtime: Double
        var data: Data
    }

    /// The seed script for J2ME: every record of the save, folders first.
    static func j2meSeed(records: [SeedRecord]) -> String {
        let list: [[String: Any]] = records.map { record in
            [
                "pathname": record.pathname,
                "isDir": record.isDir,
                "parentDir": record.parentDir.map { $0 as Any } ?? NSNull(),
                "mtime": record.mtime,
                "data": record.data.base64EncodedString(),
            ]
        }
        return seedScript(["kind": "j2me", "records": list])
    }

    private static func seedScript(_ object: [String: Any]) -> String {
        guard let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]),
              let json = String(data: data, encoding: .utf8) else {
            return "window.__continuumSeed = null;"
        }
        return "window.__continuumSeed = \(json);"
    }
}

// MARK: - What the page says

enum WebPlayerEvent: Equatable {
    case ready
    /// Flash only: the movie's stage size.
    case metadata(width: Double, height: Double)
    case flashSave([(String, String)])
    case j2meSave([WebPlayerFile])
    case failed(String)
    case log(String)

    static func == (lhs: WebPlayerEvent, rhs: WebPlayerEvent) -> Bool {
        switch (lhs, rhs) {
        case (.ready, .ready): return true
        case let (.metadata(a, b), .metadata(c, d)): return a == c && b == d
        case let (.flashSave(a), .flashSave(b)):
            return a.count == b.count && zip(a, b).allSatisfy { $0.0 == $1.0 && $0.1 == $1.1 }
        case let (.j2meSave(a), .j2meSave(b)): return a == b
        case let (.failed(a), .failed(b)), let (.log(a), .log(b)): return a == b
        default: return false
        }
    }

    /// Decodes a posted message. Nil for anything malformed, which is dropped.
    init?(message body: Any) {
        guard let dict = body as? [String: Any], let type = dict["type"] as? String else { return nil }
        switch type {
        case "ready":
            self = .ready
        case "metadata":
            let width = (dict["width"] as? NSNumber)?.doubleValue ?? 0
            let height = (dict["height"] as? NSNumber)?.doubleValue ?? 0
            self = .metadata(width: width, height: height)
        case "flashSave":
            guard let items = WebPlayerEvent.flashItems(dict["items"]) else { return nil }
            self = .flashSave(items)
        case "j2meSave":
            guard let files = WebPlayerEvent.j2meFiles(dict["files"]) else { return nil }
            self = .j2meSave(files)
        case "failed":
            self = .failed((dict["message"] as? String) ?? "the player stopped with no reason given")
        case "log":
            self = .log((dict["message"] as? String) ?? "")
        default:
            return nil
        }
    }

    /// `[[key, value], ...]` as pairs. Nil when it is not that shape.
    static func flashItems(_ value: Any?) -> [(String, String)]? {
        guard let list = value as? [Any] else { return nil }
        var out: [(String, String)] = []
        for item in list {
            guard let pair = item as? [Any], pair.count == 2, let key = pair[0] as? String else {
                return nil
            }
            // A null value is an item the movie removed.
            guard let text = pair[1] as? String else { continue }
            out.append((key, text))
        }
        return out
    }

    /// `[{path, mtime, data(base64)}, ...]` as files. Nil when it is not that shape.
    static func j2meFiles(_ value: Any?) -> [WebPlayerFile]? {
        guard let list = value as? [Any] else { return nil }
        var out: [WebPlayerFile] = []
        for item in list {
            guard let dict = item as? [String: Any], let path = dict["path"] as? String,
                  let text = dict["data"] as? String, let data = Data(base64Encoded: text) else {
                return nil
            }
            let mtime = (dict["mtime"] as? NSNumber)?.doubleValue ?? 0
            out.append(WebPlayerFile(path: path, mtime: mtime, data: data))
        }
        return out
    }
}

/// One file of a J2ME phone, as the page hands it back.
struct WebPlayerFile: Equatable {
    var path: String
    var mtime: Double
    var data: Data
}

// MARK: - Per-game settings

/// What a game remembers about how it plays: its key remap, and for J2ME the phone type and
/// screen size. Stored per game (by file name, like the per-game skin) in UserDefaults.
struct WebPlayerGameSettings: Codable, Equatable {
    /// The remap, as differences from the defaults (`slot=token;...`, see players/keys.rs).
    var keyOverrides: String = ""
    /// "nokia" or "standard" (J2ME only).
    var phoneType: String = "nokia"
    /// "240x320", or empty for the size the jar declares, else 240x320 (J2ME only).
    var screenSize: String = ""

    static func storageKey(kind: WebPlayerKind, gameName: String) -> String {
        "continuum.player.\(kind.rawValue).\(gameName)"
    }

    static func load(kind: WebPlayerKind, gameName: String,
                     from defaults: UserDefaults = .standard) -> WebPlayerGameSettings {
        guard let data = defaults.data(forKey: storageKey(kind: kind, gameName: gameName)),
              let settings = try? JSONDecoder().decode(WebPlayerGameSettings.self, from: data) else {
            return WebPlayerGameSettings()
        }
        return settings
    }

    func save(kind: WebPlayerKind, gameName: String, to defaults: UserDefaults = .standard) {
        let key = Self.storageKey(kind: kind, gameName: gameName)
        if self == WebPlayerGameSettings() {
            defaults.removeObject(forKey: key)
        } else if let data = try? JSONEncoder().encode(self) {
            defaults.set(data, forKey: key)
        }
    }
}

// MARK: - Held keys

enum WebPlayerKeys {
    /// The token stored for a button that sends nothing.
    static let unbound = "none"

    /// The keys held, from the pad slots held and the slot-to-key table. Two buttons on one key
    /// hold it once, so letting go of one of them does not release a key the other still holds.
    static func held(slots: Set<Int>, table: [Int: String]) -> Set<String> {
        var keys = Set<String>()
        for slot in slots {
            if let token = table[slot], token != unbound, !token.isEmpty { keys.insert(token) }
        }
        return keys
    }

    /// What changed between two held-key sets: releases first, then presses, each sorted so a
    /// frame's events are always sent in the same order.
    static func changes(from old: Set<String>, to new: Set<String>) -> (released: [String], pressed: [String]) {
        (old.subtracting(new).sorted(), new.subtracting(old).sorted())
    }

    /// The slots held in a W3C-ordered button array.
    static func slots(from buttons: [Bool]) -> Set<Int> {
        var out = Set<Int>()
        for (index, down) in buttons.enumerated() where down { out.insert(index) }
        return out
    }

    /// The pad buttons' names, by slot, for the settings screen.
    static let slotNames: [Int: String] = [
        0: "B (bottom)", 1: "A (right)", 2: "Y (left)", 3: "X (top)", 4: "L", 5: "R",
        6: "L2", 7: "R2", 8: "Select", 9: "Start", 10: "L3", 11: "R3",
        12: "Up", 13: "Down", 14: "Left", 15: "Right",
    ]

    /// The pad buttons' names on the J2ME pad, where the labels are the phone's keys.
    static let j2meSlotNames: [Int: String] = [
        0: "0 key", 1: "OK key", 2: "1 key", 3: "3 key", 4: "* key", 5: "# key",
        6: "7 key", 7: "9 key", 8: "LSK (Select)", 9: "RSK (Start)", 10: "L3", 11: "R3",
        12: "Up", 13: "Down", 14: "Left", 15: "Right",
    ]
}
