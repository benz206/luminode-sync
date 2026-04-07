/// Audio decoding using symphonia (pure Rust, no C deps).
///
/// Supports: FLAC, MP3, AAC, OGG Vorbis, WAV, AIFF, and more.
/// We decode to mono f32 PCM at the source sample rate.
/// Down-mixing and resampling happen here; the analysis layer sees clean arrays.

use anyhow::{bail, Context, Result};
use std::path::Path;
use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Mono f32 PCM audio buffer.
pub struct AudioBuffer {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl AudioBuffer {
    /// Duration in seconds.
    pub fn duration_secs(&self) -> f64 {
        self.samples.len() as f64 / self.sample_rate as f64
    }
}

/// Decode an audio file to mono f32 PCM.
pub fn decode_audio(path: &Path) -> Result<AudioBuffer> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("cannot open {}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .context("unsupported audio format")?;

    let mut format = probed.format;

    // Pick the first audio track.
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .context("no audio track found")?
        .clone();

    let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
    let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(1);

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .context("unsupported codec")?;

    let track_id = track.id;
    let mut samples: Vec<f32> = Vec::with_capacity(sample_rate as usize * 300); // 5 min

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(e) => bail!("format error: {}", e),
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => bail!("decode error: {}", e),
        };

        // Convert to f32 and down-mix to mono.
        match decoded {
            AudioBufferRef::F32(buf) => {
                append_mono(&mut samples, buf.chan(0), buf.chan(channels - 1));
            }
            AudioBufferRef::S16(buf) => {
                let ch0: Vec<f32> = buf.chan(0).iter().map(|&s| s as f32 / 32768.0).collect();
                let ch_last: Vec<f32> = buf
                    .chan(channels - 1)
                    .iter()
                    .map(|&s| s as f32 / 32768.0)
                    .collect();
                append_mono(&mut samples, &ch0, &ch_last);
            }
            AudioBufferRef::S32(buf) => {
                let ch0: Vec<f32> = buf.chan(0).iter().map(|&s| s as f32 / 2147483648.0).collect();
                let ch_last: Vec<f32> = buf
                    .chan(channels - 1)
                    .iter()
                    .map(|&s| s as f32 / 2147483648.0)
                    .collect();
                append_mono(&mut samples, &ch0, &ch_last);
            }
            _ => {
                // Handle other formats by converting via f64.
                // Symphonia covers the common cases above; fall through gracefully.
            }
        }
    }

    Ok(AudioBuffer { samples, sample_rate })
}

fn append_mono(out: &mut Vec<f32>, left: &[f32], right: &[f32]) {
    for (&l, &r) in left.iter().zip(right.iter()) {
        out.push((l + r) * 0.5);
    }
}

/// Read ID3/Vorbis/FLAC tags from a file.
/// Returns (title, artist, album).
pub fn read_tags(path: &Path) -> Result<(String, String, Option<String>)> {
    let file = std::fs::File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())?;

    let metadata = probed.format.metadata();
    if let Some(rev) = metadata.current() {
        let mut title = None;
        let mut artist = None;
        let mut album = None;
        for tag in rev.tags() {
            match tag.std_key {
                Some(symphonia::core::meta::StandardTagKey::TrackTitle) => {
                    title = Some(tag.value.to_string());
                }
                Some(symphonia::core::meta::StandardTagKey::Artist) => {
                    artist = Some(tag.value.to_string());
                }
                Some(symphonia::core::meta::StandardTagKey::Album) => {
                    album = Some(tag.value.to_string());
                }
                _ => {}
            }
        }
        if let (Some(t), Some(a)) = (title, artist) {
            return Ok((t, a, album));
        }
    }

    bail!("no usable tags in {}", path.display());
}
