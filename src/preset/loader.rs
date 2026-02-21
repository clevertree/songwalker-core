use std::sync::Arc;

use base64::Engine as _;
use crate::preset::{
    AudioCodec, AudioReference, LibraryIndex, PresetDescriptor, SampleZone,
    LoadedZone, PresetInstance,
};

use super::cache::DiskCache;

/// Default base URL for the songwalker-library.
pub const DEFAULT_LIBRARY_URL: &str = "https://clevertree.github.io/songwalker-library";

/// Fetches preset data from a remote library and manages decoding.
pub struct PresetLoader {
    /// Base URL for the preset library.
    base_url: String,
    /// HTTP client (reused across requests).
    client: reqwest::Client,
    /// Disk cache for persistence.
    cache: DiskCache,
}

impl PresetLoader {
    pub fn new() -> Self {
        Self {
            base_url: DEFAULT_LIBRARY_URL.to_string(),
            client: reqwest::Client::builder()
                .user_agent("SongWalker-VSTi/0.1")
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            cache: DiskCache::new(),
        }
    }

    pub fn with_base_url(mut self, url: String) -> Self {
        self.base_url = url;
        self
    }

    /// Initialize: ensure cache directories exist.
    pub fn init(&self) {
        let _ = self.cache.ensure_dirs();
    }

    /// Fetch the root library index from the network.
    ///
    /// Always fetches from network to get fresh data. Caches the result on disk.
    pub async fn fetch_root_index(&self) -> Result<serde_json::Value, String> {
        // Fetch from network
        let url = format!("{}/index.json", self.base_url);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch root index {}: {}", url, e))?;

        if !response.status().is_success() {
            return Err(format!("Network error {} fetching root index: {}", response.status(), url));
        }

        let text = response
            .text()
            .await
            .map_err(|e| format!("Failed to read root index response: {}", e))?;

        // Cache it
        let _ = self.cache.write_root_index(&text);

        serde_json::from_str(&text).map_err(|e| format!("Failed to parse root index: {}", e))
    }

    /// Fetch a specific library's index by relative path (e.g. "Aspirin/index.json").
    ///
    /// `cache_key` is used for disk cache (typically the library display name).
    /// Returns the raw JSON Value for flexible parsing by PresetManager.
    pub async fn fetch_library_index_by_path(
        &self,
        path: &str,
        cache_key: &str,
    ) -> Result<serde_json::Value, String> {
        // Try cache
        if let Some(cached) = self.cache.read_library_index(cache_key) {
            if let Ok(val) = serde_json::from_str(&cached) {
                return Ok(val);
            }
        }

        // Fetch from network
        let url = format!("{}/{}", self.base_url, path);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch library index {}: {}", url, e))?;

        if !response.status().is_success() {
            return Err(format!("Network error {} fetching library index: {}", response.status(), url));
        }

        let text = response
            .text()
            .await
            .map_err(|e| format!("Failed to read library index response: {}", e))?;

        // Cache it
        let _ = self.cache.write_library_index(cache_key, &text);

        serde_json::from_str(&text)
            .map_err(|e| format!("Failed to parse library index {}: {}", path, e))
    }

    /// Fetch a specific library's index (legacy — uses songwalker-core LibraryIndex type).
    pub async fn fetch_library_index(
        &self,
        library: &str,
    ) -> Result<LibraryIndex, String> {
        // Try cache
        if let Some(cached) = self.cache.read_library_index(library) {
            if let Ok(index) = serde_json::from_str(&cached) {
                return Ok(index);
            }
        }

        // Fetch from network
        let url = format!("{}/{}/index.json", self.base_url, library);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch library index for {}: {}", library, e))?;

        let text = response
            .text()
            .await
            .map_err(|e| format!("Failed to read response: {}", e))?;

        let _ = self.cache.write_library_index(library, &text);

        serde_json::from_str(&text)
            .map_err(|e| format!("Failed to parse library index for {}: {}", library, e))
    }

    /// Fetch and fully load a preset (descriptor + all sample data).
    ///
    /// Returns a `PresetInstance` ready for use on the audio thread.
    pub async fn load_preset(
        &self,
        library: &str,
        preset_path: &str,
        host_sample_rate: f32,
    ) -> Result<PresetInstance, String> {
        // Fetch preset descriptor
        let descriptor = self.fetch_preset_descriptor(library, preset_path).await?;

        // Load all sample zones
        let zones = self
            .load_zones(library, preset_path, &descriptor, host_sample_rate)
            .await?;

        Ok(PresetInstance { descriptor, zones })
    }

