//! Printing as the Python tools printed: values in Python's repr (lists, tuples, dicts, None) and
//! JSON in Python's json.dumps layout, so their outputs stay comparable with older runs.

use std::fmt::{self, Display, Write};

use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub enum Py {
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<Py>),
    Tuple(Vec<Py>),
    Dict(Vec<(Py, Py)>),
}

pub fn float_repr(x: f64) -> String {
    if x.is_finite() && x.fract() == 0.0 && x.abs() < 1e16 {
        format!("{x:.1}")
    } else {
        let s = format!("{x:?}");
        // Python writes the exponent with a sign and two digits: 1e-07, 1e+16
        match s.split_once('e') {
            Some((m, e)) => {
                let (sign, d) = e.strip_prefix('-').map_or(("+", e), |d| ("-", d));
                format!("{m}e{sign}{d:0>2}")
            }
            None => s,
        }
    }
}

fn str_repr(s: &str) -> String {
    let q = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::new();
    out.push(q);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if c == q => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push(q);
    out
}

impl Display for Py {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Py::None => write!(f, "None"),
            Py::Bool(b) => write!(f, "{}", if *b { "True" } else { "False" }),
            Py::Int(i) => write!(f, "{i}"),
            Py::Float(x) => write!(f, "{}", float_repr(*x)),
            Py::Str(s) => write!(f, "{}", str_repr(s)),
            Py::List(v) => write!(f, "[{}]", v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", ")),
            Py::Tuple(v) if v.len() == 1 => write!(f, "({},)", v[0]),
            Py::Tuple(v) => write!(f, "({})", v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", ")),
            Py::Dict(v) => write!(f, "{{{}}}", v.iter().map(|(k, x)| format!("{k}: {x}")).collect::<Vec<_>>().join(", ")),
        }
    }
}

/// A JSON value as Python sees it after json.loads: whole numbers that were written as integers
/// stay integers.
pub fn of(v: &Value) -> Py {
    match v {
        Value::Null => Py::None,
        Value::Bool(b) => Py::Bool(*b),
        Value::Number(n) => n.as_i64().map(Py::Int).unwrap_or_else(|| Py::Float(n.as_f64().unwrap_or(0.0))),
        Value::String(s) => Py::Str(s.clone()),
        Value::Array(a) => Py::List(a.iter().map(of).collect()),
        Value::Object(o) => Py::Dict(o.iter().map(|(k, x)| (Py::Str(k.clone()), of(x))).collect()),
    }
}

fn json_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                let mut b = [0u16; 2];
                for u in c.encode_utf16(&mut b) {
                    let _ = write!(out, "\\u{u:04x}");
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn dump(v: &Value, indent: usize, level: usize, out: &mut String) {
    let pad = |n: usize| " ".repeat(n * indent);
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => match n.as_i64() {
            Some(i) => {
                let _ = write!(out, "{i}");
            }
            None => out.push_str(&float_repr(n.as_f64().unwrap_or(0.0))),
        },
        Value::String(s) => json_str(s, out),
        Value::Array(a) if a.is_empty() => out.push_str("[]"),
        Value::Object(o) if o.is_empty() => out.push_str("{}"),
        Value::Array(a) => {
            out.push_str("[\n");
            for (i, x) in a.iter().enumerate() {
                out.push_str(&pad(level + 1));
                dump(x, indent, level + 1, out);
                out.push_str(if i + 1 < a.len() { ",\n" } else { "\n" });
            }
            out.push_str(&pad(level));
            out.push(']');
        }
        Value::Object(o) => {
            out.push_str("{\n");
            for (i, (k, x)) in o.iter().enumerate() {
                out.push_str(&pad(level + 1));
                json_str(k, out);
                out.push_str(": ");
                dump(x, indent, level + 1, out);
                out.push_str(if i + 1 < o.len() { ",\n" } else { "\n" });
            }
            out.push_str(&pad(level));
            out.push('}');
        }
    }
}

/// json.dumps(v, indent=indent)
pub fn dumps(v: &Value, indent: usize) -> String {
    let mut s = String::new();
    dump(v, indent, 0, &mut s);
    s
}

/// A Counter's most_common(): highest count first, ties in the order first seen.
pub fn most_common<K: Clone + PartialEq>(c: &bb_core::omap::OMap<K, i64>) -> Vec<(K, i64)> {
    let mut v: Vec<(K, i64)> = c.iter().map(|(k, n)| (k.clone(), *n)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v
}

/// statistics.median
pub fn median(xs: &[f64]) -> f64 {
    bb_core::kit::median(xs)
}
