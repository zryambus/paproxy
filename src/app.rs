use std::sync::Arc;

use crossterm::event::KeyModifiers;
use ratatui::{Frame, layout::{Constraint, Layout, Margin, Rect}, style::{Color, Style}, text::Line, widgets::{Block, Cell, Clear, Row, Scrollbar, ScrollbarState, Table, TableState}};

use crate::{state::State, tui};

enum Action {}

pub struct App {
    state: Arc<State>,

    table_state: TableState,
    scroll_state: ScrollbarState,

    logs_height: u16,
}

impl App {
    pub fn new() -> Self {
        Self {
            state: Arc::new(State::new()),
            scroll_state: ScrollbarState::new(0),
            table_state: TableState::new(),
            logs_height: 0
        }
    }

    pub fn state(&self) -> Arc<State> {
        self.state.clone()
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        let mut tui = tui::Tui::new()?
            .tick_rate(4.0) // 4 ticks per second
            .frame_rate(60.0) // 30 frames per second
            .mouse(true)
            .paste(false);

        tui.enter()?; // Starts event handler, enters raw mode, enters alternate screen

        loop {
            tui.draw(|frame| {
                // Deref allows calling `tui.terminal.draw`
                self.ui(frame);
            })?;

            if let Some(evt) = tui.next().await {
                // `tui.next().await` blocks till next event
                let mut maybe_action = self.handle_event(evt);
                while let Some(action) = maybe_action {
                    maybe_action = self.update(action);
                }
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
            .style(Style::default().fg(Color::Gray))
            .style_error(Style::default().fg(Color::Red))
            .style_warn(Style::default().fg(Color::Yellow))
            .output_timestamp(Some("%H:%M:%S%.3f".to_string()))
            .output_target(false)
            .output_file(false)
            .output_line(false);
        frame.render_widget(logger_widget, area);
    }

    fn render_table(&mut self, frame: &mut Frame, area: Rect, entries: &Vec<(String, (u64, u64), u64)>) {
        let rows: Vec<Row> = entries.iter()
            .map(|(path, (sent, received), total)| Row::new([
                Cell::new(path.as_str()),
                Cell::from(human_bytes::human_bytes(*sent as f64)),
                Cell::from(human_bytes::human_bytes(*received as f64)),
                Cell::from(human_bytes::human_bytes(*total as f64)),
            ]).height(1))
            .collect();
        let table = Table::new(rows, [Constraint::Fill(1), Constraint::Length(9), Constraint::Length(9), Constraint::Length(10)])
            .header(Row::new([Cell::new("Handler"), Cell::new("Tx"), Cell::new("Rx"), Cell::new("Sum")]).height(1))
            .block(Block::bordered());

        frame.render_stateful_widget(table, area, &mut self.table_state);
    }

    fn render_scroll(&mut self, frame: &mut Frame, area: Rect, entries: &Vec<(String, (u64, u64), u64)>) {
        let scroll = Scrollbar::new(ratatui::widgets::ScrollbarOrientation::VerticalRight);
        self.scroll_state = self.scroll_state.content_length(entries.len());
        frame.render_stateful_widget(scroll, area.inner(Margin::new(1, 1)), &mut self.scroll_state);
    }

    fn render_info(&mut self, frame: &mut Frame, area: Rect, state: &Arc<State>) {
        let line = Line::from(format!(
            "HTTP traffic {} WebSocket traffic {}",
            human_bytes::human_bytes(state.total_traffic() as f64),
            human_bytes::human_bytes(state.websocket_traffic() as f64)
        )).centered();
        frame.render_widget(line, area);
    }

    fn ui(&mut self, frame: &mut Frame) {
        let state = self.state();

        let area = frame.area();
        
        let layout = Layout::vertical([Constraint::Fill(1), Constraint::Fill(1), Constraint::Length(1)]);
        let [logs_area, table_area, info_area] = layout.areas(area);
        
        self.logs_height = logs_area.height;
        
        let mut entries: Vec<(String, (u64, u64), u64)> = vec![];
        for r in state.get_info().iter() {
            entries.push((r.key().to_owned(), (r.0, r.1), r.0 + r.1));
        }
        
        entries.sort_by(|lhs, rhs| rhs.2.cmp(&lhs.2));
        
        frame.render_widget(Clear, area);

        self.render_logs(frame, logs_area);
        self.render_table(frame, table_area, &entries);
        self.render_scroll(frame, table_area, &entries);
        self.render_info(frame, info_area, &state);
    }

    fn update(&self, _action: Action) -> Option<Action> {
        None
    }

    fn handle_event(&mut self, event: tui::Event) -> Option<Action> {
        match event {
            tui::Event::Key(key_evt) => {
                match key_evt.code {
                    crossterm::event::KeyCode::Char('c') | crossterm::event::KeyCode::Char('с') if key_evt.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.state.shutdown();
                    },
                    _ => {}
                }
            },
            tui::Event::Mouse(mouse_evt) => {
                match mouse_evt.kind {
                    crossterm::event::MouseEventKind::ScrollDown => {
                        if mouse_evt.row <= self.logs_height {
                        } else {
                            self.scroll_state.next();
                            self.table_state = self.table_state.with_offset(self.scroll_state.get_position());
                        };
                    },
                    crossterm::event::MouseEventKind::ScrollUp => {
                        if mouse_evt.row <= self.logs_height {
                        } else {
                            self.scroll_state.prev();
                            self.table_state = self.table_state.with_offset(self.scroll_state.get_position());
                        };
                    },
                    _ => {}
                }
            }
            _ => {}
        }
        None
    }
}
