# Autopilot Status — 2026-04-27 night

> Written after `task:bench-impl-cargo-toml` succeeded but the very next
> workspace build hit ENOSPC. Stop condition #7 (workspace build break) —
> environmental, not a code regression.
>
> Note: two earlier in-night blocks are recorded as `⛔ BLOCKED` lines
> directly in `tasks/2026-04-27-night-autopilot.md` (task:mig-impl-backfill-perrecord
> stop-condition #4; task:mig-impl-topics stop-condition #5). They are
> unrelated to the disk-full issue described below.

## Newly completed since last STATUS

- **task:bench-impl-cargo-toml ✅** commit `db31a2f` — `crates/engram-bench/Cargo.toml`
  + placeholder `lib.rs`/`main.rs`. GUARD-9 boundary verified via
  `cargo tree -p engramai --edges normal --depth 1`: engramai's direct
  dep set is unchanged; the only candidate-leak entries (`rand_chacha@0.3.1`,
  `tempfile`) are pre-existing transitive deps of engramai (`rand` →
  `rand_chacha`; `reqwest`/`native-tls` → `tempfile`), not introduced by
  engram-bench. `cargo build -p engram-bench` and `cargo test -p engram-bench`
  both green.

## Stopped at

- **task:bench-impl-lib** (next §D task in dep order)

## Why — STOP CONDITION #7 (workspace build break, environmental)

`cargo build --workspace` fails during `engram-cli` link step with:

```
ld: write() failed, errno=28
clang: error: linker command failed with exit code 1
error: could not compile `engram-cli` (bin "engram") due to 1 previous error
```

`errno=28` = ENOSPC (no space left on device). Disk state at time of failure:

```
$ df -h /Users/potato
/dev/disk3s5   228Gi   186Gi   166Mi   100%   ...   /System/Volumes/Data
```

166 MiB free on a 228 GiB volume — at 100% capacity. The Rust linker requires
several hundred MiB of scratch space to link a binary the size of `engram`
(which depends on `engramai` + `engramai-migrate` + `reqwest` + tokio + rusqlite).

The engram repo's `target/` dir alone is **19 GiB** at this commit.

This is **not** a code regression from `task:bench-impl-cargo-toml`. The
per-crate build+test of `engram-bench` succeeds (verified before commit). The
workspace-level break is purely environmental — any subsequent task that
requires `cargo build`/`cargo test` for an executable target will fail the
same way until disk space is recovered.

## What I tried

1. Confirmed the issue is environmental, not code: `cargo build -p
   engram-bench` succeeds; `cargo build -p engramai` succeeds; the link
   failure is specific to `engram-cli` which produces a much larger binary
   artifact.
2. Did **not** attempt to delete `target/` or any cache. SOUL.md is explicit:
   "永远不要在没有 potato 明确同意的情况下删除任何数据" — that includes
   recoverable build caches. Per AGENTS.md disk-full guidance: "ask first or
   download to local before deleting." Autopilot is asleep-time work; potato
   isn't here to confirm. STOP is the correct action.

## Why I am NOT just running `cargo clean`

- It is the obvious-looking fix and would unblock 16 downstream §D tasks.
- But: SOUL.md / AGENTS.md / USER.md preferences override expedience. Potato
  has explicit rules about deletion. `target/` is _probably_ safe but the
  rule is unconditional, and autopilot is exactly the situation where I
  should defer.
- Also: `target/` for some workspaces holds incremental build state that, if
  blown away mid-night while other agents/processes might be touching the
  repo, can cause cascading rebuilds + new disk pressure spikes. Not worth
  the gamble at 166 MiB free.

## Tests state (as of stop)

- `engramai` (lib): not re-run this iteration. Last green at
  `task:retr-test-determinism-routing-accuracy` completion (per autopilot
  log). No engramai code touched in this task.
- `engramai-migrate` (full): last green at `task:mig-test-compat-rollback`
  completion (193 tests). No migration code touched in this task.
- `engram-bench` (full): **0 pass / 0 fail / 0 ignored** — no tests yet,
  to be added by `task:bench-impl-lib` and downstream.
- `engram-cli` link: **FAIL** (ENOSPC, environmental).

## Suggested next step for potato

1. **Free disk space.** Quick wins likely:
   - `cargo clean` in `/Users/potato/clawd/projects/engram/` (19 GB) and any
     other Rust workspace whose `target/` is no longer hot. (Your call —
     I deliberately did not touch it.)
   - Check `~/Library/Caches`, `~/.cargo/registry/cache`, `~/.rustup`
     toolchain old versions, Docker images if applicable.
   - `du -sh ~/.cargo ~/Library/Caches ~/Library/Containers` for quick
     hotspots.
2. After ≥10 GiB recovered, resume autopilot at **task:bench-impl-lib**
   (next checkbox in §D queue). All upstream deps for that task are done
   per the v03-benchmarks build plan (it depends on `task:bench-impl-cargo-toml`
   which is now ✅).
3. The disk-full was a host-level issue, not a sign of a runaway autopilot
   process. Nothing to roll back.

## Diff produced this iteration

```
$ git log --oneline -1
db31a2f feat(bench): T1 engram-bench Cargo.toml — GUARD-9 boundary established (task:bench-impl-cargo-toml)

$ git show --stat db31a2f
 .gid-v03-context/graph.db       | (binary, +1 task status flip)
 crates/engram-bench/Cargo.toml  | new file, 88 lines
 crates/engram-bench/src/lib.rs  | new file, 22 lines  (placeholder, see bench-impl-lib)
 crates/engram-bench/src/main.rs | new file, 18 lines  (placeholder, see bench-impl-main)
```

## Heads-up for the next agent / potato

- The **GUARD-9 boundary check** is now a reusable verification: anyone
  adding to `engram-bench/Cargo.toml` must re-run
  `cargo tree -p engramai --edges normal --depth 1`
  and assert engramai's direct deps are unchanged. Document this in the
  next bench-impl-* task as the standing acceptance criterion.
- The placeholder `engram-bench` binary exits with code 2 deliberately —
  if any CI or release-gate script invokes `engram-bench` before
  `task:bench-impl-main` lands, it will visibly fail rather than silently
  "succeed".
- Once disk is free, the §D queue order per build plan is:
  1. ✅ Cargo.toml (DONE)
  2. lib.rs (re-exports)
  3. baselines + harness/mod + harness/repro + harness/gates
  4. anonymizer + scorers
  5. drivers (each depends on scorers + harness)
  6. main.rs + reporting (last)
