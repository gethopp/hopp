use crate::events::{ClientEvent, KeystrokeData};
use crate::screenshare_client;
use livekit::prelude::*;
use std::{io, time::Duration};
use tokio::time::sleep;

/// Sends a keystroke event (down and up) via the LiveKit data channel.
async fn send_keystroke(room: &Room, key: &str, shift: bool, down: bool) -> io::Result<()> {
    let keystroke_data = KeystrokeData {
        key: vec![key.to_string()], // Assuming single key for now
        meta: false,
        ctrl: false,
        shift,
        alt: false,
        down,
    };
    let event = ClientEvent::Keystroke(keystroke_data);
    let payload = serde_json::to_vec(&event).map_err(io::Error::other)?;
    room.local_participant()
        .publish_data(DataPacket {
            payload,
            reliable: true, // Keystrokes should be reliable
            ..Default::default()
        })
        .await
        .map_err(io::Error::other)?;
    Ok(())
}

/// Simulates pressing and releasing a key, optionally with Shift.
async fn simulate_key_press(room: &Room, key: &str, shift: bool) -> io::Result<()> {
    println!("Sending KeyDown: '{key}', Shift: {shift}");
    send_keystroke(room, key, shift, true).await?; // Key Down
    sleep(Duration::from_millis(50)).await; // Small delay
    println!("Sending KeyUp: '{key}', Shift: {shift}");
    send_keystroke(room, key, shift, false).await?; // Key Up
    sleep(Duration::from_millis(100)).await; // Delay between different keys
    Ok(())
}

/// Tests sending various keyboard characters with and without Shift.
async fn internal_test_keyboard_chars(room: &Room) -> io::Result<()> {
    // Define characters to test
    let chars = "abcdefghijklmnopqrstuvwxyz0123456789`-=[]\\;',./";
    let shifted_chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZ)!@#$%^&*(_+{}|:\"<>?"; // Corresponding shifted chars

    println!("Testing lowercase and numbers/symbols...");
    for char_code in chars.chars() {
        simulate_key_press(room, &char_code.to_string(), false).await?;
        sleep(Duration::from_millis(50)).await; // Small delay
    }

    simulate_key_press(room, "Enter", false).await?;

    println!("Testing uppercase and shifted symbols...");
    for shifted_char_code in shifted_chars.chars() {
        simulate_key_press(room, &shifted_char_code.to_string(), true).await?;
        sleep(Duration::from_millis(50)).await; // Small delay
    }

    // Test some special keys (names might vary depending on interpretation)
    println!("Testing special keys...");
    let special_keys = [
        "Tab",
        "Backspace",
        "Delete",
        "ArrowUp",
        "ArrowDown",
        "ArrowLeft",
        "ArrowRight",
        "Escape",
        " ",
    ]; // Space added
    for key in special_keys.iter() {
        simulate_key_press(room, key, false).await?;
        sleep(Duration::from_millis(50)).await; // Small delay
    }

    println!("Keyboard character test finished.");
    Ok(())
}

/// Connects screenshare, runs the keyboard character test, and stops screenshare.
pub async fn test_keyboard_chars() -> io::Result<()> {
    println!("Starting keyboard test...");
    let (sender, _event_socket, _) = screenshare_client::start_screenshare_session()?;

    sleep(Duration::from_secs(2)).await; // Give time for screenshare to potentially start

    let token = crate::livekit_utils::generate_token("Test Keyboard");
    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");

    let (room, mut _rx) = Room::connect(&url, &token, RoomOptions::default())
        .await
        .unwrap();
    println!("Connected to room: {}", room.name());

    internal_test_keyboard_chars(&room).await?;

    println!("Stopping screenshare...");
    screenshare_client::stop_screenshare(&sender)?;
    println!("Screenshare stopped.");
    println!("Keyboard test complete.");
    Ok(())
}
