//! Orthogonal edge routing on the cell grid.
//!
//! Terminals have no sub-cell resolution, so every edge is a Manhattan path
//! with at most two bends: it leaves the source on the side facing the target,
//! runs to the midpoint, turns, and enters the target from the opposite side.
//! That keeps lines readable with box-drawing characters and makes the pulse
//! animation trivial (walk along the segments).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    pub fn cx(&self) -> i32 {
        self.x + self.w / 2
    }
    pub fn cy(&self) -> i32 {
        self.y + self.h / 2
    }
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// Which side of `a` the edge leaves from and which side of `b` it enters.
pub fn sides(a: Rect, b: Rect) -> (Dir, Dir) {
    let gap_x = if b.x >= a.right() {
        b.x - a.right()
    } else if a.x >= b.right() {
        a.x - b.right()
    } else {
        -1
    };
    let gap_y = if b.y >= a.bottom() {
        b.y - a.bottom()
    } else if a.y >= b.bottom() {
        a.y - b.bottom()
    } else {
        -1
    };
    if gap_x >= 0 && (gap_y < 0 || gap_x >= gap_y) {
        if b.x >= a.right() {
            (Dir::Right, Dir::Left)
        } else {
            (Dir::Left, Dir::Right)
        }
    } else if gap_y >= 0 {
        if b.y >= a.bottom() {
            (Dir::Down, Dir::Up)
        } else {
            (Dir::Up, Dir::Down)
        }
    } else {
        (Dir::Right, Dir::Left)
    }
}

/// Polyline (inclusive corner points) from the border of `a` to the border of `b`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn route(a: Rect, b: Rect) -> Vec<(i32, i32)> {
    route_offset(a, b, 0, 0)
}

/// Like [`route`], but the ports are shifted along their side by `off_a` /
/// `off_b` cells so several edges on one side do not share a row or column.
pub fn route_offset(a: Rect, b: Rect, off_a: i32, off_b: i32) -> Vec<(i32, i32)> {
    let clamp_y = |r: Rect, off: i32| (r.cy() + off).clamp(r.y + 1, r.bottom() - 2);
    let clamp_x = |r: Rect, off: i32| (r.cx() + off).clamp(r.x + 1, r.right() - 2);
    let gap_x = if b.x >= a.right() {
        b.x - a.right()
    } else if a.x >= b.right() {
        a.x - b.right()
    } else {
        -1
    };
    let gap_y = if b.y >= a.bottom() {
        b.y - a.bottom()
    } else if a.y >= b.bottom() {
        a.y - b.bottom()
    } else {
        -1
    };

    // Prefer the horizontal exit when the boxes are side by side.
    if gap_x >= 0 && (gap_y < 0 || gap_x >= gap_y) {
        let (sx, ex) = if b.x >= a.right() {
            (a.right() - 1, b.x)
        } else {
            (a.x, b.right() - 1)
        };
        let sy = clamp_y(a, off_a);
        let mut ey = clamp_y(b, off_b);
        if (sy - ey).abs() <= 1 {
            ey = sy; // a one-row jog reads as a glitch; keep it straight
        }
        let mid = (sx + ex) / 2;
        if sy == ey {
            return vec![(sx, sy), (ex, ey)];
        }
        return vec![(sx, sy), (mid, sy), (mid, ey), (ex, ey)];
    }
    if gap_y >= 0 {
        let (sy, ey) = if b.y >= a.bottom() {
            (a.bottom() - 1, b.y)
        } else {
            (a.y, b.bottom() - 1)
        };
        let sx = clamp_x(a, off_a);
        let mut ex = clamp_x(b, off_b);
        if (sx - ex).abs() <= 2 {
            ex = sx;
        }
        let mid = (sy + ey) / 2;
        if sx == ex {
            return vec![(sx, sy), (ex, ey)];
        }
        return vec![(sx, sy), (sx, mid), (ex, mid), (ex, ey)];
    }
    // Overlapping boxes: draw a straight segment between centres.
    vec![(a.cx(), a.cy()), (b.cx(), b.cy())]
}

