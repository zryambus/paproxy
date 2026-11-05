use std::{
    ops::{Deref, DerefMut},
    time::Duration,
};

use futures::{FutureExt, StreamExt};
use ratatui::backend::CrosstermBackend as Backend;
use ratatui::crossterm::{
    cursor,
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event as CrosstermEvent, KeyEvent, KeyEventKind, MouseEvent,
    },
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use serde_derive::{Deserialize, Serialize};
use tokio::{
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Event {
    Init,
    Quit,
    Error,
    Closed,
    Tick,
    Render,
    FocusGained,
    FocusLost,
    Paste(String),
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
}

pub struct Tui {
    pub terminal: ratatui::Terminal<Backend<std::io::Stderr>>,
    pub task: JoinHandle<()>,
    pub cancellation_token: CancellationToken,
    pub event_rx: UnboundedReceiver<Event>,
    pub event_tx: UnboundedSender<Event>,
    pub frame_rate: f64,
    pub tick_rate: f64,
    pub mouse: bool,
    pub paste: bool,
}

impl Tui {
    pub fn new() -> anyhow::Result<Self> {
        let tick_rate = 4.0;
        let frame_rate = 10.0;
        let terminal = ratatui::Terminal::new(Backend::new(std::io::stderr()))?;
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let cancellation_token = CancellationToken::new();
        let task = tokio::spawn(async {});
        let mouse = false;
        let paste = false;
        Ok(Self {
            terminal,
            task,
            cancellation_token,
            event_rx,
            event_tx,
            frame_rate,
            tick_rate,
            mouse,
            paste,
        })
    }

    pub fn tick_rate(mut self, tick_rate: f64) -> Self {
        self.tick_rate = tick_rate;
        self
    }

    pub fn frame_rate(mut self, frame_rate: f64) -> Self {
        self.frame_rate = frame_rate;
        self
    }

    pub fn mouse(mut self, mouse: bool) -> Self {
        self.mouse = mouse;
        self
    }

    pub fn paste(mut self, paste: bool) -> Self {
        self.paste = paste;
        self
    }

    pub fn start(&mut self) {
        let tick_delay = Duration::from_secs_f64(1.0 / self.tick_rate);
        let render_delay = Duration::from_secs_f64(1.0 / self.frame_rate);
        self.cancel();
        self.cancellation_token = CancellationToken::new();
        let _cancellation_token = self.cancellation_token.clone();
        let _event_tx = self.event_tx.clone();
        self.task = tokio::spawn(async move {
            let mut reader = crossterm::event::EventStream::new();
            let mut tick_interval = tokio::time::interval(tick_delay);
            let mut render_interval = tokio::time::interval(render_delay);
            // Sending fails only when the receiver is dropped, i.e. the TUI is shutting down
            if _event_tx.send(Event::Init).is_err() {
                return;
            }
            loop {
                let tick_delay = tick_interval.tick();
                let render_delay = render_interval.tick();
                let crossterm_event = reader.next().fuse();
                let event = tokio::select! {
                    _ = _cancellation_token.cancelled() => break,
                    maybe_event = crossterm_event => match maybe_event {
                        Some(Ok(CrosstermEvent::Key(key))) if key.kind == KeyEventKind::Press => {
                            Some(Event::Key(key))
                        }
                        Some(Ok(CrosstermEvent::Key(_))) => None,
                        Some(Ok(CrosstermEvent::Mouse(mouse))) => Some(Event::Mouse(mouse)),
                        Some(Ok(CrosstermEvent::Resize(x, y))) => Some(Event::Resize(x, y)),
                        Some(Ok(CrosstermEvent::FocusLost)) => Some(Event::FocusLost),
                        Some(Ok(CrosstermEvent::FocusGained)) => Some(Event::FocusGained),
                        Some(Ok(CrosstermEvent::Paste(s))) => Some(Event::Paste(s)),
                        Some(Err(_)) => Some(Event::Error),
                        None => None,
                    },
                    _ = tick_delay => Some(Event::Tick),
                    _ = render_delay => Some(Event::Render),
                };

                if let Some(event) = event
                    && _event_tx.send(event).is_err()
                {
                    break;
                }
            }
        });
    }

    pub fn stop(&self) {
        // The task only forwards terminal events, so it is safe to abort it without waiting
        self.cancel();
        self.task.abort();
    }

    pub fn enter(&mut self) -> anyhow::Result<()> {
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(std::io::stderr(), EnterAlternateScreen, cursor::Hide)?;
        if self.mouse {
            crossterm::execute!(std::io::stderr(), EnableMouseCapture)?;
        }
        if self.paste {
            crossterm::execute!(std::io::stderr(), EnableBracketedPaste)?;
        }
        self.start();
        Ok(())
    }

    pub fn exit(&mut self) -> anyhow::Result<()> {
        self.stop();
        if crossterm::terminal::is_raw_mode_enabled()? {
            self.flush()?;
            if self.paste {
                crossterm::execute!(std::io::stderr(), DisableBracketedPaste)?;
            }
            if self.mouse {
                crossterm::execute!(std::io::stderr(), DisableMouseCapture)?;
            }
            crossterm::execute!(std::io::stderr(), LeaveAlternateScreen, cursor::Show)?;
            crossterm::terminal::disable_raw_mode()?;
        }
        Ok(())
    }

    pub fn cancel(&self) {
        self.cancellation_token.cancel();
    }

    #[allow(dead_code)]
    pub fn suspend(&mut self) -> anyhow::Result<()> {
        self.exit()?;
        #[cfg(not(windows))]
        signal_hook::low_level::raise(signal_hook::consts::signal::SIGTSTP)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn resume(&mut self) -> anyhow::Result<()> {
        self.enter()?;
        Ok(())
    }

    pub async fn next(&mut self) -> Option<Event> {
        self.event_rx.recv().await
    }
}

impl Deref for Tui {
    type Target = ratatui::Terminal<Backend<std::io::Stderr>>;

    fn deref(&self) -> &Self::Target {
        &self.terminal
    }
}

impl DerefMut for Tui {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.terminal
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        if let Err(e) = self.exit() {
            tracing::error!("Failed to restore terminal: {}", e);
        }
    }
}
