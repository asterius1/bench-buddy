//! Python's `difflib.SequenceMatcher(None, a, b, autojunk=False).get_opcodes()`, step for step, so
//! the ordered matching pairs the same casts as the Python checker did.

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    Equal,
    Replace,
    Delete,
    Insert,
}

fn longest(a: &[i64], b: &[i64], b2j: &HashMap<i64, Vec<usize>>, alo: usize, ahi: usize, blo: usize, bhi: usize)
           -> (usize, usize, usize) {
    let (mut besti, mut bestj, mut bestsize) = (alo, blo, 0);
    let mut j2len: HashMap<usize, usize> = HashMap::new();
    for i in alo..ahi {
        let mut newj2len = HashMap::new();
        if let Some(js) = b2j.get(&a[i]) {
            for &j in js {
                if j < blo {
                    continue;
                }
                if j >= bhi {
                    break;
                }
                let k = if j > 0 { j2len.get(&(j - 1)).copied().unwrap_or(0) } else { 0 } + 1;
                newj2len.insert(j, k);
                if k > bestsize {
                    besti = i + 1 - k;
                    bestj = j + 1 - k;
                    bestsize = k;
                }
            }
        }
        j2len = newj2len;
    }
    while besti > alo && bestj > blo && a[besti - 1] == b[bestj - 1] {
        besti -= 1;
        bestj -= 1;
        bestsize += 1;
    }
    while besti + bestsize < ahi && bestj + bestsize < bhi && a[besti + bestsize] == b[bestj + bestsize] {
        bestsize += 1;
    }
    (besti, bestj, bestsize)
}

fn matching_blocks(a: &[i64], b: &[i64]) -> Vec<(usize, usize, usize)> {
    let mut b2j: HashMap<i64, Vec<usize>> = HashMap::new();
    for (i, &x) in b.iter().enumerate() {
        b2j.entry(x).or_default().push(i);
    }
    let mut queue = vec![(0, a.len(), 0, b.len())];
    let mut blocks = vec![];
    while let Some((alo, ahi, blo, bhi)) = queue.pop() {
        let (i, j, k) = longest(a, b, &b2j, alo, ahi, blo, bhi);
        if k > 0 {
            blocks.push((i, j, k));
            if alo < i && blo < j {
                queue.push((alo, i, blo, j));
            }
            if i + k < ahi && j + k < bhi {
                queue.push((i + k, ahi, j + k, bhi));
            }
        }
    }
    blocks.sort();
    let mut out = vec![];
    let (mut i1, mut j1, mut k1) = (0, 0, 0);
    for (i2, j2, k2) in blocks {
        if i1 + k1 == i2 && j1 + k1 == j2 {
            k1 += k2;
        } else {
            if k1 > 0 {
                out.push((i1, j1, k1));
            }
            (i1, j1, k1) = (i2, j2, k2);
        }
    }
    if k1 > 0 {
        out.push((i1, j1, k1));
    }
    out.push((a.len(), b.len(), 0));
    out
}

pub fn opcodes(a: &[i64], b: &[i64]) -> Vec<(Op, usize, usize, usize, usize)> {
    let (mut i, mut j) = (0, 0);
    let mut out = vec![];
    for (ai, bj, size) in matching_blocks(a, b) {
        let op = if i < ai && j < bj {
            Some(Op::Replace)
        } else if i < ai {
            Some(Op::Delete)
        } else if j < bj {
            Some(Op::Insert)
        } else {
            None
        };
        if let Some(op) = op {
            out.push((op, i, ai, j, bj));
        }
        i = ai + size;
        j = bj + size;
        if size > 0 {
            out.push((Op::Equal, ai, i, bj, j));
        }
    }
    out
}
