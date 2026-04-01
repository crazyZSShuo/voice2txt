# Windows Speech Backend Design

Date: 2026-04-01
Project: `voice-input`

## Goal

Add a Windows-native speech recognition backend to the existing tray-based voice input app while preserving the current user interaction model:

- Hold Right Alt to record
- Release to transcribe
- Optionally refine via LLM
- Inject the final text into the focused window

The new backend must become the default on Windows builds. The existing OpenAI-compatible HTTP STT backend remains available as a manual fallback option in settings.

## Decisions

### Confirmed Product Decisions

- Windows builds default to `Windows SpeechRecognizer`
- Non-Windows builds default to `Custom STT`
- `Custom STT` remains supported
- Backend switching is manual only
- No automatic fallback from `Windows SpeechRecognizer` to `Custom STT`
- Windows speech recognition language is controlled by the Windows system, not by the app
- The existing `language` setting remains in the app for `Custom STT` only

### Non-Goals

- No attempt to automate or directly reuse the internal `Win+H` voice typing UI
- No removal of the current OpenAI-compatible STT path
- No new provider abstraction layer beyond what is needed for clean dispatch
- No redesign of the capsule, tray flow, hotkey logic, or injection flow

## Current State

Today the app has one STT path:

- Audio recording ends in `src-tauri/src/main.rs`
- The app sends WAV audio to `src-tauri/src/stt.rs`
- `stt.rs` calls an OpenAI-compatible `/v1/audio/transcriptions` endpoint
- Returned text then flows into optional LLM refinement and the existing injection path

The settings UI only supports this HTTP-based STT mode.

## Recommended Approach

Use a separate Windows-specific backend module and keep the existing HTTP STT module mostly unchanged.

### Why This Approach

- It preserves the current `Custom STT` implementation with minimal risk
- It keeps Windows-native recognition code isolated from HTTP-specific logic
- It makes manual backend switching straightforward
- It reduces regression risk in the current recording, refinement, and injection flow

## Architecture

### Modules

- Keep `src-tauri/src/stt.rs` for the existing OpenAI-compatible HTTP backend
- Add `src-tauri/src/windows_stt.rs` for the Windows-native backend
- Update `src-tauri/src/main.rs` to dispatch to the selected backend after recording stops
- Update `src-tauri/src/config.rs` to store the backend selection

### Backend Selection

Introduce an explicit backend enum in config:

- `windows_speech`
- `custom`

This backend value becomes part of persisted app configuration.

### Dispatch Model

After audio capture completes, the backend selection determines the STT path:

- `windows_speech` -> call `windows_stt.rs`
- `custom` -> call `stt.rs`

Both paths must return a final transcript string to the existing post-processing pipeline. From that point onward:

- optional LLM refinement remains unchanged
- capsule events remain compatible with the current UI
- injection remains unchanged

## Configuration Design

### Config Shape

`AppConfig` gains a new field:

- `stt_backend`

Existing fields remain:

- `language`
- `auto_start`
- `stt`
- `llm`

The existing `stt` object remains intact even when the selected backend is `windows_speech`, so a user can switch back to `custom` without losing saved endpoint settings.

### Default Values

- On Windows: `stt_backend = windows_speech`
- On non-Windows: `stt_backend = custom`

The existing `language` default remains unchanged.

### Backward Compatibility

Old config files will not contain `stt_backend`. The loader must deserialize them safely and supply the platform-appropriate default.

## Settings UI Design

### Backend Picker

Add a `Backend` selector near the top of the STT section with two visible options:

- `Windows SpeechRecognizer`
- `Custom STT`

### When `Windows SpeechRecognizer` Is Selected

- Hide `API Base URL`
- Hide `API Key`
- Hide `Model`
- Show a read-only note that recognition language is controlled by Windows
- Keep the STT test button, but make it test Windows speech recognition initialization instead of HTTP connectivity

### When `Custom STT` Is Selected

- Show the current HTTP STT fields
- Keep the current STT test behavior

### Language Setting

Keep the current app language setting in the UI, but label it clearly as applying only to `Custom STT`.

This avoids removing an existing feature while still making the Windows-native behavior explicit.

## Backend Behavior

### Windows Backend

`windows_stt.rs` is responsible for:

- initializing the Windows speech recognizer
- converting the recorded audio into the format expected by the recognizer, if needed
- running recognition against the captured recording
- returning the recognized text as a Rust `String`
- surfacing clear errors when Windows speech recognition cannot be initialized or used

The app does not pass the current app `language` setting into the Windows backend.

### Custom HTTP Backend

`stt.rs` continues to:

- send recorded WAV audio to the configured `/v1/audio/transcriptions` endpoint
- use the configured API key and model
- pass the app language setting
- return recognized text

Its existing behavior should remain unchanged unless a small adjustment is needed to fit the new dispatch entry point.

## Runtime Flow

The overall runtime behavior remains:

1. User holds Right Alt
2. Recording begins
3. User releases Right Alt
4. Audio capture stops
5. App selects STT backend based on `stt_backend`
6. App obtains transcript
7. Optional LLM refinement runs
8. Final text is injected into the focused window
9. Capsule UI completes its existing success or error flow

The user-facing change is only which STT backend is used.

## Error Handling

### Windows Backend Failures

If `windows_speech` is selected and Windows speech recognition fails:

- show the existing error path in the capsule
- return an explicit error message
- do not automatically fall back to `custom`

### Custom Backend Failures

Keep the current HTTP error behavior.

### Non-Windows + Windows Backend Selected

If a non-Windows build loads a config that selects `windows_speech`, the app must return a clear, explicit error indicating that the backend is unavailable on this platform.

It must not silently downgrade to `custom`.

## Testing Strategy

### Rust Tests

Add tests for:

- config default backend selection on Windows vs non-Windows
- config serialization and deserialization with `stt_backend`
- backward-compatible config loading when `stt_backend` is missing
- backend dispatch routing to the correct implementation
- explicit error behavior for `windows_speech` on non-Windows builds

### Frontend Verification

The project does not currently have a frontend test framework. For this change, frontend verification is done through:

- TypeScript compilation
- production build success
- inspection of conditional settings rendering during manual verification if needed

### Required Verification Commands

Minimum verification before claiming completion:

- `npm run build`
- `cargo test` from `src-tauri`

If the Windows target build remains runnable in this environment, also verify the relevant Windows build path after implementation.

## Implementation Notes

### Scope Discipline

This work should not refactor unrelated areas. In particular:

- do not redesign the tray menu
- do not alter Right Alt hook behavior
- do not change injection strategy
- do not change LLM behavior except where the STT entry point needs to remain compatible

### Migration Risk

The largest implementation risk is the Windows-native recognition integration itself. To keep risk controlled:

- add the backend as a new module
- keep dispatch explicit
- avoid broad refactors to the current HTTP STT path

## Acceptance Criteria

The design is successful when all of the following are true:

- Windows builds default to `Windows SpeechRecognizer`
- Non-Windows builds default to `Custom STT`
- Users can manually switch between backends in settings
- `Windows SpeechRecognizer` does not auto-fallback to `Custom STT`
- `Custom STT` credentials remain saved when the Windows backend is selected
- Existing LLM refinement and injection behavior still work after transcription
- Non-Windows builds show an explicit error if `windows_speech` is selected
- The project builds and the relevant tests pass
