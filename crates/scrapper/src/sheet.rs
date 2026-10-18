//! The bench rotation as a cheat sheet (`bench_sheet()` in scrap.py and `sheet()` in panel.py).

use std::collections::{BTreeSet, HashMap};

use serde_json::{json, Value};

use super::{cd, kit_of, Bench, Cast, EW, HAMMER, SHRAP};
use bb_core::kit::{clk, num, sec};

/// a skill in at least this share of full cycles' same mini loop is part of the usual loop
const REGULAR: f64 = 0.75;

pub fn shown_instant(id: i64) -> Option<&'static str> {
    Some(match id {
        6162 | -37 => "Detonate",
        6166 => "Detonate Mine Field",
        76326 => "Bloodstone Fervor",
        _ => return None,
    })
}

#[derive(Clone)]
struct Mark {
    id: i64,
    name: &'static str,
    t: f64,
    kit: &'static str,
}

#[derive(Clone)]
struct Token {
    kind: &'static str,
    id: i64,
    name: String,
    t: f64,
    end: f64,
    dur: Option<f64>,
    shrap: Option<f64>,
    n: Option<i64>,
    ids: Vec<i64>,
    late: Option<f64>,
    cd: Option<f64>,
    kit: &'static str,
    marks: Vec<Mark>,
    usual: Option<bool>,
}

/// One mini loop as display tokens: Electro-whirl + Shrapnel as one, hammer auto chains as one.
/// Instants are marks on the token they happen during, or the next one.
fn tokens(casts: &[&Cast], instants: &[(i64, f64)]) -> Vec<Token> {
    let mut real: Vec<&Cast> = casts.iter().copied().filter(|c| !c.cut).collect();
    real.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
    let mut out: Vec<Token> = vec![];
    let tok = |kind, id, name: String, t, end| Token { kind, id, name, t, end, dur: None, shrap: None, n: None, ids: vec![], late: None,
                                                    cd: None, kit: "", marks: vec![], usual: None };
    for c in real {
        if let Some(last) = out.last_mut() {
            if c.id == SHRAP && last.kind == "ew" && last.shrap.is_none() {
                last.shrap = Some(c.t);
                last.end = c.t + c.dur;
                continue;
            }
        }
        if c.id == EW {
            out.push(tok("ew", EW, "Electro-whirl + Shrapnel".into(), c.t, c.t + c.dur));
        } else if HAMMER.contains(&c.id) {
            match out.last_mut().filter(|l| l.kind == "aa") {
                Some(last) => {
                    *last.n.as_mut().unwrap() += 1;
                    last.end = c.t + c.dur;
                    last.ids.push(c.id);
                }
                None => {
                    let mut t = tok("aa", 30501, "Hammer auto chain".into(), c.t, c.t + c.dur);
                    t.n = Some(1);
                    t.ids = vec![c.id];
                    out.push(t);
                }
            }
        } else {
            let mut t = tok("skill", c.id, c.name.clone(), c.t, c.t + c.dur);
            t.dur = Some(c.dur);
            t.late = c.ready.map(|r| c.t - r);
            t.cd = cd(c.id);
            out.push(t);
        }
    }
    for x in out.iter_mut() {
        x.kit = kit_of(x.id).unwrap_or("");
    }
    let mut inst = instants.to_vec();
    inst.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    for (id, t) in inst {
        let mark = Mark { id, name: shown_instant(id).unwrap(), t, kit: kit_of(id).unwrap_or("") };
        let host = out.iter().position(|x| x.t <= t && t < x.end)
            .or_else(|| out.iter().position(|x| x.t >= t))
            .or_else(|| out.len().checked_sub(1));
        if let Some(h) = host {
            out[h].marks.push(mark);
        }
    }
    out
}

struct SMini {
    start: f64,
    len: Option<f64>,
    burst: bool,
    tokens: Vec<Token>,
    missing: Vec<i64>,
    /// indices of the filler tokens
    filler: Vec<usize>,
}

struct SRow {
    n: usize,
    start: f64,
    len: Option<f64>,
    minis: Vec<Option<SMini>>,
    pre: Vec<Token>,
}

fn skill_name(sm: &Value, x: i64) -> String {
    sm[format!("s{x}")]["name"].as_str().map(String::from)
        .unwrap_or_else(|| shown_instant(x).map(String::from).unwrap_or_else(|| x.to_string()))
}

