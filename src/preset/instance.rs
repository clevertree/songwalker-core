use std::sync::Arc;
use crate::preset::types::{PresetDescriptor, SampleZone};

/// A fully prepared preset instance, ready for rendering.
/// Contains metadata + decoded PCM sample buffer.
pub struct PresetInstance {
    /// The original preset descriptor (metadata, graph info).
    pub descriptor: PresetDescriptor,
    /// Decoded sample zones with PCM data.
    pub zones: Vec<LoadedZone>,
}

/// A sample zone with decoded PCM audio data.
pub struct LoadedZone {
    /// The original zone descriptor (key range, pitch, loop points, etc.).
    pub zone: SampleZone,
    /// Decoded audio data (mono or interleaved stereo) at the host sample rate.
    pub pcm_data: Arc<[f32]>,
    /// Number of channels (1=mono, 2=stereo).
    pub channels: u16,
    /// Original sample rate.
    pub sample_rate: u32,
}

impl PresetInstance {
    /// Find the best matching zone for a given note and velocity.
    pub fn find_zone(&self, note: u8, velocity: f32) -> Option<&LoadedZone> {
        self.find_zone_indexed(note, velocity).map(|(_, z)| z)
    }

    /// Find the best matching zone, returning its index and a reference.
    pub fn find_zone_indexed(&self, note: u8, velocity: f32) -> Option<(usize, &LoadedZone)> {
        let vel_u8 = (velocity * 127.0) as u8;
        self.zones.iter().enumerate().find(|(_, z)| {
            note >= z.zone.key_range.low
                && note <= z.zone.key_range.high
                && z.zone
                    .velocity_range
                    .as_ref()
                    .map_or(true, |vr| vel_u8 >= vr.low && vel_u8 <= vr.high)
        })
    }
}

impl LoadedZone {
    /// Get the native sample rate of this zone's pitch info.
    pub fn pitch(&self) -> &crate::preset::types::ZonePitch {
        &self.zone.pitch
    }

    /// Get the sample rate this zone was decoded at.
    pub fn sample_rate(&self) -> u32 {
        self.zone.sample_rate
    }
}
