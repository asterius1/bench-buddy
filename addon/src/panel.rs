//! The Splits panel drawn the way `modules/berserker/views/splits.html` draws it: the same colours, sizes,
//! columns, flags and hover cards, with Segoe UI (what the page's `system-ui` is on Windows) loaded
//! through Nexus in the page's sizes. Everything is placed by hand on one draw list per window;
//! imgui allows only one at a time (a second one panics), so hover cards are drawn afterwards, in
//! their own window beside the panel, as the page shows them.

use crate::{
    report::{clock, sgn, Border, Chip, Part, Report, Sec, Tone, Window as Cd, FILL_MODES, LATE_OK},
    State, SHOW_LIVE,
};
use nexus::{
    font::add_font_from_file,
    imgui::{sys, Condition, DrawListMut, MouseButton, StyleColor, StyleVar, Ui, Window, WindowFlags},
    texture::{get_texture, load_texture_from_url},
};
use std::{
    collections::{HashMap, HashSet},
    ffi::{c_char, CStr},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
};

// ---- colours (the page's tokens) ------------------------------------------------------

pub type C = [f32; 4];
const fn rgb(r: u8, g: u8, b: u8) -> C {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
}
const fn rgba(r: u8, g: u8, b: u8, a: f32) -> C {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a]
}
pub const INK: C = rgb(255, 255, 255);
pub const INK2: C = rgb(195, 194, 183);
pub const MUTED: C = rgb(137, 135, 129);
pub const BAD: C = rgb(255, 107, 97);
const WARN: C = rgb(240, 165, 58);
const OK: C = rgb(74, 194, 107);
const ACCENT: C = rgb(94, 162, 240);
const BORDER: C = rgba(255, 255, 255, 0.10);
const PANEL: C = rgba(34, 35, 38, 0.94);
const TIP: C = rgba(26, 26, 25, 0.97);
const SEC_BG: C = rgb(58, 60, 64);
const TILE: C = rgb(51, 51, 51);
const BLACK: C = rgb(0, 0, 0);
const HOVER: C = rgba(255, 255, 255, 0.04);

/// Tempo against the bench: grey within 50 ms, green ahead, red behind, full at 400 ms.
fn tempo(d: Option<f64>) -> C {
    let Some(d) = d.filter(|d| d.abs() > 50.0) else { return SEC_BG };
    let k = (0.35 + 0.65 * ((d.abs() - 50.0) / 350.0).min(1.0)) as f32;
    let to = if d > 0.0 { rgb(168, 32, 28) } else { rgb(24, 120, 58) };
    let m = |i: usize| SEC_BG[i] + (to[i] - SEC_BG[i]) * k;
    [m(0), m(1), m(2), 1.0]
}

/// The page's `cls`: red over +30 ms, green under -30 ms, muted between.
fn cls(ms: f64) -> C {
    if ms > 30.0 {
        BAD
    } else if ms < -30.0 {
        OK
    } else {
        MUTED
    }
}

// ---- fonts ----------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub enum F {
    Body,  // 15
    Small, // 13
    Tiny,  // 12
    Micro, // 10, text tiles
    SemiS, // 13 semibold
    SemiM, // 14 semibold
    Bold,  // 17 semibold
    H3,    // 16 semibold
    Lag,   // 26 semibold
    Big,   // 34 semibold
    Ttl,   // 14
    Nano,  // 11
}
// two pixels over the page's sizes: the page's sizes read too small in game
const FONTS: [(&str, &str, f32); 12] = [
    ("BENCH_BUDDY_BODY", "segoeui.ttf", 15.0),
    ("BENCH_BUDDY_SMALL", "segoeui.ttf", 13.0),
    ("BENCH_BUDDY_TINY", "segoeui.ttf", 12.0),
    ("BENCH_BUDDY_MICRO", "segoeui.ttf", 10.0),
    ("BENCH_BUDDY_SEMIS", "seguisb.ttf", 13.0),
    ("BENCH_BUDDY_SEMIM", "seguisb.ttf", 14.0),
    ("BENCH_BUDDY_BOLD", "seguisb.ttf", 17.0),
    ("BENCH_BUDDY_H3", "seguisb.ttf", 16.0),
    ("BENCH_BUDDY_LAG", "seguisb.ttf", 26.0),
    ("BENCH_BUDDY_BIG", "seguisb.ttf", 34.0),
    ("BENCH_BUDDY_TTL", "segoeui.ttf", 14.0),
    ("BENCH_BUDDY_NANO", "segoeui.ttf", 11.0),
];
static LOADED: [AtomicUsize; 12] = [const { AtomicUsize::new(0) }; 12];
/// Latin-1 plus the dashes, the ellipsis and the minus sign the page uses.
static RANGES: [u16; 9] = [0x0020, 0x00FF, 0x2013, 0x2014, 0x2026, 0x2026, 0x2212, 0x2212, 0];

unsafe extern "C-unwind" fn font_received(id: *const c_char, font: *mut sys::ImFont) {
    if id.is_null() {
        return;
    }
    let id = unsafe { CStr::from_ptr(id) }.to_string_lossy();
    if let Some(i) = FONTS.iter().position(|f| f.0 == id) {
        LOADED[i].store(font as usize, Ordering::Release);
    }
}

pub fn load_fonts() {
    let dir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    for (id, file, size) in FONTS {
        let path = std::path::Path::new(&dir).join("Fonts").join(file);
        if !path.exists() {
            continue;
        }
        let cfg = unsafe {
            let c = sys::ImFontConfig_ImFontConfig();
            (*c).GlyphRanges = RANGES.as_ptr();
            (*c).OversampleH = 2;
            &*c
        };
        add_font_from_file(id, &path, size, Some(cfg), font_received).revert_on_unload();
    }
}

/// Runs `f` with the font pushed (the default font while it is not loaded yet).
fn with_font<R>(font: F, f: impl FnOnce() -> R) -> R {
    let p = LOADED[font as usize].load(Ordering::Acquire) as *mut sys::ImFont;
    if !p.is_null() {
        unsafe { sys::igPushFont(p) };
    }
    let r = f();
    if !p.is_null() {
        unsafe { sys::igPopFont() };
    }
    r
}

/// The game draws the minus sign, the dashes and the ellipsis as "?" (seen in game), so they go
/// out as plain ASCII.
fn plain(s: &str) -> std::borrow::Cow<'_, str> {
    if !s.contains(['\u{2212}', '\u{2013}', '\u{2014}', '\u{2026}']) {
        return s.into();
    }
    s.replace(['\u{2212}', '\u{2013}', '\u{2014}'], "-").replace('\u{2026}', "...").into()
}

fn tw(ui: &Ui, font: F, s: &str) -> f32 {
    with_font(font, || ui.calc_text_size(plain(s).as_ref())[0])
}

/// Text with its top-left at `p`.
fn txt(dl: &DrawListMut, font: F, p: [f32; 2], c: C, s: &str) {
    with_font(font, || dl.add_text(p, c, plain(s).as_ref()));
}

/// Text ending at `p[0]`.
fn rtxt(ui: &Ui, dl: &DrawListMut, font: F, p: [f32; 2], c: C, s: &str) -> f32 {
    let w = tw(ui, font, s);
    txt(dl, font, [p[0] - w, p[1]], c, s);
    w
}

fn line_h(font: F) -> f32 {
    FONTS[font as usize].2 * 1.25
}

// ---- icons ----------------------------------------------------------------------------

static REQUESTED: Mutex<Option<HashSet<u32>>> = Mutex::new(None);

fn texture(id: u32, icons: &HashMap<u32, String>) -> Option<nexus::imgui::TextureId> {
    let key = format!("BENCH_BUDDY_{id}");
    if let Some(t) = get_texture(&key) {
        return Some(t.id());
    }
    let mut req = REQUESTED.lock().ok()?;
    let req = req.get_or_insert_with(HashSet::new);
    if let Some(url) = icons.get(&id) {
        if req.insert(id) {
            if let Some(i) = url.find("/file/") {
                load_texture_from_url(&key, &url[..i], &url[i..], None);
            }
        }
    }
    None
}

