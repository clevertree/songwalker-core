# Copilot Instructions — songwalker-core

## Project Overview

`songwalker-core` is the Rust core library for the SongWalker music programming language. It provides:
- **Lexer / Parser** — tokenizes and parses `.sw` source files into an AST
- **Compiler** — compiles the AST into a flat `EventList` (timed note/track events)
- **DSP Engine** — renders `EventList` to audio (oscillators, samplers, envelopes, filters, mixer)
- **Preset System** — instrument preset descriptors (sampler zones, composites)
- **WASM bindings** — `compile_song()`, `render_song_wav()`, `render_song_samples()`, `core_version()`

## Downstream Repos

Three repos depend on this crate:

| Repo | Path | Dependency Type | Notes |
|------|------|----------------|-------|
| **songwalker-web** | `../songwalker-web` | WASM copy in `src/wasm/` | Browser editor + player |
| **songwalker-cli** | `../songwalker-cli` | `Cargo.toml` path dep | CLI renderer + tuner |
| **songwalker-vsti** | `../songwalker-vsti` | `Cargo.toml` path dep (no default-features) | VST3 plugin |

## Deploying Core Updates

After making changes to songwalker-core:

### 1. Run core tests
```bash
cd /home/ari/dev/songwalker-core
cargo test
```

### 2. Rebuild WASM and embed in songwalker-web
```bash
cd /home/ari/dev/songwalker-core
wasm-pack build --target web --out-dir /home/ari/dev/songwalker-web/src/wasm
```
This overwrites the JS/TS/WASM files in `songwalker-web/src/wasm/` directly.

### 3. Run downstream tests
```bash
# CLI (path dep, auto-picks up changes)
cd /home/ari/dev/songwalker-cli && cargo test

# VSTi (path dep, auto-picks up changes)
cd /home/ari/dev/songwalker-vsti && cargo test
```

### 4. Commit in order
Commit songwalker-core first, then the downstream repos that were affected:
```bash
cd /home/ari/dev/songwalker-core && git add -A && git commit -m "..." && git push
cd /home/ari/dev/songwalker-web && git add -A && git commit -m "..." && git push
cd /home/ari/dev/songwalker-cli && git add -A && git commit -m "..." && git push
cd /home/ari/dev/songwalker-vsti && git add -A && git commit -m "..." && git push
```

Only commit downstream repos if they have actual changes (check `git status` first).

### 5. Version bump & release build
After significant changes (bug fixes, new features, breaking changes), bump the
patch version **before** committing and push a tag to trigger a release build:

**When to bump:** Bug fixes, new features, behavioral changes, dependency updates
that affect output. **Do not bump** for docs-only, test-only, or refactor-only changes.

```bash
# Bump patch version in Cargo.toml (e.g. 0.1.0 → 0.1.1)
# Update songwalker-core/Cargo.toml version field
# Update songwalker-vsti/Cargo.toml version field (keep in sync)

# After all tests pass and all repos are committed+pushed:
cd /home/ari/dev/songwalker-vsti
git tag v<NEW_VERSION>
git push origin v<NEW_VERSION>
```

The tag push triggers the `Build & Release` workflow in songwalker-vsti.

### 6. Verify GitHub Actions
After pushing, check that any triggered GitHub Actions workflows succeed:
```bash
# Check runs for each repo that has workflows
cd /home/ari/dev/songwalker-vsti && gh run list --limit 3

# Watch a specific run (get ID from run list)
gh run watch <run-id>

# View logs for a failed run
gh run view <run-id> --log-failed
```

If a workflow fails, inspect the logs, fix the issue, and push again.
Repeat until the workflow passes before moving on.

**Current workflow triggers:**
- **songwalker-vsti** — `Build & Release` runs on tag pushes (`v*`) and `workflow_dispatch`
- **songwalker-core, songwalker-web, songwalker-cli** — no workflows yet

## Version

The crate version lives in `Cargo.toml` (`version = "..."`) and is exposed at runtime via:
- **Rust:** `songwalker_core::VERSION` (compile-time `env!("CARGO_PKG_VERSION")`)
- **WASM:** `core_version()` — returns the version string to JS

The web editor displays this in the header via `core_version()` after WASM init.

## Testing

All tests are in-module (`#[cfg(test)] mod tests`). Key test areas:
- `src/lexer.rs` — tokenization
- `src/parser.rs` — AST construction
- `src/compiler.rs` — event compilation, instrument resolution, `loadPreset`
- `src/preset.rs` — preset descriptors, playback rate, zone matching
- `src/dsp/*.rs` — oscillator, envelope, sampler, engine, mixer, filter, tuner, renderer
