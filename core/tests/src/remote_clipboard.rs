use crate::events::{AddToClipboardData, ClientEvent, ClipboardPayload, PasteFromClipboardData};
use crate::screenshare_client;
use livekit::prelude::*;
use std::{io, time::Duration};
use tokio::time::sleep;

/// Sends a paste from clipboard event via the LiveKit data channel.
async fn send_paste_from_clipboard(room: &Room, data: Option<ClipboardPayload>) -> io::Result<()> {
    let paste_data = PasteFromClipboardData { data };
    let event = ClientEvent::PasteFromClipboard(paste_data);
    let payload = serde_json::to_vec(&event).map_err(io::Error::other)?;

    room.local_participant()
        .publish_data(DataPacket {
            payload,
            reliable: true,
            ..Default::default()
        })
        .await
        .map_err(io::Error::other)?;

    Ok(())
}

/// Sends an add to clipboard event via the LiveKit data channel.
async fn send_add_to_clipboard(room: &Room, is_copy: bool) -> io::Result<()> {
    let add_to_clipboard_data = AddToClipboardData { is_copy };
    let event = ClientEvent::AddToClipboard(add_to_clipboard_data);
    let payload = serde_json::to_vec(&event).map_err(io::Error::other)?;

    room.local_participant()
        .publish_data(DataPacket {
            payload,
            reliable: true,
            ..Default::default()
        })
        .await
        .map_err(io::Error::other)?;

    Ok(())
}

/// Tests PasteFromClipboard with a single payload.
/// This test sends a single clipboard packet and verifies it was set correctly.
async fn internal_test_paste_single_payload(room: &Room) -> io::Result<()> {
    println!("\n=== Testing PasteFromClipboard with Single Payload ===");

    let test_text = "Hello, this is a test clipboard content!";
    println!("Test text: '{}'", test_text);

    let clipboard_payload = ClipboardPayload {
        packet_id: 0,
        total_packets: 1,
        data: test_text.as_bytes().to_vec(),
    };

    println!("Sending single clipboard packet...");
    send_paste_from_clipboard(room, Some(clipboard_payload)).await?;

    // Wait for the clipboard to be set and paste operation to complete
    println!("Waiting for clipboard to be set and paste to execute...");
    sleep(Duration::from_millis(500)).await;

    // Verify clipboard content with retries
    println!("Verifying clipboard content (will retry up to 5 times)...");
    let mut clipboard = arboard::Clipboard::new().map_err(io::Error::other)?;
    let max_attempts = 5;
    let mut success = false;

    for attempt in 1..=max_attempts {
        println!("  Attempt {}/{}...", attempt, max_attempts);
        match clipboard.get_text() {
            Ok(clipboard_content) => {
                if clipboard_content == test_text {
                    println!(
                        "✓ SUCCESS: Clipboard content matches expected text on attempt {}",
                        attempt
                    );
                    println!("  Expected: '{}'", test_text);
                    println!("  Got:      '{}'", clipboard_content);
                    success = true;
                    break;
                } else {
                    println!("  Clipboard content does not match (attempt {}):", attempt);
                    println!("    Expected: '{}'", test_text);
                    println!("    Got:      '{}'", clipboard_content);
                }
            }
            Err(e) => {
                println!("  Could not read clipboard (attempt {}): {:?}", attempt, e);
            }
        }

        if attempt < max_attempts {
            println!("  Waiting 1 second before retry...");
            sleep(Duration::from_secs(1)).await;
        }
    }

    if !success {
        println!(
            "✗ FAILURE: Clipboard content did not match after {} attempts",
            max_attempts
        );
        return Err(io::Error::other("Clipboard content mismatch after retries"));
    }

    println!("Single payload test completed successfully.\n");
    Ok(())
}

