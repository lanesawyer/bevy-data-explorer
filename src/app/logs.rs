//! Keeping the log where the app can show it.
//!
//! Bevy's log goes to the terminal, which is exactly where it is not when
//! someone is reporting what went wrong. A `tracing` layer keeps the last few
//! hundred records in memory as well, and the debug panel reads them from
//! there.
//!
//! The layer is handed to `LogPlugin::custom_layer`, which runs once while the
//! plugin is built and is the only hook into the subscriber — so the shared
//! buffer is made here, put in the world as a resource, and captured by the
//! layer. Both ends hold the same `Arc`.
//!
//! Records arrive from whichever thread logged them, including the task pools,
//! so the buffer is behind a mutex and every read takes a copy. It holds
//! [`MAX_RECORDS`] and drops the oldest, because a viewer left open all day
//! must not grow without bound.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use bevy::log::BoxedLayer;
use bevy::log::tracing::field::{Field, Visit};
use bevy::log::tracing::{Event, Level, Subscriber};
use bevy::log::tracing_subscriber::Layer;
use bevy::log::tracing_subscriber::layer::Context;
use bevy::prelude::*;

/// How many records are kept. Enough to cover opening a dataset and whatever
/// went wrong with it, without keeping a day's worth.
pub const MAX_RECORDS: usize = 500;

/// One line of the log.
#[derive(Clone)]
pub struct LogRecord {
    pub level: Level,
    /// The module that logged it, which is what says whether a warning is ours.
    pub target: String,
    pub message: String,
}

impl LogRecord {
    /// The line as it reads in the panel and in a copied report.
    pub fn line(&self) -> String {
        format!("{:<5} {}  {}", self.level, self.target, self.message)
    }
}

#[derive(Default)]
struct Tail {
    records: VecDeque<LogRecord>,
    /// Counts everything ever recorded, not what is held. The panel rebuilds
    /// when this moves, which a length cannot tell it once the buffer is full.
    written: u64,
}

/// The log as the app can read it back.
#[derive(Resource, Clone, Default)]
pub struct LogTail(Arc<Mutex<Tail>>);

impl LogTail {
    fn push(&self, record: LogRecord) {
        // A poisoned lock means a panic while logging; the app is going down
        // and there is nothing useful to do about it here.
        let Ok(mut tail) = self.0.lock() else { return };
        if tail.records.len() == MAX_RECORDS {
            tail.records.pop_front();
        }
        tail.records.push_back(record);
        tail.written += 1;
    }

    /// How many records have ever been written.
    pub fn written(&self) -> u64 {
        self.0.lock().map(|tail| tail.written).unwrap_or(0)
    }

    /// The last `count` records, oldest first.
    pub fn recent(&self, count: usize) -> Vec<LogRecord> {
        let Ok(tail) = self.0.lock() else {
            return Vec::new();
        };
        let skip = tail.records.len().saturating_sub(count);
        tail.records.iter().skip(skip).cloned().collect()
    }

    /// Everything held, as text to paste into a report.
    pub fn report(&self) -> String {
        let Ok(tail) = self.0.lock() else {
            return String::new();
        };
        tail.records
            .iter()
            .map(LogRecord::line)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Pulls a record apart into what is worth showing.
///
/// `tracing` carries fields rather than a formatted string: the message is the
/// field named `message`, and anything else a caller attached is appended,
/// since a warning's fields are often the half that says which node or tile it
/// was about.
///
/// A record that came through the `log` crate — most of what our dependencies
/// write — arrives with its real target, module path, file and line as fields
/// and `log` as its own target. The target is taken from `log.target` when it
/// is there, and the rest of that bookkeeping is dropped: a panel showing the
/// path of a file inside a crates.io checkout, per line, shows nothing else.
struct Message {
    message: String,
    fields: String,
    target: Option<String>,
}

impl Message {
    fn new() -> Self {
        Message {
            message: String::new(),
            fields: String::new(),
            target: None,
        }
    }
}

impl Visit for Message {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        match field.name() {
            "message" => self.message = format!("{value:?}"),
            name if name.starts_with("log.") => {}
            name => {
                if !self.fields.is_empty() {
                    self.fields.push(' ');
                }
                self.fields.push_str(&format!("{name}={value:?}"));
            }
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "message" => self.message = value.to_string(),
            "log.target" => self.target = Some(value.to_string()),
            _ => self.record_debug(field, &value),
        }
    }
}

struct TailLayer(LogTail);

impl<S: Subscriber> Layer<S> for TailLayer {
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let mut visitor = Message::new();
        event.record(&mut visitor);
        let message = if visitor.fields.is_empty() {
            visitor.message
        } else {
            format!("{} {}", visitor.message, visitor.fields)
        };
        let metadata = event.metadata();
        self.0.push(LogRecord {
            level: *metadata.level(),
            target: visitor
                .target
                .unwrap_or_else(|| metadata.target().to_string()),
            message,
        });
    }
}

