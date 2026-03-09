//! Speech-to-Text via Whisper.
//!
//! Converts audio files (OGG) to text using OpenAI Whisper CLI.

use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

/// Transcribe an OGG audio file to text using Whisper.
///
/// Process:
/// 1. Convert OGG to WAV (16kHz mono) using ffmpeg
/// 2. Run whisper CLI on the WAV file
/// 3. Read the output text file
/// 4. Return transcription
///
/// Falls back to a placeholder message if Whisper isn't available.
pub async fn transcribe(ogg_path: &str) -> anyhow::Result<String> {
    let ogg_path = Path::new(ogg_path);
    if !ogg_path.exists() {
        anyhow::bail!("Audio file not found: {}", ogg_path.display());
    }

    let wav_path = "/tmp/rustclaw_stt.wav";
    let txt_path = "/tmp/rustclaw_stt.txt";

    // Step 1: Convert OGG to WAV using ffmpeg
    let ffmpeg_result = Command::new("ffmpeg")
        .args([
            "-y",           // Overwrite output
            "-i", ogg_path.to_str().unwrap_or(""),
            "-ar", "16000", // 16kHz sample rate
            "-ac", "1",     // Mono
            wav_path,
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

    // Step 2: Run Whisper on the WAV file
    // Try 'whisper' first, then 'whisper-cli'
    let whisper_result = run_whisper(wav_path, "/tmp").await;

    match whisper_result {
        Ok(()) => {
            tracing::debug!("Whisper transcription complete");
        }
        Err(e) => {
            tracing::warn!("Whisper failed: {}", e);
            // Clean up
            let _ = tokio::fs::remove_file(wav_path).await;
            return Ok("[Voice message received - STT not configured]".to_string());
        }
    }

    // Step 3: Read the output text file
    // Whisper outputs to /tmp/rustclaw_stt.txt (same base name as input + .txt)
    let transcription = match tokio::fs::read_to_string(txt_path).await {
        Ok(text) => text.trim().to_string(),
        Err(e) => {
            tracing::warn!("Failed to read transcription: {}", e);
            // Clean up
            let _ = tokio::fs::remove_file(wav_path).await;
            return Ok("[Voice message received - transcription failed]".to_string());
        }
    };

    // Clean up temp files
    let _ = tokio::fs::remove_file(wav_path).await;
    let _ = tokio::fs::remove_file(txt_path).await;

    if transcription.is_empty() {
        Ok("[Voice message received - no speech detected]".to_string())
    } else {
        Ok(transcription)
    }
}

/// Run Whisper CLI on a WAV file.
async fn run_whisper(wav_path: &str, output_dir: &str) -> anyhow::Result<()> {
    // Try 'whisper' command first (standard OpenAI Whisper)
    let result = Command::new("whisper")
        .args([
            wav_path,
            "--model", "base",
            "--output_format", "txt",
            "--output_dir", output_dir,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .await;

    match result {
        Ok(status) if status.success() => return Ok(()),
        Ok(_) => {
            // whisper exists but failed, don't try alternatives
            anyhow::bail!("whisper command failed");
        }
        Err(_) => {
            // whisper not found, try whisper-cli
        }
    }

    // Try 'whisper-cli' as fallback
    let result = Command::new("whisper-cli")
        .args([
            wav_path,
            "--model", "base",
            "--output_format", "txt",
            "--output_dir", output_dir,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .await;

    match result {
        Ok(status) if status.success() => Ok(()),
        Ok(_) => anyhow::bail!("whisper-cli command failed"),
        Err(e) => anyhow::bail!("Neither whisper nor whisper-cli available: {}", e),
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
