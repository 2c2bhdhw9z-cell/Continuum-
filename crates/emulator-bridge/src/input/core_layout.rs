//! Joypad buttons as one core reads them, when that differs from the retro pad meaning the rest
//! of the app was built around.
//!
//! The Mega Drive's touch pads, controller mapping and saved remap profiles press retro buttons
//! in the layout the Mega Drive and 32X cores before the licence swap read: Y = A, B = B, A = C,
//! L = X, X = Y, R = Z.
//! BlastEm reads B = A, A = B, R = C, Y = X, X = Y, L = Z. Translating here, on the last snapshot
//! before each core frame, keeps every Mega Drive button doing what it did with no change
//! anywhere else. Online play exchanges the untranslated input, so both phones agree.

use super::{Button, InputSnapshot};

/// `(the button the app pressed, the button this core reads for it)`. A permutation: every
/// button on the left also appears on the right.
pub type Table = &'static [(Button, Button)];

/// The pre-swap Mega Drive retro meaning to BlastEm's. Select (Mode), Start and the
/// D-pad are the same in both.
const BLASTEM: Table = &[
    (Button::Y, Button::B),
    (Button::B, Button::A),
    (Button::A, Button::R),
    (Button::L, Button::Y),
    (Button::X, Button::X),
    (Button::R, Button::L),
];

/// The table for `core_id`, or `None` when its buttons need no translation.
pub fn table_for(core_id: &str) -> Option<Table> {
    match core_id {
        "blastem" => Some(BLASTEM),
        _ => None,
    }
}

/// Rewrites the joypad buttons on ports 0 and 1 (BlastEm reads only two) through `table`.
pub fn translate(snapshot: &mut InputSnapshot, table: Table) {
    let moved: u32 = table.iter().fold(0, |mask, (from, _)| mask | 1 << *from as u32);
    for port in snapshot.ports.iter_mut().take(2) {
        let pressed = port.buttons;
        let mut out = pressed & !moved;
        for (from, to) in table {
            if pressed & (1 << *from as u32) != 0 {
                out |= 1 << *to as u32;
            }
        }
        port.buttons = out;
    }
}

/// [`translate`] for `core_id`'s table, or nothing at all.
pub fn translate_for(core_id: &str, snapshot: &mut InputSnapshot) {
    if let Some(table) = table_for(core_id) {
        translate(snapshot, table);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::InputState;

    fn pressed(buttons: &[Button], port: usize) -> InputSnapshot {
        let mut state = InputState::default();
        for b in buttons {
            state.set_button(port, *b, true);
        }
        state.snapshot()
    }

    #[test]
    fn mega_drive_buttons_reach_blastem_as_it_reads_them() {
        // The app's Mega Drive A is retro Y; BlastEm reads its A on retro B.
        let mut s = pressed(&[Button::Y], 0);
        translate_for("blastem", &mut s);
        assert!(s.button(0, Button::B));
        assert!(!s.button(0, Button::Y));
        for (from, to) in [
            (Button::B, Button::A),
            (Button::A, Button::R),
            (Button::L, Button::Y),
            (Button::X, Button::X),
            (Button::R, Button::L),
        ] {
            let mut s = pressed(&[from], 1);
            translate_for("blastem", &mut s);
            assert!(s.button(1, to), "{from:?} -> {to:?}");
            assert_eq!(s.port(1).buttons.count_ones(), 1);
        }
    }

    #[test]
    fn start_select_and_the_d_pad_are_unchanged_and_other_cores_untouched() {
        let all = [Button::Start, Button::Select, Button::Up, Button::Down, Button::Left, Button::Right];
        let mut s = pressed(&all, 0);
        let before = s.port(0).buttons;
        translate_for("blastem", &mut s);
        assert_eq!(s.port(0).buttons, before);
        // Several at once keep their count (a permutation).
        let mut s = pressed(&[Button::Y, Button::B, Button::A, Button::Start], 0);
        translate_for("blastem", &mut s);
        assert_eq!(s.port(0).buttons.count_ones(), 4);
        assert!(s.button(0, Button::B) && s.button(0, Button::A) && s.button(0, Button::R));
        // Another core, and ports past the second, are left alone.
        let mut s = pressed(&[Button::Y], 0);
        translate_for("gearsystem", &mut s);
        assert!(s.button(0, Button::Y));
        let mut s = pressed(&[Button::Y], 2);
        translate_for("blastem", &mut s);
        assert!(s.button(2, Button::Y));
    }
}
