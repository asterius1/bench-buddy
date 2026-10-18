//! The scrapper pull reduced to what the Nexus addon draws (`addon/src/scrap.rs`): per cycle
//! three whirl cells and the cycle split, every icon and card already decided. Texts are final.

use serde_json::{json, Value};

use super::{hits, EW, HAMMER, SHRAP, TWENTY};
use bb_core::kit::{clk, num, sec, sgn, tone};
use bb_core::omap::OMap;

const LATE_OK: f64 = 400.0;
const WHIRL: [&str; 3] = ["bomb burst", "nade filler", "other filler"];

fn f(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(Value::as_f64)
}
fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}
fn kind(c: &Value) -> &str {
    s(c, "kind")
}

/// A whirl's name; a cycle can have a fourth one (an extra Electro-whirl), which has no cell.
fn whirl(k: usize) -> String {
    WHIRL.get(k).map(|w| w.to_string()).unwrap_or_else(|| format!("whirl {}", k + 1))
}

fn capitalize(x: &str) -> String {
    let mut c = x.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase()).unwrap_or_default()
}

/// The chips of a mini loop's sections, in order.
fn chips(mini: &Value) -> Vec<&Value> {
    mini.get("sections").and_then(Value::as_object)
        .map(|s| s.values().flat_map(|x| x["chips"].as_array().into_iter().flatten()).collect())
        .unwrap_or_default()
}

/// Every chip of a row as (whirl index or None for the opener, section, chip index).
fn chip_paths(r: &Value) -> Vec<(Option<usize>, String, usize)> {
    let mut out = vec![];
    for (k, m) in r["minis"].as_array().into_iter().flatten().enumerate() {
        for (n, sct) in m.get("sections").and_then(Value::as_object).into_iter().flatten() {
            for i in 0..sct["chips"].as_array().map_or(0, |a| a.len()) {
                out.push((Some(k), n.clone(), i));
            }
        }
    }
    out
}

fn chip_at<'a>(r: &'a mut Value, p: &(Option<usize>, String, usize)) -> &'a mut Value {
    match p.0 {
        Some(k) => &mut r["minis"][k]["sections"][p.1.as_str()]["chips"][p.2],
        None => &mut r["pre"]["chips"][p.2],
    }
}

/// Moved fillers and the 20 s marks, as grid.html's pairMoves().
fn judge(x: &mut Value) {
    for r in x["rows"].as_array_mut().into_iter().flatten() {
        let mut paths = chip_paths(r);
        let pre_n = r.get("pre").and_then(|p| p["chips"].as_array()).map_or(0, |a| a.len());
        let whirls = paths.clone();
        paths.extend((0..pre_n).map(|i| (None, String::new(), i)));
        let lt: Vec<_> = paths.iter().filter(|p| {
            let c = chip_at(r, p);
            TWENTY.contains(&c["id"].as_i64().unwrap_or(0)) && f(c, "late").is_some()
        }).cloned().collect();
        let base = lt.iter().map(|p| f(chip_at(r, p), "late").unwrap()).fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.min(x)))).unwrap_or(0.0);
        let rbase = lt.iter().filter_map(|p| f(chip_at(r, p), "refLate")).fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.min(x)))).unwrap_or(0.0);
        for p in &lt {
            let c = chip_at(r, p);
            let late = f(c, "late").unwrap();
            let e = late - base - f(c, "refLate").map(|rl| rl - rbase).unwrap_or(0.0);
            c["excess"] = num(e);
        }
        let ghosts: Vec<_> = whirls.iter().filter(|p| kind(chip_at(r, p)) == "ghost").cloned().collect();
        let mut used = vec![false; ghosts.len()];
        for p in &whirls {
            let (k, id) = {
                let c = chip_at(r, p);
                if kind(c) != "extra" {
                    continue;
                }
                (p.0.unwrap(), c["id"].clone())
            };
            let g = ghosts.iter().enumerate().position(|(gi, gp)| !used[gi] && gp.0 != Some(k) && chip_at(r, gp)["id"] == id);
            if let Some(gi) = g {
                used[gi] = true;
                let gk = ghosts[gi].0.unwrap();
                let gc = chip_at(r, &ghosts[gi]);
                gc["kind"] = "movedfrom".into();
                gc["movedTo"] = k.into();
                let c = chip_at(r, p);
                c["kind"] = "moved".into();
                c["movedFrom"] = gk.into();
            }
        }
    }
}

fn in_fervor(c: &Value, fervor: &[(f64, f64)]) -> bool {
    f(c, "t").is_some_and(|t| fervor.iter().any(|&(a, b)| a <= t && t < b))
}

