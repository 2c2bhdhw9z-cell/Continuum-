//! Events: the event tickets Nintendo handed out for Ruby to LeafGreen, as G1R Deluxe's Events
//! tool offers them: the Eon Ticket (Southern Island: Latias or Latios), the AuroraTicket (Birth
//! Island: Deoxys), the MysticTicket (Navel Rock: Lugia and Ho-Oh) and the Old Sea Map (Faraway
//! Island: Mew), each only for the games and languages it really went to.
//!
//! What the game itself checks is set, from pret's decompilations (the ferry scripts at Lilycove
//! Harbor and Vermilion City): the ticket in the Key Items pocket and the flag that opens the
//! ship's route, and the "received" flag the Wonder Card would have set.

use super::items;
use super::mon::Game;

pub struct Event {
    pub id: &'static str,
    pub name: &'static str,
    pub place: &'static str,
    /// The ticket's number in Ruby to LeafGreen.
    pub item: u16,
}

pub const EVENTS: [Event; 4] = [
    Event { id: "eon", name: "Eon Ticket", place: "Southern Island", item: 275 },
    Event { id: "aurora", name: "AuroraTicket", place: "Birth Island", item: 371 },
    Event { id: "mystic", name: "MysticTicket", place: "Navel Rock", item: 370 },
    Event { id: "oldseamap", name: "Old Sea Map", place: "Faraway Island", item: 376 },
];

/// The flags for `id` in `game`: (opens the route, the Wonder Card's "received"), by pret's
/// numbers. None when the event never went to that game.
fn flags(id: &str, game: Game) -> Option<(u16, Option<u16>)> {
    use Game::*;
    Some(match (id, game) {
        ("eon", Ruby | Sapphire) => (0x853, None),
        ("eon", Emerald) => (0x8B3, None),
        ("aurora", FireRed | LeafGreen) => (0x84B, Some(0x2A7)),
        ("aurora", Emerald) => (0x8D5, Some(0x13A)),
        ("mystic", FireRed | LeafGreen) => (0x84A, Some(0x2A8)),
        ("mystic", Emerald) => (0x8E0, Some(0x13B)),
        ("oldseamap", Emerald) => (0x8D6, Some(0x13C)),
        _ => return None,
    })
}

/// Whether the event really went to this game in this language (1 Japanese, 2 English, 3
/// French, 4 Italian, 5 German, 7 Spanish), as G1R Deluxe lists them.
fn historical(id: &str, game: Game, language: u8) -> bool {
    let western = matches!(language, 2 | 3 | 4 | 5 | 7);
    match id {
        "eon" => true,
        "aurora" => game != Game::Emerald || western,
        "mystic" => matches!(language, 1 | 2),
        "oldseamap" => language == 1,
        _ => false,
    }
}

/// Where the ticket is used, for the player.
pub fn how(game: Game) -> &'static str {
    match game {
        Game::FireRed | Game::LeafGreen => "Board the Seagallop at Vermilion City's harbor once the Sevii Islands as far as Rainbow Pass are open, and show the ticket.",
        _ => "Show the ticket at Lilycove City's harbor once you have beaten the Elite Four.",
    }
}

/// The events for this game: each with whether it is already in the save.
pub fn list(game: Game, language: u8, save: &[u8]) -> Result<Vec<(&'static Event, bool)>, String> {
    let keys = items::key_items(game, save)?;
    let mut out = Vec::new();
    for e in &EVENTS {
        let Some((open, received)) = flags(e.id, game) else { continue };
        if !historical(e.id, game, language) {
            continue;
        }
        let have = keys.contains(&e.item) || items::flag(game, save, open)?
            || received.map(|f| items::flag(game, save, f)).transpose()?.unwrap_or(false);
        out.push((e, have));
    }
    Ok(out)
}

/// Gives the save an event's ticket.
pub fn receive(game: Game, language: u8, save: &mut [u8], id: &str) -> Result<String, String> {
    let e = EVENTS.iter().find(|e| e.id == id).ok_or("There's no such event.")?;
    let (open, received) = flags(id, game).ok_or_else(|| format!("The {} never went to Pokémon {}.", e.name, game.name()))?;
    if !historical(id, game, language) {
        return Err(format!("The {} was never handed out for this language of Pokémon {}.", e.name, game.name()));
    }
    if list(game, language, save)?.iter().any(|(x, have)| x.id == id && *have) {
        return Err(format!("This save already has the {}.", e.name));
    }
    let block = items::current_block(game, save)?.ok_or("Only Ruby to LeafGreen have event tickets.")?;
    items::add_key_item(game, save, e.item)?;
    items::set_flag(game, save, &block, open)?;
    if let Some(f) = received {
        items::set_flag(game, save, &block, f)?;
    }
    items::seal_block(game, save, Some(block))?;
    Ok(format!("The {} is in the Key Items pocket. {}", e.name, how(game)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pokebox::save::{self, tests::blank};

    #[test]
    fn a_ticket_goes_into_the_key_items_with_its_flags_and_only_once() {
        let mut s = blank(Game::Emerald);
        let ids: Vec<_> = list(Game::Emerald, 2, &s).unwrap().iter().map(|(e, _)| e.id).collect();
        assert_eq!(ids, ["eon", "aurora", "mystic"], "the Old Sea Map only went to Japan");
        receive(Game::Emerald, 2, &mut s, "aurora").unwrap();
        save::check(Game::Emerald, &mut s).unwrap();
        assert!(items::key_items(Game::Emerald, &s).unwrap().contains(&371));
        assert!(items::flag(Game::Emerald, &s, 0x8D5).unwrap() && items::flag(Game::Emerald, &s, 0x13A).unwrap());
        assert!(receive(Game::Emerald, 2, &mut s, "aurora").is_err());
        let mut f = blank(Game::FireRed);
        assert!(receive(Game::FireRed, 2, &mut f, "eon").is_err(), "the Eon Ticket never went to FireRed");
        receive(Game::FireRed, 2, &mut f, "mystic").unwrap();
        save::check(Game::FireRed, &mut f).unwrap();
        assert!(items::flag(Game::FireRed, &f, 0x84A).unwrap());
    }
}
