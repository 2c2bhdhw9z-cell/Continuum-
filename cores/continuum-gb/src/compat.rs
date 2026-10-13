//! What a Game Boy Color's start-up code does for a black-and-white cartridge, which is how a
//! Color colours a game that never knew about colour.
//!
//! The start-up code checks that the game is Nintendo's own, adds up the sixteen bytes of its
//! title, and looks the sum up in a table of the games it knows. Fourteen sums are shared by more
//! than one title, and for those the title's fourth letter tells them apart. The answer is three
//! palettes - one for the background and one for each of the two object palettes - which it
//! writes into the Color's palette memory before switching the console into Game Boy mode. From
//! then on the game's own BGP, OBP0 and OBP1 pick which of those colours each shade becomes. A
//! game it does not know, or one that is not Nintendo's, gets one fixed set. While the logo is up
//! the player can override the pick by holding one of twelve button combinations.
//!
//! The algorithm is Pan Docs' ("Power Up Sequence", "Compatibility palettes"). The table and the
//! twelve button schemes are The Cutting Room Floor's ("Notes:Game Boy Color Bootstrap ROM", CC BY
//! 3.0), which lists, for every palette set the start-up code holds, the title sums that pick it
//! and the colours, transcribed from the start-up code's own tables. The colours are kept here
//! exactly as that page writes them; `rgb15` turns them into what the palette memory holds. One
//! row is corrected: see Radar Mission below.

/// Three palettes, lightest colour first: the background's, then the objects' that OBP0 and OBP1
/// index. Colours are 24-bit, as the documentation gives them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palettes {
    pub bg: [u32; 4],
    pub obj0: [u32; 4],
    pub obj1: [u32; 4],
}

const fn p(bg: [u32; 4], obj0: [u32; 4], obj1: [u32; 4]) -> Palettes {
    Palettes { bg, obj0, obj1 }
}

/// A title the start-up code knows: by the sum of its title bytes alone, or for a sum more than
/// one game shares, by the sum and the title's fourth letter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Title {
    Sum(u8),
    Letter(u8, u8),
}
use Title::{Letter, Sum};

/// What every game the start-up code does not pick for gets: a title it does not know, a shared
/// sum whose fourth letter matches none of the games that share it, and every game that is not
/// Nintendo's. It is also the Right + A button scheme.
pub const DEFAULT: Palettes = p([0xFFFFFF, 0x7BFF31, 0x0063C5, 0x000000],
    [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000],
    [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000]);

