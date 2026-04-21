//! Voice Emotion Analysis — Extracts emotional signals from voice messages.
//!
//! Uses wav2vec2-based Speech Emotion Recognition (SER) via a Python helper
//! script to classify audio into 8 emotion categories: angry, calm, disgust,
//! fearful, happy, neutral, sad, surprised.
//!
//! Architecture integration:
//!   - Called from channel adapters (Telegram, Signal) after voice → WAV conversion
//!   - Produces InteroceptiveSignal with SignalSource::VoiceEmotion
//!   - Signals fed to InteroceptiveHub for integration with other subsystems
//!   - Agent becomes aware of user's emotional tone via system prompt injection

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;

use engramai::interoceptive::{InteroceptiveSignal, SignalContext, SignalSource};
use serde::Deserialize;
use tokio::process::Command;

/// Path to the Python SER helper script (relative to workspace root).
const VOICE_EMOTION_SCRIPT: &str = "scripts/voice_emotion.py";

/// Raw output from the Python helper.
#[derive(Debug, Deserialize)]
struct SerOutput {
    primary_emotion: Option<String>,
    confidence: Option<f64>,
    all_scores: Option<HashMap<String, f64>>,
    success: bool,
    error: Option<String>,
}

/// Analyzed voice emotion result.
#[derive(Debug, Clone)]
pub struct VoiceEmotionResult {
    /// Primary detected emotion (e.g., "happy", "angry", "sad").
    pub primary_emotion: String,
    /// Confidence score for the primary emotion [0.0, 1.0].
    pub confidence: f64,
    /// All emotion scores (label → probability).
    pub all_scores: HashMap<String, f64>,
}

impl VoiceEmotionResult {
    /// Convert this result into an InteroceptiveSignal.
    ///
    /// Mapping from emotion labels to valence/arousal:
    ///   - happy:    valence=+0.8, arousal=0.6
    ///   - calm:     valence=+0.4, arousal=0.1
    ///   - neutral:  valence= 0.0, arousal=0.1
    ///   - surprised: valence=+0.2, arousal=0.8
    ///   - sad:      valence=-0.6, arousal=0.2
    ///   - fearful:  valence=-0.5, arousal=0.8
    ///   - angry:    valence=-0.8, arousal=0.9
    ///   - disgust:  valence=-0.7, arousal=0.5
    ///
    /// The valence/arousal are weighted by confidence so uncertain
    /// classifications don't overly influence the interoceptive state.
    pub fn to_signal(&self, domain: &str, speaker_id: Option<String>) -> InteroceptiveSignal {
        let (base_valence, base_arousal) = emotion_to_valence_arousal(&self.primary_emotion);

        // Weight by confidence — uncertain emotions have muted effect
        let valence = base_valence * self.confidence;
        let arousal = base_arousal * self.confidence;

        InteroceptiveSignal::new(
            SignalSource::VoiceEmotion,
            Some(domain.to_string()),
            valence,
            arousal,
        )
        .with_context(SignalContext::VoiceEmotion {
            primary_emotion: self.primary_emotion.clone(),
            confidence: self.confidence,
            all_scores: self.all_scores.clone(),
            speaker_id,
        })
    }
}

/// Map emotion labels to (valence, arousal) in the circumplex model.
///
/// Based on Russell's circumplex model of affect (1980):
///   - Valence: pleasure-displeasure axis (-1 to +1)
///   - Arousal: activation-deactivation axis (0 to 1)
fn emotion_to_valence_arousal(emotion: &str) -> (f64, f64) {
    match emotion {
        "happy" => (0.8, 0.6),
        "calm" => (0.4, 0.1),
        "neutral" => (0.0, 0.1),
        "surprised" => (0.2, 0.8),
        "sad" => (-0.6, 0.2),
        "fearful" => (-0.5, 0.8),
        "angry" => (-0.8, 0.9),
        "disgust" => (-0.7, 0.5),
        _ => (0.0, 0.3), // Unknown emotion → slight arousal, neutral valence
    }
}

