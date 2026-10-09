//! Local-only OpenTelemetry exporters. No collector, sockets or network exporter.
use opentelemetry::{
    Context, KeyValue,
    trace::{TraceContextExt, Tracer},
};
use opentelemetry_sdk::{
    Resource,
    error::{OTelSdkError, OTelSdkResult},
    logs::{LogBatch, LogExporter, SdkLoggerProvider},
    trace::{SdkTracerProvider, SpanData, SpanExporter},
};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tracing_subscriber::prelude::*;

const MAX_BYTES: u64 = 5 * 1024 * 1024;
const BACKUPS: usize = 3;
const RETENTION_SECS: u64 = 7 * 24 * 60 * 60;

fn nanos(time: SystemTime) -> String {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_string()
}

fn log_value(value: &opentelemetry::logs::AnyValue) -> Value {
    use opentelemetry::logs::AnyValue;
    match value {
        AnyValue::Int(v) => json!(v),
        AnyValue::Double(v) => json!(v),
        AnyValue::String(v) => json!(v.as_str()),
        AnyValue::Boolean(v) => json!(v),
        AnyValue::Bytes(v) => json!(v),
        AnyValue::ListAny(v) => Value::Array(v.iter().map(log_value).collect()),
        AnyValue::Map(v) => Value::Object(
            v.iter()
                .map(|(k, v)| (k.to_string(), log_value(v)))
                .collect(),
        ),
        _ => json!(format!("{value:?}")),
    }
}

#[derive(Debug)]
struct LocalFile {
    path: PathBuf,
    file: File,
    bytes: u64,
    limit: u64,
}

fn private_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

