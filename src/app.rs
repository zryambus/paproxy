use std::sync::Arc;

use crossterm::event::KeyModifiers;
use ratatui::{Frame, layout::{Constraint, Layout}, style::{Color, Style}, text::{Line, Span}, widgets::{Block, Clear, List, ListItem}};

use crate::{state::State, tui};

enum Action {}

pub struct App {
    state: Arc<State>,
}

impl App {
    pub fn new() -> Self {
        Self {
            state: Arc::new(State::new())
        }
    }

    pub fn state(&self) -> Arc<State> {
        self.state.clone()
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        let mut tui = tui::Tui::new()?
            .tick_rate(4.0) // 4 ticks per second
            .frame_rate(60.0) // 30 frames per second
            .mouse(false)
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

    fn ui(&self, frame: &mut Frame) {
        let state = self.state();

        let area = frame.area();
    
        frame.render_widget(Clear, area);

        let layout = Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).margin(1);
        let [log_area, list_area] = layout.areas(area);

        let logger_widget = tui_logger::TuiLoggerWidget::default()
            .block(Block::bordered().title(Line::from("Log").centered()))
            .style(Style::default().fg(Color::Gray))
            .style_error(Style::default().fg(Color::Red))
            .style_warn(Style::default().fg(Color::Yellow))
            .output_timestamp(Some("%H:%M:%S%.3f".to_string()))
            .output_target(false)
            .output_file(false)
            .output_line(false);
        frame.render_widget(logger_widget, log_area);

        let mut entries: Vec<(String, (u64, u64), u64)> = vec![];
        for r in state.get_info().iter() {
            entries.push((r.key().to_owned(), (r.0, r.1), r.0 + r.1));
        }

        entries.sort_by(|lhs, rhs| rhs.2.cmp(&lhs.2));

        let items: Vec<ListItem> = entries.iter()
            .map(|(path, (sent, received), total)| {
                ListItem::new(Line::from(vec![
                    Span::raw(path.clone()),
                    Span::raw(format!(" Tx {}", human_bytes::human_bytes(*sent as f64))),
                    Span::raw(format!(" Rx {}", human_bytes::human_bytes(*received as f64))),
                    Span::raw(format!(" Sum {}", human_bytes::human_bytes(*total as f64))),
                ]))
            })
            .collect();
        let list = List::new(items)
            .block(Block::bordered()
                .title(Line::from("Requests").centered())
                .title_bottom(Line::from(format!(
                    "HTTP traffic {} WebSocket traffic {}",
                    human_bytes::human_bytes(state.total_traffic() as f64),
                    human_bytes::human_bytes(state.websocket_traffic() as f64)
                )).centered())
            );
        frame.render_widget(list, list_area);
    }

    fn update(&self, _action: Action) -> Option<Action> {
        None
    }

    fn handle_event(&self, event: tui::Event) -> Option<Action> {
        match event {
            tui::Event::Key(key_evt) => {
                match key_evt.code {
                    crossterm::event::KeyCode::Char('c') | crossterm::event::KeyCode::Char('с') if key_evt.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.state.shutdown();
                    },
                    _ => {}
                }
            },
            _ => {}
        }
        None
    }
}