/// Keep the log in the world as well as on the terminal.
///
/// Handed to `LogPlugin::custom_layer`, which is why it is a plain function
/// taking the app: it is called once, while the log plugin is being built, and
/// is the only chance to put the shared buffer somewhere the app can find it.
pub fn capture(app: &mut App) -> Option<BoxedLayer> {
    let tail = LogTail::default();
    app.insert_resource(tail.clone());
    Some(Box::new(TailLayer(tail)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(message: &str) -> LogRecord {
        LogRecord {
            level: Level::INFO,
            target: "test".into(),
            message: message.into(),
        }
    }

    #[test]
    fn the_oldest_records_are_dropped_rather_than_growing_for_ever() {
        let tail = LogTail::default();
        for index in 0..MAX_RECORDS + 10 {
            tail.push(record(&format!("line {index}")));
        }
        let held = tail.recent(MAX_RECORDS * 2);
        assert_eq!(held.len(), MAX_RECORDS);
        assert_eq!(held[0].message, "line 10", "the first ten are gone");
        assert_eq!(tail.written(), MAX_RECORDS as u64 + 10);
    }

    #[test]
    fn what_has_been_written_keeps_counting_once_the_buffer_is_full() {
        // The panel rebuilds on this, and the length stops moving long before
        // the log does.
        let tail = LogTail::default();
        for _ in 0..MAX_RECORDS + 5 {
            tail.push(record("x"));
        }
        let before = tail.written();
        tail.push(record("y"));
        assert_eq!(tail.written(), before + 1);
        assert_eq!(tail.recent(usize::MAX).len(), MAX_RECORDS);
    }

    #[test]
    fn only_the_last_few_are_asked_for() {
        let tail = LogTail::default();
        for index in 0..10 {
            tail.push(record(&format!("line {index}")));
        }
        let last = tail.recent(3);
        assert_eq!(last.len(), 3);
        assert_eq!(last[0].message, "line 7");
        assert_eq!(last[2].message, "line 9", "oldest first, newest last");
    }

    #[test]
    fn a_report_is_one_line_each() {
        let tail = LogTail::default();
        tail.push(record("first"));
        tail.push(record("second"));
        let report = tail.report();
        assert_eq!(report.lines().count(), 2);
        assert!(report.contains("first") && report.contains("second"));
        // The level and the module are in it: a message on its own rarely says
        // which part of the app it came from.
        assert!(report.contains("INFO") && report.contains("test"));
    }

    #[test]
    fn an_empty_log_reports_nothing_rather_than_a_blank_line() {
        assert!(LogTail::default().report().is_empty());
    }

    /// Run `emit` against a subscriber wearing the real layer, and collect what
    /// it recorded.
    ///
    /// Feeding actual events through is the only way to check the visitor: a
    /// hand-built stand-in would go on passing after the real one drifted.
    fn captured(emit: impl FnOnce()) -> Vec<LogRecord> {
        use bevy::log::tracing_subscriber::layer::SubscriberExt;
        use bevy::log::tracing_subscriber::registry::Registry;

        let tail = LogTail::default();
        let subscriber = Registry::default().with(TailLayer(tail.clone()));
        bevy::log::tracing::subscriber::with_default(subscriber, emit);
        tail.recent(usize::MAX)
    }

    #[test]
    fn a_records_own_fields_are_kept_beside_its_message() {
        // A warning's fields are usually the half that says what it was about.
        let records = captured(|| bevy::log::warn!(node = "r042", "node is short"));
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].level, Level::WARN);
        assert!(records[0].message.starts_with("node is short"));
        assert!(
            records[0].message.contains("node="),
            "{}",
            records[0].message
        );
    }

    #[test]
    fn the_log_bridges_bookkeeping_is_dropped() {
        // Everything a dependency writes arrives this way: its real target as a
        // field, and a file path inside a crates.io checkout that would fill
        // the panel on its own.
        let records = captured(|| {
            bevy::log::info!(
                log.target = "sctk_adwaita::buttons",
                log.file = "/home/x/.cargo/registry/src/sctk/buttons.rs",
                log.line = 172,
                "Ignoring unknown button type"
            )
        });
        assert_eq!(records[0].message, "Ignoring unknown button type");
        assert_eq!(records[0].target, "sctk_adwaita::buttons");
        assert!(
            !records[0].line().contains(".cargo"),
            "the panel would show a checkout path on every line: {}",
            records[0].line()
        );
    }

    #[test]
    fn a_plain_message_is_left_as_it_was_written() {
        let records = captured(|| bevy::log::error!("could not read the store"));
        assert_eq!(records[0].message, "could not read the store");
        assert_eq!(records[0].level, Level::ERROR);
        assert!(records[0].target.starts_with("bevy_data_explorer"));
    }
}
