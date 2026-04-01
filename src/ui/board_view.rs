//! Renders the Catan hex board with shaped hex cells, buildings, roads, and cursor.

use std::collections::HashMap;

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders};

use crate::game::board::{
    self, EdgeCoord, EdgeDirection, HexCoord, Terrain, VertexCoord, VertexDirection,
};
use crate::game::state::{Building, GameState};

use super::{CursorKind, InputMode, PLAYER_COLORS};

// ── Layout constants ──────────────────────────────────────────────────

/// Horizontal distance between hex centers in the same row.
const HEX_COL_Q: i16 = 8;
/// Horizontal offset per r-row (half of HEX_COL_Q).
const HEX_COL_R: i16 = 4;
/// Vertical distance between hex row centers.
const HEX_ROW: i16 = 4;
/// Vertical offset from center to North/South vertex.
const VERT_OFF: i16 = 2;

// ── Terrain colors ───────────────────────────────────────────────────

fn terrain_color(t: Terrain) -> Color {
    match t {
        Terrain::Forest => Color::Rgb(34, 100, 34),
        Terrain::Hills => Color::Rgb(178, 102, 51),
        Terrain::Pasture => Color::Rgb(100, 180, 100),
        Terrain::Fields => Color::Rgb(200, 170, 50),
        Terrain::Mountains => Color::Rgb(120, 120, 140),
        Terrain::Desert => Color::Rgb(180, 160, 120),
    }
}

fn terrain_fg(_t: Terrain) -> Color {
    Color::White
}

// ── Probability dots ────────────────────────────────────────────────

