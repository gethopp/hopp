use clap::Args;
use std::fs::{self, File};
use std::io;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Args, Debug)]
pub struct SmokeArgs {
    /// Scenario names; omitted means the standard local suite
    pub scenarios: Vec<String>,
    /// Print scenario names without building or starting anything
    #[arg(long)]
    pub list: bool,
    /// Repeated transitions within the same core process
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u32).range(1..))]
    pub cycles: u32,
    /// Treat unavailable devices/permissions as failures instead of skips
    #[arg(long)]
    pub require_devices: bool,
    /// Existing test-support core binary; omitted means build it first
    #[arg(long)]
    pub core_bin: Option<PathBuf>,
    /// Hard wall-clock deadline per scenario, including startup and shutdown
    #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u64).range(1..))]
    pub timeout_seconds: u64,
    /// Opt-in macOS connection profile; requires previously granted sudo credentials
    #[arg(long, value_parser = ["good", "medium", "bad", "3g"])]
    pub net: Option<String>,
}

/// Owns children even while a scenario fails, times out, or Ctrl-C is received.
struct ChildGuard(Child);

impl ChildGuard {
    fn spawn(command: &mut Command) -> io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        command.spawn().map(Self)
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            // Kill the process group too: a timed-out scenario may own a fixture child.
            let _ = Command::new("/bin/kill")
                .args(["-KILL", &format!("-{}", self.0.id())])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct SocketGuard(PathBuf);

impl Drop for SocketGuard {
    fn drop(&mut self) {
        #[cfg(not(windows))]
        let _ = fs::remove_file(&self.0);
    }
}

struct NetworkGuard {
    script: PathBuf,
    session: String,
    stop: std::sync::mpsc::Sender<()>,
    keepalive: Option<std::thread::JoinHandle<()>>,
}

fn keep_sudo_alive() -> (std::sync::mpsc::Sender<()>, std::thread::JoinHandle<()>) {
    let (stop, stopped) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        while stopped.recv_timeout(Duration::from_secs(30))
            == Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        {
            let _ = Command::new("sudo")
                .args(["-n", "true"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    });
    (stop, thread)
}

impl NetworkGuard {
    fn new(script: PathBuf, session: String) -> Self {
        let (stop, keepalive) = keep_sudo_alive();
        Self {
            script,
            session,
            stop,
            keepalive: Some(keepalive),
        }
    }
}

impl Drop for NetworkGuard {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.keepalive.take() {
            let _ = thread.join();
        }
        match Command::new("sudo")
            .arg("-n")
            .arg(&self.script)
            .args(["off", "--session", &self.session])
            .status()
        {
            Ok(status) if status.success() => {}
            result => eprintln!(
                "Network cleanup failed ({result:?}); run sudo {} off --session {}",
                self.script.display(),
                self.session
            ),
        }
    }
}

fn wait_child(
    child: &mut Child,
    deadline: Instant,
    interrupted: &AtomicBool,
) -> io::Result<ExitStatus> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if interrupted.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "smoke run interrupted",
            ));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "scenario exceeded its hard deadline",
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn livekit_address(url: &str) -> io::Result<(String, u16)> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "invalid LIVEKIT_URL host/port");
    let (rest, default_port) = if let Some(rest) = url.strip_prefix("ws://") {
        (rest, 80)
    } else if let Some(rest) = url.strip_prefix("wss://") {
        (rest, 443)
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "LIVEKIT_URL must use ws:// or wss://",
        ));
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') || authority.is_empty() {
        return Err(invalid());
    }
    let (host, port) = if let Some(ipv6) = authority.strip_prefix('[') {
        let (host, suffix) = ipv6.split_once(']').ok_or_else(invalid)?;
        host.parse::<std::net::Ipv6Addr>().map_err(|_| invalid())?;
        let port = match suffix {
            "" => default_port,
            suffix => suffix
                .strip_prefix(':')
                .ok_or_else(invalid)?
                .parse()
                .map_err(|_| invalid())?,
        };
        (host, port)
    } else {
        if authority.contains(['[', ']']) {
            return Err(invalid());
        }
        match authority.split_once(':') {
            Some((host, port)) => (host, port.parse().map_err(|_| invalid())?),
            None => (authority, default_port),
        }
    };
    if host.is_empty() || port == 0 || host.chars().any(char::is_whitespace) {
        return Err(invalid());
    }
    Ok((host.into(), port))
}