impl LocalFile {
    fn open(directory: &Path, limit: u64) -> io::Result<Self> {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::{DirBuilderExt, MetadataExt};
            builder.mode(0o700);
            builder.create(directory)?;
            let metadata = fs::symlink_metadata(directory)?;
            // Refuse substituted directories and files accessible by other users.
            if !metadata.is_dir() || metadata.mode() & 0o077 != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "A pasta de logs precisa ser privada (0700) e não pode ser um link.",
                ));
            }
        }
        #[cfg(not(unix))]
        builder.create(directory)?;
        for entry in fs::read_dir(directory)?.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("rsx-")
                && (name.ends_with(".jsonl")
                    || (1..=BACKUPS).any(|i| name.ends_with(&format!(".jsonl.{i}"))))
                && let Ok(metadata) = entry.metadata()
                && metadata.is_file()
                && metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|age| age.as_secs() > RETENTION_SECS)
            {
                fs::remove_file(entry.path())?;
            }
        }
        let path = directory.join(format!(
            "rsx-{}-{}.jsonl",
            nanos(SystemTime::now()),
            std::process::id()
        ));
        let file = private_file(&path)?;
        Ok(Self {
            path,
            file,
            bytes: 0,
            limit,
        })
    }

    fn backup(&self, index: usize) -> PathBuf {
        PathBuf::from(format!("{}.{index}", self.path.display()))
    }

    fn write(&mut self, value: Value) -> io::Result<()> {
        let mut line = serde_json::to_vec(&value)?;
        line.push(b'\n');
        if self.bytes > 0 && self.bytes + line.len() as u64 > self.limit {
            match fs::remove_file(self.backup(BACKUPS)) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            for i in (1..BACKUPS).rev() {
                match fs::rename(self.backup(i), self.backup(i + 1)) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
            }
            fs::rename(&self.path, self.backup(1))?;
            self.file = private_file(&self.path)?;
            self.bytes = 0;
        }
        self.file.write_all(&line)?;
        self.file.flush()?;
        self.bytes += line.len() as u64;
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct LocalExporter(Arc<Mutex<LocalFile>>);
impl LocalExporter {
    fn write(&self, value: Value) -> OTelSdkResult {
        let result = self
            .0
            .lock()
            .map_err(|e| io::Error::other(e.to_string()))
            .and_then(|mut file| file.write(value));
        result.map_err(|e| {
            eprintln!("RSX: falha ao gravar telemetria local: {e}");
            OTelSdkError::InternalFailure(e.to_string())
        })
    }
}
impl LogExporter for LocalExporter {
    async fn export(&self, batch: LogBatch<'_>) -> OTelSdkResult {
        for (record, scope) in batch.iter() {
            let attributes: serde_json::Map<String, Value> = record
                .attributes_iter()
                .map(|(k, v)| (k.to_string(), log_value(v)))
                .collect();
            let context = record.trace_context();
            self.write(json!({
                "kind": "log", "timestamp_unix_ns": nanos(record.timestamp().unwrap_or_else(SystemTime::now)),
                "service.name": "rsx", "service.version": env!("CARGO_PKG_VERSION"), "process.pid": std::process::id(),
                "scope": scope.name(), "severity": record.severity_text(), "body": record.body().map(log_value),
                "trace_id": context.map(|c| c.trace_id.to_string()), "span_id": context.map(|c| c.span_id.to_string()),
                "attributes": attributes
            }))?;
        }
        Ok(())
    }
}
impl SpanExporter for LocalExporter {
    async fn export(&self, batch: Vec<SpanData>) -> OTelSdkResult {
        for span in batch {
            let attributes: serde_json::Map<String, Value> = span
                .attributes
                .iter()
                .map(|kv| (kv.key.to_string(), json!(kv.value.to_string())))
                .collect();
            self.write(json!({
                "kind": "span", "service.name": "rsx", "service.version": env!("CARGO_PKG_VERSION"), "process.pid": std::process::id(),
                "name": span.name, "trace_id": span.span_context.trace_id().to_string(), "span_id": span.span_context.span_id().to_string(),
                "parent_span_id": span.parent_span_id.to_string(), "start_unix_ns": nanos(span.start_time), "end_unix_ns": nanos(span.end_time),
                "duration_ms": span.end_time.duration_since(span.start_time).unwrap_or_default().as_secs_f64() * 1000.0,
                "status": format!("{:?}", span.status), "attributes": attributes
            }))?;
        }
        Ok(())
    }
}

pub struct Telemetry {
    logs: SdkLoggerProvider,
    traces: SdkTracerProvider,
}
impl Drop for Telemetry {
    fn drop(&mut self) {
        tracing::info!("application.stopped");
        if let Err(e) = self.logs.shutdown() {
            eprintln!("RSX: falha ao encerrar logs: {e}");
        }
        if let Err(e) = self.traces.shutdown() {
            eprintln!("RSX: falha ao encerrar traces: {e}");
        }
    }
}

pub fn init() -> Result<Telemetry, Box<dyn std::error::Error>> {
    let home = std::env::var_os("HOME").ok_or("HOME indisponível para salvar logs locais")?;
    let directory = PathBuf::from(home).join("Library/Logs/RSX");
    let exporter = LocalExporter(Arc::new(Mutex::new(LocalFile::open(
        &directory, MAX_BYTES,
    )?)));
    let resource = Resource::builder_empty()
        .with_attributes([
            KeyValue::new("service.name", "rsx"),
            KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
        ])
        .build();
    let logs = SdkLoggerProvider::builder()
        .with_resource(resource.clone())
        .with_simple_exporter(exporter.clone())
        .build();
    let traces = SdkTracerProvider::builder()
        .with_sampler(opentelemetry_sdk::trace::Sampler::AlwaysOn)
        .with_resource(resource)
        .with_simple_exporter(exporter)
        .build();
    tracing_subscriber::registry()
        .with(opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(&logs))
        .try_init()?;
    opentelemetry::global::set_tracer_provider(traces.clone());
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(panic = %info, backtrace = %std::backtrace::Backtrace::force_capture(), "application.panic");
        previous(info);
    }));
    tracing::info!("application.started");
    Ok(Telemetry { logs, traces })
}

