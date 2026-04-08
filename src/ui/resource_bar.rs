//! Player resource and status panel.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use super::PLAYER_TEXT_COLORS;
use crate::game::board::Resource;
use crate::game::state::GameState;

/// Resource display colors.
pub(crate) fn resource_color(r: Resource) -> Color {
    match r {
        Resource::Wood => Color::Green,
        Resource::Brick => Color::Red,
        Resource::Sheep => Color::LightGreen,
        Resource::Wheat => Color::Yellow,
        Resource::Ore => Color::Gray,
    }
}

/// Full display name for a resource.
fn resource_name(r: Resource) -> &'static str {
    match r {
        Resource::Wood => "Wood",
        Resource::Brick => "Brick",
        Resource::Sheep => "Sheep",
        Resource::Wheat => "Wheat",
        Resource::Ore => "Ore",
    }
}

/// Build player info lines (shared by bordered and borderless renderers).
fn build_player_lines(
    state: &GameState,
    player_names: &[String],
    human_player_index: Option<usize>,
) -> Vec<Line<'static>> {
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

        let show_full = match human_player_index {
            Some(human_idx) => i == human_idx,
            None => true,
        };

        if show_full {
            // Full resource breakdown with names across two lines.
            let resources_top = [
                (Resource::Wood, resource_name(Resource::Wood)),
                (Resource::Brick, resource_name(Resource::Brick)),
                (Resource::Sheep, resource_name(Resource::Sheep)),
            ];
            let resources_bottom = [
                (Resource::Wheat, resource_name(Resource::Wheat)),
                (Resource::Ore, resource_name(Resource::Ore)),
            ];

            let mut top_spans: Vec<Span> = vec![Span::raw("  ")];
            for (r, label) in &resources_top {
                let count = ps.resource_count(*r);
                let rc = resource_color(*r);
                top_spans.push(Span::styled(
                    format!("{}:{} ", label, count),
                    Style::default().fg(rc),
                ));
            }
            lines.push(Line::from(top_spans));

            let mut bottom_spans: Vec<Span> = vec![Span::raw("  ")];
            for (r, label) in &resources_bottom {
                let count = ps.resource_count(*r);
                let rc = resource_color(*r);
                bottom_spans.push(Span::styled(
                    format!("{}:{} ", label, count),
                    Style::default().fg(rc),
                ));
            }
            lines.push(Line::from(bottom_spans));
        } else {
            // Opponents: only total card count.
            let total = ps.total_resources();
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("Resources: {}", total),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }

        // Dev cards and knights.
        let dev = ps.dev_cards.len();
        let knights = ps.knights_played;
        let mut extras: Vec<Span> = vec![Span::raw("  ")];
        if dev > 0 {
            extras.push(Span::styled(
                format!("Dev Cards:{} ", dev),
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

    lines
}

/// Render player info without a border (for use inside a shared panel).
pub fn render_players_inner(
    state: &GameState,
    player_names: &[String],
    human_player_index: Option<usize>,
    area: Rect,
    buf: &mut Buffer,
) {
    let lines = build_player_lines(state, player_names, human_player_index);
    let paragraph = Paragraph::new(lines);
    paragraph.render(area, buf);
}

/// Render the player info panel (with border).
pub fn render_players(
    state: &GameState,
    player_names: &[String],
    human_player_index: Option<usize>,
    area: Rect,
    buf: &mut Buffer,
) {
    let lines = build_player_lines(state, player_names, human_player_index);
    let block = Block::default()
        .title(" Players ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let paragraph = Paragraph::new(lines).block(block);
    paragraph.render(area, buf);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::board::Board;
    use crate::game::state::GameState;
    use ratatui::buffer::Buffer;

    fn render_to_string(state: &GameState, names: &[String], human: Option<usize>) -> String {
        let area = Rect::new(0, 0, 38, 20);
        let mut buf = Buffer::empty(area);
        render_players(state, names, human, area, &mut buf);
        let mut lines = Vec::new();
        for y in 0..area.height {
            let mut line = String::new();
            for x in 0..area.width {
                line.push_str(buf[(x, y)].symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        lines.join("\n")
    }

    #[test]
    fn human_player_sees_own_resources_with_full_names() {
        let board = Board::default_board();
        let mut state = GameState::new(board, 3);
        state.players[0].add_resource(Resource::Wood, 2);
        state.players[0].add_resource(Resource::Ore, 1);
        let names = vec!["Alice".into(), "Bob".into(), "Carol".into()];
        let output = render_to_string(&state, &names, Some(0));

        // Human player (Alice) shows full resource names.
        assert!(
            output.contains("Wood:2"),
            "should show Wood:2 for human player"
        );
        assert!(
            output.contains("Ore:1"),
            "should show Ore:1 for human player"
        );

        // Opponents show only card count, not resource breakdown.
        assert!(
            output.contains("Resources: 0"),
            "opponents should show resource count"
        );
        // Opponents should NOT have "Wood:" in their section.
        let bob_section = output.split("Bob").nth(1).unwrap_or("");
        assert!(
            !bob_section.contains("Wood:"),
            "opponent should not show resource names"
        );
    }

    #[test]
    fn spectator_mode_shows_all_resources() {
        let board = Board::default_board();
        let mut state = GameState::new(board, 2);
        state.players[0].add_resource(Resource::Brick, 3);
        state.players[1].add_resource(Resource::Sheep, 1);
        let names = vec!["Alice".into(), "Bob".into()];
        let output = render_to_string(&state, &names, None);

        // Both players show full resources in spectator mode.
        assert!(
            output.contains("Brick:3"),
            "spectator should see all resources"
        );
        assert!(
            output.contains("Sheep:1"),
            "spectator should see all resources"
        );
        assert!(
            !output.contains("Resources:"),
            "spectator mode should not show resource counts as totals"
        );
    }

    #[test]
    fn opponent_card_count_reflects_total_resources() {
        let board = Board::default_board();
        let mut state = GameState::new(board, 2);
        state.players[1].add_resource(Resource::Wood, 2);
        state.players[1].add_resource(Resource::Brick, 3);
        let names = vec!["Alice".into(), "Bob".into()];
        let output = render_to_string(&state, &names, Some(0));

        assert!(
            output.contains("Resources: 5"),
            "opponent should show total resource count of 5"
        );
    }
}
