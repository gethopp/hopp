use crate::screenshare_client;
use socket_lib::{DrawingEnabled, Message};
use std::{io, time::Duration};

pub fn test_local_drawing_permanent() -> io::Result<()> {
    println!("\n=== TEST: Local Drawing (Permanent) ===");

    // Start screenshare session
    let (mut socket, _) = screenshare_client::start_screenshare_session()?;

    // Enable permanent drawing mode
    socket.send_message(Message::DrawingEnabled(DrawingEnabled { permanent: true }))?;
    println!("Permanent drawing enabled. Draw with mouse, press Escape to exit.");
    println!("You have 15 seconds to test drawing...");

    // Wait for user to test manually
    std::thread::sleep(Duration::from_secs(15));

    // Stop screenshare
    screenshare_client::stop_screenshare_session(&mut socket)?;

    println!("Test completed.");
    Ok(())
}

pub fn test_local_drawing_non_permanent() -> io::Result<()> {
    println!("\n=== TEST: Local Drawing (Non-Permanent) ===");

    // Start screenshare session
    let (mut socket, _) = screenshare_client::start_screenshare_session()?;

    // Enable non-permanent drawing mode
    socket.send_message(Message::DrawingEnabled(DrawingEnabled { permanent: false }))?;
    println!("Non-permanent drawing enabled. Draw with mouse, press Escape to exit.");
    println!("You have 15 seconds to test drawing...");

    // Wait for user to test manually
    std::thread::sleep(Duration::from_secs(15));

    // Stop screenshare
    screenshare_client::stop_screenshare_session(&mut socket)?;

    println!("Test completed.");
    Ok(())
}