fn short(id: u32, name: &str) -> String {
    match id {
        23285 => "stow".into(),
        23275 => "dodge".into(),
        _ => name.chars().take(4).collect::<String>().to_lowercase(),
    }
}

/// A skill tile: the icon on black, or a grey tile with its short name (the page's `.tx`).
fn icon(ui: &Ui, dl: &DrawListMut, p: [f32; 2], s: f32, id: u32, name: &str, icons: &HashMap<u32, String>, ghost: bool) {
    let q = [p[0] + s, p[1] + s];
    let r = 3.0;
    match texture(id, icons) {
        Some(t) => {
            dl.add_rect(p, q, BLACK).filled(true).rounding(r).build();
            // the page greys a missed cast out at 35%
            let tint = if ghost { [0.75, 0.75, 0.75, 0.35] } else { INK };
            dl.add_image_rounded(t, p, q, r).col(tint).build();
        }
        None => {
            dl.add_rect(p, q, TILE).filled(true).rounding(r).build();
            let label = short(id, name);
            let w = tw(ui, F::Micro, &label);
            let c = if ghost { rgba(195, 194, 183, 0.35) } else { INK2 };
            txt(dl, F::Micro, [p[0] + (s - w) / 2.0, p[1] + (s - line_h(F::Micro)) / 2.0], c, &label);
        }
    }
}

fn dashed_rect(dl: &DrawListMut, a: [f32; 2], b: [f32; 2], c: C, t: f32) {
    let (dash, gap) = (4.0, 3.0);
    let seg = |p: [f32; 2], q: [f32; 2]| {
        let len = ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2)).sqrt();
        let (dx, dy) = ((q[0] - p[0]) / len, (q[1] - p[1]) / len);
        let mut d = 0.0;
        while d < len {
            let e = (d + dash).min(len);
            dl.add_line([p[0] + dx * d, p[1] + dy * d], [p[0] + dx * e, p[1] + dy * e], c).thickness(t).build();
            d += dash + gap;
        }
    };
    seg(a, [b[0], a[1]]);
    seg([b[0], a[1]], b);
    seg(b, [a[0], b[1]]);
    seg([a[0], b[1]], a);
}

/// The marks the page puts on a problem cast (`.ic.slow`, `.idle`, `.few`, `.nobuff`, `.extra`,
/// `.ghost`).
fn marks(ui: &Ui, dl: &DrawListMut, p: [f32; 2], s: f32, c: &Chip) {
    let q = [p[0] + s, p[1] + s];
    if c.kind == "ghost" {
        dashed_rect(dl, [p[0] - 1.0, p[1] - 1.0], [q[0] + 1.0, q[1] + 1.0], BAD, 1.5);
    }
    if c.has("slow") {
        dl.add_rect([p[0], q[1]], [q[0], q[1] + 3.0], WARN).filled(true).build();
    }
    if c.has("idle") {
        dl.add_rect([q[0] + 1.0, p[1] + 3.0], [q[0] + 4.0, q[1] - 3.0], WARN).filled(true).rounding(2.0).build();
    }
    if c.has("few") {
        dl.add_circle([q[0] + 1.5, q[1] + 1.5], 4.5, BLACK).filled(true).build();
        dl.add_circle([q[0] + 1.5, q[1] + 1.5], 3.5, BAD).filled(true).build();
    }
    if c.has("nobuff") {
        dl.add_circle([p[0] - 1.5, q[1] + 1.5], 4.5, BLACK).filled(true).build();
        dl.add_circle([p[0] - 1.5, q[1] + 1.5], 3.5, ACCENT).filled(true).build();
    }
    if c.kind == "extra" {
        let x = q[0] - 4.0;
        let y = p[1] - 7.0;
        for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
            txt(dl, F::SemiS, [x + dx, y + dy], BLACK, "+");
        }
        txt(dl, F::SemiS, [x, y], INK, "+");
    }
    let _ = ui;
}

// ---- the panel ------------------------------------------------------------------------

#[path = "scrap.rs"]
pub mod scrap;

/// What the mouse is over: a card to draw beside the panel, centred on `y`.
enum Tip {
    Chip(usize, usize, usize),
    Sec(usize, usize),
    Part(usize),
    Rot(usize),
    Live,
}

const COL_L: f32 = 58.0;
const COL_N1: f32 = 62.0;
const COL_N2: f32 = 72.0;
const GAP: f32 = 6.0;

