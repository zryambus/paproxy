use std::sync::Arc;

use crossterm::event::KeyModifiers;
use dashmap::DashMap;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    text::Line,
    widgets::{Block, Cell, Clear, Row, Scrollbar, ScrollbarState, Table, TableState},
};
use tui_logger::{LogFormatter, TuiWidgetState};

use crate::{state::State, tui};

pub struct App {
    state: Arc<State>,

    table_state: TableState,
    scroll_state: ScrollbarState,
    logs_state: TuiWidgetState,

    logs_height: u16,
}

impl App {
    pub fn new(state: Arc<State>) -> Self {
        let logs_state = TuiWidgetState::default();

        Self {
            state,
            scroll_state: ScrollbarState::new(0),
            table_state: TableState::new(),
            logs_state,
            logs_height: 0,
        }
    }

    pub fn state(&self) -> Arc<State> {
        self.state.clone()
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        let mut tui = tui::Tui::new()?
            .tick_rate(4.0) // 4 ticks per second
            .frame_rate(10.0) // 10 frames per second
            .mouse(true)
            .paste(false);

        tui.enter()?; // Starts event handler, enters raw mode, enters alternate screen

        loop {
            tui.draw(|frame| {
                // Deref allows calling `tui.terminal.draw`
                self.ui(frame);
            })?;

            // `tui.next().await` blocks till next event
            if let Some(evt) = tui.next().await {
                self.handle_event(evt);
            };

            if self.state.is_shutdown() {
                break;
            }
        }

        tui.exit()?; // stops event handler, exits raw mode, exits alternate screen

        Ok(())
    }

    fn render_logs(&mut self, frame: &mut Frame, area: Rect) {
        let logger_widget = tui_logger::TuiLoggerWidget::default()
            .block(Block::bordered().title(Line::from("Log").centered()))
            .opt_formatter(Some(Box::new(LogsFormatter {})))
            .state(&self.logs_state);
        frame.render_widget(logger_widget, area);
    }

    fn render_table(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        entries: &[(String, (u64, u64), u64)],
    ) {
        let rows: Vec<Row> = entries
            .iter()
            .map(|(path, (sent, received), total)| {
                Row::new([
                    Cell::new(path.as_str()),
                    Cell::from(human_bytes::human_bytes(*sent as f64)),
                    Cell::from(human_bytes::human_bytes(*received as f64)),
                    Cell::from(human_bytes::human_bytes(*total as f64)),
                ])
                .height(1)
            })
            .collect();
        let table = Table::new(
            rows,
            [
                Constraint::Fill(1),
                Constraint::Length(9),
                Constraint::Length(9),
                Constraint::Length(10),
            ],
        )
        .header(
            Row::new([
                Cell::new("Handler"),
                Cell::new("Tx"),
                Cell::new("Rx"),
                Cell::new("Sum"),
            ])
            .height(1),
        )
        .block(Block::bordered());

        frame.render_stateful_widget(table, area, &mut self.table_state);
    }

    fn render_scroll(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        entries: &[(String, (u64, u64), u64)],
    ) {
        let scroll = Scrollbar::new(ratatui::widgets::ScrollbarOrientation::VerticalRight);
        self.scroll_state = self.scroll_state.content_length(entries.len());
        frame.render_stateful_widget(
            scroll,
            area.inner(Margin::new(1, 1)),
            &mut self.scroll_state,
        );
    }

    fn render_info(&mut self, frame: &mut Frame, area: Rect, state: &Arc<State>) {
        let line = Line::from(format!(
            "HTTP traffic {} WebSocket traffic {}",
            human_bytes::human_bytes(state.total_traffic() as f64),
            human_bytes::human_bytes(state.websocket_traffic() as f64)
        ))
        .centered();
        frame.render_widget(line, area);
    }