    /// Fetch preset JSON descriptor.
    async fn fetch_preset_descriptor(
        &self,
        library: &str,
        preset_path: &str,
    ) -> Result<PresetDescriptor, String> {
        // Try cache
        if let Some(cached) = self.cache.read_preset(library, preset_path) {
            if let Ok(desc) = serde_json::from_str(&cached) {
                return Ok(desc);
            }
        }

        // Fetch from network
        let url = format!("{}/{}/{}", self.base_url, library, preset_path);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch preset {}: {}", url, e))?;

        if !response.status().is_success() {
            return Err(format!("Network error {} fetching preset: {}", response.status(), url));
        }

        let text = response
            .text()
            .await
            .map_err(|e| format!("Failed to read preset response: {}", e))?;

        let _ = self.cache.write_preset(library, preset_path, &text);

        serde_json::from_str(&text)
            .map_err(|e| format!("Failed to parse preset {}/{}: {}", library, preset_path, e))
    }

    /// Load and decode all sample zones from a preset.
    async fn load_zones(
        &self,
        library: &str,
        preset_path: &str,
        descriptor: &PresetDescriptor,
        host_sample_rate: f32,
    ) -> Result<Vec<LoadedZone>, String> {
        let zones = extract_zones(&descriptor.graph);
        let mut loaded = Vec::with_capacity(zones.len());

        for zone in zones {
            let pcm = self
                .load_sample(library, preset_path, &zone.audio, zone.sample_rate, host_sample_rate)
                .await?;

            loaded.push(LoadedZone {
                zone: zone.clone(),
                pcm_data: Arc::from(pcm),
                channels: 1, // TODO: detect stereo
                sample_rate: zone.sample_rate,
            });
        }

        Ok(loaded)
    }

    /// Load a single sample (from cache or network), decode to f32 PCM.
    async fn load_sample(
        &self,
        library: &str,
        preset_path: &str,
        audio_ref: &AudioReference,
        _source_sample_rate: u32,
        _host_sample_rate: f32,
    ) -> Result<Vec<f32>, String> {
        let cache_key = audio_ref_cache_key(audio_ref);

        // Check disk cache
        if let Some(cached) = self.cache.read_sample(library, preset_path, &cache_key) {
            return Ok(cached);
        }

        // Fetch and decode
        let raw_bytes = match audio_ref {
            AudioReference::External { url, .. } => {
                let full_url = if url.starts_with("http") {
                    url.clone()
                } else {
                    // The URL is relative to the preset.json file location.
                    // Resolve it by combining the library path and preset directory.
                    let preset_dir = preset_path
                        .rsplit_once('/')
                        .map(|(dir, _)| dir)
                        .unwrap_or("");
                    if preset_dir.is_empty() {
                        format!("{}/{}/{}", self.base_url, library, url)
                    } else {
                        format!("{}/{}/{}/{}", self.base_url, library, preset_dir, url)
                    }
                };
                let response = self.client
                    .get(&full_url)
                    .send()
                    .await
                    .map_err(|e| format!("Failed to fetch sample {}: {}", full_url, e))?;
                if !response.status().is_success() {
                    return Err(format!("HTTP {} fetching sample: {}", response.status(), full_url));
                }
                response
                    .bytes()
                    .await
                    .map_err(|e| format!("Failed to read sample bytes: {}", e))?
                    .to_vec()
            }
            AudioReference::InlineFile { data, .. } => {
                base64::engine::general_purpose::STANDARD.decode(data)
                    .map_err(|e| format!("Failed to decode base64 sample: {}", e))?
            }
            AudioReference::InlinePcm { data, bits_per_sample } => {
                // Already PCM, base64-decode and convert
                let decoded = base64::engine::general_purpose::STANDARD.decode(data)
                    .map_err(|e| format!("Failed to decode inline PCM: {}", e))?;
                let samples = decode_raw_pcm(&decoded, *bits_per_sample);
                let _ = self.cache.write_sample(library, preset_path, &cache_key, &samples);
                return Ok(samples);
            }
            AudioReference::ContentAddressed { hash, .. } => {
                let url = format!("{}/{}/{}", self.base_url, library, hash);
                let response = self.client
                    .get(&url)
                    .send()
                    .await
                    .map_err(|e| format!("Failed to fetch content-addressed sample {}: {}", url, e))?;
                if !response.status().is_success() {
                    return Err(format!("HTTP {} fetching sample: {}", response.status(), url));
                }
                response
                    .bytes()
                    .await
                    .map_err(|e| format!("Failed to read sample bytes: {}", e))?
                    .to_vec()
            }
        };

        // Decode audio to f32 PCM
        let codec = audio_ref_codec(audio_ref);
        let samples = decode_audio(&raw_bytes, &codec)?;

        // TODO: Resample if source_sample_rate != host_sample_rate

        // Cache the decoded PCM
        let _ = self.cache.write_sample(library, preset_path, &cache_key, &samples);

        Ok(samples)
    }
}

