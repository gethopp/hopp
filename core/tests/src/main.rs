use clap::{Parser, Subcommand, ValueEnum};
use std::io;

mod audio_capture;
mod camera;
mod events;
mod livekit_utils;
mod local_drawing;
mod remote_clipboard;
mod remote_cursor;
mod remote_drawing;
mod remote_keyboard;
mod screenshare_client;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Test cursor functionality
    Cursor {
        /// Type of cursor test to run
        #[arg(value_enum)]
        test_type: CursorTest,
    },
    /// Test keyboard functionality
    Keyboard,
    /// Test clipboard functionality
    Clipboard {
        /// Type of clipboard test to run
        #[arg(value_enum)]
        test_type: ClipboardTest,
    },
    /// Test screenshare functionality
    Screenshare {
        /// Type of screenshare test to run
        #[arg(value_enum)]
        test_type: ScreenshareTest,
    },
    /// Test drawing functionality
    Drawing {
        /// Type of drawing test to run
        #[arg(value_enum)]
        test_type: DrawingTest,
    },
    /// Test local drawing functionality (sharer drawing)
    LocalDrawing {
        /// Type of local drawing test to run
        #[arg(value_enum)]
        test_type: LocalDrawingTest,
    },
    /// Test audio capture functionality
    Audio {
        /// Type of audio test to run
        #[arg(value_enum)]
        test_type: AudioTest,
        /// Optional mic device ID to use for capture tests
        #[arg(long)]
        mic_id: Option<String>,
    },
    /// Test camera capture functionality
    Camera {
        /// Type of camera test to run
        #[arg(value_enum)]
        test_type: CameraTest,
        /// Optional camera name to use
        #[arg(long)]
        camera_name: Option<String>,
    },
}

#[derive(Clone, ValueEnum, Debug)]
enum LocalDrawingTest {
    /// Test local drawing with permanent mode ON
    Permanent,
    /// Test local drawing with permanent mode OFF
    NonPermanent,
}

#[derive(Clone, ValueEnum, Debug)]
enum CursorTest {
    /// Run complete cursor test for single cursor
    Complete,
    /// Run click test
    Click,
    /// Run move test
    Move,
    /// Run scroll test
    Scroll,
    /// Multiple participants
    MultipleParticipants,
    /// Test multiple cursors with control handoff
    CursorControl,
    /// Test cursor hiding after inactivity
    HideOnInactivity,
    /// Test staggered participant joining
    StaggeredJoining,
    /// Test same first name participants
    SameFirstNameParticipants,
    /// Test diverse participant names rendering
    NamesRendering,
    /// Test Unicode character rendering
    NamesUnicode,
    /// Test cursor window edges
    WindowEdges,
    /// Test concurrent scrolling
    ConcurrentScrolling,
    /// Test transitions: Remote enabled then disabled
    TransitionsRemoteEnabledThenDisabled,
}

#[derive(Clone, ValueEnum, Debug)]
enum ClipboardTest {
    /// Test paste with single payload
    PasteSingle,
    /// Test paste with multiple payloads
    PasteMultiple,
    /// Test add to clipboard (copy)
    AddCopy,
    /// Test add to clipboard (cut)
    AddCut,
}

#[derive(Clone, ValueEnum, Debug)]
enum ScreenshareTest {
    /// Run basic screenshare test
    Basic,
    /// Test available content consistency across multiple requests
    AvailableContent,
    /// Screen share every available monitor for 10 seconds each
    EveryMonitor,
    /// Start call, wait 5s, end call, start another call
    CallRestartCycle,
}

#[derive(Clone, ValueEnum, Debug)]
enum AudioTest {
    /// List available audio devices
    ListDevices,
    /// Capture from all devices (15s each)
    CaptureAll,
    /// Test mute/unmute cycle
    MuteUnmute,
    /// Capture for 30 seconds from default device
    Capture30s,
}

#[derive(Clone, ValueEnum, Debug)]
enum CameraTest {
    /// List available cameras
    ListDevices,
    /// Share camera for 30 seconds
    Share30s,
    /// Subscribe to camera tracks and log when received
    TrackSubscribe,
}

#[derive(Clone, ValueEnum, Debug)]
enum DrawingTest {
    /// Test drawing with permanent mode ON (lines stay visible)
    PermanentOn,
    /// Test drawing with permanent mode OFF (lines fade away)
    PermanentOff,
    /// Test click animation mode
    ClickAnimation,
    /// Test 4 participants drawing 3 lines each concurrently in different quarters
    FourParticipantsConcurrent,
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let args = Args::parse();

