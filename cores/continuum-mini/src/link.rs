//! The console's infrared line. With nothing connected the receiver reads dark, as on PokeMini
//! (the emulator cMini replaces); a link to a second console sets `connected` and feeds `rx`.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Infrared {
    /// A second console is on the other end.
    pub connected: bool,
    /// The transmitter is enabled, and what it sends.
    pub tx_on: bool,
    pub tx: bool,
    /// What the other console sends.
    pub rx: bool,
}

impl Infrared {
    pub fn transmit(&mut self, on: bool, bit: bool) {
        self.tx_on = on;
        self.tx = on && bit;
    }

    pub fn receiving(&self) -> bool {
        self.connected && self.rx
    }
}
