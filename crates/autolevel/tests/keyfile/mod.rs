//! The answer keys as files: read once, their objects' names in the file's order, and re-written
//! from the Rust's own output when a rule is changed on purpose (AUTOLEVEL.md, decision 2).
//!
//! `AUTOLEVEL_KEY_REWRITE=1 cargo test --release -p autolevel -p autolevel_build --test key`
//! re-writes every value of a key that the Rust no longer gives, in the file's own text: every
//! other byte, and the order of every object's names, stay as they were. Only what a test
//! compares and reaches the end of can be re-written (a count that changes is still a failure),
//! and a test that re-wrote anything says so; run the tests again without it to see them pass.
//!
//! Shared by both crates' key tests (`autolevel_build`'s takes it by path).

#![allow(dead_code)] // each test crate uses part of it

use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;

/// A key's objects with their names in the order the file has them (serde_json's `Value` sorts
/// them, and the order of a level's rooms, walks and views is part of the key), and where each
/// value lies in the text.
pub enum Tree {
    Leaf(Span),
    Arr(Vec<Tree>, Span),
    Obj(Vec<(String, Tree)>, Span),
}

/// Bytes `start..end` of a key's text.
pub type Span = (usize, usize);

impl Tree {
    /// Read one JSON value from `text` at `at`, leaving `at` just past it.
    pub fn read(text: &str, at: &mut usize) -> Tree {
        let b = text.as_bytes();
        let skip = |at: &mut usize| {
            while b[*at].is_ascii_whitespace() {
                *at += 1;
            }
        };
        // a string's text, quotes and all
        let string = |at: &mut usize| {
            let start = *at;
            *at += 1;
            while b[*at] != b'"' {
                *at += if b[*at] == b'\\' { 2 } else { 1 };
            }
            *at += 1;
            &text[start..*at]
        };
        skip(at);
        let start = *at;
        match b[*at] {
            b'{' => {
                let mut out = Vec::new();
                *at += 1;
                loop {
                    skip(at);
                    if b[*at] == b'}' {
                        *at += 1;
                        return Tree::Obj(out, (start, *at));
                    }
                    if b[*at] == b',' {
                        *at += 1;
                        continue;
                    }
                    let name: String = serde_json::from_str(string(at)).unwrap();
                    skip(at);
                    assert_eq!(b[*at], b':', "a name with no value at byte {at}");
                    *at += 1;
                    out.push((name, Tree::read(text, at)));
                }
            }
            b'[' => {
                let mut out = Vec::new();
                *at += 1;
                loop {
                    skip(at);
                    if b[*at] == b']' {
                        *at += 1;
                        return Tree::Arr(out, (start, *at));
                    }
                    if b[*at] == b',' {
                        *at += 1;
                        continue;
                    }
                    out.push(Tree::read(text, at));
                }
            }
            b'"' => {
                string(at);
                Tree::Leaf((start, *at))
            }
            _ => {
                while !matches!(b[*at], b',' | b']' | b'}') && !b[*at].is_ascii_whitespace() {
                    *at += 1;
                }
                Tree::Leaf((start, *at))
            }
        }
    }

    pub fn items(&self) -> &[Tree] {
        match self {
            Tree::Arr(items, _) => items,
            _ => panic!("not a list"),
        }
    }

    pub fn get(&self, name: &str) -> &Tree {
        match self {
            Tree::Obj(fields, _) => fields.iter().find(|(k, _)| k == name).map_or_else(|| panic!("no {name} in the key"), |(_, v)| v),
            _ => panic!("not an object"),
        }
    }

    /// An object's names, in the file's order.
    pub fn names(&self) -> Vec<&str> {
        match self {
            Tree::Obj(fields, _) => fields.iter().map(|(k, _)| k.as_str()).collect(),
            _ => panic!("not an object"),
        }
    }

    fn span(&self) -> Span {
        match self {
            Tree::Leaf(s) | Tree::Arr(_, s) | Tree::Obj(_, s) => *s,
        }
    }
}