    // Handle different commands
    match args.command {
        Commands::Cursor { test_type } => {
            match test_type {
                CursorTest::Complete => {
                    println!("Running complete cursor test...");
                    remote_cursor::test_cursor().await?;
                }
                CursorTest::Click => {
                    println!("Running click test...");
                    // Example coordinates, adjust as needed or make configurable
                    remote_cursor::test_cursor_click(0.5, 0.5).await?;
                }
                CursorTest::Move => {
                    println!("Running move test...");
                    remote_cursor::test_cursor_move().await?;
                }
                CursorTest::Scroll => {
                    println!("Running scroll test...");
                    remote_cursor::test_cursor_scroll().await?;
                }
                CursorTest::MultipleParticipants => {
                    println!("Running multiple participants test...");
                    remote_cursor::test_multiple_participants().await?;
                }
                CursorTest::CursorControl => {
                    println!("Running cursor control test...");
                    remote_cursor::test_multiple_cursors_with_control().await?;
                }
                CursorTest::HideOnInactivity => {
                    println!("Running cursor hide on inactivity test...");
                    remote_cursor::test_cursor_hide_on_inactivity().await?;
                }
                CursorTest::StaggeredJoining => {
                    println!("Running staggered participant joining test...");
                    remote_cursor::test_staggered_participant_joining().await?;
                }
                CursorTest::SameFirstNameParticipants => {
                    println!("Running same first name participants test...");
                    remote_cursor::test_same_first_name_participants().await?;
                }
                CursorTest::NamesRendering => {
                    println!("Running diverse participant names rendering test...");
                    remote_cursor::test_name_length_rendering().await?;
                }
                CursorTest::NamesUnicode => {
                    println!("Running Unicode character rendering test...");
                    remote_cursor::test_unicode_character_rendering().await?;
                }
                CursorTest::WindowEdges => {
                    println!("Running window edges test...");
                    remote_cursor::test_cursor_window_edges().await?;
                }
                CursorTest::ConcurrentScrolling => {
                    println!("Running concurrent scrolling test...");
                    remote_cursor::test_concurrent_scrolling().await?;
                }
                CursorTest::TransitionsRemoteEnabledThenDisabled => {
                    println!("Running transitions test: Remote enabled then disabled...");
                    remote_cursor::test_transitions_remote_enabled_then_disabled().await?;
                }
            }
            println!("Cursor test finished.");
        }
        Commands::Keyboard => {
            println!("Running keyboard test...");
            remote_keyboard::test_keyboard_chars().await?;
            println!("Keyboard test finished.");
        }
        Commands::Clipboard { test_type } => {
            match test_type {
                ClipboardTest::PasteSingle => {
                    println!("Running paste single payload test...");
                    remote_clipboard::test_paste_single().await?;
                }
                ClipboardTest::PasteMultiple => {
                    println!("Running paste multiple payloads test...");
                    remote_clipboard::test_paste_multiple().await?;
                }
                ClipboardTest::AddCopy => {
                    println!("Running add to clipboard (copy) test...");
                    remote_clipboard::test_add_copy().await?;
                }
                ClipboardTest::AddCut => {
                    println!("Running add to clipboard (cut) test...");
                    remote_clipboard::test_add_cut().await?;
                }
            }
            println!("Clipboard test finished.");
        }
        Commands::Screenshare { test_type } => {
            match test_type {
                ScreenshareTest::Basic => {
                    println!("Running basic screenshare test...");
                    screenshare_client::screenshare_test()?;
                }
                ScreenshareTest::AvailableContent => {
                    println!("Running available content test...");
                    screenshare_client::test_available_content_consistency()?;
                }
                ScreenshareTest::EveryMonitor => {
                    println!("Running every monitor screenshare test...");
                    screenshare_client::test_every_monitor()?;
                }
                ScreenshareTest::CallRestartCycle => {
                    println!("Running call restart cycle test...");
                    screenshare_client::test_call_restart_cycle()?;
                }
            }
            println!("Screenshare test finished.");
        }
        Commands::Drawing { test_type } => {
            match test_type {
                DrawingTest::PermanentOn => {
                    println!("Running drawing test with permanent mode OFF...");
                    remote_drawing::test_draw_and_clear_all_paths().await?;
                }
                DrawingTest::PermanentOff => {
                    println!("Running drawing test with permanent mode ON...");
                    remote_drawing::test_draw_and_clear_paths_individually().await?;
                }
                DrawingTest::ClickAnimation => {
                    println!("Running click animation mode test...");
                    remote_drawing::test_click_animation_mode().await?;
                }
                DrawingTest::FourParticipantsConcurrent => {
                    println!("Running 4 participants concurrent drawing test...");
                    remote_drawing::test_four_participants_concurrent_drawing().await?;
                }
            }
            println!("Drawing test finished.");
        }
        Commands::Audio { test_type, mic_id } => {
            match test_type {
                AudioTest::ListDevices => {
                    println!("Running audio list devices test...");
                    audio_capture::test_list_devices()?;
                }
                AudioTest::CaptureAll => {
                    println!("Running capture all devices test...");
                    audio_capture::test_capture_all_devices(15)?;
                }
                AudioTest::MuteUnmute => {
                    println!("Running mute/unmute test...");
                    audio_capture::test_mute_unmute()?;
                }
                AudioTest::Capture30s => {
                    println!("Running 30s capture test...");
                    audio_capture::test_capture_30s(mic_id.as_deref())?;
                }
            }
            println!("Audio test finished.");
        }
        Commands::Camera {
            test_type,
            camera_name,
        } => {
            match test_type {
                CameraTest::ListDevices => {
                    println!("Running camera list devices test...");
                    camera::test_list_cameras()?;
                }
                CameraTest::Share30s => {
                    println!("Running camera 30s test...");
                    camera::test_camera_30s(camera_name.as_deref())?;
                }
                CameraTest::TrackSubscribe => {
                    println!("Running camera track subscribe test...");
                    camera::test_camera_track_subscribe().await?;
                }
            }
            println!("Camera test finished.");
        }
        Commands::LocalDrawing { test_type } => {
            match test_type {
                LocalDrawingTest::Permanent => {
                    println!("Running local drawing test (permanent)...");
                    local_drawing::test_local_drawing_permanent()?;
                }
                LocalDrawingTest::NonPermanent => {
                    println!("Running local drawing test (non-permanent)...");
                    local_drawing::test_local_drawing_non_permanent()?;
                }
            }
            println!("Local drawing test finished.");
        }
    }

    Ok(())
}
