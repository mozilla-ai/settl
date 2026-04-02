//! Renders the Catan hex board with colored terrain tiles and player pieces.

use std::collections::HashMap;

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use super::PLAYER_COLORS;
use crate::game::board::{Terrain, VertexDirection};
use crate::game::state::{Building, GameState};

/// Color for each terrain type.
fn terrain_color(t: Terrain) -> Color {
    match t {
        Terrain::Forest => Color::Green,
        Terrain::Hills => Color::Red,
        Terrain::Pasture => Color::LightGreen,
        Terrain::Fields => Color::Yellow,
        Terrain::Mountains => Color::Gray,
        Terrain::Desert => Color::DarkGray,
    }
}

/// Render the hex board as styled lines.
pub fn render_board(state: &GameState, area: Rect, buf: &mut Buffer) {
    let board = &state.board;

    // Group hexes by row.
    let mut rows: HashMap<i8, Vec<&crate::game::board::Hex>> = HashMap::new();
    for hex in &board.hexes {
        rows.entry(hex.coord.r).or_default().push(hex);
    }
    let mut row_keys: Vec<i8> = rows.keys().copied().collect();
    row_keys.sort();
    let max_width = rows.values().map(|v| v.len()).max().unwrap_or(0);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));

    for &r in &row_keys {
        let mut hex_row: Vec<&crate::game::board::Hex> = rows[&r].clone();
        hex_row.sort_by_key(|h| h.coord.q);

        let indent_count = (max_width - hex_row.len()) * 3;
        let indent = " ".repeat(indent_count);

        // Build a single formatted line string with styling.
        let mut spans: Vec<Span<'static>> = vec![Span::raw(indent)];

        for (i, hex) in hex_row.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw(" "));
            }

            let abbr = hex.terrain.abbr().to_string();
            let number: String = hex
                .number_token
                .map(|n| format!("{:>2}", n))
                .unwrap_or_else(|| "--".to_string());

            let color = terrain_color(hex.terrain);
            let is_robber = state.robber_hex == hex.coord;
            let is_hot = hex.number_token == Some(6) || hex.number_token == Some(8);

            let num_style = if is_hot {
                Style::default().fg(Color::White).bg(color).bold()
            } else {
                Style::default().fg(Color::White).bg(color)
            };

            spans.push(Span::styled("[".to_string(), Style::default().fg(color)));
            spans.push(Span::styled(
                abbr,
                Style::default().fg(Color::White).bg(color),
            ));
            spans.push(Span::styled(number, num_style));
            if is_robber {
                spans.push(Span::styled(
                    "!".to_string(),
                    Style::default().fg(Color::Black).bg(Color::Red).bold(),
                ));
            }
            spans.push(Span::styled("]".to_string(), Style::default().fg(color)));
        }

        lines.push(Line::from(spans));
    }

    // Add building info below the board.
    lines.push(Line::from(""));

    if !state.buildings.is_empty() {
        let mut building_info: Vec<Span> = vec![Span::styled(
            " Pieces: ",
            Style::default().fg(Color::White).bold(),
        )];
        let mut building_list: Vec<_> = state.buildings.iter().collect();
        building_list.sort_by_key(|(v, _)| (v.hex.q, v.hex.r));

        for (v, b) in building_list.iter().take(16) {
            let (player, symbol) = match b {
                Building::Settlement(p) => (*p, "S"),
                Building::City(p) => (*p, "C"),
            };
            let color = PLAYER_COLORS.get(player).copied().unwrap_or(Color::White);
            let dir = match v.dir {
                VertexDirection::North => "N",
                VertexDirection::South => "S",
            };
            building_info.push(Span::styled(
                format!("{}{}({},{},{}) ", symbol, player, v.hex.q, v.hex.r, dir),
                Style::default().fg(color),
            ));
        }
        if state.buildings.len() > 16 {
            building_info.push(Span::raw(format!("+{}", state.buildings.len() - 16)));
        }
        lines.push(Line::from(building_info));
    }

    // Road counts per player.
    if !state.roads.is_empty() {
        let mut road_counts = [0u32; 4];
        for &p in state.roads.values() {
            if p < 4 {
                road_counts[p] += 1;
            }
        }
        let mut road_spans: Vec<Span> = vec![Span::styled(
            " Roads:  ",
            Style::default().fg(Color::White).bold(),
        )];
        for (p, &count) in road_counts.iter().enumerate() {
            if count > 0 {
                let color = PLAYER_COLORS.get(p).copied().unwrap_or(Color::White);
                road_spans.push(Span::styled(
                    format!("P{}: {}  ", p, count),
                    Style::default().fg(color),
                ));
            }
        }
        lines.push(Line::from(road_spans));
    }

    let block = Block::default()
        .title(" Board ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let paragraph = Paragraph::new(lines).block(block);
    paragraph.render(area, buf);
}