    fn ui(&mut self, frame: &mut Frame) {
        let state = self.state();

        let area = frame.area();

        let layout = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ]);
        let [logs_area, table_area, info_area] = layout.areas(area);

        self.logs_height = logs_area.height;

        let mut entries: Vec<(String, (u64, u64), u64)> = vec![];
        for r in state.get_info().iter() {
            entries.push((r.key().to_owned(), (r.0, r.1), r.0 + r.1));
        }

        entries.sort_by_key(|e| std::cmp::Reverse(e.2));

        frame.render_widget(Clear, area);

        self.render_logs(frame, logs_area);
        self.render_table(frame, table_area, &entries);
        self.render_scroll(frame, table_area, &entries);
        self.render_info(frame, info_area, &state);
    }

    fn handle_event(&mut self, event: tui::Event) {
        match event {
            tui::Event::Key(key_evt) => match key_evt.code {
                crossterm::event::KeyCode::Char('c') | crossterm::event::KeyCode::Char('с')
                    if key_evt.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    self.state.shutdown();
                }
                crossterm::event::KeyCode::Char('d') | crossterm::event::KeyCode::Char('в') => {
                    let state = self.state();
                    tokio::task::spawn_blocking(move || {
                        dump_results(state.get_info()).ok();
                    });
                }
                _ => {}
            },
            tui::Event::Mouse(mouse_evt) => match mouse_evt.kind {
                crossterm::event::MouseEventKind::ScrollDown => {
                    if mouse_evt.row <= self.logs_height {
                        self.logs_state
                            .transition(tui_logger::TuiWidgetEvent::NextPageKey);
                    } else {
                        self.scroll_state.next();
                        self.table_state = self
                            .table_state
                            .with_offset(self.scroll_state.get_position());
                    };
                }
                crossterm::event::MouseEventKind::ScrollUp => {
                    if mouse_evt.row <= self.logs_height {
                        self.logs_state
                            .transition(tui_logger::TuiWidgetEvent::PrevPageKey);
                    } else {
                        self.scroll_state.prev();
                        self.table_state = self
                            .table_state
                            .with_offset(self.scroll_state.get_position());
                    };
                }
                _ => {}
            },
            _ => {}
        }
    }
}

struct LogsFormatter {}

impl LogFormatter for LogsFormatter {
    fn min_width(&self) -> u16 {
        9 + 4
    }

    fn format(&self, width: usize, evt: &tui_logger::ExtLogRecord) -> Vec<Line<'_>> {
        let mut output = String::new();
        output.push_str(&format!("{} ", evt.timestamp.strftime("%H:%M:%S%.3f")));
        output.push_str(&format!("{}: ", evt.level));

        let mut sublines: Vec<&str> = evt.msg().lines().rev().collect();
        let first_line = sublines.pop().unwrap_or_default();
        // Truncate by chars, not bytes: messages may contain non-ASCII text
        let remaining = width.saturating_sub(output.chars().count());
        output.extend(first_line.chars().take(remaining));
        [Line::from(output)].into()
    }
}

use std::io::Write;

pub fn dump_results(results: &DashMap<String, (u64, u64)>) -> anyhow::Result<()> {
    let path = "logs/results.csv";
    simple_file_rotation::FileRotation::new(path)
        .max_old_files(5)
        .file_extension("csv")
        .rotate()?;

    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    write_results(results, file)?;
    Ok(())
}

fn write_results(
    results: &DashMap<String, (u64, u64)>,
    mut out: impl Write,
) -> std::io::Result<()> {
    writeln!(out, "path,tx,rx")?;
    for r in results.iter() {
        // CSV escapes quotes inside a quoted field by doubling them
        let path = r.key().replace('"', "\"\"");
        writeln!(out, "\"{}\",{},{}", path, r.0, r.1)?;
    }
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_are_written_as_valid_csv() {
        let results = DashMap::new();
        results.insert(r#"/api?q="x",y"#.to_owned(), (1, 2));

        let mut out = Vec::new();
        write_results(&results, &mut out).unwrap();

        assert_eq!(
            String::from_utf8(out).unwrap(),
            "path,tx,rx
\"/api?q=\"\"x\"\",y\",1,2
"
        );
    }
}
