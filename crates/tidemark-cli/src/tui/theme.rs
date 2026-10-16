//! Owned by: W7-A
//! Tide palette and glyph selection from color flags and the terminal environment.

use ratatui::style::Color;

/// How much color the terminal can show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Level {
    /// 24-bit colors from the Tide review palette.
    Rgb,
    /// xterm-256 approximations of that palette.
    Indexed,
    /// 16-color ANSI stand-ins. Background stays unset.
    Basic,
    /// Bold, dim, and reversed only.
    Mono,
}

/// One semantic paint role.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Role {
    Bg,
    Deep,
    Edge,
    Wave,
    Text,
    Dim,
    Teal,
    Foam,
    Sea,
    Sand,
    Coral,
}

/// Characters that change with the locale and `TERM`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Glyphs {
    pub anchor: &'static str,
    pub warn: &'static str,
    pub rise: &'static str,
    pub ebb: &'static str,
    pub marker: &'static str,
    pub level_on: &'static str,
    pub level_off: &'static str,
    pub bar: &'static str,
    pub refresh: &'static str,
    pub wave: &'static str,
    pub open: &'static str,
    pub sort_asc: &'static str,
    pub sort_desc: &'static str,
    pub sep: &'static str,
    pub spinner: [&'static str; 4],
    pub key_move: &'static str,
    pub key_descend: &'static str,
    pub key_up: &'static str,
    pub unicode: bool,
}

const UNICODE: Glyphs = Glyphs {
    anchor: "⚓",
    warn: "⚠",
    rise: "▲",
    ebb: "▼",
    marker: "▶",
    level_on: "▆",
    level_off: "┈",
    bar: "█",
    refresh: "⟳",
    wave: "≋",
    open: "●",
    sort_asc: "▴",
    sort_desc: "▾",
    sep: "·",
    spinner: ["⠋", "⠙", "⠸", "⠴"],
    key_move: "↑↓",
    key_descend: "→/⏎",
    key_up: "←",
    unicode: true,
};

const ASCII: Glyphs = Glyphs {
    anchor: "H",
    warn: "!",
    rise: "^",
    ebb: "v",
    marker: ">",
    level_on: "#",
    level_off: ".",
    bar: "#",
    refresh: "*",
    wave: "~",
    open: "*",
    sort_asc: "^",
    sort_desc: "v",
    sep: ".",
    spinner: ["|", "/", "-", "\\"],
    key_move: "up/down",
    key_descend: "enter",
    key_up: "left",
    unicode: false,
};

/// Resolved Tide skin for one browser session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Theme {
    pub level: Level,
    pub glyphs: Glyphs,
}

impl Theme {
    /// Pick a skin. `no_color_present` is true when `NO_COLOR` is set to any value, including empty.
    pub(super) fn select(
        color: bool,
        no_color_present: bool,
        colorterm: Option<&str>,
        term: Option<&str>,
        lang: Option<&str>,
    ) -> Self {
        let level = if !color || no_color_present {
            Level::Mono
        } else if matches!(colorterm, Some("truecolor" | "24bit")) {
            Level::Rgb
        } else if term.is_some_and(|value| value.contains("256color")) {
            Level::Indexed
        } else {
            Level::Basic
        };
        let ascii =
            term.is_some_and(|value| value == "linux") || crate::render::tree::ascii_for_lang(lang);
        Self {
            level,
            glyphs: if ascii { ASCII } else { UNICODE },
        }
    }

    /// Stable Unicode monochrome skin for render tests.
    #[cfg(test)]
    pub(super) fn plain() -> Self {
        Self::select(false, false, None, Some("xterm-256color"), Some("C.UTF-8"))
    }

    pub(super) fn color(self, role: Role) -> Color {
        match self.level {
            Level::Mono => Color::Reset,
            Level::Basic => basic(role),
            Level::Indexed => Color::Indexed(indexed(role)),
            Level::Rgb => rgb(role),
        }
    }

    pub(super) fn strata(self, index: usize) -> Color {
        match self.level {
            Level::Mono => Color::Reset,
            Level::Basic => basic_strata(index),
            Level::Indexed => Color::Indexed(INDEXED_STRATA[index.min(5)]),
            Level::Rgb => rgb_strata(index),
        }
    }
}

fn basic(role: Role) -> Color {
    match role {
        Role::Bg => Color::Reset,
        Role::Deep | Role::Edge | Role::Sea | Role::Wave => Color::Blue,
        Role::Text => Color::Reset,
        Role::Dim => Color::DarkGray,
        Role::Teal | Role::Foam => Color::Cyan,
        Role::Sand => Color::Yellow,
        Role::Coral => Color::Red,
    }
}

fn basic_strata(index: usize) -> Color {
    match index {
        // Reset, not White: Basic has no foam, and White paints the newest bar as a solid block.
        0 => Color::Reset,
        1 | 2 => Color::Cyan,
        3 | 4 => Color::Blue,
        _ => Color::DarkGray,
    }
}

fn indexed(role: Role) -> u8 {
    match role {
        Role::Bg => 233,
        Role::Deep => 17,
        Role::Edge | Role::Wave => 23,
        Role::Text => 195,
        Role::Dim => 66,
        Role::Teal => 37,
        Role::Foam => 122,
        Role::Sea => 67,
        Role::Sand => 186,
        Role::Coral => 210,
    }
}

const INDEXED_STRATA: [u8; 6] = [122, 79, 37, 31, 24, 23];

fn rgb(role: Role) -> Color {
    let (red, green, blue) = match role {
        Role::Bg => (0x07, 0x15, 0x26),
        Role::Deep => (0x0d, 0x22, 0x38),
        Role::Edge => (0x1d, 0x3d, 0x57),
        Role::Wave => (0x12, 0x3a, 0x5a),
        Role::Text => (0xcf, 0xe8, 0xef),
        Role::Dim => (0x6f, 0x93, 0xa6),
        Role::Teal => (0x2e, 0xc4, 0xb6),
        Role::Foam => (0x9e, 0xf0, 0xe6),
        Role::Sea => (0x3a, 0x86, 0xb5),
        Role::Sand => (0xe9, 0xc4, 0x6a),
        Role::Coral => (0xff, 0x7b, 0x6b),
    };
    Color::Rgb(red, green, blue)
}

fn rgb_strata(index: usize) -> Color {
    let (red, green, blue) = match index {
        0 => (0x9e, 0xf0, 0xe6),
        1 => (0x5f, 0xd3, 0xc6),
        2 => (0x2e, 0xa7, 0xa0),
        3 => (0x2a, 0x7f, 0x9e),
        4 => (0x24, 0x55, 0x7d),
        _ => (0x1b, 0x3a, 0x5c),
    };
    Color::Rgb(red, green, blue)
}
