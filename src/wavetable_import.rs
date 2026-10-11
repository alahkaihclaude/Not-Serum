//! WAV -> wavetable frames, and the compact form stored in plugin state. Runs off the audio thread.

use crate::dsp::wavetable::{Wavetable, FRAME};
use base64::Engine;
use crossbeam_queue::ArrayQueue;
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub const MAX_FRAMES: usize = 256;
/// Larger files are rejected before parsing.
pub const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;

/// Reads the frame size from a `clm ` chunk (e.g. `<!>2048 ...`), the common wavetable-WAV convention.
fn clm_frame_size(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().ok()?) as usize;
        let body = bytes.get(pos + 8..(pos + 8).saturating_add(size).min(bytes.len()))?;
        if id == b"clm " {
            let text = String::from_utf8_lossy(body);
            let digits: String = text.split("<!>").nth(1)?.chars().take_while(|c| c.is_ascii_digit()).collect();
            return digits.parse().ok().filter(|&n| (2..=65536).contains(&n));
        }
        pos += 8 + size + (size & 1); // chunks are padded to even length
    }
    None
}

/// Mono samples in [-1, 1] from any PCM / float WAV hound can read.
fn read_mono(bytes: &[u8]) -> Result<Vec<f32>, String> {
    let reader = hound::WavReader::new(Cursor::new(bytes)).map_err(|e| format!("not a readable WAV file ({e})"))?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.into_samples::<f32>().collect::<Result<_, _>>(),
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample.clamp(1, 32) - 1)) as f32;
            reader.into_samples::<i32>().map(|s| s.map(|s| s as f32 * scale)).collect()
        }
    }
    .map_err(|e| format!("corrupt WAV data ({e})"))?;
    Ok(interleaved
        .chunks(channels)
        .map(|c| {
            let s = c.iter().sum::<f32>() / channels as f32;
            if s.is_finite() { s } else { 0.0 }
        })
        .collect())
}

/// Cyclic linear resampling of one cycle to `FRAME` samples.
fn resample_cycle(cycle: &[f32]) -> Vec<f32> {
    if cycle.len() == FRAME {
        return cycle.to_vec();
    }
    let n = cycle.len();
    (0..FRAME)
        .map(|i| {
            let p = i as f32 * n as f32 / FRAME as f32;
            let j = p as usize;
            let t = p - j as f32;
            cycle[j % n] + (cycle[(j + 1) % n] - cycle[j % n]) * t
        })
        .collect()
}

/// Splits a wavetable WAV into frames of `FRAME` samples (at most `MAX_FRAMES`).
///
/// Frame size comes from the `clm ` chunk, else 2048. If the length isn't a whole number of frames,
/// the whole file is treated as one single cycle.
pub fn parse_wav(bytes: &[u8]) -> Result<Vec<Vec<f32>>, String> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err("file is larger than 64 MB".into());
    }
    let samples = read_mono(bytes)?;
    if samples.len() < 2 {
        return Err("WAV file has no audio".into());
    }
    let frame = clm_frame_size(bytes).unwrap_or(FRAME);
    let frames: Vec<Vec<f32>> = if samples.len() % frame == 0 {
        samples.chunks(frame).take(MAX_FRAMES).map(resample_cycle).collect()
    } else {
        vec![resample_cycle(&samples)]
    };
    Ok(frames)
}

/// A custom table as saved in the project: name plus 16-bit, deflate-compressed, base64 frames.
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct StoredTable {
    pub name: String,
    pub frames: usize,
    pub data: String,
}