/// Like [`route_offset`], but the path steers around `obstacles` (other
/// nodes' rects). The two-bend route is kept when it is clear; otherwise an
/// A* search on the cell grid (turns cost extra, so the result stays
/// Manhattan-readable) finds a detour, and when no detour exists the plain
/// route is returned unchanged.
pub fn route_avoiding(a: Rect, b: Rect, off_a: i32, off_b: i32, obstacles: &[Rect]) -> Vec<(i32, i32)> {
    let direct = route_offset(a, b, off_a, off_b);
    let blockers: Vec<Rect> = obstacles
        .iter()
        .copied()
        .filter(|o| *o != a && *o != b && !overlaps(*o, a) && !overlaps(*o, b))
        .collect();
    if blockers.is_empty() || !blockers.iter().any(|o| crosses(&direct, *o)) {
        return direct;
    }
    if direct.len() < 2 {
        return direct;
    }
    let (sx, sy) = direct[0];
    let (ex, ey) = direct[direct.len() - 1];
    let exit = dir_from(sx, sy, direct[1].0, direct[1].1);
    let entry = dir_from(direct[direct.len() - 2].0, direct[direct.len() - 2].1, ex, ey);
    let (ox, oy) = step(exit);
    let (ix, iy) = step(entry);
    // Search from the cell just outside the source port to the cell just
    // outside the target port, then glue the ports back on.
    let start = (sx + ox, sy + oy, exit);
    let goal = (ex - ix, ey - iy);
    // Search area: the two endpoints plus every blocker, with a margin.
    let mut bx0 = a.x.min(b.x);
    let mut by0 = a.y.min(b.y);
    let mut bx1 = a.right().max(b.right());
    let mut by1 = a.bottom().max(b.bottom());
    for o in &blockers {
        bx0 = bx0.min(o.x);
        by0 = by0.min(o.y);
        bx1 = bx1.max(o.right());
        by1 = by1.max(o.bottom());
    }
    let bounds = Rect { x: bx0 - 3, y: by0 - 3, w: bx1 - bx0 + 6, h: by1 - by0 + 6 };
    // First keep one cell of air around every box so lines do not hug
    // borders; when boxes sit too close for that, allow hugging.
    for pad in [1, 0] {
        let inflated: Vec<Rect> = blockers
            .iter()
            .chain([a, b].iter())
            .map(|r| Rect { x: r.x - pad, y: r.y - pad, w: r.w + 2 * pad, h: r.h + 2 * pad })
            .collect();
        let blocked = |x: i32, y: i32| -> bool {
            if !bounds.contains(x, y) {
                return true;
            }
            // The cells right outside the ports sit inside the endpoints'
            // inflated rects; they are the only exceptions.
            if (x, y) == (start.0, start.1) || (x, y) == goal {
                return false;
            }
            inflated.iter().any(|r| r.contains(x, y))
        };
        if let Some(mut cells) = astar(start, goal, &blocked) {
            cells.insert(0, (sx, sy));
            cells.push((ex, ey));
            return simplify(&cells);
        }
    }
    direct
}

fn overlaps(p: Rect, q: Rect) -> bool {
    p.x < q.right() && q.x < p.right() && p.y < q.bottom() && q.y < p.bottom()
}

/// True when any cell of the polyline lies inside `r`.
pub fn crosses(points: &[(i32, i32)], r: Rect) -> bool {
    for w in points.windows(2) {
        let (x0, y0) = w[0];
        let (x1, y1) = w[1];
        if y0 == y1 {
            let (lo, hi) = (x0.min(x1), x0.max(x1));
            if y0 >= r.y && y0 < r.bottom() && hi >= r.x && lo < r.right() {
                return true;
            }
        } else {
            let (lo, hi) = (y0.min(y1), y0.max(y1));
            if x0 >= r.x && x0 < r.right() && hi >= r.y && lo < r.bottom() {
                return true;
            }
        }
    }
    false
}

fn step(d: Dir) -> (i32, i32) {
    match d {
        Dir::Up => (0, -1),
        Dir::Down => (0, 1),
        Dir::Left => (-1, 0),
        Dir::Right => (1, 0),
    }
}

const TURN_COST: i32 = 4;

