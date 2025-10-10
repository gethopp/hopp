use clap::{Parser, Subcommand, ValueEnum};
use std::io;

mod events;
mod livekit_utils;
mod remote_cursor;
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
    /// Test screenshare functionality
    Screenshare,
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
    /// Click animation
    ClickAnimation,
    /// Test transitions: Remote enabled with animation toggling
    TransitionsRemoteEnabledAnimation,
    /// Test transitions: Remote enabled then disabled
    TransitionsRemoteEnabledThenDisabled,
    /// Test transitions: Remote disabled with animation
    TransitionsRemoteDisabledAnimation,
    /// Test transitions: Mixed remote control and animation
    TransitionsMixed,
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
                CursorTest::ClickAnimation => {
                    println!("Running click animation test...");
                    remote_cursor::test_click_animation().await?;
                }
                CursorTest::TransitionsRemoteEnabledAnimation => {
                    println!("Running transitions test: Remote enabled with animation...");
                    remote_cursor::test_transitions_remote_enabled_with_animation().await?;
                }
                CursorTest::TransitionsRemoteEnabledThenDisabled => {
                    println!("Running transitions test: Remote enabled then disabled...");
                    remote_cursor::test_transitions_remote_enabled_then_disabled().await?;
                }
                CursorTest::TransitionsRemoteDisabledAnimation => {
                    println!("Running transitions test: Remote disabled with animation...");
                    remote_cursor::test_transitions_remote_disabled_with_animation().await?;
                }
                CursorTest::TransitionsMixed => {
                    println!("Running transitions test: Mixed remote and animation...");
                    remote_cursor::test_transitions_mixed_remote_and_animation().await?;
                }
            }
            println!("Cursor test finished.");
        }
        Commands::Keyboard => {
            println!("Running keyboard test...");
            remote_keyboard::test_keyboard_chars().await?;
            println!("Keyboard test finished.");
        }
        Commands::Screenshare => {
            println!("Running screenshare test...");
            screenshare_client::screenshare_test()?;
        }
    }

    Ok(())
}
