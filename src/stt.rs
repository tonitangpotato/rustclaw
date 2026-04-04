//! Speech-to-Text via whisper.cpp (C++ implementation, ~3x faster than Python whisper).
//!
//! Converts audio files (OGG) to text using whisper-cli with large-v3-turbo model.

use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

/// Default whisper.cpp model path.
const DEFAULT_MODEL: &str = "/Users/potato/.local/share/whisper-cpp/models/ggml-large-v3-turbo.bin";

/// Transcribe an OGG audio file to text using whisper.cpp.
///
/// Process:
/// 1. Convert OGG to WAV (16kHz mono) using ffmpeg
/// 2. Run whisper-cli on the WAV file
/// 3. Read the output text file
/// 4. Return transcription
///
/// Falls back to original whisper if whisper-cli is not available.
pub async fn transcribe(ogg_path: &str) -> anyhow::Result<String> {
    let ogg_path = Path::new(ogg_path);
    if !ogg_path.exists() {
        anyhow::bail!("Audio file not found: {}", ogg_path.display());
    }

    // Use unique temp files to allow parallel STT
    let uid = std::process::id() ^ (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos());
    let wav_path = format!("/tmp/rustclaw_stt_{}.wav", uid);
    let out_base = format!("/tmp/rustclaw_stt_out_{}", uid);

    // Step 1: Convert OGG to WAV using ffmpeg
    let ffmpeg_result = Command::new("ffmpeg")
        .args([
            "-y",           // Overwrite output
            "-i", ogg_path.to_str().unwrap_or(""),
            "-ar", "16000", // 16kHz sample rate
            "-ac", "1",     // Mono
            &wav_path,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;

    match ffmpeg_result {
        Ok(status) if status.success() => {
            tracing::debug!("Converted OGG to WAV: {}", wav_path);
        }
        Ok(status) => {
            tracing::warn!("ffmpeg failed with status: {}", status);
            return Ok("[Voice message received - audio conversion failed]".to_string());
        }
        Err(e) => {
            tracing::warn!("ffmpeg not available: {}", e);
            return Ok("[Voice message received - ffmpeg not installed]".to_string());
        }
    }

    // Step 2: Try whisper-cli (whisper.cpp) first, then fall back to Python whisper
    let model_path = DEFAULT_MODEL;
    if Path::new(model_path).exists() {
        match run_whisper_cpp(&wav_path, model_path, &out_base).await {
            Ok(text) => {
                let _ = tokio::fs::remove_file(&wav_path).await;
                return Ok(text);
            }
            Err(e) => {
                tracing::warn!("whisper.cpp failed, falling back to Python whisper: {}", e);
            }
        }
    } else {
        tracing::warn!("whisper.cpp model not found at {}, falling back to Python whisper", model_path);
    }

    // Fallback: Python whisper
    match run_whisper_python(&wav_path, "/tmp").await {
        Ok(()) => {}
        Err(e) => {
            tracing::warn!("Python whisper also failed: {}", e);
            let _ = tokio::fs::remove_file(&wav_path).await;
            return Ok("[Voice message received - STT not configured]".to_string());
        }
    }

    let python_txt = format!("/tmp/rustclaw_stt_{}.txt", uid);
    let transcription = match tokio::fs::read_to_string(&python_txt).await {
        Ok(text) => text.trim().to_string(),
        Err(e) => {
            tracing::warn!("Failed to read transcription: {}", e);
            let _ = tokio::fs::remove_file(&wav_path).await;
            return Ok("[Voice message received - transcription failed]".to_string());
        }
    };

    let _ = tokio::fs::remove_file(&wav_path).await;
    let _ = tokio::fs::remove_file(&python_txt).await;

    if transcription.is_empty() {
        Ok("[Voice message received - no speech detected]".to_string())
    } else {
        Ok(transcription)
    }
}

/// Run whisper.cpp (whisper-cli) on a WAV file. Returns transcription text directly.
async fn run_whisper_cpp(wav_path: &str, model_path: &str, out_base: &str) -> anyhow::Result<String> {
    let txt_path = format!("{}.txt", out_base);

    // Clean up old output
    let _ = tokio::fs::remove_file(&txt_path).await;

    let output = Command::new("whisper-cli")
        .args([
            "-m", model_path,
            "-l", "zh",
            "-f", wav_path,
            "-otxt",
            "-of", out_base,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("whisper-cli failed: {}", stderr);
    }

    let text = tokio::fs::read_to_string(&txt_path).await?;
    let _ = tokio::fs::remove_file(&txt_path).await;

    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        anyhow::bail!("whisper-cli produced empty output");
    }

    tracing::info!("whisper.cpp transcription ({} chars)", trimmed.len());
    Ok(trimmed)
}

/// Fallback: Run Python whisper CLI on a WAV file.
async fn run_whisper_python(wav_path: &str, output_dir: &str) -> anyhow::Result<()> {
    let result = Command::new("whisper")
        .args([
            wav_path,
            "--model", "turbo",
            "--output_format", "txt",
            "--output_dir", output_dir,
            "--language", "zh",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await;

    match result {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("whisper command failed: {}", stderr);
        }
        Err(e) => anyhow::bail!("whisper not available: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_transcribe_nonexistent_file() {
        let result = transcribe("/nonexistent/file.ogg").await;
        assert!(result.is_err());
    }
}