/// A* over (x, y, heading). Returns every cell from `start` to `goal`.
fn astar(
    start: (i32, i32, Dir),
    goal: (i32, i32),
    blocked: &dyn Fn(i32, i32) -> bool,
) -> Option<Vec<(i32, i32)>> {
    use std::cmp::Reverse;
    use std::collections::{BinaryHeap, HashMap};
    if blocked(start.0, start.1) || blocked(goal.0, goal.1) {
        return None;
    }
    let h = |x: i32, y: i32| (x - goal.0).abs() + (y - goal.1).abs();
    let mut best: HashMap<(i32, i32, Dir), i32> = HashMap::new();
    let mut came: HashMap<(i32, i32, Dir), (i32, i32, Dir)> = HashMap::new();
    let mut open = BinaryHeap::new();
    best.insert(start, 0);
    open.push(Reverse((h(start.0, start.1), 0, start.0, start.1, start.2)));
    let mut expanded = 0usize;
    while let Some(Reverse((_, g, x, y, d))) = open.pop() {
        if best.get(&(x, y, d)).map(|b| g > *b).unwrap_or(false) {
            continue;
        }
        if (x, y) == goal {
            let mut path = vec![(x, y)];
            let mut cur = (x, y, d);
            while let Some(p) = came.get(&cur) {
                path.push((p.0, p.1));
                cur = *p;
            }
            path.reverse();
            return Some(path);
        }
        expanded += 1;
        if expanded > 60_000 {
            return None;
        }
        for nd in [Dir::Up, Dir::Down, Dir::Left, Dir::Right] {
            if opposite(nd) == d {
                continue;
            }
            let (dx, dy) = step(nd);
            let (nx, ny) = (x + dx, y + dy);
            if blocked(nx, ny) {
                continue;
            }
            let ng = g + 1 + if nd != d { TURN_COST } else { 0 };
            let key = (nx, ny, nd);
            if best.get(&key).map(|b| ng < *b).unwrap_or(true) {
                best.insert(key, ng);
                came.insert(key, (x, y, d));
                open.push(Reverse((ng + h(nx, ny), ng, nx, ny, nd)));
            }
        }
    }
    None
}

fn opposite(d: Dir) -> Dir {
    match d {
        Dir::Up => Dir::Down,
        Dir::Down => Dir::Up,
        Dir::Left => Dir::Right,
        Dir::Right => Dir::Left,
    }
}

/// Drop collinear interior cells so only endpoints and corners remain.
fn simplify(cells: &[(i32, i32)]) -> Vec<(i32, i32)> {
    let mut out: Vec<(i32, i32)> = Vec::new();
    for &c in cells {
        if out.last() == Some(&c) {
            continue;
        }
        if out.len() >= 2 {
            let p = out[out.len() - 2];
            let q = out[out.len() - 1];
            if (p.0 == q.0 && q.0 == c.0) || (p.1 == q.1 && q.1 == c.1) {
                *out.last_mut().unwrap() = c;
                continue;
            }
        }
        out.push(c);
    }
    out
}

#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub x: i32,
    pub y: i32,
    pub ch: char,
}

