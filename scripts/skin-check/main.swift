// Check program for the pure-Foundation skin code. Built and run by scripts/check-skins.sh with
// real swiftc on Linux, against the app's own source files (not copies):
//   SkinLibrary.swift, SkinFunctions.swift (top half), ManicSkinItems.swift,
//   SkinFunctionPending.swift (against a stub EngineHost), and DeltaSkinNormalizedRect sliced out
//   of DeltaSkinImport.swift.
// Argument 1 is native/ios, so the dispatcher's switch can be read and compared with the table.

import Foundation

var failures = 0
var checks = 0
func expect(_ condition: @autoclosure () -> Bool, _ what: String, line: Int = #line) {
    checks += 1
    if !condition() {
        failures += 1
        print("FAIL line \(line): \(what)")
    }
}

let iosDir = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "native/ios"

// MARK: 1. Manic and Delta identifiers

let manicTable: [(String, [String])] = [
    ("wsc", ["wswan"]), ("flash", ["flash"]), ("wii", ["wii"]), ("ngc", ["gamecube"]),
    ("amiga", ["amiga"]), ("c64", ["c64"]), ("ngp", ["ngp"]), ("pce", ["tg16", "pcecd"]),
    ("symbian", ["symbian"]), ("dos", ["dos"]), ("j2me", ["j2me"]), ("doom", ["doom"]),
    ("jaguar", ["jaguar"]), ("lynx", ["lynx"]), ("7800", ["atari7800"]), ("5200", ["atari5200"]),
    ("2600", ["atari2600"]), ("arcade", ["arcade"]), ("dc", ["dreamcast"]), ("ps1", ["ps1"]),
    ("pm", ["pokemini"]), ("vb", ["vb"]), ("n64", ["n64"]), ("ss", ["saturn"]), ("md", ["genesis"]),
    ("mcd", ["segacd"]), ("32x", ["sega32x"]), ("ms", ["sms"]), ("gg", ["gg"]),
    ("sg1000", ["sg1000"]), ("psp", ["psp"]), ("3ds", ["n3ds"]), ("ds", ["ds"]), ("gba", ["gba"]),
    ("gbc", ["gbc"]), ("gb", ["gb"]), ("nes", ["nes"]), ("snes", ["snes"]),
]
expect(manicTable.count == 38, "the brief lists 38 Manic identifiers")
for (suffix, ids) in manicTable {
    let got = SkinGameTypes.systemIDs(forGameType: "public.aoshuang.game." + suffix)
    expect(got == ids, "public.aoshuang.game.\(suffix) -> \(ids), got \(got)")
    expect(SkinGameTypes.isManic("public.aoshuang.game." + suffix), "\(suffix) reads as Manic")
}
expect(SkinGameTypes.systemIDs(forGameType: "  PUBLIC.AOSHUANG.GAME.GBA ") == ["gba"],
       "identifiers are trimmed and case-insensitive")
let deltaTable: [(String, String)] = [
    ("com.rileytestut.delta.game.gbc", "gbc"), ("com.rileytestut.delta.game.gba", "gba"),
    ("com.rileytestut.delta.game.ds", "ds"), ("com.rileytestut.delta.game.nes", "nes"),
    ("com.rileytestut.delta.game.snes", "snes"), ("com.rileytestut.delta.game.n64", "n64"),
    ("com.rileytestut.delta.game.genesis", "genesis"),
]
for (id, system) in deltaTable {
    expect(SkinGameTypes.systemIDs(forGameType: id).first == system, "Delta \(id) -> \(system)")
    expect(!SkinGameTypes.isManic(id), "Delta \(id) is not Manic")
}
expect(SkinGameTypes.systemIDs(forGameType: "com.example.unknown").isEmpty, "unknown id is empty")
expect(SkinGameTypes.systemIDs(forGameType: nil).isEmpty, "nil id is empty")
expect(SkinGameTypes.systemIDs(forGameType: "public.aoshuang.game.nope").isEmpty,
       "unknown Manic suffix is empty")

// MARK: 2. Sharing

let groups: [[String]] = [["gb", "gbc"], ["genesis", "segacd", "sega32x"],
                          ["sms", "gg", "sg1000"], ["nes", "fds"], ["dos", "doom"]]