fn probability_dots(number: u8) -> &'static str {
    match number {
        2 | 12 => "\u{00b7}",
        3 | 11 => "\u{00b7}\u{00b7}",
        4 | 10 => "\u{00b7}\u{00b7}\u{00b7}",
        5 | 9 => "\u{00b7}\u{00b7}\u{00b7}\u{00b7}",
        6 | 8 => "\u{00b7}\u{00b7}\u{00b7}\u{00b7}\u{00b7}",
        _ => "",
    }
}

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

        // Compute hex centers. The base offset ensures all coords are positive.
        // Min q=-2, min r=-2. With the formula col = q*HEX_COL_Q + r*HEX_COL_R,
        // min col = -2*8 + (-2)*4 = -24. So base_col = 26 to add margin.
        // Min row = -2*4 = -8. So base_row = 10.
        let base_col: i16 = 26;
        let base_row: i16 = 10;

        for &c in &coords {
            let cx = c.q as i16 * HEX_COL_Q + c.r as i16 * HEX_COL_R + base_col;
            let cy = c.r as i16 * HEX_ROW + base_row;
            hex_centers.insert(c, (cx, cy));
        }

        // Compute vertex positions. North(q,r) = top of hex (q,r).
        // In this flat-style layout, vertices v0 and v1 share the same row.
        // We compute position from the hex that "owns" the vertex.
        for &c in &coords {
            let (cx, cy) = hex_centers[&c];
            let north = VertexCoord::new(c, VertexDirection::North);
            let south = VertexCoord::new(c, VertexDirection::South);
            vertex_pos.entry(north).or_insert((cx, cy - VERT_OFF));
            vertex_pos.entry(south).or_insert((cx, cy + VERT_OFF));
        }

        // Compute edge midpoint positions (for cursor targeting and road display).
        for &c in &coords {
            let (cx, cy) = hex_centers[&c];
            // NE edge: from North(q,r) to South(q+1,r-1)
            // Midpoint between (cx, cy-2) and (cx+4, cy-2) = (cx+2, cy-2)
            let ne = EdgeCoord::new(c, EdgeDirection::NorthEast);
            edge_pos
                .entry(ne)
                .or_insert((cx + HEX_COL_R / 2 + 1, cy - VERT_OFF));

            // E edge: from South(q+1,r-1) to North(q,r+1)
            // Midpoint between (cx+4, cy-2) and (cx+4, cy+2) = (cx+4, cy)
            let e = EdgeCoord::new(c, EdgeDirection::East);
            edge_pos.entry(e).or_insert((cx + HEX_COL_R, cy));

            // SE edge: from South(q,r) to North(q,r+1)
            // Midpoint between (cx, cy+2) and (cx+4, cy+2) = (cx+2, cy+2)
            let se = EdgeCoord::new(c, EdgeDirection::SouthEast);
            edge_pos
                .entry(se)
                .or_insert((cx + HEX_COL_R / 2 + 1, cy + VERT_OFF));
        }

        // Compute bounding box.
        let max_col = hex_centers.values().map(|(c, _)| c + 5).max().unwrap_or(0);
        let max_row = hex_centers.values().map(|(_, r)| r + 3).max().unwrap_or(0);

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
    pub fn hex_center_pos(&self, h: &HexCoord) -> (u16, u16) {
        self.hex_centers
            .get(h)
            .map(|&(c, r)| (c.max(0) as u16, r.max(0) as u16))
            .unwrap_or((0, 0))
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
    // Draw border.
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

    // Layer 1: Draw hex cells (terrain + number).
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
            let scr_col = off_col as i16 + vx;
            let scr_row = off_row as i16 + vy;
            let (player_id, ch) = match building {
                Building::Settlement(p) => (*p, '\u{25b2}'), // ▲
                Building::City(p) => (*p, '\u{25a0}'),       // ■
            };
            let color = PLAYER_COLORS
                .get(player_id)
                .copied()
                .unwrap_or(Color::White);
            set_cell(
                scr_col,
                scr_row,
                ch,
                Style::default().fg(color).bold(),
                inner,
                buf,
            );
        }
    }

    // Layer 4: Draw ports.
    for port in &state.board.ports {
        for v in [&port.vertices.0, &port.vertices.1] {
            if let Some(&(vx, vy)) = grid.vertex_pos.get(v) {
                let scr_col = off_col as i16 + vx;
                let scr_row = off_row as i16 + vy;
                // Only draw port marker if no building is there.
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

    // Layer 5: Draw cursor overlay (legal positions + selected).
    draw_cursor_overlay(grid, off_col, off_row, inner, buf, input_mode);
}

/// Draw a single hex cell (terrain background + abbreviation + number).
fn draw_hex_cell(
    hex: &crate::game::board::Hex,
    state: &GameState,
    cx: i16,
    cy: i16,
    area: Rect,
    buf: &mut Buffer,
) {
    let bg = terrain_color(hex.terrain);
    let fg = terrain_fg(hex.terrain);
    let is_robber = state.robber_hex == hex.coord;

    // Draw a 7-wide, 3-tall colored block centered on (cx, cy).
    for dy in -1..=1i16 {
        for dx in -3..=3i16 {
            set_cell(
                cx + dx,
                cy + dy,
                ' ',
                Style::default().bg(if is_robber && dy == 0 { Color::Red } else { bg }),
                area,
                buf,
            );
        }
    }

    // Terrain abbreviation.
    let abbr = hex.terrain.abbr();
    let label = if is_robber {
        format!("R {}", abbr)
    } else {
        abbr.to_string()
    };

    let style = if is_robber {
        Style::default().fg(Color::White).bg(Color::Red).bold()
    } else {
        Style::default().fg(fg).bg(bg)
    };

    // Draw label at (cx-2, cy-1) or centered.
    for (i, ch) in label.chars().enumerate() {
        set_cell(cx - 2 + i as i16, cy - 1, ch, style, area, buf);
    }

    // Number token.
    if let Some(n) = hex.number_token {
        let num_str = format!("{:>2}", n);
        let is_hot = n == 6 || n == 8;
        let num_style = if is_hot {
            Style::default().fg(Color::Red).bg(bg).bold()
        } else {
            Style::default().fg(fg).bg(bg)
        };
        for (i, ch) in num_str.chars().enumerate() {
            set_cell(cx + 1 + i as i16, cy - 1, ch, num_style, area, buf);
        }

        // Probability dots on the line below.
        let dots = probability_dots(n);
        let dots_style = if is_hot {
            Style::default().fg(Color::Red).bg(bg).bold()
        } else {
            Style::default().fg(Color::DarkGray).bg(bg)
        };
        let dot_start = cx - (dots.chars().count() as i16) / 2;
        for (i, ch) in dots.chars().enumerate() {
            set_cell(dot_start + i as i16, cy, ch, dots_style, area, buf);
        }
    } else {
        // Desert: show "--"
        set_cell(cx + 1, cy - 1, '-', style, area, buf);
        set_cell(cx + 2, cy - 1, '-', style, area, buf);
    }
}

/// Draw a road segment at an edge midpoint.
fn draw_road_segment(
    dir: EdgeDirection,
    mx: i16,
    my: i16,
    color: Color,
    area: Rect,
    buf: &mut Buffer,
) {
    let style = Style::default().fg(color).bold();
    match dir {
        EdgeDirection::NorthEast | EdgeDirection::SouthEast => {
            // Horizontal-ish segment
            set_cell(mx - 1, my, '\u{2550}', style, area, buf); // ═
            set_cell(mx, my, '\u{2550}', style, area, buf);
            set_cell(mx + 1, my, '\u{2550}', style, area, buf);
        }
        EdgeDirection::East => {
            // Vertical segment
            set_cell(mx, my - 1, '\u{2551}', style, area, buf); // ║
            set_cell(mx, my, '\u{2551}', style, area, buf);
            set_cell(mx, my + 1, '\u{2551}', style, area, buf);
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
    if let InputMode::BoardCursor {
        kind,
        legal_vertices,
        legal_edges,
        legal_hexes,
        positions,
        selected,
    } = input_mode
    {
        let legal_style = Style::default().fg(Color::Yellow).bold();
        let cursor_style = Style::default().fg(Color::Black).bg(Color::Yellow).bold();

        match kind {
            CursorKind::Settlement => {
                for (i, v) in legal_vertices.iter().enumerate() {
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
                        }; // ◆ / ◇
                        set_cell(sx, sy, ch, style, area, buf);
                    }
                }
            }
            CursorKind::Road => {
                for (i, e) in legal_edges.iter().enumerate() {
                    if let Some(&(ex, ey)) = grid.edge_pos.get(e) {
                        let sx = off_col as i16 + ex;
                        let sy = off_row as i16 + ey;
                        let style = if i == *selected {
                            cursor_style
                        } else {
                            legal_style
                        };
                        set_cell(sx - 1, sy, '=', style, area, buf);
                        set_cell(sx, sy, '=', style, area, buf);
                        set_cell(sx + 1, sy, '=', style, area, buf);
                    }
                }
            }
            CursorKind::Robber => {
                for (i, h) in legal_hexes.iter().enumerate() {
                    let (hx, hy) = grid.hex_center_pos(h);
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

        // Draw position description for the selected cursor position.
        let _ = (positions, selected); // used for screen positions in navigation
    }
}

/// Safe cell setter: only writes if within the given area.
fn set_cell(col: i16, row: i16, ch: char, style: Style, area: Rect, buf: &mut Buffer) {
    let col = col as u16;
    let row = row as u16;
    if col >= area.x && col < area.x + area.width && row >= area.y && row < area.y + area.height {
        if let Some(cell) = buf.cell_mut(ratatui::layout::Position::new(col, row)) {
            cell.set_char(ch);
            cell.set_style(style);
        }
    }
}
