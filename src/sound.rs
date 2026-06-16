// sound.rs — XAudio2 音声バックエンド（Windows のみ、WAV PCM 8/16-bit 対応）

// ── Windows 実装 ──────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod backend {
    use std::collections::HashMap;
    use std::sync::Arc;
    use windows::{
        core::PCWSTR,
        Win32::{
            Media::Audio::{
                AUDIO_STREAM_CATEGORY, WAVEFORMATEX,
                XAudio2::{
                    IXAudio2, IXAudio2MasteringVoice, IXAudio2SourceVoice,
                    IXAudio2VoiceCallback, XAudio2CreateWithVersionInfo,
                    XAUDIO2_BUFFER, XAUDIO2_COMMIT_NOW,
                    XAUDIO2_DEFAULT_CHANNELS, XAUDIO2_DEFAULT_FREQ_RATIO,
                    XAUDIO2_DEFAULT_PROCESSOR, XAUDIO2_DEFAULT_SAMPLERATE,
                    XAUDIO2_END_OF_STREAM, XAUDIO2_LOOP_INFINITE,
                },
            },
            System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED},
        },
    };

    // XAudio2Create の ntddiversion 引数: NTDDI_WIN10
    const NTDDI_WIN10: u32 = 0x0A00_0000;

    struct SoundEntry {
        samples:     Arc<Vec<i16>>,
        channels:    u16,
        sample_rate: u32,
    }

    struct ActiveVoice {
        voice: IXAudio2SourceVoice,
        _data: Arc<Vec<i16>>, // XAudio2 がバッファを参照している間 PCM データを保持
    }

    impl Drop for ActiveVoice {
        fn drop(&mut self) {
            unsafe {
                let _ = self.voice.Stop(0, XAUDIO2_COMMIT_NOW);
                self.voice.DestroyVoice();
            }
        }
    }

    // フィールドは宣言順に drop される: voices → sounds → _master → engine
    pub struct AudioState {
        voices:  HashMap<u32, ActiveVoice>,
        sounds:  HashMap<u32, SoundEntry>,
        next_id: u32,
        _master: IXAudio2MasteringVoice,
        engine:  IXAudio2,
    }

    impl AudioState {
        pub fn new() -> Option<Self> {
            unsafe {
                // WIC と同じアパートメントモデルで COM を初期化（失敗しても続行）
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

                let mut engine: Option<IXAudio2> = None;
                XAudio2CreateWithVersionInfo(
                    &mut engine,
                    0,
                    XAUDIO2_DEFAULT_PROCESSOR,
                    NTDDI_WIN10,
                )
                .ok()?;
                let engine = engine?;

                let mut master: Option<IXAudio2MasteringVoice> = None;
                engine
                    .CreateMasteringVoice(
                        &mut master,
                        XAUDIO2_DEFAULT_CHANNELS,
                        XAUDIO2_DEFAULT_SAMPLERATE,
                        0,
                        PCWSTR(std::ptr::null()), // デフォルトデバイス
                        None,                      // エフェクトチェーン無し
                        AUDIO_STREAM_CATEGORY(6),  // AudioCategory_GameEffects
                    )
                    .ok()?;

                Some(Self {
                    voices:  HashMap::new(),
                    sounds:  HashMap::new(),
                    next_id: 1,
                    _master: master?,
                    engine,
                })
            }
        }

        pub fn load(&mut self, path: &str) -> u32 {
            let bytes = match std::fs::read(path) {
                Ok(b)  => b,
                Err(_) => { crate::log_warn!("サウンドファイルを読み込めません: '{path}'"); return 0; }
            };
            let lower = path.to_ascii_lowercase();
            let result = if lower.ends_with(".ogg") {
                decode_ogg(&bytes)
            } else {
                decode_wav(&bytes)
            };
            let (samples, channels, sample_rate) = match result {
                Some(v) => v,
                None => {
                    crate::log_warn!(
                        "サウンドのデコードに失敗しました: '{path}' (WAV PCM 8/16-bit または OGG Vorbis のみ対応)"
                    );
                    return 0;
                }
            };
            let id = self.next_id;
            self.next_id += 1;
            self.sounds.insert(id, SoundEntry { samples: Arc::new(samples), channels, sample_rate });
            id
        }

        pub fn play(&mut self, handle: u32, looping: bool) {
            let Some(entry) = self.sounds.get(&handle) else { return };
            self.voices.remove(&handle); // 再生中なら停止・破棄

            let wfx = WAVEFORMATEX {
                wFormatTag:      1, // WAVE_FORMAT_PCM
                nChannels:       entry.channels,
                nSamplesPerSec:  entry.sample_rate,
                nAvgBytesPerSec: entry.sample_rate * entry.channels as u32 * 2,
                nBlockAlign:     entry.channels * 2,
                wBitsPerSample:  16,
                cbSize:          0,
            };

            let samples = Arc::clone(&entry.samples);

            let voice = unsafe {
                let mut v: Option<IXAudio2SourceVoice> = None;
                if self
                    .engine
                    .CreateSourceVoice(
                        &mut v,
                        &wfx,
                        0,
                        XAUDIO2_DEFAULT_FREQ_RATIO,
                        None::<&IXAudio2VoiceCallback>, // コールバック無し
                        None,                           // 送信先リスト（マスタリングボイスへ）
                        None,                           // エフェクトチェーン無し
                    )
                    .is_err()
                {
                    return;
                }
                match v { Some(v) => v, None => return }
            };

            let buf = XAUDIO2_BUFFER {
                Flags:      XAUDIO2_END_OF_STREAM,
                AudioBytes: (samples.len() * 2) as u32,
                pAudioData: samples.as_ptr() as *const u8,
                PlayBegin:  0,
                PlayLength: 0,
                LoopBegin:  0,
                LoopLength: 0,
                LoopCount:  if looping { XAUDIO2_LOOP_INFINITE } else { 0 },
                pContext:   std::ptr::null_mut(),
            };

            unsafe {
                if voice.SubmitSourceBuffer(&buf, None).is_err() { return; }
                if voice.Start(0, XAUDIO2_COMMIT_NOW).is_err() { return; }
            }

            self.voices.insert(handle, ActiveVoice { voice, _data: samples });
        }

        pub fn stop(&mut self, handle: u32) {
            self.voices.remove(&handle);
        }

        pub fn set_volume(&mut self, handle: u32, volume: f32) {
            if let Some(av) = self.voices.get(&handle) {
                unsafe { let _ = av.voice.SetVolume(volume, XAUDIO2_COMMIT_NOW); }
            }
        }

        pub fn free(&mut self, handle: u32) {
            self.voices.remove(&handle); // Drop が Stop + DestroyVoice を呼ぶ
            self.sounds.remove(&handle);
        }
    }

    /// RIFF WAV（PCM 8/16-bit）を i16 PCM サンプル列にデコードする。
    fn decode_wav(bytes: &[u8]) -> Option<(Vec<i16>, u16, u32)> {
        if bytes.len() < 12 { return None; }
        if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" { return None; }

        let mut pos            = 12usize;
        let mut channels:        Option<u16> = None;
        let mut sample_rate:     Option<u32> = None;
        let mut bits_per_sample: Option<u16> = None;
        let mut audio_format:    Option<u16> = None;
        let mut samples:         Option<Vec<i16>> = None;

        while pos + 8 <= bytes.len() {
            let tag  = &bytes[pos..pos + 4];
            let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().ok()?) as usize;
            pos += 8;
            if pos + size > bytes.len() { break; }

            match tag {
                b"fmt " if size >= 16 => {
                    audio_format    = Some(u16::from_le_bytes(bytes[pos..pos + 2].try_into().ok()?));
                    channels        = Some(u16::from_le_bytes(bytes[pos + 2..pos + 4].try_into().ok()?));
                    sample_rate     = Some(u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().ok()?));
                    bits_per_sample = Some(u16::from_le_bytes(bytes[pos + 14..pos + 16].try_into().ok()?));
                }
                b"data" => {
                    let raw = &bytes[pos..pos + size];
                    samples = match (audio_format, bits_per_sample) {
                        (Some(1), Some(16)) => Some(
                            raw.chunks_exact(2)
                               .map(|c| i16::from_le_bytes([c[0], c[1]]))
                               .collect(),
                        ),
                        (Some(1), Some(8)) => Some(
                            // 8-bit WAV は符号なし → i16 に変換
                            raw.iter().map(|&b| ((b as i16) - 128) << 8).collect(),
                        ),
                        _ => return None,
                    };
                }
                _ => {}
            }
            pos += size + (size & 1); // ワードアライン
        }

        Some((samples?, channels?, sample_rate?))
    }

    /// OGG Vorbis を i16 インターリーブ PCM サンプル列にデコードする。
    fn decode_ogg(bytes: &[u8]) -> Option<(Vec<i16>, u16, u32)> {
        use lewton::inside_ogg::OggStreamReader;
        let cursor = std::io::Cursor::new(bytes);
        let mut reader = OggStreamReader::new(cursor).ok()?;
        let channels    = reader.ident_hdr.audio_channels as u16;
        let sample_rate = reader.ident_hdr.audio_sample_rate;
        let mut samples = Vec::<i16>::new();
        loop {
            match reader.read_dec_packet_itl() {
                Ok(Some(pck)) => samples.extend_from_slice(&pck),
                Ok(None)      => break,
                Err(_)        => return None,
            }
        }
        Some((samples, channels, sample_rate))
    }
}