for group in groups {
    for a in group {
        for b in group {
            expect(SkinSharing.fits(skinSystems: [a], on: b), "\(a) skin fits \(b)")
        }
    }
}
expect(!SkinSharing.fits(skinSystems: ["gba"], on: "gb"), "a GBA skin does not fit GB")
expect(!SkinSharing.fits(skinSystems: ["nes"], on: "snes"), "an NES skin does not fit SNES")
expect(SkinSharing.related(to: "genesis") == ["segacd", "sega32x"], "Mega Drive relatives")
expect(SkinSharing.related(to: "psp").isEmpty, "PSP has no relatives")

// MARK: 3. The library

func record(_ id: String, _ systems: [String], _ age: TimeInterval) -> SkinRecord {
    SkinRecord(id: id, name: id.uppercased(), systems: systems,
               importedAt: Date(timeIntervalSince1970: 1_000_000 + age))
}
var index = SkinLibraryIndex()
expect(index.resolve(system: "gba", gameKey: nil) == nil, "empty library: built-in pad")
index.add(record("gba-old", ["gba"], 1), makeDefaultFor: [])
index.add(record("gba-new", ["gba"], 2), makeDefaultFor: [])
expect(index.resolve(system: "gba", gameKey: nil) == "gba-new", "no default: newest own skin")
index.setDefault(.skin("gba-old"), for: "gba")
expect(index.resolve(system: "gba", gameKey: nil) == "gba-old", "the default wins over newest")
index.setGame(.skin("gba-new"), for: "Zelda.gba")
expect(index.resolve(system: "gba", gameKey: "Zelda.gba") == "gba-new", "the game's choice wins")
expect(index.resolve(system: "gba", gameKey: "Other.gba") == "gba-old", "other games: default")
index.setGame(.none, for: "Pad.gba")
expect(index.resolve(system: "gba", gameKey: "Pad.gba") == nil, "a game can ask for no skin")
index.setDefault(.none, for: "gba")
expect(index.resolve(system: "gba", gameKey: nil) == nil, "a system default can be no skin")
expect(index.resolve(system: "gba", gameKey: "Zelda.gba") == "gba-new",
       "a game's own skin still wins over a no-skin default")
index.setDefault(.automatic, for: "gba")
expect(index.defaults["gba"] == nil, "automatic clears the stored default")

// Sharing: a Game Boy skin on a Game Boy Color game, through a default and through newest.
index.add(record("gb-skin", ["gb"], 3), makeDefaultFor: ["gb"])
expect(index.resolve(system: "gbc", gameKey: nil) == "gb-skin", "GBC uses the GB default")
index.add(record("gbc-skin", ["gbc"], 4), makeDefaultFor: [])
expect(index.resolve(system: "gbc", gameKey: nil) == "gb-skin",
       "a relative's default beats an undefaulted own skin")
index.setDefault(.skin("gbc-skin"), for: "gbc")
expect(index.resolve(system: "gbc", gameKey: nil) == "gbc-skin", "own default wins")
expect(index.skins(for: "gbc").map(\.id) == ["gbc-skin", "gb-skin"], "own skins list first")
index.setGame(.skin("gba-new"), for: "Tetris.gb")
expect(index.resolve(system: "gb", gameKey: "Tetris.gb") == "gb-skin",
       "a game choice that does not fit the system is skipped")
index.add(record("md", ["genesis"], 5), makeDefaultFor: [])
expect(index.resolve(system: "sega32x", gameKey: nil) == "md", "a 32X game uses a Mega Drive skin")
expect(index.resolve(system: "segacd", gameKey: nil) == "md", "a Sega CD game too")

// Rename and delete.
expect(index.rename("md", to: "  Mega Drive Classic  "), "rename works")
expect(index.records["md"]?.name == "Mega Drive Classic", "rename trims")
expect(!index.rename("md", to: "   "), "an empty name is refused")
expect(!index.rename("missing", to: "x"), "renaming a missing skin is refused")
index.setDefault(.skin("md"), for: "genesis")
index.setGame(.skin("md"), for: "Sonic.md")
expect(index.delete("md") != nil, "delete returns the record")
expect(index.records["md"] == nil && index.defaults["genesis"] == nil
       && index.perGame["Sonic.md"] == nil, "delete clears defaults and game choices")
expect(index.resolve(system: "genesis", gameKey: "Sonic.md") == nil, "deleted: back to the pad")
index.setGame(.skin("ghost"), for: "Ghost.gba")
expect(index.resolve(system: "gba", gameKey: "Ghost.gba") == "gba-new",
       "a choice naming a skin that is gone is skipped")

