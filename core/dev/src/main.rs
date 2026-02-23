use clap::Parser;
use colored::Colorize;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::broadcast;

// ── Config types ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct Config {
    livekit: LivekitConfig,
    defaults: Defaults,
    participants: HashMap<String, Participant>,
    scenarios: HashMap<String, Scenario>,
}

#[derive(Deserialize)]
struct LivekitConfig {
    url: String,
    api_key: String,
    api_secret: String,
}

#[derive(Deserialize)]
struct Defaults {
    rust_log: String,
}

#[derive(Deserialize, Clone)]
struct Participant {
    name: String,
    #[serde(default)]
    camera_name: String,
    #[serde(default)]
    screenshare: bool,
}

#[derive(Deserialize)]
struct Scenario {
    description: String,
    participants: Vec<String>,
}

// ── CLI ─────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "hopp-dev", about = "Dev runner for hopp_core scenarios")]
struct Cli {
    /// Scenario name to run (from config.toml)
    scenario: Option<String>,

    /// List available scenarios
    #[arg(long)]
    list: bool,

    /// Path to config file
    #[arg(long, default_value = "config.toml")]
    config: PathBuf,

    /// Override RUST_LOG level
    #[arg(long)]
    rust_log: Option<String>,
}

// ── Color palette for participant prefixes ───────────────────────────────────

const COLORS: &[&str] = &["blue", "magenta", "cyan", "yellow", "green", "red"];

fn colored_prefix(label: &str, index: usize) -> String {
    let color = COLORS[index % COLORS.len()];
    let tag = format!("[{}]", label);
    match color {
        "blue" => tag.blue().bold().to_string(),
        "magenta" => tag.magenta().bold().to_string(),
        "cyan" => tag.cyan().bold().to_string(),
        "yellow" => tag.yellow().bold().to_string(),
        "green" => tag.green().bold().to_string(),
        "red" => tag.red().bold().to_string(),
        _ => tag.white().bold().to_string(),
    }
}

// ── Config loading ──────────────────────────────────────────────────────────

fn load_config(path: &Path) -> Config {
    let contents = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()));
    toml::from_str(&contents).unwrap_or_else(|e| panic!("Failed to parse {}: {e}", path.display()))
}

// ── Scenario listing ────────────────────────────────────────────────────────

fn list_scenarios(config: &Config) {
    println!("{}", "Available scenarios:".bold());
    println!();

    let mut names: Vec<&String> = config.scenarios.keys().collect();
    names.sort();

    for name in names {
        let scenario = &config.scenarios[name];
        let participants = scenario.participants.join(", ");
        println!(
            "  {:<20} {} [{}]",
            name.green().bold(),
            scenario.description,
            participants.dimmed()
        );
    }
    println!();
}

// ── Pipe child stdout/stderr with colored prefix ────────────────────────────

fn pipe_output(child: &mut Child, prefix: String, mut shutdown_rx: broadcast::Receiver<()>) {
    if let Some(stdout) = child.stdout.take() {
        let p = prefix.clone();
        let mut rx = shutdown_rx.resubscribe();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            loop {
                tokio::select! {
                    line = lines.next_line() => {
                        match line {
                            Ok(Some(line)) => println!("{p} {line}"),
                            Ok(None) => break,
                            Err(_) => break,
                        }
                    }
                    _ = rx.recv() => break,
                }
            }
        });
    }

    if let Some(stderr) = child.stderr.take() {
        let p = prefix;
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            loop {
                tokio::select! {
                    line = lines.next_line() => {
                        match line {
                            Ok(Some(line)) => eprintln!("{p} {line}"),
                            Ok(None) => break,
                            Err(_) => break,
                        }
                    }
                    _ = shutdown_rx.recv() => break,
                }
            }
        });
    }
}

// ── Orchestrator ────────────────────────────────────────────────────────────