fn flags(c: &Value, fervor: &[(f64, f64)]) -> Vec<String> {
    let k = kind(c);
    let mut f_: Vec<String> = if ["ghost", "extra", "moved", "movedfrom"].contains(&k) { vec![k.into()] } else { vec![] };
    for x in c.get("flags").and_then(Value::as_array).into_iter().flatten() {
        if let Some(x) = x.as_str().filter(|x| *x == "slow" || *x == "few") {
            f_.push(x.into());
        }
    }
    if f(c, "excess").is_some_and(|e| e > LATE_OK) {
        f_.push("cdlate".into());
    }
    if s(c, "pre") == "hit" {
        f_.push("inferred".into());
    }
    if in_fervor(c, fervor) {
        f_.push("fervor".into());
    }
    f_
}

fn problem(f_: &[String]) -> bool {
    f_.iter().any(|x| ["ghost", "extra", "moved", "slow", "few", "cdlate"].contains(&x.as_str()))
}

fn wi(c: &Value, k: &str) -> usize {
    c[k].as_u64().unwrap_or(0) as usize
}

/// [label, value, tone] rows of a skill's card; tone 0 plain, 1 green, 2 red, 3 amber, 4 muted, 5 blue.
fn card(c: &Value, fervor: &[(f64, f64)]) -> Vec<Value> {
    let mut rows = vec![];
    let k = kind(c);
    match k {
        "ghost" => rows.push(json!(["", "the bench casts it here, you didn't", 2])),
        "movedfrom" => rows.push(json!(["", format!("you cast it in the {} instead", whirl(wi(c, "movedTo"))), 4])),
        "moved" => rows.push(json!(["", format!("the bench casts it in the {}", whirl(wi(c, "movedFrom"))), 5])),
        "extra" => rows.push(json!(["", "the bench has no such cast in this cycle", 3])),
        _ => {}
    }
    if k == "ghost" || k == "movedfrom" {
        rows.push(json!(["Bench cast time", sec(f(c, "refDur")), 0]));
        rows.push(json!(["Bench at", clk(f(c, "refT")), 0]));
        return rows;
    }
    let rf = c.get("ref").filter(|r| !r.is_null());
    rows.push(json!(["At", clk(f(c, "t")) + &rf.map(|r| format!("   bench {}", clk(f(r, "t")))).unwrap_or_default(), 0]));
    let pre = s(c, "pre");
    if !pre.is_empty() {
        let v = match pre {
            "end" => "under way at the fight start: placed from its end".to_string(),
            "order" => "mine placed before the fight: time from your opener order".to_string(),
            _ => format!("placed before the fight: from its first hit at {}", clk(f(c, "hit"))),
        };
        rows.push(json!(["", v, 4]));
    }
    rows.push(json!(["Cast time", sec(f(c, "dur")) + &f(c, "refDur").map(|d| format!("   bench {}", sec(Some(d)))).unwrap_or_default(), 0]));
    if let Some(l) = f(c, "landed") {
        let e = f(c, "expect").unwrap_or(0.0);
        rows.push(json!(["Hits", format!("{} of {}", l as i64, e as i64), if l < e { 2 } else { 0 }]));
    }
    let stop = s(c, "stop");
    if !stop.is_empty() && stop != "done" {
        rows.push(json!(["Ended by", stop, 0]));
    }
    if let (Some(slot), true) = (f(c, "slot"), pre.is_empty()) {
        rows.push(json!(["Start to next cast", sec(Some(slot)) + &f(c, "refSlot").map(|d| format!("   bench {}", sec(Some(d)))).unwrap_or_default(), 0]));
    }
    if let (Some(lost), true) = (f(c, "lost"), pre.is_empty()) {
        rows.push(json!(["Lost here", sgn(Some(lost)), tone(Some(lost))]));
    }
    if let Some(w) = f(c, "cdWait").filter(|&w| w != 0.0) {
        rows.push(json!(["Next skill on cooldown", format!("{} (not counted)", sec(Some(w))), 4]));
    }
    if let Some(late) = f(c, "late").filter(|&l| l >= 0.0) {
        let mut v = sgn(Some(late)) + &f(c, "refLate").map(|r| format!("   bench {}", sgn(Some(r)))).unwrap_or_default();
        let ex = f(c, "excess");
        if let Some(e) = ex.filter(|&e| e > 50.0) {
            v += &format!("   {} beyond the cycle's drift", sgn(Some(e)));
        }
        rows.push(json!(["After ready", v, if ex.unwrap_or(0.0) > LATE_OK { 3 } else { 0 }]));
    }
    if in_fervor(c, fervor) {
        rows.push(json!(["Bloodstone Fervor", "on (+7% strike)", 0]));
    }
    rows
}