/// Every palette set the start-up code can pick by itself, with the titles that pick it, in the
/// documentation's order. The comment on each row is the documentation's entry and shuffle
/// numbers and the games those titles belong to.
const KNOWN: &[(&[Title], Palettes)] = &[
    // 00/3: Moguranya, Mole Mania
    (&[Letter(0xB3, b'U')], p([0xFFFFFF, 0xADAD84, 0x42737B, 0x000000],
        [0xFFFFFF, 0xFF7300, 0x944200, 0x000000],
        [0xFFFFFF, 0xFF7300, 0x944200, 0x000000])),
    // 00/5: Game Boy Wars, Wario Land - Super Mario Land 3
    (&[Sum(0x59), Letter(0xC6, b'A')], p([0xFFFFFF, 0xADAD84, 0x42737B, 0x000000],
        [0xFFFFFF, 0xFF7300, 0x944200, 0x000000],
        [0xFFFFFF, 0x5ABDFF, 0xFF0000, 0x0000FF])),
    // 00/1: Radar Mission. The documentation files this title under entry 01 with shuffle 0, but
    // the start-up code's own byte for it is $20 - entry 00 with shuffle 1, the two numbers the
    // other way round - which is these colours; SameBoy's start-up code picks the same three.
    (&[Sum(0x8C)], p([0xFFFFFF, 0xADAD84, 0x42737B, 0x000000],
        [0xFFFFFF, 0xFF7300, 0x944200, 0x000000],
        [0xFFFFFF, 0xADAD84, 0x42737B, 0x000000])),
    // 01/5: Donkey Kong Land, Super Donkey Kong GB
    (&[Sum(0x86), Sum(0xA8)], p([0xFFFF9C, 0x94B5FF, 0x639473, 0x003A3A],
        [0xFFC542, 0xFFD600, 0x943A00, 0x4A0000],
        [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000])),
    // 02/5: Soccer, Tennis, Top Rank Tennis, Top Ranking Tennis
    (&[Letter(0xBF, b'C'), Sum(0xCE), Sum(0xD1), Sum(0xF0)], p([0x6BFF00, 0xFFFFFF, 0xFF524A, 0x000000],
        [0xFFFFFF, 0xFFFFFF, 0x63A5FF, 0x0000FF],
        [0xFFFFFF, 0xFFAD63, 0x843100, 0x000000])),
    // 03/5: Baseball
    (&[Sum(0x36)], p([0x52DE00, 0xFF8400, 0xFFFF00, 0xFFFFFF],
        [0xFFFFFF, 0xFFFFFF, 0x63A5FF, 0x0000FF],
        [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000])),
    // 04/3: Game & Watch Gallery, Game Boy Gallery, Game Boy Gallery 2
    (&[Sum(0x34), Letter(0x66, b'E'), Letter(0xF4, b' ')], p([0xFFFFFF, 0x7BFF00, 0xB57300, 0x000000],
        [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000],
        [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000])),
    // 05/3: Mario & Yoshi, Yoshi, Yoshi no Tamago
    (&[Sum(0x3D), Letter(0x6A, b'I')], p([0xFFFFFF, 0x52FF00, 0xFF4200, 0x000000],
        [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000],
        [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000])),
    // 05/4: Tetris Attack, Yoshi no Panepon
    (&[Sum(0x95), Letter(0xB3, b'R')], p([0xFFFFFF, 0x52FF00, 0xFF4200, 0x000000],
        [0xFFFFFF, 0x52FF00, 0xFF4200, 0x000000],
        [0xFFFFFF, 0x5ABDFF, 0xFF0000, 0x0000FF])),
    // 06/0: Balloon Kid, Tetris Blast
    (&[Sum(0x71), Sum(0xFF)], p([0xFFFFFF, 0xFF9C00, 0xFF0000, 0x000000],
        [0xFFFFFF, 0xFF9C00, 0xFF0000, 0x000000],
        [0xFFFFFF, 0xFF9C00, 0xFF0000, 0x000000])),
    // 06/3: Donkey Kong
    (&[Sum(0x19)], p([0xFFFFFF, 0xFF9C00, 0xFF0000, 0x000000],
        [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000],
        [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000])),
    // 06/4: Yoshi no Cookie, Yoshi's Cookie
    (&[Sum(0x3E), Sum(0xE0)], p([0xFFFFFF, 0xFF9C00, 0xFF0000, 0x000000],
        [0xFFFFFF, 0xFF9C00, 0xFF0000, 0x000000],
        [0xFFFFFF, 0x5ABDFF, 0xFF0000, 0x0000FF])),
    // 07/0: Pocket Monsters - Pikachu, Tetris
    (&[Sum(0x15), Sum(0xDB)], p([0xFFFFFF, 0xFFFF00, 0xFF0000, 0x000000],
        [0xFFFFFF, 0xFFFF00, 0xFF0000, 0x000000],
        [0xFFFFFF, 0xFFFF00, 0xFF0000, 0x000000])),
    // 07/4: Qix, Tetris 2, Tetris Flash
    (&[Letter(0x0D, b'R'), Sum(0x69), Sum(0xF2)], p([0xFFFFFF, 0xFFFF00, 0xFF0000, 0x000000],
        [0xFFFFFF, 0xFFFF00, 0xFF0000, 0x000000],
        [0xFFFFFF, 0x5ABDFF, 0xFF0000, 0x0000FF])),
    // 08/0: Alleyway
    (&[Sum(0x88)], p([0xA59CFF, 0xFFFF00, 0x006300, 0x000000],
        [0xA59CFF, 0xFFFF00, 0x006300, 0x000000],
        [0xA59CFF, 0xFFFF00, 0x006300, 0x000000])),
    // 08/3: Kirby no Pinball, Kirby's Pinball Land
    (&[Sum(0x1D)], p([0xA59CFF, 0xFFFF00, 0x006300, 0x000000],
        [0xFF6352, 0xD60000, 0x630000, 0x000000],
        [0xFF6352, 0xD60000, 0x630000, 0x000000])),
    // 08/5: Hoshi no Kirby 1 and 2, Kirby's Dream Land 1 and 2, Kirby's Block Ball
    (&[Letter(0x27, b'B'), Sum(0x49), Sum(0x5C), Letter(0xB3, b'B')], p([0xA59CFF, 0xFFFF00, 0x006300, 0x000000],
        [0xFF6352, 0xD60000, 0x630000, 0x000000],
        [0x0000FF, 0xFFFFFF, 0xFFFF7B, 0x0084FF])),
    // 09/5: Super Mario Land 2 - 6 Golden Coins
    (&[Sum(0xC9)], p([0xFFFFCE, 0x63EFEF, 0x9C8431, 0x5A5A5A],
        [0xFFFFFF, 0xFF7300, 0x944200, 0x000000],
        [0xFFFFFF, 0x63A5FF, 0x0000FF, 0x000000])),
    // 0A/3: Super Mario Land
    (&[Letter(0x46, b'E')], p([0xB5B5FF, 0xFFFF94, 0xAD5A42, 0x000000],
        [0x000000, 0xFFFFFF, 0xFF8484, 0x943A3A],
        [0x000000, 0xFFFFFF, 0xFF8484, 0x943A3A])),
    // 0B/1: Pokemon Blue, Pocket Monsters Ao
    (&[Letter(0x61, b'E')], p([0xFFFFFF, 0x63A5FF, 0x0000FF, 0x000000],
        [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000],
        [0xFFFFFF, 0x63A5FF, 0x0000FF, 0x000000])),
    // 0B/2: Dr. Mario
    (&[Sum(0x3C)], p([0xFFFFFF, 0x63A5FF, 0x0000FF, 0x000000],
        [0xFFFFFF, 0x63A5FF, 0x0000FF, 0x000000],
        [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000])),
    // 0B/5: Wave Race
    (&[Sum(0x4E)], p([0xFFFFFF, 0x63A5FF, 0x0000FF, 0x000000],
        [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000],
        [0xFFFFFF, 0xFFFF7B, 0x0084FF, 0xFF0000])),
    // 0C/2: Pinocchio
    (&[Sum(0x9C)], p([0xFFFFFF, 0x8C8CDE, 0x52528C, 0x000000],
        [0xFFFFFF, 0x8C8CDE, 0x52528C, 0x000000],
        [0xFFC542, 0xFFD600, 0x943A00, 0x4A0000])),
    // 0C/3: Pocket Bomberman
    (&[Letter(0x0D, b'E')], p([0xFFFFFF, 0x8C8CDE, 0x52528C, 0x000000],
        [0xFFC542, 0xFFD600, 0x943A00, 0x4A0000],
        [0xFFC542, 0xFFD600, 0x943A00, 0x4A0000])),
    // 0C/5: Donkey Kong Land (Japan), Donkey Kong Land 2, Donkey Kong Land III
    (&[Letter(0x18, b'K'), Letter(0x6A, b'K'), Sum(0x6B)], p([0xFFFFFF, 0x8C8CDE, 0x52528C, 0x000000],
        [0xFFC542, 0xFFD600, 0x943A00, 0x4A0000],
        [0xFFFFFF, 0x5ABDFF, 0xFF0000, 0x0000FF])),
    // 0D/1: Kaeru no Tame ni Kane wa Naru
    (&[Letter(0xD3, b'R')], p([0xFFFFFF, 0x8C8CDE, 0x52528C, 0x000000],
        [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000],
        [0xFFFFFF, 0x8C8CDE, 0x52528C, 0x000000])),
    // 0D/3: Kid Icarus - Of Myths and Monsters
    (&[Letter(0xBF, b' ')], p([0xFFFFFF, 0x8C8CDE, 0x52528C, 0x000000],
        [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000],
        [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000])),
    // 0D/5: Killer Instinct
    (&[Sum(0x9D)], p([0xFFFFFF, 0x8C8CDE, 0x52528C, 0x000000],
        [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000],
        [0xFFFFFF, 0xFFAD63, 0x843100, 0x000000])),
    // 0E/3: Asteroids & Missile Command, Golf, Nintendo World Cup, Play Action Football, Toy Story
    (&[Letter(0x28, b'F'), Sum(0x4B), Sum(0x90), Sum(0x9A), Sum(0xBD)], p([0xFFFFFF, 0x7BFF31, 0x008400, 0x000000],
        [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000],
        [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000])),
    // 0E/5: Magnetic Soccer, Mystic Quest, Othello, Vegas Stakes
    (&[Sum(0x17), Letter(0x27, b'N'), Letter(0x61, b'A'), Sum(0x8B)], p([0xFFFFFF, 0x7BFF31, 0x008400, 0x000000],
        [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000],
        [0xFFFFFF, 0x63A5FF, 0x0000FF, 0x000000])),
    // 0F/3: The Chessmaster, Dynablaster, King of the Zoo
    (&[Sum(0x39), Sum(0x43), Sum(0x97)], p([0xFFFFFF, 0xFFAD63, 0x843100, 0x000000],
        [0xFFFFFF, 0x63A5FF, 0x0000FF, 0x000000],
        [0xFFFFFF, 0x63A5FF, 0x0000FF, 0x000000])),
    // 0F/5: Adventures of Lolo, Defender & Joust, Battle Arena Toshinden, The King of Fighters '95,
    // Mega Man 1 to 3, Street Fighter II, Super R.C. Pro-Am
    (&[Sum(0x01), Sum(0x10), Sum(0x29), Sum(0x52), Sum(0x5D), Sum(0x68), Sum(0x6D), Sum(0xF6)],
        p([0xFFFFFF, 0xFFAD63, 0x843100, 0x000000],
        [0xFFFFFF, 0x63A5FF, 0x0000FF, 0x000000],
        [0xFFFFFF, 0x7BFF31, 0x008400, 0x000000])),
    // 10/1: Pokemon Red, Pocket Monsters Aka, Game Boy Camera Gold
    (&[Sum(0x14)], p([0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000],
        [0xFFFFFF, 0x7BFF31, 0x008400, 0x000000],
        [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000])),
    // 11/5: The Legend of Zelda - Link's Awakening, Zelda no Densetsu - Yume o Miru Shima
    (&[Sum(0x70)], p([0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000],
        [0xFFFFFF, 0x00FF00, 0x318400, 0x004A00],
        [0xFFFFFF, 0x63A5FF, 0x0000FF, 0x000000])),
    // 12/0: F-1 Race, Game Boy Gallery (Europe), Kirby's Star Stacker, Mario's Picross, Picross 2,
    // Nigel Mansell's World Championship Racing, Yakuman
    (&[Sum(0x0C), Sum(0x16), Sum(0x35), Sum(0x67), Sum(0x75), Sum(0x92), Sum(0x99), Sum(0xB7)],
        p([0xFFFFFF, 0xFFAD63, 0x843100, 0x000000],
        [0xFFFFFF, 0xFFAD63, 0x843100, 0x000000],
        [0xFFFFFF, 0xFFAD63, 0x843100, 0x000000])),
    // 12/3: Battletoads in Ragnarok's World
    (&[Letter(0xA5, b'R')], p([0xFFFFFF, 0xFFAD63, 0x843100, 0x000000],
        [0xFFFFFF, 0x7BFF31, 0x008400, 0x000000],
        [0xFFFFFF, 0x7BFF31, 0x008400, 0x000000])),
    // 12/5: A Boy and His Blob in the Rescue of Princess Blobette, Star Wars
    (&[Sum(0xA2), Sum(0xF7)], p([0xFFFFFF, 0xFFAD63, 0x843100, 0x000000],
        [0xFFFFFF, 0x7BFF31, 0x008400, 0x000000],
        [0xFFFFFF, 0x63A5FF, 0x0000FF, 0x000000])),
    // 13/0: Galaga & Galaxian, Solar Striker, Space Invaders
    (&[Letter(0x28, b'A'), Letter(0xA5, b'A'), Sum(0xE8)], p([0x000000, 0x008484, 0xFFDE00, 0xFFFFFF],
        [0x000000, 0x008484, 0xFFDE00, 0xFFFFFF],
        [0x000000, 0x008484, 0xFFDE00, 0xFFFFFF])),
    // 14/5: Metroid II - Return of Samus
    (&[Letter(0x46, b'R')], p([0xFFFFFF, 0x63A5FF, 0x0000FF, 0x000000],
        [0xFFFF00, 0xFF0000, 0x630000, 0x000000],
        [0xFFFFFF, 0x7BFF31, 0x008400, 0x000000])),
    // 15/5: Wario Land II
    (&[Letter(0xD3, b'I')], p([0xFFFFFF, 0xADAD84, 0x42737B, 0x000000],
        [0xFFFFFF, 0xFFAD63, 0x843100, 0x000000],
        [0xFFFFFF, 0x63A5FF, 0x0000FF, 0x000000])),
    // 16/0: X
    (&[Sum(0x58)], p([0xFFFFFF, 0xA5A5A5, 0x525252, 0x000000],
        [0xFFFFFF, 0xA5A5A5, 0x525252, 0x000000],
        [0xFFFFFF, 0xA5A5A5, 0x525252, 0x000000])),
    // 1B/0: Pocket Camera
    (&[Sum(0x6F)], p([0xFFFFFF, 0xFFCE00, 0x9C6300, 0x000000],
        [0xFFFFFF, 0xFFCE00, 0x9C6300, 0x000000],
        [0xFFFFFF, 0xFFCE00, 0x9C6300, 0x000000])),
    // 1C/1: James Bond 007, Pocket Monsters Midori
    (&[Sum(0xAA)], p([0xFFFFFF, 0x7BFF31, 0x0063C5, 0x000000],
        [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000],
        [0xFFFFFF, 0x7BFF31, 0x0063C5, 0x000000])),
    // 1C/3: Centipede & Millipede, Ken Griffey Jr. presents Major League Baseball, Tetris Plus,
    // Wario Blast. The sum 00 is the table's own "no game" entry, so this is `DEFAULT` too.
    (&[Sum(0x00), Letter(0x18, b'I'), Sum(0x3F), Letter(0x66, b'L'), Letter(0xC6, b' ')], DEFAULT),
    // 1C/5: Pac-In-Time
    (&[Letter(0xF4, b'-')], p([0xFFFFFF, 0x7BFF31, 0x0063C5, 0x000000],
        [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000],
        [0xFFFFFF, 0x63A5FF, 0x0000FF, 0x000000])),
];