// Legacy adoption: one skin per system, keyed by the system id.
var legacy = SkinLibraryIndex()
legacy.adoptLegacy(["gba": "Old GBA", "ps1": "Old PS1"])
expect(legacy.records["gba"]?.systems == ["gba"] && legacy.records["gba"]?.format == "legacy",
       "a legacy skin becomes a record keyed by its system")
expect(legacy.defaults["gba"] == "gba" && legacy.defaults["ps1"] == "ps1",
       "a legacy skin stays its system's default")
legacy.keepOnly(["gba"])
expect(legacy.records["ps1"] == nil && legacy.defaults["ps1"] == nil,
       "records with no stored skin are dropped")

// Codable: round trip and forgiving decode.
let encoded = try! JSONEncoder().encode(index)
let decoded = try! JSONDecoder().decode(SkinLibraryIndex.self, from: encoded)
expect(decoded == index, "the library round-trips")
let damaged = #"{"records": 7, "defaults": {"gba": "gba-old"}}"#.data(using: .utf8)!
let partial = try! JSONDecoder().decode(SkinLibraryIndex.self, from: damaged)
expect(partial.records.isEmpty && partial.defaults["gba"] == "gba-old",
       "one damaged field does not lose the others")
let thin = #"{"id": "x"}"#.data(using: .utf8)!
let thinRecord = try! JSONDecoder().decode(SkinRecord.self, from: thin)
expect(thinRecord.name == "Skin" && thinRecord.systems.isEmpty && !thinRecord.hasSound,
       "a record missing fields decodes with defaults")
expect(SkinLibraryIndex.newID().hasPrefix("skin-"), "new ids are not system ids")

// A random id is not a console. Only a real system id is adopted.
var stray = SkinLibraryIndex()
stray.adoptLegacy(["gba": "Old GBA", "skin-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee": "Nope"],
                  knownSystems: ["gba", "ps1"])
expect(stray.records["gba"] != nil, "a real system id is still adopted")
expect(stray.records["skin-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"] == nil,
       "a random skin id is not turned into a console")

// The same named skin gets the same id everywhere, and two copies collapse into one.
let stable = SkinLibraryIndex.stableID(identifier: "com.example.gba", systems: ["gba"])
expect(stable == SkinLibraryIndex.stableID(identifier: " COM.EXAMPLE.GBA ", systems: ["gba"]),
       "the shared id ignores case and spaces")
expect(stable == SkinLibraryIndex.stableID(identifier: "com.example.gba", systems: ["GBA"]),
       "the shared id ignores how the console id is capitalised")
expect(stable != SkinLibraryIndex.stableID(identifier: "com.example.gba", systems: ["gbc"]),
       "a different console is a different skin")
expect(SkinLibraryIndex.stableID(identifier: "  ", systems: ["gba"]) == nil,
       "a skin that does not name itself has no shared id")
var twins = SkinLibraryIndex()
var first = record("skin-old-a", ["gba"], 1)
first.identifier = "com.example.gba"
var second = record("skin-old-b", ["gba"], 5)
second.identifier = "com.example.gba"
twins.records[first.id] = first
twins.records[second.id] = second
twins.defaults["gba"] = "skin-old-a"
twins.perGame["Tetris.gba"] = "skin-old-b"
let fold = twins.coalesceNamedSkins()
expect(twins.records.count == 1 && twins.records[stable!] != nil, "two copies become the shared id")
expect(twins.defaults["gba"] == stable && twins.perGame["Tetris.gba"] == stable,
       "choices follow the skin to the shared id")
expect(fold.renames["skin-old-a"] == stable && fold.renames["skin-old-b"] == stable,
       "both old ids are renamed")
expect(twins.records[stable!]?.name == "SKIN-OLD-B", "the newer copy's name is the one kept")
expect(fold.winners[stable!] == "skin-old-b", "the newer copy is the one whose files are kept")

// The copy already on the shared id wins when it is the newer one. The spare is what moves.
var parked = SkinLibraryIndex()
var spare = record("skin-old-a", ["gba"], 1)
spare.identifier = "com.example.gba"
var current = record(stable!, ["gba"], 9)
current.identifier = "com.example.gba"
parked.records[spare.id] = spare
parked.records[current.id] = current
let parkedFold = parked.coalesceNamedSkins()
expect(parkedFold.winners[stable!] == stable, "files already on the shared id stay when they are newer")
expect(parkedFold.renames["skin-old-a"] == stable && parkedFold.renames[stable!] == nil,
       "only the spare copy is renamed")

