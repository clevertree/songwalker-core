//! WAV renderer — renders an EventList to a WAV byte buffer.

use crate::compiler::EventList;
use super::engine::AudioEngine;

/// Render an EventList to a WAV file as bytes (16-bit stereo PCM).
pub fn render_wav(event_list: &EventList, sample_rate: u32) -> Vec<u8> {
    let engine = AudioEngine::new(sample_rate as f64);
    let pcm = engine.render_pcm_i16(event_list);

    encode_wav(&pcm, sample_rate, 2)
}

/// Encode interleaved i16 PCM samples to a WAV byte buffer.
fn encode_wav(samples: &[i16], sample_rate: u32, channels: u16) -> Vec<u8> {
    let bits_per_sample: u16 = 16;
    let byte_rate = sample_rate * channels as u32 * (bits_per_sample as u32 / 8);
    let block_align = channels * (bits_per_sample / 8);
    let data_size = (samples.len() * 2) as u32;
    let file_size = 36 + data_size;

    let mut buf = Vec::with_capacity(44 + data_size as usize);

    // RIFF header
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&file_size.to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    // fmt chunk
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // chunk size
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM format
    buf.extend_from_slice(&channels.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits_per_sample.to_le_bytes());

    // data chunk
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_size.to_le_bytes());
    for &sample in samples {
        buf.extend_from_slice(&sample.to_le_bytes());
    }

    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::{EndMode, Event, EventKind, EventList, InstrumentConfig};

    #[test]
    fn wav_header_valid() {
        let song = EventList {
            events: vec![Event {
                time: 0.0,
                kind: EventKind::Note {
                    pitch: "C4".to_string(),
                    velocity: 100.0,
                    gate: 1.0,
                    instrument: InstrumentConfig::default(),
                    source_start: 0,
                    source_end: 0,
                },
            }],
            total_beats: 1.0,
            end_mode: EndMode::Gate,
        };

        let wav = render_wav(&song, 44100);

        // Check RIFF header
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");

        // Check sample rate
        let sr = u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]);
        assert_eq!(sr, 44100);

        // Check channels
        let ch = u16::from_le_bytes([wav[22], wav[23]]);
        assert_eq!(ch, 2);
    }

    #[test]
    fn wav_size_correct() {
        let song = EventList {
            events: vec![],
            total_beats: 1.0,
            end_mode: EndMode::Gate,
        };

        let wav = render_wav(&song, 44100);

        // 1 beat at 120 BPM = 0.5s = 22050 samples * 2 channels * 2 bytes = 88200 data bytes
        let data_size = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]);
        assert_eq!(data_size, 88200);
        assert_eq!(wav.len(), 44 + 88200);
    }

    #[test]
    fn full_pipeline_parse_compile_render() {
        // End-to-end test: parse SW source, compile, render to WAV
        let source = r#"
track.beatsPerMinute = 120;
riff();

track riff() {
    C4 /4
    E4 /4
    G4 /4
    C5 /4
}
"#;
        let program = crate::parse(source).expect("parse failed");
        let event_list = crate::compiler::compile(&program).expect("compile failed");
        let wav = render_wav(&event_list, 22050); // lower rate for faster test

        // Should produce a valid WAV
        assert_eq!(&wav[0..4], b"RIFF");
        assert!(wav.len() > 44, "WAV should have audio data");

        // Verify it's not all silence
        let data_start = 44;
        let mut has_nonzero = false;
        for i in (data_start..wav.len()).step_by(2) {
            if i + 1 < wav.len() {
                let sample = i16::from_le_bytes([wav[i], wav[i + 1]]);
                if sample != 0 {
                    has_nonzero = true;
                    break;
                }
            }
        }
        assert!(has_nonzero, "Rendered WAV should contain non-silent audio");
    }
}
