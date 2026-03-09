//! Text-to-Speech integration using edge-tts.
//!
//! Pipeline: text → edge-tts (mp3) → ffmpeg (opus/ogg) → send as voice

use std::path::Path;

/// Default TTS voice.
const DEFAULT_VOICE: &str = "en-US-EmmaMultilingualNeural";

/// TTS engine configuration.
pub struct TtsConfig {
    pub voice: String,
    pub edge_tts_path: String,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            voice: DEFAULT_VOICE.to_string(),
            edge_tts_path: "/Users/potato/.openclaw/tts-env/bin/edge-tts".to_string(),
        }
    }
}

/// Synthesize text to an OGG/Opus file for Telegram voice messages.
/// Returns the path to the output file.
pub async fn synthesize(text: &str, config: &TtsConfig) -> anyhow::Result<String> {
    let tmp_mp3 = "/tmp/rustclaw_tts.mp3";
    let tmp_ogg = "/tmp/rustclaw_tts.ogg";

    // Write text to temp file to avoid shell escaping issues
    let tmp_input = "/tmp/rustclaw_tts_input.txt";
    tokio::fs::write(tmp_input, text).await?;

    // Run edge-tts
    let tts_output = tokio::process::Command::new(&config.edge_tts_path)
        .arg("--voice")
        .arg(&config.voice)
        .arg("--file")
        .arg(tmp_input)
        .arg("--write-media")
        .arg(tmp_mp3)
        .output()
        .await?;

    if !tts_output.status.success() {
        let stderr = String::from_utf8_lossy(&tts_output.stderr);
        anyhow::bail!("edge-tts failed: {}", stderr);
    }

    // Convert to OGG/Opus with ffmpeg
    let ffmpeg_output = tokio::process::Command::new("ffmpeg")
        .args(["-y", "-i", tmp_mp3, "-c:a", "libopus", tmp_ogg])
        .output()
        .await?;

    if !ffmpeg_output.status.success() {
        let stderr = String::from_utf8_lossy(&ffmpeg_output.stderr);
        anyhow::bail!("ffmpeg conversion failed: {}", stderr);
    }

    // Verify output exists
    if !Path::new(tmp_ogg).exists() {
        anyhow::bail!("TTS output file not created");
    }

    Ok(tmp_ogg.to_string())
}