// A fake console saved by an older build is removed. A real legacy skin is not.
var polluted = SkinLibraryIndex()
polluted.adoptLegacy(["gba": "Old GBA", "skin-deadbeef": "Nope"])
expect(polluted.records["skin-deadbeef"] != nil, "without a console list the old call still adopts")
let dropped = polluted.dropFakeConsoles(knownSystems: ["gba"])
expect(polluted.records["gba"] != nil && polluted.records["skin-deadbeef"] == nil,
       "a saved fake console is removed and a real one stays")
expect(dropped == ["skin-deadbeef"], "the fake id is the one reported")
expect(polluted.defaults["skin-deadbeef"] == nil && polluted.defaults["gba"] == "gba",
       "the fake console's default goes and the real one stays")

// One unreadable skin does not wipe the library, and a library whose every skin is unreadable
// is refused rather than saved as empty.
let mixed = #"{"records":{"good":{"id":"good","systems":["gba"]},"bad":{"name":"no id"}},"defaults":{"gba":"good"},"perGame":{}}"#
    .data(using: .utf8)!
let kept = SkinLibraryIndex.stored(mixed)
expect(kept?.records["good"]?.systems == ["gba"] && kept?.records["bad"] == nil,
       "a bad skin is dropped and the good one stays")
expect(kept?.defaults["gba"] == "good", "defaults survive a bad skin")
let allBad = #"{"records":{"bad":{"name":"no id"}},"defaults":{},"perGame":{}}"#.data(using: .utf8)!
expect(SkinLibraryIndex.stored(allBad) == nil, "a library that lost every skin is not trusted")
let notJson = "nope".data(using: .utf8)!
expect(SkinLibraryIndex.stored(notJson) == nil, "a file that is not a library is not trusted")

// MARK: 4. The function list and the dispatcher table

let manicFunctions = """
flex quickSave quickLoad fastForward toggleFastForward fastForward2x fastForward3x fastForward4x \
reverseScreens volume saveStates cheatCodes skins filters screenshot haptics controllers \
orientation functionLayout restart resolution quit amiibo homeMenu toggleControlls blowing \
palette swapDisk insertDisc shake toggleAnalog retroAchievements airPlayScaling airPlayLayout \
gameplayManuals triggerPro tvType leftDifficulty rightDifficulty screenScaling j2meSettings \
dosSettings coreSettings rewind slowMotion wswanRotation ndsLidToggle skinButtonBinding
""".split(separator: " ").map(String.init)
expect(manicFunctions.count == 48, "the brief lists 48 functions")
expect(SkinFunction.allCases.map(\.rawValue) == manicFunctions,
       "SkinFunction is exactly Manic's list, in order")
for name in manicFunctions {
    expect(SkinFunction.named(name)?.rawValue == name, "\(name) parses")
    expect(SkinFunction.named(name.uppercased())?.rawValue == name, "\(name) ignores case")
    let function = SkinFunction(rawValue: name)!
    expect(!function.title.isEmpty && !function.caption.isEmpty, "\(name) has words")
    expect(function.caption.count <= 6, "\(name) caption fits a small button")
}
expect(SkinFunction.named("toggle_controls") == .toggleControlls, "alias toggleControls")
expect(SkinFunction.named("swap-disc") == .swapDisk, "alias swapDisc")
expect(SkinFunction.named("a") == nil && SkinFunction.named("start") == nil,
       "game buttons are not functions")
expect(SkinFunction.named("menu") == nil && SkinFunction.named("home") == nil,
       "menu and home stay buttons at this level")
expect(Set(SkinFunction.allCases.filter(\.isHold).map(\.rawValue))
       == ["fastForward", "fastForward2x", "fastForward3x", "fastForward4x", "rewind", "blowing"],
       "the held functions")
expect(SkinFunction.fastForward3x.holdSpeed == 3, "3x is 3")

