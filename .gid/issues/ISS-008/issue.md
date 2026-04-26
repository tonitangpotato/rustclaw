---
id: "ISS-008"
title: "Telegram Disconnect / Reconnect Loop"
status: open
priority: P2
created: 2026-04-19
component: "src/channels/telegram.rs"
---
# ISS-008: Telegram Long-Poll Silent Disconnect

**Status**: ✅ Fixed  
**Severity**: High  
**Component**: `src/channels/telegram.rs`  
**Date Reported**: 2026-04-14  
**Date Fixed**: 2026-04-14  

## Symptom

Telegram polling silently stops receiving messages. Process is alive, no errors in logs, but no new messages arrive. Recovers only when the process is restarted or sometimes when a new message "kicks" the connection.

## Root Cause

The `reqwest::Client` used for Telegram long-polling was created with `Client::new()` — **no HTTP-level timeout, no TCP keep-alive**.

Telegram's `getUpdates` API uses long-polling with a server-side `timeout: 30` parameter (server holds the connection up to 30s before returning empty). However:

1. **No client-side timeout** — if the TCP connection is silently killed by an intermediate device (NAT router, ISP, cloud proxy), `reqwest` will hang indefinitely waiting for a response that will never come.
2. **No TCP keep-alive** — default OS TCP keep-alive is ~2 hours. Intermediate NAT devices typically expire idle connections after 60-120 seconds. Without keep-alive probes, a dead connection is undetectable.
3. **The result** — the polling loop hangs on `.send().await` forever. No error is raised, no timeout fires, the loop never advances.

### Why Matrix didn't have this bug

The Matrix channel implementation already had the correct pattern:
```rust
.timeout(Duration::from_millis(sync_timeout_ms + 10000))
```
Telegram was simply missing this safeguard.

## Fix

### 1. HTTP Client with proper timeouts and keep-alive

```rust
// Before (broken):
let client = reqwest::Client::new();

// After (fixed):
let client = reqwest::Client::builder()
    .timeout(Duration::from_secs(45))         // Absolute request timeout (30s poll + 15s buffer)
    .connect_timeout(Duration::from_secs(10)) // Connection establishment timeout
    .tcp_keepalive(Duration::from_secs(15))   // TCP probe every 15s — detects dead connections fast
    .build()?;
```

**Why 45s**: Telegram server-side timeout is 30s. Normal response arrives within 0-30s. If 45s elapses with no response, the connection is certainly dead → reqwest raises a timeout error → loop retries.

**Why 15s keep-alive**: Most NAT devices expire idle connections at 60-120s. Sending TCP probes every 15s keeps the connection alive through NAT and also detects dead peers within seconds.

### 2. Consecutive error tracking

Added `consecutive_errors` counter with `tracing::warn!` logging to make silent-disconnect patterns visible in logs.

## Files Changed

- `src/channels/telegram.rs` — `Client::new()` → `Client::builder()` with timeouts; added consecutive error counter in polling loop

## Verification

- `cargo build --release` — 0 errors, 0 new warnings
- Deploy and monitor: timeout errors should now appear in logs instead of silent hangs
