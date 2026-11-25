//! Canvas renderer: dotted grid, routed edges with pulses, agent nodes with
//! live previews, guest cursors. Pure function of (Workspace, Camera).

use hubcodes_core::model::{Agent, AgentId, Status, Workspace, NODE_H, NODE_H_COMPACT, NODE_W, NODE_W_COMPACT};
use hubcodes_core::providers::Registry;
use crate::routing::{self, Rect};
use crate::theme;
use ansi_to_tui::IntoText;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect as URect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Widget};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zoom {
    Full,
    Compact,
}

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub ox: i32,
    pub oy: i32,
    pub zoom: Zoom,
}

impl Default for Camera {
    fn default() -> Self {
        Camera {
            ox: 0,
            oy: 0,
            zoom: Zoom::Full,
        }
    }
}

impl Camera {
    pub fn node_rect(&self, a: &Agent) -> Rect {
        match self.zoom {
            Zoom::Full => Rect {
                x: a.x - self.ox,
                y: a.y - self.oy,
                w: NODE_W,
                h: NODE_H,
            },
            Zoom::Compact => Rect {
                x: a.x / 2 - self.ox,
                y: a.y / 2 - self.oy,
                w: NODE_W_COMPACT,
                h: NODE_H_COMPACT,
            },
        }
    }

    /// Screen → canvas conversion for mouse handling.
    pub fn to_canvas(self, sx: i32, sy: i32) -> (i32, i32) {
        match self.zoom {
            Zoom::Full => (sx + self.ox, sy + self.oy),
            Zoom::Compact => ((sx + self.ox) * 2, (sy + self.oy) * 2),
        }
    }

    pub fn toggle_zoom(&mut self) {
        self.zoom = match self.zoom {
            Zoom::Full => Zoom::Compact,
            Zoom::Compact => Zoom::Full,
        };
    }
}

pub struct CanvasView<'a> {
    pub ws: &'a Workspace,
    pub cam: Camera,
    pub reg: &'a Registry,
    pub selected: Option<AgentId>,
    /// Agents marked for a group message (double border).
    pub marked: &'a std::collections::BTreeSet<AgentId>,
    /// Box select in progress, screen coordinates relative to the canvas area.
    pub box_sel: Option<((i32, i32), (i32, i32))>,
    pub connect_from: Option<AgentId>,
    /// Composer text of the selected node ("" when empty).
    pub draft: &'a str,
    pub now: Instant,
    pub frame: u64,
}

impl CanvasView<'_> {
    pub fn hit_test(ws: &Workspace, cam: Camera, area: URect, sx: u16, sy: u16) -> Option<AgentId> {
        let px = sx as i32 - area.x as i32;
        let py = sy as i32 - area.y as i32;
        // Later nodes are drawn on top, so test in reverse.
        ws.agents
            .iter()
            .rev()
            .find(|a| cam.node_rect(a).contains(px, py))
            .map(|a| a.id)
    }
}

fn put(buf: &mut Buffer, area: URect, x: i32, y: i32, ch: char, style: Style) {
    if x < 0 || y < 0 || x >= area.width as i32 || y >= area.height as i32 {
        return;
    }
    let cell = &mut buf[(area.x + x as u16, area.y + y as u16)];
    cell.set_char(ch);
    cell.set_style(style);
}

fn put_str(buf: &mut Buffer, area: URect, x: i32, y: i32, s: &str, style: Style) {
    for (i, ch) in s.chars().enumerate() {
        put(buf, area, x + i as i32, y, ch, style);
    }
}

