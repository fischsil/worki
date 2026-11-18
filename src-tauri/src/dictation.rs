//! Dictation for the chat: the microphone's audio goes to Apple's speech recognition, on the
//! device where the language supports it, and the text comes back as the user speaks.

use std::ptr::NonNull;
use std::sync::{mpsc, Mutex};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::AnyThread;
use objc2_avf_audio::{AVAudioEngine, AVAudioPCMBuffer, AVAudioTime};
use objc2_foundation::{NSError, NSString, NSUserDefaults};
use objc2_speech::{
    SFSpeechAudioBufferRecognitionRequest, SFSpeechRecognitionResult, SFSpeechRecognitionTask,
    SFSpeechRecognizer, SFSpeechRecognizerAuthorizationStatus,
};
use serde::Serialize;
use tauri::ipc::Channel;

use crate::{blocking, CmdResult};

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DictationEvent {
    /// Everything recognized so far; `done` once the user stopped and the text is final.
    Text {
        text: String,
        done: bool,
    },
    Failed {
        message: String,
    },
}

struct Dictation {
    engine: Retained<AVAudioEngine>,
    request: Retained<SFSpeechAudioBufferRecognitionRequest>,
    task: Retained<SFSpeechRecognitionTask>,
    _recognizer: Retained<SFSpeechRecognizer>,
}

// SAFETY: AVAudioEngine, the request and the task may be used from any thread, and the mutex
// keeps uses one at a time.
unsafe impl Send for Dictation {}

impl Dictation {
    fn stop_listening(&self) {
        unsafe {
            self.engine.stop();
            self.engine.inputNode().removeTapOnBus(0);
            self.request.endAudio();
        }
    }
}

static CURRENT: Mutex<Option<Dictation>> = Mutex::new(None);

/// Asks for speech recognition the first time; false when the user turned it down.
#[tauri::command]
pub async fn dictation_access() -> CmdResult<bool> {
    let status = unsafe { SFSpeechRecognizer::authorizationStatus() };
    if status != SFSpeechRecognizerAuthorizationStatus::NotDetermined {
        return Ok(status == SFSpeechRecognizerAuthorizationStatus::Authorized);
    }
    let (tx, rx) = mpsc::channel();
    {
        let answer = RcBlock::new(move |status: SFSpeechRecognizerAuthorizationStatus| {
            tx.send(status).ok();
        });
        unsafe { SFSpeechRecognizer::requestAuthorization(&answer) };
    }
    let status = blocking(move || Ok(rx.recv()?)).await?;
    Ok(status == SFSpeechRecognizerAuthorizationStatus::Authorized)
}

/// Starts listening; `on_event` gets the text until `stop_dictation` or `cancel_dictation`.
#[tauri::command]
pub fn start_dictation(on_event: Channel<DictationEvent>) -> CmdResult<()> {
    cancel_dictation();
    if !dictation_turned_on() {
        return Err("Turn on Dictation in System Settings › Keyboard. Apple's speech recognition needs it, even on this Mac.".into());
    }
    let recognizer = unsafe { SFSpeechRecognizer::init(SFSpeechRecognizer::alloc()) }
        .ok_or("Speech recognition doesn't support your language")?;
    if !unsafe { recognizer.isAvailable() } {
        return Err("Speech recognition isn't available right now".into());
    }
    let request = unsafe { SFSpeechAudioBufferRecognitionRequest::new() };
    unsafe {
        request.setShouldReportPartialResults(true);
        request.setAddsPunctuation(true);
        if recognizer.supportsOnDeviceRecognition() {
            request.setRequiresOnDeviceRecognition(true);
        }
    }

    let engine = unsafe { AVAudioEngine::new() };
    let input = unsafe { engine.inputNode() };
    let format = unsafe { input.outputFormatForBus(0) };
    if unsafe { format.sampleRate() } <= 0.0 {
        return Err("worki can't use the microphone. Allow it in System Settings › Privacy & Security › Microphone.".into());
    }
    let listener = request.clone();
    let tap = RcBlock::new(
        move |buffer: NonNull<AVAudioPCMBuffer>, _: NonNull<AVAudioTime>| unsafe {
            listener.appendAudioPCMBuffer(buffer.as_ref());
        },
    );
    unsafe {
        input.installTapOnBus_bufferSize_format_block(
            0,
            1024,
            Some(&format),
            RcBlock::as_ptr(&tap),
        );
        engine.prepare();
    }
    if let Err(e) = unsafe { engine.startAndReturnError() } {
        unsafe { input.removeTapOnBus(0) };
        return Err(e.localizedDescription().to_string());
    }

    let results = RcBlock::new(
        move |result: *mut SFSpeechRecognitionResult, error: *mut NSError| {
            if let Some(result) = unsafe { result.as_ref() } {
                let text = unsafe { result.bestTranscription().formattedString() }.to_string();
                let done = unsafe { result.isFinal() };
                on_event.send(DictationEvent::Text { text, done }).ok();
                if done {
                    return;
                }
            }
            if let Some(error) = unsafe { error.as_ref() } {
                let message = error.localizedDescription().to_string();
                on_event.send(DictationEvent::Failed { message }).ok();
            }
        },
    );
    let task = unsafe { recognizer.recognitionTaskWithRequest_resultHandler(&request, &results) };
    *CURRENT.lock().unwrap() = Some(Dictation {
        engine,
        request,
        task,
        _recognizer: recognizer,
    });
    Ok(())
}

/// Whether Dictation or Siri is on, which speech recognition refuses to work without; true when
/// the settings can't be read.
fn dictation_turned_on() -> bool {
    let suite = NSString::from_str("com.apple.assistant.support");
    let Some(settings) = NSUserDefaults::initWithSuiteName(NSUserDefaults::alloc(), Some(&suite))
    else {
        return true;
    };
    ["Dictation Enabled", "Assistant Enabled"]
        .iter()
        .any(|key| {
            let key = NSString::from_str(key);
            settings.objectForKey(&key).is_none() || settings.boolForKey(&key)
        })
}

/// Stops listening; the final text follows as the last event.
#[tauri::command]
pub fn stop_dictation() {
    if let Some(dictation) = CURRENT.lock().unwrap().as_ref() {
        dictation.stop_listening();
    }
}

#[tauri::command]
pub fn cancel_dictation() {
    if let Some(dictation) = CURRENT.lock().unwrap().take() {
        dictation.stop_listening();
        unsafe { dictation.task.cancel() };
    }
}