fn icon(c: &Value, fervor: &[(f64, f64)]) -> Value {
    json!({"id": c["id"], "name": c["name"], "f": flags(c, fervor), "card": card(c, fervor)})
}

/// All casts of a cell compressed like grid.html: Electro-whirl + Shrapnel one token, autos one.
fn tokens(chips: &[&Value], fervor: &[(f64, f64)]) -> Vec<Value> {
    let mut out: Vec<Value> = vec![];
    for &c in chips {
        let k = kind(c);
        if k == "cut" {
            continue;
        }
        let real = k != "ghost" && k != "movedfrom";
        let id = c["id"].as_i64().unwrap_or(0);
        if let Some(last) = out.last_mut() {
            if real && id == SHRAP && last["t"] == "ew" && last["casts"].as_array().unwrap().len() == 1 {
                last["casts"].as_array_mut().unwrap().push(icon(c, fervor));
                continue;
            }
        }
        if real && HAMMER.contains(&id) {
            if let Some(last) = out.last_mut().filter(|l| l["t"] == "aa") {
                last["casts"].as_array_mut().unwrap().push(icon(c, fervor));
                continue;
            }
            out.push(json!({"t": "aa", "casts": [icon(c, fervor)]}));
            continue;
        }
        if real && id == EW {
            out.push(json!({"t": "ew", "casts": [icon(c, fervor)]}));
            continue;
        }
        out.push(json!({"t": "one", "casts": [icon(c, fervor)]}));
    }
    let gone = |t: &Value| t["casts"][0]["f"].as_array().unwrap().iter().any(|x| x == "ghost" || x == "movedfrom");
    let (g, rest): (Vec<Value>, Vec<Value>) = out.into_iter().partition(|t| gone(t));
    rest.into_iter().chain(g).collect()
}

fn tiles(chips: &[&Value]) -> Vec<Value> {
    let mut out = vec![];
    for &c in chips {
        let k = kind(c);
        if k == "cut" {
            continue;
        }
        let gone = k == "ghost" || k == "movedfrom";
        let rf = c.get("ref").filter(|r| !r.is_null());
        let b = if gone { c["id"].clone() } else { rf.map(|r| r["id"].clone()).unwrap_or(Value::Null) };
        let y = if gone { Value::Null } else { c["id"].clone() };
        let fl: Vec<&str> = c.get("flags").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
        let first = |key: &str| whirl(wi(c, key)).split_whitespace().next().unwrap_or("").to_string();
        let (lab, t) = if fl.contains(&"few") {
            (format!("{}/{}", c["landed"], c["expect"]), 2)
        } else if k == "ghost" {
            ("miss".into(), 2)
        } else if k == "movedfrom" {
            (format!("to {}", first("movedTo")), 4)
        } else if k == "moved" {
            (format!("from {}", first("movedFrom")), 5)
        } else if k == "extra" {
            ("extra".into(), 3)
        } else {
            (sgn(f(c, "lost")), tone(f(c, "lost")))
        };
        let (bt, bd) = if gone { (c.get("refT").cloned(), c.get("refDur").cloned()) }
                       else { (rf.and_then(|r| r.get("t")).cloned(), rf.and_then(|r| r.get("dur")).cloned()) };
        out.push(json!({"b": b, "y": y, "lab": lab, "tone": t, "name": c["name"],
                        "bt": bt, "bd": bd, "yt": if gone { Value::Null } else { c.get("t").cloned().unwrap_or(Value::Null) },
                        "yd": if gone { Value::Null } else { c.get("dur").cloned().unwrap_or(Value::Null) },
                        "slow": fl.contains(&"slow"), "few": fl.contains(&"few"), "kind": k}));
    }
    out
}

