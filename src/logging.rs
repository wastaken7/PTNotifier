use crate::models::{LogEvent, UiEvent};
use chrono::Utc;
use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

#[derive(Clone)]
pub struct LogBroadcaster {
    tx: broadcast::Sender<UiEvent>,
    buffer: Arc<Mutex<VecDeque<LogEvent>>>,
}

impl LogBroadcaster {
    pub fn new(tx: broadcast::Sender<UiEvent>) -> Self {
        Self {
            tx,
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(200))),
        }
    }

    pub fn push(&self, ev: LogEvent) {
        {
            let mut buf = self.buffer.lock().unwrap();
            if buf.len() >= 200 {
                buf.pop_front();
            }
            buf.push_back(ev.clone());
        }
        let _ = self.tx.send(UiEvent::Log(ev));
    }

    pub fn recent(&self) -> Vec<LogEvent> {
        self.buffer.lock().unwrap().iter().cloned().collect()
    }
}

struct MessageVisitor {
    message: String,
    fields: Vec<String>,
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        } else {
            self.fields.push(format!("{}={value:?}", field.name()));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields.push(format!("{}={value}", field.name()));
        }
    }
}

pub struct BroadcastLayer {
    broadcaster: LogBroadcaster,
}

impl BroadcastLayer {
    pub fn new(broadcaster: LogBroadcaster) -> Self {
        Self { broadcaster }
    }
}

impl<S: Subscriber> Layer<S> for BroadcastLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = MessageVisitor {
            message: String::new(),
            fields: Vec::new(),
        };
        event.record(&mut visitor);

        let full_msg = if visitor.fields.is_empty() {
            visitor.message
        } else if visitor.message.is_empty() {
            visitor.fields.join(" ")
        } else {
            format!("{} ({})", visitor.message, visitor.fields.join(", "))
        };

        let meta = event.metadata();
        self.broadcaster.push(LogEvent {
            timestamp: Utc::now().to_rfc3339(),
            level: meta.level().as_str().to_string(),
            target: meta.target().to_string(),
            message: full_msg,
        });
    }
}