fn token_ids(ts: &[Token]) -> BTreeSet<i64> {
    ts.iter().map(|t| t.id).chain(ts.iter().flat_map(|t| t.marks.iter().map(|k| k.id))).collect()
}

fn sheet_card(t: &Token) -> Vec<Value> {
    if t.kind == "slot" {
        return vec![json!(["", "the floating skills of each cycle go here (see the rows below)", 0])];
    }
    let mut rows = vec![json!(["At", clk(Some(t.t)), 0])];
    if t.kind == "aa" {
        rows = vec![json!(["Auto casts", t.n.unwrap_or(1).to_string(), 0]), json!(["From", clk(Some(t.t)), 0]),
                    json!(["Length", sec(Some(t.end - t.t)), 0])];
    } else if t.kind == "ew" {
        rows = vec![json!(["Electro-whirl", clk(Some(t.t)), 0]), json!(["Shrapnel", clk(t.shrap), 0])];
    } else {
        if let Some(d) = t.dur.filter(|&d| d != 0.0) {
            rows.push(json!(["Cast time", sec(Some(d)), 0]));
        }
        if let Some(cd) = t.cd {
            rows.push(json!(["Recharge", format!("{} s, {:.1} s with alacrity", cd as i64, cd / 1.25), 0]));
        }
        if let Some(l) = t.late {
            rows.push(json!(["Cast after ready", sec(Some(l)), 0]));
        }
        if t.usual == Some(false) {
            rows.push(json!(["", "floating: not in this whirl every cycle", 5]));
        }
    }
    rows.extend(t.marks.iter().map(|k| json!([k.name, clk(Some(k.t)), 4])));
    rows
}

fn tok_json(t: &Token) -> Value {
    json!({"t": t.kind, "id": t.id, "name": t.name, "n": t.n.unwrap_or(1), "kit": t.kit, "usual": t.usual.unwrap_or(true),
           "marks": t.marks.iter().map(|k| k.id).collect::<Vec<_>>(), "card": sheet_card(t)})
}

