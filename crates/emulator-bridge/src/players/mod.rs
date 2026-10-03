//! The bundled players: Flash (Ruffle) and J2ME (J2meJS), which run inside the app in a web view
//! that loads local files only (native/ios/WebPlayers.swift), the way Manic EMU runs them. They
//! are not libretro cores and never pass through the engine's session, so this module holds only
//! the behaviour around them that Android will want too:
//!
//!   * [`keys`]: what each pad button sends (a keyboard key for Flash, a phone key for J2ME), the
//!     per-game remap format, and the J2ME screen sizes;
//!   * [`flash_save`]: Ruffle's SharedObjects as Manic's `.json`;
//!   * [`j2me_save`]: the phone's record stores as Manic's `.J2meJS.srm`;
//!   * [`jar`]: the MIDlet class, name and screen size a `.jar`'s manifest declares.

pub mod flash_save;
pub mod flat_json;
pub mod j2me_save;
pub mod jar;
pub mod keys;