/// One of the twelve schemes a player picks on a real Color by holding buttons while its logo is
/// up, with those buttons as cBoy's pad byte holds them (bit 0 A, 1 B, 4 Right, 5 Left, 6 Up,
/// 7 Down). The start-up code compares the whole pad, so it is exactly these buttons and no others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pick {
    pub buttons: u8,
    pub palettes: Palettes,
}

const UP: u8 = 0x40;
const DOWN: u8 = 0x80;
const LEFT: u8 = 0x20;
const RIGHT: u8 = 0x10;
const A: u8 = 0x01;
const B: u8 = 0x02;

/// The twelve, in the order the start-up code keeps them (the documentation's "Manual Select
/// Palette Configurations"): Up, Up + A, Up + B, Left, Left + A, Left + B, Down, Down + A,
/// Down + B, Right, Right + A, Right + B. Six of them are sets no game gets by itself.
pub const PICKS: [Pick; 12] = [
    // Up: 12/0, brown
    Pick { buttons: UP, palettes: p([0xFFFFFF, 0xFFAD63, 0x843100, 0x000000],
        [0xFFFFFF, 0xFFAD63, 0x843100, 0x000000],
        [0xFFFFFF, 0xFFAD63, 0x843100, 0x000000]) },
    // Up + A: 10/5, red
    Pick { buttons: UP | A, palettes: p([0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000],
        [0xFFFFFF, 0x7BFF31, 0x008400, 0x000000],
        [0xFFFFFF, 0x63A5FF, 0x0000FF, 0x000000]) },
    // Up + B: 19/3, dark brown
    Pick { buttons: UP | B, palettes: p([0xFFE6C5, 0xCE9C84, 0x846B29, 0x5A3108],
        [0xFFFFFF, 0xFFAD63, 0x843100, 0x000000],
        [0xFFFFFF, 0xFFAD63, 0x843100, 0x000000]) },
    // Left: 18/5, blue
    Pick { buttons: LEFT, palettes: p([0xFFFFFF, 0x63A5FF, 0x0000FF, 0x000000],
        [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000],
        [0xFFFFFF, 0x7BFF31, 0x008400, 0x000000]) },
    // Left + A: 0D/5, dark blue
    Pick { buttons: LEFT | A, palettes: p([0xFFFFFF, 0x8C8CDE, 0x52528C, 0x000000],
        [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000],
        [0xFFFFFF, 0xFFAD63, 0x843100, 0x000000]) },
    // Left + B: 16/0, grey
    Pick { buttons: LEFT | B, palettes: p([0xFFFFFF, 0xA5A5A5, 0x525252, 0x000000],
        [0xFFFFFF, 0xA5A5A5, 0x525252, 0x000000],
        [0xFFFFFF, 0xA5A5A5, 0x525252, 0x000000]) },
    // Down: 17/0, pastel
    Pick { buttons: DOWN, palettes: p([0xFFFFA5, 0xFF9494, 0x9494FF, 0x000000],
        [0xFFFFA5, 0xFF9494, 0x9494FF, 0x000000],
        [0xFFFFA5, 0xFF9494, 0x9494FF, 0x000000]) },
    // Down + A: 07/0, orange
    Pick { buttons: DOWN | A, palettes: p([0xFFFFFF, 0xFFFF00, 0xFF0000, 0x000000],
        [0xFFFFFF, 0xFFFF00, 0xFF0000, 0x000000],
        [0xFFFFFF, 0xFFFF00, 0xFF0000, 0x000000]) },
    // Down + B: 1A/5, yellow
    Pick { buttons: DOWN | B, palettes: p([0xFFFFFF, 0xFFFF00, 0x7B4A00, 0x000000],
        [0xFFFFFF, 0x63A5FF, 0x0000FF, 0x000000],
        [0xFFFFFF, 0x7BFF31, 0x008400, 0x000000]) },
    // Right: 05/0, green
    Pick { buttons: RIGHT, palettes: p([0xFFFFFF, 0x52FF00, 0xFF4200, 0x000000],
        [0xFFFFFF, 0x52FF00, 0xFF4200, 0x000000],
        [0xFFFFFF, 0x52FF00, 0xFF4200, 0x000000]) },
    // Right + A: 1C/3, dark green, the same set as `DEFAULT`
    Pick { buttons: RIGHT | A, palettes: DEFAULT },
    // Right + B: 13/0, reversed
    Pick { buttons: RIGHT | B, palettes: p([0x000000, 0x008484, 0xFFDE00, 0xFFFFFF],
        [0x000000, 0x008484, 0xFFDE00, 0xFFFFFF],
        [0x000000, 0x008484, 0xFFDE00, 0xFFFFFF]) },
];

