//! Pixel-rendered board using `ratatui-image`.
//!
//! Generates an offscreen `RgbaImage` of the hex board, then renders it
//! into the board panel via `StatefulImage`. The terminal must support a
//! pixel graphics protocol (Kitty, Sixel, or iTerm2).

use std::collections::HashMap;

use ab_glyph::{FontRef, PxScale};
use image::{DynamicImage, Rgba, RgbaImage};
use imageproc::drawing::{
    draw_filled_circle_mut, draw_filled_rect_mut, draw_line_segment_mut, draw_polygon_mut,
    draw_text_mut,
};
use imageproc::point::Point;
use imageproc::rect::Rect as ImgRect;
use ratatui::prelude::*;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::{Resize, StatefulImage};

use crate::game::board::{self, EdgeDirection, HexCoord, Terrain, VertexCoord};
use crate::game::state::{Building, GameState};

use super::{CursorKind, InputMode, PLAYER_COLORS};

// ── Image dimensions ─────────────────────────────────────────────────

const IMG_WIDTH: u32 = 750;
const IMG_HEIGHT: u32 = 700;

// ── Pixel hex geometry ───────────────────────────────────────────────

/// Horizontal pixel distance between hex centers per q-unit.
const PX_HEX_Q: f64 = 130.0;
/// Horizontal pixel offset per r-row.
const PX_HEX_R: f64 = 65.0;
/// Vertical pixel distance between hex row centers.
const PX_HEX_ROW: f64 = 116.0;
/// Vertical offset from center to N/S vertex (pointy-top hex).
const PX_VERT_OFF: f64 = 68.0;
/// Horizontal offset from center to NE/SE vertex.
const PX_SIDE_X: f64 = 65.0;
/// Vertical offset from center to NE/SE vertex.
const PX_SIDE_Y: f64 = 34.0;

/// Base pixel offset so the board is centered in the image.
const BASE_PX_X: f64 = 375.0;
const BASE_PX_Y: f64 = 350.0;

// ── Colors ───────────────────────────────────────────────────────────

const BG_COLOR: Rgba<u8> = Rgba([20, 20, 30, 255]);

fn terrain_rgba(t: Terrain) -> Rgba<u8> {
    match t {
        Terrain::Forest => Rgba([34, 120, 34, 255]),
        Terrain::Hills => Rgba([178, 102, 51, 255]),
        Terrain::Pasture => Rgba([80, 180, 60, 255]),
        Terrain::Fields => Rgba([200, 170, 50, 255]),
        Terrain::Mountains => Rgba([140, 140, 150, 255]),
        Terrain::Desert => Rgba([180, 160, 120, 255]),
    }
}

fn player_rgba(player_id: usize) -> Rgba<u8> {
    // Match ratatui PLAYER_COLORS but as RGBA.
    match PLAYER_COLORS.get(player_id) {
        Some(Color::LightRed) => Rgba([255, 100, 100, 255]),
        Some(Color::LightBlue) => Rgba([100, 150, 255, 255]),
        Some(Color::LightGreen) => Rgba([100, 255, 100, 255]),
        Some(Color::LightMagenta) => Rgba([255, 100, 255, 255]),
        _ => Rgba([255, 255, 255, 255]),
    }
}

// ── PixelHexGrid ─────────────────────────────────────────────────────

/// Maps axial hex coordinates to pixel positions in the board image.
pub struct PixelHexGrid {
    hex_centers: HashMap<HexCoord, (f64, f64)>,
    vertex_pos: HashMap<VertexCoord, (f64, f64)>,
    #[allow(dead_code)]
    edge_midpoints: HashMap<board::EdgeCoord, (f64, f64)>,
}