/// Tests PasteFromClipboard with multiple payloads.
/// This test sends multiple clipboard packets and verifies they were combined correctly.
async fn internal_test_paste_multiple_payloads(room: &Room) -> io::Result<()> {
    println!("\n=== Testing PasteFromClipboard with Multiple Payloads ===");

    // Create a longer text that will be split into multiple packets
    let test_text = "This is packet 1. This is packet 2. This is packet 3. All combined!";
    println!("Test text: '{}'", test_text);

    // Split the text into 3 packets
    let packet1_text = "This is packet 1. ";
    let packet2_text = "This is packet 2. ";
    let packet3_text = "This is packet 3. All combined!";

    let payloads = vec![
        ClipboardPayload {
            packet_id: 0,
            total_packets: 3,
            data: packet1_text.as_bytes().to_vec(),
        },
        ClipboardPayload {
            packet_id: 1,
            total_packets: 3,
            data: packet2_text.as_bytes().to_vec(),
        },
        ClipboardPayload {
            packet_id: 2,
            total_packets: 3,
            data: packet3_text.as_bytes().to_vec(),
        },
    ];

    println!("Sending {} clipboard packets...", payloads.len());
    for (idx, payload) in payloads.iter().enumerate() {
        println!("  Sending packet {}/{}", idx + 1, payloads.len());
        send_paste_from_clipboard(room, Some(payload.clone())).await?;
        // Small delay between packets to simulate realistic network conditions
        sleep(Duration::from_millis(50)).await;
    }

    // Wait for the clipboard to be set and paste operation to complete
    println!("Waiting for clipboard to be set and paste to execute...");
    sleep(Duration::from_millis(500)).await;

    // Verify clipboard content with retries
    println!("Verifying clipboard content (will retry up to 5 times)...");
    let mut clipboard = arboard::Clipboard::new().map_err(io::Error::other)?;
    let max_attempts = 5;
    let mut success = false;

    for attempt in 1..=max_attempts {
        println!("  Attempt {}/{}...", attempt, max_attempts);
        match clipboard.get_text() {
            Ok(clipboard_content) => {
                if clipboard_content == test_text {
                    println!(
                        "✓ SUCCESS: Clipboard content matches expected text on attempt {}",
                        attempt
                    );
                    println!("  Expected: '{}'", test_text);
                    println!("  Got:      '{}'", clipboard_content);
                    success = true;
                    break;
                } else {
                    println!("  Clipboard content does not match (attempt {}):", attempt);
                    println!("    Expected: '{}'", test_text);
                    println!("    Got:      '{}'", clipboard_content);
                }
            }
            Err(e) => {
                println!("  Could not read clipboard (attempt {}): {:?}", attempt, e);
            }
        }

        if attempt < max_attempts {
            println!("  Waiting 1 second before retry...");
            sleep(Duration::from_secs(1)).await;
        }
    }

    if !success {
        println!(
            "✗ FAILURE: Clipboard content did not match after {} attempts",
            max_attempts
        );
        return Err(io::Error::other("Clipboard content mismatch after retries"));
    }

    println!("Multiple payload test completed successfully.\n");
    Ok(())
}

/// Tests AddToClipboard functionality.
/// This test simulates a user selecting text and copying/cutting it.
async fn internal_test_add_to_clipboard(room: &Room) -> io::Result<()> {
    println!("\n=== Testing AddToClipboard (Copy) ===");

    println!("Please select some text in any application within the next 5 seconds...");
    println!("(e.g., highlight text in a text editor, browser, or terminal)");

    // Give user time to select text
    for i in (1..=5).rev() {
        println!("  {} seconds remaining...", i);
        sleep(Duration::from_secs(1)).await;
    }

    println!("\nSending AddToClipboard event (Copy)...");
    send_add_to_clipboard(room, true).await?;

    // Wait for the copy operation to complete
    println!("Waiting for copy operation to complete...");
    sleep(Duration::from_millis(500)).await;

    // Read what was copied
    println!("Reading clipboard content...");
    let mut clipboard = arboard::Clipboard::new().map_err(io::Error::other)?;
    let copied_content = match clipboard.get_text() {
        Ok(content) => {
            println!("✓ SUCCESS: Clipboard content retrieved");
            println!("  Clipboard now contains: '{}'", content);
            if content.is_empty() {
                println!("  ⚠ Warning: Clipboard is empty. Did you select any text?");
                return Ok(());
            }
            content
        }
        Err(e) => {
            println!("✗ FAILURE: Could not read clipboard: {:?}", e);
            return Err(io::Error::other(format!("Clipboard read error: {:?}", e)));
        }
    };

    // Now test paste with empty payload (should trigger Cmd+V/Ctrl+V)
    println!("\nSending PasteFromClipboard event (empty payload to trigger paste)...");
    send_paste_from_clipboard(room, None).await?;

    println!("Waiting for paste operation to complete...");
    sleep(Duration::from_millis(500)).await;

    println!("✓ Paste command sent successfully");
    println!(
        "  The copied text ('{}') should now be pasted in the active application",
        copied_content
    );

    println!("\nAddToClipboard (Copy) test completed.\n");
    Ok(())
}

