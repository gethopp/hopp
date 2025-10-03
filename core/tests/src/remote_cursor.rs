use crate::events::{ClickAnimation, ClientEvent, ClientPoint, MouseClickData, WheelDelta};
use crate::livekit_utils;
use crate::screenshare_client;
use livekit::prelude::*;
use rand::{rngs::StdRng, Rng, SeedableRng};
use socket_lib::Message;
use std::{io, time::Duration};
use tokio::time::sleep;

/// Sends a mouse click event (down and up) via the LiveKit data channel.
async fn send_mouse_click(room: &Room, x: f64, y: f64, button: u32) -> io::Result<()> {
    // Mouse Down
    let click_down_data = MouseClickData {
        x,
        y,
        button, // 0 for left, 2 for right
        clicks: 1,
        down: true,
        shift: false,
        meta: false,
        ctrl: false,
        alt: false,
    };
    let event_down = ClientEvent::MouseClick(click_down_data);
    let payload_down = serde_json::to_vec(&event_down).map_err(io::Error::other)?;
    room.local_participant()
        .publish_data(DataPacket {
            payload: payload_down,
            reliable: true, // Clicks should be reliable
            ..Default::default()
        })
        .await
        .map_err(io::Error::other)?;

    // Short delay between down and up for realism? Maybe not needed.
    sleep(Duration::from_millis(50)).await;

    // Mouse Up
    let click_up_data = MouseClickData {
        x,
        y,
        button,
        clicks: 1,
        down: false, // Mouse up
        shift: false,
        meta: false,
        ctrl: false,
        alt: false,
    };
    let event_up = ClientEvent::MouseClick(click_up_data);
    let payload_up = serde_json::to_vec(&event_up).map_err(io::Error::other)?;
    room.local_participant()
        .publish_data(DataPacket {
            payload: payload_up,
            reliable: true,
            ..Default::default()
        })
        .await
        .map_err(io::Error::other)?;

    Ok(())
}

/// Sends a ClickAnimation event via the LiveKit data channel.
async fn send_click_animation(room: &Room, enabled: bool) -> io::Result<()> {
    let click_animation_data = ClickAnimation { enabled };
    let event = ClientEvent::ClickAnimation(click_animation_data);
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

/// Smoothly moves the cursor from one position to another
async fn smooth_cursor_move(
    room: &Room,
    from_x: f64,
    from_y: f64,
    to_x: f64,
    to_y: f64,
) -> io::Result<()> {
    let steps = 30; // Number of steps for smooth movement
    let delay = Duration::from_millis(16); // ~60fps

    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let x = from_x + (to_x - from_x) * t;
        let y = from_y + (to_y - from_y) * t;

        send_mouse_move(room, x, y).await?;
        sleep(delay).await;
    }

    Ok(())
}

/// Sends a mouse move event via the LiveKit data channel.
async fn send_mouse_move(room: &Room, x: f64, y: f64) -> io::Result<()> {
    let point = ClientPoint {
        x,
        y,
        pointer: false,
    };
    let event = ClientEvent::MouseMove(point);
    let payload = serde_json::to_vec(&event).map_err(io::Error::other)?;
    room.local_participant()
        .publish_data(DataPacket {
            payload,
            reliable: false, // Mouse movements can tolerate some loss
            ..Default::default()
        })
        .await
        .map_err(io::Error::other)?;
    Ok(())
}

/// Sends a mouse wheel event via the LiveKit data channel.
async fn send_mouse_wheel(room: &Room, delta_x: f64, delta_y: f64) -> io::Result<()> {
    let wheel_delta = WheelDelta {
        deltaX: delta_x,
        deltaY: delta_y,
    };
    let event = ClientEvent::WheelEvent(wheel_delta);
    let payload = serde_json::to_vec(&event).map_err(io::Error::other)?;
    room.local_participant()
        .publish_data(DataPacket {
            payload,
            reliable: false, // Wheel events can tolerate some loss
            ..Default::default()
        })
        .await
        .map_err(io::Error::other)?;
    Ok(())
}

async fn internal_cursor_move(
    room: &Room,
    start_x: f64,
    start_y: f64,
    side_length: f64,
) -> io::Result<()> {
    let step = 0.005;
    let delay = Duration::from_millis(20); // Small delay between movements

    // Starting position
    let mut x = start_x;
    let mut y = start_y;

    println!("Starting square at ({x}, {y})");

    // Move to starting position
    send_mouse_move(room, x, y).await?;
    sleep(delay).await;

    // Side 1: Move right (top edge of square)
    let target_x1 = start_x + side_length;
    while x < target_x1 {
        x += step;
        if x > target_x1 {
            x = target_x1;
        }
        send_mouse_move(room, x, y).await?;
        sleep(delay).await;
    }
    println!("Completed top edge, reached ({x}, {y})");

    // Side 2: Move down (right edge of square)
    let target_y1 = start_y + side_length;
    while y < target_y1 {
        y += step;
        if y > target_y1 {
            y = target_y1;
        }
        send_mouse_move(room, x, y).await?;
        sleep(delay).await;
    }
    println!("Completed right edge, reached ({x}, {y})");

    // Side 3: Move left (bottom edge of square)
    while x > start_x {
        x -= step;
        if x < start_x {
            x = start_x;
        }
        send_mouse_move(room, x, y).await?;
        sleep(delay).await;
    }
    println!("Completed bottom edge, reached ({x}, {y})");

    // Side 4: Move up (left edge of square) - back to start
    while y > start_y {
        y -= step;
        if y < start_y {
            y = start_y;
        }
        send_mouse_move(room, x, y).await?;
        sleep(delay).await;
    }
    println!("Completed left edge, back to start ({x}, {y})");

    Ok(())
}