impl Widget for CanvasView<'_> {
    fn render(self, area: URect, buf: &mut Buffer) {
        let ws = self.ws;
        let cam = self.cam;

        // 1. Background grid, offset by the camera so it pans with the canvas.
        for y in 0..area.height as i32 {
            for x in 0..area.width as i32 {
                let cx = x + cam.ox;
                let cy = y + cam.oy;
                if cx.rem_euclid(theme::GRID_STEP_X) == 0 && cy.rem_euclid(theme::GRID_STEP_Y) == 0 {
                    put(buf, area, x, y, '·', theme::grid());
                } else {
                    put(buf, area, x, y, ' ', Style::default());
                }
            }
        }

        // 2. Edges. First assign a port slot per (node, side) so parallel
        // edges leaving the same side get their own row/column, then draw
        // lines, then labels and pulses on top.
        let mut slots: std::collections::HashMap<(AgentId, routing::Dir), Vec<usize>> = Default::default();
        let mut edge_sides: Vec<Option<(routing::Dir, routing::Dir)>> = Vec::with_capacity(ws.edges.len());
        for (i, e) in ws.edges.iter().enumerate() {
            let (Some(a), Some(b)) = (ws.agent(e.from), ws.agent(e.to)) else {
                edge_sides.push(None);
                continue;
            };
            let (sa, sb) = routing::sides(cam.node_rect(a), cam.node_rect(b));
            slots.entry((e.from, sa)).or_default().push(i);
            slots.entry((e.to, sb)).or_default().push(i);
            edge_sides.push(Some((sa, sb)));
        }
        let offset_for = |node: AgentId, side: routing::Dir, edge_idx: usize| -> i32 {
            let v = &slots[&(node, side)];
            let n = v.len() as i32;
            let k = v.iter().position(|&x| x == edge_idx).unwrap_or(0) as i32;
            let step = if matches!(side, routing::Dir::Left | routing::Dir::Right) { 2 } else { 4 };
            (k - (n - 1) / 2) * step - if n % 2 == 0 { step / 2 } else { 0 }
        };
        let all_rects: Vec<Rect> = ws.agents.iter().map(|a| cam.node_rect(a)).collect();
        let mut routed: Vec<(usize, Vec<(i32, i32)>)> = Vec::new();
        for (i, e) in ws.edges.iter().enumerate() {
            let (Some(a), Some(b)) = (ws.agent(e.from), ws.agent(e.to)) else {
                continue;
            };
            let Some((sa, sb)) = edge_sides[i] else { continue };
            let ra = cam.node_rect(a);
            let rb = cam.node_rect(b);
            let points = routing::route_avoiding(
                ra,
                rb,
                offset_for(e.from, sa, i),
                offset_for(e.to, sb, i),
                &all_rects,
            );
            routed.push((i, points));
        }
        for (i, points) in &routed {
            let e = &ws.edges[*i];
            let active = e
                .last_traffic
                .map(|t| self.now.duration_since(t).as_secs() < 8)
                .unwrap_or(false)
                || !e.pulses.is_empty();
            let involved = self.selected == Some(e.from) || self.selected == Some(e.to);
            let heavy = active || involved;
            let mut style = theme::edge(active);
            if involved {
                style = style.add_modifier(Modifier::BOLD);
            }
            for c in routing::rasterize(points, heavy) {
                put(buf, area, c.x, c.y, c.ch, style);
            }
        }
        for (i, points) in &routed {
            let e = &ws.edges[*i];
            if cam.zoom == Zoom::Full && !e.label.is_empty() {
                let (mx, my) = routing::midpoint(points);
                let label = format!(" {} ", e.label);
                put_str(buf, area, mx - label.chars().count() as i32 / 2, my, &label, theme::edge_label());
            }
        }
        for (i, points) in &routed {
            for p in &ws.edges[*i].pulses {
                let t = if p.reverse { 1.0 - p.progress } else { p.progress };
                let (px, py) = routing::point_at(points, t);
                put(buf, area, px, py, '●', theme::pulse());
            }
        }

        // 3. Nodes.
        for a in &ws.agents {
            let r = cam.node_rect(a);
            let screen = URect {
                x: (area.x as i32 + r.x).max(area.x as i32) as u16,
                y: (area.y as i32 + r.y).max(area.y as i32) as u16,
                width: 0,
                height: 0,
            };
            // Clip to the canvas area.
            let x0 = area.x as i32 + r.x;
            let y0 = area.y as i32 + r.y;
            let x1 = (x0 + r.w).min(area.right() as i32);
            let y1 = (y0 + r.h).min(area.bottom() as i32);
            if x1 <= area.x as i32 || y1 <= area.y as i32 || x0 >= area.right() as i32 || y0 >= area.bottom() as i32 {
                continue;
            }
            let clipped = URect {
                x: screen.x,
                y: screen.y,
                width: (x1 - screen.x as i32).max(0) as u16,
                height: (y1 - screen.y as i32).max(0) as u16,
            };
            if clipped.width < 4 || clipped.height < 2 {
                continue;
            }
            let fully_visible = x0 >= area.x as i32 && y0 >= area.y as i32;
            // Nodes sit above the grid and the edges: blank the footprint first.
            for yy in clipped.y..clipped.bottom() {
                for xx in clipped.x..clipped.right() {
                    let cell = &mut buf[(xx, yy)];
                    cell.set_char(' ');
                    cell.set_style(Style::default());
                }
            }
            render_node(self.ws, self.reg, a, clipped, buf, &self, fully_visible);
        }

        // 4. Connect-mode hint on the source node.
        if let Some(from) = self.connect_from {
            if let Some(a) = ws.agent(from) {
                let r = cam.node_rect(a);
                put_str(buf, area, r.x + 1, r.bottom() - 1, " pick target → ", theme::accent());
            }
        }

        // 5. Off-screen agents that need you: a marker on the nearest edge.
        let (w, h) = (area.width as i32, area.height as i32);
        let mut used: Vec<Rect> = Vec::new();
        for a in ws.agents.iter().filter(|a| a.status == Status::Waiting) {
            let Some((dir, x, y)) = edge_marker(cam.node_rect(a), w, h) else { continue };
            let label = match dir {
                routing::Dir::Left => format!("◀ ! {} ", a.name),
                routing::Dir::Right => format!(" ! {} ▶", a.name),
                routing::Dir::Up => format!("▲ ! {} ", a.name),
                routing::Dir::Down => format!("▼ ! {} ", a.name),
            };
            let lw = label.chars().count() as i32;
            let (mut mx, mut my) = match dir {
                routing::Dir::Left => (0, y),
                routing::Dir::Right => (w - lw, y),
                routing::Dir::Up => ((x - lw / 2).clamp(0, (w - lw).max(0)), 0),
                routing::Dir::Down => ((x - lw / 2).clamp(0, (w - lw).max(0)), h - 1),
            };
            // Nudge away from markers already placed on this edge.
            let mut slot = Rect { x: mx, y: my, w: lw, h: 1 };
            let mut tries = 0;
            while used.iter().any(|u| overlaps_rect(*u, slot)) && tries < 8 {
                match dir {
                    routing::Dir::Left | routing::Dir::Right => my += 1,
                    _ => mx += lw,
                }
                slot = Rect { x: mx, y: my, w: lw, h: 1 };
                tries += 1;
            }
            used.push(slot);
            put_str(buf, area, mx, my, &label, theme::needs_you_marker());
        }
        if let Some(((x0, y0), (x1, y1))) = self.box_sel {
            let (bx0, by0) = (area.x as i32 + x0.min(x1), area.y as i32 + y0.min(y1));
            let (bx1, by1) = (area.x as i32 + x0.max(x1), area.y as i32 + y0.max(y1));
            let style = theme::node_border(true, Color::Indexed(75));
            for x in bx0..=bx1 {
                for y in [by0, by1] {
                    if x >= area.x as i32 && x < area.right() as i32 && y >= area.y as i32 && y < area.bottom() as i32 {
                        buf[(x as u16, y as u16)].set_symbol("╌").set_style(style);
                    }
                }
            }
            for y in by0..=by1 {
                for x in [bx0, bx1] {
                    if x >= area.x as i32 && x < area.right() as i32 && y >= area.y as i32 && y < area.bottom() as i32 {
                        buf[(x as u16, y as u16)].set_symbol("╎").set_style(style);
                    }
                }
            }
        }
    }
}

