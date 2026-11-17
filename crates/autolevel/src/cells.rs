//! Sets of grid cells. The Python keeps them in hash sets, whose order nobody defines; here
//! they are ordered sets, so anything that walks one does it the same way every time.
//!
//! Two grids use them: the router's cells (3 m) and the furnisher's half cells ("h", 59 units).

use std::collections::BTreeSet;

/// (x, y) of a cell.
pub type Cell = (i32, i32);
/// A set of cells, ordered by x then y.
pub type Cells = BTreeSet<Cell>;
/// (x0, y0, x1, y1): the cells with x0 <= x < x1 and y0 <= y < y1.
pub type Rect = (i32, i32, i32, i32);

/// Every cell of a rectangle.
pub fn rect_cells(r: Rect) -> Cells {
    (r.0..r.2).flat_map(|x| (r.1..r.3).map(move |y| (x, y))).collect()
}

/// A set of cells as rows `[y, x0, x1)` of consecutive cells, sorted: the one spelling the
/// answer key writes sets in.
pub fn runs(cells: &Cells) -> Vec<[i32; 3]> {
    let mut sorted: Vec<Cell> = cells.iter().copied().collect();
    sorted.sort_by_key(|&(x, y)| (y, x));
    let mut out: Vec<[i32; 3]> = Vec::new();
    for (x, y) in sorted {
        match out.last_mut() {
            Some(row) if row[0] == y && row[2] == x => row[2] = x + 1,
            _ => out.push([y, x, x + 1]),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rectangle_is_its_rows() {
        let mut cells = rect_cells((2, 5, 5, 7));
        assert_eq!(cells.len(), 6);
        assert_eq!(runs(&cells), [[5, 2, 5], [6, 2, 5]]);
        cells.remove(&(3, 6));
        assert_eq!(runs(&cells), [[5, 2, 5], [6, 2, 3], [6, 4, 5]]);
    }
}