/// Tests AddToClipboard functionality with Cut operation.
async fn internal_test_add_to_clipboard_cut(room: &Room) -> io::Result<()> {
    println!("\n=== Testing AddToClipboard (Cut) ===");

    println!("Please select some text in any editable application within the next 5 seconds...");
    println!("(e.g., highlight text in a text editor where it can be cut)");
    println!("⚠ Note: The selected text will be removed (cut operation)");

    // Give user time to select text
    for i in (1..=5).rev() {
        println!("  {} seconds remaining...", i);
        sleep(Duration::from_secs(1)).await;
    }

    println!("\nSending AddToClipboard event (Cut)...");
    send_add_to_clipboard(room, false).await?;

    // Wait for the cut operation to complete
    println!("Waiting for cut operation to complete...");
    sleep(Duration::from_millis(500)).await;

    // Read what was cut
    println!("Reading clipboard content...");
    let mut clipboard = arboard::Clipboard::new().map_err(io::Error::other)?;
    let cut_content = match clipboard.get_text() {
        Ok(content) => {
            println!("✓ SUCCESS: Clipboard content retrieved");
            println!("  Clipboard now contains: '{}'", content);
            if content.is_empty() {
                println!("  ⚠ Warning: Clipboard is empty. Did you select any text?");
                return Ok(());
            }
            content
        }
        Err(e) => {
            println!("✗ FAILURE: Could not read clipboard: {:?}", e);
            return Err(io::Error::other(format!("Clipboard read error: {:?}", e)));
        }
    };

    // Now test paste with empty payload (should trigger Cmd+V/Ctrl+V)
    println!("\nSending PasteFromClipboard event (empty payload to trigger paste)...");
    send_paste_from_clipboard(room, None).await?;

    println!("Waiting for paste operation to complete...");
    sleep(Duration::from_millis(500)).await;

    println!("✓ Paste command sent successfully");
    println!(
        "  The cut text ('{}') should now be pasted in the active application",
        cut_content
    );

    println!("\nAddToClipboard (Cut) test completed.\n");
    Ok(())
}

/// Public function to test paste with single payload only.
pub async fn test_paste_single() -> io::Result<()> {
    println!("Starting paste single payload test...");
    let (sender, _event_socket, _) = screenshare_client::start_screenshare_session()?;

    sleep(Duration::from_secs(2)).await;

    let token = crate::livekit_utils::generate_token("Test Clipboard Paste Single");
    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");

    let (room, mut _rx) = Room::connect(&url, &token, RoomOptions::default())
        .await
        .unwrap();
    sleep(Duration::from_secs(5)).await;
    println!("Connected to room: {}", room.name());

    internal_test_paste_single_payload(&room).await?;

    println!("Stopping screenshare...");
    screenshare_client::stop_screenshare(&sender)?;
    println!("Screenshare stopped.");
    Ok(())
}

/// Public function to test paste with multiple payloads only.
pub async fn test_paste_multiple() -> io::Result<()> {
    println!("Starting paste multiple payloads test...");
    let (sender, _event_socket, _) = screenshare_client::start_screenshare_session()?;

    sleep(Duration::from_secs(2)).await;

    let token = crate::livekit_utils::generate_token("Test Clipboard Paste Multiple");
    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");

    let (room, mut _rx) = Room::connect(&url, &token, RoomOptions::default())
        .await
        .unwrap();
    sleep(Duration::from_secs(5)).await;
    println!("Connected to room: {}", room.name());

    internal_test_paste_multiple_payloads(&room).await?;

    println!("Stopping screenshare...");
    screenshare_client::stop_screenshare(&sender)?;
    println!("Screenshare stopped.");
    Ok(())
}

/// Public function to test add to clipboard (copy) only.
pub async fn test_add_copy() -> io::Result<()> {
    println!("Starting add to clipboard (copy) test...");
    let (sender, _event_socket, _) = screenshare_client::start_screenshare_session()?;

    sleep(Duration::from_secs(2)).await;

    let token = crate::livekit_utils::generate_token("Test Clipboard Add Copy");
    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");

    let (room, mut _rx) = Room::connect(&url, &token, RoomOptions::default())
        .await
        .unwrap();
    sleep(Duration::from_secs(5)).await;
    println!("Connected to room: {}", room.name());

    internal_test_add_to_clipboard(&room).await?;

    println!("Stopping screenshare...");
    screenshare_client::stop_screenshare(&sender)?;
    println!("Screenshare stopped.");
    Ok(())
}

/// Public function to test add to clipboard (cut) only.
pub async fn test_add_cut() -> io::Result<()> {
    println!("Starting add to clipboard (cut) test...");
    let (sender, _event_socket, _) = screenshare_client::start_screenshare_session()?;

    sleep(Duration::from_secs(2)).await;

    let token = crate::livekit_utils::generate_token("Test Clipboard Add Cut");
    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");

    let (room, mut _rx) = Room::connect(&url, &token, RoomOptions::default())
        .await
        .unwrap();
    sleep(Duration::from_secs(5)).await;
    println!("Connected to room: {}", room.name());

    internal_test_add_to_clipboard_cut(&room).await?;

    println!("Stopping screenshare...");
    screenshare_client::stop_screenshare(&sender)?;
    println!("Screenshare stopped.");
    Ok(())
}