impl PixelHexGrid {
    fn new() -> Self {
        let coords = board::board_hex_coords();
        let mut hex_centers = HashMap::new();
        let mut vertex_pos: HashMap<VertexCoord, (f64, f64)> = HashMap::new();
        let mut edge_midpoints: HashMap<board::EdgeCoord, (f64, f64)> = HashMap::new();

        for &c in &coords {
            let cx = c.q as f64 * PX_HEX_Q + c.r as f64 * PX_HEX_R + BASE_PX_X;
            let cy = c.r as f64 * PX_HEX_ROW + BASE_PX_Y;
            hex_centers.insert(c, (cx, cy));
        }

        // Compute vertex pixel positions (6 per hex, dedup via entry).
        for &c in &coords {
            let (cx, cy) = hex_centers[&c];
            let verts = board::hex_vertices(c);
            vertex_pos.entry(verts[0]).or_insert((cx, cy - PX_VERT_OFF)); // N
            vertex_pos
                .entry(verts[1])
                .or_insert((cx + PX_SIDE_X, cy - PX_SIDE_Y)); // NE
            vertex_pos
                .entry(verts[2])
                .or_insert((cx + PX_SIDE_X, cy + PX_SIDE_Y)); // SE
            vertex_pos.entry(verts[3]).or_insert((cx, cy + PX_VERT_OFF)); // S
            vertex_pos
                .entry(verts[4])
                .or_insert((cx - PX_SIDE_X, cy + PX_SIDE_Y)); // SW
            vertex_pos
                .entry(verts[5])
                .or_insert((cx - PX_SIDE_X, cy - PX_SIDE_Y)); // NW
        }

        // Compute edge midpoint positions (6 per hex, dedup).
        for &c in &coords {
            let (cx, cy) = hex_centers[&c];
            let edges = board::hex_edges(c);
            // NE edge: midpoint between N and NE vertex
            edge_midpoints
                .entry(edges[0])
                .or_insert((cx + PX_SIDE_X / 2.0, cy - (PX_VERT_OFF + PX_SIDE_Y) / 2.0));
            // E edge: midpoint between NE and SE vertex
            edge_midpoints
                .entry(edges[1])
                .or_insert((cx + PX_SIDE_X, cy));
            // SE edge: midpoint between SE and S vertex
            edge_midpoints
                .entry(edges[2])
                .or_insert((cx + PX_SIDE_X / 2.0, cy + (PX_VERT_OFF + PX_SIDE_Y) / 2.0));
            // SW edge
            edge_midpoints
                .entry(edges[3])
                .or_insert((cx - PX_SIDE_X / 2.0, cy + (PX_VERT_OFF + PX_SIDE_Y) / 2.0));
            // W edge
            edge_midpoints
                .entry(edges[4])
                .or_insert((cx - PX_SIDE_X, cy));
            // NW edge
            edge_midpoints
                .entry(edges[5])
                .or_insert((cx - PX_SIDE_X / 2.0, cy - (PX_VERT_OFF + PX_SIDE_Y) / 2.0));
        }

        PixelHexGrid {
            hex_centers,
            vertex_pos,
            edge_midpoints,
        }
    }

    /// Get the 6 vertex pixel positions for a hex (clockwise from N).
    #[allow(dead_code)]
    fn hex_vertex_pixels(&self, h: HexCoord) -> [Point<i32>; 6] {
        let verts = board::hex_vertices(h);
        let mut pts = [Point::new(0, 0); 6];
        for (i, v) in verts.iter().enumerate() {
            if let Some(&(x, y)) = self.vertex_pos.get(v) {
                pts[i] = Point::new(x as i32, y as i32);
            }
        }
        pts
    }
}

// ── BoardImageRenderer ───────────────────────────────────────────────

pub struct BoardImageRenderer {
    protocol: Option<StatefulProtocol>,
    base_image: Option<RgbaImage>,
    board_fingerprint: u64,
    cursor_fingerprint: u64,
    last_area: (u16, u16),
    pixel_grid: PixelHexGrid,
    font: FontRef<'static>,
    font_bold: FontRef<'static>,
}