//// What a click in the panel asks for.
enum Act {
    Fill(&'static str),
    /// show this pull (None: follow the newest)
    Pick(Option<String>),
    ToggleList,
    /// keep the list and the arrows to one build ("" every build)
    Build(&'static str),
    Scrap(scrap::SAct),
}

/// Where the shown pull sits among the logs, for the arrows and the title.
struct Nav {
    live: bool,
    following: bool,
    idx: Option<usize>,
    logs: Vec<String>,
    /// how the live capture is doing
    status: crate::LiveLine,
}

/// Where the pull list opens, set by the title each frame.
static LIST_AT: Mutex<[f32; 4]> = Mutex::new([0.0; 4]);

/// Draws the panel; returns false when it was closed.
pub fn draw(ui: &Ui, s: &mut State) -> bool {
    let colors = [
        ui.push_style_color(StyleColor::WindowBg, PANEL),
        ui.push_style_color(StyleColor::Border, BORDER),
        ui.push_style_color(StyleColor::ResizeGrip, rgba(255, 255, 255, 0.06)),
    ];
    let vars = [
        ui.push_style_var(StyleVar::WindowRounding(8.0)),
        ui.push_style_var(StyleVar::WindowPadding([12.0, 12.0])),
        ui.push_style_var(StyleVar::WindowBorderSize(1.0)),
    ];
    // the running pull, judged live, while following the newest; else the chosen log
    let scrapper = s.kind == "scrapper";
    let live = !scrapper && SHOW_LIVE.load(Ordering::Relaxed) && s.selected.is_none() && s.live_report.is_some();
    let shown_log = s.selected.clone().unwrap_or_else(|| s.latest.clone());
    let nav = Nav {
        live,
        following: s.selected.is_none(),
        idx: s.history.iter().filter(|h| s.build.is_empty() || h.kind == s.build).position(|h| h.log == shown_log),
        logs: s.history.iter().filter(|h| s.build.is_empty() || h.kind == s.build).map(|h| h.log.clone()).collect(),
        status: s.live_line(),
    };
    let mut tip: Option<(Tip, f32)> = None;
    let mut stip: Option<(scrap::STip, f32)> = None;
    let mut panel_box = [0.0f32; 4];
    let mut acts = Vec::new();
    Window::new("Golem Splits##bench_buddy")
        .size([640.0, 900.0], Condition::FirstUseEver)
        .position([20.0, 80.0], Condition::FirstUseEver)
        .size_constraints([420.0, 300.0], [2000.0, 3000.0])
        .flags(WindowFlags::NO_TITLE_BAR | WindowFlags::NO_SCROLLBAR | WindowFlags::NO_SCROLL_WITH_MOUSE | WindowFlags::NO_COLLAPSE)
        .build(ui, || {
            let wp = ui.window_pos();
            let ws = ui.window_size();
            panel_box = [wp[0], wp[1], ws[0], ws[1]];
            with_font(F::Body, || {
                let p0 = ui.cursor_screen_pos();
                let avail = ui.content_region_avail();
                let dl = ui.get_window_draw_list();
                let r = if live { s.live_report.as_ref() } else { s.report.as_ref() };
                if scrapper {
                    if let Some(v) = s.scrap.as_ref() {
                        let log = v["log"].as_str().unwrap_or("").to_string();
                        tip = title_row(ui, &dl, p0[0], p0[1], avail[0], &log, "Scrapper", &nav, &mut acts);
                        let y0 = p0[1] + line_h(F::Ttl) + 2.0;
                        let mut sa = Vec::new();
                        stip = scrap::draw(ui, &dl, [p0[0], y0], [avail[0], avail[1] - (y0 - p0[1])], v, s.sheet.as_ref(), s.show_sheet, s.scrap_all, &s.icons, &mut sa);
                        acts.extend(sa.into_iter().map(Act::Scrap));
                    } else {
                        txt(&dl, F::Small, p0, MUTED, &s.status);
                    }
                    drop(dl);
                    ui.dummy(avail);
                    return;
                }
                match r {
                    Some(r) => tip = draw_report(ui, &dl, p0, avail, r, &s.fill, &s.icons, &nav, &mut acts),
                    None => {
                        txt(&dl, F::Small, p0, MUTED, &s.status);
                        if live_status(ui, &dl, [p0[0], p0[1] + 20.0], None, &nav.status) {
                            tip = Some((Tip::Live, p0[1] + 28.0));
                        }
                    }
                }
                if r.is_some() && !s.status.is_empty() {
                    rtxt(ui, &dl, F::Tiny, [p0[0] + avail[0], p0[1] + avail[1] - line_h(F::Tiny)], MUTED, &s.status);
                }
                drop(dl);
                ui.dummy(avail);
            });
        });
    for v in vars {
        v.pop();
    }
    for c in colors {
        c.pop();
    }
    if s.list_open {
        if let Some(a) = draw_list(ui, s, &shown_log) {
            acts.push(a);
        }
    }
    for a in acts {
        match a {
            Act::Fill(m) => s.set_fill(m),
            Act::ToggleList => s.list_open = !s.list_open,
            Act::Build(k) => {
                s.build = k;
                // show that build's newest pull
                let newest = s.history.iter().rev().find(|h| k.is_empty() || h.kind == k).map(|h| h.log.clone());
                if let Some(log) = newest.filter(|_| !k.is_empty()) {
                    let log = Some(log).filter(|l| *l != s.latest);
                    s.selected = log.clone();
                    *crate::lock(&crate::WANT) = Some(log);
                }
            }
            Act::Scrap(scrap::SAct::Sheet) => s.show_sheet = !s.show_sheet,
            Act::Scrap(scrap::SAct::All) => s.scrap_all = !s.scrap_all,
            Act::Pick(log) => {
                s.list_open = false;
                let log = log.filter(|l| *l != s.latest);
                s.selected = log.clone();
                *crate::lock(&crate::WANT) = Some(log);
            }
        }
    }
    let r = if live { s.live_report.as_ref() } else { s.report.as_ref() };
    if let Some((t, y)) = stip {
        if !s.list_open {
            scrap::draw_tip(ui, t, y, panel_box, &s.icons);
        }
        return true;
    }
    if let Some((t, y)) = tip {
        if !s.list_open {
            draw_tip(ui, t, y, panel_box, r, &s.icons, &nav.status.details);
        }
    }
    true
}

/// "17 Oct 15:54" from a log name "20261017-155441".
fn log_label(log: &str) -> String {
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let g = |a: usize, b: usize| log.get(a..b).unwrap_or("");
    let m = g(4, 6).parse::<usize>().unwrap_or(1).clamp(1, 12);
    format!("{} {} {}:{}", g(6, 8).trim_start_matches('0'), M[m - 1], g(9, 11), g(11, 13))
}

/// The pull list under the title, newest first; returns the pick.
fn draw_list(ui: &Ui, s: &State, shown: &str) -> Option<Act> {
    let at = *crate::lock(&LIST_AT);
    let mut act = None;
    let colors = [ui.push_style_color(StyleColor::WindowBg, TIP), ui.push_style_color(StyleColor::Border, BORDER)];
    let vars = [
        ui.push_style_var(StyleVar::WindowRounding(8.0)),
        ui.push_style_var(StyleVar::WindowPadding([6.0, 6.0])),
        ui.push_style_var(StyleVar::WindowBorderSize(1.0)),
    ];
    let list: Vec<(usize, &crate::Hist)> = s.history.iter().enumerate().filter(|(_, h)| s.build.is_empty() || h.kind == s.build).collect();
    let rows = list.len().min(16) as f32;
    Window::new("##bench_buddy_pulls")
        .position([at[0], at[1] + at[3] + 4.0], Condition::Always)
        .size([400.0, rows * 22.0 + 12.0 + 26.0], Condition::Always)
        // reopened, the window keeps its old place behind the panel the click just raised: keep it on top
        .focused(true)
        .flags(WindowFlags::NO_DECORATION | WindowFlags::NO_SAVED_SETTINGS | WindowFlags::NO_NAV)
        .build(ui, || {
            with_font(F::Ttl, || {
                // the build filter
                let p = ui.cursor_screen_pos();
                let dl = ui.get_window_draw_list();
                let mut bx = p[0];
                for (k, label) in [("", "All builds"), ("berserker", "Condi Berserker"), ("scrapper", "Power Scrapper")] {
                    let bw = tw(ui, F::Small, label) + 16.0;
                    let (a, b) = ([bx, p[1]], [bx + bw, p[1] + 18.0]);
                    let on = s.build == k;
                    let hov = ui.is_mouse_hovering_rect(a, b);
                    if hov {
                        dl.add_rect(a, b, HOVER).filled(true).rounding(9.0).build();
                        if ui.is_mouse_clicked(MouseButton::Left) {
                            act = Some(Act::Build(k));
                        }
                    }
                    dl.add_rect(a, b, if on { ACCENT } else { BORDER }).rounding(9.0).build();
                    txt(&dl, F::Small, [bx + 8.0, p[1] + 1.0], if on { ACCENT } else { INK2 }, label);
                    bx += bw + 4.0;
                }
                drop(dl);
                ui.dummy([1.0, 22.0]);
                for &(k, h) in list.iter().rev() {
                    let p = ui.cursor_screen_pos();
                    let w = ui.content_region_avail()[0];
                    let hit = ui.invisible_button(format!("##pull{k}"), [w, 20.0]);
                    let dl = ui.get_window_draw_list();
                    let on = h.log == shown;
                    if on || ui.is_item_hovered() {
                        dl.add_rect(p, [p[0] + w, p[1] + 20.0], if on { rgba(94, 162, 240, 0.18) } else { HOVER }).filled(true).rounding(4.0).build();
                    }
                    let newest = k + 1 == s.history.len();
                    txt(&dl, F::Ttl, [p[0] + 6.0, p[1] + 2.0], if on { INK } else { INK2 }, &log_label(&h.log));
                    txt(&dl, F::Ttl, [p[0] + 110.0, p[1] + 2.0], INK, &clock(h.secs * 1000.0));
                    txt(&dl, F::Ttl, [p[0] + 170.0, p[1] + 2.0], MUTED, &format!("{} {}", h.loops, if h.kind == "scrapper" { "cycles" } else { "loops" }));
                    txt(&dl, F::Ttl, [p[0] + 230.0, p[1] + 2.0], if h.kind == "scrapper" { ACCENT } else { INK2 }, if h.kind == "scrapper" { "Scrapper" } else { "Berserker" });
                    if newest {
                        rtxt(ui, &dl, F::Small, [p[0] + w - 6.0, p[1] + 3.0], ACCENT, "newest");
                    }
                    drop(dl);
                    if hit {
                        act = Some(Act::Pick(Some(h.log.clone())));
                    }
                }
            });
            // a click anywhere else closes the list
            if act.is_none() && ui.is_mouse_clicked(MouseButton::Left) && !ui.is_window_hovered() {
                let m = ui.io().mouse_pos;
                let on_title = m[0] >= at[0] && m[0] <= at[0] + at[2] && m[1] >= at[1] && m[1] <= at[1] + at[3];
                if !on_title {
                    act = Some(Act::ToggleList);
                }
            }
        });
    for v in vars {
        v.pop();
    }
    for c in colors {
        c.pop();
    }
    act
}

/// A small chevron button; returns whether it was clicked.
fn chevron(ui: &Ui, dl: &DrawListMut, p: [f32; 2], left: bool, enabled: bool) -> bool {
    let (a, b) = (p, [p[0] + 16.0, p[1] + 16.0]);
    let hov = enabled && ui.is_mouse_hovering_rect(a, b);
    if hov {
        dl.add_rect(a, b, HOVER).filled(true).rounding(4.0).build();
    }
    let c = if !enabled { rgba(137, 135, 129, 0.35) } else if hov { INK } else { INK2 };
    let (x, y) = (p[0] + 8.0, p[1] + 8.0);
    let d = if left { 2.0 } else { -2.0 };
    dl.add_line([x + d, y - 4.0], [x - d, y], c).thickness(1.6).build();
    dl.add_line([x - d, y], [x + d, y + 4.0], c).thickness(1.6).build();
    hov && ui.is_mouse_clicked(MouseButton::Left)
}

fn swatch(dl: &DrawListMut, p: [f32; 2], kind: &str) {
    let q = [p[0] + 14.0, p[1] + 14.0];
    match kind {
        "ghost" => dashed_rect(dl, p, q, BAD, 1.5),
        "err" => dl.add_rect([p[0] + 1.0, p[1] + 1.0], [q[0] - 1.0, q[1] - 1.0], BAD).rounding(3.0).thickness(2.0).build(),
        "xtra" => dl.add_rect([p[0] + 1.0, p[1] + 1.0], [q[0] - 1.0, q[1] - 1.0], WARN).rounding(3.0).thickness(2.0).build(),
        _ => {
            dl.add_rect(p, q, rgb(85, 85, 85)).filled(true).rounding(3.0).build();
            match kind {
                "slow" => dl.add_rect([p[0], q[1]], [q[0], q[1] + 3.0], WARN).filled(true).build(),
                "few" => dl.add_circle([q[0], q[1]], 3.0, BAD).filled(true).build(),
                "extra" => txt(dl, F::SemiS, [q[0] - 4.0, p[1] - 7.0], INK, "+"),
                _ => {}
            }
        }
    }
}

/// The title row: previous, the pull (opens the list), next; "newest" when looking back; the live
/// status on the right. Returns the hovered card.
fn title_row(ui: &Ui, dl: &DrawListMut, x0: f32, y: f32, w: f32, log: &str, kind: &str, nav: &Nav, acts: &mut Vec<Act>) -> Option<(Tip, f32)> {
    let mut tip = None;
    let n = nav.logs.len();
    // from the live pull the previous one is the newest log
    let prev = if nav.live { n.checked_sub(1) } else { nav.idx.and_then(|i| i.checked_sub(1)) };
    if chevron(ui, dl, [x0 - 2.0, y - 1.0], true, prev.is_some()) {
        if let Some(i) = prev {
            acts.push(Act::Pick(Some(nav.logs[i].clone())));
        }
    }
    let title = if nav.live {
        "Live pull · filling in · vs bench".to_string()
    } else {
        let tl = log.get(..8).map(|_| log_label(log)).unwrap_or_default();
        format!("{kind} golem pull · {tl} · vs bench")
    };
    let tx = x0 + 18.0;
    let ttw = tw(ui, F::Ttl, &title);
    let (ta, tb) = ([tx - 2.0, y - 1.0], [tx + ttw + 16.0, y + 15.0]);
    let th = ui.is_mouse_hovering_rect(ta, tb);
    if th {
        dl.add_rect(ta, tb, HOVER).filled(true).rounding(4.0).build();
        if ui.is_mouse_clicked(MouseButton::Left) {
            acts.push(Act::ToggleList);
        }
    }
    *crate::lock(&LIST_AT) = [ta[0], ta[1], tb[0] - ta[0], tb[1] - ta[1]];
    txt(dl, F::Ttl, [tx, y], if nav.live { ACCENT } else if th { INK } else { MUTED }, &title);
    // the list's caret
    let (cx, cy) = (tx + ttw + 8.0, y + 8.0);
    dl.add_triangle([cx - 3.5, cy - 2.0], [cx + 3.5, cy - 2.0], [cx, cy + 2.5], MUTED).filled(true).build();
    let newer = !nav.following;
    if chevron(ui, dl, [tb[0] + 2.0, y - 1.0], false, newer) {
        let next = nav.idx.map_or(n, |i| i + 1);
        acts.push(Act::Pick(if next + 1 >= n { None } else { Some(nav.logs[next].clone()) }));
    }
    if newer {
        let nx = tb[0] + 24.0;
        let label = "newest";
        let bw = tw(ui, F::Small, label) + 16.0;
        let (a, b) = ([nx, y - 1.0], [nx + bw, y + 15.0]);
        let hov = ui.is_mouse_hovering_rect(a, b);
        if hov {
            dl.add_rect(a, b, HOVER).filled(true).rounding(9.0).build();
            if ui.is_mouse_clicked(MouseButton::Left) {
                acts.push(Act::Pick(None));
            }
        }
        dl.add_rect(a, b, ACCENT).rounding(9.0).build();
        txt(dl, F::Small, [nx + 8.0, y], ACCENT, label);
    }
    // the live capture's status, right-aligned on the title row
    if live_status(ui, dl, [x0 + w, y], Some(tb[0] + 90.0), &nav.status) {
        tip = Some((Tip::Live, y + 8.0));
    }
    tip
}

/// Draws the report; returns the hovered card and pushes clicks to `acts`.
#[allow(clippy::too_many_arguments)]
fn draw_report(
    ui: &Ui,
    dl: &DrawListMut,
    p: [f32; 2],
    size: [f32; 2],
    r: &Report,
    fill: &str,
    icons: &HashMap<u32, String>,
    nav: &Nav,
    acts: &mut Vec<Act>,
) -> Option<(Tip, f32)> {
    let (x0, w) = (p[0], size[0]);
    let mut y = p[1];
    let mut tip = title_row(ui, dl, x0, y, w, &r.log, "Berserker", nav, acts);
    y += line_h(F::Ttl) + 2.0;
    txt(dl, F::Big, [x0, y - 4.0], INK, &clock(r.secs * 1000.0));
    let lag_y = y + 34.0 - 26.0 - 12.0;
    let lw = rtxt(ui, dl, F::Lag, [x0 + w, lag_y - 4.0], BAD, &sgn(r.lag_end));
    rtxt(ui, dl, F::Small, [x0 + w, lag_y + 26.0], MUTED, "behind bench at end");
    if ui.is_mouse_hovering_rect([x0 + w - lw.max(110.0), lag_y], [x0 + w, lag_y + 40.0]) {
        tip = Some((Tip::Rot(r.parts.len().saturating_sub(1)), lag_y + 20.0));
    }
    y += 34.0 + 4.0;
    let mut cx = x0;
    for (k, (n, label)) in ["slow", "ghost", "few", "extra"].iter().zip(r.counts.iter().zip(["slow", "missed", "missed hits", "extra"])) {
        swatch(dl, [cx, y + 1.0], k);
        cx += 19.0;
        let ns = n.to_string();
        txt(dl, F::SemiM, [cx, y], INK, &ns);
        cx += tw(ui, F::SemiM, &ns) + 4.0;
        txt(dl, F::Ttl, [cx, y], INK2, label);
        cx += tw(ui, F::Ttl, label) + 14.0;
    }
    if let Some((s, d)) = r.damage {
        let pct = format!("{}{:.1}%", if d > 0.0 { "+" } else { "\u{2212}" }, d.abs());
        let pw = rtxt(ui, dl, F::SemiM, [x0 + w, y], if d < 0.0 { BAD } else { OK }, &pct);
        rtxt(ui, dl, F::Ttl, [x0 + w - pw - 4.0, y], MUTED, &format!("Damage at {s} s"));
    }
    y += 16.0 + 8.0;
    dl.add_line([x0, y], [x0 + w, y], BORDER).build();
    // filler mode buttons
    y += 6.0;
    txt(dl, F::Small, [x0, y], MUTED, "Fillers judged by");
    let mut bx = x0 + tw(ui, F::Small, "Fillers judged by") + 6.0;
    for (k, label) in FILL_MODES {
        let bw = tw(ui, F::Small, label) + 16.0;
        let (a, b) = ([bx, y - 1.0], [bx + bw, y + 15.0]);
        let on = k == fill;
        let hov = ui.is_mouse_hovering_rect(a, b);
        if hov {
            dl.add_rect(a, b, HOVER).filled(true).rounding(9.0).build();
            if ui.is_mouse_clicked(MouseButton::Left) {
                acts.push(Act::Fill(k));
            }
        }
        dl.add_rect(a, b, if on { ACCENT } else { BORDER }).rounding(9.0).build();
        txt(dl, F::Small, [bx + 8.0, y], if on { ACCENT } else { INK2 }, label);
        bx += bw + 4.0;
    }
    y += 16.0 + 6.0;
    // column heads
    let n1x = x0 + w - COL_N2 - GAP - COL_N1;
    txt(dl, F::Small, [x0, y], MUTED, "Split");
    txt(dl, F::Small, [x0 + COL_L + GAP, y], MUTED, "Sections · problem casts");
    rtxt(ui, dl, F::Small, [n1x + COL_N1, y], MUTED, "Split");
    rtxt(ui, dl, F::Small, [x0 + w, y], MUTED, "Behind");
    y += 14.0 + 3.0;
    // rows share what is left above the footer, at most 52 each
    let foot_h = 7.0 + 16.0;
    let swords = r.parts.iter().filter(|x| x.kind == "sword").count() as f32;
    let rows = r.parts.len().max(1) as f32;
    let room = p[1] + size[1] - foot_h - y - 2.0 * (rows - 1.0) - 3.0 * swords;
    let h = (room / rows).min(56.0).max(24.0);
    for (i, x) in r.parts.iter().enumerate() {
        if x.kind == "sword" {
            y += 3.0;
        }
        if let Some(t) = draw_split(ui, dl, [x0, y], w, h, i, x, icons) {
            tip = Some((t, y + h / 2.0));
        }
        y += h + 2.0;
    }
    // footer legend
    let fy = (p[1] + size[1] - foot_h).max(y);
    dl.add_line([x0, fy], [x0 + w, fy], BORDER).build();
    let mut fx = x0;
    let fy = fy + 7.0;
    for (k, label) in [
        ("slow", "next cast late"),
        ("ghost", "missed"),
        ("few", "missed hits"),
        ("extra", "extra"),
        ("tempo", "ahead / behind"),
        ("err", "missed cast or hits"),
        ("xtra", "extra"),
    ] {
        if k == "tempo" {
            let (a, b) = ([fx, fy + 2.0], [fx + 46.0, fy + 12.0]);
            let (g, m, rr) = (rgb(24, 120, 58), SEC_BG, rgb(168, 32, 28));
            let pack = |c: C| nexus::imgui::ImColor32::from_rgba_f32s(c[0], c[1], c[2], c[3]);
            dl.add_rect_filled_multicolor(a, [a[0] + 23.0, b[1]], pack(g), pack(m), pack(m), pack(g));
            dl.add_rect_filled_multicolor([a[0] + 23.0, a[1]], b, pack(m), pack(rr), pack(rr), pack(m));
            fx += 51.0;
        } else {
            swatch(dl, [fx, fy], k);
            fx += 19.0;
        }
        txt(dl, F::Small, [fx, fy], MUTED, label);
        fx += tw(ui, F::Small, label) + 12.0;
    }
    tip
}

#[allow(clippy::too_many_arguments)]
fn draw_split(ui: &Ui, dl: &DrawListMut, p: [f32; 2], w: f32, h: f32, i: usize, x: &Part, icons: &HashMap<u32, String>) -> Option<Tip> {
    let mut tip = None;
    let b = [p[0] + w, p[1] + h];
    if ui.is_mouse_hovering_rect(p, b) {
        dl.add_rect(p, b, HOVER).filled(true).rounding(4.0).build();
    }
    // label
    let cy = p[1] + h / 2.0;
    txt(dl, F::Bold, [p[0] + 4.0, cy - 19.0], INK, &x.num);
    txt(dl, F::Small, [p[0] + 4.0, cy + 2.0], MUTED, x.name);
    if ui.is_mouse_hovering_rect(p, [p[0] + COL_L, b[1]]) {
        tip = Some(Tip::Part(i));
    }
    // sections
    let sx = p[0] + COL_L + GAP;
    let sw = w - COL_L - COL_N1 - COL_N2 - 3.0 * GAP;
    let k = x.secs.len().max(1) as f32;
    let secw = (sw - 3.0 * (k - 1.0)) / k;
    for (j, s) in x.secs.iter().enumerate() {
        let a = [sx + j as f32 * (secw + 3.0), p[1]];
        if let Some(t) = draw_sec(ui, dl, a, [a[0] + secw, b[1]], i, j, x, s, icons) {
            tip = Some(t);
        }
    }
    // split: tempo box, delta and visit length
    let a = [sx + sw + GAP, p[1]];
    let e = [a[0] + COL_N1, b[1]];
    dl.add_rect(a, e, tempo(Some(x.delta))).filled(true).rounding(4.0).build();
    rtxt(ui, dl, F::Bold, [e[0] - 4.0, cy - 19.0], INK, &sgn(x.delta));
    let len = x.len.map_or("unfinished".to_string(), |l| format!("{:.2} s", l / 1000.0));
    rtxt(ui, dl, F::Tiny, [e[0] - 4.0, cy + 3.0], MUTED, &len);
    if ui.is_mouse_hovering_rect(a, e) {
        tip = Some(Tip::Part(i));
    }
    // behind
    let c = [e[0] + GAP, p[1]];
    let lag_c = if x.delta > 30.0 { BAD } else if x.delta < -30.0 { OK } else { INK };
    rtxt(ui, dl, F::Bold, [b[0] - 4.0, cy - 19.0], lag_c, &sgn(x.lag_end));
    if let Some(be) = x.bench_end {
        rtxt(ui, dl, F::Tiny, [b[0] - 4.0, cy + 3.0], MUTED, &format!("bench {}", clock(be)));
    }
    if ui.is_mouse_hovering_rect(c, b) {
        tip = Some(Tip::Rot(i));
    }
    tip
}

const MAXIC: [(&str, usize); 3] = [("opener", 12), ("sword", 10), ("bow", 8)];

#[allow(clippy::too_many_arguments)]
fn draw_sec(ui: &Ui, dl: &DrawListMut, a: [f32; 2], b: [f32; 2], i: usize, j: usize, x: &Part, s: &Sec, icons: &HashMap<u32, String>) -> Option<Tip> {
    let hovered = ui.is_mouse_hovering_rect(a, b);
    dl.add_rect(a, b, tempo(s.tempo)).filled(true).rounding(4.0).build();
    let (ia, ib) = ([a[0] + 1.0, a[1] + 1.0], [b[0] - 1.0, b[1] - 1.0]);
    match s.border {
        Border::Err => dl.add_rect(ia, ib, BAD).rounding(4.0).thickness(2.0).build(),
        Border::Extra => dl.add_rect(ia, ib, WARN).rounding(4.0).thickness(2.0).build(),
        Border::Skip => dashed_rect(dl, ia, ib, BAD, 2.0),
        Border::None => {}
    }
    if hovered {
        dl.add_rect(a, b, ACCENT).rounding(4.0).build();
    }
    // top line: name (and the fill budget) left, the judged time right
    let ty = a[1] + 3.0;
    let tc = match s.tone {
        Tone::Ink => INK,
        Tone::Muted => MUTED,
        Tone::Bad => BAD,
    };
    let rw = rtxt(ui, dl, F::SemiS, [b[0] - 7.0, ty - 1.0], tc, &s.text);
    let room_top = b[0] - a[0] - 14.0 - rw - 4.0;
    txt(dl, F::Small, [a[0] + 7.0, ty - 1.0], INK2, &s.label);
    if !s.budget.is_empty() {
        let lw = tw(ui, F::Small, &s.label) + 4.0;
        if lw + tw(ui, F::Tiny, &s.budget) <= room_top {
            txt(dl, F::Tiny, [a[0] + 7.0 + lw, ty], INK2, &s.budget);
        }
    }
    // problem casts
    let y = ty + 15.0;
    let pr: Vec<(usize, &Chip)> = s.chips.iter().enumerate().filter(|(_, c)| c.problem()).collect();
    let mut tipc = None;
    if pr.is_empty() {
        txt(dl, F::Small, [a[0] + 7.0, y + 4.0], rgba(137, 135, 129, 0.6), "clean");
    } else {
        let cap = MAXIC.iter().find(|m| m.0 == x.kind).map_or(10, |m| m.1);
        let room = b[0] - a[0] - 14.0;
        let n = pr.len().min(cap) as f32;
        let size = ((room - 3.0 * (n - 1.0)) / n).floor().min(24.0).min(b[1] - y - 4.0).max(10.0);
        let fit = ((room + 3.0) / (size + 3.0)).floor().max(1.0) as usize;
        let mut show = pr.len().min(cap);
        if show > fit {
            show = fit.saturating_sub(1).max(1);
        }
        for (k, (ci, c)) in pr.iter().take(show).enumerate() {
            let p = [a[0] + 7.0 + k as f32 * (size + 3.0), y];
            icon(ui, dl, p, size, c.id, &c.name, icons, c.kind == "ghost");
            marks(ui, dl, p, size, c);
            if ui.is_mouse_hovering_rect(p, [p[0] + size, p[1] + size]) {
                dl.add_rect([p[0] - 1.0, p[1] - 1.0], [p[0] + size + 1.0, p[1] + size + 1.0], INK).rounding(3.0).build();
                tipc = Some(*ci);
            }
        }
        if pr.len() > show {
            txt(dl, F::Small, [a[0] + 7.0 + show as f32 * (size + 3.0), y + size / 2.0 - 7.0], MUTED, &format!("+{}", pr.len() - show));
        }
    }
    match (tipc, hovered) {
        (Some(c), _) => Some(Tip::Chip(i, j, c)),
        (None, true) => Some(Tip::Sec(i, j)),
        _ => None,
    }
}

// ---- hover cards ----------------------------------------------------------------------

fn where_(x: &Part) -> String {
    if x.kind == "opener" {
        "Opener".into()
    } else {
        format!("Loop {} {}", x.num, x.name)
    }
}

fn secs(ms: Option<f64>) -> String {
    ms.map_or("\u{2014}".into(), |v| format!("{:.2} s", v / 1000.0))
}

const TIP_W: f32 = 440.0;

/// The live status: a dot and a short line; ending at `p[0]` when `min_x` is given (not
/// further left than that), else starting at `p[0]`. Returns whether the mouse is on it.
fn live_status(ui: &Ui, dl: &DrawListMut, p: [f32; 2], min_x: Option<f32>, st: &crate::LiveLine) -> bool {
    let c = match st.tone {
        1 => OK,
        2 => WARN,
        3 => BAD,
        _ => MUTED,
    };
    let mut text = st.text.clone();
    let room = min_x.map_or(f32::MAX, |m| p[0] - m - 12.0);
    while tw(ui, F::Small, &text) > room && text.chars().count() > 6 {
        text = text.chars().take(text.chars().count() - 2).collect::<String>();
        text.push('…');
    }
    let w = tw(ui, F::Small, &text) + 12.0;
    let x = if min_x.is_some() { p[0] - w } else { p[0] };
    dl.add_circle([x + 4.0, p[1] + 7.5], 3.5, c).filled(true).build();
    txt(dl, F::Small, [x + 12.0, p[1]], if st.tone == 0 { MUTED } else { c }, &text);
    ui.is_mouse_hovering_rect([x, p[1] - 1.0], [x + w, p[1] + 15.0])
}

#[allow(clippy::too_many_arguments)]
fn draw_tip(ui: &Ui, t: Tip, y: f32, panel: [f32; 4], r: Option<&Report>, icons: &HashMap<u32, String>, live: &[String]) {
    let colors = [ui.push_style_color(StyleColor::WindowBg, TIP), ui.push_style_color(StyleColor::Border, BORDER)];
    let vars = [
        ui.push_style_var(StyleVar::WindowRounding(8.0)),
        ui.push_style_var(StyleVar::WindowPadding([12.0, 10.0])),
        ui.push_style_var(StyleVar::WindowBorderSize(1.0)),
    ];
    let disp = ui.io().display_size;
    let right = panel[0] + panel[2] + 12.0;
    let x = if right + TIP_W < disp[0] { right } else { (panel[0] - 12.0 - TIP_W).max(0.0) };
    Window::new("##bench_buddy_tip")
        .position([x, y], Condition::Always)
        .position_pivot([0.0, 0.5])
        .flags(
            WindowFlags::NO_DECORATION
                | WindowFlags::ALWAYS_AUTO_RESIZE
                | WindowFlags::NO_INPUTS
                | WindowFlags::NO_FOCUS_ON_APPEARING
                | WindowFlags::NO_NAV
                | WindowFlags::NO_SAVED_SETTINGS,
        )
        .build(ui, || {
            with_font(F::Body, || {
                let p0 = ui.cursor_screen_pos();
                let dl = ui.get_window_draw_list();
                let h = match (t, r) {
                    (Tip::Live, _) => live_tip(&dl, p0, live),
                    (Tip::Chip(i, j, c), Some(r)) => chip_tip(ui, &dl, p0, r, i, &r.parts[i].secs[j], &r.parts[i].secs[j].chips[c], icons),
                    (Tip::Sec(i, j), Some(r)) => sec_tip(ui, &dl, p0, r, i, &r.parts[i].secs[j], icons),
                    (Tip::Part(i), Some(r)) => part_tip(ui, &dl, p0, r, &r.parts[i]),
                    (Tip::Rot(i), Some(r)) => rot_tip(ui, &dl, p0, r, i),
                    _ => 0.0,
                };
                drop(dl);
                ui.dummy([TIP_W - 24.0, h]);
            });
        });
    for v in vars {
        v.pop();
    }
    for c in colors {
        c.pop();
    }
}

/// A "term  value" list like the page's `dl`; values are runs of (text, colour). Returns its height.
fn deflist(ui: &Ui, dl: &DrawListMut, p: [f32; 2], rows: &[(String, Vec<(String, C)>)]) -> f32 {
    let kw = rows.iter().map(|r| tw(ui, F::Ttl, &r.0)).fold(0.0, f32::max) + 12.0;
    let mut y = p[1];
    for (k, v) in rows {
        txt(dl, F::Ttl, [p[0], y], MUTED, k);
        let mut x = p[0] + kw;
        for (s, c) in v {
            txt(dl, F::Ttl, [x, y], *c, s);
            x += tw(ui, F::Ttl, s);
        }
        y += 18.0;
    }
    y - p[1]
}

fn tag(ui: &Ui, dl: &DrawListMut, p: [f32; 2], s: &str, c: C) -> f32 {
    let w = tw(ui, F::Small, s) + 12.0;
    dl.add_rect([p[0], p[1]], [p[0] + w, p[1] + 16.0], c).rounding(9.0).build();
    txt(dl, F::Small, [p[0] + 6.0, p[1] + 1.0], c, s);
    w
}

fn v(s: impl Into<String>, c: C) -> (String, C) {
    (s.into(), c)
}

#[allow(clippy::too_many_arguments)]
fn chip_tip(ui: &Ui, dl: &DrawListMut, p: [f32; 2], r: &Report, i: usize, s: &Sec, c: &Chip, icons: &HashMap<u32, String>) -> f32 {
    let x = &r.parts[i];
    icon(ui, dl, p, 28.0, c.id, &c.name, icons, false);
    let mut hx = p[0] + 36.0;
    txt(dl, F::H3, [hx, p[1] + 5.0], INK, &c.name);
    hx += tw(ui, F::H3, &c.name) + 8.0;
    let mut tags: Vec<(&str, C)> = Vec::new();
    if c.kind == "ghost" {
        tags.push(("missed", BAD));
    }
    if c.kind == "extra" {
        tags.push(("extra", MUTED));
    }
    for f in &c.flags {
        match f.as_str() {
            "slow" => tags.push(("next cast late", WARN)),
            "idle" => tags.push(("wait after", WARN)),
            "few" => tags.push(("missed hits", BAD)),
            "nobuff" => tags.push(("no buff", ACCENT)),
            _ => {}
        }
    }
    for (t, col) in tags {
        hx += tag(ui, dl, [hx, p[1] + 6.0], t, col) + 4.0;
    }
    let mut y = p[1] + 34.0;
    txt(dl, F::Ttl, [p[0], y], MUTED, &format!("{} · {}", where_(x), s.long));
    y += 20.0;
    let mut rows: Vec<(String, Vec<(String, C)>)> = Vec::new();
    if c.kind == "ghost" {
        rows.push(("Bench cast".into(), vec![v(secs(c.ref_dur), INK)]));
        rows.push(("You".into(), vec![v("did not cast it", INK)]));
    } else {
        if let Some(sl) = c.slot {
            let mut vv = vec![v(secs(Some(sl)), INK)];
            if let Some(b) = c.ref_slot {
                vv.push(v(format!("  bench {}", secs(Some(b))), MUTED));
            }
            rows.push(("Start to next cast".into(), vv));
        }
        if let Some(l) = c.lost {
            let col = if l > 50.0 { BAD } else if l < -50.0 { OK } else { MUTED };
            rows.push(("Lost here".into(), vec![v(format!("{} s", sgn(l)), col)]));
        }
        rows.push(("Cast time".into(), vec![v(secs(c.dur), INK), v(format!("  bench {}, not judged", secs(c.ref_dur)), MUTED)]));
        if let Some(w) = c.cd_wait.filter(|w| *w > 0.0) {
            rows.push(("Cooldown wait".into(), vec![v(secs(Some(w)), INK), v("  next skill not ready, not counted", MUTED)]));
        }
        if let Some(g) = c.gap {
            rows.push(("Wait after".into(), vec![v(secs(Some(g)), INK)]));
        }
        if let Some(e) = c.expect {
            let h = c.hits.unwrap_or(0);
            rows.push(("Hits".into(), vec![v(format!("{h} / {e}"), if h < e { BAD } else { INK })]));
        }
        if !c.stop.is_empty() {
            rows.push(("Ended by".into(), vec![v(c.stop.clone(), INK)]));
        }
        if let Some(t) = c.t {
            rows.push(("In pull".into(), vec![v(clock(t), INK)]));
        }
        if let Some(l) = c.lag {
            rows.push(("Behind bench".into(), vec![v(format!("{} s", sgn(l)), if l > 0.0 { BAD } else { OK })]));
        }
        if c.kind == "extra" {
            rows.push(("Bench".into(), vec![v("not in this section", INK)]));
        }
    }
    y += deflist(ui, dl, [p[0], y], &rows);
    y - p[1]
}

fn window_card(ui: &Ui, dl: &DrawListMut, p: [f32; 2], cd: &(Cd, Option<Cd>)) -> f32 {
    let (m, r) = (&cd.0, cd.1.clone().unwrap_or_default());
    let late = |a: Option<f64>| -> Vec<(String, C)> {
        match a {
            None => vec![v("\u{2014}", INK)],
            Some(a) => vec![v(format!("{} s", sgn(a)), if a > LATE_OK { BAD } else { OK })],
        }
    };
    let row = |k: &str, a: Option<f64>, b: Option<f64>, is_late: bool| -> (String, Vec<(String, C)>) {
        let mut vv = if is_late { late(a) } else { vec![v(secs(a), INK)] };
        if let Some(b) = b {
            vv.push(v(format!("  bench {}", if is_late { format!("{} s", sgn(b)) } else { secs(Some(b)) }), MUTED));
        }
        (k.to_string(), vv)
    };
    let rows = vec![
        row("Window until SE is ready", m.budget, r.budget, false),
        row("Filled with casts", m.used, r.used, false),
        row("Left empty", m.idle, r.idle, false),
        row("Closing SE after ready", m.se_late, r.se_late, true),
        row("Closing FoF after ready", m.fof_late, r.fof_late, true),
    ];
    let mut h = deflist(ui, dl, p, &rows) + 2.0;
    for line in [
        "SE and FoF come back 4.0 s after their cast ends. The fillers only",
        "matter if they push the closing SE past that; a close that starts",
        "early waits for FoF, which is not a loss.",
    ] {
        txt(dl, F::Ttl, [p[0], p[1] + h], MUTED, line);
        h += 16.0;
    }
    h + 8.0
}

fn sec_tip(ui: &Ui, dl: &DrawListMut, p: [f32; 2], r: &Report, i: usize, s: &Sec, icons: &HashMap<u32, String>) -> f32 {
    let x = &r.parts[i];
    let d = match (s.time, s.ref_time) {
        (Some(a), Some(b)) => Some(a - b),
        _ => None,
    };
    let head = format!("{} · {} ", where_(x), s.long);
    txt(dl, F::H3, p, INK, &head);
    if let Some(d) = d {
        txt(dl, F::H3, [p[0] + tw(ui, F::H3, &head), p[1]], cls(d), &format!("{} s", sgn(d)));
    }
    let mut y = p[1] + 20.0;
    let chips: Vec<&Chip> = s.chips.iter().collect();
    let entered = chips.iter().find(|c| c.ref_t.is_some() && c.t.is_some() && c.kind != "ghost").and_then(|c| c.lag);
    let mut sub = format!("{} vs bench {}", secs(s.time), secs(s.ref_time));
    if let Some(l) = entered {
        sub += &format!(" · entered {} s behind", sgn(l));
    }
    txt(dl, F::Ttl, [p[0], y], MUTED, &sub);
    y += 22.0;
    if (s.key == "fill" || s.key == "close") && x.cd.is_some() {
        y += window_card(ui, dl, [p[0], y], x.cd.as_ref().unwrap());
    }
    // aligned columns: the bench cast on top (the target), yours under it
    txt(dl, F::Tiny, [p[0], y + 6.0], MUTED, "Bench");
    txt(dl, F::Tiny, [p[0], y + 32.0], MUTED, "You");
    for (k, c) in chips.iter().enumerate() {
        let cx = p[0] + 40.0 + k as f32 * 28.0;
        if cx + 24.0 > p[0] + TIP_W - 24.0 {
            break;
        }
        if c.kind == "ghost" {
            icon(ui, dl, [cx, y], 24.0, c.id, &c.name, icons, false);
            dashed_rect(dl, [cx, y + 26.0], [cx + 24.0, y + 50.0], BAD, 1.5);
        } else {
            if let Some(rid) = c.ref_id {
                icon(ui, dl, [cx, y], 24.0, rid, &c.ref_name, icons, false);
            }
            icon(ui, dl, [cx, y + 26.0], 24.0, c.id, &c.name, icons, false);
            if c.kind == "extra" {
                dl.add_rect([cx - 1.5, y + 24.5], [cx + 25.5, y + 51.5], WARN).rounding(3.0).thickness(1.5).build();
            } else if c.has("slow") {
                dl.add_rect([cx - 2.0, y + 24.0], [cx + 26.0, y + 52.0], WARN).rounding(3.0).thickness(2.0).build();
            }
            if c.has("few") {
                dl.add_circle([cx + 23.0, y + 49.0], 3.5, BAD).filled(true).build();
            }
        }
        let (dd, col) = match c.kind.as_str() {
            "ghost" => ("miss".to_string(), BAD),
            "extra" => ("extra".to_string(), WARN),
            _ => match c.delta.filter(|d| d.abs() > 30.0) {
                Some(d) => (sgn(d), cls(d)),
                None => (String::new(), MUTED),
            },
        };
        if !dd.is_empty() {
            let w = tw(ui, F::Nano, &dd);
            txt(dl, F::Nano, [cx + 12.0 - w / 2.0, y + 52.0], col, &dd);
        }
    }
    y += 68.0;
    // time axis: both lanes start at their section's first cast, same scale
    let bc: Vec<(&Chip, f64, f64)> = chips
        .iter()
        .filter(|c| c.ref_t.is_some())
        .map(|c| (*c, c.ref_t.unwrap(), c.ref_dur.unwrap_or(0.0)))
        .collect();
    let yc: Vec<(&Chip, f64, f64)> = chips
        .iter()
        .filter(|c| c.kind != "ghost" && c.t.is_some())
        .map(|c| (*c, c.t.unwrap(), c.dur.unwrap_or(0.0)))
        .collect();
    let b0 = bc.iter().map(|z| z.1).fold(f64::INFINITY, f64::min);
    let y0 = yc.iter().map(|z| z.1).fold(f64::INFINITY, f64::min);
    let b_end = s.ref_time.unwrap_or_else(|| bc.iter().map(|z| z.1 - b0 + z.2).fold(0.0, f64::max));
    let y_end = s.time.unwrap_or_else(|| yc.iter().map(|z| z.1 - y0 + z.2).fold(0.0, f64::max));
    let span = b_end.max(y_end).max(500.0);
    let lane_w = TIP_W - 24.0 - 40.0;
    let px = |ms: f64| (ms / span) as f32 * lane_w;
    let lane = |ly: f32, name: &str, list: &[(&Chip, f64, f64)], t0: f64, mine: bool, end: Option<f64>| {
        let l0 = p[0] + 40.0;
        dl.add_rect([l0, ly], [l0 + lane_w, ly + 22.0], rgba(255, 255, 255, 0.03)).filled(true).rounding(3.0).build();
        txt(dl, F::Tiny, [p[0], ly + 4.0], MUTED, name);
        for (c, t, dur) in list {
            let a = [l0 + px(t - t0), ly + 1.0];
            let b = [a[0] + px(*dur).max(2.0), ly + 21.0];
            let missed = !mine && c.kind == "ghost";
            let (bg, edge) = if missed {
                (None, Some(BAD))
            } else if mine && c.kind == "extra" {
                (None, Some(WARN))
            } else if mine && c.has("slow") {
                (Some(rgb(138, 90, 18)), Some(WARN))
            } else if mine {
                (Some(rgb(63, 95, 134)), if c.has("few") { Some(BAD) } else { None })
            } else {
                (Some(rgb(75, 77, 82)), None)
            };
            if let Some(bg) = bg {
                dl.add_rect(a, b, bg).filled(true).rounding(3.0).build();
            }
            if let Some(e) = edge {
                dl.add_rect([a[0] + 0.75, a[1] + 0.75], [b[0] - 0.75, b[1] - 0.75], e).rounding(3.0).thickness(1.5).build();
            }
            if b[0] - a[0] > 18.0 {
                let (id, nm) = if mine || c.kind == "ghost" { (c.id, c.name.as_str()) } else { (c.ref_id.unwrap_or(c.id), c.ref_name.as_str()) };
                icon(ui, dl, [a[0] + 1.0, a[1] + 1.0], 18.0, id, nm, icons, false);
            }
        }
        if let Some(e) = end {
            let ex = l0 + px(e);
            dl.add_rect([ex, ly - 2.0], [ex + 2.0, ly + 24.0], INK2).filled(true).build();
        }
    };
    lane(y, "Bench", &bc, b0, false, s.ref_time.map(|_| b_end));
    lane(y + 24.0, "You", &yc, y0, true, s.time.map(|_| y_end));
    y += 50.0;
    let step = if span > 3000.0 { 1000.0 } else { 500.0 };
    let mut t = 0.0;
    while t <= span {
        let label = format!("{:.1}", t / 1000.0);
        let w = tw(ui, F::Nano, &label);
        txt(dl, F::Nano, [p[0] + 40.0 + px(t) - w / 2.0, y], MUTED, &label);
        t += step;
    }
    y += 14.0;
    y - p[1]
}

fn part_tip(ui: &Ui, dl: &DrawListMut, p: [f32; 2], r: &Report, x: &Part) -> f32 {
    txt(dl, F::H3, p, INK, &where_(x));
    let mut rows = vec![
        ("Visit".to_string(), vec![v(secs(x.len), INK), v(format!("  bench {}", secs(x.ref_len)), MUTED)]),
        ("Split".to_string(), vec![v(format!("{} s", sgn(x.delta)), cls(x.delta))]),
    ];
    let ag = match x.kind.as_str() {
        "sword" => r.sword.as_ref(),
        "bow" => r.bow.as_ref(),
        _ => None,
    };
    if let Some(a) = ag {
        rows.push((format!("Your {} visits", x.kind), vec![v(format!("median {:.2} s · {:.2}\u{2013}{:.2}", a.med, a.min, a.max), INK)]));
    }
    if x.ref_n != x.n && x.kind != "opener" {
        rows.push(("Compared with".into(), vec![v(format!("bench loop {}", x.ref_n), INK)]));
    }
    for s in &x.secs {
        let vv = match (s.time, s.ref_time) {
            (Some(a), Some(b)) => vec![v(format!("{} ", secs(Some(a))), INK), v(sgn(a - b), cls(a - b))],
            _ => vec![v("\u{2014}", INK)],
        };
        rows.push((s.long.clone(), vv));
    }
    let h = deflist(ui, dl, [p[0], p[1] + 24.0], &rows);
    24.0 + h
}

fn rot_tip(ui: &Ui, dl: &DrawListMut, p: [f32; 2], r: &Report, i: usize) -> f32 {
    let x = &r.parts[i];
    let head = "Behind bench ";
    txt(dl, F::H3, p, INK, head);
    txt(dl, F::H3, [p[0] + tw(ui, F::H3, head), p[1]], if x.lag_end > 0.0 { BAD } else { OK }, &format!("{} s", sgn(x.lag_end)));
    txt(dl, F::Ttl, [p[0], p[1] + 20.0], MUTED, &format!("after {} · this split {} s", where_(x), sgn(x.delta)));
    let (w, h, pad) = (356.0f32, 110.0f32, 6.0f32);
    let o = [p[0], p[1] + 42.0];
    let ys: Vec<f64> = r.parts.iter().map(|q| q.lag_end).collect();
    let lo = ys.iter().cloned().fold(0.0, f64::min);
    let hi = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let n = (r.parts.len().max(2) - 1) as f32;
    let xp = |j: usize| o[0] + pad + j as f32 * (w - 2.0 * pad) / n;
    let yp = |val: f64| o[1] + h - pad - ((val - lo) / if hi - lo == 0.0 { 1.0 } else { hi - lo }) as f32 * (h - 2.0 * pad);
    dl.add_line([o[0] + pad, yp(0.0)], [o[0] + w - pad, yp(0.0)], BORDER).build();
    for j in 1..ys.len() {
        dl.add_line([xp(j - 1), yp(ys[j - 1])], [xp(j), yp(ys[j])], MUTED).build();
    }
    for j in 1..ys.len() {
        let c = if r.parts[j].delta > 0.0 { BAD } else { OK };
        dl.add_line([xp(j), yp(ys[j - 1])], [xp(j), yp(ys[j])], c).thickness(3.0).build();
    }
    dl.add_circle([xp(i), yp(ys[i])], 4.5, INK).filled(true).build();
    let mut worst = 1.min(r.parts.len() - 1);
    for (j, q) in r.parts.iter().enumerate().skip(1) {
        if q.delta > r.parts[worst].delta {
            worst = j;
        }
    }
    let ly = o[1] + h + 4.0;
    let t = format!("Most lost: {} ", where_(&r.parts[worst]));
    txt(dl, F::Ttl, [p[0], ly], MUTED, &t);
    txt(dl, F::Ttl, [p[0] + tw(ui, F::Ttl, &t), ly], BAD, &format!("{} s", sgn(r.parts[worst].delta)));
    ly + 18.0 - p[1]
}

fn live_tip(dl: &DrawListMut, p: [f32; 2], lines: &[String]) -> f32 {
    txt(dl, F::H3, p, INK, "Live capture");
    let mut y = p[1] + 22.0;
    for l in lines {
        txt(dl, F::Ttl, [p[0], y], INK2, l);
        y += 18.0;
    }
    y - p[1]
}
