//! Renders the Catan hex board with shaped hex cells, buildings, roads, and cursor.

use std::collections::HashMap;

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders};

use crate::game::board::{self, EdgeCoord, EdgeDirection, HexCoord, Terrain, VertexCoord};
use crate::game::state::{Building, GameState};

use super::{CursorLegal, InputMode, PLAYER_COLORS};

// ── Layout constants ──────────────────────────────────────────────────

/// Horizontal distance between hex centers in the same row.
const HEX_COL_Q: i16 = 20;
/// Horizontal offset per r-row (half of HEX_COL_Q).
const HEX_COL_R: i16 = 10;
/// Vertical distance between hex row centers.
const HEX_ROW: i16 = 9;
/// Vertical offset from center to North/South vertex.
const VERT_OFF: i16 = 5;

// ── Terrain colors ───────────────────────────────────────────────────

fn terrain_color(t: Terrain) -> Color {
    match t {
        Terrain::Forest => Color::Rgb(34, 120, 34),
        Terrain::Hills => Color::Rgb(178, 102, 51),
        Terrain::Pasture => Color::Rgb(80, 180, 60),
        Terrain::Fields => Color::Rgb(200, 170, 50),
        Terrain::Mountains => Color::Rgb(140, 140, 150),
        Terrain::Desert => Color::Rgb(180, 160, 120),
    }
}

/// Foreground color for text rendered on terrain fill.
const TERRAIN_FG: Color = Color::White;

// ── HexGrid ─────────────────────────────────────────────────────────

/// Precomputed screen positions for all hex elements.
pub struct HexGrid {
    /// Hex center positions (col, row) in board-local coordinates.
    hex_centers: HashMap<HexCoord, (i16, i16)>,
    /// Vertex screen positions.
    vertex_pos: HashMap<VertexCoord, (i16, i16)>,
    /// Edge midpoint screen positions (for cursor targeting).
    edge_pos: HashMap<EdgeCoord, (i16, i16)>,
    /// Board-local bounding box.
    pub width: u16,
    pub height: u16,
}

impl Default for HexGrid {
    fn default() -> Self {
        Self::new()
    }
}

impl HexGrid {
    pub fn new() -> Self {
        let coords = board::board_hex_coords();
        let mut hex_centers = HashMap::new();
        let mut vertex_pos: HashMap<VertexCoord, (i16, i16)> = HashMap::new();
        let mut edge_pos: HashMap<EdgeCoord, (i16, i16)> = HashMap::new();

        let base_col: i16 = 52;
        let base_row: i16 = 25;

        for &c in &coords {
            let cx = c.q as i16 * HEX_COL_Q + c.r as i16 * HEX_COL_R + base_col;
            let cy = c.r as i16 * HEX_ROW + base_row;
            hex_centers.insert(c, (cx, cy));
        }

        // Compute ALL 6 vertex positions per hex.
        let side_dy = HEX_ROW - VERT_OFF;
        for &c in &coords {
            let (cx, cy) = hex_centers[&c];
            let verts = board::hex_vertices(c);
            vertex_pos.entry(verts[0]).or_insert((cx, cy - VERT_OFF));
            vertex_pos
                .entry(verts[1])
                .or_insert((cx + HEX_COL_R, cy - side_dy));
            vertex_pos
                .entry(verts[2])
                .or_insert((cx + HEX_COL_R, cy + side_dy));
            vertex_pos.entry(verts[3]).or_insert((cx, cy + VERT_OFF));
            vertex_pos
                .entry(verts[4])
                .or_insert((cx - HEX_COL_R, cy + side_dy));
            vertex_pos
                .entry(verts[5])
                .or_insert((cx - HEX_COL_R, cy - side_dy));
        }

        // Compute ALL 6 edge midpoint positions per hex.
        let half_r = HEX_COL_R / 2;
        let edge_dy = VERT_OFF - 1;
        for &c in &coords {
            let (cx, cy) = hex_centers[&c];
            let edges = board::hex_edges(c);
            edge_pos
                .entry(edges[0])
                .or_insert((cx + half_r, cy - edge_dy));
            edge_pos.entry(edges[1]).or_insert((cx + HEX_COL_R, cy));
            edge_pos
                .entry(edges[2])
                .or_insert((cx + half_r, cy + edge_dy));
            edge_pos
                .entry(edges[3])
                .or_insert((cx - half_r, cy + edge_dy));
            edge_pos.entry(edges[4]).or_insert((cx - HEX_COL_R, cy));
            edge_pos
                .entry(edges[5])
                .or_insert((cx - half_r, cy - edge_dy));
        }

        let max_col = hex_centers
            .values()
            .map(|(c, _)| c + HEX_COL_R + 1)
            .max()
            .unwrap_or(0);
        let max_row = hex_centers
            .values()
            .map(|(_, r)| r + VERT_OFF + 1)
            .max()
            .unwrap_or(0);

        HexGrid {
            hex_centers,
            vertex_pos,
            edge_pos,
            width: (max_col + 2) as u16,
            height: (max_row + 2) as u16,
        }
    }