/// Attach an OTEL span to the current thread; propagation across threads is explicit.
#[must_use]
pub struct Operation {
    _guard: opentelemetry::context::ContextGuard,
    context: Context,
}
impl Operation {
    pub fn start(name: &'static str) -> Self {
        let span = opentelemetry::global::tracer("rsx").start(name);
        let context = Context::current_with_span(span);
        Self {
            _guard: context.clone().attach(),
            context,
        }
    }
}
impl Drop for Operation {
    fn drop(&mut self) {
        self.context.span().end();
    }
}

pub fn error(title: &str, message: &str) {
    Context::current()
        .span()
        .set_status(opentelemetry::trace::Status::error(message.to_owned()));
    tracing::error!(
        error.title = title,
        error.message = message,
        "operation.failed"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::trace::TracerProvider;
    fn private_directory() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        }
        directory
    }
    #[test]
    fn removes_only_expired_telemetry_files() {
        let dir = private_directory();
        let expired = dir.path().join("rsx-old.jsonl");
        let unrelated = dir.path().join("notes.txt");
        let fresh = dir.path().join("rsx-fresh.jsonl");
        for path in [&expired, &unrelated] {
            let file = private_file(path).unwrap();
            file.set_times(std::fs::FileTimes::new().set_modified(UNIX_EPOCH))
                .unwrap();
        }
        private_file(&fresh).unwrap();
        let _file = LocalFile::open(dir.path(), MAX_BYTES).unwrap();
        assert!(!expired.exists());
        assert!(unrelated.exists());
        assert!(fresh.exists());
    }
    #[test]
    fn rotates_and_writes_private_json_lines() {
        let dir = private_directory();
        let mut file = LocalFile::open(dir.path(), 100).unwrap();
        for n in 0..20 {
            file.write(json!({"event": n, "message": "test record"}))
                .unwrap();
        }
        assert!(file.backup(3).exists());
        assert!(!file.backup(4).exists());
        for entry in fs::read_dir(dir.path()).unwrap() {
            let path = entry.unwrap().path();
            for line in fs::read_to_string(&path).unwrap().lines() {
                serde_json::from_str::<Value>(line).unwrap();
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    fs::metadata(path).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
        }
    }
    #[cfg(unix)]
    #[test]
    fn rejects_public_directory_and_symlink() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = private_directory();
        let public = dir.path().join("public");
        fs::create_dir(&public).unwrap();
        fs::set_permissions(&public, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(LocalFile::open(&public, MAX_BYTES).is_err());
        let link = dir.path().join("link");
        symlink(dir.path(), &link).unwrap();
        assert!(LocalFile::open(&link, MAX_BYTES).is_err());
    }
    #[test]
    fn exports_real_otel_logs_and_correlated_spans() {
        let dir = private_directory();
        let file = LocalFile::open(dir.path(), MAX_BYTES).unwrap();
        let path = file.path.clone();
        let exporter = LocalExporter(Arc::new(Mutex::new(file)));
        let logs = SdkLoggerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let traces = SdkTracerProvider::builder()
            .with_simple_exporter(exporter)
            .build();
        let subscriber = tracing_subscriber::registry()
            .with(opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(&logs));
        tracing::subscriber::with_default(subscriber, || {
            let span = traces.tracer("test").start("capture.test");
            let context = Context::current_with_span(span);
            let _guard = context.clone().attach();
            error("Test", "synthetic failure");
            context.span().end();
        });
        logs.shutdown().unwrap();
        traces.shutdown().unwrap();
        let records: Vec<Value> = fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["severity"], "ERROR");
        assert_eq!(records[0]["trace_id"], records[1]["trace_id"]);
        assert!(
            records[1]["status"]
                .as_str()
                .unwrap()
                .contains("synthetic failure")
        );
    }
}