impl BoardImageRenderer {
    pub fn new(picker: &Picker) -> Self {
        let font_data: &'static [u8] = include_bytes!("../../assets/DejaVuSansMono.ttf");
        let font_bold_data: &'static [u8] = include_bytes!("../../assets/DejaVuSansMono-Bold.ttf");
        let font = FontRef::try_from_slice(font_data).expect("valid font");
        let font_bold = FontRef::try_from_slice(font_bold_data).expect("valid bold font");

        // Create initial empty protocol with a blank image.
        let blank = RgbaImage::from_pixel(IMG_WIDTH, IMG_HEIGHT, BG_COLOR);
        let protocol = picker.new_resize_protocol(DynamicImage::ImageRgba8(blank));

        BoardImageRenderer {
            protocol: Some(protocol),
            base_image: None,
            board_fingerprint: 0,
            cursor_fingerprint: 0,
            last_area: (0, 0),
            pixel_grid: PixelHexGrid::new(),
            font,
            font_bold,
        }
    }

    /// Create a renderer without a graphics protocol (for tests and image generation).
    #[allow(dead_code)]
    pub fn new_test() -> Self {
        let font_data: &'static [u8] = include_bytes!("../../assets/DejaVuSansMono.ttf");
        let font_bold_data: &'static [u8] = include_bytes!("../../assets/DejaVuSansMono-Bold.ttf");
        let font = FontRef::try_from_slice(font_data).expect("valid font");
        let font_bold = FontRef::try_from_slice(font_bold_data).expect("valid bold font");

        BoardImageRenderer {
            protocol: None,
            base_image: None,
            board_fingerprint: 0,
            cursor_fingerprint: 0,
            last_area: (0, 0),
            pixel_grid: PixelHexGrid::new(),
            font,
            font_bold,
        }
    }

    pub fn render(
        &mut self,
        f: &mut Frame,
        state: &GameState,
        picker: &Picker,
        area: Rect,
        input_mode: &InputMode,
    ) {
        if area.width < 5 || area.height < 3 {
            return;
        }

        // Check cache: regenerate base image if board state changed.
        let board_fp = compute_fingerprint(state);
        let board_changed = board_fp != self.board_fingerprint || self.base_image.is_none();
        if board_changed {
            let img = self.generate_base_image(state);
            self.base_image = Some(img);
            self.board_fingerprint = board_fp;
        }

        // Check if cursor state changed.
        let cursor_fp = cursor_fingerprint(input_mode);
        let cursor_changed = cursor_fp != self.cursor_fingerprint;

        // Check if area size changed (terminal resize).
        let area_key = (area.width, area.height);
        let area_changed = area_key != self.last_area;

        // Only rebuild the protocol when something actually changed.
        let needs_rebuild =
            board_changed || cursor_changed || area_changed || self.protocol.is_none();

        if needs_rebuild {
            self.cursor_fingerprint = cursor_fp;
            self.last_area = area_key;

            // Composite cursor overlay if in BoardCursor mode.
            let final_image = if matches!(input_mode, InputMode::BoardCursor { .. }) {
                let mut img = self.base_image.as_ref().unwrap().clone();
                self.draw_cursor_overlay(&mut img, input_mode);
                img
            } else {
                self.base_image.as_ref().unwrap().clone()
            };

            let protocol = picker.new_resize_protocol(DynamicImage::ImageRgba8(final_image));
            self.protocol = Some(protocol);
            log::debug!("board image protocol rebuilt (board={board_changed}, cursor={cursor_changed}, area={area_changed})");
        }

        if let Some(ref mut proto) = self.protocol {
            let widget = StatefulImage::default().resize(Resize::Scale(None));
            f.render_stateful_widget(widget, area, proto);
            // Log any encoding errors for debugging.
            if let Some(Err(e)) = proto.last_encoding_result() {
                log::error!("ratatui-image encoding error: {:?}", e);
            }
        }
    }

    fn generate_base_image(&self, state: &GameState) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(IMG_WIDTH, IMG_HEIGHT, BG_COLOR);

