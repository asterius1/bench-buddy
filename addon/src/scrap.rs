//! The Power Scrapper view: `scrapper/grid.html` in game. The checker reduces everything to what is
//! drawn (`scrapper/panel.py`, `/api/scrapper/panel` and `/api/scrapper/sheet`): per cycle three
//! whirl cells (bomb burst, nade filler, other filler) and the cycle split, every icon's marks and
//! every card's text. This only lays them out. A button swaps to the bench cheat sheet.

use super::{dashed_rect, icon, line_h, rgb, rgba, rtxt, tempo, tw, txt, with_font, C, F};
use super::{ACCENT, BAD, BLACK, BORDER, HOVER, INK, INK2, MUTED, OK, SEC_BG, TIP, WARN};
use nexus::imgui::{Condition, DrawListMut, MouseButton, StyleColor, StyleVar, Ui, Window, WindowFlags};
use serde_json::Value;
use std::collections::HashMap;

/// What the mouse is over in the scrapper view.
#[derive(Clone)]
pub enum STip {
    /// a skill: its card rows and name
    Icon(Value),
    /// a whirl cell's card
    Cell(Value),
    /// a cycle's card
    Cyc(Value),
}

pub enum SAct {
    Sheet,
    All,
}

fn tone(t: i64) -> C {
    match t {
        1 => OK,
        2 => BAD,
        3 => WARN,
        4 => MUTED,
        5 => ACCENT,
        _ => INK,
    }
}

fn f(v: &Value) -> Option<f64> {
    v.as_f64()
}

fn sgn(ms: f64) -> String {
    format!("{}{:.2}", if ms > 0.0 { "+" } else if ms < 0.0 { "\u{2212}" } else { "\u{b1}" }, ms.abs() / 1000.0)
}

const KIT: [(u32, &str); 18] = [
    (5807, "grenade"), (5809, "grenade"), (5806, "grenade"), (5810, "grenade"),
    (5823, "bomb"), (5822, "bomb"), (5813, "bomb"), (76530, "bomb"),
    (30885, "mortar"), (30307, "mortar"), (56920, "gyro"), (30357, "gyro"),
    (6161, "mine"), (6164, "mine"), (30088, "hammer"), (30713, "hammer"), (30501, "hammer"), (29785, "hammer"),
];

fn kit_c(k: &str) -> C {
    match k {
        "grenade" => rgb(74, 194, 107),
        "bomb" => rgb(240, 138, 58),
        "mortar" => rgb(166, 123, 224),
        "gyro" => rgb(94, 162, 240),
        "mine" => rgb(232, 195, 74),
        _ => rgb(143, 148, 156),
    }
}

fn kit_of(id: u32) -> &'static str {
    KIT.iter().find(|k| k.0 == id).map_or("hammer", |k| k.1)
}

fn has(v: &Value, flag: &str) -> bool {
    v["f"].as_array().is_some_and(|a| a.iter().any(|x| x == flag))
}

/// One icon with the page's marks; returns whether the mouse is on it.
fn mark_icon(ui: &Ui, dl: &DrawListMut, p: [f32; 2], s: f32, w: f32, c: &Value, icons: &HashMap<u32, String>) -> bool {
    let id = c["id"].as_u64().unwrap_or(0) as u32;
    let name = c["name"].as_str().unwrap_or("");
    let gone = has(c, "ghost") || has(c, "movedfrom");
    let q = [p[0] + w, p[1] + s];
    if w > s + 0.5 {
        dl.add_rect(p, q, rgb(42, 43, 46)).filled(true).build();
        icon(ui, dl, [p[0] + 2.0, p[1] + s * 0.17], s * 0.66, id, name, icons, gone);
    } else {
        icon(ui, dl, p, s, id, name, icons, gone);
    }
    dl.add_rect([p[0], q[1]], [q[0], q[1] + 3.0], kit_c(kit_of(id))).filled(true).build();
    if has(c, "ghost") {
        dashed_rect(dl, [p[0], p[1]], q, BAD, 1.5);
    }
    if has(c, "movedfrom") {
        dashed_rect(dl, [p[0], p[1]], q, MUTED, 1.0);
    }
    if has(c, "few") {
        dl.add_rect([p[0] + 1.0, p[1] + 1.0], [q[0] - 1.0, q[1] - 1.0], BAD).thickness(2.0).build();
    }
    if has(c, "slow") {
        dl.add_rect([p[0], q[1]], [q[0], q[1] + 4.0], WARN).filled(true).build();
    }
    if has(c, "cdlate") {
        dl.add_circle([p[0] + 1.0, p[1] + 1.0], 4.5, BLACK).filled(true).build();
        dl.add_circle([p[0] + 1.0, p[1] + 1.0], 3.5, WARN).filled(true).build();
    }
    if has(c, "fervor") {
        dl.add_rect([p[0], p[1] - 3.0], [q[0], p[1] - 1.0], rgb(208, 71, 58)).filled(true).build();
    }
    if has(c, "inferred") {
        dashed_rect(dl, p, q, INK2, 1.0);
    }
    if has(c, "extra") {
        let (x, y) = (q[0] - 4.0, p[1] - 7.0);
        for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
            txt(dl, F::SemiS, [x + dx, y + dy], BLACK, "+");
        }
        txt(dl, F::SemiS, [x, y], INK, "+");
    }
    if has(c, "moved") {
        // a small blue corner: the bench casts it in another whirl
        dl.add_triangle([q[0] - 9.0, p[1]], [q[0], p[1]], [q[0], p[1] + 9.0], ACCENT).filled(true).build();
    }
    let hov = ui.is_mouse_hovering_rect(p, [q[0], q[1] + 3.0]);
    if hov {
        dl.add_rect([p[0] - 1.0, p[1] - 1.0], [q[0] + 1.0, q[1] + 1.0], INK).build();
    }
    hov
}