fn overlaps_rect(p: Rect, q: Rect) -> bool {
    p.x < q.right() && q.x < p.right() && p.y < q.bottom() && q.y < p.bottom()
}

fn render_node(
    ws: &Workspace,
    reg: &Registry,
    a: &Agent,
    rect: URect,
    buf: &mut Buffer,
    view: &CanvasView,
    full: bool,
) {
    let provider_color = reg.get(&a.provider).and_then(|p| p.color_index()).map(Color::Indexed).unwrap_or(Color::Gray);
    let glyph = reg.get(&a.provider).map(|p| p.glyph.clone()).unwrap_or_default();
    let selected = view.selected == Some(a.id);
    let guests: Vec<_> = ws
        .participants
        .iter()
        .filter(|p| !p.is_me && p.selected == Some(a.id))
        .collect();

    let mut border = theme::node_border(selected, provider_color);
    if let Some(g) = guests.first() {
        if !selected {
            border = Style::default().fg(theme::guest(g.color_idx));
        }
    }
    let marked = view.marked.contains(&a.id);
    if marked && !selected {
        border = theme::node_border(true, provider_color);
    }
    let border_type = if selected { BorderType::Thick } else if marked { BorderType::Double } else { BorderType::Rounded };

    let status_style = theme::status(a.status);
    let blink = a.status == Status::Working && view.frame % 10 < 5;
    let status_glyph = if blink { "◉" } else { a.status.glyph() };

    let mut title = vec![
        Span::styled(format!(" {} ", status_glyph), status_style),
        Span::styled(a.name.clone(), theme::node_title(provider_color)),
        Span::styled(format!(" · {}{} ", glyph, a.provider), theme::dim()),
    ];
    if let Some(role) = &a.role {
        title.push(Span::styled(format!("[{role}] "), theme::dim()));
    }

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(border)
        .title(Line::from(title));

    if let Some(g) = guests.first() {
        let tag = format!(" {} ", g.name);
        block = block.title_top(
            Line::from(Span::styled(
                tag,
                Style::default()
                    .fg(Color::Indexed(232))
                    .bg(theme::guest(g.color_idx))
                    .add_modifier(Modifier::BOLD),
            ))
            .right_aligned(),
        );
    }

    if view.cam.zoom == Zoom::Full && full {
        let footer = Span::styled(format!(" {} ", a.status.label()), status_style);
        block = block.title_bottom(Line::from(footer).right_aligned());
        let n = ws.neighbours(a.id).len();
        if n > 0 {
            block = block.title_bottom(Line::from(Span::styled(format!(" ⇄ {n} "), theme::dim())));
        }
        if a.open {
            block = block.title_top(Line::from(Span::styled(" ▣ open ", theme::accent())).right_aligned());
        }
    }

    let inner = block.inner(rect);
    block.render(rect, buf);

    if view.cam.zoom == Zoom::Compact || inner.height == 0 {
        return;
    }

    // Selected node: the last inner rows are the composer (type-first
    // input). It grows with the draft, up to COMPOSER_MAX_LINES, and keeps
    // at least one preview row.
    let composer = selected && inner.height >= 3;
    let mut preview_area = inner;
    if composer {
        let room = (inner.width as usize).saturating_sub(3).max(1);
        let lines = wrap_draft(view.draft, room);
        let max = (COMPOSER_MAX_LINES as u16).min(inner.height - 1);
        let rows = (lines.len() as u16).clamp(1, max);
        let area = URect { y: inner.y + inner.height - rows, height: rows, ..inner };
        preview_area = URect { height: inner.height - rows, ..inner };
        let text: Vec<Line> = if view.draft.is_empty() {
            vec![Line::from(vec![
                Span::styled("› ", theme::accent()),
                Span::styled("type here · Enter = focus", theme::dim()),
            ])]
        } else {
            composer_lines(&lines, rows as usize)
        };
        Paragraph::new(text).render(area, buf);
    }
    let inner = preview_area;

    // Preview: last `inner.height` lines, ANSI-styled, truncated to width.
    let take = inner.height as usize;
    let start = a.preview.len().saturating_sub(take);
    let mut lines: Vec<Line> = Vec::with_capacity(take);
    for raw in &a.preview[start..] {
        let text = raw.as_bytes().into_text().ok();
        let line = text
            .and_then(|t| t.lines.into_iter().next())
            .unwrap_or_else(|| Line::from(strip_ansi(raw)));
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled("(no output yet)", theme::dim())));
    }
    Paragraph::new(lines).style(theme::text()).render(inner, buf);
}

