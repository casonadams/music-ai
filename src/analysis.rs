use std::{fs::File, path::Path};

use anyhow::{Context, Result, anyhow};
use ebur128::{EbuR128, Mode};
use serde::{Deserialize, Serialize};
use symphonia::core::{
    codecs::audio::AudioDecoderOptions,
    errors::Error as DecodeError,
    formats::{FormatOptions, TrackType, probe::Hint},
    io::{MediaSource, MediaSourceStream},
    meta::MetadataOptions,
};

const CLIPPING_DBTP: f64 = -0.1;
const NEAR_SILENT_LUFS: f64 = -60.0;

/// Objective measurements of a rendered song (EBU R128 loudness, true peak).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Analysis {
    pub duration_secs: f64,
    pub sample_rate: u32,
    pub channels: u32,
    /// Integrated loudness; `None` when the audio is digital silence.
    pub integrated_lufs: Option<f64>,
    pub loudness_range_lu: Option<f64>,
    pub true_peak_dbtp: Option<f64>,
    pub clipping: bool,
    pub near_silent: bool,
}

pub fn analyze_file(path: &Path) -> Result<Analysis> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    analyze(Box::new(file), &hint).with_context(|| format!("analyzing {}", path.display()))
}

fn analyze(source: Box<dyn MediaSource>, hint: &Hint) -> Result<Analysis> {
    let stream = MediaSourceStream::new(source, Default::default());
    let mut format = symphonia::default::get_probe().probe(
        hint,
        stream,
        FormatOptions::default(),
        MetadataOptions::default(),
    )?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| anyhow!("no audio track"))?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| anyhow!("unsupported audio codec"))?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())?;
    let track_id = track.id;

    let mut meter: Option<(EbuR128, u32, u32)> = None;
    let mut samples: Vec<f32> = Vec::new();
    let mut frames = 0u64;
    while let Some(packet) = format.next_packet()? {
        if packet.track_id != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(DecodeError::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        };
        let (meter, _, channels) = match &mut meter {
            Some(m) => m,
            None => {
                let spec = decoded.spec();
                let channels = u32::try_from(spec.channels().count())?;
                let ebu =
                    EbuR128::new(channels, spec.rate(), Mode::I | Mode::LRA | Mode::TRUE_PEAK)?;
                meter.insert((ebu, spec.rate(), channels))
            }
        };
        samples.resize(decoded.samples_interleaved(), 0.0);
        decoded.copy_to_slice_interleaved(&mut samples);
        meter.add_frames_f32(&samples)?;
        frames += (samples.len() / *channels as usize) as u64;
    }

    let (meter, sample_rate, channels) = meter.ok_or_else(|| anyhow!("no audio decoded"))?;
    let finite = |v: f64| v.is_finite().then(|| round1(v));
    let integrated_lufs = meter.loudness_global().ok().and_then(finite);
    let peak = (0..channels)
        .filter_map(|c| meter.true_peak(c).ok())
        .fold(0.0_f64, f64::max);
    let true_peak_dbtp = finite(20.0 * peak.log10());
    Ok(Analysis {
        duration_secs: round1(frames as f64 / f64::from(sample_rate)),
        sample_rate,
        channels,
        integrated_lufs,
        loudness_range_lu: meter.loudness_range().ok().and_then(finite),
        true_peak_dbtp,
        clipping: true_peak_dbtp.is_some_and(|p| p >= CLIPPING_DBTP),
        near_silent: integrated_lufs.is_none_or(|l| l <= NEAR_SILENT_LUFS),
    })
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use std::{f32::consts::TAU, io::Cursor};

    use super::*;

    const RATE: u32 = 44_100;

    /// 16-bit PCM WAV of a 440 Hz sine at `amplitude` (0.0 = silence).
    fn sine_wav(amplitude: f32, channels: u16, secs: u32) -> Vec<u8> {
        let frames = RATE * secs;
        let mut data = Vec::new();
        for n in 0..frames {
            let v = (amplitude * (TAU * 440.0 * n as f32 / RATE as f32).sin() * 32767.0) as i16;
            for _ in 0..channels {
                data.extend_from_slice(&v.to_le_bytes());
            }
        }
        let block = channels * 2;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&channels.to_le_bytes());
        wav.extend_from_slice(&RATE.to_le_bytes());
        wav.extend_from_slice(&(RATE * u32::from(block)).to_le_bytes());
        wav.extend_from_slice(&block.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        wav
    }

    fn run(wav: Vec<u8>) -> Analysis {
        let mut hint = Hint::new();
        hint.with_extension("wav");
        analyze(Box::new(Cursor::new(wav)), &hint).unwrap()
    }

    #[test]
    fn measures_duration_format_and_peak() {
        let a = run(sine_wav(0.5, 2, 2));
        assert_eq!(a.duration_secs, 2.0);
        assert_eq!(a.sample_rate, RATE);
        assert_eq!(a.channels, 2);
        assert!((a.true_peak_dbtp.unwrap() - -6.0).abs() < 0.2);
        assert!(!a.clipping && !a.near_silent);
    }

    #[test]
    fn louder_audio_measures_louder() {
        let quiet = run(sine_wav(0.1, 2, 2)).integrated_lufs.unwrap();
        let loud = run(sine_wav(0.5, 2, 2)).integrated_lufs.unwrap();
        assert!((loud - quiet - 14.0).abs() < 0.5, "{quiet} vs {loud}");
    }

    #[test]
    fn full_scale_audio_is_flagged_as_clipping() {
        assert!(run(sine_wav(1.0, 1, 2)).clipping);
    }

    #[test]
    fn silence_has_no_loudness_and_is_flagged() {
        let a = run(sine_wav(0.0, 2, 2));
        assert_eq!(a.integrated_lufs, None);
        assert!(a.near_silent);
    }

    #[test]
    fn non_audio_is_an_error() {
        let mut hint = Hint::new();
        hint.with_extension("wav");
        assert!(analyze(Box::new(Cursor::new(b"not audio".to_vec())), &hint).is_err());
    }
}