// Every pending method is reached, and every reach is a pending method.
let pendingAll = Set(SkinFunctionPendingNames.all)
expect(pendingAll.count == 27, "27 pending methods")
var reached = Set<String>()
for function in SkinFunction.allCases {
    if case .pending(let name) = function.route {
        expect(pendingAll.contains(name), "\(function.rawValue) routes to a listed method \(name)")
        reached.insert(name)
    }
}
for state in SkinFunctionState.allCases {
    if let reader = state.pendingReader { reached.insert(reader) }
}
reached.formUnion(SkinFunctionPendingNames.menuOnly)
expect(reached == pendingAll, "every pending method is used: missing \(pendingAll.subtracting(reached))")
for function in [SkinFunction.dosSettings, .coreSettings] {
    expect(function.route == .pending("showCoreSettings"), "\(function) opens core settings")
}
expect(SkinFunction.j2meSettings.route == .existing("showJ2MESettings"),
       "j2meSettings opens the J2ME player's settings")
let bound: [SkinFunction: SkinFunctionState] = [
    .reverseScreens: .screensSwapped, .volume: .muted, .toggleControlls: .controlsHidden,
    .toggleAnalog: .analogMode, .tvType: .tvColour, .leftDifficulty: .leftDifficultyA,
    .rightDifficulty: .rightDifficultyA,
]
for function in SkinFunction.allCases {
    expect(function.boundState == bound[function], "\(function) state binding")
}

// PadAppAction is the same list plus menu, so floating buttons reach every function.
expect(Set(PadAppAction.allCases.map(\.rawValue)) == Set(manicFunctions + ["menu"]),
       "PadAppAction is every function plus menu")
for action in PadAppAction.allCases where action != .menu {
    expect(action.skinFunction?.rawValue == action.rawValue, "\(action) maps onto the dispatcher")
    expect(action.isHold == action.skinFunction!.isHold, "\(action) hold matches")
}
for old in ["quickSave", "quickLoad", "fastForward", "rewind", "screenshot", "menu"] {
    expect(PadAppAction(rawValue: old) != nil, "a floating button saved as \(old) still decodes")
}

// The dispatcher's switch, read from the source: each function's case calls its route's method.
let functionsSource = try! String(contentsOfFile: iosDir + "/SkinFunctions.swift", encoding: .utf8)
guard let start = functionsSource.range(of: "func performSkinFunction(_ function: SkinFunction"),
      let end = functionsSource.range(of: "func performSkinFunction(named", range: start.upperBound..<functionsSource.endIndex) else {
    fatalError("dispatcher not found")
}
let dispatcher = String(functionsSource[start.upperBound..<end.lowerBound])
/// The text of the `case` arm naming `.<name>` (alone or in a list), up to the next arm.
func arm(for name: String) -> String? {
    let lines = dispatcher.components(separatedBy: "\n")
    var collecting = false
    var out: [String] = []
    for line in lines {
        let trimmed = line.trimmingCharacters(in: .whitespaces)
        if trimmed.hasPrefix("case .") && trimmed.hasSuffix(":") {
            if collecting { break }
            let names = trimmed.dropFirst(5).dropLast()
                .split(separator: ",").map { $0.trimmingCharacters(in: CharacterSet(charactersIn: " .")) }
            if names.contains(name) { collecting = true; continue }
        } else if collecting {
            out.append(trimmed)
        }
    }
    return collecting ? out.joined(separator: "\n") : nil
}
for function in SkinFunction.allCases {
    guard let body = arm(for: function.rawValue) else {
        expect(false, "dispatcher has a case for \(function.rawValue)")
        continue
    }
    switch function.route {
    case .pending(let name):
        expect(body.contains(name + "("), "\(function.rawValue) calls \(name)(")
        expect(body.contains("status ="), "\(function.rawValue) puts the answer on the status line")
    case .sheet(let name):
        let opens = body.contains("sheet = ." + name) || (name == "layoutEditor" && body.contains("showLayoutEditor = true"))
        expect(opens, "\(function.rawValue) opens the \(name) sheet")
    case .existing(let name):
        let key = name.components(separatedBy: CharacterSet(charactersIn: "./(")).filter { !$0.isEmpty }
        let method = key.count > 1 ? key[1] : key[0]
        let hit = body.contains(method) || body.contains(String(method.prefix(10)))
        expect(hit, "\(function.rawValue) reaches existing code \(name)")
    }
}
expect(dispatcher.contains("guard running || webPlayer != nil, activeEntry != nil"),
       "no game: refused with a line")
expect(dispatcher.contains("player.kind.refusal(for: function)"),
       "a bundled player refuses the functions it cannot do, with a line")