fn server_answers(host: &str, port: u16) -> bool {
    (host, port).to_socket_addrs().is_ok_and(|addresses| {
        addresses
            .into_iter()
            .any(|address| TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_ok())
    })
}

fn log_stdio(path: &Path) -> io::Result<(Stdio, Stdio)> {
    let file = File::create(path)?;
    Ok((Stdio::from(file.try_clone()?), Stdio::from(file)))
}

fn build_core(core_dir: &Path) -> io::Result<PathBuf> {
    println!("Building core and fixture with test-support...");
    let status = Command::new("cargo")
        .args(["build", "--features", "test-support", "--bins"])
        .env("HOPP_CORE_BIN_DEFAULT", "1")
        .current_dir(core_dir)
        .status()?;
    if !status.success() {
        return Err(io::Error::other("core build failed"));
    }
    Ok(core_dir
        .join("target/debug")
        .join(format!("hopp_core{}", std::env::consts::EXE_SUFFIX)))
}

pub fn run(args: SmokeArgs) -> io::Result<()> {
    if args.list {
        for scenario in crate::smoke::SCENARIOS {
            println!("{scenario}");
        }
        return Ok(());
    }
    let scenarios: Vec<String> = if args.scenarios.is_empty() {
        crate::smoke::SCENARIOS
            .iter()
            .filter(|name| !matches!(**name, "network-drop" | "audio-signal"))
            .map(|name| (*name).into())
            .collect()
    } else {
        args.scenarios.clone()
    };
    for scenario in &scenarios {
        if !crate::smoke::SCENARIOS.contains(&scenario.as_str()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown scenario {scenario:?}; use smoke --list"),
            ));
        }
    }
    let interrupted = Arc::new(AtomicBool::new(false));
    let signal = interrupted.clone();
    ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))
        .map_err(|error| io::Error::other(format!("Ctrl-C handler: {error}")))?;
    let tests_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let core_dir = tests_dir.parent().expect("tests under core");
    let core_bin = match &args.core_bin {
        Some(path) => fs::canonicalize(path)?,
        None => build_core(core_dir)?,
    };
    let fixture_bin = core_bin
        .parent()
        .expect("core has parent")
        .join(format!("hopp_test_fixture{}", std::env::consts::EXE_SUFFIX));
    let out = tests_dir.join("out").join(format!(
        "smoke-{}-{}",
        std::process::id(),
        rand::random::<u32>()
    ));
    fs::create_dir_all(&out)?;
    println!("Logs: {}", out.display());
    let url = std::env::var("LIVEKIT_URL").unwrap_or_else(|_| "ws://127.0.0.1:7880".into());
    let key = std::env::var("LIVEKIT_API_KEY").unwrap_or_else(|_| "devkey".into());
    let secret = std::env::var("LIVEKIT_API_SECRET").unwrap_or_else(|_| "secret".into());
    let (host, port) = livekit_address(&url)?;
    let mut _server = None;
    if !server_answers(&host, port) {
        if !matches!(host.as_str(), "localhost" | "127.0.0.1") || port != 7880 {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                "configured LiveKit server is unavailable; auto-start is only for localhost:7880",
            ));
        }
        let (stdout, stderr) = log_stdio(&out.join("livekit.log"))?;
        let mut command = Command::new("livekit-server");
        command.args([
            "--dev",
            "--node-ip",
            "127.0.0.1",
            "--rtc.enable_loopback_candidate",
        ]);
        #[cfg(target_os = "macos")]
        command.args([
            "--config-body",
            "rtc:\n  interfaces:\n    includes: [lo0]\n",
        ]);
        let mut server =
            ChildGuard::spawn(command.stdout(stdout).stderr(stderr)).map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("starting LiveKit (install livekit-server first): {error}"),
                )
            })?;
        let deadline = Instant::now() + Duration::from_secs(10);
        while !server_answers(&host, port) {
            if interrupted.load(Ordering::Relaxed)
                || server.0.try_wait()?.is_some()
                || Instant::now() >= deadline
            {
                return Err(io::Error::other("LiveKit did not start; see livekit.log"));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        _server = Some(server);
    }
    let session = format!("smoke{}x{}", std::process::id(), rand::random::<u32>());
    let mut _network = None;
    if let Some(profile) = &args.net {
        let network = NetworkGuard::new(tests_dir.join("netem.sh"), session.clone());
        let status = Command::new("sudo")
            .arg("-n")
            .arg(&network.script)
            .args([
                profile,
                "--remote",
                &host,
                "--session",
                &session,
                "--port",
                &port.to_string(),
            ])
            .status()?;
        if !status.success() {
            return Err(io::Error::other(
                "network shaping failed; grant sudo credentials with sudo -v before --net",
            ));
        }
        _network = Some(network);
    }
    let mut failures = 0;
    let mut skips = 0;
    for (index, scenario) in scenarios.iter().enumerate() {
        if interrupted.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "smoke run interrupted",
            ));
        }
        let result = run_scenario(
            &args,
            &core_bin,
            &fixture_bin,
            &out,
            scenario,
            index,
            &url,
            &key,
            &secret,
            &host,
            port,
            &session,
            &interrupted,
        );
        match result {
            Ok(Outcome::Pass) => println!("PASS {scenario}"),
            Ok(Outcome::Skip) if !args.require_devices => {
                skips += 1;
                println!("SKIP {scenario} (see scenario log)");
            }
            Ok(Outcome::Skip) => {
                failures += 1;
                println!("FAIL {scenario}: required device coverage was skipped");
            }
            Err(error) => {
                failures += 1;
                println!("FAIL {scenario}: {error}");
            }
        }
    }
    println!(
        "{} passed, {skips} skipped, {failures} failed; logs: {}",
        scenarios.len() - failures - skips,
        out.display()
    );
    if failures > 0 {
        Err(io::Error::other(format!(
            "{failures} smoke scenarios failed"
        )))
    } else {
        Ok(())
    }
}