async fn run_scenario(scenario_name: &str, config: &Config, rust_log: &str) {
    let scenario = config.scenarios.get(scenario_name).unwrap_or_else(|| {
        eprintln!(
            "{} Unknown scenario '{}'. Use --list to see available scenarios.",
            "error:".red().bold(),
            scenario_name
        );
        std::process::exit(1);
    });

    // Resolve participant profiles
    let participants: Vec<(String, Participant)> = scenario
        .participants
        .iter()
        .map(|key| {
            let p = config.participants.get(key).unwrap_or_else(|| {
                eprintln!(
                    "{} Participant '{}' referenced in scenario '{}' not found in config.",
                    "error:".red().bold(),
                    key,
                    scenario_name
                );
                std::process::exit(1);
            });
            (key.clone(), p.clone())
        })
        .collect();

    println!(
        "{} Running scenario '{}': {}",
        "=>".green().bold(),
        scenario_name.bold(),
        scenario.description
    );
    println!(
        "{} Participants: {}",
        "=>".green().bold(),
        participants
            .iter()
            .map(|(k, p)| format!("{} ({})", k, p.name))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!();

    // Shared shutdown signal
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    // Track all spawned children for cleanup
    let mut children: Vec<(String, Child)> = Vec::new();

    // We spawn one core per participant so each participant has its own UI.
    // This matches your manual workflow (separate core process for \"clone\").
    // The timeout should be enough to allow compilation of the core process.
    // If too slow build, run first `cd core && cargo build --bin hopp_core`
    let timeout = tokio::time::Duration::from_secs(120);
    let poll_interval = tokio::time::Duration::from_millis(250);

    for (i, (key, participant)) in participants.iter().enumerate() {
        // Use a unique socket per participant and pass it explicitly to core + its client.
        let socket_path = format!(
            "{}/core-socket-dev-{}-{}",
            std::env::temp_dir().display(),
            std::process::id(),
            key
        );
        let socket = PathBuf::from(&socket_path);
        if socket.exists() {
            let _ = std::fs::remove_file(&socket);
        }

        // ── Spawn core for participant ─────────────────────────────────
        let core_label = format!("core/{key}");
        let core_prefix = colored_prefix(&core_label, i);
        println!("{core_prefix} Spawning core for participant '{key}'...");

        let mut core_child = Command::new("cargo")
            .args([
                "run",
                "--bin",
                "hopp_core",
                "--",
                "--socket-path",
                &socket_path,
            ])
            .env("HOPP_CORE_BIN_DEFAULT", "1")
            .env("RUST_LOG", rust_log)
            .env("LIVEKIT_URL", &config.livekit.url)
            .env("LIVEKIT_API_KEY", &config.livekit.api_key)
            .env("LIVEKIT_API_SECRET", &config.livekit.api_secret)
            .current_dir("..")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap_or_else(|e| panic!("Failed to spawn core for '{key}': {e}"));

        pipe_output(
            &mut core_child,
            core_prefix.clone(),
            shutdown_tx.subscribe(),
        );
        children.push((core_label, core_child));

        // ── Wait for socket file ───────────────────────────────────────
        println!("{core_prefix} Waiting for socket at {socket_path}...");
        let start = tokio::time::Instant::now();
        loop {
            if socket.exists() {
                println!("{core_prefix} Socket ready.");
                break;
            }
            if start.elapsed() > timeout {
                eprintln!(
                    "{} Timed out waiting for socket at {socket_path}",
                    "error:".red().bold()
                );
                kill_all(&mut children).await;
                std::process::exit(1);
            }
            tokio::time::sleep(poll_interval).await;
        }

        // ── Spawn test client bound to that core ───────────────────────
        let client_prefix = colored_prefix(key, i + 10);
        println!(
            "{client_prefix} Spawning test client for '{}' ({})...",
            key, participant.name
        );

        let mut args = vec![
            "run".to_string(),
            "--manifest-path".to_string(),
            "tests/Cargo.toml".to_string(),
            "--".to_string(),
            "--socket-path".to_string(),
            socket_path.clone(),
            "call".to_string(),
            "--name".to_string(),
            participant.name.clone(),
        ];

        // Config semantics: empty string means \"no camera\" (the tests binary will skip it)
        args.push("--camera-name".to_string());
        args.push(participant.camera_name.clone());

        if participant.screenshare {
            args.push("--screenshare".to_string());
        }

        let mut client_child = Command::new("cargo")
            .args(&args)
            .env("RUST_LOG", rust_log)
            .env("LIVEKIT_URL", &config.livekit.url)
            .env("LIVEKIT_API_KEY", &config.livekit.api_key)
            .env("LIVEKIT_API_SECRET", &config.livekit.api_secret)
            .current_dir("..")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap_or_else(|e| panic!("Failed to spawn test client for '{key}': {e}"));

        pipe_output(&mut client_child, client_prefix, shutdown_tx.subscribe());
        children.push((format!("client/{key}"), client_child));
    }

    println!();
    println!(
        "{} All processes running. Press {} to stop.",
        "=>".green().bold(),
        "Ctrl+C".yellow().bold()
    );

    // ── Wait for Ctrl+C ─────────────────────────────────────────────────
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C");

    println!();
    println!("{} Shutting down...", "=>".yellow().bold());

    // Signal output readers to stop
    let _ = shutdown_tx.send(());

    // Kill all children in reverse order (clients first, then core)
    kill_all(&mut children).await;

    println!("{} Done.", "=>".green().bold());
}

// ── Shutdown: kill all children in reverse order ────────────────────────────

async fn kill_all(children: &mut Vec<(String, Child)>) {
    for (name, child) in children.iter_mut().rev() {
        let prefix = format!("[{}]", name).dimmed();
        print!("{prefix} Stopping... ");
        match child.kill().await {
            Ok(_) => println!("stopped."),
            Err(e) => println!("already exited ({e})."),
        }
    }
}

// ── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let config = load_config(&cli.config);

    if cli.list {
        list_scenarios(&config);
        return;
    }

    let scenario = match cli.scenario {
        Some(s) => s,
        None => {
            eprintln!(
                "{} No scenario specified. Use --list to see available scenarios.",
                "error:".red().bold()
            );
            std::process::exit(1);
        }
    };

    let rust_log = cli.rust_log.as_deref().unwrap_or(&config.defaults.rust_log);

    run_scenario(&scenario, &config, rust_log).await;
}