/// Rasterise a polyline into box-drawing cells. `heavy` switches to the
/// thick glyph set. The final cell is an arrowhead pointing into the target.
pub fn rasterize(points: &[(i32, i32)], heavy: bool) -> Vec<Cell> {
    let (h, v) = if heavy { ('━', '┃') } else { ('─', '│') };
    let mut cells: Vec<Cell> = Vec::new();
    if points.len() < 2 {
        return cells;
    }
    // Consecutive segments share their joint; push it once so the corner
    // glyph below is not overdrawn by the next segment's plain bar.
    let push = |cells: &mut Vec<Cell>, x: i32, y: i32, ch: char| {
        if cells.last().map(|c| c.x == x && c.y == y).unwrap_or(false) {
            return;
        }
        cells.push(Cell { x, y, ch });
    };
    for w in points.windows(2) {
        let (x0, y0) = w[0];
        let (x1, y1) = w[1];
        if y0 == y1 {
            let dx = (x1 - x0).signum();
            let mut x = x0;
            loop {
                push(&mut cells, x, y0, h);
                if x == x1 {
                    break;
                }
                x += dx;
            }
        } else {
            let dy = (y1 - y0).signum();
            let mut y = y0;
            loop {
                push(&mut cells, x0, y, v);
                if y == y1 {
                    break;
                }
                y += dy;
            }
        }
    }
    // Corners: replace the cell at each interior point with the right joint.
    for i in 1..points.len() - 1 {
        let (px, py) = points[i];
        let (ax, ay) = points[i - 1];
        let (bx, by) = points[i + 1];
        let from = dir_from(px, py, ax, ay);
        let to = dir_from(px, py, bx, by);
        let ch = corner(from, to, heavy);
        if let Some(c) = cells.iter_mut().find(|c| c.x == px && c.y == py) {
            c.ch = ch;
        }
    }
    // Arrowhead on the last cell.
    let n = points.len();
    let (ex, ey) = points[n - 1];
    let (px, py) = points[n - 2];
    let arrow = match dir_from(px, py, ex, ey) {
        Dir::Right => '▶',
        Dir::Left => '◀',
        Dir::Down => '▼',
        Dir::Up => '▲',
    };
    if let Some(c) = cells.iter_mut().rev().find(|c| c.x == ex && c.y == ey) {
        c.ch = arrow;
    }
    cells
}

/// Direction from (x0,y0) towards (x1,y1); assumes axis alignment.
fn dir_from(x0: i32, y0: i32, x1: i32, y1: i32) -> Dir {
    if x1 > x0 {
        Dir::Right
    } else if x1 < x0 {
        Dir::Left
    } else if y1 > y0 {
        Dir::Down
    } else {
        Dir::Up
    }
}

fn corner(a: Dir, b: Dir, heavy: bool) -> char {
    use Dir::*;
    let set = |l: char, h: char| if heavy { h } else { l };
    match (a, b) {
        (Right, Down) | (Down, Right) => set('┌', '┏'),
        (Left, Down) | (Down, Left) => set('┐', '┓'),
        (Right, Up) | (Up, Right) => set('└', '┗'),
        (Left, Up) | (Up, Left) => set('┘', '┛'),
        (Left, Right) | (Right, Left) => set('─', '━'),
        _ => set('│', '┃'),
    }
}

pub fn length(points: &[(i32, i32)]) -> i32 {
    points
        .windows(2)
        .map(|w| (w[1].0 - w[0].0).abs() + (w[1].1 - w[0].1).abs())
        .sum()
}

/// Point at fraction `t` (0..=1) along the polyline.
pub fn point_at(points: &[(i32, i32)], t: f32) -> (i32, i32) {
    if points.is_empty() {
        return (0, 0);
    }
    let total = length(points).max(1) as f32;
    let mut remaining = (t.clamp(0.0, 1.0) * total).round() as i32;
    for w in points.windows(2) {
        let (x0, y0) = w[0];
        let (x1, y1) = w[1];
        let seg = (x1 - x0).abs() + (y1 - y0).abs();
        if remaining <= seg {
            let dx = (x1 - x0).signum();
            let dy = (y1 - y0).signum();
            return (x0 + dx * remaining, y0 + dy * remaining);
        }
        remaining -= seg;
    }
    points[points.len() - 1]
}