/// Width units of one token: a skill 1, Electro-whirl + Shrapnel 2, an auto chain by its length.
fn units(t: &Value) -> f32 {
    match t["t"].as_str() {
        Some("ew") => t["casts"].as_array().map_or(1, |a| a.len()) as f32,
        Some("aa") => (t["casts"].as_array().map_or(1, |a| a.len()) as f32 * 0.36).max(1.1),
        _ => 1.0,
    }
}

/// Draws the scrapper pull; returns the hovered card.
#[allow(clippy::too_many_arguments)]
pub fn draw(ui: &Ui, dl: &DrawListMut, p: [f32; 2], size: [f32; 2], v: &Value, sheet: Option<&Value>, show_sheet: bool, all: bool, icons: &HashMap<u32, String>, acts: &mut Vec<SAct>) -> Option<(STip, f32)> {
    let (x0, w) = (p[0], size[0]);
    let mut y = p[1];
    let mut tip = None;
    // time and the head figures
    txt(dl, F::Big, [x0, y - 4.0], INK, &super::clock(f(&v["secs"]).unwrap_or(0.0) * 1000.0));
    let bw = tw(ui, F::Big, &super::clock(f(&v["secs"]).unwrap_or(0.0) * 1000.0));
    txt(dl, F::Ttl, [x0 + bw + 8.0, y + 18.0], MUTED, &format!("bench {}", super::clock(f(&v["benchSecs"]).unwrap_or(0.0) * 1000.0)));
    let mut hx = x0 + w;
    if let Some(h) = v["head"].as_array() {
        for it in h.iter().rev() {
            let label = it[0].as_str().unwrap_or("");
            let val = it[1].as_str().unwrap_or("");
            let vw = tw(ui, F::SemiM, val).max(tw(ui, F::Small, label));
            hx -= vw;
            txt(dl, F::Small, [hx, y + 2.0], MUTED, label);
            txt(dl, F::SemiM, [hx, y + 19.0], tone(it[2].as_i64().unwrap_or(0)), val);
            hx -= 14.0;
        }
    }
    y += 46.0;
    // buttons: differences / all casts, and the cheat sheet
    let mut bx = x0;
    let button = |label: &str, on: bool, x: f32| -> (bool, f32) {
        let bw = tw(ui, F::Small, label) + 16.0;
        let (a, b) = ([x, y - 1.0], [x + bw, y + 17.0]);
        let hov = ui.is_mouse_hovering_rect(a, b);
        if hov {
            dl.add_rect(a, b, HOVER).filled(true).rounding(9.0).build();
        }
        dl.add_rect(a, b, if on { ACCENT } else { BORDER }).rounding(9.0).build();
        txt(dl, F::Small, [x + 8.0, y], if on { ACCENT } else { INK2 }, label);
        (hov && ui.is_mouse_clicked(MouseButton::Left), bw)
    };
    if !show_sheet {
        let (c1, w1) = button("differences", !all, bx);
        bx += w1 + 4.0;
        let (c2, _) = button("all casts", all, bx);
        if (c1 && all) || (c2 && !all) {
            acts.push(SAct::All);
        }
    } else {
        txt(dl, F::Small, [bx, y], MUTED, "Bench rotation: usual skills plain, floating ones outlined");
    }
    let label = if show_sheet { "your pull" } else { "cheat sheet" };
    let lw = tw(ui, F::Small, label) + 16.0;
    let (c3, _) = button(label, show_sheet, x0 + w - lw);
    if c3 {
        acts.push(SAct::Sheet);
    }
    y += 25.0;
    if show_sheet {
        return match sheet {
            Some(s) => draw_sheet(ui, dl, [x0, y], [w, p[1] + size[1] - y], s, icons),
            None => {
                txt(dl, F::Small, [x0, y], MUTED, "loading the bench rotation…");
                None
            }
        };
    }
    // columns: the cycle number, the three whirls, then the cycle's split and the running total
    // behind, as the Berserker panel's Split and Behind
    const LBL: f32 = 24.0;
    const SPL: f32 = 62.0;
    const BEH: f32 = 66.0;
    const GAPX: f32 = 3.0;
    let cw = (w - LBL - SPL - BEH - 5.0 * GAPX) / 3.0;
    let sx = x0 + LBL + GAPX + 3.0 * (cw + GAPX);
    for (k, h) in ["Bomb burst", "Nade filler", "Other filler"].iter().enumerate() {
        txt(dl, F::Small, [x0 + LBL + GAPX + k as f32 * (cw + GAPX), y], MUTED, h);
    }
    rtxt(ui, dl, F::Small, [sx + SPL, y], MUTED, "Split");
    rtxt(ui, dl, F::Small, [x0 + w, y], MUTED, "Behind");
    y += 18.0;
    let rows = v["rows"].as_array().cloned().unwrap_or_default();
    let n = rows.len().max(1) as f32;
    let foot = 20.0;
    // a row is as tall as its icons need; what is left of the window stays below the legend
    let h = ((p[1] + size[1] - foot - y - 3.0 * (n - 1.0)) / n).clamp(44.0, 68.0);
    for r in &rows {
        let b = [x0 + w, y + h];
        let cy = y + h / 2.0;
        let hov_row = ui.is_mouse_hovering_rect([x0, y], b);
        if hov_row {
            dl.add_rect([x0, y], [x0 + LBL, b[1]], HOVER).filled(true).rounding(4.0).build();
        }
        txt(dl, F::Bold, [x0 + 3.0, cy - line_h(F::Bold) / 2.0], INK, &r["n"].to_string());
        if ui.is_mouse_hovering_rect([x0, y], [x0 + LBL, b[1]]) {
            tip = Some((STip::Cyc(r["card"].clone()), cy));
        }
        // cells
        for k in 0..3 {
            let a = [x0 + LBL + GAPX + k as f32 * (cw + GAPX), y];
            let e = [a[0] + cw, b[1]];
            let c = &r["cells"][k];
            if c.is_null() {
                continue;
            }
            if let Some(t) = draw_cell(ui, dl, a, e, c, all, icons) {
                tip = Some((t, cy));
            }
        }
        // split: the cycle against the bench's, and its length
        let a = [sx, y];
        let e = [sx + SPL, b[1]];
        let d = f(&r["delta"]);
        dl.add_rect(a, e, tempo(d)).filled(true).rounding(4.0).build();
        let dt = d.map_or("\u{2026}".to_string(), sgn);
        rtxt(ui, dl, F::Bold, [e[0] - 4.0, cy - 19.0], INK, &dt);
        let len = f(&r["len"]).map_or("unfinished".to_string(), |l| format!("{:.2} s", l / 1000.0));
        rtxt(ui, dl, F::Tiny, [e[0] - 4.0, cy + 3.0], MUTED, &len);
        // behind: the sum of the splits so far, and where the cycle started
        let bh = f(&r["behind"]);
        let bc = match bh {
            Some(x) if x > 30.0 => BAD,
            Some(x) if x < -30.0 => OK,
            _ => INK,
        };
        rtxt(ui, dl, F::Bold, [b[0] - 4.0, cy - 19.0], bc, &bh.map_or(String::new(), sgn));
        rtxt(ui, dl, F::Tiny, [b[0] - 4.0, cy + 3.0], MUTED, &format!("at {}", r["start"].as_str().unwrap_or("")));
        if ui.is_mouse_hovering_rect(a, b) {
            tip = Some((STip::Cyc(r["card"].clone()), cy));
        }
        y += h + 3.0;
    }
    // footer legend
    let fy = y + 4.0;
    dl.add_line([x0, fy - 3.0], [x0 + w, fy - 3.0], BORDER).build();
    let mut fx = x0;
    for (c, label) in [(BAD, "missed (dashed) · missed hits (box)"), (WARN, "late"), (ACCENT, "other whirl"), (rgb(208, 71, 58), "Fervor")] {
        dl.add_rect([fx, fy + 3.0], [fx + 10.0, fy + 13.0], c).filled(true).rounding(2.0).build();
        fx += 14.0;
        txt(dl, F::Small, [fx, fy], MUTED, label);
        fx += tw(ui, F::Small, label) + 12.0;
    }
    tip
}

