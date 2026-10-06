//! The one place visual values live. ryolune wears the lsuite design system v2
//! (`desktop/assets/lsuite/tokens.json`, a copy of lsuite's `design/tokens.json`,
//! lsuite.xyz/design): black and white, cut square, with grain.
//!
//! Everything in the chrome is a grey; the accent is the ink of the mode (white in the dark,
//! black in the light) for selection, focus, the playhead and the primary action, and a chosen
//! thing is inverted (paper on ink). Red only for what destroys or records (record, arm, errors,
//! a clipping meter); warnings and success are greys with an icon or a word. Corners are square
//! ([`radius`] is zero; only dials stay round), floating surfaces cast a hard offset shadow
//! ([`Theme::float_shadow`]) and the page behind the chrome is film grain and dithered light
//! (`ui::grain`). The work keeps its colours: track colours, clips, notes and waveforms are what
//! the person made, and makers' logos keep theirs.
//!
//! Add a token here, in both modes, never a colour in a view. `contrast` tests below keep
//! text readable on every surface, glass tiers included, over the page at its densest grain
//! and dither, and over the brightest and darkest desktop behind the window.

use gpui::{point, px, App, BoxShadow, Global, Hsla, Rgba};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Dark,
    Light,
}

/// A glass tier: translucent fill and its opaque fallback.
#[derive(Clone, Copy, Debug)]
pub struct Glass {
    pub fill: Hsla,
    pub opaque: Hsla,
}

/// The whole lsuite token set for one mode; a token a view does not use yet stays, so the
/// two modes keep the same shape as the design system.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct Theme {
    pub mode: Mode,
    /// Opaque fills instead of glass: the system asks for reduced transparency, or the
    /// platform cannot blur the window.
    pub opaque: bool,

    /// The page: what the chrome sits on, under the grain and the dithered light. Slightly
    /// translucent over the blurred desktop.
    pub backdrop: Hsla,
    /// The ink of the grain and the dithered light (white on the dark page, black on the
    /// light one), and how strong each is (`--ls-grain-strength`, `--ls-dither-strength`).
    pub ink: Hsla,
    pub grain: f32,
    pub dither: f32,

    // Work surfaces: never glass, never grain.
    pub bg: Hsla,
    pub bg_raised: Hsla,
    pub bg_sunken: Hsla,
    pub lane: Hsla,
    pub lane_alt: Hsla,
    pub lane_selected: Hsla,
    pub lane_agent: Hsla,
    pub lane_empty: Hsla,
    pub ruler: Hsla,
    pub editor: Hsla,
    pub display: Hsla,
    pub well: Hsla,
    /// The track a fader cap, a slider thumb or a knob's arc runs in.
    pub groove: Hsla,

    // Controls.
    pub control: Hsla,
    pub control_hover: Hsla,
    pub control_pressed: Hsla,
    pub control_edge: Hsla,
    pub control_highlight: Hsla,
    pub thumb: Hsla,
    pub knob: Hsla,
    pub knob_edge: Hsla,

    // Text.
    pub text: Hsla,
    pub text_2: Hsla,
    pub text_3: Hsla,
    pub text_on_accent: Hsla,
    pub text_display: Hsla,

    // Lines.
    pub line: Hsla,
    pub line_strong: Hsla,
    pub hairline: Hsla,
    pub bar_line: Hsla,
    pub beat_line: Hsla,

    // Glass tiers (DESIGN.md: chrome, floating, modal) and their trim.
    pub glass_1: Glass,
    pub glass_2: Glass,
    pub glass_3: Glass,
    pub glass_edge: Hsla,
    pub glass_highlight: Hsla,
    /// The soft shadow under the hard one in the dark mode (transparent by day), and under
    /// things that lift off the work (a dragged clip, a fader cap).
    pub glass_shadow: Hsla,
    /// The hard offset shadow under floating surfaces (`--ls-glass-shadow`).
    pub drop: Hsla,
    /// The smaller hard shadow under the primary action (`--ls-chip-shadow`).
    pub chip: Hsla,
    pub scrim: Hsla,

    // Accent: the ink of the mode.
    pub accent: Hsla,
    pub accent_hover: Hsla,
    /// The accent as a fill that carries text (primary buttons, lit keys).
    pub accent_fill: Hsla,
    pub accent_text: Hsla,
    pub accent_soft: Hsla,
    pub accent_ring: Hsla,
    pub accent_glow: Hsla,

    // States.
    pub danger: Hsla,
    pub warning: Hsla,
    pub success: Hsla,
    pub record: Hsla,
    /// Lit mute and solo keys: inverted like every chosen thing (the letter says which).
    pub mute: Hsla,
    pub solo: Hsla,
    pub meter: Hsla,
    pub meter_hot: Hsla,
    pub meter_clip: Hsla,
    pub meter_off: Hsla,

    // Musical surfaces.
    pub key_white: Hsla,
    pub key_black: Hsla,
    pub key_label: Hsla,
    pub black_key_row: Hsla,
    pub note: Hsla,
    pub note_edge: Hsla,
    pub velocity: Hsla,
    pub waveform: Hsla,
    pub clip_text: Hsla,
    pub clip_title: Hsla,
    pub cycle: Hsla,
    pub cycle_lane: Hsla,
    pub marker: Hsla,
    pub hover: Hsla,
    pub selection_text: Hsla,

    // Region editor (piano roll, score, step, controller lane).
    /// Every other key row, lifted a little so rows read across a wide roll.
    pub row_shade: Hsla,
    /// The bar ticks in the editor's ruler.
    pub ruler_tick: Hsla,
    /// The step lines of the step view.
    pub step_cell: Hsla,
    /// A note being drawn with the pencil.
    pub pencil_preview: Hsla,
    /// Where a note being moved or resized would land.
    pub drag_ghost: Hsla,
    pub drag_ghost_edge: Hsla,
    /// The light along a note's top edge.
    pub note_highlight: Hsla,
    /// The drop under a note.
    pub note_shadow: Hsla,
    /// Staff lines, ledger lines and duration tails of the score view.
    pub staff: Hsla,
    // Brand marks (the agent's model picker): makers' logos sit on a white tile in both
    // modes, in their own colours; one-colour marks take `logo_ink`.
    pub logo_tile: Hsla,
    pub logo_ink: Hsla,
    // Plugin displays and the automation graph, drawn on `display`.
    /// The graticule.
    pub display_grid: Hsla,
    /// 0 dB, the centre and unity lines.
    pub display_zero: Hsla,
    /// Faint labels and ghost traces.
    pub display_ink: Hsla,
}