struct Built {
    rows: Vec<SRow>,
    usual: Vec<Vec<i64>>,
    structure: Vec<(&'static str, Vec<Token>)>,
    full: usize,
}

/// The bench's cycles as rows of three mini loops of tokens, each skill marked usual or not against
/// the same mini loop in the other full cycles (`bench_sheet()` in scrap.py).
fn build(bench: &Bench) -> Built {
    let log = &bench.log;
    let inst: Vec<(i64, f64)> = log["players"][0]["rotation"].as_array().into_iter().flatten()
        .filter(|r| r["id"].as_i64().and_then(shown_instant).is_some())
        .flat_map(|r| {
            let id = r["id"].as_i64().unwrap();
            r["skills"].as_array().into_iter().flatten().map(move |s| (id, s["castTime"].as_f64().unwrap_or(0.0)))
        })
        .collect();
    let rows = &bench.rows;
    let t0 = bench.side.t0;
    let full: Vec<usize> = (0..rows.len()).filter(|&k| rows[k].len.is_some()).collect();
    let mut out: Vec<SRow> = vec![];
    for (k, cy) in rows.iter().enumerate() {
        let minis = cy.minis.iter().map(|m| m.as_ref().map(|m| {
            let end = m.start + m.len.filter(|&l| l != 0.0).unwrap_or(1e9);
            let cs: Vec<&Cast> = m.sections.iter().flat_map(|(_, v)| v.iter()).collect();
            let ins: Vec<(i64, f64)> = inst.iter().copied().filter(|&(_, t)| m.start <= t && t < end).collect();
            SMini { start: m.start - cy.start, len: m.len, burst: m.burst, tokens: tokens(&cs, &ins), missing: vec![], filler: vec![] }
        })).collect();
        let pre = if cy.pre.is_empty() { vec![] } else {
            let ins: Vec<(i64, f64)> = inst.iter().copied().filter(|&(_, t)| t < cy.start).collect();
            tokens(&cy.pre.iter().collect::<Vec<_>>(), &ins)
        };
        out.push(SRow { n: k, start: cy.start - t0, len: cy.len, minis, pre });
    }
    // usual skills per mini loop position, from the full cycles
    let mut usual: Vec<BTreeSet<i64>> = vec![];
    for i in 0..3 {
        let mut cnt: HashMap<i64, usize> = HashMap::new();
        for &r in &full {
            if let Some(Some(m)) = out[r].minis.get(i) {
                let ids: BTreeSet<i64> = m.tokens.iter().filter(|t| t.kind == "skill").map(|t| t.id)
                    .chain(m.tokens.iter().flat_map(|t| t.marks.iter().map(|k| k.id))).collect();
                for x in ids {
                    *cnt.entry(x).or_default() += 1;
                }
            }
        }
        usual.push(cnt.into_iter().filter(|&(_, n)| n as f64 >= REGULAR * full.len() as f64).map(|(x, _)| x).collect());
    }
    let none = BTreeSet::new();
    for r in out.iter_mut() {
        let is_full = r.len.is_some();
        for (i, m) in r.minis.iter_mut().enumerate() {
            let Some(m) = m else { continue };
            let u = usual.get(i).unwrap_or(&none);
            let have = token_ids(&m.tokens);
            for t in m.tokens.iter_mut() {
                t.usual = Some(t.kind == "ew" || t.kind == "aa" || u.contains(&t.id));
            }
            m.missing = if is_full { u.iter().copied().filter(|x| !have.contains(x) && shown_instant(*x).is_none()).collect() } else { vec![] };
        }
    }
    // the times are already from the bench's start (t0 is 0 on both sides)
    // each mini loop's filler: bomb burst only its floating skills; the others from the end of the
    // leading run of usual casts
    for r in out.iter_mut() {
        for (i, m) in r.minis.iter_mut().enumerate() {
            let Some(m) = m else { continue };
            let ts = &m.tokens;
            m.filler = if i == 0 {
                (0..ts.len()).filter(|&j| ts[j].kind == "skill" && ts[j].usual == Some(false)).collect()
            } else {
                let mut j = 0;
                while j < ts.len() && (ts[j].kind == "ew" || (ts[j].kind == "skill" && ts[j].usual == Some(true))) {
                    j += 1;
                }
                (j..ts.len()).collect()
            };
        }
    }
    let mut order: HashMap<(usize, i64), Vec<usize>> = HashMap::new();
    for &r in &full {
        for (i, m) in out[r].minis.iter().enumerate() {
            for (j, t) in m.as_ref().map(|m| m.tokens.as_slice()).unwrap_or(&[]).iter().enumerate() {
                if usual.get(i).is_some_and(|u| u.contains(&t.id)) {
                    order.entry((i, t.id)).or_default().push(j);
                }
            }
        }
    }
    let avg = |i: usize, x: i64| order.get(&(i, x)).map(|v| v.iter().sum::<usize>() as f64 / v.len() as f64).unwrap_or(99.0);
    // ties in position keep the id order; Python keeps its set order, the same on the bench (judgeparity.py)
    let usual_list: Vec<Vec<i64>> = (0..3).map(|i| {
        let mut v: Vec<i64> = usual[i].iter().copied().collect();
        v.sort_by(|a, b| avg(i, *a).partial_cmp(&avg(i, *b)).unwrap());
        v
    }).collect();
    // the structure: the opener, then each mini loop's fixed casts from the first full cycle that has
    // it, with a "filler" slot where its floating skills go
    let slot = Token { kind: "slot", id: 0, name: "filler".into(), t: 0.0, end: 0.0, dur: None, shrap: None, n: None,
                       ids: vec![], late: None, cd: None, kit: "", marks: vec![], usual: Some(true) };
    let mut structure = vec![("Opener", out.first().map(|r| r.pre.clone()).unwrap_or_default())];
    for (i, name) in ["Bomb burst", "Nade filler", "Other filler"].iter().enumerate() {
        let m = full.iter().find_map(|&r| out[r].minis.get(i).and_then(|m| m.as_ref()));
        let mut fixed: Vec<&Token> = vec![];
        let mut at = None;
        if let Some(m) = m {
            for (j, t) in m.tokens.iter().enumerate() {
                if m.filler.contains(&j) {
                    at = at.or(Some(fixed.len()));
                } else if t.kind != "aa" {
                    fixed.push(t);
                }
            }
        }
        let at = at.unwrap_or(fixed.len());
        let mut toks: Vec<Token> = fixed[..at].iter().map(|t| (*t).clone()).collect();
        toks.push(slot.clone());
        toks.extend(fixed[at..].iter().map(|t| (*t).clone()));
        structure.push((*name, toks));
    }
    Built { rows: out, usual: usual_list, structure, full: full.len() }
}

/// The bench rotation for the addon's cheat sheet view (panel.py's `sheet()`): the structure on
/// top, then rows of three cells of just the filler.
pub fn sheet(bench: &Bench) -> Value {
    let sm = &bench.log["skillMap"];
    let Built { rows: out, usual: usual_list, structure, .. } = build(bench);
    let structure: Vec<Value> = structure.iter()
        .map(|(n, ts)| json!({"name": n, "tokens": ts.iter().map(tok_json).collect::<Vec<_>>()})).collect();
    let rows_json: Vec<Value> = out.iter().map(|r| {
        let cells: Vec<Value> = (0..3).map(|k| match r.minis.get(k).and_then(|m| m.as_ref()) {
            None if k == 0 && !r.pre.is_empty() => json!({"label": "opener", "len": null, "tokens": [], "missing": []}),
            None => Value::Null,
            Some(m) => json!({"label": format!("+{:.1} s", m.start / 1000.0), "len": m.len.map(num),
                              "tokens": m.filler.iter().map(|&j| tok_json(&m.tokens[j])).collect::<Vec<_>>(),
                              "missing": m.missing.iter().map(|&x| json!({"id": x, "name": skill_name(sm, x), "kit": kit_of(x).unwrap_or("")})).collect::<Vec<_>>()}),
        }).collect();
        json!({"n": r.n, "start": clk(Some(r.start)), "len": r.len.map(num), "cells": cells})
    }).collect();
    json!({"kind": "sheet", "rows": rows_json, "structure": structure,
           "usual": usual_list.iter().map(|col| col.iter().copied().filter(|&u| shown_instant(u).is_none()).collect::<Vec<_>>()).collect::<Vec<_>>()})
}

fn raw_token(t: &Token) -> Value {
    let mut o = serde_json::Map::new();
    o.insert("kind".into(), t.kind.into());
    o.insert("id".into(), t.id.into());
    o.insert("name".into(), t.name.clone().into());
    if t.kind != "slot" {
        o.insert("t".into(), num(t.t));
        if t.kind == "skill" {
            o.insert("dur".into(), t.dur.map(num).into());
        }
        o.insert("end".into(), num(t.end));
        match t.kind {
            "ew" => {
                if let Some(s) = t.shrap {
                    o.insert("shrap".into(), num(s));
                }
            }
            "aa" => {
                o.insert("n".into(), t.n.into());
                o.insert("ids".into(), json!(t.ids));
            }
            _ => {
                o.insert("late".into(), t.late.map(num).into());
                o.insert("cd".into(), t.cd.map(num).into());
            }
        }
    }
    o.insert("kit".into(), t.kit.into());
    o.insert("marks".into(), t.marks.iter().map(|k| json!({"id": k.id, "name": k.name, "t": num(k.t), "kit": k.kit})).collect());
    if let Some(u) = t.usual {
        o.insert("usual".into(), u.into());
    }
    Value::Object(o)
}

/// The bench's cycles for the cheat sheet page (`/api/scrapper/bench`).
pub fn bench_sheet(bench: &Bench) -> Value {
    let sm = &bench.log["skillMap"];
    let b = build(bench);
    let item = |x: i64| json!({"id": x, "name": skill_name(sm, x), "kit": kit_of(x).unwrap_or("")});
    let rows: Vec<Value> = b.rows.iter().map(|r| json!({
        "n": r.n, "start": num(r.start), "len": r.len.map(num),
        "minis": r.minis.iter().map(|m| m.as_ref().map(|m| json!({
            "start": num(m.start), "len": m.len.map(num), "tokens": m.tokens.iter().map(|t| raw_token(t)).collect::<Vec<_>>(),
            "burst": m.burst, "missing": m.missing.iter().map(|&x| item(x)).collect::<Vec<_>>(),
            "filler": m.filler.iter().map(|&j| raw_token(&m.tokens[j])).collect::<Vec<_>>()})).unwrap_or(Value::Null)).collect::<Vec<_>>(),
        "pre": r.pre.iter().map(|t| raw_token(t)).collect::<Vec<_>>()})).collect();
    json!({"rows": rows, "usual": b.usual.iter().map(|col| col.iter().map(|&x| item(x)).collect::<Vec<_>>()).collect::<Vec<_>>(),
           "structure": b.structure.iter().map(|(n, ts)| json!({"name": n, "tokens": ts.iter().map(|t| raw_token(t)).collect::<Vec<_>>()})).collect::<Vec<_>>(),
           "player": bench.side.player, "log": super::PERMALINK,
           "secs": bb_core::kit::round_n((bench.side.end - bench.side.t0) / 1000.0, 1), "full": b.full})
}
