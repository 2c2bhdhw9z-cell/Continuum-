//! Amiibo dumps: checking a file is one, and the honest answer about whether a core can take it.
//!
//! ## What a dump is
//!
//! An NTAG215 tag image. 540 bytes (0x21C, 135 pages of 4) is the whole tag; 572 is the same
//! with a 32-byte signature appended by some dumping tools. The layout below is the one Azahar
//! itself checks in `AmiiboCrypto::IsAmiiboValid` (src/core/hle/service/nfc/amiibo_crypto.cpp)
//! against the struct in nfc_types.h, so a file that passes here is a file Azahar's own loader
//! would accept.
//!
//! ## Whether a core can receive one
//!
//! Read at azahar-emu/azahar commit 86a9f92: the NFC service can load a tag from a file
//! (`NfcDevice::LoadAmiibo(path)`), but nothing in `src/citra_libretro/` ever calls it. There is
//! no core option, no file the core polls for in the system directory, and no libretro extension.
//! The desktop and Android frontends call `LoadAmiibo` from their own menus, which a libretro
//! frontend cannot reach. So today NO core in this app can be handed an Amiibo, and
//! [`support_for_core`] says so in words the app can put on screen. The import, the folder and the
//! picker are real; the moment a core grows a way in, only [`support_for_core`] and the tap path
//! have to change.

/// The whole NTAG215 image.
pub const NTAG215_SIZE: usize = 0x21C;
/// The image plus a 32-byte signature.
pub const NTAG215_SIGNED_SIZE: usize = NTAG215_SIZE + 32;

/// What a dump says about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmiiboInfo {
    /// The 7-byte tag UID, hex, upper case.
    pub uid_hex: String,
    /// The 8-byte Amiibo identification block at page 21 (game/character, variant, type, model,
    /// series, format), hex, upper case. This is the id every Amiibo database is keyed by.
    pub amiibo_id_hex: String,
    pub character_id: u16,
    pub variant: u8,
    pub figure_type: u8,
    pub model_number: u16,
    pub series: u8,
    pub size: usize,
    pub signed: bool,
    /// Checks that failed but did not make the file unusable, in plain English.
    pub warnings: Vec<String>,
}

