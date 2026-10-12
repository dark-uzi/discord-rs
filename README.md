# discord-rs

A native desktop Discord client written in Rust with [egui](https://github.com/emilk/egui) / eframe.

> **Disclaimer:** This is an unofficial client that authenticates with a user account
> token ("self-bot"). That is against Discord's Terms of Service and can get an
> account terminated. Use at your own risk, ideally with a throwaway account.
> Not affiliated with or endorsed by Discord Inc.

## Features

- Guild + DM navigation, categories, unread and mention badges
- Message history with grouping, replies, edits, deletes and jump-to-message
- Attachments and image viewing/upload (drag-drop, file picker, clipboard paste)
- Emoji reactions and an emoji picker
- Typing indicators, presence, and member list
- Mentions with highlighting, autocomplete for `@` users and `#` channels
- Custom per-scope nicknames (aliases) for other users
- Search, pins, a quick switcher (`Ctrl+K`), and a message edit/delete log
- In-app notifications (toasts + history) and optional notification sound
- Radial quick-action menu (hold right-click) and a flat context menu
- Optional moderation tools (kick / ban / timeout) behind a setting
- Custom avatars, pins, dark theme, compact mode, performance mode

## Building

Requires a recent stable Rust toolchain (MSVC toolchain on Windows).

```sh
cargo run --release
```

On first launch, paste your Discord **user token** to sign in. The token is stored
in `%APPDATA%\discord-rs\session.json` so you stay signed in across restarts.

Settings and nicknames are stored alongside it (`settings.json`, `aliases.json`,
`pins.json`). Nothing is committed to this repository.

## License

BSD 3-Clause — see [LICENSE](LICENSE). You may reuse and modify this code, but
you must keep the copyright notice, and you may not use the author's name to
endorse or promote derived products.