/// Analyze a WAV file for emotional content.
///
/// Calls the Python SER script asynchronously. Returns None if:
/// - The script is not found
/// - The WAV file doesn't exist
/// - The model fails to classify
/// - Timeout (10 seconds)
///
/// This is designed to be fire-and-forget friendly — voice message
/// processing should not be blocked by SER failures.
pub async fn analyze_voice_emotion(wav_path: &str, workspace: &str) -> Option<VoiceEmotionResult> {
    let script_path = Path::new(workspace).join(VOICE_EMOTION_SCRIPT);
    if !script_path.exists() {
        tracing::debug!("Voice emotion script not found at {}", script_path.display());
        return None;
    }

    if !Path::new(wav_path).exists() {
        tracing::debug!("WAV file not found: {}", wav_path);
        return None;
    }

    let output = match tokio::time::timeout(
        std::time::Duration::from_secs(15),
        Command::new("python3")
            .arg(&script_path)
            .arg(wav_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output(),
    )
    .await
    {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            tracing::warn!("Voice emotion analysis failed to spawn: {}", e);
            return None;
        }
        Err(_) => {
            tracing::warn!("Voice emotion analysis timed out (15s)");
            return None;
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::debug!("Voice emotion script failed: {}", stderr);
        // Still try to parse stdout — script may have written error JSON
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let ser_output: SerOutput = match serde_json::from_str(&stdout) {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!("Voice emotion: failed to parse output: {} (raw: {})", e, stdout.chars().take(200).collect::<String>());
            return None;
        }
    };

    if !ser_output.success {
        tracing::debug!(
            "Voice emotion analysis unsuccessful: {}",
            ser_output.error.as_deref().unwrap_or("unknown")
        );
        return None;
    }

    let primary = ser_output.primary_emotion?;
    let confidence = ser_output.confidence.unwrap_or(0.0);
    let all_scores = ser_output.all_scores.unwrap_or_default();

    // Only report if confidence is meaningful (>20%)
    if confidence < 0.2 {
        tracing::debug!(
            "Voice emotion: low confidence ({:.1}% for {}), skipping",
            confidence * 100.0,
            primary
        );
        return None;
    }

    tracing::info!(
        "Voice emotion detected: {} ({:.1}%)",
        primary,
        confidence * 100.0
    );

    Some(VoiceEmotionResult {
        primary_emotion: primary,
        confidence,
        all_scores,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emotion_to_valence_arousal_known_emotions() {
        // Happy: high positive valence, moderate arousal
        let (v, a) = emotion_to_valence_arousal("happy");
        assert!((v - 0.8).abs() < f64::EPSILON);
        assert!((a - 0.6).abs() < f64::EPSILON);

        // Angry: high negative valence, high arousal
        let (v, a) = emotion_to_valence_arousal("angry");
        assert!((v - (-0.8)).abs() < f64::EPSILON);
        assert!((a - 0.9).abs() < f64::EPSILON);

        // Calm: moderate positive valence, low arousal
        let (v, a) = emotion_to_valence_arousal("calm");
        assert!((v - 0.4).abs() < f64::EPSILON);
        assert!((a - 0.1).abs() < f64::EPSILON);

        // Neutral: zero valence, minimal arousal
        let (v, a) = emotion_to_valence_arousal("neutral");
        assert!(v.abs() < f64::EPSILON);
        assert!((a - 0.1).abs() < f64::EPSILON);

        // Sad: negative valence, low arousal
        let (v, a) = emotion_to_valence_arousal("sad");
        assert!((v - (-0.6)).abs() < f64::EPSILON);
        assert!((a - 0.2).abs() < f64::EPSILON);

        // Fearful: negative valence, high arousal
        let (v, a) = emotion_to_valence_arousal("fearful");
        assert!((v - (-0.5)).abs() < f64::EPSILON);
        assert!((a - 0.8).abs() < f64::EPSILON);

        // Disgust: negative valence, moderate arousal
        let (v, a) = emotion_to_valence_arousal("disgust");
        assert!((v - (-0.7)).abs() < f64::EPSILON);
        assert!((a - 0.5).abs() < f64::EPSILON);

        // Surprised: slightly positive valence, high arousal
        let (v, a) = emotion_to_valence_arousal("surprised");
        assert!((v - 0.2).abs() < f64::EPSILON);
        assert!((a - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn test_emotion_to_valence_arousal_unknown() {
        let (v, a) = emotion_to_valence_arousal("confused");
        assert!(v.abs() < f64::EPSILON);
        assert!((a - 0.3).abs() < f64::EPSILON);
    }

    #[test]
    fn test_voice_emotion_to_signal() {
        let result = VoiceEmotionResult {
            primary_emotion: "angry".to_string(),
            confidence: 0.9,
            all_scores: {
                let mut m = HashMap::new();
                m.insert("angry".to_string(), 0.9);
                m.insert("neutral".to_string(), 0.05);
                m.insert("sad".to_string(), 0.05);
                m
            },
        };

        let signal = result.to_signal("communication", Some("user123".to_string()));

        assert_eq!(signal.source, SignalSource::VoiceEmotion);
        assert_eq!(signal.domain.as_deref(), Some("communication"));
        // angry base: valence=-0.8, arousal=0.9, weighted by confidence 0.9
        assert!((signal.valence - (-0.72)).abs() < 0.01);
        assert!((signal.arousal - 0.81).abs() < 0.01);

        // Context should carry emotion details
        match &signal.context {
            Some(SignalContext::VoiceEmotion {
                primary_emotion,
                confidence,
                speaker_id,
                ..
            }) => {
                assert_eq!(primary_emotion, "angry");
                assert!((confidence - 0.9).abs() < f64::EPSILON);
                assert_eq!(speaker_id.as_deref(), Some("user123"));
            }
            _ => panic!("Expected VoiceEmotion context"),
        }
    }

    #[test]
    fn test_voice_emotion_to_signal_low_confidence() {
        let result = VoiceEmotionResult {
            primary_emotion: "happy".to_string(),
            confidence: 0.3,
            all_scores: HashMap::new(),
        };

        let signal = result.to_signal("communication", None);

        // happy base: valence=0.8, arousal=0.6, weighted by 0.3
        assert!((signal.valence - 0.24).abs() < 0.01);
        assert!((signal.arousal - 0.18).abs() < 0.01);
    }

    #[test]
    fn test_voice_emotion_to_signal_neutral() {
        let result = VoiceEmotionResult {
            primary_emotion: "neutral".to_string(),
            confidence: 0.95,
            all_scores: HashMap::new(),
        };

        let signal = result.to_signal("general", None);

        // neutral: valence=0.0, arousal=0.1
        assert!(signal.valence.abs() < 0.01);
        assert!((signal.arousal - 0.095).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_analyze_nonexistent_wav() {
        let result = analyze_voice_emotion("/nonexistent/file.wav", "/tmp").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_analyze_nonexistent_script() {
        let result = analyze_voice_emotion("/tmp/test.wav", "/nonexistent/workspace").await;
        assert!(result.is_none());
    }
}