async fn internal_cursor_click(room: &Room, x: f64, y: f64) -> io::Result<()> {
    let delay = Duration::from_millis(20); // Small delay between movements
    println!("Performing left click at ({x}, {y})");
    send_mouse_click(room, x, y, 0).await?; // 0 for left button
    sleep(Duration::from_secs(3)).await;

    println!("Performing right click at ({x}, {y})");
    send_mouse_click(room, x, y, 2).await?; // 2 for right button
    sleep(Duration::from_secs(3)).await;

    let x = x + 0.1;
    println!("Moving right to ({x}, {y})");
    send_mouse_move(room, x, y).await?;
    sleep(delay).await;

    println!("Performing left click at ({x}, {y})");
    send_mouse_click(room, x, y, 0).await?;
    sleep(Duration::from_secs(1)).await;
    Ok(())
}

async fn internal_cursor_scroll(room: &Room) -> io::Result<()> {
    send_mouse_move(room, 0.3, 0.5).await?;

    let scroll_amount = 5.0;
    let delay = Duration::from_millis(10);

    println!("Scrolling down...");
    for _i in 0..50 {
        send_mouse_wheel(room, 0.0, scroll_amount).await?;
        sleep(delay).await;
    }

    println!("Scrolling up...");
    for _i in 0..50 {
        send_mouse_wheel(room, 0.0, -scroll_amount).await?;
        sleep(delay).await;
    }

    println!("Scrolling right...");
    for _i in 0..50 {
        send_mouse_wheel(room, scroll_amount, 0.0).await?;
        sleep(delay).await;
    }

    println!("Scrolling left...");
    for _i in 0..50 {
        send_mouse_wheel(room, -scroll_amount, 0.0).await?;
        sleep(delay).await;
    }

    println!("Scroll test finished.");
    Ok(())
}

/// Connects screenshare, simulates mouse movement via LiveKit, and stops screenshare.
pub async fn test_cursor() -> io::Result<()> {
    let (mut cursor_socket, _) = screenshare_client::start_screenshare_session()?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let token = livekit_utils::generate_token("Test Cursor");
    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");

    let (room, mut _rx) = Room::connect(&url, &token, RoomOptions::default())
        .await
        .unwrap();
    println!("Connected to room: {}", room.name());

    internal_cursor_move(&room, 0.0, 0.2, 0.5).await?;
    internal_cursor_click(&room, 0.7, 0.3).await?;
    internal_cursor_move(&room, 0.0, 0.2, 0.5).await?;
    internal_cursor_scroll(&room).await?;

    screenshare_client::stop_screenshare_session(&mut cursor_socket)?;

    Ok(())
}

pub async fn test_cursor_click(x: f64, y: f64) -> io::Result<()> {
    let (mut cursor_socket, _) = screenshare_client::start_screenshare_session()?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let token = livekit_utils::generate_token("Test Cursor Click");
    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");

    let (room, mut _rx) = Room::connect(&url, &token, RoomOptions::default())
        .await
        .unwrap();
    println!("Connected to room: {}", room.name());

    internal_cursor_click(&room, x, y).await?;
    screenshare_client::stop_screenshare_session(&mut cursor_socket)?;
    Ok(())
}

