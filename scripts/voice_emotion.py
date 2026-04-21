#!/usr/bin/env python3
"""Voice Emotion Analysis via wav2vec2 Speech Emotion Recognition.

Usage:
    python3 voice_emotion.py <wav_path>

Output (JSON to stdout):
    {
        "primary_emotion": "happy",
        "confidence": 0.87,
        "all_scores": {"angry": 0.02, "calm": 0.05, "happy": 0.87, ...},
        "success": true
    }

Model: ehcalabres/wav2vec2-lg-xlsr-en-speech-emotion-recognition
Labels: angry, calm, disgust, fearful, happy, neutral, sad, surprised

Audio loading: stdlib wave module (no torchaudio dependency).
"""

import json
import sys
import os
import wave
import struct

# Suppress noisy warnings
os.environ.setdefault("TOKENIZERS_PARALLELISM", "false")


def load_wav_as_float32(wav_path: str):
    """Load a 16-bit PCM WAV file into a float32 numpy array.

    Uses only stdlib `wave` + numpy. No torchaudio needed.
    Expects 16kHz mono WAV (ffmpeg pre-converts to this).
    """
    import numpy as np

    with wave.open(wav_path, "rb") as wf:
        n_channels = wf.getnchannels()
        sampwidth = wf.getsampwidth()
        framerate = wf.getframerate()
        n_frames = wf.getnframes()

        raw = wf.readframes(n_frames)

    if sampwidth == 2:
        dtype = np.int16
    elif sampwidth == 4:
        dtype = np.int32
    else:
        raise ValueError(f"Unsupported sample width: {sampwidth}")

    samples = np.frombuffer(raw, dtype=dtype).astype(np.float32)

    # If stereo, average to mono
    if n_channels > 1:
        samples = samples.reshape(-1, n_channels).mean(axis=1)

    # Normalize int16 → [-1.0, 1.0]
    if sampwidth == 2:
        samples /= 32768.0
    elif sampwidth == 4:
        samples /= 2147483648.0

    # Truncate to 30 seconds max
    max_samples = framerate * 30
    if len(samples) > max_samples:
        samples = samples[:max_samples]

    return samples, framerate


def analyze(wav_path: str) -> dict:
    """Run SER on a WAV file, return emotion scores."""
    try:
        import torch
        import numpy as np
        from transformers import AutoModelForAudioClassification, AutoFeatureExtractor
    except ImportError as e:
        return {"success": False, "error": f"Missing dependency: {e}"}

    if not os.path.exists(wav_path):
        return {"success": False, "error": f"File not found: {wav_path}"}

    model_name = "ehcalabres/wav2vec2-lg-xlsr-en-speech-emotion-recognition"

    try:
        # Load model and feature extractor
        feature_extractor = AutoFeatureExtractor.from_pretrained(model_name)
        model = AutoModelForAudioClassification.from_pretrained(model_name)
        model.eval()

        # Load WAV using stdlib (no torchaudio)
        samples, sample_rate = load_wav_as_float32(wav_path)

        # Feature extraction (expects float32 numpy array at 16kHz)
        inputs = feature_extractor(
            samples,
            sampling_rate=16000,  # ffmpeg already converts to 16kHz
            return_tensors="pt",
            padding=True,
        )

        # Inference
        with torch.no_grad():
            logits = model(**inputs).logits

        # Softmax to get probabilities
        probs = torch.nn.functional.softmax(logits, dim=-1).squeeze()

        # Map to labels
        id2label = model.config.id2label
        all_scores = {}
        for idx, prob in enumerate(probs.tolist()):
            label = id2label.get(idx, f"unknown_{idx}")
            all_scores[label] = round(prob, 4)

        # Primary emotion
        primary_idx = probs.argmax().item()
        primary_emotion = id2label.get(primary_idx, "unknown")
        confidence = probs[primary_idx].item()

        return {
            "primary_emotion": primary_emotion,
            "confidence": round(confidence, 4),
            "all_scores": all_scores,
            "success": True,
        }

    except Exception as e:
        return {"success": False, "error": str(e)}


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print(json.dumps({"success": False, "error": "Usage: voice_emotion.py <wav_path>"}))
        sys.exit(1)

    result = analyze(sys.argv[1])
    print(json.dumps(result))
    sys.exit(0 if result.get("success") else 1)
