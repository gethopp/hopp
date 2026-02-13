use crate::screenshare_client;
use std::{io, time::Duration};

/// Opens the camera window and keeps it alive for manual interaction.
///
/// Usage: `cargo run -- camera open`
///
/// Requires a running core process (`task dev` in core/).
/// The camera window will stay open for 30 seconds.
pub fn test_open_camera() -> io::Result<()> {
    println!("\n=== TEST: Open Camera Window ===");

    let mut socket = screenshare_client::connect_socket()?;
    println!("Connected to socket.");

    screenshare_client::open_camera(&mut socket)?;
    println!("OpenCamera sent. Camera window should appear.");
    println!("You have 15_000 seconds to interact with the window...");

    std::thread::sleep(Duration::from_secs(15_000));

    println!("Test completed.");
    Ok(())
}
