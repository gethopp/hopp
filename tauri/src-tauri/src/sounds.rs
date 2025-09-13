use std::{
    fs::File,
    io::BufReader,
    sync::mpsc::{Receiver, Sender},
};

use rodio::{Decoder, OutputStream, Sink};

// Sound timeout and volume constants
const SOUND_COMMAND_TIMEOUT_MS: u64 = 100;
const DEFAULT_VOLUME: f32 = 0.4;
const LOW_VOLUME: f32 = 0.1;
const MEDIUM_VOLUME: f32 = 0.3;
const DEFAULT_SPEED: f32 = 1.0;
const FAST_SPEED: f32 = 1.1;

/// Custom error types for sound-related operations.
///
/// This enum provides specific error variants for different failure modes
/// that can occur during sound playback operations.
#[derive(Debug, thiserror::Error)]
pub enum SoundsError {
    /// Failed to create the default audio output stream.
    #[error("Failed to create audio stream")]
    StreamCreationError,
    /// Failed to create an audio sink for playback.
    #[error("Failed to create audio sink")]
    SinkCreationError,
    /// Failed to open the specified sound file.
    #[error("Failed to open sound file")]
    FileOpenError,
    /// Failed to create an audio source from the sound file.
    #[error("Failed to create audio source")]
    SourceCreationError,
}

/// Commands that can be sent to control sound playback.
///
/// These commands are sent through a channel to communicate with the sound playback thread.
pub enum SoundCommand {
    /// Stop the current sound playback immediately.
    /// This will halt playback and clear the audio sink.
    Stop,
    /// Ping command to keep the playback thread alive.
    /// Used for thread communication without affecting playback.
    Ping,
}

/// Configuration settings for sound playback.
///
/// This struct contains all the parameters needed to customize how a sound is played,
/// including volume, speed, and loop behavior.
#[derive(Default)]
pub struct SoundConfig {
    /// Whether the sound should loop continuously until stopped.
    pub looped: bool,
    /// Volume level for playback, typically between 0.0 (silent) and 1.0 (full volume).
    pub volume: f32,
    /// Playback speed multiplier. 1.0 is normal speed.
    pub speed: f32,
}

/// Represents an active sound with its control channel.
///
/// This struct holds the metadata for a playing sound and provides a way to send
/// commands (like stop) to the sound's playback thread.
pub struct SoundEntry {
    /// Human-readable name or identifier for the sound.
    /// This can be used for logging or debugging purposes.
    pub name: String,
    /// Channel sender for sending commands to the sound's playback thread.
    /// Use this to send `SoundCommand` variants to control playback.
    pub tx: Sender<SoundCommand>,
}

/// Plays a sound file with the specified configuration and control channel.
///
/// This function creates an audio stream and sink, loads the specified sound file,
/// and plays it according to the provided configuration. It runs in a loop listening
/// for commands through the provided receiver channel.
///
/// # Arguments
///
/// * `sound_path` - Path to the sound file to play
/// * `config` - Configuration settings for playback (volume, speed, looping)
/// * `rx` - Receiver channel for receiving playback control commands
///
/// # Returns
///
/// * `Ok(())` - Sound played successfully and completed
/// * `Err(SoundsError)` - An error occurred during setup or playback
pub fn play_sound(
    sound_path: String,
    config: SoundConfig,
    rx: Receiver<SoundCommand>,
) -> Result<(), SoundsError> {
    let (_stream, stream_handle) =
        OutputStream::try_default().map_err(|_| SoundsError::StreamCreationError)?;
    let sink = Sink::try_new(&stream_handle).map_err(|_| SoundsError::SinkCreationError)?;

    sink.set_volume(config.volume);
    sink.set_speed(config.speed);

    let file = File::open(sound_path.clone()).map_err(|_| SoundsError::FileOpenError)?;
    let file = BufReader::new(file);
    if config.looped {
        let source = Decoder::new_looped(file).map_err(|_| SoundsError::SourceCreationError)?;
        sink.append(source);
    } else {
        let source = Decoder::new(file).map_err(|_| SoundsError::SourceCreationError)?;
        sink.append(source);
    };
    sink.play();

    loop {
        match rx.recv_timeout(std::time::Duration::from_millis(SOUND_COMMAND_TIMEOUT_MS)) {
            Ok(SoundCommand::Stop) => {
                sink.stop();
                sink.clear();
                break;
            }
            Ok(SoundCommand::Ping) => {}
            _ => {}
        }
        if sink.empty() {
            break;
        }
    }
    Ok(())
}

/// Returns a list of all available sounds with their default configurations.
///
/// This function provides a predefined collection of sound files that are available
/// in the application, along with their recommended playback settings. Each sound
/// is configured with appropriate volume, speed, and looping behavior.
///
/// # Returns
///
/// A vector of tuples where each tuple contains:
/// * `&'static str` - The file path to the sound resource
/// * `SoundConfig` - The recommended configuration for that sound
pub fn get_all_sounds() -> Vec<(&'static str, SoundConfig)> {
    vec![
        (
            "resources/sounds/ring1.mp3",
            SoundConfig {
                speed: DEFAULT_SPEED,
                volume: DEFAULT_VOLUME,
                looped: true,
            },
        ),
        (
            "resources/sounds/rejected.mp3",
            SoundConfig {
                speed: FAST_SPEED,
                volume: LOW_VOLUME,
                looped: false,
            },
        ),
        (
            "resources/sounds/bubble-pop.mp3",
            SoundConfig {
                speed: DEFAULT_SPEED,
                volume: DEFAULT_VOLUME,
                looped: false,
            },
        ),
        (
            "resources/sounds/call-rejected.mp3",
            SoundConfig {
                speed: DEFAULT_SPEED,
                volume: DEFAULT_VOLUME,
                looped: false,
            },
        ),
        (
            "resources/sounds/incoming-call.mp3",
            SoundConfig {
                speed: DEFAULT_SPEED,
                volume: MEDIUM_VOLUME,
                looped: true,
            },
        ),
    ]
}
