use std::{
    collections::VecDeque,
    fmt::Write as _,
    sync::{Arc, Mutex},
};

use tracing::{Event, Level, Subscriber, field::Field};
use tracing_subscriber::{field::Visit, layer::Context, registry::LookupSpan};

const MAX_LINES: usize = 10_000;

pub struct LogLine {
    pub time: String,
    pub level: Level,
    pub message: String,
}

#[derive(Default)]
struct Inner {
    lines: VecDeque<LogLine>,
    /// Total number of lines ever pushed, lets readers notice new lines after old ones are evicted
    total: u64,
}

/// Keeps the most recent log lines in memory for the GUI
#[derive(Clone, Default)]
pub struct LogBuffer(Arc<Mutex<Inner>>);

impl LogBuffer {
    fn push(&self, line: LogLine) {
        let mut inner = self.0.lock().unwrap();
        if inner.lines.len() == MAX_LINES {
            inner.lines.pop_front();
        }
        inner.lines.push_back(line);
        inner.total += 1;
    }

    pub fn total(&self) -> u64 {
        self.0.lock().unwrap().total
    }

    pub fn len(&self) -> usize {
        self.0.lock().unwrap().lines.len()
    }

    /// Calls `f` for each line in `range`, the range is clamped to the stored lines
    pub fn with_lines<R>(
        &self,
        range: std::ops::Range<usize>,
        mut f: impl FnMut(&LogLine) -> R,
    ) -> Vec<R> {
        let inner = self.0.lock().unwrap();
        let end = range.end.min(inner.lines.len());
        let start = range.start.min(end);
        inner.lines.range(start..end).map(&mut f).collect()
    }

    pub fn layer(&self) -> LogBufferLayer {
        LogBufferLayer(self.clone())
    }
}

pub struct LogBufferLayer(LogBuffer);

impl<S> tracing_subscriber::Layer<S> for LogBufferLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        self.0.push(LogLine {
            time: chrono::Local::now().format("%H:%M:%S%.3f").to_string(),
            level: *event.metadata().level(),
            message: visitor.finish(),
        });
    }
}

/// Collects the message and appends the other fields as `key=value`
#[derive(Default)]
struct MessageVisitor {
    message: String,
    fields: String,
}

impl MessageVisitor {
    fn finish(mut self) -> String {
        if !self.fields.is_empty() {
            if !self.message.is_empty() {
                self.message.push(' ');
            }
            self.message.push_str(self.fields.trim_start());
        }
        self.message
    }
}

impl Visit for MessageVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        // Metadata of records bridged from the log crate, e.g. by GPUI
        if field.name().starts_with("log.") {
            return;
        }
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            let _ = write!(self.fields, " {}={}", field.name(), value);
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name().starts_with("log.") {
            return;
        }
        if field.name() == "message" {
            let _ = write!(self.message, "{:?}", value);
        } else {
            let _ = write!(self.fields, " {}={:?}", field.name(), value);
        }
    }
}

#[cfg(test)]
mod tests {
    use tracing_subscriber::prelude::*;

    use super::*;

    #[test]
    fn events_are_captured_with_fields() {
        let buffer = LogBuffer::default();
        let subscriber = tracing_subscriber::registry().with(buffer.layer());

        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!(path = "/api", "Request failed");
        });

        assert_eq!(buffer.total(), 1);
        let lines = buffer.with_lines(0..10, |line| (line.level, line.message.clone()));
        assert_eq!(
            lines,
            [(Level::WARN, "Request failed path=/api".to_owned())]
        );
    }

    #[test]
    fn old_lines_are_evicted() {
        let buffer = LogBuffer::default();
        for i in 0..MAX_LINES + 5 {
            buffer.push(LogLine {
                time: String::new(),
                level: Level::INFO,
                message: i.to_string(),
            });
        }

        assert_eq!(buffer.len(), MAX_LINES);
        assert_eq!(buffer.total(), (MAX_LINES + 5) as u64);
        assert_eq!(buffer.with_lines(0..1, |line| line.message.clone()), ["5"]);
    }
}
