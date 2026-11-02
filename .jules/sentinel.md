## 2025-05-18 - Shell Command Injection via OS Link Handlers

**Vulnerability:** `open_url` spawned `cmd /C start "" <url>` on Windows when clicking links or images. Untrusted Discord message content and embeds could inject shell commands (e.g., `& calc.exe`).
**Learning:** Passing user-controlled strings into shell command processors like `cmd.exe` or `sh -c` introduces command injection risks, even when using commands like `start`.
**Prevention:** Avoid invoking system shells (`cmd.exe`) to handle URLs or file associations. Use safe, direct OS APIs or dedicated libraries like `webbrowser` along with strict URL scheme validation (`http://` or `https://`).

## 2026-03-06 - Insecure Fallback File Permissions for Auth Tokens on Unix

**Vulnerability:** When the OS keyring/credential manager was absent or failed, `save_token` wrote the user's Discord token to `session.json` using standard `fs::write` / default umask, making the auth token readable by other local users on shared Unix systems.
**Learning:** `std::fs::write` creates files with default permissions (subject to umask, usually 0644 or world-readable). Sensitive files (like auth tokens or private keys) must explicitly specify restrictive file creation permissions (0600) and directory permissions (0700).
**Prevention:** Always use `OpenOptionsExt::mode(0o600)` and `PermissionsExt::from_mode(0o700)` on Unix when storing credentials, session tokens, or sensitive user configurations on disk.