/// Checks a dump and reads its identity. `Err` carries the sentence to show the user.
pub fn inspect(data: &[u8]) -> Result<AmiiboInfo, String> {
    if data.len() != NTAG215_SIZE && data.len() != NTAG215_SIGNED_SIZE {
        return Err(format!(
            "this file is {} bytes, and an Amiibo dump is {NTAG215_SIZE} or {NTAG215_SIGNED_SIZE}",
            data.len()
        ));
    }

    // Capability container, page 3: F1 10 FF EE. Every NTAG215 has it, so a file without it is not
    // a tag image at all, whatever its size.
    if data[0x0C..0x10] != [0xF1, 0x10, 0xFF, 0xEE] {
        return Err(
            "this file is the right size but is not an NTAG215 tag image (its capability \
             container is wrong)"
                .into(),
        );
    }
    // The format byte at the end of the identification block. 0x02 on every Amiibo.
    if data[0x5B] != 0x02 {
        return Err(format!(
            "this file is an NTAG215 image but not an Amiibo (format byte {:#04x}, expected 0x02)",
            data[0x5B]
        ));
    }

    let mut warnings = Vec::new();
    // ISO/IEC 14443-3 check bytes over the UID, as Azahar checks them.
    const CASCADE_TAG: u8 = 0x88;
    if CASCADE_TAG ^ data[0] ^ data[1] ^ data[2] != data[3] {
        warnings.push("the first UID check byte does not match".to_string());
    }
    if data[4] ^ data[5] ^ data[6] ^ data[7] != data[8] {
        warnings.push("the second UID check byte does not match".to_string());
    }
    if data[0x0A..0x0C] != [0x0F, 0xE0] {
        warnings.push("the static lock bytes are not the usual 0F E0".to_string());
    }
    if data[0x208..0x20B] != [0x01, 0x00, 0x0F] {
        warnings.push("the dynamic lock bytes are not the usual 01 00 0F".to_string());
    }
    if data[0x20C..0x210] != [0x00, 0x00, 0x00, 0x04] {
        warnings.push("CFG0 is not the usual 00 00 00 04".to_string());
    }
    if data[0x210] != 0x5F {
        warnings.push("CFG1 is not the usual 5F".to_string());
    }

    let uid: Vec<u8> = data[0..3].iter().chain(&data[4..8]).copied().collect();
    let identification = &data[0x54..0x5C];
    Ok(AmiiboInfo {
        uid_hex: hex(&uid),
        amiibo_id_hex: hex(identification),
        character_id: u16::from_be_bytes([identification[0], identification[1]]),
        variant: identification[2],
        figure_type: identification[3],
        model_number: u16::from_be_bytes([identification[4], identification[5]]),
        series: identification[6],
        size: data.len(),
        signed: data.len() == NTAG215_SIGNED_SIZE,
        warnings,
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// Whether a core can be handed an Amiibo, and the sentence explaining the answer.
pub fn support_for_core(core_id: &str) -> (bool, String) {
    match core_id {
        "azahar" => (
            false,
            "Azahar's libretro build cannot receive an Amiibo yet: its NFC service can read a tag \
             file, but the libretro frontend never passes one in, and there is no core option or \
             file it watches. Your Amiibo is saved in the Amiibo folder and will work once the \
             core adds a way in."
                .into(),
        ),
        "melonds" => (
            false,
            "The DS has no NFC reader, so DS games never ask for an Amiibo.".into(),
        ),
        "" => (false, "no game is running".into()),
        other => (false, format!("the {other} core has no Amiibo support")),
    }
}

/// What happens when the user taps an Amiibo in the in-game picker: a readable answer, always.
///
/// The file is inspected first, so a damaged file says that rather than the support sentence.
pub fn tap(core_id: &str, file_name: &str, data: &[u8]) -> String {
    let info = match inspect(data) {
        Ok(info) => info,
        Err(reason) => return format!("amiibo {file_name}: {reason}"),
    };
    let (supported, reason) = support_for_core(core_id);
    if supported {
        // Unreachable today; kept so the shape of the success line is decided now.
        format!("amiibo {file_name} ({}) tapped", info.amiibo_id_hex)
    } else {
        format!(
            "amiibo {file_name} ({}) not tapped: {reason}",
            info.amiibo_id_hex
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic but structurally correct NTAG215 Amiibo image.
    fn sample(size: usize) -> Vec<u8> {
        let mut data = vec![0u8; size];
        let uid = [0x04, 0xA1, 0xB2, 0xC3, 0xD4, 0xE5, 0xF6];
        data[0..3].copy_from_slice(&uid[0..3]);
        data[3] = 0x88 ^ uid[0] ^ uid[1] ^ uid[2];
        data[4..8].copy_from_slice(&uid[3..7]);
        data[8] = uid[3] ^ uid[4] ^ uid[5] ^ uid[6];
        data[0x0A] = 0x0F;
        data[0x0B] = 0xE0;
        data[0x0C..0x10].copy_from_slice(&[0xF1, 0x10, 0xFF, 0xEE]);
        // Mario, Super Smash Bros. series: 00 00 00 00 00 00 00 02.
        data[0x54..0x5C].copy_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02]);
        data[0x208..0x20C].copy_from_slice(&[0x01, 0x00, 0x0F, 0xBD]);
        data[0x20C..0x210].copy_from_slice(&[0x00, 0x00, 0x00, 0x04]);
        data[0x210] = 0x5F;
        data
    }

    #[test]
    fn sizes_match_the_azahar_struct() {
        assert_eq!(NTAG215_SIZE, 540);
        assert_eq!(NTAG215_SIGNED_SIZE, 572);
    }

    #[test]
    fn a_clean_dump_reads_without_warnings() {
        let info = inspect(&sample(540)).unwrap();
        assert_eq!(info.uid_hex, "04A1B2C3D4E5F6");
        assert_eq!(info.amiibo_id_hex, "0000000000000002");
        assert!(info.warnings.is_empty(), "{:?}", info.warnings);
        assert!(!info.signed);
        assert!(inspect(&sample(572)).unwrap().signed);
    }

    #[test]
    fn identification_fields_are_big_endian() {
        let mut data = sample(540);
        data[0x54..0x5C].copy_from_slice(&[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x02]);
        let info = inspect(&data).unwrap();
        assert_eq!(info.character_id, 0x0102);
        assert_eq!(info.variant, 3);
        assert_eq!(info.figure_type, 4);
        assert_eq!(info.model_number, 0x0506);
        assert_eq!(info.series, 7);
    }

    #[test]
    fn wrong_sizes_and_non_tags_are_refused_with_a_reason() {
        assert!(inspect(&[0; 532]).unwrap_err().contains("532 bytes"));
        assert!(inspect(&[0; 540]).unwrap_err().contains("capability container"));
        let mut not_amiibo = sample(540);
        not_amiibo[0x5B] = 0x00;
        assert!(inspect(&not_amiibo).unwrap_err().contains("not an Amiibo"));
    }

    #[test]
    fn bad_check_bytes_are_warnings_not_refusals() {
        let mut data = sample(540);
        data[3] ^= 0xFF;
        data[0x210] = 0;
        let info = inspect(&data).unwrap();
        assert_eq!(info.warnings.len(), 2);
    }

    #[test]
    fn no_core_claims_support_and_the_tap_says_why() {
        let (supported, reason) = support_for_core("azahar");
        assert!(!supported);
        assert!(reason.contains("cannot receive"));
        let line = tap("azahar", "mario.bin", &sample(540));
        assert!(line.contains("not tapped"));
        assert!(line.contains("0000000000000002"));
        assert!(tap("azahar", "junk.bin", &[0; 3]).contains("3 bytes"));
        assert!(!support_for_core("").0);
    }
}