fn draw_cell(ui: &Ui, dl: &DrawListMut, a: [f32; 2], b: [f32; 2], c: &Value, all: bool, icons: &HashMap<u32, String>) -> Option<STip> {
    let hovered = ui.is_mouse_hovering_rect(a, b);
    let d = f(&c["delta"]);
    dl.add_rect(a, b, tempo(d)).filled(true).rounding(5.0).build();
    let (ia, ib) = ([a[0] + 1.0, a[1] + 1.0], [b[0] - 1.0, b[1] - 1.0]);
    match c["border"].as_str() {
        Some("err") => dl.add_rect(ia, ib, BAD).rounding(5.0).thickness(2.0).build(),
        Some("xtra") => dl.add_rect(ia, ib, WARN).rounding(5.0).thickness(2.0).build(),
        _ => {}
    }
    if hovered {
        dl.add_rect(a, b, ACCENT).rounding(5.0).build();
    }
    // top line
    let ty = a[1] + 3.0;
    txt(dl, F::Small, [a[0] + 6.0, ty + 2.0], INK2, c["label"].as_str().unwrap_or(""));
    let mut rx = b[0] - 6.0;
    if let Some(d) = d {
        rx -= rtxt(ui, dl, F::Small, [rx, ty + 2.0], INK2, &sgn(d)) + 4.0;
    }
    let tm = f(&c["time"]).map_or("\u{2026}".to_string(), |t| format!("{:.2}", t / 1000.0));
    rtxt(ui, dl, F::Bold, [rx, ty - 1.0], INK, &tm);
    // icons
    let top = ty + 23.0;
    let room_w = b[0] - a[0] - 12.0;
    let room_h = b[1] - top - 8.0;
    let mut hit = None;
    if all {
        let toks = c["all"].as_array().cloned().unwrap_or_default();
        let total: f32 = toks.iter().map(units).sum::<f32>().max(1.0);
        let s = (room_w / total).min(44.0).min(room_h).max(10.0).floor();
        let mut x = a[0] + 6.0;
        for t in &toks {
            let casts = t["casts"].as_array().cloned().unwrap_or_default();
            match t["t"].as_str() {
                Some("ew") => {
                    for cc in &casts {
                        if mark_icon(ui, dl, [x, top], s, s, cc, icons) {
                            hit = Some(STip::Icon(cc.clone()));
                        }
                        x += s;
                    }
                }
                Some("aa") => {
                    let ww = s * units(t);
                    let bad = casts.iter().find(|cc| cc["f"].as_array().is_some_and(|f| !f.is_empty() && f.iter().any(|x| x != "fervor"))).unwrap_or(&casts[0]);
                    let mut shown = bad.clone();
                    shown["id"] = 30501.into();
                    if mark_icon(ui, dl, [x, top], s, ww, &shown, icons) {
                        hit = Some(STip::Icon(aa_card(&casts)));
                    }
                    rtxt(ui, dl, F::SemiS, [x + ww - 3.0, top + s / 2.0 - 7.0], INK2, &casts.len().to_string());
                    x += ww;
                }
                _ => {
                    if let Some(cc) = casts.first() {
                        if mark_icon(ui, dl, [x, top], s, s, cc, icons) {
                            hit = Some(STip::Icon(cc.clone()));
                        }
                    }
                    x += s;
                }
            }
        }
    } else {
        let diff = c["diff"].as_array().cloned().unwrap_or_default();
        if diff.is_empty() {
            txt(dl, F::Small, [a[0] + 6.0, top + 4.0], rgba(195, 194, 183, 0.6), "same as bench");
        } else {
            let n = diff.len() as f32;
            let s = ((room_w - 4.0 * (n - 1.0)) / n).min(44.0).min(room_h).max(10.0).floor();
            for (k, cc) in diff.iter().enumerate() {
                let x = a[0] + 6.0 + k as f32 * (s + 4.0);
                if mark_icon(ui, dl, [x, top], s, s, cc, icons) {
                    hit = Some(STip::Icon(cc.clone()));
                }
            }
        }
    }
    match (hit, hovered) {
        (Some(t), _) => Some(t),
        (None, true) => Some(STip::Cell(c["card"].clone())),
        _ => None,
    }
}

