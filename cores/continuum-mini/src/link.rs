//! The console's infrared pin and deterministic two-console byte transport.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Infrared {
    pub enabled: bool,
    pub tx: bool,
    pub rx: bool,
    incoming: VecDeque<bool>,
}

impl Infrared {
    pub fn write(&mut self, enabled: bool, bit: bool) {
        self.enabled = enabled;
        self.tx = enabled && bit;
    }

    pub fn receive(&mut self, bit: bool) {
        self.incoming.push_back(bit);
    }

    pub fn clock(&mut self) -> bool {
        if self.enabled {
            if let Some(bit) = self.incoming.pop_front() {
                self.rx = bit;
            }
        } else {
            self.rx = false;
        }
        self.rx
    }
}
