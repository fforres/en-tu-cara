//! Observability backbone: ONE `tracing` subscriber fanning out to layers.
//!
//! Replaces tauri-plugin-log. Why tracing: we want several sinks fed from one
//! stream, which is exactly what `tracing_subscriber`'s layer model is — far
//! simpler than hand-multiplexing a `log` logger. `tracing-subscriber`'s default
//! `tracing-log` feature bridges every existing `log::info!/warn!/error!` call
//! into tracing, so no call site changes.
//!
//! Layers:
//!   1. File (ALWAYS on) — a daily-rolling file in the logs dir. Local, full
//!      detail, kept even when shipping is off, so a user can always export it.
//!   2. PostHog events (WARN+) — ships log records as `log` events through the
//!      existing telemetry worker (`telemetry::record`), which already no-ops
//!      when telemetry is disabled. No new network path; gated by the opt-out.
//!
//! NO PII: by discipline we never log event titles/attendees/emails; messages
//! carry counts, hashes, reasons, statuses. (A future OTLP layer to PostHog Logs
//! plugs in here as a third layer.)

use std::sync::OnceLock;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::Layer as _;

/// Holds the non-blocking file-writer worker guard for the process lifetime —
/// dropping it would stop the background writer and lose buffered lines.
static FILE_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

/// Build the subscriber and install it globally (also bridges `log::`). Call once
/// at the very top of `run()` so even early startup logs are captured. `level`
/// is the file verbosity (from `ENTUCARA_LOG`); the PostHog layer is fixed at
/// WARN+ so shipping stays signal, not noise.
pub fn init(level: log::LevelFilter) {
    let file_appender = tracing_appender::rolling::daily(crate::paths::logs_dir(), "en-tu-cara.log");
    let (writer, guard) = tracing_appender::non_blocking(file_appender);
    let _ = FILE_GUARD.set(guard);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(writer)
        .with_filter(to_tracing_filter(level));

    // Stdout too, for `pnpm tauri dev` visibility (a no-op in the packaged GUI
    // app, whose stdout goes nowhere) — matches the old plugin's Stdout target.
    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stdout)
        .with_filter(to_tracing_filter(level));

    let posthog_layer = PostHogLayer.with_filter(tracing_subscriber::filter::LevelFilter::WARN);

    tracing_subscriber::registry()
        .with(file_layer)
        .with(stdout_layer)
        .with(posthog_layer)
        .init();
}

/// Map the `log` level (reused from the existing `parse_log_level`) to tracing's.
fn to_tracing_filter(level: log::LevelFilter) -> tracing_subscriber::filter::LevelFilter {
    use tracing_subscriber::filter::LevelFilter as T;
    match level {
        log::LevelFilter::Off => T::OFF,
        log::LevelFilter::Error => T::ERROR,
        log::LevelFilter::Warn => T::WARN,
        log::LevelFilter::Info => T::INFO,
        log::LevelFilter::Debug => T::DEBUG,
        log::LevelFilter::Trace => T::TRACE,
    }
}

/// A tracing layer that ships WARN+ records to PostHog as `log` events, reusing
/// the telemetry worker (which already drops when telemetry is disabled — so
/// shipping is gated by the opt-out with no extra flag).
struct PostHogLayer;

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for PostHogLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        let meta = event.metadata();
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        // Only the level/target/message leave — never structured PII. (Our log
        // messages carry counts/hashes/reasons by discipline.)
        crate::telemetry::record(
            "log",
            serde_json::json!({
                "level": meta.level().as_str(),
                "target": meta.target(),
                "message": visitor.message,
            }),
        );
    }
}

/// Pulls the `message` out of a tracing event (recorded via Debug).
#[derive(Default)]
struct MessageVisitor {
    message: String,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            use std::fmt::Write as _;
            let _ = write!(self.message, "{value:?}");
        }
    }
}
