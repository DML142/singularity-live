#[cfg(any(target_os = "windows", test))]
use std::collections::VecDeque;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use futures_util::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use serde::{Deserialize, Serialize};
use tokio::{
    net::TcpStream,
    sync::{Mutex, mpsc},
};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;

use crate::secrets::{SecretName, SecretStore};

const SONIOX_ENDPOINT: &str = "wss://stt-rt.soniox.com/transcribe-websocket";
const SONIOX_MODEL: &str = "stt-rt-v5";
#[cfg(target_os = "windows")]
const AUDIO_FRAME_BYTES: usize = 640;
const SONIOX_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

type SonioxSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>;
type SonioxWriter = SplitSink<SonioxSocket, Message>;
type SonioxReader = SplitStream<SonioxSocket>;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioInputSource {
    Microphone,
    SystemAudio,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioInputDevice {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum VoiceInputEvent {
    Started { source: AudioInputSource },
    Transcript { text: String },
    Stopped { transcript: String },
    Failed { message: String },
}

pub trait VoiceInputEventSink: Send + Sync {
    fn emit(&self, event: VoiceInputEvent);
}

pub struct VoiceInputService {
    secrets: Arc<dyn SecretStore>,
    events: Arc<dyn VoiceInputEventSink>,
    state: Mutex<VoiceInputState>,
}

#[derive(Default)]
struct VoiceInputState {
    source: Option<AudioInputSource>,
    microphone_device_id: Option<String>,
    session: Option<VoiceInputSession>,
}

struct VoiceInputSession {
    active: Arc<AtomicBool>,
    stop: CancellationToken,
    task: tauri::async_runtime::JoinHandle<()>,
}

impl VoiceInputService {
    #[must_use]
    pub fn new(secrets: Arc<dyn SecretStore>, events: Arc<dyn VoiceInputEventSink>) -> Self {
        Self {
            secrets,
            events,
            state: Mutex::new(VoiceInputState::default()),
        }
    }

    /// Sets the source used by the voice shortcut and next capture session.
    ///
    /// # Errors
    ///
    /// Returns [`VoiceInputError::Busy`] when voice capture is active.
    pub async fn set_source(
        &self,
        source: AudioInputSource,
        microphone_device_id: Option<String>,
    ) -> Result<(), VoiceInputError> {
        let mut state = self.state.lock().await;
        if state
            .session
            .as_ref()
            .is_some_and(|session| session.active.load(Ordering::Acquire))
        {
            return Err(VoiceInputError::Busy);
        }
        state.source = Some(source);
        state.microphone_device_id = microphone_device_id;
        Ok(())
    }

    /// Connects to Soniox and starts capturing audio from the selected source.
    ///
    /// # Errors
    ///
    /// Returns an error when a session is active, no API key is configured, or the
    /// transcription service cannot be reached.
    pub async fn start(
        &self,
        source: AudioInputSource,
        microphone_device_id: Option<String>,
    ) -> Result<(), VoiceInputError> {
        let mut state = self.state.lock().await;
        if state
            .session
            .as_ref()
            .is_some_and(|session| session.active.load(Ordering::Acquire))
        {
            return Err(VoiceInputError::Busy);
        }
        state.session = None;
        let secret = self
            .secrets
            .get(SecretName::SonioxApiKey)
            .map_err(|_| VoiceInputError::NotConfigured)?;
        let (mut socket, _) =
            tokio::time::timeout(SONIOX_CONNECT_TIMEOUT, connect_async(SONIOX_ENDPOINT))
                .await
                .map_err(|_| VoiceInputError::Unavailable)?
                .map_err(|_| VoiceInputError::Unavailable)?;
        let configuration = SonioxConfiguration::new(secret.expose());
        let configuration =
            serde_json::to_string(&configuration).map_err(|_| VoiceInputError::Unavailable)?;
        socket
            .send(Message::Text(configuration.into()))
            .await
            .map_err(|_| VoiceInputError::Unavailable)?;

        let (audio_sender, audio_receiver) = mpsc::channel(64);
        let stop = CancellationToken::new();
        let active = Arc::new(AtomicBool::new(true));
        let source_for_task = source;
        let device_for_task = microphone_device_id.clone();
        let capture_stop = stop.clone();
        let capture_task = tokio::task::spawn_blocking(move || {
            capture_audio(
                source_for_task,
                device_for_task.as_deref(),
                &audio_sender,
                &capture_stop,
            )
        });
        let events = Arc::clone(&self.events);
        let task_active = Arc::clone(&active);
        let task_stop = stop.clone();
        let task = tauri::async_runtime::spawn(async move {
            run_soniox_session(
                socket,
                audio_receiver,
                capture_task,
                task_stop,
                task_active,
                source,
                events,
            )
            .await;
        });
        state.source = Some(source);
        state.microphone_device_id = microphone_device_id;
        state.session = Some(VoiceInputSession { active, stop, task });
        Ok(())
    }

    pub async fn stop(&self) {
        let session = self.state.lock().await.session.take();
        if let Some(session) = session {
            session.stop.cancel();
            let _ = session.task.await;
        }
    }

    pub async fn toggle(&self) {
        let (source, microphone_device_id) = {
            let state = self.state.lock().await;
            if state
                .session
                .as_ref()
                .is_some_and(|session| session.active.load(Ordering::Acquire))
            {
                drop(state);
                self.stop().await;
                return;
            }
            (
                state.source.unwrap_or(AudioInputSource::Microphone),
                state.microphone_device_id.clone(),
            )
        };
        if let Err(error) = self.start(source, microphone_device_id).await {
            self.events.emit(VoiceInputEvent::Failed {
                message: error.safe_message().to_owned(),
            });
        }
    }
}

/// Lists active microphone endpoints supported by the current platform.
///
/// # Errors
///
/// Returns [`VoiceInputError::Unavailable`] when Windows audio endpoint enumeration fails.
#[cfg(target_os = "windows")]
pub fn list_audio_input_devices() -> Result<Vec<AudioInputDevice>, VoiceInputError> {
    list_windows_audio_input_devices()
}

/// Lists active microphone endpoints supported by the current platform.
///
/// # Errors
///
/// This implementation currently cannot fail on unsupported platforms.
#[cfg(not(target_os = "windows"))]
pub fn list_audio_input_devices() -> Result<Vec<AudioInputDevice>, VoiceInputError> {
    Ok(Vec::new())
}

#[cfg(target_os = "windows")]
fn list_windows_audio_input_devices() -> Result<Vec<AudioInputDevice>, VoiceInputError> {
    use wasapi::{DeviceEnumerator, Direction, initialize_mta};

    initialize_mta()
        .ok()
        .map_err(|_| VoiceInputError::Unavailable)?;
    let result = (|| {
        let enumerator = DeviceEnumerator::new().map_err(|_| VoiceInputError::Unavailable)?;
        let collection = enumerator
            .get_device_collection(&Direction::Capture)
            .map_err(|_| VoiceInputError::Unavailable)?;
        let count = collection
            .get_nbr_devices()
            .map_err(|_| VoiceInputError::Unavailable)?;
        (0..count)
            .map(|index| {
                let device = collection
                    .get_device_at_index(index)
                    .map_err(|_| VoiceInputError::Unavailable)?;
                let id = device.get_id().map_err(|_| VoiceInputError::Unavailable)?;
                let label = device
                    .get_friendlyname()
                    .map_err(|_| VoiceInputError::Unavailable)?;
                Ok(AudioInputDevice { id, label })
            })
            .collect()
    })();
    wasapi::deinitialize();
    result
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VoiceInputError {
    Busy,
    NotConfigured,
    Unavailable,
}

impl VoiceInputError {
    #[must_use]
    pub const fn safe_message(self) -> &'static str {
        match self {
            Self::Busy => "Voice input is already active",
            Self::NotConfigured => "Configure SONIOX_API_KEY in pnpm assist to use voice input",
            Self::Unavailable => "Soniox or the selected audio source could not be reached",
        }
    }
}

#[derive(Serialize)]
struct SonioxConfiguration<'a> {
    api_key: &'a str,
    model: &'static str,
    audio_format: &'static str,
    sample_rate: u32,
    num_channels: u8,
    language_hints: [&'static str; 3],
    context: SonioxContext,
    enable_endpoint_detection: bool,
    max_endpoint_delay_ms: u16,
}

impl<'a> SonioxConfiguration<'a> {
    fn new(api_key: &'a str) -> Self {
        Self {
            api_key,
            model: SONIOX_MODEL,
            audio_format: "pcm_s16le",
            sample_rate: 16_000,
            num_channels: 1,
            language_hints: ["ru", "uk", "en"],
            context: SonioxContext {
                terms: [
                    "Rust",
                    "TypeScript",
                    "Tauri",
                    "React",
                    "Vite",
                    "Tokio",
                    "WebSocket",
                    "JavaScript",
                    "API",
                    "OpenAI",
                    "Soniox",
                ],
            },
            enable_endpoint_detection: true,
            max_endpoint_delay_ms: 800,
        }
    }
}

#[derive(Serialize)]
struct SonioxContext {
    terms: [&'static str; 11],
}

#[derive(Deserialize)]
struct SonioxResult {
    #[serde(default)]
    tokens: Vec<SonioxToken>,
    #[serde(default)]
    finished: bool,
    #[serde(default)]
    error_code: Option<u32>,
}

#[derive(Deserialize)]
struct SonioxToken {
    text: String,
    #[serde(default)]
    is_final: bool,
}

async fn run_soniox_session(
    socket: SonioxSocket,
    mut audio_receiver: mpsc::Receiver<AudioMessage>,
    capture_task: tokio::task::JoinHandle<Result<(), CaptureError>>,
    stop: CancellationToken,
    active: Arc<AtomicBool>,
    source: AudioInputSource,
    events: Arc<dyn VoiceInputEventSink>,
) {
    let (mut writer, mut reader) = socket.split();
    let mut final_text = String::new();
    let mut partial_text = String::new();
    events.emit(VoiceInputEvent::Started { source });
    let mut failed = match stream_soniox_until_stopped(
        &mut writer,
        &mut reader,
        &mut audio_receiver,
        &stop,
        &mut final_text,
        &mut partial_text,
        &events,
    )
    .await
    {
        Ok(true) => Some("Soniox connection was closed".to_owned()),
        Ok(false) => None,
        Err(message) => Some(message),
    };
    stop.cancel();
    stop_capture_and_drain_audio(&mut writer, &mut audio_receiver, capture_task, &mut failed).await;
    if failed.is_none() {
        failed = finalize_soniox(
            &mut writer,
            &mut reader,
            &mut final_text,
            &mut partial_text,
            &events,
        )
        .await
        .err();
    }
    active.store(false, Ordering::Release);
    if let Some(message) = failed {
        events.emit(VoiceInputEvent::Failed { message });
    } else {
        events.emit(VoiceInputEvent::Stopped {
            transcript: complete_text(&final_text, &partial_text),
        });
    }
}

async fn stream_soniox_until_stopped(
    writer: &mut SonioxWriter,
    reader: &mut SonioxReader,
    audio_receiver: &mut mpsc::Receiver<AudioMessage>,
    stop: &CancellationToken,
    final_text: &mut String,
    partial_text: &mut String,
    events: &Arc<dyn VoiceInputEventSink>,
) -> Result<bool, String> {
    let mut keepalive = tokio::time::interval(Duration::from_secs(5));
    loop {
        tokio::select! {
            () = stop.cancelled() => return Ok(false),
            audio = audio_receiver.recv() => match audio {
                #[cfg(target_os = "windows")]
                Some(AudioMessage::Frame(frame)) => {
                    writer.send(Message::Binary(frame.into())).await
                        .map_err(|_| "Soniox connection was interrupted".to_owned())?;
                }
                Some(AudioMessage::Failed) | None => {
                    return Err(CaptureError::safe_message().to_owned());
                }
            },
            response = reader.next() => match response {
                Some(Ok(Message::Text(text))) => {
                    let finished = update_transcript(text.as_str(), final_text, partial_text)
                        .map_err(|()| "Soniox returned an invalid transcription response".to_owned())?;
                    events.emit(VoiceInputEvent::Transcript {
                        text: complete_text(final_text, partial_text),
                    });
                    if finished {
                        return Ok(true);
                    }
                }
                Some(Ok(Message::Close(_))) | None => {
                    return Err("Soniox connection was closed".to_owned());
                }
                Some(Err(_)) => {
                    return Err("Soniox connection was interrupted".to_owned());
                }
                Some(Ok(_)) => {}
            },
            _ = keepalive.tick() => {
                let message = serde_json::json!({"type": "keepalive"}).to_string();
                writer.send(Message::Text(message.into())).await
                    .map_err(|_| "Soniox connection was interrupted".to_owned())?;
            }
        }
    }
}

async fn stop_capture_and_drain_audio(
    writer: &mut SonioxWriter,
    audio_receiver: &mut mpsc::Receiver<AudioMessage>,
    mut capture_task: tokio::task::JoinHandle<Result<(), CaptureError>>,
    failed: &mut Option<String>,
) {
    let mut receiver_closed = false;
    let capture_result = loop {
        if receiver_closed {
            break capture_task.await;
        }
        tokio::select! {
            result = &mut capture_task => break result,
            audio = audio_receiver.recv() => match audio {
                Some(audio) => {
                    #[cfg(target_os = "windows")]
                    if let Some(frame) = forward_audio_message(audio, failed)
                        && writer.send(Message::Binary(frame.into())).await.is_err()
                    {
                        *failed = Some("Soniox connection was interrupted".to_owned());
                    }
                    #[cfg(not(target_os = "windows"))]
                    forward_audio_message(audio, failed);
                }
                None => receiver_closed = true,
            }
        }
    };
    if !matches!(capture_result, Ok(Ok(()))) {
        failed.get_or_insert_with(|| CaptureError::safe_message().to_owned());
    }
    while let Some(audio) = audio_receiver.recv().await {
        #[cfg(target_os = "windows")]
        if let Some(frame) = forward_audio_message(audio, failed)
            && writer.send(Message::Binary(frame.into())).await.is_err()
        {
            *failed = Some("Soniox connection was interrupted".to_owned());
        }
        #[cfg(not(target_os = "windows"))]
        forward_audio_message(audio, failed);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = writer;
}

fn forward_audio_message(audio: AudioMessage, failed: &mut Option<String>) -> Option<Vec<u8>> {
    match audio {
        #[cfg(target_os = "windows")]
        AudioMessage::Frame(frame) if failed.is_none() => Some(frame),
        AudioMessage::Failed => {
            failed.get_or_insert_with(|| CaptureError::safe_message().to_owned());
            None
        }
        #[cfg(target_os = "windows")]
        AudioMessage::Frame(_) => None,
    }
}

async fn finalize_soniox(
    writer: &mut SonioxWriter,
    reader: &mut SonioxReader,
    final_text: &mut String,
    partial_text: &mut String,
    events: &Arc<dyn VoiceInputEventSink>,
) -> Result<(), String> {
    let finalize = serde_json::json!({"type": "finalize"}).to_string();
    writer
        .send(Message::Text(finalize.into()))
        .await
        .map_err(|_| "Soniox connection was interrupted".to_owned())?;
    let receive_final = async {
        while let Some(response) = reader.next().await {
            let Ok(Message::Text(text)) = response else {
                continue;
            };
            if let Ok(done) = update_transcript(text.as_str(), final_text, partial_text) {
                events.emit(VoiceInputEvent::Transcript {
                    text: complete_text(final_text, partial_text),
                });
                if done {
                    break;
                }
            }
        }
    };
    let _ = tokio::time::timeout(Duration::from_millis(1_500), receive_final).await;
    Ok(())
}

fn update_transcript(
    raw: &str,
    final_text: &mut String,
    partial_text: &mut String,
) -> Result<bool, ()> {
    let result = serde_json::from_str::<SonioxResult>(raw).map_err(|_| ())?;
    if result.error_code.is_some() {
        return Err(());
    }
    let mut latest_partial = String::new();
    let mut finalized_any = false;
    for token in result.tokens {
        if token.text.starts_with('<') && token.text.ends_with('>') {
            continue;
        }
        if token.is_final {
            final_text.push_str(&token.text);
            finalized_any = true;
        } else {
            latest_partial.push_str(&token.text);
        }
    }
    if !latest_partial.is_empty() {
        *partial_text = latest_partial;
    } else if finalized_any {
        partial_text.clear();
    }
    Ok(result.finished)
}

fn complete_text(final_text: &str, partial_text: &str) -> String {
    format!("{final_text}{partial_text}").trim().to_owned()
}

enum AudioMessage {
    #[cfg(target_os = "windows")]
    Frame(Vec<u8>),
    Failed,
}

#[derive(Clone, Copy)]
enum CaptureError {
    Unavailable,
}

impl CaptureError {
    const fn safe_message() -> &'static str {
        "The selected microphone or system audio source is unavailable"
    }
}

#[cfg(target_os = "windows")]
fn capture_audio(
    source: AudioInputSource,
    microphone_device_id: Option<&str>,
    sender: &mpsc::Sender<AudioMessage>,
    stop: &CancellationToken,
) -> Result<(), CaptureError> {
    use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat, initialize_mta};

    initialize_mta()
        .ok()
        .map_err(|_| CaptureError::Unavailable)?;
    let result = (|| {
        let enumerator = DeviceEnumerator::new().map_err(|_| CaptureError::Unavailable)?;
        let device_direction = match source {
            AudioInputSource::Microphone => Direction::Capture,
            AudioInputSource::SystemAudio => Direction::Render,
        };
        let device = match (source, microphone_device_id) {
            (AudioInputSource::Microphone, Some(device_id)) => {
                let devices = enumerator
                    .get_device_collection(&Direction::Capture)
                    .map_err(|_| CaptureError::Unavailable)?;
                let count = devices
                    .get_nbr_devices()
                    .map_err(|_| CaptureError::Unavailable)?;
                let mut selected = None;
                for index in 0..count {
                    let candidate = devices
                        .get_device_at_index(index)
                        .map_err(|_| CaptureError::Unavailable)?;
                    if candidate.get_id().map_err(|_| CaptureError::Unavailable)? == device_id {
                        selected = Some(candidate);
                        break;
                    }
                }
                selected.ok_or(CaptureError::Unavailable)?
            }
            _ => enumerator
                .get_default_device(&device_direction)
                .map_err(|_| CaptureError::Unavailable)?,
        };
        let mut client = device
            .get_iaudioclient()
            .map_err(|_| CaptureError::Unavailable)?;
        let format = WaveFormat::new(16, 16, &SampleType::Int, 16_000, 1, None);
        let mode = StreamMode::PollingShared {
            autoconvert: true,
            buffer_duration_hns: 500_000,
        };
        client
            .initialize_client(&format, &Direction::Capture, &mode)
            .map_err(|_| CaptureError::Unavailable)?;
        let capture = client
            .get_audiocaptureclient()
            .map_err(|_| CaptureError::Unavailable)?;
        client
            .start_stream()
            .map_err(|_| CaptureError::Unavailable)?;

        let mut gate = VoiceActivityGate::default();
        let mut pending_bytes = VecDeque::new();
        while !stop.is_cancelled() {
            let packet = capture
                .get_next_packet_size()
                .map_err(|_| CaptureError::Unavailable)?;
            let Some(frames) = packet.filter(|frames| *frames > 0) else {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            };
            let capacity = usize::try_from(frames)
                .ok()
                .and_then(|count| count.checked_mul(2))
                .ok_or(CaptureError::Unavailable)?;
            let mut packet_bytes = vec![0; capacity];
            let (frames_read, _) = capture
                .read_from_device(&mut packet_bytes)
                .map_err(|_| CaptureError::Unavailable)?;
            let actual_bytes = usize::try_from(frames_read)
                .ok()
                .and_then(|count| count.checked_mul(2))
                .ok_or(CaptureError::Unavailable)?;
            packet_bytes.truncate(actual_bytes);
            pending_bytes.extend(packet_bytes);
            while pending_bytes.len() >= AUDIO_FRAME_BYTES {
                let frame = pending_bytes.drain(..AUDIO_FRAME_BYTES).collect::<Vec<_>>();
                for speech_frame in gate.push(frame) {
                    if sender
                        .blocking_send(AudioMessage::Frame(speech_frame))
                        .is_err()
                    {
                        return Ok(());
                    }
                }
            }
        }
        let _ = client.stop_stream();
        Ok(())
    })();
    wasapi::deinitialize();
    if result.is_err() {
        let _ = sender.blocking_send(AudioMessage::Failed);
    }
    result
}

#[cfg(not(target_os = "windows"))]
fn capture_audio(
    _source: AudioInputSource,
    _microphone_device_id: Option<&str>,
    sender: &mpsc::Sender<AudioMessage>,
    _stop: &CancellationToken,
) -> Result<(), CaptureError> {
    let _ = sender.blocking_send(AudioMessage::Failed);
    Err(CaptureError::Unavailable)
}

#[derive(Default)]
#[cfg(any(target_os = "windows", test))]
struct VoiceActivityGate {
    pre_roll: VecDeque<Vec<u8>>,
    active: bool,
    trailing_frames: u8,
}

#[cfg(any(target_os = "windows", test))]
impl VoiceActivityGate {
    fn push(&mut self, frame: Vec<u8>) -> Vec<Vec<u8>> {
        let speech = is_speech(&frame);
        if speech {
            self.active = true;
            self.trailing_frames = 20;
            let mut frames = self.pre_roll.drain(..).collect::<Vec<_>>();
            frames.push(frame);
            return frames;
        }
        if self.active {
            self.trailing_frames = self.trailing_frames.saturating_sub(1);
            if self.trailing_frames == 0 {
                self.active = false;
            } else {
                return vec![frame];
            }
        }
        self.pre_roll.push_back(frame);
        if self.pre_roll.len() > 12 {
            self.pre_roll.pop_front();
        }
        Vec::new()
    }
}

#[cfg(any(target_os = "windows", test))]
fn is_speech(frame: &[u8]) -> bool {
    let (sample_bytes, _) = frame.as_chunks::<2>();
    let mut count = 0_u64;
    let mut energy = 0_u64;
    for pair in sample_bytes {
        let sample = i32::from(i16::from_le_bytes(*pair));
        let magnitude = u64::from(sample.unsigned_abs());
        energy = energy.saturating_add(magnitude * magnitude);
        count += 1;
    }
    count > 0 && energy / count > 32_400
}

#[cfg(test)]
mod tests {
    use super::{VoiceActivityGate, complete_text, update_transcript};

    #[test]
    fn replaces_partial_words_and_preserves_final_tokens() {
        let mut final_text = String::new();
        let mut partial_text = String::new();

        update_transcript(
            r#"{"tokens":[{"text":"Hello ","is_final":true},{"text":"wor","is_final":false}]}"#,
            &mut final_text,
            &mut partial_text,
        )
        .expect("valid partial result");
        assert_eq!(complete_text(&final_text, &partial_text), "Hello wor");

        let finished = update_transcript(
            r#"{"tokens":[{"text":"world","is_final":true},{"text":"<fin>","is_final":true}],"finished":true}"#,
            &mut final_text,
            &mut partial_text,
        )
        .expect("valid final result");

        assert!(finished);
        assert_eq!(complete_text(&final_text, &partial_text), "Hello world");
    }

    #[test]
    fn voice_gate_suppresses_silence_and_keeps_preroll_and_speech_tail() {
        let mut gate = VoiceActivityGate::default();
        let silence = audio_frame(0);
        let speech = audio_frame(1_000);

        assert!(gate.push(silence).is_empty());
        assert_eq!(gate.push(speech).len(), 2);

        let mut sent_tail = 0;
        for _ in 0..20 {
            sent_tail += gate.push(audio_frame(0)).len();
        }
        assert_eq!(sent_tail, 19);
        assert!(gate.push(audio_frame(0)).is_empty());
    }

    fn audio_frame(sample: i16) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(640);
        for _ in 0..320 {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }
}