impl Global for Theme {}

impl Theme {
    pub fn new(mode: Mode, opaque: bool) -> Self {
        let mut t = match mode {
            Mode::Dark => dark(opaque),
            Mode::Light => light(opaque),
        };
        // GPUI cannot blur what is behind an element, so floating tiers (menus, popovers,
        // dialogs) would let busy content show through: their tint is laid over the raised
        // surface instead, the same colour readable over anything. The chrome (tier 1) sits on
        // the page and stays translucent.
        for tier in [&mut t.glass_2, &mut t.glass_3] {
            tier.fill = over(tier.fill, t.bg_raised);
        }
        t
    }
    pub fn get(cx: &App) -> &Theme {
        cx.global::<Theme>()
    }
    pub fn is_dark(&self) -> bool {
        self.mode == Mode::Dark
    }
    /// The fill of a glass tier, or its opaque fallback.
    pub fn glass(&self, tier: u8) -> Hsla {
        let g = match tier {
            1 => self.glass_1,
            2 => self.glass_2,
            _ => self.glass_3,
        };
        if self.opaque {
            g.opaque
        } else {
            g.fill
        }
    }
    /// Under floating surfaces (menus, popovers, tooltips, dialogs): a hard shadow 4 px down
    /// and right, no blur; in the dark a soft black one under it, since the hard one is light
    /// and would not separate the surface alone.
    pub fn float_shadow(&self) -> Vec<BoxShadow> {
        let hard = BoxShadow {
            color: self.drop,
            offset: point(px(4.0), px(4.0)),
            blur_radius: px(0.0),
            spread_radius: px(0.0),
        };
        if self.is_dark() {
            vec![
                BoxShadow {
                    color: self.glass_shadow,
                    offset: point(px(0.0), px(10.0)),
                    blur_radius: px(30.0),
                    spread_radius: px(0.0),
                },
                hard,
            ]
        } else {
            vec![hard]
        }
    }
    /// The smaller hard shadow the primary action stands on.
    pub fn chip_shadow(&self) -> Vec<BoxShadow> {
        vec![BoxShadow {
            color: self.chip,
            offset: point(px(2.0), px(2.0)),
            blur_radius: px(0.0),
            spread_radius: px(0.0),
        }]
    }
    /// A sound family's colour (browser folders, plugin faces, insert slots), by the
    /// folder name the engine files a plugin under. The chrome is grey in v2, so every family
    /// is the same quiet grey; the folder's name says which it is.
    pub fn family(&self, folder: &str) -> Hsla {
        let _ = folder;
        self.text_3
    }
    /// A note's two shades from its track colour (top and bottom of its face): the same hue
    /// at a fixed lightness, so every track's notes read alike on the editor surface.
    pub fn note_shades(&self, track: Hsla) -> (Hsla, Hsla) {
        match self.mode {
            Mode::Dark => (with_lightness(track, 0.8), with_lightness(track, 0.66)),
            Mode::Light => (with_lightness(track, 0.74), with_lightness(track, 0.62)),
        }
    }
    /// A track's colour: the document keeps a CSS colour; an unreadable one falls back to
    /// the palette by position. Track colours are the person's: they stay on the work.
    pub fn track(&self, color: &str, index: usize) -> Hsla {
        css_color(color).unwrap_or_else(|| {
            let (l, c, h) = TRACK_PALETTE[index % TRACK_PALETTE.len()].1;
            oklch(l, c, h, 1.0)
        })
    }
}