fn cell(r: &Value, k: usize, fervor: &[(f64, f64)]) -> Value {
    let minis = r["minis"].as_array().unwrap();
    let m = minis.get(k).filter(|m| !m.is_null());
    let has_pre = r.get("pre").is_some_and(|p| !p.is_null());
    let (chs, d, title, sub, time, splits): (Vec<&Value>, Option<f64>, String, String, Option<f64>, Vec<Value>);
    if m.is_none() && k == 0 && has_pre {
        chs = r["pre"]["chips"].as_array().unwrap().iter().collect();
        let first = minis.iter().find(|m| !m.is_null());
        d = first.and_then(|fm| Some(f(fm, "start")? - f(fm, "refStart")?));
        title = "Opener".into();
        sub = "From the fight start. Pre-fight casts: from their first hits (dotted), mines in your order.".into();
        time = first.and_then(|fm| f(fm, "start"));
        splits = vec![];
    } else if let Some(m) = m {
        chs = chips(m).into_iter().filter(|c| !m["len"].is_null() || kind(c) != "ghost").collect();
        d = f(m, "len").and_then(|l| Some(l - f(m, "refLen")?));
        time = f(m, "len");
        let rn = &r["refN"];
        title = format!("Cycle {} · {}", r["n"], capitalize(&whirl(k)))
            + &if !rn.is_null() && *rn != r["n"] { format!(" (bench cycle {rn})") } else { String::new() };
        sub = format!("{} · bench {} · {} · starts {}, bench {}", sec(f(m, "len")), sec(f(m, "refLen")), sgn(d),
                      clk(f(m, "start")), clk(f(m, "refStart")));
        splits = m["sections"].as_object().unwrap().iter().map(|(n, sct)| {
            let name = match n.as_str() { "core" => "Electro-whirl + Shrapnel", "fill" => "Filler", _ => "Bomb burst" };
            let dd = f(sct, "time").and_then(|t| Some(t - f(sct, "refTime")?));
            json!([name, sec(f(sct, "time")), sec(f(sct, "refTime")), dd.map(|x| sgn(Some(x))).unwrap_or_default(),
                   dd.map(|x| tone(Some(x))).unwrap_or(0)])
        }).collect();
    } else {
        return Value::Null;
    }
    let icons: Vec<Value> = chs.iter().filter(|c| kind(c) != "cut").map(|c| icon(c, fervor)).collect();
    let border = if chs.iter().any(|c| kind(c) == "ghost") { "err" } else if chs.iter().any(|c| kind(c) == "extra") { "xtra" } else { "" };
    let diff: Vec<Value> = icons.iter().filter(|i| {
        problem(&i["f"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect::<Vec<_>>())
    }).cloned().collect();
    json!({"label": m.map(|m| clk(f(m, "start"))).unwrap_or_else(|| "opener".into()), "time": time.map(num), "delta": d.map(num),
           "border": border, "diff": diff, "all": tokens(&chs, fervor),
           "card": {"title": title, "sub": sub, "splits": splits, "tiles": tiles(&chs),
                    "len": if m.is_some() { time.map(num) } else { None },
                    "refLen": m.and_then(|m| f(m, "refLen")).map(num)}})
}

fn cyc_card(r: &Value) -> Value {
    let dash = |v: Option<f64>| v.map(|x| sgn(Some(x))).unwrap_or_else(|| "—".into());
    let mut rows: Vec<Value> = r["cd"].as_array().unwrap().iter()
        .map(|c| json!([c["name"], clk(f(c, "t")), dash(f(c, "late")), dash(f(c, "refLate"))])).collect();
    rows.extend(r["cdMissing"].as_array().unwrap().iter().map(|x| json!([x["name"], "bench casts it, you didn't", "", ""])));
    let (fm, fr, b, rb) = (&r["fervor"]["mine"], &r["fervor"]["ref"], &r["buffs"], r.get("refBuffs"));
    let n = |v: &Value| v.as_array().map_or(0, |a| a.len());
    let foot = format!("Fervor covers {} of {} 20 s skills", n(&fm["inside"]), fm["of"])
        + &if fr.is_null() { String::new() } else { format!(" (bench {} of {})", n(&fr["inside"]), fr["of"]) }
        + &format!(" · Superspeed {}% (bench {}%)", b["superspeed"],
                   rb.and_then(|x| x.get("superspeed")).map(|v| v.to_string()).unwrap_or_else(|| "—".into()));
    json!({"title": format!("Cycle {}", r["n"]),
           "sub": format!("{} · bench {}. A 20 s skill is ready 16 s after its cast ends with alacrity.", sec(f(r, "len")), sec(f(r, "refLen"))),
           "rows": rows, "foot": foot})
}

/// Hits per skill in this cycle, yours (as landed) against the bench's (as full casts).
fn hits_of(r: &Value) -> OMap<i64, (String, i64, i64)> {
    let mut cs: Vec<&Value> = r["minis"].as_array().unwrap().iter().filter(|m| !m.is_null())
        .flat_map(|m| chips(m).into_iter().filter(move |c| !m["len"].is_null() || kind(c) != "ghost")).collect();
    if let Some(p) = r.get("pre").filter(|p| !p.is_null()) {
        cs.extend(p["chips"].as_array().unwrap().iter());
    }
    let mut out: OMap<i64, (String, i64, i64)> = OMap::default();
    for c in cs {
        let id = c["id"].as_i64().unwrap_or(0);
        let Some(n) = hits(id) else { continue };
        if kind(c) == "cut" {
            continue;
        }
        let row = out.entry_or(id, (c["name"].as_str().unwrap_or("").into(), 0, 0));
        if kind(c) == "ghost" || kind(c) == "movedfrom" {
            row.2 += n;
            continue;
        }
        row.1 += c.get("landed").and_then(Value::as_i64).unwrap_or(n);
        if c.get("ref").is_some_and(|r| !r.is_null()) {
            row.2 += n;
        }
    }
    out
}

fn hits_view(per: &OMap<i64, (String, i64, i64)>, title: &str, sub: &str) -> Value {
    let mut diff: Vec<(i64, i64, &str, i64, i64)> =
        per.iter().filter(|(_, (_, y, b))| y != b).map(|(&i, (nm, y, b))| (y - b, i, nm.as_str(), *y, *b)).collect();
    diff.sort_by(|a, b| (a.0, a.2).cmp(&(b.0, b.2)));
    let net: i64 = diff.iter().map(|d| d.0).sum();
    json!({"net": net, "tone": if net < 0 { 2 } else if net > 0 { 1 } else { 0 },
           "icons": diff.iter().filter(|d| d.0 < 0).take(3).map(|d| json!({"id": d.1, "name": d.2, "d": d.0})).collect::<Vec<_>>(),
           "card": {"title": title, "sub": sub,
                    "rows": diff.iter().map(|d| json!([d.2, d.3.to_string(), d.4.to_string(), format!("{:+}", d.0), if d.0 < 0 { 2 } else { 1 }])).collect::<Vec<_>>()}})
}

/// The panel for one pull, from its compare() result (changed in place by the judging).
pub fn panel(mut x: Value) -> Value {
    judge(&mut x);
    let fervor: Vec<(f64, f64)> = x["fervorIv"].as_array().unwrap().iter()
        .map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())).collect();
    let mut rows = vec![];
    let mut behind = 0.0;
    let mut total: OMap<i64, (String, i64, i64)> = OMap::default();
    let sub = "Yours as landed; the bench's as full casts (its log has only totals).";
    for r in x["rows"].as_array().unwrap() {
        let per = hits_of(r);
        for (&i, (nm, y, b)) in per.iter() {
            let t = total.entry_or(i, (nm.clone(), 0, 0));
            t.1 += y;
            t.2 += b;
        }
        let d = f(r, "len").and_then(|l| Some(l - f(r, "refLen")?));
        if let Some(d) = d {
            behind += d;
        }
        let late = f(r, "late20");
        rows.push(json!({
            "n": r["n"], "start": clk(f(r, "start")),
            "late": late.map(|l| format!("+{:.1}", l / 1000.0)).unwrap_or_default(),
            "lateTone": late.and_then(|l| Some(tone(Some(l - f(r, "refLate20")?)))).unwrap_or(0),
            "len": r["len"], "delta": d.map(num), "behind": d.map(|_| num(behind)),
            "cells": (0..3).map(|k| cell(r, k, &fervor)).collect::<Vec<_>>(), "card": cyc_card(r),
            "hits": hits_view(&per, &format!("Hits in cycle {}", r["n"]), sub),
            "hitsTotal": hits_view(&total, &format!("Hits up to the end of cycle {}", r["n"]), sub)}));
    }
    let (bb, rb) = (&x["buffs"], &x["refBuffs"]);
    let last = x["lag"].as_array().unwrap().last().and_then(Value::as_f64);
    let (dps, bdps) = (f(&x, "dps").unwrap_or(0.0), f(&x, "benchDps").unwrap_or(1.0));
    let bonus = |b: &Value| b["motion"]["bonus"].as_f64().unwrap_or(0.0);
    json!({"kind": "scrapper", "log": x["log"], "secs": x["secs"], "benchSecs": x["benchSecs"],
           "head": [["behind", sgn(last), tone(last)],
                    ["DPS", format!("{:.1}k {:+.0}%", dps / 1000.0, dps / bdps * 100.0 - 100.0), if dps < bdps { 2 } else { 1 }],
                    ["Fervor", format!("{}%  {}", bb["fervor"], rb["fervor"]), 0],
                    ["SS", format!("{}%  {}", bb["superspeed"], rb["superspeed"]), 0],
                    ["Motion", format!("+{:.1}%  {:.1}", bonus(bb), bonus(rb)), 0]],
           "rows": rows})
}
