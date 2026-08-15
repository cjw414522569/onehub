# Permission rationale (T167)

Every runtime permission requested by the app has a written rationale. The
app requests the minimum set; each is optional and the app works without it.

| Permission | Platform | Rationale | Required? |
| --- | --- | --- | --- |
| Clipboard read | Android / iOS | pasting into the terminal (bracketed paste); never read automatically | optional (paste button works on Android; iOS uses the system paste menu) |
| Notifications | Android / iOS | session reconnect / transfer-complete alerts; user opts in | optional |
| Network | Android | gateway WebSocket + SSH; required for the core function | required (Android normal) |
| Storage (scoped) | Android | export diagnostics / logs / host backup; user picks the file via SAF | optional |
| Background (foreground service) | Android | keep an active session's notification alive; no background SSH promise | optional |

No permission grants terminal content, command history, or identity to any
third party. Telemetry is default-off (T147).