pub async fn test_cursor_move() -> io::Result<()> {
    let (mut cursor_socket, content) = screenshare_client::start_screenshare_session()?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let token = livekit_utils::generate_token("Test Cursor");
    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");

    let (room, mut _rx) = Room::connect(&url, &token, RoomOptions::default())
        .await
        .unwrap();
    println!("Connected to room: {}", room.name());

    let mut index = 0;
    // TODO: Do this for every test so we can test multiple resolutions
    while index + 1 < content.len() {
        println!("Moving to next content: {}", content[index].content.id);
        internal_cursor_move(&room, 0.0, 0.2, 0.5).await?;
        screenshare_client::stop_screenshare_session(&mut cursor_socket)?;
        index += 1;
        let width = 1920.0;
        let height = 1080.0;
        screenshare_client::request_screenshare(
            &mut cursor_socket,
            content[index].content.id,
            width,
            height,
        )?;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    println!("Moving to next content: {}", content[index].content.id);
    internal_cursor_move(&room, 0.0, 0.2, 0.5).await?;

    Ok(())
}

/// Connects screenshare, simulates mouse scroll events via LiveKit, and stops screenshare.
pub async fn test_cursor_scroll() -> io::Result<()> {
    let (mut cursor_socket, _) = screenshare_client::start_screenshare_session()?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let token = livekit_utils::generate_token("Test Cursor Scroll");
    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");

    let (room, mut _rx) = Room::connect(&url, &token, RoomOptions::default())
        .await
        .unwrap();
    println!("Connected to room: {}", room.name());

    internal_cursor_scroll(&room).await?;
    screenshare_client::stop_screenshare_session(&mut cursor_socket)?;
    Ok(())
}

/// Multiple participants
pub async fn test_multiple_participants() -> io::Result<()> {
    // Start single screenshare session
    println!("Starting screenshare session...");
    let (mut cursor_socket, _) = screenshare_client::start_screenshare_session()?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Create 3 additional room connections for other participants
    let token_1 = livekit_utils::generate_token("Jane Doe");
    let token_2 = livekit_utils::generate_token("Mark Smith");
    let token_3 = livekit_utils::generate_token("John Wick");
    let token_4 = livekit_utils::generate_token("Amy Adams");

    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");
    let (room_1, mut _rx_1) = Room::connect(&url, &token_1, RoomOptions::default())
        .await
        .unwrap();
    let (room_2, mut _rx_2) = Room::connect(&url, &token_2, RoomOptions::default())
        .await
        .unwrap();
    let (room_3, mut _rx_3) = Room::connect(&url, &token_3, RoomOptions::default())
        .await
        .unwrap();
    let (room_4, mut _rx_4) = Room::connect(&url, &token_4, RoomOptions::default())
        .await
        .unwrap();

    println!("All 4 participants connected to room");

    println!("Screenshare session started. Beginning concurrent square drawing...");

    // Create tasks for each participant to draw squares concurrently
    let task_1 = tokio::spawn(async move {
        println!("Jane Doe: Drawing square in top-left quadrant");
        internal_cursor_move(&room_1, 0.1, 0.1, 0.3).await
    });

    let task_2 = tokio::spawn(async move {
        println!("Mark Smith: Drawing square in top-right quadrant");
        internal_cursor_move(&room_2, 0.6, 0.1, 0.3).await
    });

    let task_3 = tokio::spawn(async move {
        println!("John Wick: Drawing square in bottom-left quadrant");
        internal_cursor_move(&room_3, 0.1, 0.6, 0.3).await
    });

    let task_4 = tokio::spawn(async move {
        println!("Amy Adams: Drawing square in bottom-right quadrant");
        internal_cursor_move(&room_4, 0.6, 0.6, 0.3).await
    });

    // Wait for all participants to finish drawing their squares concurrently
    let results = tokio::try_join!(task_1, task_2, task_3, task_4);

    match results {
        Ok((res1, res2, res3, res4)) => {
            if let Err(e) = res1 {
                println!("Jane Doe encountered error: {e:?}");
            } else {
                println!("Jane Doe completed her square");
            }
            if let Err(e) = res2 {
                println!("Mark Smith encountered error: {e:?}");
            } else {
                println!("Mark Smith completed his square");
            }
            if let Err(e) = res3 {
                println!("John Wick encountered error: {e:?}");
            } else {
                println!("John Wick completed his square");
            }
            if let Err(e) = res4 {
                println!("Amy Adams encountered error: {e:?}");
            } else {
                println!("Amy Adams completed her square");
            }
        }
        Err(e) => {
            println!("Task execution error: {e:?}");
        }
    }

    println!("All squares completed concurrently. Stopping screenshare session...");

    // Stop the single screenshare session
    screenshare_client::stop_screenshare_session(&mut cursor_socket)?;

    println!("Screenshare session stopped.");

    Ok(())
}

/// Test where 4 participants take turns having control and all draw squares simultaneously
pub async fn test_multiple_cursors_with_control() -> io::Result<()> {
    // Start single screenshare session
    println!("Starting screenshare session for cursor control test...");
    let (mut cursor_socket, _) = screenshare_client::start_screenshare_session()?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");

    // Create persistent room connections for all participants
    let token_1 = livekit_utils::generate_token("Al");
    let token_2 = livekit_utils::generate_token("Robert");
    let token_3 = livekit_utils::generate_token("Christopher");
    let token_4 = livekit_utils::generate_token("Christopher Martin");

    let (room_1, _rx_1) = Room::connect(&url, &token_1, RoomOptions::default())
        .await
        .unwrap();
    let (room_2, _rx_2) = Room::connect(&url, &token_2, RoomOptions::default())
        .await
        .unwrap();
    let (room_3, _rx_3) = Room::connect(&url, &token_3, RoomOptions::default())
        .await
        .unwrap();
    let (room_4, _rx_4) = Room::connect(&url, &token_4, RoomOptions::default())
        .await
        .unwrap();

    println!("All 4 participants connected with persistent connections.");

    // We need to wait for the textures to load
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    // Define quadrants for each participant
    let quadrants = [
        (0.1, 0.1), // Al: top-left
        (0.6, 0.1), // Robert: top-right
        (0.1, 0.6), // Christopher: bottom-left
        (0.6, 0.6), // Christopher Martin: bottom-right
    ];

    println!("Starting cursor control test with 4 rounds...");

    // Each participant runs through all 4 iterations concurrently
    let task_1 = tokio::spawn(async move {
        for round in 0..4 {
            println!("\n=== Al: ROUND {} ===", round + 1);

            // Al clicks to take control when it's her turn (round 0)
            if round == 0 {
                let (control_x, control_y) = quadrants[0];
                println!("Al clicking at ({control_x}, {control_y}) to take control");
                send_mouse_click(&room_1, control_x, control_y, 0).await?;
                sleep(Duration::from_millis(500)).await;
            }

            println!(
                "Al: Starting square at ({}, {})",
                quadrants[0].0, quadrants[0].1
            );
            internal_cursor_move(&room_1, quadrants[0].0, quadrants[0].1, 0.3).await?;
            println!("Al: Completed square for round {}", round + 1);

            // Pause between rounds
            if round < 3 {
                sleep(Duration::from_secs(1)).await;
            }
        }
        Ok::<(), io::Error>(())
    });

    let task_2 = tokio::spawn(async move {
        for round in 0..4 {
            println!("\n=== Robert: ROUND {} ===", round + 1);

            // Robert clicks to take control when it's his turn (round 1)
            if round == 1 {
                let (control_x, control_y) = quadrants[1];
                println!("Robert clicking at ({control_x}, {control_y}) to take control");
                send_mouse_click(&room_2, control_x, control_y, 0).await?;
                sleep(Duration::from_millis(500)).await;
            }

            println!(
                "Robert: Starting square at ({}, {})",
                quadrants[1].0, quadrants[1].1
            );
            internal_cursor_move(&room_2, quadrants[1].0, quadrants[1].1, 0.3).await?;
            println!("Robert: Completed square for round {}", round + 1);

            // Pause between rounds
            if round < 3 {
                sleep(Duration::from_secs(1)).await;
            }
        }
        Ok::<(), io::Error>(())
    });

    let task_3 = tokio::spawn(async move {
        for round in 0..4 {
            println!("\n=== Christopher: ROUND {} ===", round + 1);

            // Christopher clicks to take control when it's his turn (round 2)
            if round == 2 {
                let (control_x, control_y) = quadrants[2];
                println!("Christopher clicking at ({control_x}, {control_y}) to take control");
                send_mouse_click(&room_3, control_x, control_y, 0).await?;
                sleep(Duration::from_millis(500)).await;
            }

            println!(
                "Christopher: Starting square at ({}, {})",
                quadrants[2].0, quadrants[2].1
            );
            internal_cursor_move(&room_3, quadrants[2].0, quadrants[2].1, 0.3).await?;
            println!("Christopher: Completed square for round {}", round + 1);

            // Pause between rounds
            if round < 3 {
                sleep(Duration::from_secs(1)).await;
            }
        }
        Ok::<(), io::Error>(())
    });

    let task_4 = tokio::spawn(async move {
        for round in 0..4 {
            println!("\n=== Christopher Martin: ROUND {} ===", round + 1);

            // Christopher Martin clicks to take control when it's her turn (round 3)
            if round == 3 {
                let (control_x, control_y) = quadrants[3];
                println!(
                    "Christopher Martin clicking at ({control_x}, {control_y}) to take control"
                );
                send_mouse_click(&room_4, control_x, control_y, 0).await?;
                sleep(Duration::from_millis(500)).await;
            }

            println!(
                "Christopher Martin: Starting square at ({}, {})",
                quadrants[3].0, quadrants[3].1
            );
            internal_cursor_move(&room_4, quadrants[3].0, quadrants[3].1, 0.3).await?;
            println!(
                "Christopher Martin: Completed square for round {}",
                round + 1
            );

            // Pause between rounds
            if round < 3 {
                sleep(Duration::from_secs(1)).await;
            }
        }
        Ok::<(), io::Error>(())
    });

    // Wait for all participants to finish all their iterations
    let results = tokio::try_join!(task_1, task_2, task_3, task_4);

    match results {
        Ok((res1, res2, res3, res4)) => {
            if let Err(e) = res1 {
                println!("Al: Error in test: {e:?}");
            } else {
                println!("Al: Successfully completed all rounds");
            }
            if let Err(e) = res2 {
                println!("Robert: Error in test: {e:?}");
            } else {
                println!("Robert: Successfully completed all rounds");
            }
            if let Err(e) = res3 {
                println!("Christopher: Error in test: {e:?}");
            } else {
                println!("Christopher: Successfully completed all rounds");
            }
            if let Err(e) = res4 {
                println!("Christopher Martin: Error in test: {e:?}");
            } else {
                println!("Christopher Martin: Successfully completed all rounds");
            }
        }
        Err(e) => {
            println!("Task execution error: {e:?}");
            return Err(io::Error::other(e));
        }
    }

    println!("\n=== TEST COMPLETED ===");
    println!("All 4 rounds completed successfully. Each participant had control once.");

    // Stop the screenshare session
    screenshare_client::stop_screenshare_session(&mut cursor_socket)?;
    println!("Screenshare session stopped.");

    Ok(())
}

/// Test cursor hiding after 5 seconds of inactivity
pub async fn test_cursor_hide_on_inactivity() -> io::Result<()> {
    println!("Starting cursor hide inactivity test...");
    let (mut cursor_socket, _) = screenshare_client::start_screenshare_session()?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let token = livekit_utils::generate_token("Test Cursor Hide");
    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");

    let (room, mut _rx) = Room::connect(&url, &token, RoomOptions::default())
        .await
        .unwrap();
    println!("Connected to room: {}", room.name());

    // Move cursor to the middle of the screen
    println!("Moving cursor to center of screen (0.5, 0.5)...");
    send_mouse_move(&room, 0.5, 0.5).await?;

    println!("Cursor positioned at center. Now waiting 5 seconds for inactivity...");
    println!("The cursor should become hidden after this period of inactivity.");

    // Wait for 5 seconds without any mouse activity
    sleep(Duration::from_secs(7)).await;

    println!("5-second inactivity period completed.");
    println!("If the cursor implementation is correct, it should now be hidden.");

    screenshare_client::stop_screenshare_session(&mut cursor_socket)?;
    println!("Cursor hide inactivity test completed.");

    Ok(())
}

/// Test staggered participant joining - one starts, another joins mid-session
pub async fn test_staggered_participant_joining() -> io::Result<()> {
    println!("Starting staggered participant joining test...");
    let (mut cursor_socket, _) = screenshare_client::start_screenshare_session()?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");

    println!("=== PHASE 1: First participant starts ===");

    // First participant starts immediately
    let token_1 = livekit_utils::generate_token("Alice");
    let (room_1, _rx_1) = Room::connect(&url, &token_1, RoomOptions::default())
        .await
        .unwrap();
    println!("Alice connected to room: {}", room_1.name());

    // Start first participant's task
    let task_1 = tokio::spawn(async move {
        println!("Alice: Starting square at top-left quadrant (0.1, 0.1)");
        let result = internal_cursor_move(&room_1, 0.1, 0.1, 0.4).await;

        if result.is_ok() {
            println!("Alice: Completed square successfully");
            println!("Alice: Disconnecting from the session...");

            // Explicitly disconnect Alice from the room
            let _ = room_1.close().await;

            sleep(Duration::from_millis(500)).await; // Brief delay after disconnect
            println!("Alice: Disconnected");
        }

        result
    });

    // Wait for some time to simulate mid-session joining
    println!("Waiting 3 seconds before second participant joins...");
    sleep(Duration::from_secs(2)).await;

    println!("=== PHASE 2: Second participant joins mid-session ===");

    // Second participant joins mid-session
    let token_2 = livekit_utils::generate_token("Bob");
    let (room_2, _rx_2) = Room::connect(&url, &token_2, RoomOptions::default())
        .await
        .unwrap();
    println!("Bob connected to room: {}", room_2.name());

    // Start second participant's task
    let task_2 = tokio::spawn(async move {
        println!("Bob: Starting square at top-right quadrant (0.5, 0.1)");
        internal_cursor_move(&room_2, 0.5, 0.1, 0.4).await
    });

    println!("=== PHASE 3: Waiting for both participants to complete ===");

    // Wait for both participants to finish their squares
    let results = tokio::try_join!(task_1, task_2);

    match results {
        Ok((res1, res2)) => {
            if let Err(e) = res1 {
                println!("Alice: Error during test: {e:?}");
            } else {
                println!("Alice: Successfully completed square and disconnected");
            }
            if let Err(e) = res2 {
                println!("Bob: Error drawing square: {e:?}");
            } else {
                println!("Bob: Successfully completed square");
            }
        }
        Err(e) => {
            println!("Task execution error: {e:?}");
            screenshare_client::stop_screenshare_session(&mut cursor_socket)?;
            return Err(io::Error::other(e));
        }
    }

    println!("=== TEST COMPLETED ===");
    println!("Alice completed her square and disconnected.");
    println!("Bob joined mid-session and completed his square.");
    println!("Test demonstrates staggered joining and participant disconnection.");

    screenshare_client::stop_screenshare_session(&mut cursor_socket)?;
    println!("Screenshare session stopped.");

    Ok(())
}

/// Test SVG rendering with various Unicode characters and special symbols
pub async fn test_unicode_character_rendering() -> io::Result<()> {
    println!("Starting Unicode character rendering test...");
    let (mut cursor_socket, _) = screenshare_client::start_screenshare_session()?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");

    // Create participants with diverse Unicode characters and symbols
    let participants = vec![
        (
            "José María García-López",
            "Spanish with accents and hyphens",
        ),
        ("李小明", "Chinese characters"),
        ("عبد الرحمن", "Arabic text (RTL)"),
        ("Αλέξανδρος", "Greek characters"),
        ("Владимир", "Cyrillic characters"),
        ("François", "French with cedilla"),
        ("Müller", "German with umlaut"),
        ("User_123!@#", "Numbers and symbols"),
        ("Email@domain.com", "Email-like format"),
        ("Symbol★User", "Unicode symbols"),
    ];

    println!(
        "Connecting {} participants with Unicode names...",
        participants.len()
    );

    let mut rooms = Vec::new();
    for (i, (name, description)) in participants.iter().enumerate() {
        println!(
            "Connecting participant {}: {} ({})",
            i + 1,
            name,
            description
        );
        let token = livekit_utils::generate_token(name);
        let (room, _rx) = Room::connect(&url, &token, RoomOptions::default())
            .await
            .unwrap();
        rooms.push(room);
    }

    println!("All {} participants connected to room", participants.len());
    sleep(Duration::from_secs(10)).await;

    // Position cursors in a grid pattern
    let grid_cols = 3;
    let start_x = 0.2;
    let start_y = 0.2;
    let spacing_x = 0.25;
    let spacing_y = 0.15;

    println!("Positioning Unicode cursors in grid...");

    for (i, (room, (name, description))) in rooms.iter().zip(participants.iter()).enumerate() {
        let col = i % grid_cols;
        let row = i / grid_cols;
        let x = start_x + (col as f64) * spacing_x;
        let y = start_y + (row as f64) * spacing_y;

        println!(
            "Positioning {}: '{}' at ({:.2}, {:.2})",
            description, name, x, y
        );
        send_mouse_move(room, x, y).await?;

        sleep(Duration::from_millis(500)).await;
    }

    println!("=== UNICODE CHARACTER RENDERING TEST COMPLETED ===");
    println!("Tested Unicode SVG rendering with:");
    for (name, description) in &participants {
        println!("  - {}: '{}'", description, name);
    }

    println!("Cursors will remain visible for 15 seconds for observation...");
    sleep(Duration::from_secs(15)).await;

    screenshare_client::stop_screenshare_session(&mut cursor_socket)?;
    println!("Unicode rendering test completed.");

    Ok(())
}

/// Test SVG rendering with various name lengths to stress-test text measurement
pub async fn test_name_length_rendering() -> io::Result<()> {
    println!("Starting name length rendering test...");
    let (mut cursor_socket, _) = screenshare_client::start_screenshare_session()?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");

    // Create participants with varying name lengths
    let participants = vec![
        ("A", "Single character"),
        ("Me", "Sharer name"),
        ("Bob", "Short name"),
        ("Emma", "Four chars"),
        ("Michael", "Seven chars"),
        ("Katherine", "Nine chars"),
        ("Christopher", "Eleven chars"),
        ("Alexander Johnson", "Two word name"),
        (
            "ThisIsAReallyLongNameThatMightCauseRenderingIssues",
            "Extremely long name",
        ),
        ("AlexanderGGGGGGGGGGGGGGG", "Long name with lots of Gs"),
    ];

    println!(
        "Connecting {} participants with varying name lengths...",
        participants.len()
    );

    let mut rooms = Vec::new();
    for (i, (name, description)) in participants.iter().enumerate() {
        println!(
            "Connecting participant {}: {} ({})",
            i + 1,
            name,
            description
        );
        let token = livekit_utils::generate_token(name);
        let (room, _rx) = Room::connect(&url, &token, RoomOptions::default())
            .await
            .unwrap();
        rooms.push(room);
    }

    println!("All {} participants connected to room", participants.len());
    sleep(Duration::from_secs(10)).await;

    // Position cursors in a grid pattern
    let grid_cols = 3;
    let start_x = 0.2;
    let start_y = 0.2;
    let spacing_x = 0.25;
    let spacing_y = 0.15;

    println!("Positioning length-test cursors in grid...");

    for (i, (room, (name, description))) in rooms.iter().zip(participants.iter()).enumerate() {
        let col = i % grid_cols;
        let row = i / grid_cols;
        let x = start_x + (col as f64) * spacing_x;
        let y = start_y + (row as f64) * spacing_y;

        println!(
            "Positioning {}: '{}' at ({:.2}, {:.2})",
            description, name, x, y
        );
        send_mouse_move(room, x, y).await?;

        sleep(Duration::from_millis(500)).await;
    }

    println!("=== NAME LENGTH RENDERING TEST COMPLETED ===");
    println!("Tested name length SVG rendering with:");
    for (name, description) in &participants {
        println!("  - {} chars: '{}' ({})", name.len(), name, description);
    }

    println!("Cursors will remain visible for 15 seconds for observation...");
    sleep(Duration::from_secs(15)).await;

    screenshare_client::stop_screenshare_session(&mut cursor_socket)?;
    println!("Name length rendering test completed.");

    Ok(())
}

/// Test handling participants with the same first name (original focused test)
pub async fn test_same_first_name_participants() -> io::Result<()> {
    println!("Starting same first name participants test...");
    let (mut cursor_socket, _) = screenshare_client::start_screenshare_session()?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");

    // Create 4 participants all with first name "Joe"
    let token_1 = livekit_utils::generate_token("Joe Doe");
    let token_2 = livekit_utils::generate_token("Joe Dorsey");
    let token_3 = livekit_utils::generate_token("Joe Harvey");
    let token_4 = livekit_utils::generate_token("Joe Donovan");

    let (room_1, _rx_1) = Room::connect(&url, &token_1, RoomOptions::default())
        .await
        .unwrap();
    let (room_2, _rx_2) = Room::connect(&url, &token_2, RoomOptions::default())
        .await
        .unwrap();
    let (room_3, _rx_3) = Room::connect(&url, &token_3, RoomOptions::default())
        .await
        .unwrap();
    let (room_4, _rx_4) = Room::connect(&url, &token_4, RoomOptions::default())
        .await
        .unwrap();

    println!("All 4 'Joe' participants connected to room");

    // Define distinct positions for each Joe to move to (vertically aligned)
    let positions = [
        (0.5, 0.3), // Joe Doe: top
        (0.5, 0.4), // Joe Dorsey: second
        (0.5, 0.5), // Joe Harvey: third
        (0.5, 0.6), // Joe Donovan: bottom
    ];

    println!("Starting cursor movements for all Joe participants...");

    // Move each Joe's cursor to their designated position
    println!(
        "Joe Doe: Moving cursor to position ({}, {})",
        positions[0].0, positions[0].1
    );
    send_mouse_move(&room_1, positions[0].0, positions[0].1).await?;
    println!(
        "Joe Doe: Positioned at ({}, {})",
        positions[0].0, positions[0].1
    );

    println!(
        "Joe Dorsey: Moving cursor to position ({}, {})",
        positions[1].0, positions[1].1
    );
    send_mouse_move(&room_2, positions[1].0, positions[1].1).await?;
    println!(
        "Joe Dorsey: Positioned at ({}, {})",
        positions[1].0, positions[1].1
    );

    println!(
        "Joe Harvey: Moving cursor to position ({}, {})",
        positions[2].0, positions[2].1
    );
    send_mouse_move(&room_3, positions[2].0, positions[2].1).await?;
    println!(
        "Joe Harvey: Positioned at ({}, {})",
        positions[2].0, positions[2].1
    );

    println!(
        "Joe Donovan: Moving cursor to position ({}, {})",
        positions[3].0, positions[3].1
    );
    send_mouse_move(&room_4, positions[3].0, positions[3].1).await?;
    println!(
        "Joe Donovan: Positioned at ({}, {})",
        positions[3].0, positions[3].1
    );

    println!("=== SAME FIRST NAME TEST COMPLETED ===");
    println!("All 4 'Joe' participants positioned their cursors in distinct locations.");
    println!("This tests the system's ability to handle participants with the same first name.");

    // Allow time to observe the cursors at their positions
    println!("Cursors will remain visible for 10 seconds...");
    sleep(Duration::from_secs(10)).await;

    screenshare_client::stop_screenshare_session(&mut cursor_socket)?;
    println!("Screenshare session stopped.");

    Ok(())
}

/// Test cursor movement around the window edges, starting and ending at [0,0]
pub async fn test_cursor_window_edges() -> io::Result<()> {
    println!("Starting window edges cursor test...");
    let (mut cursor_socket, _) = screenshare_client::start_screenshare_session()?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let token = livekit_utils::generate_token("Edge Tracer");
    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");

    let (room, mut _rx) = Room::connect(&url, &token, RoomOptions::default())
        .await
        .unwrap();
    println!("Connected to room: {}", room.name());
    sleep(Duration::from_secs(3)).await;

    // Trace the window edges starting from [0,0]
    internal_cursor_trace_window_edges(&room).await?;

    screenshare_client::stop_screenshare_session(&mut cursor_socket)?;
    println!("Window edges cursor test completed.");

    Ok(())
}

async fn internal_cursor_trace_window_edges(room: &Room) -> io::Result<()> {
    let step = 0.005;
    let delay = Duration::from_millis(20); // Small delay between movements

    println!("Starting window edge trace at (0, 0)");

    // Start at top-left corner [0,0]
    let mut x = 0.0;
    let mut y = 0.0;
    send_mouse_move(room, x, y).await?;
    sleep(delay).await;

    // Edge 1: Move right along top edge (from [0,0] to [1,0])
    println!("Tracing top edge: (0,0) → (1,0)");
    while x < 1.0 {
        x += step;
        if x > 1.0 {
            x = 1.0;
        }
        send_mouse_move(room, x, y).await?;
        sleep(delay).await;
    }
    println!("Completed top edge, reached ({:.3}, {:.3})", x, y);

    // Edge 2: Move down along right edge (from [1,0] to [1,1])
    println!("Tracing right edge: (1,0) → (1,1)");
    x = 0.995;
    while y < 1.0 {
        y += step;
        if y > 1.0 {
            y = 1.0;
        }
        send_mouse_move(room, x, y).await?;
        sleep(delay).await;
    }
    println!("Completed right edge, reached ({:.3}, {:.3})", x, y);

    // Edge 3: Move left along bottom edge (from [1,1] to [0,1])
    println!("Tracing bottom edge: (1,1) → (0,1)");
    y = 0.995;
    while x > 0.0 {
        x -= step;
        if x < 0.0 {
            x = 0.0;
        }
        send_mouse_move(room, x, y).await?;
        sleep(delay).await;
    }
    println!("Completed bottom edge, reached ({:.3}, {:.3})", x, y);

    // Edge 4: Move up along left edge (from [0,1] to [0,0])
    println!("Tracing left edge: (0,1) → (0,0)");
    while y > 0.0 {
        y -= step;
        if y < 0.0 {
            y = 0.0;
        }
        send_mouse_move(room, x, y).await?;
        sleep(delay).await;
    }
    println!("Completed left edge, back to start ({:.3}, {:.3})", x, y);

    println!("Window edge trace completed - full perimeter traced!");
    Ok(())
}

/// Test concurrent scrolling from two participants in different screen areas
pub async fn test_concurrent_scrolling() -> io::Result<()> {
    println!("Starting concurrent scrolling test...");
    let (mut cursor_socket, _) = screenshare_client::start_screenshare_session()?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");

    // Create two participants
    let token_1 = livekit_utils::generate_token("Alice Scroll");
    let token_2 = livekit_utils::generate_token("Bob Scroll");

    let (room_1, _rx_1) = Room::connect(&url, &token_1, RoomOptions::default())
        .await
        .unwrap();
    let (room_2, _rx_2) = Room::connect(&url, &token_2, RoomOptions::default())
        .await
        .unwrap();

    println!("Both participants connected to room");

    sleep(Duration::from_secs(2)).await;

    // Define positions - first third and last third of screen
    let pos_1 = (0.33, 0.5); // Alice: First third
    let pos_2 = (0.67, 0.5); // Bob: Last third

    // Create concurrent tasks for each participant
    let task_1 = tokio::spawn(async move {
        println!("Alice: Moving to position ({}, {})", pos_1.0, pos_1.1);
        send_mouse_move(&room_1, pos_1.0, pos_1.1).await?;

        // Phase 1: Alice scrolls up
        println!("Alice: Phase 1 - Scrolling up");
        let scroll_amount = 100.0;
        let mut rng = StdRng::from_entropy();

        for _i in 0..50 {
            send_mouse_wheel(&room_1, 0.0, -scroll_amount).await?; // negative = up
            let random_delay = Duration::from_millis(rng.gen_range(50..=150));
            sleep(random_delay).await;
        }
        println!("Alice: Completed Phase 1 (upward scrolling)");

        // Short pause between phases
        sleep(Duration::from_secs(1)).await;

        // Phase 2: Alice scrolls down (reverse)
        println!("Alice: Phase 2 - Scrolling down");
        for _i in 0..50 {
            send_mouse_wheel(&room_1, 0.0, scroll_amount).await?; // positive = down
            let random_delay = Duration::from_millis(rng.gen_range(50..=150));
            sleep(random_delay).await;
        }
        println!("Alice: Completed Phase 2 (downward scrolling)");

        Ok::<(), io::Error>(())
    });

    let task_2 = tokio::spawn(async move {
        println!("Bob: Moving to position ({}, {})", pos_2.0, pos_2.1);
        send_mouse_move(&room_2, pos_2.0, pos_2.1).await?;

        // Phase 1: Bob scrolls down
        println!("Bob: Phase 1 - Scrolling down");
        let scroll_amount = 100.0;
        let mut rng = StdRng::from_entropy();

        for _i in 0..50 {
            send_mouse_wheel(&room_2, 0.0, scroll_amount).await?; // positive = down
            let random_delay = Duration::from_millis(rng.gen_range(50..=150));
            sleep(random_delay).await;
        }
        println!("Bob: Completed Phase 1 (downward scrolling)");

        // Short pause between phases
        sleep(Duration::from_secs(1)).await;

        // Phase 2: Bob scrolls up (reverse)
        println!("Bob: Phase 2 - Scrolling up");
        for _i in 0..50 {
            send_mouse_wheel(&room_2, 0.0, -scroll_amount).await?; // negative = up
            let random_delay = Duration::from_millis(rng.gen_range(50..=150));
            sleep(random_delay).await;
        }
        println!("Bob: Completed Phase 2 (upward scrolling)");

        Ok::<(), io::Error>(())
    });

    // Wait for both participants to complete
    let results = tokio::try_join!(task_1, task_2);

    match results {
        Ok((res1, res2)) => {
            if let Err(e) = res1 {
                println!("Alice: Error during test: {e:?}");
            } else {
                println!("Alice: Successfully completed concurrent scrolling test");
            }
            if let Err(e) = res2 {
                println!("Bob: Error during test: {e:?}");
            } else {
                println!("Bob: Successfully completed concurrent scrolling test");
            }
        }
        Err(e) => {
            println!("Task execution error: {e:?}");
            screenshare_client::stop_screenshare_session(&mut cursor_socket)?;
            return Err(io::Error::other(e));
        }
    }

    println!("=== CONCURRENT SCROLLING TEST COMPLETED ===");
    println!("Alice and Bob completed scrolling in opposite directions, then reversed.");

    screenshare_client::stop_screenshare_session(&mut cursor_socket)?;
    println!("Screenshare session stopped.");

    Ok(())
}

/// Test click animation with various remote control states
pub async fn test_click_animation() -> io::Result<()> {
    // Start single screenshare session
    println!("Starting screenshare session...");
    let (mut cursor_socket, _) = screenshare_client::start_screenshare_session()?;

    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");

    // Create single participant connection
    let token = livekit_utils::generate_token("TestUser");
    let (room, _rx) = Room::connect(&url, &token, RoomOptions::default())
        .await
        .unwrap();

    println!("Participant connected.");

    // We need to wait for the textures to load
    tokio::time::sleep(std::time::Duration::from_secs(7)).await;

    // Starting position - middle of screen
    let start_x = 0.5;
    let start_y = 0.5;
    let move_distance = 0.2;

    // Track current cursor position - start slightly to the left
    let mut current_x = start_x - 0.1;
    let mut current_y = start_y;

    println!("\n=== Phase 1: Click Animation Enabled with Remote Control ===");
    // Move to starting position before first click
    println!("Moving to starting position ({start_x}, {current_y})");
    smooth_cursor_move(&room, current_x, current_y, start_x, current_y).await?;
    current_x = start_x;
    sleep(Duration::from_millis(500)).await;

    // Enable click animation
    println!("Enabling click animation");
    send_click_animation(&room, true).await?;
    sleep(Duration::from_millis(200)).await;

    // Perform a click
    println!("Clicking at ({current_x}, {current_y})");
    send_mouse_click(&room, current_x, current_y, 0).await?;
    sleep(Duration::from_millis(500)).await;

    // Move to the right
    let new_x = current_x + move_distance;
    println!("Moving to the right to ({new_x}, {current_y})");
    smooth_cursor_move(&room, current_x, current_y, new_x, current_y).await?;
    current_x = new_x;
    sleep(Duration::from_millis(500)).await;

    // Perform a click
    println!("Clicking at ({current_x}, {current_y})");
    send_mouse_click(&room, current_x, current_y, 0).await?;
    sleep(Duration::from_millis(500)).await;

    // Disable click animation
    println!("Disabling click animation");
    send_click_animation(&room, false).await?;
    sleep(Duration::from_millis(200)).await;

    println!("\n=== Phase 2: Remote Control Disabled ===");
    // Disable remote control
    println!("Disabling remote control");
    cursor_socket.send_message(Message::ControllerCursorEnabled(false))?;
    sleep(Duration::from_millis(500)).await;

    // Move down
    let new_y = current_y + move_distance;
    println!("Moving down to ({current_x}, {new_y})");
    smooth_cursor_move(&room, current_x, current_y, current_x, new_y).await?;
    current_y = new_y;
    sleep(Duration::from_millis(500)).await;

    // Enable click animation
    println!("Enabling click animation");
    send_click_animation(&room, true).await?;
    sleep(Duration::from_millis(200)).await;

    // Click - move right - click
    println!("Clicking at ({current_x}, {current_y})");
    send_mouse_click(&room, current_x, current_y, 0).await?;
    sleep(Duration::from_millis(500)).await;

    let new_x2 = current_x + move_distance;
    println!("Moving right to ({new_x2}, {current_y})");
    smooth_cursor_move(&room, current_x, current_y, new_x2, current_y).await?;
    current_x = new_x2;
    sleep(Duration::from_millis(500)).await;

    println!("Clicking at ({current_x}, {current_y})");
    send_mouse_click(&room, current_x, current_y, 0).await?;
    sleep(Duration::from_millis(500)).await;

    println!("\n=== Phase 3: Remote Control Re-enabled ===");
    // Enable remote control
    println!("Enabling remote control");
    cursor_socket.send_message(Message::ControllerCursorEnabled(true))?;
    sleep(Duration::from_millis(500)).await;

    // Disable click animation
    println!("Disabling click animation");
    send_click_animation(&room, false).await?;
    sleep(Duration::from_millis(200)).await;

    // Click
    println!("Clicking at ({current_x}, {current_y})");
    send_mouse_click(&room, current_x, current_y, 0).await?;
    sleep(Duration::from_millis(500)).await;

    // Move up
    let new_y2 = current_y - move_distance;
    println!("Moving up to ({current_x}, {new_y2})");
    smooth_cursor_move(&room, current_x, current_y, current_x, new_y2).await?;
    current_y = new_y2;
    sleep(Duration::from_millis(500)).await;

    println!("\n=== Phase 4: Final Click Animation Sequence ===");
    // Enable click animation
    println!("Enabling click animation");
    send_click_animation(&room, true).await?;
    sleep(Duration::from_millis(200)).await;

    // Click - move right - click
    println!("Clicking at ({current_x}, {current_y})");
    send_mouse_click(&room, current_x, current_y, 0).await?;
    sleep(Duration::from_millis(500)).await;

    let final_x = current_x + move_distance;
    println!("Moving right to ({final_x}, {current_y})");
    smooth_cursor_move(&room, current_x, current_y, final_x, current_y).await?;
    current_x = final_x;
    sleep(Duration::from_millis(500)).await;

    println!("Clicking at ({current_x}, {current_y})");
    send_mouse_click(&room, current_x, current_y, 0).await?;
    sleep(Duration::from_millis(500)).await;

    println!("\n=== TEST COMPLETED ===");
    println!("All click animation phases completed successfully.");

    // Stop the screenshare session
    screenshare_client::stop_screenshare_session(&mut cursor_socket)?;
    println!("Screenshare session stopped.");

    Ok(())
}