/// A card for an auto block: one row per auto.
fn aa_card(casts: &[Value]) -> Value {
    let rows: Vec<Value> = casts
        .iter()
        .map(|c| {
            let at = c["card"].as_array().and_then(|r| r.iter().find(|x| x[0] == "At")).map_or(String::new(), |x| x[1].as_str().unwrap_or("").to_string());
            let hits = c["card"].as_array().and_then(|r| r.iter().find(|x| x[0] == "Hits")).map_or(String::new(), |x| format!("  hits {}", x[1].as_str().unwrap_or("")));
            let lost = c["card"].as_array().and_then(|r| r.iter().find(|x| x[0] == "Lost here")).map_or(String::new(), |x| format!("  lost {}", x[1].as_str().unwrap_or("")));
            let bad = c["f"].as_array().is_some_and(|f| f.iter().any(|x| x == "extra" || x == "few" || x == "slow"));
            serde_json::json!([c["name"], format!("{at}{hits}{lost}"), if bad { 3 } else { 0 }])
        })
        .collect();
    serde_json::json!({"id": 30501, "name": format!("Hammer autos · {} casts", casts.len()), "card": rows})
}

/// Width units of one cheat sheet token: a skill 1, Electro-whirl + Shrapnel 2, the filler slot 1.5,
/// an auto chain by its length.
fn sheet_units(t: &Value) -> f32 {
    match t["t"].as_str() {
        Some("ew") => 2.0,
        Some("slot") => 1.5,
        Some("aa") => (t["n"].as_f64().unwrap_or(1.0) as f32 * 0.36).max(1.1),
        _ => 1.0,
    }
}

