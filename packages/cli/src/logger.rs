use colored::Colorize;
use tracing::{Event, Level, Subscriber, level_filters::LevelFilter};
use tracing_subscriber::{
    fmt::{FormatEvent, FormatFields},
    layer::SubscriberExt,
    registry::LookupSpan,
    util::SubscriberInitExt,
};

pub fn init_logger(level: LevelFilter) {
    let format_layer = tracing_subscriber::fmt::layer()
        .event_format(CustomFormatter)
        .with_writer(std::io::stderr);

    tracing_subscriber::registry()
        .with(format_layer)
        .with(level)
        .init();
}

struct CustomFormatter;

impl<S, N> FormatEvent<S, N> for CustomFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &tracing_subscriber::fmt::FmtContext<'_, S, N>,
        mut writer: tracing_subscriber::fmt::format::Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let level = *event.metadata().level();
        let level_prefix = match level {
            Level::ERROR => "❌ Error: ".bold().red().to_string(),
            Level::WARN => "⚠️  Warning: ".bold().yellow().to_string(),
            Level::INFO => String::new(),
            Level::DEBUG => "Debug: ".bold().blue().to_string(),
            Level::TRACE => "Trace: ".bold().purple().to_string(),
        };

        write!(writer, "{}", level_prefix)?;

        let mut visitor =
            tracing_subscriber::fmt::format::DefaultVisitor::new(writer.by_ref(), true);
        event.record(&mut visitor);

        writeln!(writer)
    }
}