/// `top` composited over an opaque `bottom`.
pub fn over(top: Hsla, bottom: Hsla) -> Hsla {
    let (t, b): (Rgba, Rgba) = (top.into(), bottom.into());
    let a = t.a;
    Rgba {
        r: t.r * a + b.r * (1.0 - a),
        g: t.g * a + b.g * (1.0 - a),
        b: t.b * a + b.b * (1.0 - a),
        a: 1.0,
    }
    .into()
}

/// A grey: 0 is black, 1 is white.
pub fn grey(v: f32) -> Hsla {
    Rgba {
        r: v,
        g: v,
        b: v,
        a: 1.0,
    }
    .into()
}

/// Track palette from the design spec sheet: L 0.72–0.78, C 0.12–0.14. Names are what the
/// track menu offers; the value written to the document is the CSS `oklch()` text.
pub const TRACK_PALETTE: [(&str, (f32, f32, f32)); 8] = [
    ("Drums", (0.72, 0.14, 40.0)),
    ("Bass", (0.72, 0.13, 300.0)),
    ("Keys", (0.75, 0.13, 250.0)),
    ("Pad", (0.75, 0.12, 330.0)),
    ("Vox", (0.78, 0.14, 85.0)),
    ("Backing vocals", (0.75, 0.12, 130.0)),
    ("Guitar", (0.72, 0.13, 20.0)),
    ("Riser", (0.75, 0.14, 60.0)),
];
pub fn palette_css(index: usize) -> String {
    let (_, (l, c, h)) = TRACK_PALETTE[index % TRACK_PALETTE.len()];
    format!("oklch({l} {c} {h})")
}

pub fn hex(rgb: u32) -> Hsla {
    gpui::rgb(rgb).into()
}
pub fn hexa(rgb: u32, alpha: f32) -> Hsla {
    let mut c: Hsla = gpui::rgb(rgb).into();
    c.a = alpha;
    c
}
pub fn white(alpha: f32) -> Hsla {
    hexa(0xffffff, alpha)
}
pub fn black(alpha: f32) -> Hsla {
    hexa(0x000000, alpha)
}
pub fn with_alpha(mut c: Hsla, alpha: f32) -> Hsla {
    c.a = alpha;
    c
}
/// The same colour at another OKLCH lightness (0-1): hue and chroma kept.
pub fn with_lightness(color: Hsla, lightness: f32) -> Hsla {
    let c: Rgba = color.into();
    let decode = |x: f32| {
        if x <= 0.040_45 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (decode(c.r), decode(c.g), decode(c.b));
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    let a = 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s;
    let bb = 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s;
    let chroma = (a * a + bb * bb).sqrt();
    let hue = bb.atan2(a).to_degrees();
    oklch(lightness, chroma, hue, c.a)
}

/// OKLCH to sRGB (CSS Color 4), gamut-clipped.
pub fn oklch(l: f32, c: f32, h: f32, alpha: f32) -> Hsla {
    let (a, b) = (c * h.to_radians().cos(), c * h.to_radians().sin());
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l3, m3, s3) = (l_.powi(3), m_.powi(3), s_.powi(3));
    let r = 4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3;
    let g = -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3;
    let bl = -0.004_196_086_3 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3;
    let encode = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        if x <= 0.003_130_8 {
            12.92 * x
        } else {
            1.055 * x.powf(1.0 / 2.4) - 0.055
        }
    };
    Rgba {
        r: encode(r),
        g: encode(g),
        b: encode(bl),
        a: alpha,
    }
    .into()
}