/// Most rows the in-node composer may take.
pub const COMPOSER_MAX_LINES: usize = 6;

/// Split a draft into display rows: hard newlines first, then wrap each line
/// at `width` cells. A trailing newline yields an empty last row so the
/// cursor lands on the new line.
pub fn wrap_draft(draft: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows: Vec<String> = Vec::new();
    for line in draft.split('\n') {
        let chars: Vec<char> = line.chars().collect();
        if chars.is_empty() {
            rows.push(String::new());
            continue;
        }
        for chunk in chars.chunks(width) {
            rows.push(chunk.iter().collect());
        }
    }
    if rows.is_empty() {
        rows.push(String::new());
    }
    rows
}

/// Render the last `rows` wrapped draft rows: `› ` on the first, two spaces
/// of indent on the rest, cursor after the final one.
pub fn composer_lines(wrapped: &[String], rows: usize) -> Vec<Line<'static>> {
    let start = wrapped.len().saturating_sub(rows);
    let mut out: Vec<Line> = Vec::with_capacity(rows);
    for (i, row) in wrapped[start..].iter().enumerate() {
        let prefix = if start + i == 0 { "› " } else { "  " };
        let mut spans = vec![
            Span::styled(prefix, theme::accent()),
            Span::styled(row.clone(), theme::text().add_modifier(Modifier::BOLD)),
        ];
        if start + i == wrapped.len() - 1 {
            spans.push(Span::styled("▏", theme::accent()));
        }
        out.push(Line::from(spans));
    }
    out
}

/// Which edge of the viewport is closest to an off-screen node, and where
/// along it the marker goes. Returns `None` when the node intersects the
/// viewport (it is at least partly visible).
pub fn edge_marker(r: Rect, w: i32, h: i32) -> Option<(routing::Dir, i32, i32)> {
    if r.right() > 0 && r.x < w && r.bottom() > 0 && r.y < h {
        return None;
    }
    let cx = r.cx();
    let cy = r.cy();
    let dir = if r.right() <= 0 {
        routing::Dir::Left
    } else if r.x >= w {
        routing::Dir::Right
    } else if r.bottom() <= 0 {
        routing::Dir::Up
    } else {
        routing::Dir::Down
    };
    let x = cx.clamp(0, (w - 1).max(0));
    let y = cy.clamp(0, (h - 1).max(0));
    Some((dir, x, y))
}

pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for n in chars.by_ref() {
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}
