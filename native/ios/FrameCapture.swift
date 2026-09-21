// Continuum - one captured frame, turned into a cover.
//
// THE ARTWORK TIER THAT ASKS NOBODY. Every other tier ends in a question put to somebody else: the
// exact thumbnail name, then the rewritten names, then the server's own directory listings, then
// another system's listings, then an image the user goes and finds in Files. All of those fail
// together for a ROM that has never been scanned by anyone, which is a homebrew build, a translation
// patch, a prototype or a hack. The game itself is on screen at that moment, so this tier takes a
// picture of it. All ten of the games this app was built against already have real box art from the
// internet; this exists for the eleventh.
//
// IT IS CALLED TWO DIFFERENT NUMBERS IN THIS PROJECT AND BOTH MEAN THIS FILE. The Settings note that
// used to list it as missing called it the fourth artwork tier, counting the sources a cover can end
// up coming from; ArtworkStore's own section markers call it tier 6, counting the rungs of the ladder
// that file walks, where an image picked from Files is tier 5. Neither is wrong and there is nothing
// to reconcile, but a reader who assumes they are the same count will be looking for a tier that does
// not exist.
//
// SPLIT OUT OF ArtworkStore.swift FOR THE REASON GameArtwork.swift IS. Everything in here is a pure
// function of a width, a height and a block of bytes: no store, no actor, no file, no network. That
// matters more here than it does for a name transform, because there is no iOS compiler and no
// device on the build host, so the arithmetic below is checked by extracting it verbatim into the
// offline harness and running it on Linux. The half that needs CoreGraphics cannot be checked that
// way, which is exactly why as little as possible lives in it.
//
// THREE TRAPS, and every one of them produces a cover that reads as a bug in the emulator rather
// than as a bug in this file:
//
//  1. THE STRIDE. The engine has already removed the GPU's row alignment, so the rows are TIGHT:
//     width * 4 bytes, no padding. A bitmap context asked to pick its own bytesPerRow rounds each
//     row up to a hardware-friendly multiple, and 240 * 4 is already a multiple of 64 while 241 * 4
//     is not, so a wrong stride is a bug that appears on some games and not others. It shears the
//     picture diagonally and nothing reports an error. The stride is therefore passed explicitly
//     and computed in exactly one place, `CapturedFrameLayout.plan`.
//  2. THE BYTE ORDER. Little-endian order with the alpha byte first is BGRA, which is what a Metal
//     drawable usually is, so it is the ordering a reader is most likely to reach for. Read RGBA
//     bytes through it and red and blue swap: a blue sky comes out orange with no error anywhere.
//     See `CapturedCover.bitmapInfo`.
//  3. THE ALPHA BYTE, which is the one decision in this file that could silently store nothing at
//     all. Also `CapturedCover.bitmapInfo`.
//
// ROW ORDER IS NOT ONE OF THE TRAPS, which is worth saying out loud because it is the first one in
// most screenshot code. A CGImage's backing store is defined with its FIRST row at the top, and the
// bottom-left origin people remember belongs to drawing operations, not to the bytes behind them.
// The engine documents top row first, so there is no flip here, and a flip that was wrong would be
// upside down on screen rather than silently wrong.

import CoreGraphics
import Foundation
import UIKit

// MARK: - The arithmetic

/// What one capture's bytes have to add up to, and the stride they have to be read at.
///
/// Pure and deliberately joyless. Its whole job is to turn two numbers the engine reported and one
/// byte count into either a plan or a sentence, so that the CoreGraphics call below is handed values
/// that are already known to be consistent rather than having to decide anything itself.
enum CapturedFrameLayout {
    /// The largest side accepted, in pixels.
    ///
    /// NOT A TASTE LIMIT, IT IS WHAT MAKES THE MULTIPLICATION SAFE. Two sides near the top of a
    /// UInt32 multiply out past 7e19, `Int.max` is about 9.2e18, and an overflow in Swift is a trap:
    /// on a sideloaded build that is a crash with no log where this function could have returned a
    /// sentence instead. Sixteen thousand is far above any surface this app can be handed (the
    /// largest iPad is under 3000 points at 2x) and far below where the arithmetic stops fitting.
    static let maximumSide = 16384