/// One row of cheat sheet tokens at `[x, top]`, icons `sz` tall, then the usual skills left out;
/// sets `tip` to the hovered token's card.
#[allow(clippy::too_many_arguments)]
fn sheet_toks(ui: &Ui, dl: &DrawListMut, mut x: f32, top: f32, sz: f32, toks: &[Value], miss: &[Value], floats: bool, icons: &HashMap<u32, String>, cy: f32, tip: &mut Option<(STip, f32)>) {
    for t in toks {
        let ww = sz * sheet_units(t);
        let id = t["id"].as_u64().unwrap_or(0) as u32;
        let tp = [x, top];
        match t["t"].as_str() {
            Some("ew") => {
                icon(ui, dl, tp, sz, 30088, "EW", icons, false);
                icon(ui, dl, [x + sz, top], sz, 5807, "Shrap", icons, false);
            }
            Some("aa") => {
                dl.add_rect(tp, [x + ww, top + sz], rgb(42, 43, 46)).filled(true).build();
                icon(ui, dl, [x + 2.0, top + sz * 0.17], sz * 0.66, 30501, "AA", icons, false);
                rtxt(ui, dl, F::SemiS, [x + ww - 3.0, top + sz / 2.0 - 7.0], INK2, &t["n"].to_string());
            }
            Some("slot") => {
                dashed_rect(dl, [x + 2.0, top], [x + ww - 2.0, top + sz], MUTED, 1.5);
                let lw = tw(ui, F::Tiny, "filler");
                txt(dl, F::Tiny, [x + (ww - lw) / 2.0, top + sz / 2.0 - 7.0], INK2, "filler");
            }
            _ => icon(ui, dl, tp, sz, id, t["name"].as_str().unwrap_or(""), icons, false),
        }
        if t["t"] != "slot" {
            dl.add_rect([x, top + sz], [x + ww, top + sz + 3.0], kit_c(t["kit"].as_str().unwrap_or(""))).filled(true).build();
        }
        if floats && t["usual"] == false {
            dashed_rect(dl, tp, [x + ww, top + sz], ACCENT, 1.5);
        }
        // instants above
        if let Some(m) = t["marks"].as_array() {
            for (j, mid) in m.iter().enumerate() {
                let mid = mid.as_u64().unwrap_or(0) as u32;
                let ms = (sz * 0.38).max(8.0);
                let mp = [x + j as f32 * (ms + 1.0), top - ms - 2.0];
                icon(ui, dl, mp, ms, mid, "", icons, false);
                if mid == 76326 {
                    dl.add_rect(mp, [mp[0] + ms, mp[1] + ms], rgb(208, 71, 58)).thickness(1.5).build();
                }
            }
        }
        if ui.is_mouse_hovering_rect(tp, [x + ww, top + sz + 3.0]) {
            dl.add_rect([x - 1.0, top - 1.0], [x + ww + 1.0, top + sz + 1.0], INK).build();
            *tip = Some((STip::Icon(serde_json::json!({"id": id, "name": t["name"], "card": t["card"]})), cy));
        }
        x += ww;
    }
    for m in miss {
        let id = m["id"].as_u64().unwrap_or(0) as u32;
        x += 4.0;
        icon(ui, dl, [x, top], sz, id, m["name"].as_str().unwrap_or(""), icons, true);
        dashed_rect(dl, [x, top], [x + sz, top + sz], BAD, 1.5);
        x += sz;
    }
}

fn arr(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}

/// The cheat sheet: the structure on top (opener, then each mini loop's fixed casts with a slot for
/// the filler), then one row per cycle with just its filler. The bomb burst column only holds one
/// skill, so it is narrow.
fn draw_sheet(ui: &Ui, dl: &DrawListMut, p: [f32; 2], size: [f32; 2], s: &Value, icons: &HashMap<u32, String>) -> Option<(STip, f32)> {
    const LBL: f32 = 46.0;
    const GAPX: f32 = 3.0;
    let (x0, w) = (p[0], size[0]);
    let mut y = p[1];
    let mut tip = None;
    // the structure in two lines (opener and bomb burst, then the two fillers), boxes as wide as
    // their tokens
    let st = arr(&s["structure"]);
    for line in st.chunks(2) {
        let us: Vec<f32> = line.iter().map(|c| arr(&c["tokens"]).iter().map(sheet_units).sum::<f32>()).collect();
        let room = w - GAPX * (line.len() as f32 - 1.0) - 12.0 * line.len() as f32;
        let sz = (room / us.iter().sum::<f32>().max(1.0)).min(34.0).floor();
        let bh = 18.0 + 10.0 + sz + 8.0;
        let mut x = x0;
        for (c, u) in line.iter().zip(&us) {
            let bw = u * sz + 12.0;
            let (a, b) = ([x, y], [x + bw, y + bh]);
            dl.add_rect(a, b, rgb(26, 26, 25)).filled(true).rounding(5.0).build();
            dl.add_rect(a, b, BORDER).rounding(5.0).build();
            txt(dl, F::Small, [a[0] + 6.0, a[1] + 2.0], INK2, c["name"].as_str().unwrap_or(""));
            sheet_toks(ui, dl, a[0] + 6.0, a[1] + 28.0, sz, &arr(&c["tokens"]), &[], false, icons, y + bh / 2.0, &mut tip);
            x += bw + GAPX;
        }
        y += bh + GAPX;
    }
    y += 5.0;
    // the rows: just the filler
    let rows = arr(&s["rows"]);
    let n = rows.len().max(1) as f32;
    // rows only as tall as their icons, smaller if they would not fit
    let h = ((p[1] + size[1] - y - 18.0 - 3.0 * (n - 1.0)) / n).clamp(40.0, 58.0);
    let szmax = (h - 24.0).min(34.0);
    let n0 = rows.iter().map(|r| arr(&r["cells"][0]["tokens"]).len() + arr(&r["cells"][0]["missing"]).len()).max().unwrap_or(1).max(1) as f32;
    let c0 = (n0 * (szmax + 4.0) + 12.0).max(tw(ui, F::Small, "Bomb burst") + 10.0);
    let cw = (w - LBL - c0 - 3.0 * GAPX) / 2.0;
    let colx = [x0 + LBL + GAPX, x0 + LBL + GAPX + c0 + GAPX, x0 + LBL + GAPX + c0 + GAPX + cw + GAPX];
    let colw = [c0, cw, cw];
    for (k, hd) in ["Bomb burst", "Nade filler", "Other filler"].iter().enumerate() {
        txt(dl, F::Small, [colx[k], y], MUTED, hd);
    }
    y += 18.0;
    for r in &rows {
        let cy = y + h / 2.0;
        txt(dl, F::Bold, [x0 + 2.0, cy - 16.0], INK, &r["n"].to_string());
        txt(dl, F::Tiny, [x0 + 2.0, cy + 3.0], MUTED, r["start"].as_str().unwrap_or(""));
        for k in 0..3 {
            let c = &r["cells"][k];
            if c.is_null() {
                continue;
            }
            let a = [colx[k], y];
            let b = [a[0] + colw[k], y + h];
            dl.add_rect(a, b, rgb(26, 26, 25)).filled(true).rounding(5.0).build();
            dl.add_rect(a, b, BORDER).rounding(5.0).build();
            // the start time on the right, clear of the instants drawn above the icons; the bomb
            // burst always starts the cycle, so it only says "opener"
            let label = c["label"].as_str().unwrap_or("");
            if k > 0 || label == "opener" {
                rtxt(ui, dl, F::Tiny, [b[0] - 6.0, a[1] + 2.0], MUTED, label);
            }
            let toks = arr(&c["tokens"]);
            let miss = arr(&c["missing"]);
            let total: f32 = toks.iter().map(sheet_units).sum::<f32>() + miss.len() as f32;
            let top = a[1] + 18.0;
            let sz = ((colw[k] - 12.0 - 4.0 * miss.len() as f32) / total.max(1.0)).min(szmax).min(b[1] - top - 5.0).max(10.0).floor();
            sheet_toks(ui, dl, a[0] + 6.0, top, sz, &toks, &miss, true, icons, cy, &mut tip);
        }
        y += h + 3.0;
    }
    tip
}

