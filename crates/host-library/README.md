# host-library

- Layer: L1 (core services)
- Dependencies: `core-domain`
- Scope: host library model (list / group / tags / search / sort).

## T102: host library list, group, tags, search, and sort

`crates/host-library/src/host.rs`:

- `HostRecord` - id, name, host, port, group, sorted tags, last-used.
- `HostLibrary` - add/insert/remove/get, case-insensitive `search` across
  name/host/tags/group, `filter_by_tag`, `groups` / `tags` summaries, and
  deterministic `sorted` (name / host / group / last-used, asc/desc).
- `SelectionModel` - stable index-based navigation (next/prev/first/last with
  clamping) so keyboard and touch operate the list identically.
- `view` - a deterministic text view (golden-testable).
- 10k-host performance test: search / filter / sort stay well under an
  interactive budget, and selection navigates the full list.

## T103: host editor with inline validation

`crates/host-library/src/editor.rs`:

- `HostEditorForm` - five reviewable sections (basic / auth / proxy /
  terminal / advanced), each with labeled fields in focus order.
- Inline validation: every field revalidates on `set` (including cross-field
  rules such as auth-method `key` requiring a key path, and proxy-enabled
  requiring host/port); the form is valid only when all fields pass.
- `review()` - an auditable view that masks secrets (`PASSWORD_MASK`) so the
  configuration can be reviewed before saving.
- `accessibility()` - every field is labeled, focus order is stable, and
  every error message is non-empty (screen-reader friendly).

## T104: first-connection host fingerprint review

`crates/host-library/src/fingerprint.rs`:

- `HostKeyFingerprint` - deterministic SHA-256 fingerprint of raw key bytes
  (sha2), colon-grouped display, key algorithm, and source.
- `FingerprintReview` - classifies a presented fingerprint against a known
  one: new host (medium risk), matching known (algorithm risk), changed key
  (high risk with a change notice). Approve / reject drive the review state.
- `ReviewView` - the UI shows algorithm, SHA-256 fingerprint, source, risk,
  and an explicit change notice for a key change.

## T105: authentication prompts, key selection, hardware confirmation

`crates/host-library/src/auth_prompt.rs`:

- `AuthPrompt` - password / passphrase / key-selection / hardware-confirmation
  prompts. Sensitive input is transient: `submit` moves the value out and
  clears the model (never cached), and `cancel` clears it immediately and
  terminates authentication (a later submit is refused).
- `KeySelection` - select a key by id, confirm, or cancel (unknown ids are
  rejected).
- `HardwareConfirmation` - confirm / cancel a hardware touch (YubiKey /
  Windows Hello).
- The authentication interaction matrix test covers submit / cancel /
  cancel-then-submit for every prompt kind.

## T106: desktop window, multi-tab, split, and focus model

`crates/host-library/src/workspace.rs`:

- `Workspace` - multiple `WindowModel`s, each with tabs and splittable panes;
  a single global focus location (window -> tab -> pane) stays consistent.
- Drag-drop: `move_tab` moves a tab between windows (focus follows); tabs can
  also be moved with the `MoveTabToNextWindow` shortcut.
- Shortcuts: `ShortcutMap` resolves Ctrl+T / Ctrl+W / Ctrl+Tab /
  Ctrl+Shift+[ ] / split / focus-ring chords to the same operations.
- Focus ring: `focus_next` / `focus_prev` cycle panes, tabs, and windows.
- Restore: `snapshot` / `restore` serialize the whole multi-window layout to a
  versioned snapshot (validated on restore; invalid indices rejected).

## T107: mobile session stack, bottom action bar, safe-area adaptation

`crates/host-library/src/mobile.rs`:

- `Viewport` - derives `FormFactor` (phone/tablet via the smaller dimension)
  and `Orientation`; one-handed compatibility (phone portrait).
- `effective_safe_area` - applies system safe-area insets (landscape phones
  gain side insets for a display cutout; tablets keep system values).
- `BottomActionBar` - deterministic `layout` metrics per form factor
  (golden-tested: phone portrait 3 actions / tablet & landscape expanded 5
  actions, 48/56px).