        // Layer 0: Ocean background behind all hexes.
        let ocean_color = Rgba([30, 80, 140, 255]);
        for hex in &state.board.hexes {
            let (cx, cy) = self.pixel_grid.hex_centers[&hex.coord];
            // Draw an expanded hex for the ocean fill (fills gaps between hexes).
            let expand = 8.0;
            let ocean_pts = [
                Point::new(cx as i32, (cy - PX_VERT_OFF - expand) as i32),
                Point::new(
                    (cx + PX_SIDE_X + expand * 0.87) as i32,
                    (cy - PX_SIDE_Y - expand * 0.5) as i32,
                ),
                Point::new(
                    (cx + PX_SIDE_X + expand * 0.87) as i32,
                    (cy + PX_SIDE_Y + expand * 0.5) as i32,
                ),
                Point::new(cx as i32, (cy + PX_VERT_OFF + expand) as i32),
                Point::new(
                    (cx - PX_SIDE_X - expand * 0.87) as i32,
                    (cy + PX_SIDE_Y + expand * 0.5) as i32,
                ),
                Point::new(
                    (cx - PX_SIDE_X - expand * 0.87) as i32,
                    (cy - PX_SIDE_Y - expand * 0.5) as i32,
                ),
            ];
            draw_polygon_mut(&mut img, &ocean_pts, ocean_color);
        }

        // Layer 1: Hex tiles (terrain fill, slightly expanded to close gaps).
        for hex in &state.board.hexes {
            let (cx, cy) = self.pixel_grid.hex_centers[&hex.coord];
            let grow = 2.0; // slight expansion to eliminate inter-hex gaps
            let pts = [
                Point::new(cx as i32, (cy - PX_VERT_OFF - grow) as i32),
                Point::new(
                    (cx + PX_SIDE_X + grow * 0.87) as i32,
                    (cy - PX_SIDE_Y - grow * 0.5) as i32,
                ),
                Point::new(
                    (cx + PX_SIDE_X + grow * 0.87) as i32,
                    (cy + PX_SIDE_Y + grow * 0.5) as i32,
                ),
                Point::new(cx as i32, (cy + PX_VERT_OFF + grow) as i32),
                Point::new(
                    (cx - PX_SIDE_X - grow * 0.87) as i32,
                    (cy + PX_SIDE_Y + grow * 0.5) as i32,
                ),
                Point::new(
                    (cx - PX_SIDE_X - grow * 0.87) as i32,
                    (cy - PX_SIDE_Y - grow * 0.5) as i32,
                ),
            ];
            let color = if state.robber_hex == hex.coord {
                Rgba([180, 40, 40, 255])
            } else {
                terrain_rgba(hex.terrain)
            };
            draw_polygon_mut(&mut img, &pts, color);

            // Terrain label.
            let (cx, cy) = self.pixel_grid.hex_centers[&hex.coord];
            let label = hex.terrain.label();
            let scale = PxScale::from(22.0);
            let label_w = label.len() as f64 * 13.0;
            draw_text_mut(
                &mut img,
                Rgba([255, 255, 255, 255]),
                (cx - label_w / 2.0) as i32,
                (cy - 36.0) as i32,
                scale,
                &self.font,
                label,
            );

            // Number token.
            if let Some(n) = hex.number_token {
                let is_hot = n == 6 || n == 8;
                let num_str = format!("{}", n);
                let num_scale = PxScale::from(28.0);
                let num_w = num_str.len() as f64 * 16.0;

                // Number token background circle.
                let token_color = Rgba([240, 230, 210, 255]);
                draw_filled_circle_mut(&mut img, (cx as i32, cy as i32), 18, token_color);

                let text_color = if is_hot {
                    Rgba([220, 30, 30, 255])
                } else {
                    Rgba([40, 40, 40, 255])
                };
                let font_for_num = if is_hot { &self.font_bold } else { &self.font };
                draw_text_mut(
                    &mut img,
                    text_color,
                    (cx - num_w / 2.0) as i32,
                    (cy - 13.0) as i32,
                    num_scale,
                    font_for_num,
                    &num_str,
                );

                // Probability dots below number.
                let dots = probability_dots(n);
                if dots > 0 {
                    let dot_y = (cy + 18.0) as i32;
                    let dot_start_x = cx - (dots as f64 - 1.0) * 4.0;
                    let dot_color = if is_hot {
                        Rgba([220, 30, 30, 255])
                    } else {
                        Rgba([200, 190, 170, 255])
                    };
                    for d in 0..dots {
                        let dx = (dot_start_x + d as f64 * 8.0) as i32;
                        draw_filled_circle_mut(&mut img, (dx, dot_y), 2, dot_color);
                    }
                }
            } else if state.robber_hex == hex.coord {
                // Desert with robber: draw R.
                let r_scale = PxScale::from(28.0);
                draw_text_mut(
                    &mut img,
                    Rgba([255, 255, 255, 255]),
                    (cx - 8.0) as i32,
                    (cy - 13.0) as i32,
                    r_scale,
                    &self.font_bold,
                    "R",
                );
            }

            // Robber overlay text.
            if state.robber_hex == hex.coord && hex.number_token.is_some() {
                let r_scale = PxScale::from(18.0);
                draw_text_mut(
                    &mut img,
                    Rgba([255, 200, 200, 255]),
                    (cx - 36.0) as i32,
                    (cy - 52.0) as i32,
                    r_scale,
                    &self.font_bold,
                    "ROBBER",
                );
            }
        }

