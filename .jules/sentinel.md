## 2025-05-18 - Shell Command Injection via OS Link Handlers

**Vulnerability:** `open_url` spawned `cmd /C start "" <url>` on Windows when clicking links or images. Untrusted Discord message content and embeds could inject shell commands (e.g., `& calc.exe`).
**Learning:** Passing user-controlled strings into shell command processors like `cmd.exe` or `sh -c` introduces command injection risks, even when using commands like `start`.
**Prevention:** Avoid invoking system shells (`cmd.exe`) to handle URLs or file associations. Use safe, direct OS APIs or dedicated libraries like `webbrowser` along with strict URL scheme validation (`http://` or `https://`).

## 2025-05-19 - Arbitrary Protocol Execution via egui Hyperlinks

**Vulnerability:** `ui.hyperlink_to` in egui bypasses custom URL opening wrappers (e.g. `open_url`) and passes raw strings directly to system URL handlers when clicked, allowing untrusted Discord content to trigger local file protocols (`file://`) or arbitrary OS URI schemes.
**Learning:** GUI widget libraries with built-in open-link behavior (`ui.hyperlink_to`) can bypass centralized URL scheme validation logic if used directly with untrusted input strings.
**Prevention:** Validate URL schemes (`is_safe_url`) before rendering links in the UI, and route all link clicks through a single validated handler function (`open_url`) using `ui.link().clicked()`.
