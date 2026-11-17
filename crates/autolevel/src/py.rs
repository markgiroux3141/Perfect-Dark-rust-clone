//! Where Python's arithmetic and Rust's differ. The port has to give Python's answers, so the
//! differences are named here once and used everywhere.
//!
//! - `round()` rounds halves to the even number (Rust's `round` rounds them away from zero).
//! - `//` and `%` floor (Rust's `/` and `%` truncate): use `div_euclid` and `rem_euclid`, which
//!   agree with Python for a positive divisor.
//! - `max()` and `min()` return the FIRST of equal items. Rust's `Iterator::max_by_key` returns
//!   the last, and `min_by_key` the first: use `first_max` and `first_min`.
//! - `sorted()` is stable, as `sort_by` is. `sorted(reverse=True)` keeps equal items in their
//!   original order too: sort with the comparison reversed, never sort and then reverse.

use std::cmp::Ordering;

/// Python's `round(x)` to a whole number: halves go to the even neighbour.
pub fn round(x: f64) -> i64 {
    x.round_ties_even() as i64
}

/// Python's `round(x, 1)`, for the numbers the generator rounds so: lengths in whole and half
/// cells, which ten times over are whole. (For other numbers Python rounds the decimal it would
/// print, which this does not.)
pub fn round1(x: f64) -> f64 {
    (x * 10.0).round_ties_even() / 10.0
}

/// Python's `round(x, digits)` for any float: the nearest number with that many decimal places to
/// the float's exact value (a tie, which only a float that is exactly a half can be, goes to the
/// even digit), read back as a float. Formatting to a fixed number of places rounds the same way.
pub fn round_to(x: f64, digits: usize) -> f64 {
    format!("{x:.digits$}").parse().expect("a float that was just written")
}

/// Python's `repr()` of a string of plain characters: in single quotes, or in double quotes if
/// it holds a single quote and no double one.
pub fn repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::from(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Python's `repr()` of a list of strings: `['a', 'b']`.
pub fn repr_list<S: AsRef<str>>(items: &[S]) -> String {
    format!("[{}]", items.iter().map(|s| repr(s.as_ref())).collect::<Vec<_>>().join(", "))
}

/// Python's `max(items, key=key)`: the first item with the greatest key. None if there are none.
pub fn first_max<T, K: PartialOrd>(items: impl IntoIterator<Item = T>, mut key: impl FnMut(&T) -> K) -> Option<T> {
    let mut best: Option<(K, T)> = None;
    for item in items {
        let k = key(&item);
        if best.as_ref().is_none_or(|(b, _)| k.partial_cmp(b) == Some(Ordering::Greater)) {
            best = Some((k, item));
        }
    }
    best.map(|(_, item)| item)
}

/// Python's `min(items, key=key)`: the first item with the least key. None if there are none.
pub fn first_min<T, K: PartialOrd>(items: impl IntoIterator<Item = T>, mut key: impl FnMut(&T) -> K) -> Option<T> {
    let mut best: Option<(K, T)> = None;
    for item in items {
        let k = key(&item);
        if best.as_ref().is_none_or(|(b, _)| k.partial_cmp(b) == Some(Ordering::Less)) {
            best = Some((k, item));
        }
    }
    best.map(|(_, item)| item)
}

/// An ordering for float sort keys (none of the generator's keys is ever NaN).
pub fn cmp_f64(a: f64, b: f64) -> Ordering {
    a.partial_cmp(&b).expect("a sort key was NaN")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves_round_to_even_and_ties_go_to_the_first() {
        assert_eq!([0.5, 1.5, 2.5, -0.5, -1.5, 2.4, 2.6].map(round), [0, 2, 2, 0, -2, 2, 3]);
        assert_eq!([0.0, 13.0, 14.5, 0.25, 7.75].map(round1), [0.0, 13.0, 14.5, 0.2, 7.8]);
        let items = [(1, 'a'), (3, 'b'), (3, 'c'), (1, 'd')];
        assert_eq!(first_max(items, |t| t.0), Some((3, 'b')));
        assert_eq!(first_min(items, |t| t.0), Some((1, 'a')));
        assert_eq!(first_max(Vec::<i32>::new(), |v| *v), None);
        assert_eq!((-7i32).div_euclid(2), -4); // Python's -7 // 2
        assert_eq!((-7i32).rem_euclid(2), 1); // Python's -7 % 2
    }

    #[test]
    fn rounding_to_places_is_pythons() {
        // (x, digits, Python's round(x, digits)): ties on the exact value go to even, and a decimal
        // that is not exactly a half (2.675, 1.005, 8.05) goes the way its float lies
        let cases = [
            (0.125, 2, 0.12),
            (0.375, 2, 0.38),
            (2.675, 2, 2.67),
            (1.005, 2, 1.0),
            (0.285, 2, 0.28),
            (2.5, 0, 2.0),
            (0.5, 0, 0.0),
            (1.5, 0, 2.0),
            (16.666666666666668, 1, 16.7),
            (0.20833333333333334, 2, 0.21),
            (0.47912345, 2, 0.48),
            (125.0, 1, 125.0),
            (36.845, 2, 36.84),
            (36.835, 2, 36.84),
            (7.25, 1, 7.2),
            (7.35, 1, 7.3),
            (0.045, 2, 0.04),
            (1e-9, 2, 0.0),
            (12345.67895, 3, 12345.679),
            (0.6649999999999999, 2, 0.66),
            (8.05, 1, 8.1),
            (0.05, 1, 0.1),
            (0.15, 1, 0.1),
            (0.25, 1, 0.2),
            (0.35, 1, 0.3),
        ];
        for (x, digits, want) in cases {
            assert_eq!(round_to(x, digits), want, "round({x}, {digits})");
        }
        assert_eq!(round_to(-0.004, 2).to_bits(), (-0.0f64).to_bits());
        // and where round1 is used (whole and half cells) the two agree
        for x in [0.0, 13.0, 14.5, 7.5, 21.0] {
            assert_eq!(round_to(x, 1), round1(x));
        }
    }

    #[test]
    fn strings_are_written_as_python_writes_them() {
        assert_eq!(repr("overlap a/b at (1, 2, 3)..(4, 5, 6)"), "'overlap a/b at (1, 2, 3)..(4, 5, 6)'");
        assert_eq!(repr("link declared 2 times: ['a', 'b']"), "\"link declared 2 times: ['a', 'b']\"");
        assert_eq!(repr("it's \"so\""), "'it\\'s \"so\"'");
        assert_eq!(repr_list(&["a", "b"]), "['a', 'b']");
        assert_eq!(repr_list::<&str>(&[]), "[]");
    }
}