/// The sum of the sixteen title bytes, which is what the start-up code looks a game up by, or
/// None when the game is not Nintendo's own and it does not look at all. Nintendo's is the old
/// licensee code $01, or $33 (which says the code is in the new field) with the new one "01".
pub fn title_checksum(rom: &[u8]) -> Option<u8> {
    let header = rom.get(0x134..0x14C)?;
    let nintendo = match header[0x14B - 0x134] {
        0x33 => rom.get(0x144..0x146) == Some(b"01".as_slice()),
        old => old == 0x01,
    };
    nintendo.then(|| header[..16].iter().fold(0u8, |sum, b| sum.wrapping_add(*b)))
}

/// The palettes the start-up code picks for this cartridge by itself, with no buttons held.
pub fn picked(rom: &[u8]) -> Palettes {
    let Some(sum) = title_checksum(rom) else { return DEFAULT };
    let fourth = rom[0x137];
    KNOWN
        .iter()
        .find(|(titles, _)| titles.iter().any(|t| *t == Sum(sum) || *t == Letter(sum, fourth)))
        .map_or(DEFAULT, |(_, palettes)| *palettes)
}

/// Every title the start-up code knows, as (title sum, fourth letter if the sum is shared).
pub fn known_titles() -> impl Iterator<Item = (u8, Option<u8>)> {
    KNOWN.iter().flat_map(|(titles, _)| titles.iter()).map(|t| match *t {
        Sum(s) => (s, None),
        Letter(s, l) => (s, Some(l)),
    })
}

