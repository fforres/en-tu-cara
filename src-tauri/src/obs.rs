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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::Layer as _;

/// Holds the non-blocking file-writer worker guard for the process lifetime —
/// dropping it would stop the background writer and lose buffered lines.
static FILE_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

// --- OTLP logs sink (PostHog Logs via OTLP/JSON over ureq) ------------------
// PostHog's /i/v1/logs accepts OTLP/JSON, so we hand-build the payload and POST
// it with the same blocking ureq client telemetry uses — no OpenTelemetry SDK,
// no async runtime. Same drop-on-full discipline: a wedged Logs endpoint never
// blocks a log call.

const OTLP_PATH: &str = "/i/v1/logs";
const OTLP_QUEUE_CAP: usize = 512;
const OTLP_BATCH_MAX: usize = 50;
const OTLP_FLUSH_AFTER: Duration = Duration::from_secs(10);

/// Master shipping gate (PostHog events + OTLP). Set from settings at boot; the
/// local file layer ignores it (local logs are always kept).
static SHIPPING: AtomicBool = AtomicBool::new(false);
/// Sender into the OTLP worker; `None` until `init`.
static OTLP_TX: OnceLock<SyncSender<serde_json::Value>> = OnceLock::new();
/// Resource attributes (device id, app version) for OTLP, set once settings load.
static OTLP_RESOURCE: Mutex<(String, String)> = Mutex::new((String::new(), String::new()));

fn shipping_on() -> bool {
    SHIPPING.load(Ordering::Relaxed)
}

/// Enable/disable shipping and stamp the OTLP resource. Called from setup() once
/// settings are known (boot-time gate; mirrors telemetry::start). Local file
/// logging is unaffected.
pub fn configure_shipping(enabled: bool, device_id: String, app_version: String) {
    SHIPPING.store(enabled, Ordering::Relaxed);
    if let Ok(mut g) = OTLP_RESOURCE.lock() {
        *g = (device_id, app_version);
    }
}

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

    // OTLP → PostHog Logs: INFO+ (the full stream the Logs UI wants, minus debug
    // noise/volume). Spawn the worker first so the layer's sender exists.
    spawn_otlp_worker();
    let otlp_layer = OtlpLayer.with_filter(tracing_subscriber::filter::LevelFilter::INFO);

    tracing_subscriber::registry()
        .with(file_layer)
        .with(stdout_layer)
        .with(posthog_layer)
        .with(otlp_layer)
        .init();
}

/// Map a tracing level to the OTLP severityNumber (spec: TRACE=1, DEBUG=5,
/// INFO=9, WARN=13, ERROR=17).
fn otlp_severity(level: &tracing::Level) -> u8 {
    match *level {
        tracing::Level::TRACE => 1,
        tracing::Level::DEBUG => 5,
        tracing::Level::INFO => 9,
        tracing::Level::WARN => 13,
        tracing::Level::ERROR => 17,
    }
}

/// Tracing layer that ships INFO+ records to PostHog Logs as OTLP/JSON log
/// records (enqueued to the OTLP worker; dropped if the queue is full).
struct OtlpLayer;

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for OtlpLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        if !shipping_on() {
            return;
        }
        let Some(tx) = OTLP_TX.get() else {
            return;
        };
        let meta = event.metadata();
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let ts_ns = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
        let record = serde_json::json!({
            "timeUnixNano": ts_ns.to_string(),
            "severityNumber": otlp_severity(meta.level()),
            "severityText": meta.level().as_str(),
            "body": { "stringValue": visitor.message },
            "attributes": [
                { "key": "target", "value": { "stringValue": meta.target() } }
            ],
        });
        let _ = tx.try_send(record);
    }
}

/// Background worker: batch OTLP log records and POST them as one OTLP/JSON
/// `resourceLogs` payload to PostHog's `/i/v1/logs`. Mirrors the telemetry worker
/// (own thread, bounded queue, blocking ureq, all failures swallowed).
fn spawn_otlp_worker() {
    let (tx, rx) = std::sync::mpsc::sync_channel::<serde_json::Value>(OTLP_QUEUE_CAP);
    if OTLP_TX.set(tx).is_err() {
        return; // already initialized
    }
    let endpoint = format!("{}{OTLP_PATH}", crate::telemetry::POSTHOG_HOST);
    std::thread::Builder::new()
        .name("entucara-otlp".into())
        .spawn(move || {
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(10)))
                .build()
                .into();
            let mut batch: Vec<serde_json::Value> = Vec::new();
            loop {
                match rx.recv_timeout(OTLP_FLUSH_AFTER) {
                    Ok(rec) => {
                        batch.push(rec);
                        if batch.len() >= OTLP_BATCH_MAX {
                            flush_otlp(&agent, &endpoint, &mut batch);
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        flush_otlp(&agent, &endpoint, &mut batch);
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        flush_otlp(&agent, &endpoint, &mut batch);
                        return;
                    }
                }
            }
        })
        .ok();
}

fn flush_otlp(agent: &ureq::Agent, endpoint: &str, batch: &mut Vec<serde_json::Value>) {
    if batch.is_empty() {
        return;
    }
    let (device_id, app_version) = OTLP_RESOURCE.lock().map(|g| g.clone()).unwrap_or_default();
    let body = serde_json::json!({
        "resourceLogs": [{
            "resource": { "attributes": [
                { "key": "service.name", "value": { "stringValue": "en-tu-cara" } },
                { "key": "service.version", "value": { "stringValue": app_version } },
                { "key": "device.id", "value": { "stringValue": device_id } },
            ]},
            "scopeLogs": [{
                "scope": { "name": "en-tu-cara" },
                "logRecords": std::mem::take(batch),
            }],
        }],
    });
    match agent
        .post(endpoint)
        .header("authorization", format!("Bearer {}", crate::telemetry::POSTHOG_KEY))
        .header("content-type", "application/json")
        .send_json(&body)
    {
        Ok(resp) => tracing::debug!("otlp logs flushed (HTTP {})", resp.status()),
        // debug, not warn: a warn here would itself try to ship → feedback loop.
        Err(e) => tracing::debug!("otlp logs flush failed (dropped): {e}"),
    }
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