// ── non-Windows スタブ ────────────────────────────────────────────────────────

#[cfg(not(target_os = "windows"))]
mod backend {
    pub struct AudioState;
    impl AudioState {
        pub fn new() -> Option<Self> { None }
        pub fn load(&mut self, _: &str) -> u32 { 0 }
        pub fn play(&mut self, _: u32, _: bool) {}
        pub fn stop(&mut self, _: u32) {}
        pub fn set_volume(&mut self, _: u32, _: f32) {}
        pub fn free(&mut self, _: u32) {}
    }
}

// ── スレッドローカル状態 ──────────────────────────────────────────────────────

thread_local! {
    static AUDIO: std::cell::RefCell<Option<backend::AudioState>> =
        std::cell::RefCell::new(backend::AudioState::new());
}

// ── 公開 API ──────────────────────────────────────────────────────────────────

/// 音声ファイル（WAV PCM 8/16-bit または OGG Vorbis）をロードしてハンドルを返す。失敗時は 0。
pub fn load_sound(path: &str) -> u32 {
    AUDIO.with(|a| {
        let mut a = a.borrow_mut();
        let Some(state) = a.as_mut() else { return 0 };
        state.load(path)
    })
}

/// 音声を再生する。looping = true でループ、false で1回のみ。
/// 同じハンドルが既に再生中の場合は止めて最初から再生し直す。
pub fn play_sound(handle: u32, looping: bool) {
    AUDIO.with(|a| {
        if let Some(state) = a.borrow_mut().as_mut() {
            state.play(handle, looping);
        }
    });
}

/// 指定ハンドルの再生を停止する。
pub fn stop_sound(handle: u32) {
    AUDIO.with(|a| {
        if let Some(state) = a.borrow_mut().as_mut() {
            state.stop(handle);
        }
    });
}

/// 指定ハンドルの音量を設定する（0.0 = 無音 / 1.0 = 原音量）。
pub fn set_volume(handle: u32, volume: f32) {
    AUDIO.with(|a| {
        if let Some(state) = a.borrow_mut().as_mut() {
            state.set_volume(handle, volume);
        }
    });
}

/// 指定ハンドルの音声データを解放する。再生中の場合は停止してから解放する。
pub fn free_sound(handle: u32) {
    AUDIO.with(|a| {
        if let Some(state) = a.borrow_mut().as_mut() {
            state.free(handle);
        }
    });
}

/// 全音声を停止してリソースを解放する。
pub fn free_all_sounds() {
    AUDIO.with(|a| { *a.borrow_mut() = None; });
}