- `SessionStack` - `on_system_back` pops history, otherwise asks the app to
  exit (Android/iOS system-back contract).

## T108: physical keyboard, IME, modifiers, configurable shortcuts

`crates/host-library/src/keyboard.rs`:

- `KeyMap` / `parse_key` - normalize platform key names into a neutral
  `KeyCode` (letters/digits normalized), so Windows / macOS / Linux /
  Android / iOS share one key semantic.
- `PlatformSemantics` - the primary shortcut modifier is explicit (Ctrl
  everywhere, Cmd on macOS) with readable labels.
- IME: `KeyEvent.composing` + text; shortcut chords are suppressed during
  composition.
- `KeyBindingConfig` - maps chords to `KeyAction`s; user-remappable
  (`set_binding` / `clear_binding`), with `chord_label` per platform.
- Tests: the keyboard event matrix is platform-consistent (Ctrl+T vs Cmd+T),
  IME suppresses shortcuts, and rebinding works.

## T109: mobile terminal extended keyboard and gestures

`crates/host-library/src/gestures.rs`:

- `GestureRecognizer` - deterministically disambiguates tap / long-press /
  scroll (long press starts a selection; a drag past the threshold scrolls in
  normal mode and extends the selection in selection mode; a tap ends the
  selection).
- `ExtendedKeyboard` - Ctrl / Alt / Esc / Tab / arrow keys emit chords
  independently of the touch canvas, so extended keys never conflict with
  scroll or selection.
- Tests: tap/long-press/scroll disambiguation, selection drags never scroll,
  extended keys emit chords mid-scroll and mid-selection without disturbing
  the gesture state.

## T110: secure paste confirmation, multi-line warning, bracketed paste

`crates/host-library/src/paste.rs`:

- `PasteContent::analyze` - detects newlines, control characters, suspicious
  shell fragments, and size.
- `SecurePasteFlow` - applies a configurable `PastePolicy` and returns
  Allow / Confirm / Block. A potential command injection is **previewable**
  (control chars escaped, truncated with byte count) before pasting.
- Password pasting has its own configurable policy (Allow / Confirm / Block).
- Bracketed paste: `bracketed_payload` wraps the text in `ESC[200~ ... ESC[201~`.
- Tests: newline / control-character / huge-clipboard cases.

## T111: session state, latency, reconnect, read-only indicators

`crates/host-library/src/session_status.rs`:

- `SessionStatusModel` - a validated state machine over `SessionState`
  (disconnected / connecting / connected / reconnecting / read-only / error /
  closed), with reconnect attempts and error messages.
- `StateIndicator` - every state is recognized by a **non-color** indicator:
  a glyph, a label, a description, and a visual pattern (solid / dashed /
  hatched / animated / hollow / blinking); the exhaustive test asserts all
  seven states have unique (glyph, pattern) pairs.
- Latency is shown as text ("12 ms", "1.5 s") with a quality label, never by
  color alone.

## T112: command snippets, variable hints, sensitive injection

`crates/host-library/src/snippets.rs`:

- `SnippetTemplate` - a command with `{{variable}}` placeholders; variables
  are text or secret.
- `SnippetEngine::render` - substitutes values in a **single pass** (a value
  containing `{{...}}` is inserted literally - no template injection) and
  produces a preview with secret values masked.
- `CommandHistory` - records only the masked preview, so sensitive values
  never enter history (verified by a leak test).
- `VariableHints` - prefix-based autocomplete candidates.
- Tests: template injection, history leak, missing-variable validation, and
  hint resolution.

## T113: port forwarding management and occupancy diagnosis

`crates/host-library/src/port_forwarding.rs`:

- `ForwardManager` - create local / remote / dynamic forwards (invalid or
  occupied listen ports fail with an actionable message via the occupancy
  diagnostic), pause / resume / reconnect / confirm / remove.
- `PortForward` - copy-ready `address_label` (`127.0.0.1:2222 -> 10.0.0.5:22`)
  and risk warnings (all-interfaces listen = high, privileged port = medium).
- `diagnose` - free / occupied status for the port-occupancy diagnosis UI.
- Tests cover create, pause/resume/reconnect, occupied-port failure, address
  copy, and risk warnings.