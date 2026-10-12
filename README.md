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

## Known Issues / Bugs

- **Typing indicators don't appear.** The gateway events are requested
  (`GUILD_MESSAGE_TYPING` / `DIRECT_MESSAGE_TYPING` + `guild_subscriptions`);
  still being diagnosed.
- **Member list is often empty.** Discord returns `403` for
  `/guilds/{id}/members` on user tokens, so the list only shows what the gateway
  already sent. The UI shows "unavailable" when that happens.
- **No voice/video.** Joining voice channels is not implemented (see below).
- The client is **Windows-first** — clipboard image paste uses Windows APIs
  (`clipboard-win`), so it may not work the same on other platforms.
- Embedding/preview fidelity is partial: some embed types and link previews
  render as plain text.
- Not every Discord event is handled; the UI can lag behind other clients for
  rare events (e.g. some permission/role changes).

## Not Yet Implemented / Roadmap

- **Voice & video** — blocked: real voice needs Discord's DAVE end-to-end
  encryption (MLS / RFC 9420, `libdave`), which isn't implemented.
- **Threads** — viewing, creating, and replying in threads.
- **Slash / application commands** and interactions.
- **Server-wide message search** (only the currently loaded channel is searched).
- **Native OS notifications** (currently in-app toasts + an in-app history).
- **Server & channel management** — roles, permissions, channels, categories.
- **Friend requests** — accept/deny/remove from the UI (currently view-only).
- **Group DMs** — creation and management.
- **GIF/sticker picker** (Tenor/Giphy) and scheduled events.
- **Server onboarding / join-by-invite** flow.
- Configurable per-channel notification muting.

## License

BSD 3-Clause — see [LICENSE](LICENSE). You may reuse and modify this code, but
you must keep the copyright notice, and you may not use the author's name to
endorse or promote derived products.
