//! Renders the hex board with shaped hex cells, buildings, roads, and cursor.

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

/// Upper half-block for sub-pixel compositing.
const UPPER_HALF: char = '\u{2580}';
/// Lower half-block for sub-pixel compositing.
const LOWER_HALF: char = '\u{2584}';

/// Half-widths for the 7-row hex fill diamond (cy-3 to cy+3).
const HEX_FILL_HALF_WIDTHS: [i16; 7] = [4, 6, 7, 8, 7, 6, 4];

/// Darken a color to ~60% brightness for terrain-tinted hex edges.
fn darken_color(c: Color) -> Color {
    match c {
        Color::Rgb(r, g, b) => Color::Rgb(
            (r as u16 * 3 / 5) as u8,
            (g as u16 * 3 / 5) as u8,
            (b as u16 * 3 / 5) as u8,
        ),
        Color::Red => Color::Rgb(150, 0, 0),
        _ => Color::DarkGray,
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

// ── Sub-pixel canvas ────────────────────────────────────────────────

/// Sub-pixel canvas for half-block rendering.
/// Each terminal cell maps to two vertical sub-pixels, doubling vertical resolution.
struct SubPixelCanvas {
    width: usize,
    height: usize,
    pixels: Vec<Option<Color>>,
}

impl SubPixelCanvas {
    fn new(width: usize, term_height: usize) -> Self {
        let height = term_height * 2;
        Self {
            width,
            height,
            pixels: vec![None; width * height],
        }
    }

    fn set_pixel(&mut self, x: i16, y: i16, color: Color) {
        if x < 0 || y < 0 {
            return;
        }
        let (x, y) = (x as usize, y as usize);
        if x < self.width && y < self.height {
            self.pixels[y * self.width + x] = Some(color);
        }
    }

    fn fill_row(&mut self, y: i16, x_start: i16, x_end: i16, color: Color) {
        for x in x_start..=x_end {
            self.set_pixel(x, y, color);
        }
    }

    fn get(&self, x: usize, y: usize) -> Option<Color> {
        if x < self.width && y < self.height {
            self.pixels[y * self.width + x]
        } else {
            None
        }
    }

    fn composite(&self, off_col: u16, off_row: u16, area: Rect, buf: &mut Buffer) {
        let term_height = self.height.div_ceil(2);
        for ty in 0..term_height {
            for tx in 0..self.width {
                let top = self.get(tx, ty * 2);
                let bot = self.get(tx, ty * 2 + 1);
                let col = off_col as i16 + tx as i16;
                let row = off_row as i16 + ty as i16;
                match (top, bot) {
                    (None, None) => {}
                    (Some(t), Some(b)) if t == b => {
                        set_cell(col, row, ' ', Style::default().bg(t), area, buf);
                    }
                    (Some(t), Some(b)) => {
                        set_cell(
                            col,
                            row,
                            UPPER_HALF,
                            Style::default().fg(t).bg(b),
                            area,
                            buf,
                        );
                    }
                    (Some(t), None) => {
                        let bg = read_cell_bg(col, row, area, buf);
                        set_cell(
                            col,
                            row,
                            UPPER_HALF,
                            Style::default().fg(t).bg(bg),
                            area,
                            buf,
                        );
                    }
                    (None, Some(b)) => {
                        let bg = read_cell_bg(col, row, area, buf);
                        set_cell(
                            col,
                            row,
                            LOWER_HALF,
                            Style::default().fg(b).bg(bg),
                            area,
                            buf,
                        );
                    }
                }
            }
        }
    }
}

/// Draw a hex fill directly to the terminal buffer using full character cells.
/// Uses a 7-row diamond shape with widths tuned to leave a 2-row gap between
/// the fill edge and vertex positions (where settlements and roads live).
fn draw_hex_cell_fill(cx: i16, cy: i16, fill: Style, area: Rect, buf: &mut Buffer) {
    for (i, &hw) in HEX_FILL_HALF_WIDTHS.iter().enumerate() {
        let dy = i as i16 - 3;
        for dx in -hw..=hw {
            set_cell(cx + dx, cy + dy, ' ', fill, area, buf);
        }
    }
}

/// Draw diagonal edge outlines on a hex using ╱╲ line-drawing characters.
/// Creates terrain-tinted edges: darkened terrain foreground on terrain background.
fn draw_hex_edges(cx: i16, cy: i16, bg: Color, area: Rect, buf: &mut Buffer) {
    let fg = darken_color(bg);
    let style = Style::default().fg(fg).bg(bg);

    // Upper diagonals (cy-3 to cy-1): ╱ left, ╲ right
    for (i, &hw) in HEX_FILL_HALF_WIDTHS.iter().enumerate().take(3) {
        let dy = i as i16 - 3;
        set_cell(cx - hw, cy + dy, '╱', style, area, buf);
        set_cell(cx + hw, cy + dy, '╲', style, area, buf);
    }

    // Lower diagonals (cy+1 to cy+3): ╲ left, ╱ right
    for (i, &hw) in HEX_FILL_HALF_WIDTHS.iter().enumerate().skip(4) {
        let dy = i as i16 - 3;
        set_cell(cx - hw, cy + dy, '╲', style, area, buf);
        set_cell(cx + hw, cy + dy, '╱', style, area, buf);
    }
}

/// Draw a road segment into the sub-pixel canvas as a smooth diagonal line.
fn draw_road_subpixel(
    dir: EdgeDirection,
    mx: i16,
    my: i16,
    color: Color,
    canvas: &mut SubPixelCanvas,
) {
    let smy = my * 2;
    match dir {
        EdgeDirection::NorthEast => {
            // Smooth line from lower-left to upper-right, 2 sub-pixels wide.
            let offsets: [(i16, i16); 8] = [
                (-3, -4),
                (-3, -3),
                (-2, -2),
                (-1, -1),
                (-1, 0),
                (0, 1),
                (1, 2),
                (2, 3),
            ];
            for (dx, dy) in offsets {
                canvas.set_pixel(mx + dx, smy + dy, color);
                canvas.set_pixel(mx + dx + 1, smy + dy, color);
            }
        }
        EdgeDirection::SouthEast => {
            // Smooth line from upper-right to lower-left, 2 sub-pixels wide.
            let offsets: [(i16, i16); 8] = [
                (2, -2),
                (2, -1),
                (1, 0),
                (0, 1),
                (0, 2),
                (-1, 3),
                (-2, 4),
                (-3, 5),
            ];
            for (dx, dy) in offsets {
                canvas.set_pixel(mx + dx, smy + dy, color);
                canvas.set_pixel(mx + dx + 1, smy + dy, color);
            }
        }
        EdgeDirection::East => {
            // Vertical road: 2 wide, 10 sub-pixels tall (5 terminal rows).
            for spy in (smy - 4)..=(smy + 5) {
                canvas.set_pixel(mx - 1, spy, color);
                canvas.set_pixel(mx, spy, color);
            }
        }
    }
}

/// Draw a building background shape into the sub-pixel canvas.
fn draw_building_subpixel(
    building: &Building,
    sx: i16,
    sy: i16,
    color: Color,
    canvas: &mut SubPixelCanvas,
) {
    let ssy = sy * 2;
    match building {
        Building::Settlement(_) => {
            // 5w x 2h block (rows sy-1 and sy).
            canvas.fill_row(ssy - 2, sx - 2, sx + 2, color);
            canvas.fill_row(ssy - 1, sx - 2, sx + 2, color);
            canvas.fill_row(ssy, sx - 2, sx + 2, color);
            canvas.fill_row(ssy + 1, sx - 2, sx + 2, color);
        }
        Building::City(_) => {
            // 5w x 3h block (rows sy-1, sy, sy+1).
            canvas.fill_row(ssy - 2, sx - 2, sx + 2, color);
            canvas.fill_row(ssy - 1, sx - 2, sx + 2, color);
            canvas.fill_row(ssy, sx - 2, sx + 2, color);
            canvas.fill_row(ssy + 1, sx - 2, sx + 2, color);
            canvas.fill_row(ssy + 2, sx - 2, sx + 2, color);
            canvas.fill_row(ssy + 3, sx - 2, sx + 2, color);
        }
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

    // Layer 1: Draw hex fills directly to buffer (clean full-char cells, no fringe).
    for hex in &state.board.hexes {
        if let Some(&(cx, cy)) = grid.hex_centers.get(&hex.coord) {
            let scr_col = off_col as i16 + cx;
            let scr_row = off_row as i16 + cy;
            let is_robber = state.robber_hex == hex.coord;
            let fill_bg = if is_robber {
                Color::Red
            } else {
                terrain_color(hex.terrain)
            };
            draw_hex_cell_fill(scr_col, scr_row, Style::default().bg(fill_bg), inner, buf);
            draw_hex_edges(scr_col, scr_row, fill_bg, inner, buf);
        }
    }

    // Layer 2a: Draw roads to sub-pixel canvas.
    let mut canvas = SubPixelCanvas::new(board_w as usize, board_h as usize);
    for (&edge, &player_id) in &state.roads {
        if let Some(&(ex, ey)) = grid.edge_pos.get(&edge) {
            let color = PLAYER_COLORS
                .get(player_id)
                .copied()
                .unwrap_or(Color::White);
            draw_road_subpixel(edge.dir, ex, ey, color, &mut canvas);
        }
    }

    // Layer 2b: Draw building backgrounds to sub-pixel canvas.
    for (vertex, building) in &state.buildings {
        if let Some(&(vx, vy)) = grid.vertex_pos.get(vertex) {
            let player_id = match building {
                Building::Settlement(p) | Building::City(p) => *p,
            };
            let color = PLAYER_COLORS
                .get(player_id)
                .copied()
                .unwrap_or(Color::White);
            draw_building_subpixel(building, vx, vy, color, &mut canvas);
        }
    }

    // Layer 3: Composite sub-pixel canvas to terminal buffer.
    // Background-aware: half-block edges blend with underlying hex fill colors.
    canvas.composite(off_col, off_row, inner, buf);

    // Layer 4: Draw hex text overlays (terrain labels, numbers).
    for hex in &state.board.hexes {
        if let Some(&(cx, cy)) = grid.hex_centers.get(&hex.coord) {
            let scr_col = off_col as i16 + cx;
            let scr_row = off_row as i16 + cy;
            draw_hex_text(hex, state, scr_col, scr_row, inner, buf);
        }
    }

    // Layer 5: Draw building symbols.
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
            let sym_style = Style::default().fg(Color::White).bg(color).bold();
            match building {
                Building::Settlement(_) => {
                    set_cell(sx, sy - 1, '\u{25b2}', sym_style, inner, buf);
                }
                Building::City(_) => {
                    set_cell(sx, sy, '\u{25a0}', sym_style, inner, buf);
                }
            }
        }
    }

    // Layer 6: Draw ports.
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

    // Layer 7: Draw cursor overlay.
    draw_cursor_overlay(grid, off_col, off_row, inner, buf, input_mode);
}

/// Draw text overlays (terrain label and number token) for a single hex.
fn draw_hex_text(
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

    // Number token on row cy.
    if let Some(n) = hex.number_token {
        let num_str = format!("{:>2}", n);
        let num_style = Style::default().fg(fg).bg(fill_bg);
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
                    // 3-wide marker for hex cursor to match settlement/road visual weight.
                    set_cell(sx - 1, sy, '[', style, area, buf);
                    set_cell(sx, sy, 'R', style, area, buf);
                    set_cell(sx + 1, sy, ']', style, area, buf);
                }
            }
        }
    }
}