expect(dispatcher.contains("refusedOnline && netplayLive"), "online play refusals")

// MARK: 5. The temporary stubs

let appSources = (try! FileManager.default.contentsOfDirectory(atPath: iosDir)).filter { $0.hasSuffix(".swift") }.map { try! String(contentsOfFile: iosDir + "/" + $0, encoding: .utf8) }.joined()
for name in SkinFunctionPendingNames.all {
    expect(appSources.contains("func \(name)("), "the app implements \(name)")
}
func checkStubs() {
    let host = EngineHost()
    let lines: [(String, String)] = [
        ("showCoreSettings", host.showCoreSettings()), ("showFilters", host.showFilters()),
        ("cyclePalette", host.cyclePalette()), ("cycleFastForward", host.cycleFastForward()),
        ("setHoldSpeed", host.setHoldSpeed(2, held: true)),
        ("toggleSlowMotion", host.toggleSlowMotion()), ("swapDisc", host.swapDisc()),
        ("insertDisc", host.insertDisc()), ("cycleResolution", host.cycleResolution()),
        ("cycleScreenScaling", host.cycleScreenScaling()),
        ("cycleAirPlayScaling", host.cycleAirPlayScaling()),
        ("cycleAirPlayLayout", host.cycleAirPlayLayout()), ("rotateScreen", host.rotateScreen()),
        ("toggleTVType", host.toggleTVType()), ("toggleDifficulty", host.toggleDifficulty(left: true)),
        ("shake", host.shake()), ("toggleAnalogMode", host.toggleAnalogMode()),
        ("toggleDSLid", host.toggleDSLid()), ("blowIntoMic", host.blowIntoMic(held: true)),
        ("pressHomeButton", host.pressHomeButton()), ("showControllers", host.showControllers()),
        ("cycleTriggerProfile", host.cycleTriggerProfile()),
        ("showButtonBinding", host.showButtonBinding()),
        ("showGameplayManual", host.showGameplayManual()),
    ]
    for (name, line) in lines {
        expect(line == "\(name) is not in this build yet", "stub \(name) says so plainly")
    }
    expect(!host.currentTVTypeIsColor() && !host.difficultyIsA(left: false) && !host.isAnalogMode(),
           "stub readers rest at false")
}
checkStubs()

// MARK: 6. Manic item fields