/// Is the key to be re-written from the Rust's output?
pub fn rewriting() -> bool {
    std::env::var_os("AUTOLEVEL_KEY_REWRITE").is_some()
}

/// A key being re-written: its file, its text, where each of its values lies (by the value's
/// address: the key's `Value` is never dropped), and the values to put in.
struct Open {
    /// The key's own value (its address), as the test that read it holds it.
    root: usize,
    path: String,
    text: String,
    spans: HashMap<usize, Span>,
    patches: Vec<(Span, String)>,
}

static OPEN: Mutex<Vec<Open>> = Mutex::new(Vec::new());

fn address(v: &Value) -> usize {
    v as *const Value as usize
}

/// Each value's place in the text, walking the value and the text's tree side by side.
fn index(v: &Value, t: &Tree, spans: &mut HashMap<usize, Span>) {
    spans.insert(address(v), t.span());
    match (v, t) {
        (Value::Array(items), Tree::Arr(trees, _)) => items.iter().zip(trees).for_each(|(v, t)| index(v, t, spans)),
        (Value::Object(map), Tree::Obj(fields, _)) => {
            for (name, t) in fields {
                index(&map[name], t, spans);
            }
        }
        _ => {}
    }
}

/// The key at `path`, read once and kept for the rest of the run.
pub fn read(path: &str, missing: &str) -> &'static Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e} ({missing})"));
    let value: &'static Value = Box::leak(Box::new(serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"))));
    if rewriting() {
        let mut spans = HashMap::new();
        index(value, &Tree::read(&text, &mut 0), &mut spans);
        OPEN.lock().unwrap().push(Open { root: address(value), path: path.to_string(), text, spans, patches: Vec::new() });
    }
    value
}

/// The key's value `want` is to become `mine`: true if the key is being re-written (and it is
/// noted), false if not (the caller fails as it would have).
pub fn take(want: &Value, mine: &Value) -> bool {
    if !rewriting() {
        return false;
    }
    let mut open = OPEN.lock().unwrap();
    let Some(file) = open.iter_mut().find(|f| f.spans.contains_key(&address(want))) else {
        panic!("a value that is not in a key read for re-writing");
    };
    let span = file.spans[&address(want)];
    file.patches.push((span, serde_json::to_string(mine).unwrap()));
    true
}

/// `mine` against the key's `want` (`same` saying whether they agree): if not, the key takes
/// `mine` when it is being re-written, and otherwise this fails, saying `at`.
pub fn agree(mine: &Value, want: Option<&Value>, same: impl Fn(&Value, &Value) -> bool, at: impl FnOnce() -> String) {
    if let Some(w) = want {
        if same(mine, w) || take(w, mine) {
            return;
        }
    }
    assert_eq!(Some(mine), want, "{}", at());
}

/// Write `key` (as `read` gave it) if it took any value, and say how many. Call at the end of a
/// test that compared everything (a test that stops early must not leave half a key); the tests
/// run side by side, so each writes only the key it read.
pub fn finish(key: &Value) {
    let mut open = OPEN.lock().unwrap();
    for file in open.iter_mut().filter(|f| f.root == address(key) && !f.patches.is_empty()) {
        let mut patches = std::mem::take(&mut file.patches);
        patches.sort();
        patches.dedup();
        let mut out = String::with_capacity(file.text.len());
        let mut at = 0;
        for ((start, end), text) in &patches {
            assert!(*start >= at, "{}: two values to re-write overlap at byte {start}", file.path);
            out.push_str(&file.text[at..*start]);
            out.push_str(text);
            at = *end;
        }
        out.push_str(&file.text[at..]);
        std::fs::write(&file.path, &out).unwrap_or_else(|e| panic!("{}: {e}", file.path));
        eprintln!("re-wrote {} values of {} from the Rust's output", patches.len(), file.path);
        file.text = out;
        file.spans.clear(); // the places are the old text's: nothing more is taken from this read
    }
}