/// Read the background color of an existing buffer cell.
/// Used by composite() to blend half-block edges with underlying hex fills.
fn read_cell_bg(col: i16, row: i16, area: Rect, buf: &Buffer) -> Color {
    if col < 0 || row < 0 {
        return Color::Reset;
    }
    let col = col as u16;
    let row = row as u16;
    if col >= area.x && col < area.x + area.width && row >= area.y && row < area.y + area.height {
        buf.cell(ratatui::layout::Position::new(col, row))
            .map(|c| c.bg)
            .unwrap_or(Color::Reset)
    } else {
        Color::Reset
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_buf(w: u16, h: u16) -> (Rect, Buffer) {
        let area = Rect::new(0, 0, w, h);
        (area, Buffer::empty(area))
    }

    #[test]
    fn subpixel_canvas_same_color_produces_space() {
        let mut canvas = SubPixelCanvas::new(3, 2);
        canvas.set_pixel(1, 0, Color::Red);
        canvas.set_pixel(1, 1, Color::Red);
        let (area, mut buf) = make_buf(3, 2);
        canvas.composite(0, 0, area, &mut buf);
        let cell = buf.cell(ratatui::layout::Position::new(1, 0)).unwrap();
        assert_eq!(cell.symbol(), " ");
        assert_eq!(cell.bg, Color::Red);
    }

    #[test]
    fn subpixel_canvas_different_colors_produces_upper_half() {
        let mut canvas = SubPixelCanvas::new(3, 2);
        canvas.set_pixel(1, 0, Color::Red);
        canvas.set_pixel(1, 1, Color::Blue);
        let (area, mut buf) = make_buf(3, 2);
        canvas.composite(0, 0, area, &mut buf);
        let cell = buf.cell(ratatui::layout::Position::new(1, 0)).unwrap();
        assert_eq!(cell.symbol(), "\u{2580}");
        assert_eq!(cell.fg, Color::Red);
        assert_eq!(cell.bg, Color::Blue);
    }

    #[test]
    fn subpixel_canvas_top_only_blends_with_existing_bg() {
        let mut canvas = SubPixelCanvas::new(3, 2);
        canvas.set_pixel(1, 0, Color::Green);
        let (area, mut buf) = make_buf(3, 2);
        // Pre-fill cell with a terrain background (simulating hex fill underneath)
        buf.cell_mut(ratatui::layout::Position::new(1, 0))
            .unwrap()
            .set_style(Style::default().bg(Color::Rgb(34, 120, 34)));
        canvas.composite(0, 0, area, &mut buf);
        let cell = buf.cell(ratatui::layout::Position::new(1, 0)).unwrap();
        assert_eq!(cell.symbol(), "\u{2580}");
        assert_eq!(cell.fg, Color::Green);
        assert_eq!(cell.bg, Color::Rgb(34, 120, 34));
    }

    #[test]
    fn subpixel_canvas_bottom_only_blends_with_existing_bg() {
        let mut canvas = SubPixelCanvas::new(3, 2);
        canvas.set_pixel(1, 1, Color::Yellow);
        let (area, mut buf) = make_buf(3, 2);
        buf.cell_mut(ratatui::layout::Position::new(1, 0))
            .unwrap()
            .set_style(Style::default().bg(Color::Rgb(80, 180, 60)));
        canvas.composite(0, 0, area, &mut buf);
        let cell = buf.cell(ratatui::layout::Position::new(1, 0)).unwrap();
        assert_eq!(cell.symbol(), "\u{2584}");
        assert_eq!(cell.fg, Color::Yellow);
        assert_eq!(cell.bg, Color::Rgb(80, 180, 60));
    }

    #[test]
    fn subpixel_canvas_empty_cell_untouched() {
        let canvas = SubPixelCanvas::new(3, 2);
        let (area, mut buf) = make_buf(3, 2);
        canvas.composite(0, 0, area, &mut buf);
        let cell = buf.cell(ratatui::layout::Position::new(1, 0)).unwrap();
        assert_eq!(cell.symbol(), " ");
        assert_eq!(cell.fg, Color::Reset);
    }

    #[test]
    fn hex_cell_fill_covers_7_rows() {
        let (area, mut buf) = make_buf(30, 12);
        let fill = Style::default().bg(Color::Green);
        draw_hex_cell_fill(15, 5, fill, area, &mut buf);
        // Row cy-3 (row 2): half-width 4 -> 9 chars
        assert_eq!(
            buf.cell(ratatui::layout::Position::new(15, 2)).unwrap().bg,
            Color::Green
        );
        // Row cy-4 (row 1): outside fill
        assert_eq!(
            buf.cell(ratatui::layout::Position::new(15, 1)).unwrap().bg,
            Color::Reset
        );
        // Row cy (row 5): half-width 8 -> 17 chars (cx-8 to cx+8 = 7..23)
        assert_eq!(
            buf.cell(ratatui::layout::Position::new(7, 5)).unwrap().bg,
            Color::Green
        );
        assert_eq!(
            buf.cell(ratatui::layout::Position::new(23, 5)).unwrap().bg,
            Color::Green
        );
        // Outside fill horizontally: should be default
        assert_eq!(
            buf.cell(ratatui::layout::Position::new(6, 5)).unwrap().bg,
            Color::Reset
        );
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
