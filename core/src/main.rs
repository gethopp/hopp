use std::env;

use clap::Parser;
use hopp_core::{RenderEventLoop, RenderLoopRunArgs};
use sentry_utils::init_sentry;

/// Hopp Core - Remote Desktop Control System
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the cursor texture file
    #[arg(short, long)]
    textures_path: Option<String>,

    /// Sentry DSN
    #[arg(short, long)]
    sentry_dsn: Option<String>,

    /// Socket name
    #[arg(long)]
    socket_path: Option<String>,
}

// test
fn main() -> Result<(), impl std::error::Error> {
    let args = Args::parse();

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let _guard = init_sentry("Core crashed".to_string(), args.sentry_dsn);

    #[cfg(target_os = "linux")]
    {
        /* This is needed for getting the system picker for screen sharing. */
        use glib::MainLoop;
        let main_loop = MainLoop::new(None, false);
        let _handle = std::thread::spawn(move || {
            main_loop.run();
        });
    }

    let textures_path = match args.textures_path {
        Some(path) => path,
        None => {
            let manifest_dir = env::var("CARGO_MANIFEST_DIR")
                .expect("CARGO_MANIFEST_DIR is not set, you need to set the textures_path");
            format!("{manifest_dir}/resources")
        }
    };

    let socket_path = match args.socket_path {
        Some(path) => path,
        None => std::env::temp_dir()
            .join("core-socket")
            .to_string_lossy()
            .to_string(),
    };

    let input_args = RenderLoopRunArgs { textures_path };

    let render_event_loop = RenderEventLoop::new();
    render_event_loop.run(input_args, socket_path)
}