func item(_ json: String) -> [String: Any] {
    try! JSONSerialization.jsonObject(with: json.data(using: .utf8)!) as! [String: Any]
}
let itemFrame = CGRect(x: 100, y: 200, width: 80, height: 40)
let manicSwitch = item(#"""
{"inputs": "volume", "frame": {"x": 100, "y": 200, "width": 80, "height": 40},
 "asset": {"normal": "knob.pdf", "selected": "knob_on.pdf"},
 "animation": {"type": "spring", "begin": {"x": 0, "y": 0, "width": 40, "height": 40},
               "end": {"x": 40, "y": 0, "width": 40, "height": 40}},
 "selfRetracting": false}
"""#)
expect(ManicItems.isSwitch(manicSwitch), "an animated item is a switch")
let parsed = ManicItems.toggle(manicSwitch, itemFrame: itemFrame)
expect(parsed?.selectedFileName == "knob_on.pdf", "the switch keeps its selected picture")
expect(parsed?.begin == DeltaSkinNormalizedRect(x: 0, y: 0, width: 0.5, height: 1),
       "begin frame is a fraction of the item")
expect(parsed?.end == DeltaSkinNormalizedRect(x: 0.5, y: 0, width: 0.5, height: 1),
       "end frame is a fraction of the item")
expect(parsed?.spring == true && parsed?.selfRetracting == false, "spring, latching")
expect(ManicItems.inputNames(manicSwitch["inputs"]) == ["volume"], "inputs as a single string")
expect(ManicItems.function(for: ["volume"], systemID: "gba") == .volume, "volume is a function")

let momentary = item(#"""
{"inputs": "select", "frame": {"x": 0, "y": 0, "width": 10, "height": 10},
 "selfRetracting": true, "animation": {"type": "linear", "end": {"x": 2, "y": 3}}}
"""#)
let momentaryToggle = ManicItems.toggle(momentary, itemFrame: CGRect(x: 0, y: 0, width: 10, height: 10))
expect(momentaryToggle?.selfRetracting == true && momentaryToggle?.spring == false,
       "momentary, not a spring")
expect(momentaryToggle?.begin == nil, "no begin frame is the whole item")
expect(momentaryToggle?.end == DeltaSkinNormalizedRect(x: 0.2, y: 0.3, width: 1, height: 1),
       "an end frame without a size takes the item's")
expect(ManicItems.function(for: ["select"], systemID: "atari2600") == nil, "select stays a button")

let selfRetractingString = item(#"{"inputs": "a", "selfRetracting": "true"}"#)
expect(ManicItems.isSwitch(selfRetractingString) && ManicItems.bool(selfRetractingString["selfRetracting"]),
       "selfRetracting as a string")

let deltaPressed = item(#"""
{"inputs": ["a"], "asset": {"normal": "a.pdf", "selected": "a_down.pdf"}}
"""#)
expect(!ManicItems.isSwitch(deltaPressed), "Delta's selected picture is still a pressed picture")
let selectedWithString = item(#"{"inputs": "tvType", "asset": {"selected": "on.png"}}"#)
expect(ManicItems.isSwitch(selectedWithString), "selected plus a single-string input is a switch")

expect(ManicItems.function(for: ["menu"], systemID: "gba") == .flex, "Delta menu opens the menu")
expect(ManicItems.function(for: ["menu"], systemID: "n3ds") == nil, "3DS menu stays Home")
expect(ManicItems.function(for: ["a", "b"], systemID: "gba") == nil, "a combo is not a function")
expect(ManicItems.refusal(for: ["a", "b"], systemID: "gba") == nil, "a combo is allowed")
expect(ManicItems.refusal(for: ["quickSave"], systemID: "gba") == nil, "one function is allowed")
let refusal = ManicItems.refusal(for: ["quickSave", "a"], systemID: "gba")
expect(refusal?.contains("quickSave+a") == true && refusal?.contains("left out") == true,
       "a function mixed with an input is refused with a plain line")
expect(ManicItems.inputNames(["up": "up", "down": "down"]) == ["down", "up"],
       "a dictionary's names come out in a stable order")
expect(ManicItems.inputNames([" a ", "", "b"]) == ["a", "b"], "trimmed, empties dropped")
expect(ManicItems.soundFileName == "sound.caf", "the sound file name")

// MARK: 7. The committed Manic sample, item by item, the way SkinControls.resolve decides

let samplePath = iosDir + "/../../docs/samples/manic-skin-gba-info.json"
let sample = try! JSONSerialization.jsonObject(
    with: Data(contentsOf: URL(fileURLWithPath: samplePath))) as! [String: Any]
let gameType = sample["gameTypeIdentifier"] as? String
expect(SkinGameTypes.systemIDs(forGameType: gameType) == ["gba"], "the sample is a GBA skin")
let representations = sample["representations"] as! [String: Any]
let iphone = representations["iphone"] as! [String: Any]
let edgeToEdge = iphone["edgeToEdge"] as! [String: Any]
let portrait = edgeToEdge["portrait"] as! [String: Any]
var functions: [SkinFunction] = []
var switches = 0
var refused = 0
var combos = 0
for raw in portrait["items"] as! [[String: Any]] {
    if raw["inputs"] is [String: Any] { continue }   // the D-pad
    let frame = raw["frame"] as! [String: Any]
    let rect = CGRect(x: ManicItems.number(frame["x"]) ?? 0, y: ManicItems.number(frame["y"]) ?? 0,
                      width: ManicItems.number(frame["width"]) ?? 0,
                      height: ManicItems.number(frame["height"]) ?? 0)
    let names = ManicItems.inputNames(raw["inputs"])
    if ManicItems.toggle(raw, itemFrame: rect) != nil { switches += 1 }
    if ManicItems.refusal(for: names, systemID: "gba") != nil { refused += 1; continue }
    if let function = ManicItems.function(for: names, systemID: "gba") {
        functions.append(function)
        continue
    }
    if names.count > 1 { combos += 1 }
}
expect(functions == [.quickSave, .fastForward, .flex, .volume],
       "sample functions: quickSave, fastForward, menu as flex, volume (got \(functions))")
expect(switches == 2 && refused == 1 && combos == 1, "sample: 2 switches, 1 refused, 1 combo")

print("\(checks) checks, \(failures) failed")
exit(failures == 0 ? 0 : 1)