        // Layer 2: Roads.
        for (&edge, &player_id) in &state.roads {
            let color = player_rgba(player_id);
            // Get the two vertex endpoints of this edge.
            if let Some(((x1, y1), (x2, y2))) = edge_vertex_pixels(&self.pixel_grid, edge) {
                draw_thick_line(
                    &mut img,
                    (x1 as f32, y1 as f32),
                    (x2 as f32, y2 as f32),
                    color,
                    5,
                );
            }
        }

        // Layer 3: Buildings.
        for (vertex, building) in &state.buildings {
            if let Some(&(vx, vy)) = self.pixel_grid.vertex_pos.get(vertex) {
                let player_id = match building {
                    Building::Settlement(p) | Building::City(p) => *p,
                };
                let color = player_rgba(player_id);
                match building {
                    Building::Settlement(_) => {
                        draw_settlement(&mut img, vx as i32, vy as i32, color);
                    }
                    Building::City(_) => {
                        draw_city(&mut img, vx as i32, vy as i32, color);
                    }
                }
            }
        }

        // Layer 4: Ports.
        for port in &state.board.ports {
            for v in [&port.vertices.0, &port.vertices.1] {
                if !state.buildings.contains_key(v) {
                    if let Some(&(vx, vy)) = self.pixel_grid.vertex_pos.get(v) {
                        let port_color = Rgba([255, 220, 60, 255]);
                        draw_filled_circle_mut(&mut img, (vx as i32, vy as i32), 6, port_color);
                        // Outline.
                        draw_circle_outline(
                            &mut img,
                            vx as i32,
                            vy as i32,
                            7,
                            Rgba([180, 150, 30, 255]),
                        );
                    }
                }
            }
        }

