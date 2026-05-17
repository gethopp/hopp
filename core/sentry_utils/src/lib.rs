use sentry::protocol::{Attachment, Event};
use sentry::types::random_uuid;
use sentry::{ClientInitGuard, Envelope, Level};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use sysinfo::System;

#[derive(Debug, Clone)]
pub struct SentryMetadata {
    pub user_email: String,
    pub app_version: String,
}

static SENTRY_METADATA: OnceLock<SentryMetadata> = OnceLock::new();

pub fn init_metadata(user_email: String, app_version: String) {
    let metadata = SentryMetadata {
        user_email,
        app_version,
    };

    if SENTRY_METADATA.set(metadata).is_err() {
        log::warn!("SentryMetadata already initialized, ignoring subsequent initialization");
    }
}

fn get_metadata() -> Option<&'static SentryMetadata> {
    SENTRY_METADATA.get()
}

pub fn get_log_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir().map(|mut path| {
            path.push("Library/Logs/com.hopp.app/hopp.log");
            path
        })
    }
    #[cfg(target_os = "windows")]
    {
        dirs::data_local_dir().map(|mut path| {
            path.push("com.hopp.app/logs/hopp.log");
            path
        })
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        log::warn!("get_log_path: Unsupported target OS, returning None for log path.");
        None
    }
}

fn get_system_tags() -> BTreeMap<String, String> {
    let mut tags = BTreeMap::new();
    let mut system = System::new_all();
    system.refresh_all();

    tags.insert(
        "os".to_string(),
        format!(
            "{:?} {:?}",
            std::env::consts::OS,
            System::os_version().unwrap_or_else(|| "unknown".to_string())
        ),
    );
    tags.insert(
        "server_name".to_string(),
        System::host_name().unwrap_or_else(|| "unknown".to_string()),
    );
    tags.insert("arch".to_string(), System::cpu_arch());
    match get_metadata() {
        Some(metadata) => {
            tags.insert("user_email".to_string(), metadata.user_email.clone());
            tags.insert("app_version".to_string(), metadata.app_version.clone());
        }
        None => {
            log::warn!("get_system_tags: No metadata found");
        }
    }

    tags
}

pub fn upload_logs_event(failure_reason: String) {
    let client = match sentry::Hub::current().client() {
        Some(client) => client,
        None => {
            log::warn!("upload_logs_event: No client found");
            return;
        }
    };

    let log_path = get_log_path();
    if log_path.is_none() {
        log::warn!("get_log_path: No log path found");
        return;
    }

    let log_path = log_path.unwrap();
    let logs = match std::fs::read(log_path) {
        Ok(logs) => logs,
        Err(e) => {
            log::warn!("get_log_path: Error reading log file: {e}");
            return;
        }
    };

    let log_attachment = Attachment {
        buffer: logs,
        filename: "logs.txt".to_string(),
        content_type: Some("text/plain".to_string()),
        ..Default::default()
    };

    let tags = get_system_tags();

    let event = Event {
        event_id: random_uuid(),
        message: Some(format!("Logs from Hopp: {failure_reason}")),
        level: Level::Info,
        tags,
        ..Default::default()
    };

    let mut envelope: Envelope = event.into();
    envelope.add_item(log_attachment);

    client.send_envelope(envelope);
}

#[cfg(target_os = "macos")]
fn diagnostic_reports_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|mut path| {
        path.push("Library/Logs/DiagnosticReports");
        path
    })
}

#[cfg(target_os = "macos")]
fn scan_and_load_hopp_crashes(dir: &std::path::Path) -> Vec<(PathBuf, Vec<u8>)> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            log::warn!("scan_and_load_hopp_crashes: read_dir failed: {e}");
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if !name_str.to_ascii_lowercase().starts_with("hopp") || !name_str.ends_with(".ips") {
            continue;
        }
        let path = entry.path();
        match std::fs::read(&path) {
            Ok(bytes) => out.push((path, bytes)),
            Err(e) => log::warn!("scan_and_load_hopp_crashes: read {path:?} failed: {e}"),
        }
    }
    out
}

#[cfg(target_os = "macos")]
fn pick_newest_by_filename(candidates: Vec<(PathBuf, Vec<u8>)>) -> Option<(PathBuf, Vec<u8>)> {
    candidates
        .into_iter()
        .max_by(|a, b| a.0.file_name().cmp(&b.0.file_name()))
}

pub fn upload_latest_crash() {
    #[cfg(target_os = "macos")]
    {
        let Some(dir) = diagnostic_reports_dir() else {
            log::warn!("upload_latest_crash: no DiagnosticReports dir");
            return;
        };
        let candidates = scan_and_load_hopp_crashes(&dir);
        if candidates.is_empty() {
            log::info!("upload_latest_crash: no hopp crash reports found");
            return;
        }
        let Some((path, bytes)) = pick_newest_by_filename(candidates) else {
            return;
        };

        let client = match sentry::Hub::current().client() {
            Some(c) => c,
            None => {
                log::warn!("upload_latest_crash: no Sentry client");
                return;
            }
        };

        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("crash.ips")
            .to_string();

        let attachment = Attachment {
            buffer: bytes,
            filename: filename.clone(),
            content_type: Some("text/plain".to_string()),
            ..Default::default()
        };

        let event = Event {
            event_id: random_uuid(),
            message: Some(format!("Hopp native crash: {filename}")),
            level: Level::Error,
            tags: get_system_tags(),
            ..Default::default()
        };

        let mut envelope: Envelope = event.into();
        envelope.add_item(attachment);
        client.send_envelope(envelope);
        log::info!("upload_latest_crash: sent {filename}");
    }
}

pub fn simple_event(message: String) {
    let client = match sentry::Hub::current().client() {
        Some(client) => client,
        None => {
            log::warn!("simple_event: No client found");
            return;
        }
    };
    let tags = get_system_tags();
    let event = Event {
        event_id: random_uuid(),
        message: Some(message),
        level: Level::Info,
        tags,
        ..Default::default()
    };
    let envelope: Envelope = event.into();
    client.send_envelope(envelope);
}

pub fn flush(timeout: std::time::Duration) {
    if let Some(client) = sentry::Hub::current().client() {
        let _ = client.flush(Some(timeout));
    }
}

pub fn init_sentry(failure_reason: String, dsn: Option<String>) -> Option<ClientInitGuard> {
    if dsn.is_none() {
        log::warn!("init_sentry: No DSN provided");
        return None;
    }
    let dsn = dsn.unwrap();
    Some(sentry::init((
        dsn,
        sentry::ClientOptions {
            release: sentry::release_name!(),
            before_send: Some(Arc::new(move |event| {
                upload_logs_event(failure_reason.clone());
                Some(event)
            })),
            ..Default::default()
        },
    )))
}
