//! Synthesized UI beeps for pipeline state transitions.
//!
//! Each pipeline phase gets a short, distinct tone so the user can tell by ear
//! what the app is doing without watching the capsule. Playback runs on a
//! detached thread and any failure (no output device, etc.) is logged at debug
//! and ignored — a missing beep must never disrupt the recording pipeline.

use std::time::Duration;

/// A pipeline phase that has an associated audio cue.
#[derive(Clone, Copy, Debug)]
pub enum Cue {
    /// Recording started — the mic is now listening.
    Listening,
    /// Audio captured; transcription in progress.
    Transcribing,
    /// Transcript is being polished by the LLM.
    Polishing,
    /// Final text is being typed/pasted into the target app.
    Output,
}

struct Tone {
    freq: f32,
    ms: u64,
}

/// The note sequence for each cue. Rising = "starting", falling = "finishing",
/// single blips = intermediate work, each at a distinct pitch/timbre.
fn tones(cue: Cue) -> Vec<Tone> {
    match cue {
        // Rising two-note chirp — "I'm listening".
        Cue::Listening => vec![
            Tone {
                freq: 660.0,
                ms: 90,
            },
            Tone {
                freq: 988.0,
                ms: 110,
            },
        ],
        // Single mid blip.
        Cue::Transcribing => vec![Tone {
            freq: 523.0,
            ms: 120,
        }],
        // Single higher blip.
        Cue::Polishing => vec![Tone {
            freq: 784.0,
            ms: 120,
        }],
        // Falling two-note chirp — "done, sending".
        Cue::Output => vec![
            Tone {
                freq: 988.0,
                ms: 90,
            },
            Tone {
                freq: 659.0,
                ms: 130,
            },
        ],
    }
}

/// Play the cue on a detached thread. Never blocks the caller and never panics.
pub fn play(cue: Cue) {
    std::thread::spawn(move || {
        if let Err(e) = play_inner(cue) {
            tracing::debug!("UI sound playback skipped: {e}");
        }
    });
}

fn play_inner(cue: Cue) -> anyhow::Result<()> {
    use rodio::source::{SineWave, Source};
    use rodio::{OutputStream, Sink};

    // Open the default output device for the duration of the cue only.
    let (_stream, handle) = OutputStream::try_default()?;
    let sink = Sink::try_new(&handle)?;
    for tone in tones(cue) {
        let dur = Duration::from_millis(tone.ms);
        let source = SineWave::new(tone.freq)
            .take_duration(dur)
            .amplify(0.18)
            .fade_in(Duration::from_millis(6));
        sink.append(source);
    }
    sink.sleep_until_end();
    Ok(())
}
