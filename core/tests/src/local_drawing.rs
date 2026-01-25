use crate::screenshare_client;
use socket_lib::Message;
use std::{io, time::Duration};

pub fn test_local_drawing() -> io::Result<()> {
    println!("\n=== TEST: Local Drawing ===");

    // Start screenshare session
    let (mut socket, _) = screenshare_client::start_screenshare_session()?;

    // Enable drawing mode
    socket.send_message(Message::DrawingEnabled(true))?;
    println!("Drawing enabled. Draw with mouse, press Escape to exit.");
    println!("You have 30 seconds to test drawing...");

    // Wait for user to test manually
    std::thread::sleep(Duration::from_secs(30));

    // Stop screenshare
    screenshare_client::stop_screenshare_session(&mut socket)?;

    println!("Test completed.");
    Ok(())
}
