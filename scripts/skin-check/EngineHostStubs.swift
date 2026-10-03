// Continuum skin check ONLY: stand-ins for the EngineHost methods, so the pure-Foundation skin code compiles off a Mac. Never part of the app.
//
// DELETE EACH STUB WHEN ITS REAL METHOD MERGES. The skin function dispatcher (SkinFunctions.swift)
// calls these by their final names, so the real method replaces the stub with no other change.
// Every stub answers with a plain line saying the feature is not in this build yet, which the
// dispatcher puts on the status strip: a button that silently does nothing is the failure this app
// has a rule against. The Bool readers answer false, the resting state of each switch.
//
// Owners: coreopts (core settings, filters, palettes, speeds, slow motion, discs, resolution,
// screen scaling, AirPlay, rotation, Atari switches) and input (analog mode, shake, DS lid, blow,
// 3DS Home, controllers, trigger profiles, button binding). Gameplay manuals: import.

import Foundation

extension EngineHost {
    func showCoreSettings() -> String { "showCoreSettings is not in this build yet" }
    func showFilters() -> String { "showFilters is not in this build yet" }
    func cyclePalette() -> String { "cyclePalette is not in this build yet" }
    func cycleFastForward() -> String { "cycleFastForward is not in this build yet" }
    func setHoldSpeed(_ multiplier: Double, held: Bool) -> String {
        "setHoldSpeed is not in this build yet"
    }
    func toggleSlowMotion() -> String { "toggleSlowMotion is not in this build yet" }
    func swapDisc() -> String { "swapDisc is not in this build yet" }
    func insertDisc() -> String { "insertDisc is not in this build yet" }
    func cycleResolution() -> String { "cycleResolution is not in this build yet" }
    func cycleScreenScaling() -> String { "cycleScreenScaling is not in this build yet" }
    func cycleAirPlayScaling() -> String { "cycleAirPlayScaling is not in this build yet" }
    func cycleAirPlayLayout() -> String { "cycleAirPlayLayout is not in this build yet" }
    func rotateScreen() -> String { "rotateScreen is not in this build yet" }
    func toggleTVType() -> String { "toggleTVType is not in this build yet" }
    func toggleDifficulty(left: Bool) -> String { "toggleDifficulty is not in this build yet" }
    func currentTVTypeIsColor() -> Bool { false }
    func difficultyIsA(left: Bool) -> Bool { false }
    func shake() -> String { "shake is not in this build yet" }
    func toggleAnalogMode() -> String { "toggleAnalogMode is not in this build yet" }
    func isAnalogMode() -> Bool { false }
    func toggleDSLid() -> String { "toggleDSLid is not in this build yet" }
    func blowIntoMic(held: Bool) -> String { "blowIntoMic is not in this build yet" }
    func pressHomeButton() -> String { "pressHomeButton is not in this build yet" }
    func showControllers() -> String { "showControllers is not in this build yet" }
    func cycleTriggerProfile() -> String { "cycleTriggerProfile is not in this build yet" }
    func showButtonBinding() -> String { "showButtonBinding is not in this build yet" }
    func showGameplayManual() -> String { "showGameplayManual is not in this build yet" }
}
