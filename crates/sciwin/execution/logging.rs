use crate::execution::{LogLine, RunId};
use std::{
    collections::HashMap, sync::{Arc, Mutex},
};
use tokio::sync::broadcast;
use tracing::Subscriber;
use tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};

#[derive(Debug, Clone)]
pub struct LogSink {
    pub history: Arc<Mutex<Vec<LogLine>>>,
    pub live: broadcast::Sender<LogLine>,
}

struct RunSpanData(RunId);

pub struct RunLogLayer {
    pub sinks: Arc<Mutex<HashMap<RunId, LogSink>>>,
}

#[derive(Default)]
struct RunIdVisitor(Option<String>);
impl tracing::field::Visit for RunIdVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "run_id" {
            self.0 = Some(format!("{value:?}").trim_matches('"').to_string());
        }
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "run_id" {
            self.0 = Some(value.to_string());
        }
    }
}

#[derive(Default)]
struct MsgVisitor {
    message: Option<String>,
    step: Option<String>,
}
impl tracing::field::Visit for MsgVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        match field.name() {
            "message" => self.message = Some(format!("{value:?}")),
            "step" => self.step = Some(format!("{value:?}").trim_matches('"').to_string()),
            _ => {}
        }
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "message" => self.message = Some(value.to_string()),
            "step" => self.step = Some(value.to_string()),
            _ => {}
        }
    }
}

impl<S> Layer<S> for RunLogLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: Context<'_, S>,
    ) {
        if attrs.metadata().name() != "workflow_run" {
            return;
        }
        let mut visitor = RunIdVisitor::default();
        attrs.record(&mut visitor);
        if let Some(run_id) = visitor.0 {
            ctx.span(id)
                .unwrap()
                .extensions_mut()
                .insert(RunSpanData(run_id));
        }
    }

    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        let Some(scope) = ctx.event_scope(event) else {
            return;
        };
        let Some(run_id) = scope
            .from_root()
            .find_map(|s| s.extensions().get::<RunSpanData>().map(|d| d.0.clone()))
        else {
            return;
        };

        let mut fields = MsgVisitor::default();
        event.record(&mut fields);

        let line = LogLine {
            timestamp: Some(chrono::Utc::now().naive_utc()),
            level: *event.metadata().level(),
            step: fields.step,
            message: fields.message.unwrap_or_default(),
        };

        let sinks = self.sinks.lock().unwrap();
        if let Some(sink) = sinks.get(&run_id) {
            sink.history.lock().unwrap().push(line.clone());
            let _ = sink.live.send(line);
        }
    }
}