/// Midpoint of the polyline, used for the edge label.
pub fn midpoint(points: &[(i32, i32)]) -> (i32, i32) {
    point_at(points, 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_by_side_routes_horizontally() {
        let a = Rect { x: 0, y: 0, w: 10, h: 5 };
        let b = Rect { x: 20, y: 0, w: 10, h: 5 };
        let p = route(a, b);
        assert_eq!(p.first(), Some(&(9, 2)));
        assert_eq!(p.last(), Some(&(20, 2)));
        let cells = rasterize(&p, false);
        assert_eq!(cells.last().map(|c| c.ch), Some('▶'));
    }

    #[test]
    fn stacked_routes_vertically_with_bends() {
        let a = Rect { x: 0, y: 0, w: 10, h: 5 };
        let b = Rect { x: 4, y: 12, w: 10, h: 5 };
        let p = route(a, b);
        assert_eq!(p.len(), 4);
        assert_eq!(p[0], (5, 4));
        assert_eq!(p[3], (9, 12));
        let cells = rasterize(&p, true);
        assert!(cells.iter().any(|c| c.ch == '┗' || c.ch == '┓'));
        assert_eq!(cells.last().map(|c| c.ch), Some('▼'));
    }

    fn axis_aligned(p: &[(i32, i32)]) -> bool {
        p.windows(2).all(|w| w[0].0 == w[1].0 || w[0].1 == w[1].1)
    }

    #[test]
    fn detours_around_a_node_in_between_at_full_zoom() {
        // Full-zoom node size (32×9), three in a row: a → c → b.
        let a = Rect { x: 0, y: 0, w: 32, h: 9 };
        let c = Rect { x: 50, y: 0, w: 32, h: 9 };
        let b = Rect { x: 100, y: 0, w: 32, h: 9 };
        let direct = route(a, b);
        assert!(crosses(&direct, c), "sanity: the plain route runs through c");
        let p = route_avoiding(a, b, 0, 0, &[a, c, b]);
        assert!(!crosses(&p, c), "path {p:?} crosses c");
        assert!(!crosses(&p[..p.len() - 1], b) && !crosses(&p[1..], a));
        assert!(axis_aligned(&p));
        assert_eq!(p.first(), Some(&direct[0]));
        assert_eq!(p.last(), direct.last());
        let cells = rasterize(&p, false);
        assert_eq!(cells.last().map(|c| c.ch), Some('▶'));
    }

    #[test]
    fn detours_around_a_node_in_between_at_compact_zoom() {
        // Compact node size (22×3).
        let a = Rect { x: 0, y: 0, w: 22, h: 3 };
        let c = Rect { x: 30, y: 0, w: 22, h: 3 };
        let b = Rect { x: 60, y: 0, w: 22, h: 3 };
        assert!(crosses(&route(a, b), c));
        let p = route_avoiding(a, b, 0, 0, &[c]);
        assert!(!crosses(&p, c), "path {p:?} crosses c");
        assert!(axis_aligned(&p));
        assert_eq!(p.first(), Some(&(21, 1)));
        assert_eq!(p.last(), Some(&(60, 1)));
        // Vertical stack with a node in the way, compact size.
        let a = Rect { x: 0, y: 0, w: 22, h: 3 };
        let c = Rect { x: 0, y: 6, w: 22, h: 3 };
        let b = Rect { x: 0, y: 14, w: 22, h: 3 };
        assert!(crosses(&route(a, b), c));
        let p = route_avoiding(a, b, 0, 0, &[c]);
        assert!(!crosses(&p, c), "path {p:?} crosses c");
        assert!(axis_aligned(&p));
    }

    #[test]
    fn detours_when_the_blocker_touches_the_source() {
        // Compact demo layout: tester → ops with reviewer one cell to the right
        // of tester, so there is no air for the padded search; the unpadded
        // retry must still find a way around.
        let a = Rect { x: 1, y: 7, w: 22, h: 3 };
        let c = Rect { x: 24, y: 7, w: 22, h: 3 };
        let b = Rect { x: 75, y: 15, w: 22, h: 3 };
        assert!(crosses(&route(a, b), c));
        let p = route_avoiding(a, b, 0, 0, &[a, c, b]);
        assert!(!crosses(&p, c), "path {p:?} crosses c");
        assert!(axis_aligned(&p));
    }

    #[test]
    fn clear_route_is_left_alone() {
        let a = Rect { x: 0, y: 0, w: 10, h: 5 };
        let b = Rect { x: 20, y: 0, w: 10, h: 5 };
        let c = Rect { x: 0, y: 20, w: 10, h: 5 };
        assert_eq!(route_avoiding(a, b, 0, 0, &[c]), route(a, b));
    }

    #[test]
    fn pulse_walks_the_path() {
        let p = vec![(0, 0), (10, 0), (10, 10)];
        assert_eq!(point_at(&p, 0.0), (0, 0));
        assert_eq!(point_at(&p, 0.5), (10, 0));
        assert_eq!(point_at(&p, 1.0), (10, 10));
    }
}
