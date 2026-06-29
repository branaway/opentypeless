# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

OpenTypeless is a cross-platform (macOS/Windows/Linux) AI voice-typing desktop app built with **Tauri 2** (Rust backend + React 19/TypeScript frontend). Press a global hotkey, speak, and the app transcribes (STT), rewrites with an LLM ("polish"), and types/pastes the result into whatever app is focused.

Two operating modes:
- **BYOK (bring your own key)** — local-first; the user supplies STT/LLM provider keys. All core features work with no cloud connection.
- **Cloud** — uses the OpenTypeless backend (`www.opentypeless.com`, override with `API_BASE_URL` / `VITE_API_BASE_URL`) with Better Auth login; a `SessionToken` flows from the frontend to the Rust `"cloud"` providers.

## Commands

```bash
npm install                 # install JS deps (needs Node 20+, Rust stable, Tauri prereqs)
npm run tauri dev           # run the full desktop app in dev (spawns vite on :1420)
npm run tauri build         # production build → src-tauri/target/release/bundle/
npm run dev                 # frontend only (vite), no Rust shell

# Frontend checks (match CI)
npx tsc --noEmit
npx eslint src/
npx prettier --check src/   # `npm run format` to autofix
npx vitest run              # `npm run test:watch` for watch mode
npx vitest run src/lib/__tests__/api.test.ts   # single test file

# Rust checks (match CI) — always pass the manifest path
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml stt::tests   # single module
```

CI runs the frontend checks on Ubuntu and the Rust checks on all three platforms. `cargo clippy` runs with `-D warnings` — warnings fail the build. TypeScript is strict, no `any`.

## Architecture

### The recording pipeline (Rust, `src-tauri/src/pipeline.rs`)

The heart of the app. `PipelineHandle` is a Tauri-managed state holding an atomic state machine: `Idle → Recording → Transcribing → Polishing → Outputting → Idle`. The `start`/`stop`/`abort` flow:

1. **Capture** — `audio/` records mic audio via `cpal` (16 kHz PCM).
2. **Transcribe** — audio is streamed/uploaded to an `SttProvider`.
3. **Polish** — raw transcript + context (focused app type, dictionary, scene prompt, translation target) goes to an `LlmProvider`, streaming chunks back to the capsule UI.
4. **Output** — final text is typed via `enigo` keyboard simulation or pasted via clipboard (`output/`).

State transitions are driven by atomics (`compare_exchange` guards the Idle→Recording transition) and emitted to the frontend as `pipeline:state` events. On macOS, output requires **Accessibility permission** (`AXIsProcessTrusted`) — without it, synthesized keystrokes are silently dropped, so this is checked explicitly.

### Provider abstraction (the main extension point)

Both STT and LLM are trait-based with a `create_provider(name, ...)` factory. To add a provider, implement the trait and add a match arm.

- **STT** (`src-tauri/src/stt/`): `SttProvider` trait (`connect`/`send_audio`/`recv_transcript`/`disconnect`). Providers: `cloud`, `deepgram`, `assemblyai`, `volcengine` (Doubao realtime ASR), `doubao_audio` (one-shot transcribe+polish), and any Whisper-compatible HTTP endpoint via `whisper_compat` (config centralized in `stt/config.rs`).
- **LLM** (`src-tauri/src/llm/`): `LlmProvider` trait with a single `polish()` method taking a `ChunkCallback` for streaming. Two backends: `cloud` and `openai` (OpenAI-compatible; covers most providers). Prompt construction lives in `llm/prompt.rs`; `AppType` (Email/Chat/Code/Document/Terminal/General) tailors the rewrite — note Terminal output is forced single-line so newlines don't fire as Enter.

### Tauri command layer

`src-tauri/src/commands/` modules (`config`, `stt`, `llm`, `history`, `dictionary`, `misc`) hold `#[tauri::command]` handlers. **All commands must be registered in the `generate_handler!` macro in `src-tauri/src/lib.rs`** (around line 462) or they won't be callable from the frontend. `lib.rs::run()` is the app bootstrap: plugin registration, DB/store init, global shortcut, system tray, single-instance + deep-link wiring.

### Storage

`src-tauri/src/storage/` — `ConfigManager` (key-value via `tauri-plugin-store`, the `AppConfig` struct mirrors `AppConfig` in `src/stores/appStore.ts`), plus SQLite (`rusqlite`, bundled) for `HistoryStore` and `DictionaryStore`. Schema in `src-tauri/migrations/`.

### Frontend (`src/`)

React 19 + Zustand + Tailwind v4. Two entry windows defined in `tauri.conf.json`:
- **main** (`#/...`) — settings, history, onboarding, account, upgrade. Hash-based routing in `src/lib/router.ts`.
- **capsule** (`#capsule`) — the small always-on-top floating recording indicator (`src/components/Capsule/`).

Key wiring:
- `src/lib/tauri.ts` — typed wrappers around `invoke()` for every Rust command. **When you add/change a Rust command, update this file and its callers.**
- `src/hooks/useTauriEvents.ts` — listens to backend events (`pipeline:state`, polish chunks, `hotkey:registration-failed`, etc.).
- `src/stores/appStore.ts` — global config/history/dictionary state; `authStore.ts` — Better Auth session, mirrored to Rust via `set_session_token`.
- i18n: `src/i18n/locales/*.json` (10 UI languages). User-facing strings go through `react-i18next`, not hardcoded.

### Frontend ↔ backend contract

When changing a feature end to end, the typical touch set is: Rust command in `commands/` → register in `lib.rs` → wrapper in `src/lib/tauri.ts` → store/component in `src/`. Config fields must be kept in sync between Rust `AppConfig` and the TS `AppConfig` interface.

## Platform-specific notes

- macOS: keystroke output needs Accessibility permission (handled in `pipeline.rs`); `macos-private-api` is enabled for the transparent capsule window.
- Linux: NVIDIA+Wayland WebKit crash is worked around in `lib.rs::apply_linux_workarounds` (disables DMA-BUF renderer); X11 thread init in `linux_x11.rs`.
- Conventional Commits are required for PR titles/messages (`feat:`, `fix:`, `refactor:`, etc.).
