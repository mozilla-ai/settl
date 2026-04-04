//! Player resource and status panel.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use super::PLAYER_TEXT_COLORS;
use crate::game::board::Resource;
use crate::game::state::GameState;

/// Resource display colors.
fn resource_color(r: Resource) -> Color {
    match r {
        Resource::Wood => Color::Green,
        Resource::Brick => Color::Red,
        Resource::Sheep => Color::LightGreen,
        Resource::Wheat => Color::Yellow,
        Resource::Ore => Color::Gray,
    }
}

/// Render the player info panel.
pub fn render_players(state: &GameState, player_names: &[String], area: Rect, buf: &mut Buffer) {
    let mut lines: Vec<Line> = Vec::new();

    for (i, ps) in state.players.iter().enumerate() {
        let color = PLAYER_TEXT_COLORS.get(i).copied().unwrap_or(Color::White);
        let vp = state.victory_points(i);
        let name = player_names.get(i).map(|s| s.as_str()).unwrap_or("???");

        // Player header.
        let is_current = state.current_player() == i;
        let marker = if is_current { "\u{25b8}" } else { " " };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{}{} ", marker, name),
                Style::default().fg(color).bold(),
            ),
            Span::styled(
                format!("{}VP", vp),
                Style::default()
                    .fg(if vp >= 8 { Color::Yellow } else { Color::White })
                    .bold(),
            ),
        ]));

        // Resources.
        let resources = [
            (Resource::Wood, "W"),
            (Resource::Brick, "B"),
            (Resource::Sheep, "S"),
            (Resource::Wheat, "H"),
            (Resource::Ore, "O"),
        ];
        let mut res_spans: Vec<Span> = vec![Span::raw("  ")];
        for (r, label) in &resources {
            let count = ps.resource_count(*r);
            let rc = resource_color(*r);
            res_spans.push(Span::styled(
                format!("{}:{} ", label, count),
                Style::default().fg(rc),
            ));
        }
        lines.push(Line::from(res_spans));

        // Dev cards and knights.
        let dev = ps.dev_cards.len();
        let knights = ps.knights_played;
        let mut extras: Vec<Span> = vec![Span::raw("  ")];
        if dev > 0 {
            extras.push(Span::styled(
                format!("Dev:{} ", dev),
                Style::default().fg(Color::Cyan),
            ));
        }
        if knights > 0 {
            extras.push(Span::styled(
                format!("Kn:{} ", knights),
                Style::default().fg(Color::LightRed),
            ));
        }

        // Longest road / largest army indicators.
        if state.longest_road_player == Some(i) {
            extras.push(Span::styled(
                "\u{2605}LR ",
                Style::default().fg(Color::Yellow).bold(),
            ));
        }
        if state.largest_army_player == Some(i) {
            extras.push(Span::styled(
                "\u{2605}LA ",
                Style::default().fg(Color::Yellow).bold(),
            ));
        }

        if extras.len() > 1 {
            lines.push(Line::from(extras));
        }

        lines.push(Line::from(""));
    }

    // Game info.
    lines.push(Line::from(vec![
        Span::styled("Turn: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{}", state.turn_number + 1),
            Style::default().fg(Color::White),
        ),
    ]));

    let phase_label = match &state.phase {
        crate::game::state::GamePhase::Setup { .. } => "Setup",
        crate::game::state::GamePhase::Playing { .. } => "Playing",
        crate::game::state::GamePhase::Discarding { .. } => "Discarding",
        crate::game::state::GamePhase::PlacingRobber { .. } => "Placing Robber",
        crate::game::state::GamePhase::Stealing { .. } => "Stealing",
        crate::game::state::GamePhase::GameOver { .. } => "Game Over",
    };
    lines.push(Line::from(vec![
        Span::styled("Phase: ", Style::default().fg(Color::DarkGray)),
        Span::styled(phase_label, Style::default().fg(Color::White)),
    ]));

    lines.push(Line::from(vec![
        Span::styled("Deck: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{} cards", state.dev_card_deck.len()),
            Style::default().fg(Color::White),
        ),
    ]));

    let block = Block::default()
        .title(" Players ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let paragraph = Paragraph::new(lines).block(block);
    paragraph.render(area, buf);
}