/// Extract all SampleZones from a preset graph (recursively for composites).
fn extract_zones(node: &crate::preset::types::PresetNode) -> Vec<SampleZone> {
    match node {
        crate::preset::types::PresetNode::Sampler { config } => {
            config.zones.clone()
        }
        crate::preset::types::PresetNode::Composite { children, .. } => {
            children.iter().flat_map(|c| extract_zones(c)).collect()
        }
        _ => Vec::new(),
    }
}

/// Get a cache key for an audio reference.
fn audio_ref_cache_key(audio_ref: &AudioReference) -> String {
    match audio_ref {
        AudioReference::External { url, .. } => url.clone(),
        AudioReference::ContentAddressed { hash, .. } => hash.clone(),
        AudioReference::InlineFile { data, .. } => {
            // Hash the base64 data as cache key
            format!("inline:{}", &data[..data.len().min(64)])
        }
        AudioReference::InlinePcm { .. } => "inline-pcm".to_string(),
    }
}

/// Get the codec of an audio reference.
fn audio_ref_codec(audio_ref: &AudioReference) -> AudioCodec {
    match audio_ref {
        AudioReference::External { codec, .. } => codec.clone(),
        AudioReference::InlineFile { codec, .. } => codec.clone(),
        AudioReference::ContentAddressed { codec, .. } => codec.clone(),
        AudioReference::InlinePcm { .. } => AudioCodec::Wav,
    }
}

/// Decode raw audio bytes to f32 PCM based on codec.
fn decode_audio(bytes: &[u8], codec: &AudioCodec) -> Result<Vec<f32>, String> {
    if bytes.is_empty() {
        return Err("Cannot decode empty audio data".to_string());
    }
    let samples = match codec {
        AudioCodec::Mp3 => decode_mp3(bytes)?,
        AudioCodec::Wav => decode_wav(bytes)?,
        AudioCodec::Raw => decode_raw_pcm(bytes, 16), // Raw = 16-bit signed LE PCM
        _ => return Err(format!("Unsupported codec: {:?}", codec)),
    };
    if samples.is_empty() {
        return Err(format!("Decoded 0 samples from {} bytes ({:?} codec)", bytes.len(), codec));
    }
    Ok(samples)
}

/// Decode MP3 bytes to f32 samples.
fn decode_mp3(bytes: &[u8]) -> Result<Vec<f32>, String> {
    let mut decoder = minimp3::Decoder::new(std::io::Cursor::new(bytes));
    let mut samples = Vec::new();

    loop {
        match decoder.next_frame() {
            Ok(frame) => {
                for s in &frame.data {
                    samples.push(*s as f32 / 32768.0);
                }
            }
            Err(minimp3::Error::Eof) => break,
            Err(e) => return Err(format!("MP3 decode error: {:?}", e)),
        }
    }

    Ok(samples)
}

/// Decode WAV bytes to f32 samples.
fn decode_wav(bytes: &[u8]) -> Result<Vec<f32>, String> {
    let cursor = std::io::Cursor::new(bytes);
    let reader =
        hound::WavReader::new(cursor).map_err(|e| format!("WAV decode error: {}", e))?;

    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => reader
            .into_samples::<i32>()
            .filter_map(|s| s.ok())
            .map(|s| s as f32 / (1 << (spec.bits_per_sample - 1)) as f32)
            .collect(),
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .filter_map(|s| s.ok())
            .collect(),
    };

    Ok(samples)
}

/// Decode raw PCM bytes to f32 samples.
fn decode_raw_pcm(bytes: &[u8], bits_per_sample: u8) -> Vec<f32> {
    match bits_per_sample {
        16 => {
            bytes.chunks_exact(2)
                .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]) as f32 / 32768.0)
                .collect()
        }
        24 => {
            bytes.chunks_exact(3)
                .map(|chunk| {
                    let val = (chunk[0] as i32) | ((chunk[1] as i32) << 8) | ((chunk[2] as i32) << 16);
                    // Sign extend from 24 to 32 bits
                    let val = if val & 0x800000 != 0 { val | !0xFFFFFF } else { val };
                    val as f32 / 8388608.0
                })
                .collect()
        }
        32 => {
            bytes.chunks_exact(4)
                .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect()
        }
        _ => {
            // Fallback: treat as 16-bit
            bytes.chunks_exact(2)
                .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]) as f32 / 32768.0)
                .collect()
        }
    }
}
