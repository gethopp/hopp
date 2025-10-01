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
    client.capture_event(event, None);
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