        img
    }

    fn draw_cursor_overlay(&self, img: &mut RgbaImage, input_mode: &InputMode) {
        if let InputMode::BoardCursor {
            kind,
            legal_vertices,
            legal_edges,
            legal_hexes,
            selected,
            ..
        } = input_mode
        {
            let legal_color = Rgba([255, 255, 0, 200]);
            let cursor_color = Rgba([255, 255, 0, 255]);

            match kind {
                CursorKind::Settlement => {
                    for (i, v) in legal_vertices.iter().enumerate() {
                        if let Some(&(vx, vy)) = self.pixel_grid.vertex_pos.get(v) {
                            if i == *selected {
                                draw_filled_circle_mut(
                                    img,
                                    (vx as i32, vy as i32),
                                    12,
                                    cursor_color,
                                );
                                // Inner diamond.
                                let diamond = [
                                    Point::new(vx as i32, vy as i32 - 8),
                                    Point::new(vx as i32 + 8, vy as i32),
                                    Point::new(vx as i32, vy as i32 + 8),
                                    Point::new(vx as i32 - 8, vy as i32),
                                ];
                                draw_polygon_mut(img, &diamond, Rgba([40, 40, 40, 255]));
                            } else {
                                draw_circle_outline(img, vx as i32, vy as i32, 8, legal_color);
                            }
                        }
                    }
                }
                CursorKind::Road => {
                    for (i, e) in legal_edges.iter().enumerate() {
                        if let Some(((x1, y1), (x2, y2))) = edge_vertex_pixels(&self.pixel_grid, *e)
                        {
                            let color = if i == *selected {
                                cursor_color
                            } else {
                                legal_color
                            };
                            let thickness = if i == *selected { 7 } else { 4 };
                            draw_thick_line(
                                img,
                                (x1 as f32, y1 as f32),
                                (x2 as f32, y2 as f32),
                                color,
                                thickness,
                            );
                        }
                    }
                }
                CursorKind::Robber => {
                    for (i, h) in legal_hexes.iter().enumerate() {
                        if let Some(&(cx, cy)) = self.pixel_grid.hex_centers.get(h) {
                            if i == *selected {
                                draw_filled_circle_mut(
                                    img,
                                    (cx as i32, cy as i32),
                                    20,
                                    cursor_color,
                                );
                                let r_scale = PxScale::from(24.0);
                                draw_text_mut(
                                    img,
                                    Rgba([40, 40, 40, 255]),
                                    (cx - 7.0) as i32,
                                    (cy - 11.0) as i32,
                                    r_scale,
                                    &self.font_bold,
                                    "R",
                                );
                            } else {
                                draw_circle_outline(img, cx as i32, cy as i32, 16, legal_color);
                                let r_scale = PxScale::from(20.0);
                                draw_text_mut(
                                    img,
                                    legal_color,
                                    (cx - 6.0) as i32,
                                    (cy - 9.0) as i32,
                                    r_scale,
                                    &self.font_bold,
                                    "R",
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

fn compute_fingerprint(state: &GameState) -> u64 {
    let mut fp: u64 = 0;
    fp = fp.wrapping_add(state.buildings.len() as u64 * 1000003);
    fp = fp.wrapping_add(state.roads.len() as u64 * 1000033);
    fp = fp.wrapping_add(state.robber_hex.q as u64 * 100 + state.robber_hex.r as u64);
    fp = fp.wrapping_add(state.turn_number as u64 * 17);
    fp
}

fn cursor_fingerprint(input_mode: &InputMode) -> u64 {
    match input_mode {
        InputMode::BoardCursor {
            selected,
            positions,
            ..
        } => {
            // Hash selected index and number of positions.
            let mut fp = *selected as u64 * 1000007;
            fp = fp.wrapping_add(positions.len() as u64 * 31);
            fp
        }
        // All non-cursor modes hash to the same value.
        _ => 0,
    }
}

/// Get the two vertex pixel positions for an edge (the endpoints of the road).
fn edge_vertex_pixels(
    grid: &PixelHexGrid,
    edge: board::EdgeCoord,
) -> Option<((f64, f64), (f64, f64))> {
    let verts = board::hex_vertices(edge.hex);
    let (v1, v2) = match edge.dir {
        EdgeDirection::NorthEast => (verts[0], verts[1]), // N to NE
        EdgeDirection::East => (verts[1], verts[2]),      // NE to SE
        EdgeDirection::SouthEast => (verts[2], verts[3]), // SE to S
    };
    let p1 = grid.vertex_pos.get(&v1)?;
    let p2 = grid.vertex_pos.get(&v2)?;
    Some((*p1, *p2))
}

fn probability_dots(number: u8) -> u32 {
    match number {
        2 | 12 => 1,
        3 | 11 => 2,
        4 | 10 => 3,
        5 | 9 => 4,
        6 | 8 => 5,
        _ => 0,
    }
}

/// Draw a thick line by drawing multiple parallel lines.
fn draw_thick_line(
    img: &mut RgbaImage,
    start: (f32, f32),
    end: (f32, f32),
    color: Rgba<u8>,
    thickness: i32,
) {
    // Compute perpendicular offset direction.
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.001 {
        return;
    }
    let nx = -dy / len;
    let ny = dx / len;

    let half = thickness as f32 / 2.0;
    for i in 0..thickness {
        let offset = -half + i as f32 + 0.5;
        let s = (start.0 + nx * offset, start.1 + ny * offset);
        let e = (end.0 + nx * offset, end.1 + ny * offset);
        draw_line_segment_mut(img, s, e, color);
    }
}

/// Draw a settlement (house shape).
fn draw_settlement(img: &mut RgbaImage, cx: i32, cy: i32, color: Rgba<u8>) {
    // House body.
    draw_filled_rect_mut(img, ImgRect::at(cx - 8, cy - 4).of_size(17, 13), color);
    // Roof triangle.
    let roof = [
        Point::new(cx, cy - 14),
        Point::new(cx - 11, cy - 4),
        Point::new(cx + 11, cy - 4),
    ];
    draw_polygon_mut(img, &roof, color);
    // Outline.
    let outline = Rgba([255, 255, 255, 180]);
    draw_line_segment_mut(
        img,
        (cx as f32, (cy - 14) as f32),
        ((cx - 11) as f32, (cy - 4) as f32),
        outline,
    );
    draw_line_segment_mut(
        img,
        (cx as f32, (cy - 14) as f32),
        ((cx + 11) as f32, (cy - 4) as f32),
        outline,
    );
}

/// Draw a city (larger shape with tower).
fn draw_city(img: &mut RgbaImage, cx: i32, cy: i32, color: Rgba<u8>) {
    // Main body.
    draw_filled_rect_mut(img, ImgRect::at(cx - 10, cy - 6).of_size(21, 17), color);
    // Tower.
    draw_filled_rect_mut(img, ImgRect::at(cx - 5, cy - 18).of_size(11, 13), color);
    // Tower top.
    let top = [
        Point::new(cx, cy - 24),
        Point::new(cx - 7, cy - 18),
        Point::new(cx + 7, cy - 18),
    ];
    draw_polygon_mut(img, &top, color);
    // Outline.
    let outline = Rgba([255, 255, 255, 180]);
    draw_line_segment_mut(
        img,
        (cx as f32, (cy - 24) as f32),
        ((cx - 7) as f32, (cy - 18) as f32),
        outline,
    );
    draw_line_segment_mut(
        img,
        (cx as f32, (cy - 24) as f32),
        ((cx + 7) as f32, (cy - 18) as f32),
        outline,
    );
}

impl BoardImageRenderer {
    /// Generate a board image for visual inspection.
    #[allow(dead_code)]
    pub fn generate_test_image(&self, state: &GameState) -> RgbaImage {
        self.generate_base_image(state)
    }
}

/// Draw a circle outline (no fill).
fn draw_circle_outline(img: &mut RgbaImage, cx: i32, cy: i32, radius: i32, color: Rgba<u8>) {
    let r = radius as f64;
    let steps = (r * 8.0) as usize;
    for i in 0..steps {
        let angle = 2.0 * std::f64::consts::PI * i as f64 / steps as f64;
        let x = cx as f64 + r * angle.cos();
        let y = cy as f64 + r * angle.sin();
        let px = x.round() as i32;
        let py = y.round() as i32;
        if px >= 0 && py >= 0 && (px as u32) < img.width() && (py as u32) < img.height() {
            img.put_pixel(px as u32, py as u32, color);
        }
    }
}