// ---- hover cards ----------------------------------------------------------------------

const TIP_W: f32 = 500.0;

pub fn draw_tip(ui: &Ui, t: STip, y: f32, panel: [f32; 4], icons: &HashMap<u32, String>) {
    let colors = [ui.push_style_color(StyleColor::WindowBg, TIP), ui.push_style_color(StyleColor::Border, BORDER)];
    let vars = [
        ui.push_style_var(StyleVar::WindowRounding(8.0)),
        ui.push_style_var(StyleVar::WindowPadding([12.0, 10.0])),
        ui.push_style_var(StyleVar::WindowBorderSize(1.0)),
    ];
    let disp = ui.io().display_size;
    let right = panel[0] + panel[2] + 12.0;
    let x = if right + TIP_W < disp[0] { right } else { (panel[0] - 12.0 - TIP_W).max(0.0) };
    Window::new("##bench_buddy_scrap_tip")
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
                let h = match &t {
                    STip::Icon(v) => icon_card(ui, &dl, p0, v, icons),
                    STip::Cell(v) => cell_card(ui, &dl, p0, v, icons),
                    STip::Cyc(v) => cyc_card(ui, &dl, p0, v),
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

fn rows_list(ui: &Ui, dl: &DrawListMut, p: [f32; 2], rows: &[Value]) -> f32 {
    let kw = rows.iter().map(|r| tw(ui, F::Ttl, r[0].as_str().unwrap_or(""))).fold(0.0, f32::max) + 12.0;
    let mut y = p[1];
    for r in rows {
        txt(dl, F::Ttl, [p[0], y], MUTED, r[0].as_str().unwrap_or(""));
        txt(dl, F::Ttl, [p[0] + kw, y], tone(r[2].as_i64().unwrap_or(0)), r[1].as_str().unwrap_or(""));
        y += 20.0;
    }
    y - p[1]
}

fn icon_card(ui: &Ui, dl: &DrawListMut, p: [f32; 2], v: &Value, icons: &HashMap<u32, String>) -> f32 {
    let id = v["id"].as_u64().unwrap_or(0) as u32;
    let name = v["name"].as_str().unwrap_or("");
    icon(ui, dl, p, 28.0, id, name, icons, false);
    txt(dl, F::H3, [p[0] + 36.0, p[1] + 5.0], INK, name);
    let rows = v["card"].as_array().cloned().unwrap_or_default();
    36.0 + rows_list(ui, dl, [p[0], p[1] + 36.0], &rows)
}

fn cell_card(ui: &Ui, dl: &DrawListMut, p: [f32; 2], v: &Value, icons: &HashMap<u32, String>) -> f32 {
    let mut y = p[1];
    txt(dl, F::H3, [p[0], y], INK, v["title"].as_str().unwrap_or(""));
    y += line_h(F::H3);
    txt(dl, F::Small, [p[0], y], MUTED, v["sub"].as_str().unwrap_or(""));
    y += 20.0;
    if let Some(sp) = v["splits"].as_array().filter(|a| !a.is_empty()) {
        let cols = [p[0], p[0] + 210.0, p[0] + 290.0, p[0] + 370.0];
        for (k, hd) in ["Split", "you", "bench", ""].iter().enumerate() {
            txt(dl, F::Small, [cols[k], y], MUTED, hd);
        }
        y += 18.0;
        for r in sp {
            txt(dl, F::Ttl, [cols[0], y], INK, r[0].as_str().unwrap_or(""));
            txt(dl, F::Ttl, [cols[1], y], INK, r[1].as_str().unwrap_or(""));
            txt(dl, F::Ttl, [cols[2], y], MUTED, r[2].as_str().unwrap_or(""));
            txt(dl, F::Ttl, [cols[3], y], tone(r[4].as_i64().unwrap_or(0)), r[3].as_str().unwrap_or(""));
            y += 19.0;
        }
        y += 6.0;
    }
    // tiles: the bench above, you below, the difference and the name under them
    // one row of columns whatever the count: the tiles shrink instead of wrapping
    let tiles = v["tiles"].as_array().cloned().unwrap_or_default();
    let room = TIP_W - 24.0 - 40.0;
    let pitch = (room / tiles.len().max(1) as f32).min(42.0);
    let s = (pitch - 3.0).min(28.0).floor();
    txt(dl, F::Tiny, [p[0], y + s / 2.0 - 6.0], MUTED, "Bench");
    txt(dl, F::Tiny, [p[0], y + s + 2.0 + s / 2.0 - 6.0], MUTED, "You");
    for (k, t) in tiles.iter().enumerate() {
        let x = p[0] + 40.0 + k as f32 * pitch;
        if let Some(b) = t["b"].as_u64() {
            icon(ui, dl, [x, y], s, b as u32, "", icons, false);
        }
        let yy = y + s + 2.0;
        if let Some(m) = t["y"].as_u64() {
            icon(ui, dl, [x, yy], s, m as u32, t["name"].as_str().unwrap_or(""), icons, false);
            if t["kind"] == "extra" {
                dl.add_rect([x - 1.5, yy - 1.5], [x + s + 1.5, yy + s + 1.5], WARN).rounding(3.0).thickness(1.5).build();
            } else if t["slow"] == true {
                dl.add_rect([x - 2.0, yy - 2.0], [x + s + 2.0, yy + s + 2.0], WARN).rounding(3.0).thickness(2.0).build();
            }
            if t["few"] == true {
                dl.add_circle([x + s - 1.0, yy + s - 1.0], 3.5, BAD).filled(true).build();
            }
        } else {
            dashed_rect(dl, [x, yy], [x + s, yy + s], BAD, 1.5);
        }
        let lab = t["lab"].as_str().unwrap_or("");
        let lw = tw(ui, F::Nano, lab);
        txt(dl, F::Nano, [x + s / 2.0 - lw / 2.0, yy + s + 2.0], tone(t["tone"].as_i64().unwrap_or(0)), lab);
    }
    y += 2.0 * s + 2.0 + 18.0;
    y += lanes(ui, dl, [p[0], y], v, &tiles, icons);
    y - p[1]
}

/// The bench's casts and yours on one time scale, each from its own first cast, as the Berserker
/// section card: where the time went, not only how much.
fn lanes(ui: &Ui, dl: &DrawListMut, p: [f32; 2], v: &Value, tiles: &[Value], icons: &HashMap<u32, String>) -> f32 {
    let get = |t: &Value, a: &str, b: &str| Some((t[a].as_f64()?, t[b].as_f64().unwrap_or(0.0)));
    let bc: Vec<(&Value, f64, f64)> = tiles.iter().filter_map(|t| get(t, "bt", "bd").map(|(s, d)| (t, s, d))).collect();
    let yc: Vec<(&Value, f64, f64)> = tiles.iter().filter_map(|t| get(t, "yt", "yd").map(|(s, d)| (t, s, d))).collect();
    if bc.is_empty() && yc.is_empty() {
        return 0.0;
    }
    let b0 = bc.iter().map(|z| z.1).fold(f64::INFINITY, f64::min);
    let y0 = yc.iter().map(|z| z.1).fold(f64::INFINITY, f64::min);
    let b_end = f(&v["refLen"]).unwrap_or_else(|| bc.iter().map(|z| z.1 - b0 + z.2).fold(0.0, f64::max));
    let y_end = f(&v["len"]).unwrap_or_else(|| yc.iter().map(|z| z.1 - y0 + z.2).fold(0.0, f64::max));
    let span = b_end.max(y_end).max(500.0);
    let lane_w = TIP_W - 24.0 - 40.0;
    let px = |ms: f64| (ms / span) as f32 * lane_w;
    let lane = |ly: f32, name: &str, list: &[(&Value, f64, f64)], t0: f64, mine: bool, end: Option<f64>| {
        let l0 = p[0] + 40.0;
        dl.add_rect([l0, ly], [l0 + lane_w, ly + 22.0], rgba(255, 255, 255, 0.03)).filled(true).rounding(3.0).build();
        txt(dl, F::Tiny, [p[0], ly + 4.0], MUTED, name);
        for (t, at, dur) in list {
            let a = [l0 + px(at - t0), ly + 1.0];
            let b = [a[0] + px(*dur).max(2.0), ly + 21.0];
            let missed = !mine && t["y"].is_null();
            let (bg, edge) = if missed {
                (None, Some(BAD))
            } else if mine && t["kind"] == "extra" {
                (None, Some(WARN))
            } else if mine && t["slow"] == true {
                (Some(rgb(138, 90, 18)), Some(WARN))
            } else if mine {
                (Some(rgb(63, 95, 134)), if t["few"] == true { Some(BAD) } else { None })
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
                let id = if mine { t["y"].as_u64() } else { t["b"].as_u64() }.unwrap_or(0) as u32;
                icon(ui, dl, [a[0] + 1.0, a[1] + 1.0], 18.0, id, t["name"].as_str().unwrap_or(""), icons, false);
            }
        }
        if let Some(e) = end {
            let ex = l0 + px(e);
            dl.add_rect([ex, ly - 2.0], [ex + 2.0, ly + 24.0], INK2).filled(true).build();
        }
    };
    let y = p[1];
    lane(y, "Bench", &bc, b0, false, f(&v["refLen"]).map(|_| b_end));
    lane(y + 24.0, "You", &yc, y0, true, f(&v["len"]).map(|_| y_end));
    let ty = y + 50.0;
    let step = if span > 3000.0 { 1000.0 } else { 500.0 };
    let mut t = 0.0;
    while t <= span {
        let label = format!("{:.1}", t / 1000.0);
        let w = tw(ui, F::Nano, &label);
        txt(dl, F::Nano, [p[0] + 40.0 + px(t) - w / 2.0, ty], MUTED, &label);
        t += step;
    }
    ty + 14.0 - p[1]
}

fn cyc_card(ui: &Ui, dl: &DrawListMut, p: [f32; 2], v: &Value) -> f32 {
    let mut y = p[1];
    txt(dl, F::H3, [p[0], y], INK, v["title"].as_str().unwrap_or(""));
    y += line_h(F::H3);
    txt(dl, F::Small, [p[0], y], MUTED, v["sub"].as_str().unwrap_or(""));
    y += 20.0;
    let cols = [p[0], p[0] + 190.0, p[0] + 270.0, p[0] + 370.0];
    for (k, hd) in ["Cooldown skill", "cast", "after ready", "bench"].iter().enumerate() {
        txt(dl, F::Small, [cols[k], y], MUTED, hd);
    }
    y += 18.0;
    for r in v["rows"].as_array().cloned().unwrap_or_default() {
        for k in 0..4 {
            txt(dl, F::Ttl, [cols[k], y], if k == 3 { MUTED } else { INK }, r[k].as_str().unwrap_or(""));
        }
        y += 19.0;
    }
    y += 4.0;
    txt(dl, F::Small, [p[0], y], INK2, v["foot"].as_str().unwrap_or(""));
    let _ = ui;
    y + 16.0 - p[1]
}

#[allow(dead_code)]
const _UNUSED: C = SEC_BG;

#[cfg(test)]
mod tests {
    /// `BB_SCRAP=1 cargo test --release scrap -- --nocapture` reads the newest scrapper pull and the
    /// sheet from the running checker and walks every field the view draws.
    #[test]
    fn scrap_fields() {
        if std::env::var("BB_SCRAP").is_err() {
            return;
        }
        let get = |p: &str| -> serde_json::Value { serde_json::from_str(&crate::report::get(crate::CHECKER, p).unwrap()).unwrap() };
        let latest = get("/api/scrapper/latest")["latest"].as_str().unwrap().to_string();
        let v = get(&format!("/api/scrapper/panel?log={latest}"));
        let mut icons = 0;
        for r in v["rows"].as_array().unwrap() {
            assert!(r["card"]["rows"].is_array());
            for c in r["cells"].as_array().unwrap().iter().filter(|c| !c.is_null()) {
                assert!(c["card"]["tiles"].is_array() && c["label"].is_string());
                for t in c["all"].as_array().unwrap() {
                    let casts = t["casts"].as_array().unwrap();
                    assert!(!casts.is_empty());
                    for x in casts {
                        assert!(x["id"].as_u64().is_some() && x["card"].is_array() && x["f"].is_array());
                        icons += 1;
                    }
                }
            }
        }
        let s = get("/api/scrapper/sheet");
        let toks: usize = s["rows"].as_array().unwrap().iter().flat_map(|r| r["cells"].as_array().unwrap().iter().filter(|c| !c.is_null())).map(|c| c["tokens"].as_array().unwrap().len()).sum();
        let st = s["structure"].as_array().unwrap();
        assert!(st.len() == 4 && st.iter().all(|c| c["name"].is_string() && c["tokens"].as_array().is_some_and(|t| !t.is_empty())));
        println!("{latest}: {} rows, {icons} casts; sheet {toks} tokens", v["rows"].as_array().unwrap().len());
        assert!(icons > 0 && toks > 0);
    }
}