    /// Get screen position of a vertex (board-local).
    pub fn vertex_screen_pos(&self, v: &VertexCoord) -> Option<(u16, u16)> {
        self.vertex_pos
            .get(v)
            .map(|&(c, r)| (c.max(0) as u16, r.max(0) as u16))
    }

    /// Get screen position of an edge midpoint (board-local).
    pub fn edge_screen_pos(&self, e: &EdgeCoord) -> Option<(u16, u16)> {
        self.edge_pos
            .get(e)
            .map(|&(c, r)| (c.max(0) as u16, r.max(0) as u16))
    }

    /// Get screen position of a hex center (board-local).
    pub fn hex_center_pos(&self, h: &HexCoord) -> Option<(u16, u16)> {
        self.hex_centers
            .get(h)
            .map(|&(c, r)| (c.max(0) as u16, r.max(0) as u16))
    }
}

// ── Rendering ───────────────────────────────────────────────────────

/// Render the hex board with terrain, buildings, roads, robber, and cursor.
pub fn render_board(
    state: &GameState,
    grid: &HexGrid,
    area: Rect,
    buf: &mut Buffer,
    input_mode: &InputMode,
) {
    let block = Block::default()
        .title(" Board ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    block.render(area, buf);

    if inner.width < 10 || inner.height < 5 {
        return;
    }

    // Offset to center the board in the available area.
    let board_w = grid.width;
    let board_h = grid.height;
    let off_col = inner.x + inner.width.saturating_sub(board_w) / 2;
    let off_row = inner.y + inner.height.saturating_sub(board_h) / 2;

    // Layer 1: Draw hex cells (terrain + number + probability dots).
    for hex in &state.board.hexes {
        if let Some(&(cx, cy)) = grid.hex_centers.get(&hex.coord) {
            let scr_col = off_col as i16 + cx;
            let scr_row = off_row as i16 + cy;
            draw_hex_cell(hex, state, scr_col, scr_row, inner, buf);
        }
    }

    // Layer 2: Draw roads.
    for (&edge, &player_id) in &state.roads {
        if let Some(&(ex, ey)) = grid.edge_pos.get(&edge) {
            let scr_col = off_col as i16 + ex;
            let scr_row = off_row as i16 + ey;
            let color = PLAYER_COLORS
                .get(player_id)
                .copied()
                .unwrap_or(Color::White);
            draw_road_segment(edge.dir, scr_col, scr_row, color, inner, buf);
        }
    }

    // Layer 3: Draw buildings.
    for (vertex, building) in &state.buildings {
        if let Some(&(vx, vy)) = grid.vertex_pos.get(vertex) {
            let sx = off_col as i16 + vx;
            let sy = off_row as i16 + vy;
            let player_id = match building {
                Building::Settlement(p) | Building::City(p) => *p,
            };
            let color = PLAYER_COLORS
                .get(player_id)
                .copied()
                .unwrap_or(Color::White);
            let bg_style = Style::default().bg(color);
            let sym_style = Style::default().fg(Color::White).bg(color).bold();
            match building {
                Building::Settlement(_) => {
                    for dx in -2..=2i16 {
                        set_cell(sx + dx, sy - 1, ' ', bg_style, inner, buf);
                        set_cell(sx + dx, sy, ' ', bg_style, inner, buf);
                    }
                    set_cell(sx, sy - 1, '\u{25b2}', sym_style, inner, buf);
                }
                Building::City(_) => {
                    for dx in -2..=2i16 {
                        set_cell(sx + dx, sy - 1, ' ', bg_style, inner, buf);
                        set_cell(sx + dx, sy, ' ', bg_style, inner, buf);
                        set_cell(sx + dx, sy + 1, ' ', bg_style, inner, buf);
                    }
                    set_cell(sx, sy, '\u{25a0}', sym_style, inner, buf);
                }
            }
        }
    }

    // Layer 4: Draw ports.
    for port in &state.board.ports {
        for v in [&port.vertices.0, &port.vertices.1] {
            if let Some(&(vx, vy)) = grid.vertex_pos.get(v) {
                let scr_col = off_col as i16 + vx;
                let scr_row = off_row as i16 + vy;
                if !state.buildings.contains_key(v) {
                    set_cell(
                        scr_col,
                        scr_row,
                        '*',
                        Style::default().fg(Color::Yellow),
                        inner,
                        buf,
                    );
                }
            }
        }
    }

    // Layer 5: Draw cursor overlay.
    draw_cursor_overlay(grid, off_col, off_row, inner, buf, input_mode);
}

/// Draw a single hex cell with terrain fill, label, number token, and probability dots.
fn draw_hex_cell(
    hex: &crate::game::board::Hex,
    state: &GameState,
    cx: i16,
    cy: i16,
    area: Rect,
    buf: &mut Buffer,
) {
    let bg = terrain_color(hex.terrain);
    let fg = TERRAIN_FG;
    let is_robber = state.robber_hex == hex.coord;
    let fill_bg = if is_robber { Color::Red } else { bg };
    let fill = Style::default().bg(fill_bg);

    // Rows cy-3 through cy-1: expanding fill.
    for dx in -3..=3i16 {
        set_cell(cx + dx, cy - 3, ' ', fill, area, buf);
    }
    for dx in -5..=5i16 {
        set_cell(cx + dx, cy - 2, ' ', fill, area, buf);
    }
    for dx in -7..=7i16 {
        set_cell(cx + dx, cy - 1, ' ', fill, area, buf);
    }

    // Terrain label on row cy-1.
    let label = hex.terrain.label();
    let text_style = if is_robber {
        Style::default().fg(Color::White).bg(Color::Red).bold()
    } else {
        Style::default().fg(fg).bg(fill_bg)
    };

    if is_robber {
        set_cell(cx - 5, cy - 1, 'R', text_style, area, buf);
        let label_start = cx - 3;
        for (i, ch) in label.chars().enumerate() {
            set_cell(label_start + i as i16, cy - 1, ch, text_style, area, buf);
        }
    } else {
        let label_start = cx - (label.len() as i16) / 2;
        for (i, ch) in label.chars().enumerate() {
            set_cell(label_start + i as i16, cy - 1, ch, text_style, area, buf);
        }
    }

    // Row cy: widest row with number token.
    for dx in -8..=8i16 {
        set_cell(cx + dx, cy, ' ', fill, area, buf);
    }

    if let Some(n) = hex.number_token {
        let is_hot = n == 6 || n == 8;
        let num_str = format!("{:>2}", n);
        let num_style = if is_hot && is_robber {
            Style::default().fg(Color::White).bg(fill_bg).bold()
        } else if is_hot {
            Style::default().fg(Color::Red).bg(fill_bg).bold()
        } else {
            Style::default().fg(fg).bg(fill_bg)
        };
        for (i, ch) in num_str.chars().enumerate() {
            set_cell(cx - 1 + i as i16, cy, ch, num_style, area, buf);
        }
    } else if is_robber {
        let robber_style = Style::default().fg(Color::White).bg(Color::Red).bold();
        set_cell(cx, cy, 'R', robber_style, area, buf);
    } else {
        set_cell(cx - 1, cy, '-', text_style, area, buf);
        set_cell(cx, cy, '-', text_style, area, buf);
    }

    // Rows cy+1 through cy+3: contracting fill.
    for dx in -7..=7i16 {
        set_cell(cx + dx, cy + 1, ' ', fill, area, buf);
    }
    for dx in -5..=5i16 {
        set_cell(cx + dx, cy + 2, ' ', fill, area, buf);
    }
    for dx in -3..=3i16 {
        set_cell(cx + dx, cy + 3, ' ', fill, area, buf);
    }

    // Probability dots on row cy+1 (per DESIGN.md).
    if let Some(n) = hex.number_token {
        let is_hot = n == 6 || n == 8;
        let dots = probability_dots(n);
        if dots > 0 {
            let dot_style = if is_hot {
                Style::default().fg(Color::Red).bg(fill_bg).bold()
            } else {
                Style::default().fg(Color::DarkGray).bg(fill_bg)
            };
            let start = cx - (dots as i16 - 1);
            for d in 0..dots as i16 {
                set_cell(start + d * 2, cy + 1, '\u{00b7}', dot_style, area, buf);
            }
        }
    }
}

/// Number of probability dots for a given number token.
fn probability_dots(n: u8) -> u8 {
    match n {
        2 | 12 => 1,
        3 | 11 => 2,
        4 | 10 => 3,
        5 | 9 => 4,
        6 | 8 => 5,
        _ => 0,
    }
}

/// Draw a road segment as a colored block between vertices.
fn draw_road_segment(
    dir: EdgeDirection,
    mx: i16,
    my: i16,
    color: Color,
    area: Rect,
    buf: &mut Buffer,
) {
    let style = Style::default().bg(color);
    match dir {
        EdgeDirection::NorthEast => {
            set_cell(mx - 3, my - 2, ' ', style, area, buf);
            set_cell(mx - 2, my - 2, ' ', style, area, buf);
            set_cell(mx - 2, my - 1, ' ', style, area, buf);
            set_cell(mx - 1, my - 1, ' ', style, area, buf);
            set_cell(mx, my, ' ', style, area, buf);
            set_cell(mx + 1, my, ' ', style, area, buf);
            set_cell(mx + 1, my + 1, ' ', style, area, buf);
            set_cell(mx + 2, my + 1, ' ', style, area, buf);
        }
        EdgeDirection::SouthEast => {
            set_cell(mx - 3, my + 2, ' ', style, area, buf);
            set_cell(mx - 2, my + 2, ' ', style, area, buf);
            set_cell(mx - 2, my + 1, ' ', style, area, buf);
            set_cell(mx - 1, my + 1, ' ', style, area, buf);
            set_cell(mx, my, ' ', style, area, buf);
            set_cell(mx + 1, my, ' ', style, area, buf);
            set_cell(mx + 1, my - 1, ' ', style, area, buf);
            set_cell(mx + 2, my - 1, ' ', style, area, buf);
        }
        EdgeDirection::East => {
            for dy in -2..=2i16 {
                set_cell(mx - 1, my + dy, ' ', style, area, buf);
                set_cell(mx, my + dy, ' ', style, area, buf);
            }
        }
    }
}

/// Draw cursor overlay highlighting legal positions and selected position.
fn draw_cursor_overlay(
    grid: &HexGrid,
    off_col: u16,
    off_row: u16,
    area: Rect,
    buf: &mut Buffer,
    input_mode: &InputMode,
) {
    let InputMode::BoardCursor {
        legal, selected, ..
    } = input_mode
    else {
        return;
    };

    let legal_style = Style::default().fg(Color::Yellow).bold();
    let cursor_style = Style::default().fg(Color::Black).bg(Color::Yellow).bold();

    match legal {
        CursorLegal::Settlements(verts) => {
            for (i, v) in verts.iter().enumerate() {
                if let Some(&(vx, vy)) = grid.vertex_pos.get(v) {
                    let sx = off_col as i16 + vx;
                    let sy = off_row as i16 + vy;
                    let style = if i == *selected {
                        cursor_style
                    } else {
                        legal_style
                    };
                    let ch = if i == *selected {
                        '\u{25c6}'
                    } else {
                        '\u{25c7}'
                    };
                    set_cell(sx, sy, ch, style, area, buf);
                }
            }
        }
        CursorLegal::Roads(edges) => {
            for (i, e) in edges.iter().enumerate() {
                if let Some(&(ex, ey)) = grid.edge_pos.get(e) {
                    let sx = off_col as i16 + ex;
                    let sy = off_row as i16 + ey;
                    let style = if i == *selected {
                        cursor_style
                    } else {
                        legal_style
                    };
                    match e.dir {
                        EdgeDirection::NorthEast => {
                            set_cell(sx - 1, sy - 1, '=', style, area, buf);
                            set_cell(sx, sy, '=', style, area, buf);
                            set_cell(sx + 1, sy + 1, '=', style, area, buf);
                        }
                        EdgeDirection::SouthEast => {
                            set_cell(sx - 1, sy + 1, '=', style, area, buf);
                            set_cell(sx, sy, '=', style, area, buf);
                            set_cell(sx + 1, sy - 1, '=', style, area, buf);
                        }
                        EdgeDirection::East => {
                            for dy in -2..=2i16 {
                                set_cell(sx, sy + dy, '=', style, area, buf);
                            }
                        }
                    }
                }
            }
        }
        CursorLegal::Hexes(hexes) => {
            for (i, h) in hexes.iter().enumerate() {
                if let Some((hx, hy)) = grid.hex_center_pos(h) {
                    let sx = off_col as i16 + hx as i16;
                    let sy = off_row as i16 + hy as i16;
                    let style = if i == *selected {
                        cursor_style
                    } else {
                        legal_style
                    };
                    set_cell(sx, sy, 'R', style, area, buf);
                }
            }
        }
    }
}

/// Safe cell setter: only writes if within the given area.
fn set_cell(col: i16, row: i16, ch: char, style: Style, area: Rect, buf: &mut Buffer) {
    if col < 0 || row < 0 {
        return;
    }
    let col = col as u16;
    let row = row as u16;
    if col >= area.x && col < area.x + area.width && row >= area.y && row < area.y + area.height {
        if let Some(cell) = buf.cell_mut(ratatui::layout::Position::new(col, row)) {
            cell.set_char(ch);
            cell.set_style(style);
        }
    }
}
