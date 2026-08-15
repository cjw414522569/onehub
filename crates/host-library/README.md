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

## T114: SFTP single-pane / responsive file manager

`crates/host-library/src/file_manager.rs`:

- `FilePane` - single-pane listing with navigation (cd into directories),
  desktop single-select, and mobile multi-select.
- `FileOperationManager` - queues upload / download / move / copy / delete
  with progress (`TransferProgress.percent`), conflict resolution (ask /
  overwrite / skip / rename), and cancel / retry that **reuses the same op
  id** (no duplicate submission).
- Desktop drag-drop maps to a move op; mobile selection maps to multi-file
  ops; progress, conflict handling, and lifecycle are consistent.

## T115: transfer queue, background progress, failure retry, safe notifications

`crates/host-library/src/transfer_queue.rs`:

- `TransferQueue` - manages transfers with background progress, transient /
  permanent failure classification, auto-retry under a configurable
  `RetryPolicy`, and manual retry / cancel that reuse the same entry id (no
  duplicate submission).
- `notification_for` - builds system notifications from the **safe label
  only** (never source/destination paths), so secrets cannot leak (verified
  by a notification-leak test).
- Tests: queue lifecycle/stats, cancel/retry without duplicates, transient
  auto-retry and permanent no-retry, background progress, and notification
  leak safety.

## T116: settings, theme, font, terminal, and network-policy UI

`crates/host-library/src/settings.rs`:

- `Settings` - appearance (theme), font, terminal, and network policy; every
  item declares its `EffectTiming` (immediate / on-reconnect / on-restart) so
  the UI can label exactly when a change applies.
- Persistence: versioned `SettingsSnapshot` with `snapshot` / `from_snapshot`
  (validates ranges: font size 8..72, scrollback 0..1M, keepalive 0..86400).
- `migrate_snapshot` - upgrades older snapshots, filling missing keys with
  defaults.
- `reset_to_defaults` - restores defaults.
- Tests: persistence round-trip, migration, defaults restore, and invalid /
  unknown value handling.

## T117: diagnostic bundle export with redaction

`crates/host-library/src/diagnostics.rs`:

- `DiagnosticExporter::preview` - shows which categories will be included /
  excluded before anything is exported (user confirmation).
- `RedactionPolicy::defaults()` - exports logs / config summary / system info
  only; commands, hosts, usernames, session bodies, and keys are excluded by
  default (categories can be opted in explicitly).
- `Redactor` - scrubs secrets, `user@host` tokens, and private-key blocks
  from the included categories.
- Tests: the canary-secret scan proves the default export contains none of
  the command / host / user / body / key canaries.

## T119: accessibility semantics, screen readers, reduce-motion

`crates/host-library/src/accessibility.rs`:

- `A11yTree` - a semantic tree (roles + accessible names + states) with a
  deterministic focus order and an audit that runs the WCAG 2.2 AA
  critical-path checks that are modelable here (4.1.2 names, 2.4.3 focus
  order, 2.1.1 keyboard).
- `ReduceMotionPolicy` - disables animation / smooth scrolling / cursor
  blink when the OS requests reduced motion (WCAG 2.3.3).
- `TerminalAccessibleMode` - exposes the visible screen as a screen-reader
  text buffer with a cursor announcement.
- `screen_reader_checklist` - automated in-model checks plus the
  VoiceOver / NVDA / TalkBack live-check matrix (run on native hosts).

## T120: command palette and full keyboard navigation

`crates/host-library/src/command_palette.rs`:

- `CommandPalette` - filters commands by title / keywords, navigates results
  (next / prev / wrap), and executes the selected command (closing on
  execute).
- `KeyboardFlow` - drives the palette with keyboard events only (toggle /
  type / backspace / next / prev / enter / escape) and applies the executed
  actions to the session state (connect / switch tab / switch window /
  search / port forward / disconnect).
- The keyboard end-to-end test completes connect, switch, search, forward,
  and disconnect **without a mouse**.

## T121: Windows window / tray / protocol / notification / install model

`crates/host-library/src/windows_integration.rs`:

- `WindowsArch` (x64 / arm64), `DpiContext` (logical <-> physical scaling),
  and `MonitorLayout::constrain_restore` (a window saved on a now-unplugged
  monitor is moved into a visible work area - multi-monitor correctness).
- `TrayAction` (open / new tab / reconnect / quit), `SleepWakePolicy`
  (sessions reconnect on wake), and secret-free `WindowsNotification`.
- `parse_ssh_link` - parses `ssh://user@host:port` (the security policy and
  explicit confirmation are T132; here the shape is parsed).
- The real Win32 tray / message-loop / installer bindings run on Windows
  hosts; this module locks the deterministic model.

## T122: ConPTY / system OpenSSH backend capability gate

`crates/host-library/src/backend_gate.rs`:

- `BackendGate` - the system OpenSSH backend is only enabled when
  **explicitly selected**; the built-in backend is the default and the system
  backend is never enabled implicitly.
- `BackendComparison` - the feature-support matrix (UTF-8, true color,
  bracketed paste, mouse, resize, unicode width, OSC 52 clipboard, bell)
  exposes the behavior differences between the built-in and system backends
  so they are visible to the user.

## T123: macOS menu / window / Keychain / notification / deep-link model

`crates/host-library/src/macos_integration.rs`:

- `MacArch` (Intel / Apple Silicon) and `RetinaScale` (@1x / @2x backing).
- Multi-monitor restore reuses the shared geometry (`MonitorLayout`).
- `AppNapPolicy` - App Nap is disabled during active sessions.
- `MacMenu` (default app menu) and secret-free `MacNotification`.
- Deep links reuse `parse_ssh_link`; real macOS automation and the
  physical-machine checklist run on macOS hosts.

## T124: macOS sandbox, hardened runtime, minimal entitlements

`crates/host-library/src/macos_entitlements.rs`:

- `EntitlementSet::minimal` - the baseline (sandbox, network client,
  user-selected files); on-demand entitlements (network server for
  forwarding, keychain access group) can be added explicitly.
- `NotarizationAudit` - the pre-notarization checks: hardened runtime,
  sandbox, no `get-task-allow` in release, and no extra entitlements.
- Real `codesign` / `spctl` / entitlement audits run on macOS hosts.