    /// One consistent capture: the size, the stride, and the byte count those two imply.
    struct Plan: Equatable, Sendable {
        let width: Int
        let height: Int
        /// `width * 4`. Named rather than recomputed at the call site, because the whole point of
        /// this type is that the stride is decided once. See trap 1 in the file header.
        let bytesPerRow: Int
        let byteCount: Int
    }

    /// Checks a reported capture and returns the plan for reading it, or the reason it was refused.
    ///
    /// A tuple of an optional and a sentence, which is the shape `ArtworkDisk.read(pickedFile:)`
    /// already uses in this system: every failure here ends up on the status line in front of the
    /// user, so a reason is worth more than a thrown error nobody can read.
    ///
    /// A BYTE COUNT THAT DISAGREES IS REFUSED RATHER THAN TRIMMED, including when there are too
    /// many bytes rather than too few. Too many means either harmless trailing bytes or a stride
    /// this code has guessed wrong, and those two are indistinguishable from here: one of them shows
    /// the game and the other shows a sheared smear that looks like a broken emulator. Refusing says
    /// the numbers out loud instead, which is the one outcome that can be acted on.
    static func plan(width: UInt32, height: UInt32,
                     byteCount: Int) -> (plan: Plan?, failure: String?) {
        // Widened before anything is compared or multiplied. Int is 64-bit on every device this
        // app runs on (arm64 only, see project.yml), so no UInt32 can fail to fit, and the range
        // check that follows is what the arithmetic actually depends on.
        let pixelWidth = Int(width)
        let pixelHeight = Int(height)

        guard pixelWidth > 0, pixelHeight > 0 else {
            return (nil, "the capture came back \(pixelWidth) by \(pixelHeight), so there is no "
                    + "picture in it")
        }
        guard pixelWidth <= maximumSide, pixelHeight <= maximumSide else {
            return (nil, "the capture came back \(pixelWidth) by \(pixelHeight), which is larger "
                    + "than this app will read")
        }

        let bytesPerRow = pixelWidth * 4
        let expected = bytesPerRow * pixelHeight
        guard byteCount == expected else {
            return (nil, "the capture came back as \(byteCount) byte(s) for a \(pixelWidth) by "
                    + "\(pixelHeight) picture, which needs exactly \(expected)")
        }

        return (Plan(width: pixelWidth, height: pixelHeight,
                     bytesPerRow: bytesPerRow, byteCount: expected), nil)
    }
}

// MARK: - The bytes, as an image

/// Turns the engine's RGBA bytes into the PNG the artwork store keeps.
///
/// Not isolated to any actor, exactly like `ArtworkDisk`, and `pngData` is async for the same
/// reason every function in that type is: a nonisolated async function does not run on its caller's
/// actor, so the bitmap and the PNG encode of a full-screen frame happen OFF the main actor. That
/// is tens of milliseconds and several megabytes of pixels, and the main actor is drawing a game.
enum CapturedCover {
    /// The pixel format, in one place, with both halves of it argued out.
    ///
    /// `.noneSkipLast` RATHER THAN `.premultipliedLast`, and this is the decision in this file worth
    /// reading twice. The engine's fourth byte is whatever the render pipeline happened to leave in
    /// the alpha channel of an OPAQUE game frame: no core, and nothing downstream of one, ever meant
    /// it as transparency. Read as premultiplied alpha it is taken seriously, so a frame whose alpha
    /// bytes are zero decodes as a fully transparent image, the PNG faithfully stores nothing, and
    /// the cover draws as the generated plate it was meant to replace. That failure reads as "the
    /// capture button does nothing", which is the worst possible presentation of a format mistake.
    /// Skipping the byte states what is true, cannot darken or erase a pixel whatever the engine put
    /// there, and encodes a smaller PNG with no alpha channel at all. A cover is an opaque
    /// rectangle; there is nothing for transparency to mean.
    ///
    /// `.byteOrder32Big` IS NOT DECORATION. It says the bytes are R, G, B, X in rising address
    /// order, which is what the engine documents. The default is treated as big-endian for a 32-bit
    /// pixel today, so writing it changes nothing and defends against the reader who reaches for
    /// `.byteOrder32Little` because a Metal drawable is BGRA. See trap 2 in the file header.
    ///
    /// Typed `UInt32` on purpose: `CGContext.init` takes a raw `bitmapInfo: UInt32` rather than a
    /// `CGBitmapInfo`, and handing it the wrong one of those two is a compile error on a machine
    /// that has no iOS compiler.
    static let bitmapInfo: UInt32 =
        CGImageAlphaInfo.noneSkipLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue

    /// The PNG for one captured frame, or the reason there is none.
    ///
    /// PNG rather than JPEG because this is a picture of a screen: flat colour, hard edges and text,
    /// which is what PNG is good at and what JPEG puts halos around. It is also what the rest of the
    /// artwork system stores, so `ArtworkDisk.store` and every reader of it need to know nothing new.
    static func pngData(width: UInt32, height: UInt32,
                        rgba: Data) async -> (data: Data?, failure: String?) {
        // `rgba.count` and not a size passed in beside it: the bytes are the only authority on how
        // many there are, and a caller that could disagree is a caller that will.
        let (layout, refusal) = CapturedFrameLayout.plan(width: width, height: height,
                                                         byteCount: rgba.count)
        guard let layout else {
            return (nil, refusal ?? "the capture could not be read")
        }
        guard let image = image(layout: layout, rgba: rgba) else {
            return (nil, "a \(layout.width) by \(layout.height) picture could not be built from "
                    + "the captured bytes")
        }
        guard let png = image.pngData() else {
            return (nil, "the captured picture could not be encoded as a PNG")
        }
        return (png, nil)
    }

    /// Wraps the captured bytes in a bitmap context and snapshots them.
    ///
    /// THE COPY IS ACCEPTED ON PURPOSE. `withUnsafeMutableBytes` on a second reference to the same
    /// buffer makes Data copy it, and `makeImage()` copies again into the immutable image, so a
    /// full-screen capture moves around twenty megabytes twice. That is a few milliseconds off the
    /// main actor, once, in response to a deliberate tap, and the alternative (handing CoreGraphics
    /// a data provider over bytes this code no longer owns) trades those milliseconds for a lifetime
    /// question that is answered wrongly by a crash.
    ///
    /// `bytesPerRow` comes from the plan rather than from CoreGraphics, which is trap 1 in the file
    /// header and the single most likely way for this function to go quietly wrong.
    private static func image(layout: CapturedFrameLayout.Plan, rgba: Data) -> UIImage? {
        var bytes = rgba
        // The closure's parameter type is written out so this resolves to the raw-buffer overload of
        // `withUnsafeMutableBytes` rather than to the deprecated typed-pointer one.
        let captured: CGImage? = bytes.withUnsafeMutableBytes {
            (raw: UnsafeMutableRawBufferPointer) -> CGImage? in
            guard let base = raw.baseAddress else { return nil }
            guard let context = CGContext(
                data: base,
                width: layout.width,
                height: layout.height,
                bitsPerComponent: 8,
                bytesPerRow: layout.bytesPerRow,
                // Device RGB because the engine reports no colour profile and inventing one would
                // change the colours away from what the screen just showed. The capture goes
                // through the same pipeline as a present, so this is a picture of the game as
                // displayed, and a colour conversion would be the one part of it that was not.
                space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: bitmapInfo
            ) else { return nil }
            // A snapshot of the buffer, which is what makes it safe for the image to outlive this
            // closure and the pointer it was built on.
            return context.makeImage()
        }
        guard let captured else { return nil }
        // Scale 1 and orientation up, which is right: these are real pixels from a surface, not
        // points, and the artwork store treats every cover as a plain bitmap.
        return UIImage(cgImage: captured)
    }
}
