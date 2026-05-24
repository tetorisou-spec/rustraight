use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};

// ロード済み音声を Arc<Vec<i16>> で保持し、再生のたびにゼロコピーで共有する。
#[derive(Clone)]
struct PcmSource {
    samples:     Arc<Vec<i16>>,
    pos:         usize,
    channels:    u16,
    sample_rate: u32,
}

impl Iterator for PcmSource {
    type Item = i16;
    fn next(&mut self) -> Option<i16> {
        let s = *self.samples.get(self.pos)?;
        self.pos += 1;
        Some(s)
    }
}

impl Source for PcmSource {
    fn current_frame_len(&self) -> Option<usize> { None }
    fn channels(&self) -> u16                    { self.channels }
    fn sample_rate(&self) -> u32                 { self.sample_rate }
    fn total_duration(&self) -> Option<Duration> { None }
}

// ── ストア ────────────────────────────────────────────────────────────────────

struct SoundEntry {
    samples:     Arc<Vec<i16>>,
    channels:    u16,
    sample_rate: u32,
}

struct AudioState {
    _stream:       OutputStream,       // Drop すると出力が止まるので保持
    stream_handle: OutputStreamHandle,
    next_id:       u32,
    sounds:        HashMap<u32, SoundEntry>,
    sinks:         HashMap<u32, Sink>, // ハンドルごとに再生中の Sink を1本管理
}

impl AudioState {
    fn new() -> Option<Self> {
        let (stream, stream_handle) = OutputStream::try_default().ok()?;
        Some(Self {
            _stream: stream,
            stream_handle,
            next_id: 1,
            sounds:  HashMap::new(),
            sinks:   HashMap::new(),
        })
    }
}

thread_local! {
    static AUDIO: std::cell::RefCell<Option<AudioState>> =
        std::cell::RefCell::new(AudioState::new());
}

// ── 公開 API ──────────────────────────────────────────────────────────────────

/// 音声ファイルをロードしてハンドルを返す。失敗時は 0 を返す。
/// WAV / OGG / MP3 / FLAC に対応。
pub fn load_sound(path: &str) -> u32 {
    AUDIO.with(|a| {
        let mut a = a.borrow_mut();
        let Some(state) = a.as_mut() else { return 0 };

        let bytes = match std::fs::read(path) {
            Ok(b)  => b,
            Err(_) => { eprintln!("[rustraight] load_sound: cannot read '{path}'"); return 0; }
        };

        let decoder = match Decoder::new(Cursor::new(bytes)) {
            Ok(d)  => d,
            Err(e) => { eprintln!("[rustraight] load_sound: cannot decode '{path}': {e}"); return 0; }
        };

        let channels    = decoder.channels();
        let sample_rate = decoder.sample_rate();
        let samples     = Arc::new(decoder.collect::<Vec<i16>>());

        let id = state.next_id;
        state.next_id += 1;
        state.sounds.insert(id, SoundEntry { samples, channels, sample_rate });
        id
    })
}

/// 音声を再生する。looping = true でループ、false で1回のみ。
/// 同じハンドルが既に再生中の場合は止めて最初から再生し直す。
pub fn play_sound(handle: u32, looping: bool) {
    AUDIO.with(|a| {
        let mut a = a.borrow_mut();
        let Some(state) = a.as_mut() else { return };
        let Some(entry) = state.sounds.get(&handle) else { return };

        let source = PcmSource {
            samples:     Arc::clone(&entry.samples),
            pos:         0,
            channels:    entry.channels,
            sample_rate: entry.sample_rate,
        };

        state.sinks.remove(&handle); // 既存の再生を停止

        let Ok(sink) = Sink::try_new(&state.stream_handle) else { return };
        if looping {
            sink.append(source.repeat_infinite());
        } else {
            sink.append(source);
        }
        state.sinks.insert(handle, sink);
    });
}

/// 指定ハンドルの再生を停止する。
pub fn stop_sound(handle: u32) {
    AUDIO.with(|a| {
        if let Some(state) = a.borrow_mut().as_mut() {
            state.sinks.remove(&handle);
        }
    });
}

/// 指定ハンドルの音量を設定する（0.0 = 無音 / 1.0 = 原音量）。
pub fn set_volume(handle: u32, volume: f32) {
    AUDIO.with(|a| {
        if let Some(state) = a.borrow_mut().as_mut() {
            if let Some(sink) = state.sinks.get(&handle) {
                sink.set_volume(volume);
            }
        }
    });
}

/// 全ての音声を停止してリソースを解放する。
/// OutputStream ごと drop することでシャットダウン時のパニックを防ぐ。
pub fn free_all_sounds() {
    AUDIO.with(|a| {
        *a.borrow_mut() = None;
    });
}