/// The CSS colours a document can hold: `#rgb`, `#rrggbb`, `rgb()/rgba()` and `oklch()`.
pub fn css_color(text: &str) -> Option<Hsla> {
    let t = text.trim();
    if let Some(hex_text) = t.strip_prefix('#') {
        let expanded: String = if hex_text.len() == 3 {
            hex_text.chars().flat_map(|c| [c, c]).collect()
        } else {
            hex_text.to_string()
        };
        return u32::from_str_radix(&expanded, 16)
            .ok()
            .filter(|_| expanded.len() == 6)
            .map(hex);
    }
    let (name, args) = t.split_once('(')?;
    let args = args.strip_suffix(')')?;
    let parts: Vec<&str> = args
        .split(|c: char| c == ',' || c == '/' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .collect();
    let num = |p: &str| -> Option<f32> {
        if let Some(pct) = p.strip_suffix('%') {
            pct.parse::<f32>().ok().map(|v| v / 100.0)
        } else {
            p.trim_end_matches("deg").parse::<f32>().ok()
        }
    };
    match name.trim() {
        "oklch" => {
            let l = num(parts.first()?)?;
            let c = num(parts.get(1)?)?;
            let h = num(parts.get(2)?)?;
            let a = parts.get(3).and_then(|p| num(p)).unwrap_or(1.0);
            Some(oklch(l, c, h, a))
        }
        "rgb" | "rgba" => {
            let v = |i: usize| parts.get(i).and_then(|p| p.parse::<f32>().ok());
            Some(
                Rgba {
                    r: v(0)? / 255.0,
                    g: v(1)? / 255.0,
                    b: v(2)? / 255.0,
                    a: parts.get(3).and_then(|p| num(p)).unwrap_or(1.0),
                }
                .into(),
            )
        }
        _ => None,
    }
}

fn dark(opaque: bool) -> Theme {
    let ink = gpui::white();
    let w = white;
    Theme {
        mode: Mode::Dark,
        opaque,
        backdrop: hexa(0x050505, if opaque { 1.0 } else { 0.95 }),
        ink,
        grain: 0.07,
        dither: 0.13,
        bg: hex(0x050505),
        bg_raised: hex(0x0e0e0e),
        bg_sunken: hex(0x000000),
        lane: grey(0.03),
        lane_alt: grey(0.022),
        lane_selected: grey(0.075),
        lane_agent: grey(0.06),
        lane_empty: grey(0.016),
        ruler: grey(0.045),
        editor: grey(0.028),
        display: hex(0x000000),
        well: grey(0.02),
        groove: grey(0.16),
        control: grey(0.07),
        control_hover: grey(0.13),
        control_pressed: grey(0.04),
        control_edge: w(0.2),
        control_highlight: w(0.0),
        thumb: hex(0xf2f2f2),
        knob: grey(0.1),
        knob_edge: w(0.24),
        text: hex(0xf2f2f2),
        text_2: hex(0xa8a8a8),
        text_3: hex(0x7a7a7a),
        text_on_accent: hex(0x000000),
        text_display: hex(0xf2f2f2),
        line: w(0.10),
        line_strong: w(0.20),
        hairline: w(0.06),
        bar_line: w(0.08),
        beat_line: w(0.03),
        glass_1: Glass {
            fill: hexa(0x080808, 0.62),
            opaque: hex(0x0f0f0f),
        },
        glass_2: Glass {
            fill: hexa(0x0e0e0e, 0.92),
            opaque: hex(0x0f0f0f),
        },
        glass_3: Glass {
            fill: hexa(0x121212, 0.96),
            opaque: hex(0x0f0f0f),
        },
        glass_edge: w(0.16),
        glass_highlight: w(0.0),
        glass_shadow: black(0.7),
        drop: w(0.11),
        chip: w(0.22),
        scrim: black(0.62),
        accent: ink,
        accent_hover: grey(0.82),
        accent_fill: ink,
        accent_text: ink,
        accent_soft: w(0.12),
        accent_ring: w(0.62),
        accent_glow: w(0.28),
        danger: hex(0xff5b4d),
        warning: hex(0xd9d9d9),
        success: hex(0xf2f2f2),
        record: hex(0xff5b4d),
        mute: hex(0xf2f2f2),
        solo: hex(0xf2f2f2),
        meter: grey(0.62),
        meter_hot: grey(0.95),
        meter_clip: hex(0xff5b4d),
        meter_off: grey(0.13),
        key_white: grey(0.9),
        key_black: grey(0.06),
        key_label: grey(0.42),
        black_key_row: black(0.32),
        note: grey(0.85),
        note_edge: black(0.35),
        velocity: w(0.26),
        waveform: w(0.82),
        clip_text: w(0.95),
        clip_title: black(0.24),
        cycle: w(0.16),
        cycle_lane: w(0.035),
        marker: grey(0.78),
        hover: w(0.07),
        selection_text: w(0.28),
        row_shade: w(0.022),
        ruler_tick: w(0.24),
        step_cell: w(0.05),
        pencil_preview: w(0.2),
        drag_ghost: w(0.09),
        drag_ghost_edge: w(0.42),
        note_highlight: w(0.28),
        note_shadow: black(0.4),
        staff: w(0.3),
        logo_tile: hex(0xffffff),
        logo_ink: hex(0x0a0a0a),
        display_grid: w(0.05),
        display_zero: w(0.12),
        display_ink: grey(0.55),
    }
}

fn light(opaque: bool) -> Theme {
    let ink = gpui::black();
    let k = black;
    Theme {
        mode: Mode::Light,
        opaque,
        backdrop: hexa(0xf0f0f0, if opaque { 1.0 } else { 0.93 }),
        ink,
        grain: 0.09,
        dither: 0.11,
        bg: hex(0xf0f0f0),
        bg_raised: hex(0xfbfbfb),
        bg_sunken: hex(0xe3e3e3),
        lane: hex(0xfafafa),
        lane_alt: hex(0xf3f3f3),
        lane_selected: hex(0xe8e8e8),
        lane_agent: hex(0xeeeeee),
        lane_empty: hex(0xf0f0f0),
        ruler: hex(0xf5f5f5),
        editor: hex(0xfafafa),
        display: hex(0xffffff),
        well: hex(0xe9e9e9),
        groove: hex(0xc8c8c8),
        control: hex(0xffffff),
        control_hover: hex(0xefefef),
        control_pressed: hex(0xe3e3e3),
        control_edge: k(0.26),
        control_highlight: k(0.0),
        thumb: hex(0xffffff),
        knob: hex(0xffffff),
        knob_edge: k(0.3),
        text: hex(0x0a0a0a),
        text_2: hex(0x4d4d4d),
        text_3: hex(0x6e6e6e),
        text_on_accent: hex(0xffffff),
        text_display: hex(0x0a0a0a),
        line: k(0.12),
        line_strong: k(0.26),
        hairline: k(0.07),
        bar_line: k(0.1),
        beat_line: k(0.04),
        glass_1: Glass {
            fill: hexa(0xfafafa, 0.62),
            opaque: hex(0xf7f7f7),
        },
        glass_2: Glass {
            fill: hexa(0xfcfcfc, 0.94),
            opaque: hex(0xf7f7f7),
        },
        glass_3: Glass {
            fill: hexa(0xffffff, 0.97),
            opaque: hex(0xf7f7f7),
        },
        glass_edge: k(0.22),
        glass_highlight: k(0.0),
        glass_shadow: k(0.0),
        drop: k(0.85),
        chip: k(0.9),
        scrim: hexa(0xebebeb, 0.62),
        accent: ink,
        accent_hover: grey(0.22),
        accent_fill: ink,
        accent_text: ink,
        accent_soft: k(0.09),
        accent_ring: k(0.62),
        accent_glow: k(0.18),
        danger: hex(0xc8291c),
        warning: hex(0x333333),
        success: hex(0x0a0a0a),
        record: hex(0xc8291c),
        mute: hex(0x0a0a0a),
        solo: hex(0x0a0a0a),
        meter: grey(0.42),
        meter_hot: grey(0.08),
        meter_clip: hex(0xc8291c),
        meter_off: grey(0.86),
        key_white: hex(0xffffff),
        key_black: grey(0.14),
        key_label: grey(0.5),
        black_key_row: k(0.04),
        note: grey(0.2),
        note_edge: k(0.22),
        velocity: k(0.3),
        waveform: k(0.7),
        clip_text: k(0.9),
        clip_title: white(0.38),
        cycle: k(0.14),
        cycle_lane: k(0.04),
        marker: grey(0.3),
        hover: k(0.05),
        selection_text: k(0.2),
        row_shade: k(0.02),
        ruler_tick: k(0.3),
        step_cell: k(0.05),
        pencil_preview: k(0.18),
        drag_ghost: k(0.08),
        drag_ghost_edge: k(0.42),
        note_highlight: white(0.4),
        note_shadow: k(0.14),
        staff: k(0.34),
        logo_tile: hex(0xffffff),
        logo_ink: hex(0x0a0a0a),
        display_grid: k(0.06),
        display_zero: k(0.14),
        display_ink: grey(0.45),
    }
}

/// Type sizes (lsuite: 11 · 12 · 13 controls · 15 body · 17 · 22 · 28 · 40).
#[allow(dead_code)]
pub mod size {
    pub const XS: f32 = 11.0;
    pub const SM: f32 = 12.0;
    pub const BASE: f32 = 13.0;
    pub const MD: f32 = 15.0;
    pub const LG: f32 = 17.0;
    pub const XL: f32 = 22.0;
    pub const XXL: f32 = 28.0;
}
/// Radii: zero everywhere (lsuite v2, `--ls-radius-*` are 0). The names stay so every surface
/// keeps its step on the scale; only things round in the world (knobs, dials) are round.
#[allow(dead_code)]
pub mod radius {
    pub const XS: f32 = 0.0;
    pub const SM: f32 = 0.0;
    pub const MD: f32 = 0.0;
    pub const LG: f32 = 0.0;
    pub const XL: f32 = 0.0;
}
/// Fixed layout: the panel widths of the design frame.
pub mod layout {
    pub const TITLE_BAR: f32 = 42.0;
    pub const TRANSPORT: f32 = 64.0;
    pub const BROWSER: f32 = 260.0;
    pub const BROWSER_MIN: f32 = 220.0;
    pub const INSPECTOR: f32 = 300.0;
    pub const INSPECTOR_MIN: f32 = 260.0;
    pub const AGENT: f32 = 380.0;
    pub const AGENT_MIN: f32 = 320.0;
    pub const AGENT_RAIL: f32 = 32.0;
    pub const TOOLBAR: f32 = 40.0;
    pub const RULER: f32 = 34.0;
    pub const TRACK_HEADER: f32 = 228.0;
    pub const TRACK_HEIGHT: f32 = 88.0;
    pub const EDITOR: f32 = 440.0;
    pub const ARRANGEMENT_MIN: f32 = 420.0;
    pub const WINDOW_MIN_W: f32 = 1120.0;
    pub const WINDOW_MIN_H: f32 = 760.0;
}

/// The region editor's fixed sizes.
pub mod editor {
    /// The keyboard column at the left of the roll and the controller lane's head.
    pub const KEY_COLUMN: f32 = 56.0;
    /// One key row of the piano roll.
    pub const KEY_ROW: f32 = 12.0;
    /// The bar ruler over the roll.
    pub const RULER: f32 = 18.0;
    /// The controller lane under the roll.
    pub const CONTROLLER_LANE: f32 = 96.0;
    /// Radius of a controller point, how near the pointer must come to grab one, and the
    /// space kept above the highest and below the lowest value.
    pub const CONTROLLER_POINT: f32 = 3.0;
    pub const CONTROLLER_GRIP: f32 = 6.0;
    pub const CONTROLLER_INSET: f32 = 6.0;
    /// Grab zone at a note's right edge for resizing.
    pub const NOTE_EDGE_GRIP: f32 = 5.0;
    /// Distance between staff lines, and a note head's radius, in the score view.
    pub const STAFF_GAP: f32 = 8.0;
    pub const NOTE_HEAD_R: f32 = 3.5;
}

/// The interface face: Chakra Petch, a corner cut off every letter (bundled, OFL).
pub const FONT_UI: &str = "Chakra Petch";
/// Numbers, time, code and labels in caps (bundled, OFL).
pub const FONT_MONO: &str = "IBM Plex Mono";

#[cfg(test)]
mod tests {
    use super::*;

    fn linear(c: f32) -> f32 {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    fn luminance(c: Rgba) -> f32 {
        0.2126 * linear(c.r) + 0.7152 * linear(c.g) + 0.0722 * linear(c.b)
    }
    fn rgba(c: Hsla) -> Rgba {
        c.into()
    }
    fn contrast(a: Rgba, b: Rgba) -> f32 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }
    /// What a surface can look like behind its text: over the page (plain, and at its densest
    /// grain and dither), the page itself over the brightest and the darkest desktop.
    fn composites(theme: &Theme, surface: Hsla) -> Vec<Rgba> {
        let mut out = Vec::new();
        for desk in [gpui::white(), gpui::black()] {
            let page = over(theme.backdrop, desk);
            let lit = over(with_alpha(theme.ink, theme.grain + theme.dither), page);
            for base in [page, lit] {
                out.push(rgba(over(surface, base)));
            }
        }
        out
    }

    #[test]
    fn text_is_readable_on_every_surface_and_glass_tier_in_both_modes() {
        for mode in [Mode::Dark, Mode::Light] {
            for opaque in [false, true] {
                let t = Theme::new(mode, opaque);
                let surfaces = [
                    ("glass 1", t.glass(1)),
                    ("glass 2", t.glass(2)),
                    ("glass 3", t.glass(3)),
                    ("lane", t.lane),
                    ("editor", t.editor),
                    ("control", t.control),
                    ("raised", t.bg_raised),
                    ("display", t.display),
                ];
                for (name, surface) in surfaces {
                    for base in composites(&t, surface) {
                        for (ink_name, ink, min) in [
                            ("text", t.text, 4.5),
                            ("text 2", t.text_2, 4.5),
                            ("text 3", t.text_3, 3.0),
                            ("accent", t.accent_text, 3.0),
                        ] {
                            let ratio = contrast(rgba(over(ink, Hsla::from(base))), base);
                            assert!(
                                ratio >= min,
                                "{mode:?} {ink_name} on {name}: {ratio:.2} < {min}"
                            );
                        }
                    }
                }
                // A chosen thing is inverted: paper on ink; lit keys (mute, solo, and red for
                // record) carry the paper too.
                for (name, fill) in [
                    ("accent", t.accent_fill),
                    ("record", t.record),
                    ("mute", t.mute),
                    ("solo", t.solo),
                ] {
                    let ratio = contrast(rgba(t.text_on_accent), rgba(fill));
                    assert!(ratio >= 4.5, "{mode:?} paper on {name}: {ratio:.2}");
                }
            }
        }
    }

    #[test]
    fn css_colors_from_documents_parse() {
        assert!(css_color("#ff0000").is_some());
        assert!(css_color("#f00").is_some());
        assert!(css_color("oklch(0.72 0.14 40)").is_some());
        assert!(css_color("rgba(20, 22, 32, .5)").is_some());
        assert!(css_color("nonsense").is_none());
        let red: Rgba = css_color("#ff0000").unwrap().into();
        assert!((red.r - 1.0).abs() < 1e-3 && red.g < 1e-3);
        // OKLCH white is white.
        let w: Rgba = oklch(1.0, 0.0, 0.0, 1.0).into();
        assert!(w.r > 0.99 && w.g > 0.99 && w.b > 0.99);
    }

    #[test]
    fn a_colour_keeps_its_hue_at_another_lightness() {
        let base = oklch(0.6, 0.12, 250.0, 1.0);
        let same: Rgba = with_lightness(base, 0.6).into();
        let b: Rgba = base.into();
        assert!(
            (same.r - b.r).abs() < 0.01
                && (same.g - b.g).abs() < 0.01
                && (same.b - b.b).abs() < 0.01
        );
        let lighter: Rgba = with_lightness(base, 0.8).into();
        assert!(luminance(lighter) > luminance(b));
        // Blue stays blue.
        assert!(lighter.b > lighter.r && lighter.b > lighter.g);
    }

    fn to_hex(c: Hsla) -> String {
        let c: Rgba = c.into();
        let byte = |v: f32| (v * 255.0).round() as u32;
        format!("#{:02x}{:02x}{:02x}", byte(c.r), byte(c.g), byte(c.b))
    }

    #[test]
    fn the_accent_is_the_ink_of_the_mode() {
        assert_eq!(to_hex(Theme::new(Mode::Dark, false).accent), "#ffffff");
        assert_eq!(to_hex(Theme::new(Mode::Light, false).accent), "#000000");
    }

    /// The palette follows lsuite's tokens (`desktop/assets/lsuite/tokens.json`, copied from
    /// lsuite `design/tokens.json`): when the suite changes a value, copy the file and fix
    /// the theme until this passes.
    #[test]
    fn the_palette_matches_the_lsuite_tokens() {
        let tokens: serde_json::Value =
            serde_json::from_str(include_str!("../../assets/lsuite/tokens.json")).unwrap();
        assert_eq!(tokens["version"], 2);
        for (mode, name) in [(Mode::Dark, "dark"), (Mode::Light, "light")] {
            let t = Theme::new(mode, true);
            let n = &tokens["neutral"][name];
            for (key, value) in [
                ("bg", t.bg),
                ("bg-raised", t.bg_raised),
                ("bg-sunken", t.bg_sunken),
                ("text", t.text),
                ("text-2", t.text_2),
                ("text-on-accent", t.text_on_accent),
                ("ink", t.accent),
                ("danger", t.danger),
            ] {
                assert_eq!(n[key].as_str().unwrap(), to_hex(value), "{name} {key}");
            }
            let g = &tokens["glass"][name];
            assert_eq!(
                g["opaque"].as_str().unwrap(),
                to_hex(t.glass_1.opaque),
                "{name}"
            );
            let strength = |k: &str| g[k].as_str().unwrap().parse::<f32>().unwrap();
            assert!((strength("grain") - t.grain).abs() < 1e-6, "{name} grain");
            assert!(
                (strength("dither") - t.dither).abs() < 1e-6,
                "{name} dither"
            );
            for r in ["xs", "sm", "md", "lg", "xl"] {
                assert_eq!(tokens["radius"][r], "0px");
            }
        }
        assert_eq!(radius::LG, 0.0);
        assert!(tokens["font"]["sans"].as_str().unwrap().contains(FONT_UI));
        assert!(tokens["font"]["mono"].as_str().unwrap().contains(FONT_MONO));
    }
}

/// Arrangement sizes: clips, fades, the ruler's flags and the tempo track, on the same scale
/// as [`layout`].
pub mod arrange {
    /// Space between a lane's edge and the clips on it.
    pub const CLIP_INSET: f32 = 6.0;
    /// The title strip across the top of a clip.
    pub const CLIP_TITLE: f32 = 17.0;
    pub const CLIP_RADIUS: f32 = 0.0;
    /// Height of a note in a clip's MIDI preview.
    pub const CLIP_NOTE_H: f32 = 3.0;
    /// Grab zone at each clip edge for trimming.
    pub const CLIP_EDGE_GRIP: f32 = 8.0;
    /// Fade handle in an audio clip's title strip, and its grab zone.
    pub const FADE_HANDLE: f32 = 8.0;
    pub const FADE_GRIP: f32 = 7.0;
    /// Grab zone at each cycle-range edge in the ruler.
    pub const CYCLE_GRIP: f32 = 7.0;
    /// Marker flags sit in the ruler's lower half: their top and height.
    pub const MARKER_TOP: f32 = 18.0;
    pub const MARKER_H: f32 = 14.0;
    /// Beat ticks at the ruler's foot.
    pub const RULER_TICK: f32 = 7.0;
    /// The playhead's triangle in the ruler.
    pub const PLAYHEAD_FLAG_W: f32 = 14.0;
    pub const PLAYHEAD_FLAG_H: f32 = 9.0;
    /// The tempo track: its height, a point's radius and its grab zone.
    pub const TEMPO_LANE: f32 = 64.0;
    pub const TEMPO_POINT: f32 = 4.0;
    pub const TEMPO_GRIP: f32 = 8.0;
    /// The track colour down a header's left edge.
    pub const COLOR_STRIP: f32 = 5.0;
    pub const ZOOM_RAIL: f32 = 110.0;
    /// A name or tempo typed in place.
    pub const INLINE_INPUT_H: f32 = 24.0;
}

/// The arrangement's own inks: lanes, clip faces, fades, the ruler and its flags. Derived
/// from the theme so they follow the mode.
#[derive(Clone, Debug)]
pub struct Timeline {
    pub lane_top: Hsla,
    pub lane_bottom: Hsla,
    pub ruler_bar: Hsla,
    pub ruler_tick: Hsla,
    pub ruler_bottom: Hsla,
    pub cycle_edge: Hsla,
    pub cycle_handle: Hsla,
    pub drag_ghost: Hsla,
    pub drag_ghost_edge: Hsla,
    pub pencil_preview: Hsla,
    pub drop_target: Hsla,
    pub split_guide: Hsla,
    /// What a clip face mixes the track colour with, and how much of the colour it keeps at
    /// the top and the bottom.
    pub face_base: Hsla,
    pub face_top: f32,
    pub face_bottom: f32,
    pub clip_shadow: Hsla,
    pub clip_title_bottom: Hsla,
    pub clip_highlight: Hsla,
    pub clip_contact: Hsla,
    pub clip_selected: Hsla,
    pub midi_note: Hsla,
    pub waveform_mid: Hsla,
    pub fade_shade: Hsla,
    pub fade_curve: Hsla,
    pub fade_handle: Hsla,
    pub marker_flag: Hsla,
    pub marker_lane: Hsla,
    pub tempo_fill: Hsla,
    pub header: Hsla,
    /// The diagonal hatching of the lanes past the song's end.
    pub hatch: Hsla,
}

impl Theme {
    pub fn timeline(&self) -> Timeline {
        let dark = self.mode == Mode::Dark;
        let ink = |a: f32| if dark { white(a) } else { hexa(0x000000, a) };
        Timeline {
            lane_top: if dark { white(0.018) } else { white(0.6) },
            lane_bottom: if dark {
                black(0.32)
            } else {
                hexa(0x000000, 0.07)
            },
            ruler_bar: ink(if dark { 0.24 } else { 0.3 }),
            ruler_tick: ink(if dark { 0.1 } else { 0.13 }),
            ruler_bottom: if dark {
                black(0.55)
            } else {
                hexa(0x000000, 0.1)
            },
            cycle_edge: with_alpha(self.accent, 0.85),
            cycle_handle: with_alpha(self.accent, 0.95),
            drag_ghost: ink(if dark { 0.09 } else { 0.08 }),
            drag_ghost_edge: ink(0.42),
            pencil_preview: with_alpha(self.accent, 0.24),
            drop_target: with_alpha(self.accent, if dark { 0.09 } else { 0.1 }),
            split_guide: with_alpha(self.accent, 0.95),
            face_base: self.bg_raised,
            face_top: if dark { 0.64 } else { 0.6 },
            face_bottom: if dark { 0.52 } else { 0.7 },
            clip_shadow: if dark {
                black(0.45)
            } else {
                hexa(0x000000, 0.18)
            },
            clip_title_bottom: if dark {
                black(0.12)
            } else {
                hexa(0x000000, 0.05)
            },
            clip_highlight: if dark { white(0.16) } else { white(0.75) },
            clip_contact: if dark {
                black(0.32)
            } else {
                hexa(0x000000, 0.1)
            },
            clip_selected: self.text_display,
            midi_note: if dark {
                white(0.84)
            } else {
                hexa(0x000000, 0.72)
            },
            waveform_mid: if dark {
                white(0.28)
            } else {
                hexa(0x000000, 0.24)
            },
            fade_shade: if dark {
                black(0.38)
            } else {
                hexa(0x000000, 0.14)
            },
            fade_curve: if dark {
                white(0.86)
            } else {
                hexa(0x000000, 0.72)
            },
            fade_handle: if dark {
                white(0.94)
            } else {
                hexa(0x000000, 0.82)
            },
            marker_flag: if dark {
                hexa(0x0e0e0e, 0.9)
            } else {
                white(0.94)
            },
            marker_lane: with_alpha(self.marker, if dark { 0.28 } else { 0.3 }),
            tempo_fill: with_alpha(self.accent, 0.1),
            header: if dark { grey(0.045) } else { hex(0xf7f7f7) },
            hatch: ink(if dark { 0.1 } else { 0.08 }),
        }
    }
}
