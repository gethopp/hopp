use crate::events::{ClientEvent, DrawPathPoint, DrawPoint, DrawSettings, DrawingMode};
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

/// Sends a DrawStart event with a path_id
async fn send_draw_start(room: &Room, x: f64, y: f64, path_id: u64) -> io::Result<()> {
    let point = DrawPoint { x, y };
    let draw_path_point = DrawPathPoint { point, path_id };
    let event = ClientEvent::DrawStart(draw_path_point);
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

/// Sends a DrawClearPath event to clear a specific path
async fn send_draw_clear_path(room: &Room, path_id: u64) -> io::Result<()> {
    let event = ClientEvent::DrawClearPath { path_id };
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

/// Sends a DrawClearAllPaths event to clear all paths
async fn send_draw_clear_all_paths(room: &Room) -> io::Result<()> {
    let event = ClientEvent::DrawClearAllPaths;
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

/// Draws a stroke (line) from one point to another with a specific path_id
async fn draw_stroke(
    room: &Room,
    from_x: f64,
    from_y: f64,
    to_x: f64,
    to_y: f64,
    path_id: u64,
) -> io::Result<()> {
    let steps = 20;
    let delay = Duration::from_millis(10);

    send_draw_start(room, from_x, from_y, path_id).await?;
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

/// Test click animation mode - Basic functionality
/// Triggers click animations at various points on the screen
pub async fn test_click_animation_mode() -> io::Result<()> {
    println!("\n=== TEST: Click Animation Mode - Basic ===");
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
            draw_stroke(&room_1, *from_x, *from_y, *to_x, *to_y, i as u64).await?;
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
            draw_stroke(&room_2, *from_x, *from_y, *to_x, *to_y, i as u64).await?;
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
            draw_stroke(&room_3, *from_x, *from_y, *to_x, *to_y, i as u64).await?;
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
            draw_stroke(&room_4, *from_x, *from_y, *to_x, *to_y, i as u64).await?;
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

/// Test drawing 4 lines and clearing them one by one using DrawClearPath
pub async fn test_draw_and_clear_paths_individually() -> io::Result<()> {
    println!("\n=== TEST: Draw and Clear Paths Individually ===");
    let (mut cursor_socket, _) = screenshare_client::start_screenshare_session()?;

    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");
    let token = livekit_utils::generate_token("PathClearTester");
    let (room, _rx) = Room::connect(&url, &token, RoomOptions::default())
        .await
        .unwrap();

    println!("Participant connected. Waiting for setup...");
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // Enable drawing mode
    println!("Enabling drawing mode with permanent=true");
    send_drawing_mode(&room, DrawingMode::Draw(DrawSettings { permanent: true })).await?;
    sleep(Duration::from_millis(500)).await;

    // Draw 4 lines in different positions with different path_ids
    let lines = vec![
        ((0.2, 0.2), (0.4, 0.2), 0, "top horizontal"),
        ((0.6, 0.4), (0.8, 0.4), 1, "middle horizontal"),
        ((0.2, 0.6), (0.4, 0.6), 2, "lower horizontal"),
        ((0.6, 0.8), (0.8, 0.8), 3, "bottom horizontal"),
    ];

    println!("\nDrawing 4 lines with path_ids 0, 1, 2, 3...");
    for (from, to, path_id, name) in &lines {
        println!("Drawing {} line (path_id: {})", name, path_id);
        draw_stroke(&room, from.0, from.1, to.0, to.1, *path_id).await?;
        sleep(Duration::from_millis(500)).await;
    }

    println!("\nAll 4 lines drawn. Waiting 2 seconds...");
    sleep(Duration::from_secs(2)).await;

    // Clear paths one by one
    println!("\nClearing paths one by one...");
    for (_, _, path_id, name) in &lines {
        println!("Clearing {} line (path_id: {})", name, path_id);
        send_draw_clear_path(&room, *path_id).await?;
        sleep(Duration::from_secs(2)).await;
    }

    println!("\nAll paths cleared.");
    sleep(Duration::from_secs(1)).await;

    // Disable drawing mode
    println!("Disabling drawing mode");
    send_drawing_mode(&room, DrawingMode::Disabled).await?;

    println!("\n=== TEST COMPLETED ===");
    screenshare_client::stop_screenshare_session(&mut cursor_socket)?;
    Ok(())
}

/// Test drawing multiple lines and clearing all of them at once using DrawClearAllPaths
pub async fn test_draw_and_clear_all_paths() -> io::Result<()> {
    println!("\n=== TEST: Draw and Clear All Paths ===");
    let (mut cursor_socket, _) = screenshare_client::start_screenshare_session()?;

    let url = std::env::var("LIVEKIT_URL").expect("LIVEKIT_URL environment variable not set");
    let token = livekit_utils::generate_token("ClearAllTester");
    let (room, _rx) = Room::connect(&url, &token, RoomOptions::default())
        .await
        .unwrap();

    println!("Participant connected. Waiting for setup...");
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // Enable drawing mode
    println!("Enabling drawing mode with permanent=true");
    send_drawing_mode(&room, DrawingMode::Draw(DrawSettings { permanent: true })).await?;
    sleep(Duration::from_millis(500)).await;

    // Draw multiple lines forming a grid pattern
    println!("\nDrawing multiple lines to form a grid pattern...");
    let mut path_id = 0;

    // Vertical lines
    for i in 0..5 {
        let x = 0.2 + (i as f64 * 0.15);
        println!("Drawing vertical line {} at x={:.2}", i + 1, x);
        draw_stroke(&room, x, 0.2, x, 0.8, path_id).await?;
        path_id += 1;
        sleep(Duration::from_millis(300)).await;
    }

    // Horizontal lines
    for i in 0..5 {
        let y = 0.2 + (i as f64 * 0.15);
        println!("Drawing horizontal line {} at y={:.2}", i + 1, y);
        draw_stroke(&room, 0.2, y, 0.8, y, path_id).await?;
        path_id += 1;
        sleep(Duration::from_millis(300)).await;
    }

    println!(
        "\nGrid pattern complete with {} lines. Waiting 3 seconds...",
        path_id
    );
    sleep(Duration::from_secs(3)).await;

    // Clear all paths at once
    println!("\nClearing ALL paths at once using DrawClearAllPaths...");
    send_draw_clear_all_paths(&room).await?;
    println!("All paths should now be cleared.");
    sleep(Duration::from_secs(2)).await;

    // Disable drawing mode
    println!("Disabling drawing mode");
    send_drawing_mode(&room, DrawingMode::Disabled).await?;

    println!("\n=== TEST COMPLETED ===");
    screenshare_client::stop_screenshare_session(&mut cursor_socket)?;
    Ok(())
}