impl StoredTable {
    /// Frames are peak-normalized before 16-bit quantization (the table is normalized on load anyway).
    pub fn encode(name: &str, frames: &[Vec<f32>]) -> Self {
        let mut raw = Vec::with_capacity(frames.len() * FRAME * 2);
        for f in frames {
            let peak = f.iter().fold(0.0f32, |m, s| m.max(s.abs())).max(1e-9);
            for s in f {
                raw.extend_from_slice(&(((s / peak).clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
            }
        }
        let packed = miniz_oxide::deflate::compress_to_vec(&raw, 6);
        Self { name: name.to_string(), frames: frames.len(), data: base64::engine::general_purpose::STANDARD.encode(packed) }
    }

    /// Rejects anything malformed rather than panicking: state can come from anywhere.
    pub fn decode(&self) -> Result<Vec<Vec<f32>>, String> {
        if self.frames == 0 || self.frames > MAX_FRAMES {
            return Err(format!("bad frame count {}", self.frames));
        }
        let expected = self.frames * FRAME * 2;
        let packed = base64::engine::general_purpose::STANDARD.decode(&self.data).map_err(|e| e.to_string())?;
        let raw = miniz_oxide::inflate::decompress_to_vec_with_limit(&packed, expected).map_err(|e| format!("{e:?}"))?;
        if raw.len() != expected {
            return Err("stored wavetable has the wrong size".into());
        }
        Ok(raw
            .chunks_exact(FRAME * 2)
            .map(|f| f.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32767.0).collect())
            .collect())
    }
}

/// Hands custom tables between the loader thread, the GUI and the audio thread for one oscillator.
///
/// The audio thread only touches the two lock-free queues: it pops new tables from `incoming` and
/// pushes the ones it replaced onto `retired`, so their memory is freed here, never in `process()`.
pub struct TableSlot {
    incoming: ArrayQueue<Arc<Wavetable>>,
    retired: ArrayQueue<Arc<Wavetable>>,
    /// What the GUI draws: (name, table, stored form). Never locked by the audio thread.
    pub current: Mutex<Option<(String, Arc<Wavetable>, StoredTable)>>,
    /// Last load error, shown in the GUI.
    pub error: Mutex<Option<String>>,
    /// Set after a successful load so the GUI switches the osc's Table parameter to Custom.
    pub select_custom: AtomicBool,
}

impl Default for TableSlot {
    fn default() -> Self {
        Self {
            incoming: ArrayQueue::new(4),
            retired: ArrayQueue::new(16),
            current: Mutex::new(None),
            error: Mutex::new(None),
            select_custom: AtomicBool::new(false),
        }
    }
}

impl TableSlot {
    /// Audio thread: a newly loaded table, if any.
    pub fn take_new(&self) -> Option<Arc<Wavetable>> {
        self.incoming.pop()
    }

    /// Audio thread: hand back a replaced table to be freed elsewhere.
    pub fn retire(&self, table: Arc<Wavetable>) {
        // shortcut: if 16 swaps pile up with no load in between, the overflow is freed on the audio thread
        let _ = self.retired.push(table);
    }

    fn drain_retired(&self) {
        while self.retired.pop().is_some() {}
    }

    /// Builds the table from its stored form, so what plays now is exactly what a reloaded project plays.
    fn build(stored: &StoredTable) -> Result<Arc<Wavetable>, String> {
        Ok(Arc::new(Wavetable::from_frames(&stored.decode()?)))
    }

    /// Loader thread: read, convert, store in `persisted` (project state) and send to the audio thread.
    pub fn load_file(&self, path: &Path, persisted: &Mutex<Option<StoredTable>>) {
        let result = (|| {
            let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
            if len as usize > MAX_FILE_BYTES {
                return Err("file is larger than 64 MB".to_string());
            }
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            let name = path.file_name().map_or("custom".into(), |n| n.to_string_lossy().into_owned());
            let stored = StoredTable::encode(&name, &parse_wav(&bytes)?);
            let table = Self::build(&stored)?;
            Ok((name, table, stored))
        })();
        match result {
            Ok((name, table, stored)) => {
                *persisted.lock().unwrap() = Some(stored.clone());
                self.publish(name, table, stored);
                self.select_custom.store(true, Ordering::Release);
                *self.error.lock().unwrap() = None;
            }
            Err(e) => *self.error.lock().unwrap() = Some(e),
        }
    }

    fn publish(&self, name: String, table: Arc<Wavetable>, stored: StoredTable) {
        self.drain_retired();
        *self.current.lock().unwrap() = Some((name, table.clone(), stored));
        // keep only the newest if the audio thread hasn't caught up
        if let Err(table) = self.incoming.push(table) {
            let _ = self.incoming.pop();
            let _ = self.incoming.push(table);
        }
    }

    /// Main thread, after state load (`initialize`): rebuild from the project if it differs from what's loaded.
    /// Returns the table the synth should use for this osc.
    pub fn restore(&self, persisted: &Mutex<Option<StoredTable>>) -> Option<Arc<Wavetable>> {
        self.drain_retired();
        while self.incoming.pop().is_some() {}
        let stored = persisted.lock().unwrap().clone();
        let mut current = self.current.lock().unwrap();
        match stored {
            None => {
                *current = None;
                None
            }
            Some(s) if current.as_ref().is_some_and(|(_, _, c)| *c == s) => current.as_ref().map(|(_, t, _)| t.clone()),
            Some(s) => match Self::build(&s) {
                Ok(table) => {
                    *current = Some((s.name.clone(), table.clone(), s));
                    Some(table)
                }
                Err(e) => {
                    *self.error.lock().unwrap() = Some(format!("saved wavetable is damaged: {e}"));
                    *current = None;
                    None
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(samples: &[f32], channels: u16, clm: Option<&str>) -> Vec<u8> {
        let spec = hound::WavSpec { channels, sample_rate: 44100, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut cur = Cursor::new(Vec::new());
        let mut w = hound::WavWriter::new(&mut cur, spec).unwrap();
        for s in samples {
            w.write_sample((s * 32767.0) as i16).unwrap();
        }
        w.finalize().unwrap();
        let mut bytes = cur.into_inner();
        if let Some(text) = clm {
            // append a clm chunk and fix the RIFF size
            let mut body = text.as_bytes().to_vec();
            if body.len() % 2 == 1 {
                body.push(0);
            }
            bytes.extend_from_slice(b"clm ");
            bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&body);
            let riff = (bytes.len() - 8) as u32;
            bytes[4..8].copy_from_slice(&riff.to_le_bytes());
        }
        bytes
    }

    fn saw(len: usize, cycles: usize) -> Vec<f32> {
        (0..len * cycles).map(|i| (i % len) as f32 / len as f32 * 2.0 - 1.0).collect()
    }

    #[test]
    fn plain_2048_frames() {
        let frames = parse_wav(&wav(&saw(2048, 8), 1, None)).unwrap();
        assert_eq!(frames.len(), 8);
        assert!(frames.iter().all(|f| f.len() == FRAME));
    }

    #[test]
    fn clm_chunk_sets_frame_size() {
        let frames = parse_wav(&wav(&saw(256, 16), 1, Some("<!>256 10000000 wavetable"))).unwrap();
        assert_eq!(frames.len(), 16);
        assert_eq!(frames[0].len(), FRAME);
        // resampled saw still ramps up across the frame
        assert!(frames[0][100] < frames[0][1500]);
    }

    #[test]
    fn odd_length_is_one_cycle() {
        let frames = parse_wav(&wav(&saw(600, 1), 1, None)).unwrap();
        assert_eq!(frames.len(), 1);
    }

    #[test]
    fn stereo_is_mixed_to_mono() {
        // left = +0.5, right = -0.5 -> silence
        let interleaved: Vec<f32> = (0..2048).flat_map(|_| [0.5, -0.5]).collect();
        let frames = parse_wav(&wav(&interleaved, 2, None)).unwrap();
        assert!(frames[0].iter().all(|s| s.abs() < 1e-3));
    }

    #[test]
    fn frame_count_is_capped() {
        let frames = parse_wav(&wav(&saw(64, 300), 1, Some("<!>64"))).unwrap();
        assert_eq!(frames.len(), MAX_FRAMES);
    }

    #[test]
    fn garbage_is_rejected_without_panicking() {
        for junk in [&b""[..], b"RIFF", b"RIFF\x10\0\0\0WAVEjunkjunk", &[0xffu8; 1000][..], b"not a wav file at all"] {
            assert!(parse_wav(junk).is_err());
        }
        // truncated real file: any result is fine, it just must not panic
        let w = wav(&saw(2048, 2), 1, None);
        let _ = parse_wav(&w[..w.len() / 3]);
        // clm chunk with a lying size must not panic
        let mut w = wav(&saw(2048, 1), 1, Some("<!>2048"));
        let n = w.len();
        w[n - 12..n - 8].copy_from_slice(&u32::MAX.to_le_bytes());
        let _ = parse_wav(&w);
    }

    #[test]
    fn slot_load_then_restore_in_new_instance() {
        // 4 frames: saw morphing toward square; also left in the temp dir as a handy GUI test file
        let frames: Vec<f32> = (0..4)
            .flat_map(|f| (0..2048).map(move |i| {
                let x = i as f32 / 2048.0;
                let (saw, sq) = (2.0 * x - 1.0, if x < 0.5 { 1.0 } else { -1.0 });
                saw + (sq - saw) * f as f32 / 3.0
            }))
            .collect();
        let path = std::env::temp_dir().join("morphwave_test_table.wav");
        std::fs::write(&path, wav(&frames, 1, Some("<!>2048"))).unwrap();

        let (slot, persisted) = (TableSlot::default(), Mutex::new(None));
        slot.load_file(&path, &persisted);
        assert!(slot.error.lock().unwrap().is_none());
        assert!(slot.select_custom.load(Ordering::Acquire));
        let sent = slot.take_new().expect("table queued for the audio thread");
        assert_eq!(sent.frames(), 4);
        assert_eq!(persisted.lock().unwrap().as_ref().unwrap().name, "morphwave_test_table.wav");

        // "reopen the project": a fresh slot restores from the persisted state alone
        let saved = Mutex::new(persisted.lock().unwrap().clone());
        let fresh = TableSlot::default();
        assert_eq!(fresh.restore(&saved).unwrap().frames(), 4);
        // same state again: reuses the built table instead of rebuilding
        let again = fresh.restore(&saved).unwrap();
        assert!(Arc::ptr_eq(&again, &fresh.current.lock().unwrap().as_ref().unwrap().1));

        // damaged state: no table, an error for the GUI, no panic
        saved.lock().unwrap().as_mut().unwrap().data.truncate(10);
        let broken = TableSlot::default();
        assert!(broken.restore(&saved).is_none());
        assert!(broken.error.lock().unwrap().is_some());

        // a missing file reports an error instead of panicking
        slot.load_file(Path::new("definitely/not/here.wav"), &persisted);
        assert!(slot.error.lock().unwrap().is_some());
    }

    #[test]
    fn stored_table_round_trips_and_rejects_bad_data() {
        let frames = parse_wav(&wav(&saw(2048, 3), 1, None)).unwrap();
        let stored = StoredTable::encode("saw.wav", &frames);
        let back = stored.decode().unwrap();
        assert_eq!(back.len(), 3);
        let peak = frames[1].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        for (a, b) in frames[1].iter().zip(&back[1]) {
            assert!((a / peak - b).abs() < 1e-3);
        }
        assert!(StoredTable { data: "!!!".into(), ..stored.clone() }.decode().is_err());
        assert!(StoredTable { frames: 2, ..stored.clone() }.decode().is_err());
        assert!(StoredTable { frames: 100000, ..stored }.decode().is_err());
    }
}
