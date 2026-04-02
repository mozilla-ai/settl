//! Reusable vertical menu renderer with keyboard-navigable highlight.

use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

/// Render a vertical menu list with the selected item highlighted.
///
/// Each item is rendered as a line. The selected item gets a `>` marker
/// and an inverted color highlight. Items are centered in the given area.
pub fn render_menu(items: &[&str], selected: usize, area: Rect, buf: &mut Buffer, accent: Color) {
    // Find the widest item to center the block.
    let max_width = items.iter().map(|s| s.len()).max().unwrap_or(0) + 4; // +4 for "> " prefix and padding
    let block_width = (max_width as u16).min(area.width);
    let x_offset = area.x + area.width.saturating_sub(block_width) / 2;

    for (i, item) in items.iter().enumerate() {
        if i as u16 >= area.height {
            break;
        }
        let y = area.y + i as u16;
        let row_area = Rect::new(x_offset, y, block_width, 1);

        let (prefix, style) = if i == selected {
            ("> ", Style::default().fg(Color::Black).bg(accent).bold())
        } else {
            ("  ", Style::default().fg(Color::White))
        };

        let line = Line::from(vec![
            Span::styled(prefix, style),
            Span::styled(*item, style),
        ]);
        let paragraph = Paragraph::new(line);
        paragraph.render(row_area, buf);
    }
}
