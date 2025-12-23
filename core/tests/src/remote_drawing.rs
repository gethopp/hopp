use crate::events::{ClientEvent, DrawPoint, DrawSettings, DrawingMode};
use crate::livekit_utils;
use crate::screenshare_client;
use livekit::prelude::*;
use std::{io, time::Duration};
use tokio::time::sleep;

/// Sends a DrawingMode event
async fn send_drawing_mode(room: &Room, mode: DrawingMode) -> io::Result<()> {
    let event = ClientEvent::DrawingMode(mode);
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

/// Sends a DrawStart event
async fn send_draw_start(room: &Room, x: f64, y: f64) -> io::Result<()> {
    let point = DrawPoint { x, y };
    let event = ClientEvent::DrawStart(point);
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

/// Sends a DrawAddPoint event
async fn send_draw_add_point(room: &Room, x: f64, y: f64) -> io::Result<()> {
    let point = DrawPoint { x, y };
    let event = ClientEvent::DrawAddPoint(point);
    let payload = serde_json::to_vec(&event).map_err(io::Error::other)?;
    room.local_participant()
        .publish_data(DataPacket {
            payload,
            reliable: false, // Points can tolerate some loss for smoother drawing
            ..Default::default()
        })
        .await
        .map_err(io::Error::other)?;
    Ok(())
}

/// Sends a DrawEnd event
async fn send_draw_end(room: &Room, x: f64, y: f64) -> io::Result<()> {
    let point = DrawPoint { x, y };
    let event = ClientEvent::DrawEnd(point);
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

/// Sends a ClickAnimation event at a specific point
async fn send_click_animation_at(room: &Room, x: f64, y: f64) -> io::Result<()> {
    let point = DrawPoint { x, y };
    let event = ClientEvent::ClickAnimation(point);
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

/// Draws a stroke (line) from one point to another
async fn draw_stroke(
    room: &Room,
    from_x: f64,
    from_y: f64,
    to_x: f64,
    to_y: f64,
) -> io::Result<()> {
    let steps = 20;
    let delay = Duration::from_millis(10);

    send_draw_start(room, from_x, from_y).await?;
    sleep(delay).await;

    for i in 1..steps {
        let t = i as f64 / steps as f64;
        let x = from_x + (to_x - from_x) * t;
        let y = from_y + (to_y - from_y) * t;
        send_draw_add_point(room, x, y).await?;
        sleep(delay).await;
    }

    send_draw_end(room, to_x, to_y).await?;
    sleep(Duration::from_millis(50)).await;

    Ok(())
}

/// Letter drawing definitions - each letter is a set of strokes
/// Coordinates are normalized (0-1) and will be scaled and positioned

struct LetterStroke {
    from: (f64, f64),
    to: (f64, f64),
}

fn get_letter_strokes(letter: char) -> Vec<LetterStroke> {
    match letter {
        'H' => vec![
            // Left vertical
            LetterStroke {
                from: (0.0, 0.0),
                to: (0.0, 1.0),
            },
            // Right vertical
            LetterStroke {
                from: (0.6, 0.0),
                to: (0.6, 1.0),
            },
            // Middle horizontal
            LetterStroke {
                from: (0.0, 0.5),
                to: (0.6, 0.5),
            },
        ],
        'e' => vec![
            // Middle horizontal
            LetterStroke {
                from: (0.0, 0.5),
                to: (0.5, 0.5),
            },
            // Top curve (simplified as line)
            LetterStroke {
                from: (0.5, 0.5),
                to: (0.5, 0.3),
            },
            LetterStroke {
                from: (0.5, 0.3),
                to: (0.25, 0.2),
            },
            LetterStroke {
                from: (0.25, 0.2),
                to: (0.0, 0.3),
            },
            LetterStroke {
                from: (0.0, 0.3),
                to: (0.0, 0.5),
            },
            // Bottom curve
            LetterStroke {
                from: (0.0, 0.5),
                to: (0.0, 0.8),
            },
            LetterStroke {
                from: (0.0, 0.8),
                to: (0.25, 1.0),
            },
            LetterStroke {
                from: (0.25, 1.0),
                to: (0.5, 0.9),
            },
        ],
        'l' => vec![
            // Vertical line
            LetterStroke {
                from: (0.2, 0.0),
                to: (0.2, 1.0),
            },
        ],
        'o' => vec![
            // Circle (simplified as strokes)
            LetterStroke {
                from: (0.25, 0.2),
                to: (0.0, 0.4),
            },
            LetterStroke {
                from: (0.0, 0.4),
                to: (0.0, 0.7),
            },
            LetterStroke {
                from: (0.0, 0.7),
                to: (0.25, 1.0),
            },
            LetterStroke {
                from: (0.25, 1.0),
                to: (0.5, 0.7),
            },
            LetterStroke {
                from: (0.5, 0.7),
                to: (0.5, 0.4),
            },
            LetterStroke {
                from: (0.5, 0.4),
                to: (0.25, 0.2),
            },
        ],
        ' ' => vec![], // Space - no strokes
        'W' => vec![
            // Left diagonal down
            LetterStroke {
                from: (0.0, 0.0),
                to: (0.2, 1.0),
            },
            // Left-center diagonal up
            LetterStroke {
                from: (0.2, 1.0),
                to: (0.4, 0.5),
            },
            // Right-center diagonal down
            LetterStroke {
                from: (0.4, 0.5),
                to: (0.6, 1.0),
            },
            // Right diagonal up
            LetterStroke {
                from: (0.6, 1.0),
                to: (0.8, 0.0),
            },
        ],
        'r' => vec![
            // Vertical stem
            LetterStroke {
                from: (0.0, 0.3),
                to: (0.0, 1.0),
            },
            // Top curve
            LetterStroke {
                from: (0.0, 0.4),
                to: (0.2, 0.25),
            },
            LetterStroke {
                from: (0.2, 0.25),
                to: (0.4, 0.3),
            },
        ],
        'd' => vec![
            // Bowl
            LetterStroke {
                from: (0.4, 0.3),
                to: (0.2, 0.2),
            },
            LetterStroke {
                from: (0.2, 0.2),
                to: (0.0, 0.4),
            },
            LetterStroke {
                from: (0.0, 0.4),
                to: (0.0, 0.7),
            },
            LetterStroke {
                from: (0.0, 0.7),
                to: (0.2, 1.0),
            },
            LetterStroke {
                from: (0.2, 1.0),
                to: (0.4, 0.8),
            },
            // Vertical stem (full height)
            LetterStroke {
                from: (0.4, 0.0),
                to: (0.4, 1.0),
            },
        ],
        '!' => vec![
            // Vertical line
            LetterStroke {
                from: (0.2, 0.0),
                to: (0.2, 0.7),
            },
            // Dot
            LetterStroke {
                from: (0.2, 0.9),
                to: (0.2, 1.0),
            },
        ],
        _ => vec![],
    }
}

/// Draws a single letter at a given position with a given size
async fn draw_letter(
    room: &Room,
    letter: char,
    base_x: f64,
    base_y: f64,
    width: f64,
    height: f64,
) -> io::Result<()> {
    let strokes = get_letter_strokes(letter);

    for stroke in strokes {
        let from_x = base_x + stroke.from.0 * width;
        let from_y = base_y + stroke.from.1 * height;
        let to_x = base_x + stroke.to.0 * width;
        let to_y = base_y + stroke.to.1 * height;

        draw_stroke(room, from_x, from_y, to_x, to_y).await?;
    }

    Ok(())
}

/// Draws "Hello World!" text
async fn draw_hello_world(room: &Room, start_x: f64, start_y: f64) -> io::Result<()> {
    let text = "Hello World!";
    let letter_width = 0.05;
    let letter_height = 0.08;
    let letter_spacing = 0.06;
    let space_width = 0.03;

    let mut current_x = start_x;

    for letter in text.chars() {
        if letter == ' ' {
            current_x += space_width;
        } else {
            println!("Drawing letter: '{}'", letter);
            draw_letter(
                room,
                letter,
                current_x,
                start_y,
                letter_width,
                letter_height,
            )
            .await?;
            current_x += letter_spacing;
        }
    }

    Ok(())
}

/// Test drawing with permanent mode ON
/// Lines should remain visible indefinitely
pub async fn test_drawing_permanent_on() -> io::Result<()> {
    println!("\n=== TEST: Drawing Mode - Permanent ON ===");
    let (mut cursor_socket, _) = screenshare_client::start_screenshare_session()?;

    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");
    let token = livekit_utils::generate_token("DrawingTester");
    let (room, _rx) = Room::connect(&url, &token, RoomOptions::default())
        .await
        .unwrap();

    println!("Participant connected. Waiting for setup...");
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // Enable drawing mode with permanent = true
    println!("Enabling drawing mode with permanent=true");
    send_drawing_mode(&room, DrawingMode::Draw(DrawSettings { permanent: true })).await?;
    sleep(Duration::from_millis(500)).await;

    // Draw "Hello World!"
    println!("Drawing 'Hello World!' with permanent strokes...");
    draw_hello_world(&room, 0.1, 0.4).await?;

    println!("Drawing complete. Lines should remain visible.");
    println!("Waiting 10 seconds to observe permanent lines...");
    sleep(Duration::from_secs(10)).await;

    // Disable drawing mode
    println!("Disabling drawing mode");
    send_drawing_mode(&room, DrawingMode::Disabled).await?;

    println!("\n=== TEST COMPLETED ===");
    screenshare_client::stop_screenshare_session(&mut cursor_socket)?;
    Ok(())
}

/// Test drawing with permanent mode OFF
/// Lines should fade away after a while
pub async fn test_drawing_permanent_off() -> io::Result<()> {
    println!("\n=== TEST: Drawing Mode - Permanent OFF ===");
    let (mut cursor_socket, _) = screenshare_client::start_screenshare_session()?;

    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");
    let token = livekit_utils::generate_token("DrawingTester");
    let (room, _rx) = Room::connect(&url, &token, RoomOptions::default())
        .await
        .unwrap();

    println!("Participant connected. Waiting for setup...");
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // Enable drawing mode with permanent = false
    println!("Enabling drawing mode with permanent=false");
    send_drawing_mode(&room, DrawingMode::Draw(DrawSettings { permanent: false })).await?;
    sleep(Duration::from_millis(500)).await;

    // Draw "Hello World!"
    println!("Drawing 'Hello World!' with fading strokes...");
    draw_hello_world(&room, 0.1, 0.4).await?;

    println!("Drawing complete. Lines should fade away after a while.");
    println!("Waiting 15 seconds to observe fading effect...");
    sleep(Duration::from_secs(15)).await;

    // Disable drawing mode
    println!("Disabling drawing mode");
    send_drawing_mode(&room, DrawingMode::Disabled).await?;

    println!("\n=== TEST COMPLETED ===");
    screenshare_client::stop_screenshare_session(&mut cursor_socket)?;
    Ok(())
}

/// Test click animation mode
/// Triggers click animations at various points on the screen
pub async fn test_click_animation_mode() -> io::Result<()> {
    println!("\n=== TEST: Click Animation Mode ===");
    let (mut cursor_socket, _) = screenshare_client::start_screenshare_session()?;

    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");
    let token = livekit_utils::generate_token("ClickAnimTester");
    let (room, _rx) = Room::connect(&url, &token, RoomOptions::default())
        .await
        .unwrap();

    println!("Participant connected. Waiting for setup...");
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // Enable click animation mode
    println!("Enabling click animation mode");
    send_drawing_mode(&room, DrawingMode::ClickAnimation).await?;
    sleep(Duration::from_millis(500)).await;

    // Trigger click animations at various positions
    let positions = [
        (0.2, 0.2, "top-left"),
        (0.8, 0.2, "top-right"),
        (0.5, 0.5, "center"),
        (0.2, 0.8, "bottom-left"),
        (0.8, 0.8, "bottom-right"),
        (0.3, 0.5, "left-center"),
        (0.7, 0.5, "right-center"),
        (0.5, 0.3, "top-center"),
        (0.5, 0.7, "bottom-center"),
    ];

    for (x, y, name) in positions {
        println!("Click animation at {} ({}, {})", name, x, y);
        send_click_animation_at(&room, x, y).await?;
        sleep(Duration::from_millis(800)).await;
    }

    // Rapid fire click animations in a pattern
    println!("\nRapid click animation sequence...");
    for i in 0..10 {
        let angle = (i as f64) * 0.628; // ~36 degrees per step
        let x = 0.5 + 0.2 * angle.cos();
        let y = 0.5 + 0.2 * angle.sin();
        send_click_animation_at(&room, x, y).await?;
        sleep(Duration::from_millis(200)).await;
    }

    println!("Click animation sequence complete.");
    sleep(Duration::from_secs(2)).await;

    // Disable drawing mode
    println!("Disabling click animation mode");
    send_drawing_mode(&room, DrawingMode::Disabled).await?;

    println!("\n=== TEST COMPLETED ===");
    screenshare_client::stop_screenshare_session(&mut cursor_socket)?;
    Ok(())
}

/// Test 4 participants drawing 3 lines each simultaneously in different quarters
/// Each participant draws in a different quarter of the screen:
/// - Participant 1: Top-left quarter (0.0-0.5, 0.0-0.5)
/// - Participant 2: Top-right quarter (0.5-1.0, 0.0-0.5)
/// - Participant 3: Bottom-left quarter (0.0-0.5, 0.5-1.0)
/// - Participant 4: Bottom-right quarter (0.5-1.0, 0.5-1.0)
pub async fn test_four_participants_concurrent_drawing() -> io::Result<()> {
    println!("\n=== TEST: 4 Participants Concurrent Drawing ===");
    let (mut cursor_socket, _) = screenshare_client::start_screenshare_session()?;

    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");

    // Create 4 participants
    let token_1 = livekit_utils::generate_token("Participant 1");
    let token_2 = livekit_utils::generate_token("Participant 2");
    let token_3 = livekit_utils::generate_token("Participant 3");
    let token_4 = livekit_utils::generate_token("Participant 4");

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

    println!("All 4 participants connected. Waiting for setup...");
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // Enable drawing mode for all participants with permanent = true
    println!("Enabling drawing mode for all participants...");
    send_drawing_mode(&room_1, DrawingMode::Draw(DrawSettings { permanent: true })).await?;
    send_drawing_mode(&room_2, DrawingMode::Draw(DrawSettings { permanent: true })).await?;
    send_drawing_mode(&room_3, DrawingMode::Draw(DrawSettings { permanent: true })).await?;
    send_drawing_mode(&room_4, DrawingMode::Draw(DrawSettings { permanent: true })).await?;
    sleep(Duration::from_millis(500)).await;

    // Define quarters and line coordinates for each participant
    // Each participant will draw 3 lines in their quarter
    // Top-left quarter (0.0-0.5, 0.0-0.5)
    let lines_1 = vec![
        ((0.1, 0.1), (0.4, 0.1)), // Horizontal line at top
        ((0.1, 0.2), (0.4, 0.3)), // Diagonal line
        ((0.1, 0.4), (0.4, 0.4)), // Horizontal line at bottom
    ];

    // Top-right quarter (0.5-1.0, 0.0-0.5)
    let lines_2 = vec![
        ((0.6, 0.1), (0.9, 0.1)), // Horizontal line at top
        ((0.6, 0.2), (0.9, 0.3)), // Diagonal line
        ((0.6, 0.4), (0.9, 0.4)), // Horizontal line at bottom
    ];

    // Bottom-left quarter (0.0-0.5, 0.5-1.0)
    let lines_3 = vec![
        ((0.1, 0.6), (0.4, 0.6)), // Horizontal line at top
        ((0.1, 0.7), (0.4, 0.8)), // Diagonal line
        ((0.1, 0.9), (0.4, 0.9)), // Horizontal line at bottom
    ];

    // Bottom-right quarter (0.5-1.0, 0.5-1.0)
    let lines_4 = vec![
        ((0.6, 0.6), (0.9, 0.6)), // Horizontal line at top
        ((0.6, 0.7), (0.9, 0.8)), // Diagonal line
        ((0.6, 0.9), (0.9, 0.9)), // Horizontal line at bottom
    ];

    println!("Starting concurrent drawing...");
    println!("Participant 1: Drawing 3 lines in top-left quarter");
    println!("Participant 2: Drawing 3 lines in top-right quarter");
    println!("Participant 3: Drawing 3 lines in bottom-left quarter");
    println!("Participant 4: Drawing 3 lines in bottom-right quarter");

    // Create tasks for each participant to draw concurrently
    let task_1 = tokio::spawn(async move {
        for (i, ((from_x, from_y), (to_x, to_y))) in lines_1.iter().enumerate() {
            println!(
                "Participant 1: Drawing line {} from ({:.2}, {:.2}) to ({:.2}, {:.2})",
                i + 1,
                from_x,
                from_y,
                to_x,
                to_y
            );
            draw_stroke(&room_1, *from_x, *from_y, *to_x, *to_y).await?;
        }
        println!("Participant 1: Completed all 3 lines");
        Ok::<(), io::Error>(())
    });

    let task_2 = tokio::spawn(async move {
        for (i, ((from_x, from_y), (to_x, to_y))) in lines_2.iter().enumerate() {
            println!(
                "Participant 2: Drawing line {} from ({:.2}, {:.2}) to ({:.2}, {:.2})",
                i + 1,
                from_x,
                from_y,
                to_x,
                to_y
            );
            draw_stroke(&room_2, *from_x, *from_y, *to_x, *to_y).await?;
        }
        println!("Participant 2: Completed all 3 lines");
        Ok::<(), io::Error>(())
    });

    let task_3 = tokio::spawn(async move {
        for (i, ((from_x, from_y), (to_x, to_y))) in lines_3.iter().enumerate() {
            println!(
                "Participant 3: Drawing line {} from ({:.2}, {:.2}) to ({:.2}, {:.2})",
                i + 1,
                from_x,
                from_y,
                to_x,
                to_y
            );
            draw_stroke(&room_3, *from_x, *from_y, *to_x, *to_y).await?;
        }
        println!("Participant 3: Completed all 3 lines");
        Ok::<(), io::Error>(())
    });

    let task_4 = tokio::spawn(async move {
        for (i, ((from_x, from_y), (to_x, to_y))) in lines_4.iter().enumerate() {
            println!(
                "Participant 4: Drawing line {} from ({:.2}, {:.2}) to ({:.2}, {:.2})",
                i + 1,
                from_x,
                from_y,
                to_x,
                to_y
            );
            draw_stroke(&room_4, *from_x, *from_y, *to_x, *to_y).await?;
        }
        println!("Participant 4: Completed all 3 lines");
        Ok::<(), io::Error>(())
    });

    // Wait for all participants to finish drawing concurrently
    let results = tokio::try_join!(task_1, task_2, task_3, task_4);

    match results {
        Ok((res1, res2, res3, res4)) => {
            if let Err(e) = res1 {
                println!("Participant 1 encountered error: {e:?}");
            } else {
                println!("Participant 1: Successfully completed all lines");
            }
            if let Err(e) = res2 {
                println!("Participant 2 encountered error: {e:?}");
            } else {
                println!("Participant 2: Successfully completed all lines");
            }
            if let Err(e) = res3 {
                println!("Participant 3 encountered error: {e:?}");
            } else {
                println!("Participant 3: Successfully completed all lines");
            }
            if let Err(e) = res4 {
                println!("Participant 4 encountered error: {e:?}");
            } else {
                println!("Participant 4: Successfully completed all lines");
            }
        }
        Err(e) => {
            println!("Task execution error: {e:?}");
            screenshare_client::stop_screenshare_session(&mut cursor_socket)?;
            return Err(io::Error::other(e));
        }
    }

    println!("\nAll participants completed drawing concurrently.");
    println!("Waiting 5 seconds to observe the drawn lines...");
    sleep(Duration::from_secs(5)).await;

    println!("\n=== TEST COMPLETED ===");
    screenshare_client::stop_screenshare_session(&mut cursor_socket)?;
    Ok(())
}
