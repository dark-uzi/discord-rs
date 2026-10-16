## 2025-05-18 - Shell Command Injection via OS Link Handlers

**Vulnerability:** `open_url` spawned `cmd /C start "" <url>` on Windows when clicking links or images. Untrusted Discord message content and embeds could inject shell commands (e.g., `& calc.exe`).
**Learning:** Passing user-controlled strings into shell command processors like `cmd.exe` or `sh -c` introduces command injection risks, even when using commands like `start`.
**Prevention:** Avoid invoking system shells (`cmd.exe`) to handle URLs or file associations. Use safe, direct OS APIs or dedicated libraries like `webbrowser` along with strict URL scheme validation (`http://` or `https://`).