enum Outcome {
    Pass,
    Skip,
}

fn run_scenario(
    args: &SmokeArgs,
    core_bin: &Path,
    fixture_bin: &Path,
    out: &Path,
    scenario: &str,
    index: usize,
    url: &str,
    key: &str,
    secret: &str,
    host: &str,
    port: u16,
    session: &str,
    interrupted: &AtomicBool,
) -> io::Result<Outcome> {
    println!("Running {scenario} ({} cycles)...", args.cycles);
    // The parent owns this lease too, because a timed-out child cannot run Drop.
    let _drop_network = (scenario == "network-drop").then(|| {
        NetworkGuard::new(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("netem.sh"),
            format!("{session}-drop"),
        )
    });
    let socket =
        std::env::temp_dir().join(format!("hopp-smoke-{}-{index}.sock", std::process::id()));
    #[cfg(windows)]
    let socket = PathBuf::from(format!(
        "127.0.0.1:{}",
        20000 + rand::random::<u16>() % 30000
    ));
    let _ = fs::remove_file(&socket);
    let _socket_guard = SocketGuard(socket.clone());
    let (stdout, stderr) = log_stdio(&out.join(format!("{scenario}.core.log")))?;
    let mut core = ChildGuard::spawn(
        Command::new(core_bin)
            .args(["--socket-path", &socket.to_string_lossy()])
            .env("RUST_LOG", "hopp_core=info")
            .stdout(stdout)
            .stderr(stderr),
    )?;
    let deadline = Instant::now() + Duration::from_secs(args.timeout_seconds);
    let ready_deadline = deadline.min(Instant::now() + Duration::from_secs(15));
    loop {
        #[cfg(not(windows))]
        let ready = socket.exists();
        #[cfg(windows)]
        let ready = true; // The child retries connecting; a readiness connection would consume core's sole client.
        if ready {
            break;
        }
        if core.0.try_wait()?.is_some()
            || Instant::now() >= ready_deadline
            || interrupted.load(Ordering::Relaxed)
        {
            return Err(io::Error::other(
                "core did not open its socket; see core log",
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let (stdout, stderr) = log_stdio(&out.join(format!("{scenario}.log")))?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args([
            "--socket-path",
            &socket.to_string_lossy(),
            "smoke-scenario",
            scenario,
            "--cycles",
            &args.cycles.to_string(),
        ])
        .env("LIVEKIT_URL", url)
        .env("LIVEKIT_API_KEY", key)
        .env("LIVEKIT_API_SECRET", secret)
        .env("HOPP_TEST_ROOM", format!("{session}-{index}"))
        .env("HOPP_TEST_FIXTURE_BIN", fixture_bin)
        .env("HOPP_TEST_NETEM_HOST", host)
        .env("HOPP_TEST_NETEM_PORT", port.to_string())
        .env("HOPP_TEST_NETEM_SESSION", session)
        .stdout(stdout)
        .stderr(stderr);
    if args.require_devices {
        command.arg("--require-devices");
    }
    let mut harness = ChildGuard::spawn(&mut command)?;
    let status = wait_child(&mut harness.0, deadline, interrupted)?;
    let shutdown_deadline = deadline.min(Instant::now() + Duration::from_secs(5));
    let core_status = wait_child(&mut core.0, shutdown_deadline, interrupted).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("core did not exit after harness disconnect: {error}"),
        )
    })?;
    let _ = fs::remove_file(&socket);
    if !core_status.success() {
        return Err(io::Error::other(format!(
            "core exited unexpectedly: {core_status}"
        )));
    }
    if fs::read_to_string(out.join(format!("{scenario}.core.log")))?.contains("panicked at") {
        return Err(io::Error::other("core log contains a panic"));
    }
    match status.code() {
        Some(0) => Ok(Outcome::Pass),
        Some(77) => Ok(Outcome::Skip),
        _ => Err(io::Error::other(format!(
            "scenario exited {status}; see {scenario}.log"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sudo_keepalive_stops_without_waiting_for_its_interval() {
        let (stop, thread) = keep_sudo_alive();
        let started = Instant::now();
        stop.send(()).unwrap();
        thread.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn parses_livekit_hosts_without_confusing_ipv6_and_ports() {
        assert_eq!(
            livekit_address("ws://localhost:7880/path").unwrap(),
            ("localhost".into(), 7880)
        );
        assert_eq!(
            livekit_address("ws://[::1]:7881").unwrap(),
            ("::1".into(), 7881)
        );
        assert_eq!(
            livekit_address("wss://example.com/path").unwrap(),
            ("example.com".into(), 443)
        );
        assert_eq!(
            livekit_address("ws://localhost").unwrap(),
            ("localhost".into(), 80)
        );
        for invalid in [
            "https://localhost:7880",
            "ws://[::1]junk",
            "ws://[::1]:",
            "ws://[::1]:0",
            "ws://::1",
            "ws://:7880",
            "ws://host:abc",
        ] {
            assert!(livekit_address(invalid).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    #[cfg(unix)]
    fn watchdog_times_out_a_stuck_child() {
        let mut child = ChildGuard::spawn(Command::new("/bin/sleep").arg("30")).unwrap();
        let result = wait_child(
            &mut child.0,
            Instant::now() + Duration::from_millis(100),
            &AtomicBool::new(false),
        );
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
        drop(child);
    }
}