/// The two titles the start-up code also draws the Game Boy's logo map for: games that animate
/// the logo the Game Boy's own start-up leaves on the screen, which a Color's does not leave.
pub fn draws_the_logo(checksum: Option<u8>) -> bool {
    matches!(checksum, Some(0x43 | 0x58))
}

/// A documented colour as the palette memory holds it: five bits a channel, red lowest. The
/// documentation spreads 0-31 over 0-255 to the nearest whole number, so rounding back to the
/// nearest gives the hardware's value exactly (a test checks every colour here round-trips).
pub fn rgb15(c: u32) -> u16 {
    let five = |v: u32| ((v * 31 + 127) / 255) as u16;
    five(c >> 16 & 0xFF) | five(c >> 8 & 0xFF) << 5 | five(c & 0xFF) << 10
}

impl Palettes {
    /// The 48 bytes of palette memory the start-up code fills, as three runs of eight: the
    /// background's palette 0, and the objects' palettes 0 and 1. Each colour is two bytes, low
    /// byte first.
    pub fn bytes(&self) -> [[u8; 8]; 3] {
        let run = |colours: &[u32; 4]| {
            let mut out = [0u8; 8];
            for (i, c) in colours.iter().enumerate() {
                out[i * 2..i * 2 + 2].copy_from_slice(&rgb15(*c).to_le_bytes());
            }
            out
        };
        [run(&self.bg), run(&self.obj0), run(&self.obj1)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cartridge header with this title, from Nintendo or not.
    fn header(title: &[u8], nintendo: bool) -> Vec<u8> {
        let mut rom = vec![0u8; 0x8000];
        rom[0x134..0x134 + title.len()].copy_from_slice(title);
        rom[0x14B] = if nintendo { 0x01 } else { 0x00 };
        rom
    }

    /// A made-up title with this sum and this fourth letter.
    fn title_summing_to(sum: u8, fourth: u8) -> Vec<u8> {
        let mut t = vec![b'Q'; 15];
        t[3] = fourth;
        let have = t.iter().fold(0u8, |s, b| s.wrapping_add(*b));
        t.push(sum.wrapping_sub(have));
        t
    }

    /// Every colour here is one a Color can show, and is written the way the documentation
    /// writes it: rounding it to five bits and back gives the same number. A colour copied wrong
    /// by a digit almost never lands back on itself, so this is what catches a typo.
    #[test]
    fn every_colour_is_on_the_colours_own_ladder() {
        let all = KNOWN.iter().map(|(_, p)| *p).chain(PICKS.iter().map(|k| k.palettes));
        let back = |v: u16| (u32::from(v) * 255 + 15) / 31;
        for set in all {
            for c in set.bg.iter().chain(&set.obj0).chain(&set.obj1) {
                let v = rgb15(*c);
                let again = back(v & 31) << 16 | back(v >> 5 & 31) << 8 | back(v >> 10 & 31);
                assert_eq!(again, *c, "{c:06X} is not a colour the documentation could have written");
            }
        }
    }

    /// The table's shape is the start-up code's: 65 titles known by their sum alone and 29 that
    /// share one of fourteen sums, told apart by their fourth letter; no sum is known both ways,
    /// and no title twice.
    #[test]
    fn the_table_has_the_start_up_codes_shape() {
        let titles: Vec<Title> = KNOWN.iter().flat_map(|(t, _)| t.iter().copied()).collect();
        let sums: Vec<u8> = titles.iter().filter_map(|t| if let Sum(s) = t { Some(*s) } else { None }).collect();
        let shared: std::collections::BTreeSet<u8> =
            titles.iter().filter_map(|t| if let Letter(s, _) = t { Some(*s) } else { None }).collect();
        assert_eq!((sums.len(), titles.len() - sums.len(), shared.len()), (65, 29, 14));
        assert!(sums.iter().all(|s| !shared.contains(s)), "a sum is known both alone and by letter");
        for (i, t) in titles.iter().enumerate() {
            assert!(!titles[..i].contains(t), "{t:?} is in the table twice");
        }
    }

    #[test]
    fn a_header_gets_the_colours_the_start_up_code_picks_for_it() {
        // Tetris: yellow and red everywhere.
        let tetris = picked(&header(b"TETRIS", true));
        assert_eq!(tetris.bg, [0xFFFFFF, 0xFFFF00, 0xFF0000, 0x000000]);
        assert_eq!((tetris.obj0, tetris.obj1), (tetris.bg, tetris.bg));
        // Pokemon Red: a red background, green objects through OBP0.
        let red = picked(&header(b"POKEMON RED", true));
        assert_eq!(red.bg, [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000]);
        assert_eq!(red.obj0, [0xFFFFFF, 0x7BFF31, 0x008400, 0x000000]);
        // The same title from someone other than Nintendo is not looked up at all.
        assert_eq!(picked(&header(b"TETRIS", false)), DEFAULT);
        assert_eq!(title_checksum(&header(b"TETRIS", false)), None);
        // The new licensee field says Nintendo the long way round, and is believed.
        let mut long_way = header(b"TETRIS", false);
        long_way[0x14B] = 0x33;
        long_way[0x144..0x146].copy_from_slice(b"01");
        assert_eq!(picked(&long_way), tetris);
        long_way[0x145] = b'8';
        assert_eq!(picked(&long_way), DEFAULT, "a new licensee that is not Nintendo's");
    }

    /// Super Mario Land and Metroid II have the same title sum, $46, and only the fourth letter
    /// tells them apart: E for SUP(E)R MARIOLAND, R for MET(R)OID2. Pokemon Blue and Vegas Stakes
    /// share $61 the same way. A shared sum with any other fourth letter is not a game it knows.
    #[test]
    fn a_shared_title_sum_is_split_by_the_fourth_letter() {
        let mario = header(b"SUPER MARIOLAND", true);
        let metroid = header(b"METROID2", true);
        assert_eq!((title_checksum(&mario), title_checksum(&metroid)), (Some(0x46), Some(0x46)));
        assert_eq!(picked(&mario).bg, [0xB5B5FF, 0xFFFF94, 0xAD5A42, 0x000000]);
        assert_eq!(picked(&metroid).obj0, [0xFFFF00, 0xFF0000, 0x630000, 0x000000]);
        let blue = header(b"POKEMON BLUE", true);
        let vegas = header(b"VEGAS STAKES", true);
        assert_eq!((title_checksum(&blue), title_checksum(&vegas)), (Some(0x61), Some(0x61)));
        assert_eq!(picked(&blue).bg, [0xFFFFFF, 0x63A5FF, 0x0000FF, 0x000000]);
        assert_eq!(picked(&vegas).bg, [0xFFFFFF, 0x7BFF31, 0x008400, 0x000000]);
        assert_eq!(picked(&header(&title_summing_to(0x61, b'Z'), true)), DEFAULT);
        assert_eq!(picked(&header(&title_summing_to(0x46, b'E'), true)), picked(&mario), "the sum and the letter are all it reads");
    }

    #[test]
    fn a_game_it_does_not_know_gets_the_default_colours() {
        // A sum nothing in the table has.
        let unknown = header(&title_summing_to(0x02, b'Q'), true);
        assert_eq!(title_checksum(&unknown), Some(0x02));
        assert_eq!(picked(&unknown), DEFAULT);
        assert_eq!(DEFAULT.bg, [0xFFFFFF, 0x7BFF31, 0x0063C5, 0x000000]);
        assert_eq!(DEFAULT.obj0, [0xFFFFFF, 0xFF8484, 0x943A3A, 0x000000]);
        // As the palette memory holds it: 7BFF31 is red 15, green 31, blue 6.
        assert_eq!(rgb15(0x7BFF31), 15 | 31 << 5 | 6 << 10);
        assert_eq!(DEFAULT.bytes()[0][2..4], (15u16 | 31 << 5 | 6 << 10).to_le_bytes());
    }

    /// The twelve button schemes, each with its own buttons, and each the colours the
    /// documentation gives for it.
    #[test]
    fn each_button_scheme() {
        let want: [(u8, u32, u32, u32); 12] = [
            (UP, 0xFFAD63, 0xFFAD63, 0xFFAD63),
            (UP | A, 0xFF8484, 0x7BFF31, 0x63A5FF),
            (UP | B, 0xCE9C84, 0xFFAD63, 0xFFAD63),
            (LEFT, 0x63A5FF, 0xFF8484, 0x7BFF31),
            (LEFT | A, 0x8C8CDE, 0xFF8484, 0xFFAD63),
            (LEFT | B, 0xA5A5A5, 0xA5A5A5, 0xA5A5A5),
            (DOWN, 0xFF9494, 0xFF9494, 0xFF9494),
            (DOWN | A, 0xFFFF00, 0xFFFF00, 0xFFFF00),
            (DOWN | B, 0xFFFF00, 0x63A5FF, 0x7BFF31),
            (RIGHT, 0x52FF00, 0x52FF00, 0x52FF00),
            (RIGHT | A, 0x7BFF31, 0xFF8484, 0xFF8484),
            (RIGHT | B, 0x008484, 0x008484, 0x008484),
        ];
        for (pick, (buttons, bg, obj0, obj1)) in PICKS.iter().zip(want) {
            assert_eq!(pick.buttons, buttons);
            assert_eq!((pick.palettes.bg[1], pick.palettes.obj0[1], pick.palettes.obj1[1]), (bg, obj0, obj1), "{buttons:02X}");
        }
        // Twelve different schemes, and twelve different button combinations.
        for (i, a) in PICKS.iter().enumerate() {
            for b in &PICKS[..i] {
                assert_ne!(a.palettes, b.palettes);
                assert_ne!(a.buttons, b.buttons);
            }
        }
        // The two that are also games' own: Up is F-1 Race's, Right + A the default.
        assert_eq!(PICKS[0].palettes, picked(&header(b"F1RACE", true)));
        assert_eq!(PICKS[10].palettes, DEFAULT);
        assert_eq!(PICKS[11].palettes.bg, [0x000000, 0x008484, 0xFFDE00, 0xFFFFFF], "reversed: black is the lightest");
    }

    #[test]
    fn only_two_titles_get_the_logo_map() {
        assert!(draws_the_logo(Some(0x43)) && draws_the_logo(Some(0x58)));
        assert!(!draws_the_logo(Some(0xDB)) && !draws_the_logo(None));
    }
}
