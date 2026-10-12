use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, NaiveDate};
use eframe::egui::{self, RichText};
use tokio::sync::mpsc::UnboundedSender;

use crate::config;
use crate::model::{Attachment, Embed, Emoji, Member, Message, User};
use crate::state::{Command, ConnectionStatus, LogEntry, LogKind, PendingFile, SharedState, DM_HOME};

struct ReplyTarget {
    channel_id: String,
    message_id: String,
    author: String,
    preview: String,
}

struct EditTarget {
    channel_id: String,
    message_id: String,
}

enum MsgAction {
    Reply {
        channel_id: String,
        message_id: String,
        author: String,
        preview: String,
    },
    Edit {
        channel_id: String,
        message_id: String,
        content: String,
    },
    Delete {
        channel_id: String,
        message_id: String,
    },
    OpenProfile {
        user_id: String,
    },
    SetNickname {
        scope: String,
        user_id: String,
        display: String,
        current: String,
    },
    OpenImage {
        url: String,
    },
    JumpTo {
        channel_id: String,
        message_id: String,
    },
    ToggleReaction {
        channel_id: String,
        message_id: String,
        emoji: Emoji,
    },
    PickReaction {
        channel_id: String,
        message_id: String,
    },
    OpenDm {
        user_id: String,
    },
    Moderate {
        guild_id: String,
        user_id: String,
        user_name: String,
        action: ModAction,
    },
}

#[derive(Clone)]
enum EmojiTarget {
    Composer,
    Reaction {
        channel_id: String,
        message_id: String,
    },
}

struct EmojiPicker {
    target: EmojiTarget,
    search: String,
    focused: bool,
}

pub struct App {
    shared: Arc<Mutex<SharedState>>,
    cmd_tx: UnboundedSender<Command>,
    token_input: String,
    draft: String,
    auto_selected: bool,
    auto_channels_requested: bool,
    replying_to: Option<ReplyTarget>,
    editing: Option<EditTarget>,
    focus_input: bool,
    settings: config::Settings,
    settings_open: bool,
    profile_open: Option<ProfileView>,
    copied_at: Option<Instant>,
    nickname_edit: Option<NicknameEdit>,
    image_view: Option<String>,
    image_zoom: f32,
    image_pan: egui::Vec2,
    mod_dialog: Option<ModDialog>,
    typing_sent: Option<Instant>,
    jump_bottom: bool,
    emoji_picker: Option<EmojiPicker>,
    search: String,
    pins_open: bool,
    log_open: bool,
    friends_open: bool,
    render: RenderState,
    member_search: String,
    switcher_open: bool,
    switcher_query: String,
    switcher_focus: bool,
    search_focus: bool,
anchor_channel: Option<String>,
    scroll_override: Option<f32>,
    jump_to: Option<JumpTarget>,
    visuals_dark: Option<bool>,
    last_title: String,
    toasts: Vec<Toast>,
    mem_last_channel: Option<String>,
    pending_files: Vec<PendingFile>,
    notifications_open: bool,
    unseen_notifications: bool,
}

struct Toast {
    title: String,
    body: String,
    mention: bool,
    error: bool,
    channel_id: Option<String>,
    born: Instant,
}

struct ProfileView {
    user_id: String,
    guild_id: Option<String>,
    requested: bool,
}

/// Target for "jump to message" (from pinned messages): select the channel and
/// scroll until the message is loaded and visible.
#[derive(Clone)]
struct JumpTarget {
    channel_id: String,
    message_id: String,
    /// Oldest message id we already asked the gateway to fetch older than.
    tried: Option<String>,
}

struct NicknameEdit {
    scope: String,
    user_id: String,
    display: String,
    input: String,
    focused: bool,
}

impl App {
    pub fn new(shared: Arc<Mutex<SharedState>>, cmd_tx: UnboundedSender<Command>) -> Self {
        Self {
            shared,
            cmd_tx,
            token_input: String::new(),
            draft: String::new(),
            auto_selected: false,
            auto_channels_requested: false,
            replying_to: None,
            editing: None,
            focus_input: false,
            settings: config::load_settings(),
            settings_open: false,
            profile_open: None,
            copied_at: None,
            nickname_edit: None,
            image_view: None,
            image_zoom: 1.0,
            image_pan: egui::Vec2::ZERO,
            mod_dialog: None,
            typing_sent: None,
            jump_bottom: false,
            emoji_picker: None,
            search: String::new(),
            pins_open: false,
            log_open: false,
            friends_open: false,
            render: RenderState::default(),
            member_search: String::new(),
            switcher_open: false,
            switcher_query: String::new(),
            switcher_focus: false,
            search_focus: false,
            anchor_channel: None,
            scroll_override: None,
            jump_to: None,
            visuals_dark: None,
            last_title: String::new(),
            toasts: Vec::new(),
            mem_last_channel: None,
            pending_files: Vec::new(),
            notifications_open: false,
            unseen_notifications: false,
        }
    }

    fn open_profile(&mut self, user_id: String, guild_id: Option<String>) {
        self.profile_open = Some(ProfileView {
            user_id,
            guild_id,
            requested: false,
        });
    }

    /// Candidate users for the `@` autocomplete: current guild's members,
    /// friends, then any other known users.
    fn autocomplete_users(&self) -> Vec<(String, String)> {
        let s = self.shared.lock().unwrap();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut out: Vec<(String, String)> = Vec::new();
        if let Some(gid) = s.selected_guild.as_ref() {
            if let Some(g) = s.guilds.iter().find(|g| &g.id == gid) {
                for m in &g.members {
                    if seen.insert(m.user.id.clone()) {
                        out.push((m.user.id.clone(), m.display_name()));
                    }
                }
            }
        }
        for rel in &s.friends {
            if seen.insert(rel.user.id.clone()) {
                out.push((rel.user.id.clone(), rel.user.display_name()));
            }
        }
        for (id, u) in &s.users {
            if seen.insert(id.clone()) {
                out.push((id.clone(), u.display_name()));
            }
        }
        out.sort_by(|a, b| {
            let pa = presence_rank(s.presence_of(&a.0));
            let pb = presence_rank(s.presence_of(&b.0));
            pa.cmp(&pb)
                .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
        });
        out
    }

    /// Candidate text channels for the `#` autocomplete in the current guild.
    fn autocomplete_channels(&self) -> Vec<(String, String)> {
        let s = self.shared.lock().unwrap();
        let Some(gid) = s.selected_guild.as_ref() else {
            return Vec::new();
        };
        if gid == DM_HOME {
            return Vec::new();
        }
        let mut chans: Vec<(String, String)> = s
            .text_channels(gid)
            .into_iter()
            .map(|c| {
                let name = c.display_name();
                (c.id, name)
            })
            .collect();
        chans.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
        chans
    }

    /// Read files from disk into the pending-upload list.
    fn stage_paths(&mut self, paths: Vec<std::path::PathBuf>) {
        for path in paths {
            match std::fs::read(&path) {
                Ok(data) => {
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("file")
                        .to_string();
                    self.pending_files.push(PendingFile::from_bytes(name, data));
                }
                Err(e) => eprintln!("attach: failed to read {}: {e:#}", path.display()),
            }
        }
    }

    /// If the clipboard holds an image, stage it as a PNG attachment.
    /// Returns `true` when something was staged.
    fn stage_clipboard_image(&mut self) -> bool {
        // 1) Bitmap via arboard (reads CF_DIBV5).
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            if let Ok(image) = clipboard.get_image() {
                if let Some(png) = encode_png(image.width, image.height, &image.bytes) {
                    self.pending_files
                        .push(PendingFile::from_bytes("clipboard.png".to_string(), png));
                    return true;
                }
            }
            // 2) Image files copied in Explorer (CF_HDROP).
            if let Ok(files) = clipboard.get().file_list() {
                let imgs: Vec<std::path::PathBuf> = files
                    .into_iter()
                    .filter(|p| is_image_path(&p.to_string_lossy()))
                    .collect();
                if !imgs.is_empty() {
                    self.stage_paths(imgs);
                    return true;
                }
            }
        }
        // 3) Classic DIB (CF_DIB) / DIBV5 via the Windows clipboard, covering
        //    apps that publish only a raw DIB rather than via arboard.
        #[cfg(target_os = "windows")]
        {
            if let Ok(_clip) = clipboard_win::Clipboard::new_attempts(5) {
                for fmt in [
                    clipboard_win::formats::CF_DIBV5,
                    clipboard_win::formats::CF_DIB,
                ] {
                    if clipboard_win::is_format_avail(fmt) {
                        let mut data = Vec::new();
                        if clipboard_win::raw::get_vec(fmt, &mut data).is_ok() {
                            if let Some((w, h, rgba)) = dib_to_rgba(&data) {
                                if let Some(png) = encode_png(w, h, &rgba) {
                                    self.pending_files.push(PendingFile::from_bytes(
                                        "clipboard.png".to_string(),
                                        png,
                                    ));
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }
        // Only complain when the clipboard had no text either (i.e. Ctrl+V was
        // not a normal text paste handled by the composer's TextEdit).
        let has_text = arboard::Clipboard::new()
            .and_then(|mut c| c.get().text())
            .map(|t| !t.is_empty())
            .unwrap_or(false);
        if !has_text {
            self.shared
                .lock()
                .unwrap()
                .push_error("No image found in the clipboard.");
        }
        false
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if dropped.is_empty() {
            return;
        }
        if self.shared.lock().unwrap().selected_channel.is_none() {
            return;
        }
        for f in dropped {
            let path = f.path().to_path_buf();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string();
            let data = if path.as_os_str().is_empty() {
                f.bytes().ok()
            } else {
                match std::fs::read(&path) {
                    Ok(d) => Some(d),
                    Err(_) => f.bytes().ok(),
                }
            };
            if let Some(data) = data {
                self.pending_files
                    .push(PendingFile::from_bytes(name, data));
            }
        }
    }

    fn select_channel(&mut self, channel_id: &str) {
        let mut fetch_channels: Option<String> = None;
        let mut resolve: Option<String> = None;
        {
            let mut s = self.shared.lock().unwrap();
            let guild = s.channels.get(channel_id).map(|c| {
                if c.guild_id.is_none() && c.is_dm() {
                    DM_HOME.to_string()
                } else {
                    c.guild_id.clone().unwrap_or_else(|| DM_HOME.to_string())
                }
            });
            match guild {
                Some(g) => {
                    let changed = s.selected_guild.as_deref() != Some(g.as_str());
                    s.selected_guild = Some(g.clone());
                    if changed && g != DM_HOME && !s.has_channels(&g) {
                        fetch_channels = Some(g);
                    }
                }
                None => {
                    // Channel not cached: fetch it to learn its guild so the
                    // sidebar switches to the right server.
                    resolve = Some(channel_id.to_string());
                }
            }
            s.selected_channel = Some(channel_id.to_string());
            s.clear_unread(channel_id);
        }
        if let Some(g) = fetch_channels {
            let _ = self.cmd_tx.send(Command::FetchGuildChannels(g));
        }
        if let Some(cid) = resolve {
            let _ = self.cmd_tx.send(Command::OpenChannel(cid));
        }
        let _ = self.cmd_tx.send(Command::AckChannel(channel_id.to_string()));
        let _ = self.cmd_tx.send(Command::FetchMessages(channel_id.to_string()));
    }

    /// When the selected channel changes, drop the outgoing channels' stored
    /// history and release their decoded image bytes/textures. This keeps RAM
    /// from growing with every chat (and picture) the user has visited.
    fn reap_channel_memory(&mut self, ctx: &egui::Context) {
        let current = self.shared.lock().unwrap().selected_channel.clone();
        if current == self.mem_last_channel {
            return;
        }
        self.mem_last_channel = current.clone();
        let uris = {
            let mut s = self.shared.lock().unwrap();
            s.drop_history_except(current.as_deref())
        };
        let keep = self.image_view.clone();
        for u in uris {
            if Some(&u) == keep.as_ref() {
                continue;
            }
            ctx.forget_image(&u);
        }
    }

    /// Drain notifications queued by the gateway, showing in-app toasts and
    /// playing a sound. Runs every frame regardless of the selected server.
    fn pump_notices(&mut self, ctx: &egui::Context) {
        let (notices, errors) = {
            let mut s = self.shared.lock().unwrap();
            (s.take_notices(), s.take_errors())
        };
        let mut added = false;
        if self.settings.notifications && !notices.is_empty() {
            for n in notices {
                let title = if n.mention {
                    format!("{} mentioned you in {}", n.author, n.channel_name)
                } else {
                    format!("{} \u{2014} {}", n.author, n.channel_name)
                };
                self.toasts.push(Toast {
                    title,
                    body: n.body,
                    mention: n.mention,
                    error: false,
                    channel_id: Some(n.channel_id),
                    born: Instant::now(),
                });
                added = true;
            }
            if !self.notifications_open {
                self.unseen_notifications = true;
            }
            if self.settings.notification_sound {
                play_notification_sound();
            }
        }
        for e in errors {
            self.toasts.push(Toast {
                title: "Error".to_string(),
                body: e,
                mention: false,
                error: true,
                channel_id: None,
                born: Instant::now(),
            });
            added = true;
        }
        if !added {
            return;
        }
        if self.toasts.len() > 4 {
            let drop = self.toasts.len() - 4;
            self.toasts.drain(0..drop);
        }
        ctx.request_repaint();
    }

    fn render_toasts(&mut self, ctx: &egui::Context) {
        if self.toasts.is_empty() {
            return;
        }
        let now = Instant::now();
        self.toasts
            .retain(|t| now.duration_since(t.born) < Duration::from_secs(6));
        if self.toasts.is_empty() {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(200));
        let mut dismiss: Option<usize> = None;
        let mut jumped: Option<String> = None;
        egui::Area::new(egui::Id::new("notifications"))
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-16.0, 48.0))
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_max_width(320.0);
                for (i, t) in self.toasts.iter().enumerate() {
                    let frame = if t.error {
                        egui::Frame::window(ui.style())
                            .fill(egui::Color32::from_rgb(58, 22, 24))
                            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(200, 70, 70)))
                            .inner_margin(egui::Margin::symmetric(10, 8))
                    } else {
                        egui::Frame::window(ui.style())
                            .inner_margin(egui::Margin::symmetric(10, 8))
                    };
                    let resp = frame
                        .show(ui, |ui| {
                            ui.set_max_width(300.0);
                            if t.error {
                                ui.label(
                                    RichText::new(&t.title)
                                        .strong()
                                        .size(13.0)
                                        .color(egui::Color32::from_rgb(255, 150, 150)),
                                );
                            } else {
                                let color = if t.mention {
                                    egui::Color32::from_rgb(150, 170, 255)
                                } else {
                                    ui.visuals().strong_text_color()
                                };
                                ui.label(RichText::new(&t.title).strong().size(13.0).color(color));
                            }
                            ui.label(RichText::new(&t.body).size(12.0));
                        })
                        .response;
                    let r = resp
                        .interact(egui::Sense::click())
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if r.clicked() {
                        if let Some(cid) = &t.channel_id {
                            jumped = Some(cid.clone());
                        }
                        dismiss = Some(i);
                    }
                    ui.add_space(6.0);
                }
            });
        if let Some(channel_id) = jumped {
            self.select_channel(&channel_id);
        }
        if let Some(i) = dismiss {
            if i < self.toasts.len() {
                self.toasts.remove(i);
            }
        }
    }

    fn notifications_window(&mut self, ui: &mut egui::Ui) {
        if !self.notifications_open {
            return;
        }
        let mut open = true;
        let mut jump: Option<String> = None;
        egui::Window::new("Notifications")
            .open(&mut open)
            .resizable(true)
            .default_width(380.0)
            .default_height(440.0)
            .show(ui.ctx(), |ui| {
                let items: Vec<(String, String, String, bool)> = {
                    let s = self.shared.lock().unwrap();
                    s.notice_log()
                        .iter()
                        .map(|n| {
                            (
                                n.author.clone(),
                                n.channel_name.clone(),
                                n.body.clone(),
                                n.mention,
                            )
                        })
                        .collect()
                };
                let ids: Vec<String> = {
                    let s = self.shared.lock().unwrap();
                    s.notice_log().iter().map(|n| n.channel_id.clone()).collect()
                };
                if items.is_empty() {
                    ui.weak("No notifications yet.");
                    return;
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for i in (0..items.len()).rev() {
                            let (author, channel, body, mention) = &items[i];
                            let resp = ui
                                .group(|ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        let color = if *mention {
                                            egui::Color32::from_rgb(240, 120, 120)
                                        } else {
                                            ui.visuals().strong_text_color()
                                        };
                                        ui.label(RichText::new(author).strong().color(color));
                                        ui.weak(channel);
                                    });
                                    ui.label(RichText::new(body).size(12.0));
                                })
                                .response
                                .interact(egui::Sense::click())
                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                            if resp.clicked() {
                                jump = Some(ids[i].clone());
                            }
                            ui.add_space(4.0);
                        }
                    });
            });
        if let Some(cid) = jump {
            self.notifications_open = false;
            self.select_channel(&cid);
        }
        self.notifications_open = open;
    }

    fn status(&self) -> ConnectionStatus {
        self.shared.lock().unwrap().status.clone()
    }

    fn ui_login(&mut self, ui: &mut egui::Ui) {
        let status = self.status();
        let error = self.shared.lock().unwrap().last_error.clone();

        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(60.0);
            ui.vertical_centered(|ui| {
                ui.heading("Discord");
                ui.label("Native client - sign in with your account token");
                ui.add_space(16.0);
            });

            ui.horizontal(|ui| {
                ui.label("Token:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.token_input)
                        .password(true)
                        .desired_width(f32::INFINITY)
                        .hint_text("Paste your token here"),
                );
            });
            ui.add_space(8.0);

            let connecting = matches!(status, ConnectionStatus::Connecting);
            ui.horizontal(|ui| {
                let can_connect = !connecting && !self.token_input.trim().is_empty();
                if ui
                    .add_enabled(can_connect, egui::Button::new("Connect"))
                    .clicked()
                {
                    let token = self.token_input.trim().to_string();
                    let _ = self.cmd_tx.send(Command::Connect(token));
                }
                if connecting {
                    ui.spinner();
                    ui.label("Connecting...");
                }
            });

            if let Some(err) = error {
                ui.add_space(8.0);
                ui.colored_label(egui::Color32::from_rgb(240, 110, 110), err);
            }

            ui.add_space(24.0);
            ui.separator();
            ui.label(
                "This client authenticates as a user account (not a bot). \
                 Your token is saved locally so you stay logged in; click Log out to forget it.",
            );
        });
    }

    fn ui_chat(&mut self, ui: &mut egui::Ui) {
        self.maybe_auto_select();

        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::K)) {
            self.switcher_open = true;
            self.switcher_query.clear();
            self.switcher_focus = true;
        }
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) {
            self.search_focus = true;
        }

        self.top_bar(ui);
        self.guild_rail(ui);
        self.channel_list(ui);
        self.pins_panel(ui);
        if !self.pins_open {
            self.member_list(ui);
        }
        self.input_bar(ui);
        self.message_view(ui);
        self.profile_window(ui);
        self.settings_window(ui);
        self.nickname_window(ui);
        self.emoji_picker_window(ui);
        self.switcher_window(ui);
        self.image_viewer(ui);
        self.log_window(ui);
        self.notifications_window(ui);
        self.moderation_window(ui);
    }

    fn moderation_window(&mut self, ui: &mut egui::Ui) {
        let Some(mut dlg) = self.mod_dialog.take() else {
            return;
        };
        let mut close = false;
        let mut confirm = false;
        let (title, needs_minutes) = match dlg.action {
            ModAction::Kick => ("Kick member", false),
            ModAction::Ban => ("Ban member", false),
            ModAction::Timeout(_) => ("Timeout member", true),
            ModAction::RemoveTimeout => ("Remove timeout", false),
        };
        egui::Window::new(title)
            .id(egui::Id::new("mod_dialog"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ui.ctx(), |ui| {
                ui.set_min_width(320.0);
                close = window_header(ui, title);
                ui.separator();
                ui.label(
                    RichText::new(format!("{} \u{2022} {}", dlg.user_name, dlg.user_id))
                        .weak(),
                );
                ui.add_space(4.0);
                if needs_minutes {
                    ui.horizontal(|ui| {
                        ui.label("Duration:");
                        ui.add(
                            egui::DragValue::new(&mut dlg.minutes)
                                .range(1..=40320)
                                .speed(1.0)
                                .suffix(" min"),
                        );
                    });
                    ui.horizontal_wrapped(|ui| {
                        for (label, mins) in
                            [("60s", 1u32), ("5m", 5), ("10m", 10), ("1h", 60), ("1d", 1440), ("1w", 10080)]
                        {
                            if ui.small_button(label).clicked() {
                                dlg.minutes = mins;
                            }
                        }
                    });
                    ui.add_space(4.0);
                }
                ui.label("Reason (optional)");
                let resp = ui.add(
                    egui::TextEdit::multiline(&mut dlg.reason)
                        .desired_rows(2)
                        .hint_text("Reason shown in the audit log")
                        .desired_width(f32::INFINITY),
                );
                if !dlg.focused {
                    resp.request_focus();
                    dlg.focused = true;
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let confirm_btn = if matches!(dlg.action, ModAction::Ban | ModAction::Kick) {
                        egui::Button::new("Confirm").fill(egui::Color32::from_rgb(180, 60, 60))
                    } else {
                        egui::Button::new("Confirm")
                    };
                    if ui.add(confirm_btn).clicked() {
                        confirm = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });

        if confirm {
            let reason = if dlg.reason.trim().is_empty() {
                None
            } else {
                Some(dlg.reason.trim().to_string())
            };
            let cmd = match dlg.action {
                ModAction::Kick => Command::KickUser {
                    guild_id: dlg.guild_id.clone(),
                    user_id: dlg.user_id.clone(),
                    reason,
                },
                ModAction::Ban => Command::BanUser {
                    guild_id: dlg.guild_id.clone(),
                    user_id: dlg.user_id.clone(),
                    reason,
                },
                ModAction::Timeout(_) => Command::TimeoutUser {
                    guild_id: dlg.guild_id.clone(),
                    user_id: dlg.user_id.clone(),
                    seconds: dlg.minutes as u64 * 60,
                    reason,
                },
                ModAction::RemoveTimeout => Command::TimeoutUser {
                    guild_id: dlg.guild_id.clone(),
                    user_id: dlg.user_id.clone(),
                    seconds: 0,
                    reason,
                },
            };
            let _ = self.cmd_tx.send(cmd);
            close = true;
        }
        if !close {
            self.mod_dialog = Some(dlg);
        }
    }

    fn switcher_window(&mut self, ui: &mut egui::Ui) {
        if !self.switcher_open {
            return;
        }
        let mut close = false;
        let mut chosen: Option<(String, String)> = None;
        egui::Window::new("Jump to")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 120.0))
            .show(ui.ctx(), |ui| {
                ui.set_min_width(360.0);
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.switcher_query)
                        .hint_text("Search channels and DMs")
                        .desired_width(f32::INFINITY),
                );
                if self.switcher_focus {
                    resp.request_focus();
                    self.switcher_focus = false;
                }
                let q = self.switcher_query.trim().to_lowercase();
                let mut entries: Vec<(String, String, String)> = Vec::new();
                {
                    let s = self.shared.lock().unwrap();
                    for guild in &s.guilds {
                        for ch in s.text_channels(&guild.id) {
                            entries.push((
                                format!("{}  # {}", guild.display_name(), ch.display_name()),
                                guild.id.clone(),
                                ch.id.clone(),
                            ));
                        }
                    }
                    for ch in s.dm_channels() {
                        entries.push((
                            format!("DM  {}", ch.dm_display_name()),
                            DM_HOME.to_string(),
                            ch.id.clone(),
                        ));
                    }
                }
                if !q.is_empty() {
                    entries.retain(|(label, _, _)| label.to_lowercase().contains(&q));
                }
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .show(ui, |ui| {
                        for (label, guild_id, channel_id) in entries.iter().take(50) {
                            if ui.selectable_label(false, label).clicked() {
                                chosen = Some((guild_id.clone(), channel_id.clone()));
                            }
                        }
                    });
            });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
        if let Some((guild_id, channel_id)) = chosen {
            {
                let mut s = self.shared.lock().unwrap();
                s.selected_guild = Some(guild_id);
                s.selected_channel = Some(channel_id.clone());
                s.clear_unread(&channel_id);
            }
            let _ = self.cmd_tx.send(Command::AckChannel(channel_id.clone()));
            let _ = self.cmd_tx.send(Command::FetchMessages(channel_id));
            close = true;
        }
        if close {
            self.switcher_open = false;
        }
    }

    fn profile_window(&mut self, ui: &mut egui::Ui) {
        let Some(view) = &self.profile_open else {
            return;
        };
        let user_id = view.user_id.clone();
        let guild_id = view.guild_id.clone();
        let requested = view.requested;

        let (user, have_full) = {
            let s = self.shared.lock().unwrap();
            let full = s.profiles.get(&user_id).cloned();
            let have_full = full.is_some();
            let base = full.or_else(|| s.profile(&user_id).cloned());
            (base, have_full)
        };

        if !requested && !have_full {
            let _ = self.cmd_tx.send(Command::FetchProfile {
                user_id: user_id.clone(),
                guild_id,
            });
            if let Some(view) = self.profile_open.as_mut() {
                view.requested = true;
            }
        }

        let Some(user) = user else {
            return;
        };

        let copied = self
            .copied_at
            .map(|t| t.elapsed() < Duration::from_millis(1200))
            .unwrap_or(false);
        let mut close = false;
        egui::Window::new("Profile")
            .id(egui::Id::new("profile_window"))
            .title_bar(false)
            .collapsible(false)
            .resizable(true)
            .default_size([340.0, 460.0])
            .min_size([260.0, 180.0])
            .show(ui.ctx(), |ui| {
                ui.set_min_width(300.0);
                close = window_header(ui, "Profile");
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add(
                        egui::Image::new(&user.avatar_url(160))
                            .fit_to_exact_size(egui::vec2(96.0, 96.0))
                            .corner_radius(48.0),
                    );
                    ui.add_space(6.0);
                    ui.heading(user.display_name());
                    ui.weak(user.handle());
                    if user.bot.unwrap_or(false) {
                        ui.label(RichText::new("BOT").small().strong());
                    }
                });
                ui.add_space(6.0);
                ui.separator();
                if let Some(bio) = user.bio.as_ref().filter(|b| !b.trim().is_empty()) {
                    ui.label(RichText::new("About me").strong());
                    ui.label(bio);
                    ui.add_space(6.0);
                    ui.separator();
                }
                ui.label(RichText::new("User ID").strong());
                ui.monospace(&user.id);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let copy_btn = if copied {
                        egui::Button::new("Copied!").fill(egui::Color32::from_rgb(46, 120, 60))
                    } else {
                        egui::Button::new("Copy ID")
                    };
                    if ui.add(copy_btn).clicked() {
                        ui.ctx().copy_text(user.id.clone());
                        self.copied_at = Some(Instant::now());
                    }
                    let is_self = self
                        .shared
                        .lock()
                        .unwrap()
                        .me
                        .as_ref()
                        .map(|m| m.id == user.id)
                        .unwrap_or(false);
                    if !is_self && ui.button("Message").clicked() {
                        let _ = self.cmd_tx.send(Command::OpenDm(user.id.clone()));
                    }
                });
                    });
            });

        if close {
            self.profile_open = None;
        }
    }

    fn settings_window(&mut self, ui: &mut egui::Ui) {
        if !self.settings_open {
            return;
        }
        let mut close = false;
        let mut changed = false;
        egui::Window::new("Settings")
            .id(egui::Id::new("settings_window"))
            .title_bar(false)
            .collapsible(false)
            .resizable(true)
            .show(ui.ctx(), |ui| {
                ui.set_min_width(300.0);
                close = window_header(ui, "Settings");
                ui.separator();
                egui::CollapsingHeader::new("Appearance")
                    .default_open(true)
                    .show(ui, |ui| {
                        changed |= ui
                            .checkbox(&mut self.settings.show_avatars, "Show avatars in chat")
                            .changed();
                        changed |= ui
                            .checkbox(&mut self.settings.compact, "Compact mode")
                            .changed();
                        changed |= ui
                            .checkbox(
                                &mut self.settings.use_names,
                                "Use usernames instead of display names",
                            )
                            .changed();
                        changed |= ui
                            .checkbox(
                                &mut self.settings.use_custom_names,
                                "Use custom names (nicknames you've set)",
                            )
                            .changed();
                        changed |= ui
                            .checkbox(&mut self.settings.time_24h, "24-hour timestamps")
                            .changed();
                        changed |= ui
                            .checkbox(&mut self.settings.theme_dark, "Dark theme")
                            .changed();
                    });
                egui::CollapsingHeader::new("Content")
                    .default_open(true)
                    .show(ui, |ui| {
                        changed |= ui
                            .checkbox(&mut self.settings.load_images, "Load images and media")
                            .changed();
                    });
                egui::CollapsingHeader::new("Members")
                    .default_open(true)
                    .show(ui, |ui| {
                        changed |= ui
                            .checkbox(&mut self.settings.show_member_list, "Show member list")
                            .changed();
                        changed |= ui
                            .checkbox(&mut self.settings.dm_avatars, "Show avatars in DM list")
                            .changed();
                    });
                egui::CollapsingHeader::new("Moderation")
                    .default_open(false)
                    .show(ui, |ui| {
                        changed |= ui
                            .checkbox(&mut self.settings.moderation, "Enable moderation tools (kick/ban/timeout)")
                            .changed();
                        changed |= ui
                            .checkbox(&mut self.settings.message_log, "Log message edits and deletions")
                            .changed();
                    });
                egui::CollapsingHeader::new("Menus & Gestures")
                    .default_open(false)
                    .show(ui, |ui| {
                        changed |= ui
                            .checkbox(
                                &mut self.settings.radial_menu,
                                "Enable radial menu (hold right-click on a message)",
                            )
                            .changed();
                    });
                egui::CollapsingHeader::new("Notifications")
                    .default_open(false)
                    .show(ui, |ui| {
                        changed |= ui
                            .checkbox(
                                &mut self.settings.notifications,
                                "Show notifications for DMs and mentions",
                            )
                            .changed();
                        changed |= ui
                            .checkbox(
                                &mut self.settings.notification_sound,
                                "Play a sound for notifications",
                            )
                            .changed();
                    });
                egui::CollapsingHeader::new("Performance")
                    .default_open(false)
                    .show(ui, |ui| {
                        changed |= ui
                            .checkbox(
                                &mut self.settings.performance_mode,
                                "Performance mode (less RAM / fewer repaints)",
                            )
                            .changed();
                    });
            });
        if changed {
            config::save_settings(&self.settings);
        }
        if close {
            self.settings_open = false;
        }
    }

    fn nickname_window(&mut self, ui: &mut egui::Ui) {
        let Some(edit) = self.nickname_edit.as_mut() else {
            return;
        };
        let mut save = false;
        let mut close = false;
        let display = edit.display.clone();
        let is_reset = edit.input.trim().is_empty();
        egui::Window::new("Set Nickname")
            .id(egui::Id::new("nickname_window"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.set_min_width(280.0);
                close = window_header(ui, "Set Nickname");
                ui.separator();
                ui.weak(format!("For {display}"));
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut edit.input)
                        .hint_text("Leave empty to reset")
                        .desired_width(f32::INFINITY),
                );
                if !edit.focused {
                    resp.request_focus();
                    edit.focused = true;
                }
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let label = if is_reset { "Reset" } else { "Save" };
                    if ui.button(label).clicked() || enter {
                        save = true;
                    }
                    if ui.button("Cancel").clicked() || escape {
                        close = true;
                    }
                });
            });

        if save {
            if let Some(edit) = self.nickname_edit.take() {
                self.shared
                    .lock()
                    .unwrap()
                    .set_alias(&edit.scope, &edit.user_id, &edit.input);
            }
        } else if close {
            self.nickname_edit = None;
        }
    }

    fn log_window(&mut self, ui: &mut egui::Ui) {
        if !self.log_open {
            return;
        }
        let enabled = self.settings.message_log;
        let time_24h = self.settings.time_24h;
        let entries: Vec<LogEntry> = self.shared.lock().unwrap().message_log.clone();
        let mut open = true;
        egui::Window::new("Message Log")
            .id(egui::Id::new("log_window"))
            .default_size([460.0, 400.0])
            .resizable(true)
            .open(&mut open)
            .show(ui.ctx(), |ui| {
                if !enabled {
                    ui.weak("Logging is disabled. Enable it in Settings \u{2192} Moderation.");
                    return;
                }
                ui.weak(format!("{} events", entries.len()));
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if entries.is_empty() {
                            ui.weak("No edits or deletions yet.");
                        }
                        for e in &entries {
                            let (tag, color) = match e.kind {
                                LogKind::Edited => ("edited", egui::Color32::from_rgb(240, 200, 90)),
                                LogKind::Deleted => ("deleted", egui::Color32::from_rgb(230, 110, 110)),
                            };
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(tag)
                                        .color(color)
                                        .strong(),
                                );
                                ui.label(RichText::new(&e.author).strong());
                                ui.weak(format!("in #{}", e.channel_name));
                            });
                            if e.kind == LogKind::Deleted {
                                ui.label(
                                    RichText::new(&e.before)
                                        .italics()
                                        .color(egui::Color32::from_gray(160)),
                                );
                            } else {
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    ui.label(
                                        RichText::new(&e.before)
                                            .strikethrough()
                                            .color(egui::Color32::from_gray(150)),
                                    );
                                    ui.label("\u{2192}");
                                    ui.label(&e.after);
                                });
                            }
                            if let Some(t) = parse_ts(&Some(e.at.clone())) {
                                let fmt = if time_24h { "%m/%d %H:%M" } else { "%m/%d %I:%M %p" };
                                ui.weak(t.format(fmt).to_string());
                            }
                            ui.separator();
                        }
                    });
            });
        if !open {
            self.log_open = false;
        }
    }

    fn pins_panel(&mut self, ui: &mut egui::Ui) {
        if !self.pins_open {
            return;
        }
        let Some(cid) = self.shared.lock().unwrap().selected_channel.clone() else {
            self.pins_open = false;
            return;
        };
        let mut close = false;
        let mut request_pins = false;
        let mut jump: Option<(String, String)> = None;
        {
            let s = self.shared.lock().unwrap();
            if !s.pins.contains_key(&cid) {
                request_pins = true;
            }
        }
        if request_pins {
            let _ = self.cmd_tx.send(Command::FetchPins(cid.clone()));
        }
        egui::Panel::right("pins_panel")
            .exact_size(260.0)
            .resizable(false)
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.heading("\u{1F4CC} Pinned");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("x").clicked() {
                            close = true;
                        }
                    });
                });
                ui.separator();
                let s = self.shared.lock().unwrap();
                match s.pins.get(&cid) {
                    Some(pins) if !pins.is_empty() => {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for m in pins {
                                let name = s
                                    .users
                                    .get(&m.author.id)
                                    .map(|u| u.display_name().to_string())
                                    .unwrap_or_else(|| m.author.display_name().to_string());
                                let preview: String =
                                    m.content.lines().next().unwrap_or("").chars().take(80).collect();
                                ui.group(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(name).strong());
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui
                                                    .small_button("Jump")
                                                    .on_hover_text("Jump to this message")
                                                    .clicked()
                                                {
                                                    jump = Some((m.channel_id.clone(), m.id.clone()));
                                                }
                                            },
                                        );
                                    });
                                    ui.label(preview);
                                });
                            }
                        });
                    }
                    Some(_) => {
                        ui.weak("No pinned messages.");
                    }
                    None => {
                        ui.weak("Loading pins...");
                    }
                }
            });
        if close {
            self.pins_open = false;
        }
        if let Some((cid, mid)) = jump {
            self.pins_open = false;
            self.jump_to = Some(JumpTarget {
                channel_id: cid.clone(),
                message_id: mid,
                tried: None,
            });
            {
                let mut s = self.shared.lock().unwrap();
                s.selected_channel = Some(cid.clone());
                s.clear_unread(&cid);
            }
            let _ = self.cmd_tx.send(Command::AckChannel(cid.clone()));
            let _ = self.cmd_tx.send(Command::FetchMessages(cid));
        }
    }

    fn member_list(&mut self, ui: &mut egui::Ui) {
        if !self.settings.show_member_list {
            return;
        }
        let Some(gid) = self.shared.lock().unwrap().selected_guild.clone() else {
            return;
        };
        if gid == DM_HOME {
            return;
        }
        let show_avatars = self.settings.show_avatars;
        let use_names = self.settings.use_names;
        let use_custom_names = self.settings.use_custom_names;
        let open_uid = self.profile_open.as_ref().map(|p| p.user_id.clone());
        let mut profile: Option<String> = None;
        let mut nickname: Option<NicknameEdit> = None;
        let mut request_members = false;
        {
            let mut s = self.shared.lock().unwrap();
            if s.guild_members(&gid).is_empty() && s.begin_members_fetch(&gid) {
                request_members = true;
            }
        }
        if request_members {
            let _ = self.cmd_tx.send(Command::FetchGuildMembers(gid.clone()));
        }
        egui::Panel::right("member_list")
            .exact_size(200.0)
            .resizable(false)
            .show(ui, |ui| {
                ui.add_space(8.0);
                let s = self.shared.lock().unwrap();
                let mut members = s.guild_members(&gid);
                let guild = s.guilds.iter().find(|g| g.id == gid);
                let rank = |uid: &str| -> u8 {
                    match guild.map(|g| g.status_of(uid)).unwrap_or("offline") {
                        "online" => 0,
                        "idle" => 1,
                        "dnd" => 2,
                        _ => 3,
                    }
                };
                members.sort_by(|a, b| {
                    rank(&a.user.id)
                        .cmp(&rank(&b.user.id))
                        .then_with(|| {
                            member_display_name(&s, use_custom_names, use_names, &gid, a)
                                .to_lowercase()
                                .cmp(&member_display_name(&s, use_custom_names, use_names, &gid, b).to_lowercase())
                        })
                });
                let online = members
                    .iter()
                    .filter(|m| rank(&m.user.id) != 3)
                    .count();
                ui.horizontal(|ui| {
                    ui.heading("Members");
                    ui.weak(format!("{online}/{}", members.len()));
                });
                ui.add(
                    egui::TextEdit::singleline(&mut self.member_search)
                        .hint_text("Search members")
                        .desired_width(f32::INFINITY),
                );
                let q = self.member_search.trim().to_lowercase();
                if !q.is_empty() {
                    members.retain(|m| {
                        member_display_name(&s, use_custom_names, use_names, &gid, m)
                            .to_lowercase()
                            .contains(&q)
                    });
                }
                ui.separator();
                if members.is_empty() && q.is_empty() {
                    let pending = s.members_pending(&gid);
                    let loaded = s.members_loaded(&gid);
                    ui.weak(if pending {
                        "Loading members..."
                    } else if loaded {
                        "Member list unavailable (Discord blocks user tokens)."
                    } else {
                        "Members not loaded."
                    });
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for m in &members {
                            let name = member_display_name(&s, use_custom_names, use_names, &gid, m);
                            let selected = open_uid.as_deref() == Some(m.user.id.as_str());
                            let img_url = m.user.avatar_url(64);
                            let status = s
                                .guilds
                                .iter()
                                .find(|g| g.id == gid)
                                .map(|g| g.status_of(&m.user.id).to_string())
                                .unwrap_or_default();
                            let resp = ui
                                .horizontal(|ui| {
                                    if show_avatars {
                                        avatar_ring(ui, &img_url, 24.0, Some(&status));
                                    }
                                    let text = RichText::new(name.clone());
                                    ui.label(if selected {
                                        text.strong()
                                    } else {
                                        text
                                    });
                                })
                                .response
                                .interact(egui::Sense::click())
                                .on_hover_text(m.username());
                            if resp.clicked() {
                                profile = Some(m.user.id.clone());
                            }
                            let user_id = m.user.id.clone();
                            let display = m.display_name();
                            let current = s.alias(&gid, &user_id).unwrap_or("").to_string();
                            resp.context_menu(|ui| {
                                if ui.button("View Profile").clicked() {
                                    profile = Some(user_id.clone());
                                    ui.close();
                                }
                                if ui.button("Set Nickname...").clicked() {
                                    nickname = Some(NicknameEdit {
                                        scope: gid.clone(),
                                        user_id: user_id.clone(),
                                        display: display.clone(),
                                        input: current.clone(),
                                        focused: false,
                                    });
                                    ui.close();
                                }
                            });
                        }
                    });
            });
        if let Some(uid) = profile {
            self.open_profile(uid, Some(gid.clone()));
        }
        if let Some(n) = nickname {
            self.nickname_edit = Some(n);
        }
    }

    fn image_viewer(&mut self, ui: &mut egui::Ui) {
        let Some(url) = self.image_view.clone() else {
            return;
        };
        let mut close = false;
        let mut zoom = self.image_zoom;
        let mut pan = self.image_pan;
        let prev_zoom = zoom;
        let screen = ui
            .ctx()
            .input(|i| i.raw.screen_rect)
            .unwrap_or_else(|| egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0)));
        egui::Window::new("Image")
            .id(egui::Id::new("image_viewer"))
            .title_bar(false)
            .collapsible(false)
            .resizable(true)
            .default_pos(egui::pos2(screen.center().x - 320.0, screen.center().y - 240.0))
            .default_size(egui::vec2(
                (screen.width() * 0.6).max(320.0),
                (screen.height() * 0.6).max(240.0),
            ))
            .show(ui.ctx(), |ui| {
                close = window_header(ui, "Image");
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.small_button("\u{2212}").on_hover_text("Zoom out").clicked() {
                        zoom = (zoom - 0.25).max(0.25);
                    }
                    if ui.small_button("+").on_hover_text("Zoom in").clicked() {
                        zoom = (zoom + 0.25).min(8.0);
                    }
                    if ui.small_button("Reset").clicked() {
                        zoom = 1.0;
                        pan = egui::Vec2::ZERO;
                    }
                    ui.weak(format!("{:.0}%", zoom * 100.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Copy link").clicked() {
                            ui.ctx().copy_text(url.clone());
                        }
                        if ui.small_button("Open").clicked() {
                            open_url(&url);
                        }
                    });
                });
                let viewport = ui.available_rect_before_wrap();

                // Button zoom keeps the view centred.
                if zoom != prev_zoom && prev_zoom > 0.0 {
                    let c = viewport.center() - viewport.min;
                    let ratio = zoom / prev_zoom;
                    pan = (pan + c) * ratio - c;
                }

                // Wheel = zoom toward the cursor (not pan).
                let pointer = ui.ctx().input(|i| i.pointer.hover_pos());
                let scroll = ui.ctx().input(|i| i.smooth_scroll_delta.y);
                if scroll != 0.0 {
                    if let Some(p) = pointer {
                        if viewport.contains(p) {
                            let z_old = zoom;
                            zoom = (zoom * (1.0 + scroll * 0.0018)).clamp(0.25, 8.0);
                            let focal = p - viewport.min;
                            let ratio = zoom / z_old;
                            pan = (pan + focal) * ratio - focal;
                        }
                    }
                }

                let target = viewport.size() * zoom;
                let mut drag = egui::Vec2::ZERO;
                let output = egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .animated(false)
                    .wheel_scroll_multiplier(egui::vec2(0.0, 0.0))
                    .scroll_offset(pan)
                    .show(ui, |ui| {
                        let resp = ui.add(
                            egui::Image::new(&url)
                                .fit_to_exact_size(target)
                                .maintain_aspect_ratio(true)
                                .sense(egui::Sense::drag()),
                        );
                        if resp.dragged() {
                            drag = resp.drag_delta();
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                        } else if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                        }
                    });
                // Content follows the cursor: dragging right/down decreases the offset.
                let max_off = (output.content_size - output.inner_rect.size()).max(egui::Vec2::ZERO);
                pan = (output.state.offset - drag).clamp(egui::Vec2::ZERO, max_off);
            });
        self.image_zoom = zoom;
        self.image_pan = pan;
        if close || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.image_view = None;
            // The full-size view uses a distinct (width-suffixed) URI from the
            // inline thumbnail, so releasing it frees the big texture without
            // disturbing what's still on screen.
            if url.contains("width=") {
                ui.ctx().forget_image(&url);
            }
        }
    }

    fn emoji_picker_window(&mut self, ui: &mut egui::Ui) {
        let Some(mut picker) = self.emoji_picker.take() else {
            return;
        };
        let mut close = false;
        let mut chosen: Option<String> = None;
        egui::Window::new("Emoji")
            .id(egui::Id::new("emoji_picker"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                close = window_header(ui, "Emoji");
                ui.separator();
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut picker.search)
                        .hint_text("Search emoji")
                        .desired_width(f32::INFINITY),
                );
                if !picker.focused {
                    resp.request_focus();
                    picker.focused = true;
                }
                ui.add_space(2.0);
                let query = picker.search.trim().to_lowercase();
                egui::ScrollArea::vertical()
                    .max_height(280.0)
                    .show(ui, |ui| {
                        if query.is_empty() && !self.settings.recent_emojis.is_empty() {
                            ui.weak("Recent");
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
                                for ch in self.settings.recent_emojis.iter() {
                                    if emoji_button(ui, ch, 22.0).clicked() {
                                        chosen = Some(ch.clone());
                                    }
                                }
                            });
                            ui.separator();
                        }
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
                            for (name, ch) in EMOJIS.iter() {
                                if !query.is_empty() && !name.contains(&query) {
                                    continue;
                                }
                                if emoji_button(ui, ch, 22.0)
                                    .on_hover_text(*name)
                                    .clicked()
                                {
                                    chosen = Some((*ch).to_string());
                                }
                            }
                        });
                    });
            });

        if let Some(emoji) = chosen {
            self.settings.recent_emojis.retain(|e| e != &emoji);
            self.settings.recent_emojis.insert(0, emoji.clone());
            self.settings.recent_emojis.truncate(24);
            config::save_settings(&self.settings);
            match &picker.target {
                EmojiTarget::Composer => {
                    self.draft.push_str(&emoji);
                    self.focus_input = true;
                    close = true;
                }
                EmojiTarget::Reaction {
                    channel_id,
                    message_id,
                } => {
                    let em = Emoji {
                        id: None,
                        name: Some(emoji.clone()),
                        animated: None,
                    };
                    self.shared.lock().unwrap().react_add(
                        channel_id,
                        message_id,
                        em.clone(),
                        true,
                    );
                    let _ = self.cmd_tx.send(Command::AddReaction {
                        channel_id: channel_id.clone(),
                        message_id: message_id.clone(),
                        emoji: em.key(),
                    });
                }
            }
        }

        if !close {
            self.emoji_picker = Some(picker);
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let status = self.status();
        let mut open_self: Option<String> = None;
        egui::Panel::top("top_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                let s = self.shared.lock().unwrap();
                if let Some(me) = &s.me {
                    let name = me.display_name();
                    if ui
                        .add(egui::Label::new(RichText::new(name).strong()).sense(egui::Sense::click()))
                        .clicked()
                    {
                        open_self = Some(me.id.clone());
                    }
                    let handle = me.handle();
                    ui.weak(handle);
                }
                drop(s);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Log out").clicked() {
                        let _ = self.cmd_tx.send(Command::Disconnect);
                    }
                    if ui.button("Settings").clicked() {
                        self.settings_open = true;
                    }
                    let bell = if self.unseen_notifications {
                        "\u{1F514}\u{2022}"
                    } else {
                        "\u{1F514}"
                    };
                    if ui
                        .button(bell)
                        .on_hover_text("Notification history")
                        .clicked()
                    {
                        self.notifications_open = !self.notifications_open;
                        if self.notifications_open {
                            self.unseen_notifications = false;
                        }
                    }
                    if ui
                        .button("Jump")
                        .on_hover_text("Quick switcher (Ctrl+K)")
                        .clicked()
                    {
                        self.switcher_open = true;
                        self.switcher_query.clear();
                        self.switcher_focus = true;
                    }
                    match status {
                        ConnectionStatus::Reconnecting => {
                            ui.spinner();
                            ui.colored_label(egui::Color32::from_rgb(220, 180, 90), "Reconnecting");
                        }
                        _ => {
                            ui.weak("Connected");
                        }
                    }
                });
            });
        });

        if let Some(user_id) = open_self {
            self.open_profile(user_id, None);
        }
    }

    fn guild_rail(&mut self, ui: &mut egui::Ui) {
        let mut clicked: Option<String> = None;
        let mut pin_toggle: Option<String> = None;
        egui::Panel::left("guild_rail")
            .exact_size(72.0)
            .resizable(false)
            .show(ui, |ui| {
                ui.add_space(8.0);
                let s = self.shared.lock().unwrap();
                let dm_selected = s.selected_guild.as_deref() == Some(DM_HOME);
                let dm_unread = s.dm_unread_total();
                let dm_mentions = s.dm_mentions_total();
                let dm_resp = ui
                    .add_sized(
                        [48.0, 48.0],
                        egui::Button::selectable(dm_selected, "@").corner_radius(16.0),
                    )
                    .on_hover_text("Direct Messages");
                if dm_unread > 0 {
                    badge(ui, dm_resp.rect, dm_unread, dm_mentions > 0);
                }
                if dm_resp.clicked() {
                    clicked = Some(DM_HOME.to_string());
                }
                ui.separator();
                let mut guilds: Vec<&crate::model::Guild> = s.guilds.iter().collect();
                guilds.sort_by_key(|g| if s.is_guild_pinned(&g.id) { 0 } else { 1 });
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for guild in guilds {
                            let selected = s.selected_guild.as_deref() == Some(guild.id.as_str());
                            let unread = s.guild_unread(&guild.id);
                            let guild_mentions = s.guild_mentions(&guild.id);
                            let pinned = s.is_guild_pinned(&guild.id);
                            ui.add_space(4.0);
                            let resp = match guild.icon_url(56) {
                                Some(url) => ui
                                    .vertical_centered(|ui| {
                                        ui.add(
                                            egui::Image::new(&url)
                                                .fit_to_exact_size(egui::vec2(48.0, 48.0))
                                                .corner_radius(if selected { 14.0 } else { 24.0 }),
                                        )
                                    })
                                    .inner
                                    .interact(egui::Sense::click()),
                                None => {
                                    let initial = guild
                                        .display_name()
                                        .chars()
                                        .next()
                                        .map(|c| c.to_uppercase().to_string())
                                        .unwrap_or_else(|| "?".to_string());
                                    ui.add_sized(
                                        [48.0, 48.0],
                                        egui::Button::selectable(selected, initial)
                                            .corner_radius(if selected { 14.0 } else { 24.0 }),
                                    )
                                }
                            };
                            let resp = resp.on_hover_text(guild.display_name());
                            if unread > 0 {
                                badge(ui, resp.rect, unread, guild_mentions > 0);
                            }
                            if resp.clicked() {
                                clicked = Some(guild.id.clone());
                            }
                            let gid = guild.id.clone();
                            resp.context_menu(|ui| {
                                if ui
                                    .button(if pinned { "Unpin Server" } else { "Pin Server" })
                                    .clicked()
                                {
                                    pin_toggle = Some(gid.clone());
                                    ui.close();
                                }
                            });
                        }
                    });
            });

        if let Some(id) = pin_toggle {
            self.shared.lock().unwrap().toggle_pin_guild(&id);
        }

        if let Some(id) = clicked {
            let changed = self.shared.lock().unwrap().selected_guild.as_deref() != Some(id.as_str());
            {
                let mut s = self.shared.lock().unwrap();
                s.selected_guild = Some(id.clone());
                if changed {
                    s.selected_channel = None;
                }
            }
            if id == DM_HOME {
                let _ = self.cmd_tx.send(Command::FetchDms);
            } else {
                if !self.shared.lock().unwrap().has_channels(&id) {
                    let _ = self.cmd_tx.send(Command::FetchGuildChannels(id.clone()));
                }
                let request_members = self.shared.lock().unwrap().begin_members_fetch(&id);
                if request_members {
                    let _ = self.cmd_tx.send(Command::FetchGuildMembers(id));
                }
            }
        }
    }

    fn channel_list(&mut self, ui: &mut egui::Ui) {
        let mut clicked: Option<String> = None;
        let mut nickname: Option<NicknameEdit> = None;
        let mut pin_toggle: Option<(bool, String)> = None;
        let mut set_friends: Option<bool> = None;
        let mut friend_dm: Option<String> = None;
        let mut mark_read: Option<String> = None;
        let friends_open = self.friends_open;
        let show_avatars = self.settings.dm_avatars;
        let use_names = self.settings.use_names;
        egui::Panel::left("channel_list")
            .exact_size(200.0)
            .resizable(false)
            .show(ui, |ui| {
                let s = self.shared.lock().unwrap();
                let Some(guild_id) = s.selected_guild.clone() else {
                    ui.add_space(8.0);
                    ui.label("Select a server or Direct Messages");
                    return;
                };

                if guild_id == DM_HOME {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if friends_open {
                            if ui
                                .small_button("\u{2190}")
                                .on_hover_text("Back to Direct Messages")
                                .clicked()
                            {
                                set_friends = Some(false);
                            }
                            ui.label(RichText::new("Friends").size(15.0).strong());
                        } else {
                            ui.label(RichText::new("Direct Messages").size(15.0).strong());
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .small_button("Friends")
                                        .on_hover_text("Show your friends")
                                        .clicked()
                                    {
                                        set_friends = Some(true);
                                    }
                                },
                            );
                        }
                    });
                    ui.separator();
                    if friends_open {
                        let mut friends: Vec<&crate::model::Relationship> =
                            s.friends.iter().filter(|r| r.kind == 1).collect();
                        friends.sort_by(|a, b| {
                            let oa = presence_rank(s.friend_presence(&a.user.id));
                            let ob = presence_rank(s.friend_presence(&b.user.id));
                            oa.cmp(&ob)
                                .then_with(|| a.user.display_name().cmp(&b.user.display_name()))
                        });
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                if friends.is_empty() {
                                    ui.weak("No friends loaded.");
                                }
                                for rel in &friends {
                                    let u = &rel.user;
                                    let status = s.friend_presence(&u.id);
                                    let name = if use_names {
                                        u.username.clone()
                                    } else {
                                        s.name_in(DM_HOME, u)
                                    };
                                    let label = format!("{} \u{2014} {}", name, status);
                                    let mut hit = false;
                                    let resp = ui.horizontal(|ui| {
                                        if show_avatars {
                                            if avatar_ring(
                                                ui,
                                                &u.avatar_url(64),
                                                24.0,
                                                Some(status),
                                            )
                                            .clicked()
                                            {
                                                hit = true;
                                            }
                                        }
                                        if ui.selectable_label(false, name.clone()).clicked() {
                                            hit = true;
                                        }
                                    });
                                    if hit {
                                        friend_dm = Some(u.id.clone());
                                    }
                                    resp.response.on_hover_text(&label);
                                    ui.add_space(2.0);
                                }
                            });
                        return;
                    }
                    let mut channels = s.dm_channels();
                    channels.sort_by(|a, b| {
                        let pa = s.is_dm_pinned(&a.id);
                        let pb = s.is_dm_pinned(&b.id);
                        pb.cmp(&pa).then_with(|| {
                            let la: u64 =
                                a.last_message_id.as_deref().unwrap_or("0").parse().unwrap_or(0);
                            let lb: u64 =
                                b.last_message_id.as_deref().unwrap_or("0").parse().unwrap_or(0);
                            lb.cmp(&la)
                        })
                    });
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if channels.is_empty() {
                                ui.weak("No conversations loaded.");
                            }
                            for channel in &channels {
                                let selected =
                                    s.selected_channel.as_deref() == Some(channel.id.as_str());
                                let unread = s.channel_unread(&channel.id);
                                let mention = s.channel_mentions(&channel.id) > 0;
                                let pinned = s.is_dm_pinned(&channel.id);
                                let other = channel.recipients.first();
                                let name = match other {
                                    Some(u) if use_names => u.username.clone(),
                                    Some(u) => s.name_in(DM_HOME, u),
                                    None => channel.dm_display_name(),
                                };
                                let mut img_url: Option<String> = None;
                                if show_avatars {
                                    if let Some(u) = other {
                                        img_url = Some(u.avatar_url(64));
                                    }
                                }
                                let presence = other.map(|u| s.presence_of(&u.id).to_string());
                                let resp = ui
                                    .horizontal(|ui| {
                                        if let Some(url) = &img_url {
                                            avatar_ring(
                                                ui,
                                                url,
                                                24.0,
                                                presence.as_deref(),
                                            );
                                        }
                                        let text = RichText::new(name.clone());
                                        ui.selectable_label(
                                            selected,
                                            if unread > 0 { text.strong() } else { text },
                                        )
                                    })
                                    .inner;
                                if unread > 0 {
                                    badge(ui, resp.rect, unread, mention);
                                }
                                if resp.clicked() {
                                    clicked = Some(channel.id.clone());
                                }
                                let cid = channel.id.clone();
                                let user_id = other.map(|u| u.id.clone());
                                let display = other
                                    .map(|u| u.display_name())
                                    .unwrap_or_else(|| name.clone());
                                let current = user_id
                                    .as_ref()
                                    .and_then(|id| s.alias(DM_HOME, id))
                                    .unwrap_or("")
                                    .to_string();
                                resp.context_menu(|ui| {
                                    if unread > 0 && ui.button("Mark as Read").clicked() {
                                        mark_read = Some(cid.clone());
                                        ui.close();
                                    }
                                    if ui
                                        .button(if pinned { "Unpin" } else { "Pin" })
                                        .clicked()
                                    {
                                        pin_toggle = Some((false, cid.clone()));
                                        ui.close();
                                    }
                                    if let Some(uid) = &user_id {
                                        if ui.button("Set Nickname...").clicked() {
                                            nickname = Some(NicknameEdit {
                                                scope: DM_HOME.to_string(),
                                                user_id: uid.clone(),
                                                display: display.clone(),
                                                input: current.clone(),
                                                focused: false,
                                            });
                                            ui.close();
                                        }
                                    }
                                });
                            }
                        });
                    return;
                }

                let title = s
                    .guilds
                    .iter()
                    .find(|g| g.id == guild_id)
                    .map(|g| g.display_name())
                    .unwrap_or_else(|| "Server".to_string());
                ui.add_space(8.0);
                ui.heading(title);
                ui.separator();

                let channels = s.text_channels(&guild_id);
                let voice = s.voice_channels(&guild_id);
                let categories = s.categories(&guild_id);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if channels.is_empty() {
                            ui.weak("Loading channels...");
                        }
                        let uncategorized: Vec<&crate::model::Channel> = channels
                            .iter()
                            .filter(|c| c.parent_id.is_none())
                            .collect();
                        render_channel_rows(ui, &s, &uncategorized, &mut clicked, &mut mark_read);
                        let unc_voice: Vec<&crate::model::Channel> = voice
                            .iter()
                            .filter(|c| c.parent_id.is_none())
                            .collect();
                        render_voice_rows(ui, &unc_voice);
                        for cat in &categories {
                            let in_cat: Vec<&crate::model::Channel> = channels
                                .iter()
                                .filter(|c| c.parent_id.as_deref() == Some(cat.id.as_str()))
                                .collect();
                            let in_cat_voice: Vec<&crate::model::Channel> = voice
                                .iter()
                                .filter(|c| c.parent_id.as_deref() == Some(cat.id.as_str()))
                                .collect();
                            if in_cat.is_empty() && in_cat_voice.is_empty() {
                                continue;
                            }
                            egui::CollapsingHeader::new(
                                RichText::new(cat.display_name().to_uppercase()).size(11.0),
                            )
                            .default_open(true)
                            .show(ui, |ui| {
                                render_channel_rows(ui, &s, &in_cat, &mut clicked, &mut mark_read);
                                render_voice_rows(ui, &in_cat_voice);
                            });
                        }
                    });
            });

        if let Some(n) = nickname {
            self.nickname_edit = Some(n);
        }
        if let Some((is_guild, id)) = pin_toggle {
            let mut s = self.shared.lock().unwrap();
            if is_guild {
                s.toggle_pin_guild(&id);
            } else {
                s.toggle_pin_dm(&id);
            }
        }

        if let Some(id) = clicked {
            let mut s = self.shared.lock().unwrap();
            s.selected_channel = Some(id.clone());
            s.clear_unread(&id);
            drop(s);
            let _ = self.cmd_tx.send(Command::AckChannel(id.clone()));
            let _ = self.cmd_tx.send(Command::FetchMessages(id));
        }
        if let Some(id) = mark_read {
            self.shared.lock().unwrap().clear_unread(&id);
            let _ = self.cmd_tx.send(Command::AckChannel(id));
        }
        if let Some(v) = set_friends {
            self.friends_open = v;
        }
        if let Some(user_id) = friend_dm {
            self.friends_open = false;
            let _ = self.cmd_tx.send(Command::OpenDm(user_id));
        }
    }

    fn message_view(&mut self, ui: &mut egui::Ui) {
        let shared = self.shared.clone();
        let s = shared.lock().unwrap();

        let Some(channel_id) = s.selected_channel.clone() else {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.centered_and_justified(|ui| ui.weak("Select a channel to start chatting"));
            });
            return;
        };

        let me_id = s.me.as_ref().map(|u| u.id.clone());
        let guild_context = s.selected_guild.clone().filter(|g| g != DM_HOME);
        let name = s
            .channels
            .get(&channel_id)
            .map(|c| if c.is_dm() { c.dm_display_name() } else { format!("# {}", c.display_name()) })
            .unwrap_or_else(|| "channel".to_string());

        let loading = s.loading_older.contains(&channel_id);
        let reached_top = s.reached_top.contains(&channel_id);
        let topic = s
            .channels
            .get(&channel_id)
            .and_then(|c| c.topic.clone())
            .filter(|t| !t.is_empty());
        let messages: Option<&Vec<Message>> = s.messages.get(&channel_id);
        let msg_len = messages.map(|m| m.len()).unwrap_or(0);

        let mut action: Option<MsgAction> = None;
        let mut fetch_older: Option<String> = None;
        let mut trim_to: Option<usize> = None;

        // Detect a channel switch so we can drop stale layout cache and apply a
        // single clean bottom snap once the new channel's messages first appear
        // (avoids the down-then-up jitter from egui's stick-to-bottom diffing).
        if self.anchor_channel.as_deref() != Some(channel_id.as_str()) {
            self.anchor_channel = Some(channel_id.clone());
            self.render.rects.clear();
            self.render.seg_cache.clear();
        }

        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                let head = ui.heading(name);
                if let Some(topic) = &topic {
                    head.on_hover_text(topic);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if self.pins_open { "\u{1F4CC} Pins" } else { "\u{1F4CC}" };
                    if ui
                        .selectable_label(self.pins_open, label)
                        .on_hover_text("Pinned messages")
                        .clicked()
                    {
                        self.pins_open = !self.pins_open;
                    }
                    let llabel = if self.log_open { "\u{1F4DC} Log" } else { "\u{1F4DC}" };
                    if ui
                        .selectable_label(self.log_open, llabel)
                        .on_hover_text("Message log (edits/deletes)")
                        .clicked()
                    {
                        self.log_open = !self.log_open;
                    }
                    let sresp = ui.add(
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text("Search messages")
                            .desired_width(200.0),
                    );
                    if self.search_focus {
                        sresp.request_focus();
                        self.search_focus = false;
                    }
                    if !self.search.is_empty() && ui.small_button("x").clicked() {
                        self.search.clear();
                    }
                });
            });
            ui.separator();
            let search = self.search.trim().to_lowercase();

            let jump = self.jump_bottom;
            let mut scroll = egui::ScrollArea::vertical().stick_to_bottom(true);
            if let Some(o) = self.scroll_override.take() {
                scroll = scroll.vertical_scroll_offset(o);
            }
            let output = scroll
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let Some(messages) = messages else {
                        ui.weak("No messages loaded.");
                        return;
                    };
                    if messages.is_empty() {
                        ui.weak("No messages yet.");
                    }
                    if loading {
                        ui.weak("Loading older messages...");
                    } else if reached_top && !messages.is_empty() {
                        ui.weak("This is the beginning of the channel.");
                        ui.add_space(4.0);
                    }
                    let searching = !search.is_empty();
                    let mut prev: Option<&Message> = None;
                    let mut shown = 0usize;
                    for message in messages.iter() {
                        if searching && !message.content.to_lowercase().contains(&search) {
                            continue;
                        }
                        shown += 1;
                        render_message(
                            ui,
                            message,
                            if searching { None } else { prev },
                            me_id.as_deref(),
                            &s,
                            &self.settings,
                            &mut action,
                            &mut self.render,
                        );
                        prev = Some(message);
                    }
                    if searching && shown == 0 {
                        ui.weak("No matching messages.");
                    }
                });

            // Jump to a pinned message: once its rect is known this frame,
            // request that exact scroll offset for the next frame; otherwise page
            // further back until the message eventually loads into view.
            if let Some(t) = self.jump_to.clone() {
                if t.channel_id == channel_id {
                    if let Some(r) = self.render.rects.get(&t.message_id) {
                        self.scroll_override = Some((r.top() - 8.0).max(0.0));
                        self.jump_to = None;
                    } else if let Some(msgs) = messages {
                        if let Some(oldest) = msgs.first().map(|m| m.id.clone()) {
                            if t.tried.as_deref() != Some(oldest.as_str()) {
                                self.jump_to.as_mut().unwrap().tried = Some(oldest.clone());
                                let _ = self.cmd_tx.send(Command::FetchOlder {
                                    channel_id: t.channel_id.clone(),
                                    before: oldest,
                                });
                            }
                        }
                    }
                }
            }

            let max_offset = (output.content_size.y - output.inner_rect.height()).max(0.0);

            // Auto-load history only when genuinely pinned to the very top of a
            // scrollable list (max_offset > 0 keeps short lists from churning).
            if !loading && !reached_top {
                if let Some(messages) = messages {
                    if !messages.is_empty()
                        && max_offset > 120.0
                        && output.state.offset.y < 120.0
                    {
                        fetch_older = messages.first().map(|m| m.id.clone());
                    }
                }
            }

            let at_bottom = max_offset - output.state.offset.y < 60.0;
            // Pinned to the bottom: release text (and images) scrolled far past.
            const BOTTOM_KEEP: usize = 120;
            if at_bottom && !loading && msg_len > BOTTOM_KEEP + 40 {
                trim_to = Some(BOTTOM_KEEP);
            }
            if !at_bottom && !jump {
                let pos = egui::pos2(
                    output.inner_rect.right() - 96.0,
                    output.inner_rect.bottom() - 40.0,
                );
                egui::Area::new(egui::Id::new("jump_bottom"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(pos)
                    .show(ui.ctx(), |ui| {
                        if ui.button("\u{2193} Jump").clicked() {
                            self.jump_bottom = true;
                        }
                    });
            }

            if jump {
                // Jump using the measured content height (finite value) next frame;
                // never feed INFINITY into the scroll offset (it corrupts state).
                self.scroll_override = Some(output.content_size.y);
                self.jump_bottom = false;
            }

            if let Some((pos, ctx)) = self.render.right_menu.clone() {
                let mut menu_act: Option<MsgAction> = None;
                let area = egui::Area::new(egui::Id::new("right_message_menu"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(pos)
                    .show(ui.ctx(), |ui| {
                        egui::Frame::menu(ui.style()).show(ui, |ui| {
                            message_menu(ui, &ctx, &mut menu_act);
                        });
                    });
                let menu_rect = area.response.rect;
                let clicked_any = ui.input(|i| i.pointer.any_click());
                let over_menu = ui.input(|i| {
                    i.pointer
                        .interact_pos()
                        .map(|p| menu_rect.contains(p))
                        .unwrap_or(false)
                });
                if menu_act.is_some() || (clicked_any && !over_menu) {
                    self.render.right_menu = None;
                }
                if action.is_none() {
                    action = menu_act;
                }
            }
        });

        drop(s);

        if let Some(keep) = trim_to {
            let uris = self
                .shared
                .lock()
                .unwrap()
                .trim_channel_history(&channel_id, keep);
            for u in uris {
                ui.ctx().forget_image(&u);
            }
        }

        if let Some(before) = fetch_older {
            let _ = self
                .cmd_tx
                .send(Command::FetchOlder { channel_id, before });
        }

        match action {
            Some(MsgAction::Reply {
                channel_id,
                message_id,
                author,
                preview,
            }) => {
                self.replying_to = Some(ReplyTarget {
                    channel_id,
                    message_id,
                    author,
                    preview,
                });
                self.editing = None;
                self.focus_input = true;
            }
            Some(MsgAction::Edit {
                channel_id,
                message_id,
                content,
            }) => {
                self.draft = content;
                self.editing = Some(EditTarget {
                    channel_id,
                    message_id,
                });
                self.replying_to = None;
                self.focus_input = true;
            }
            Some(MsgAction::Delete {
                channel_id,
                message_id,
            }) => {
                let _ = self.cmd_tx.send(Command::DeleteMessage {
                    channel_id,
                    message_id,
                });
            }
            Some(MsgAction::OpenProfile { user_id }) => {
                self.open_profile(user_id, guild_context);
            }
            Some(MsgAction::SetNickname {
                scope,
                user_id,
                display,
                current,
            }) => {
                self.nickname_edit = Some(NicknameEdit {
                    scope,
                    user_id,
                    display,
                    input: current,
                    focused: false,
                });
            }
            Some(MsgAction::OpenImage { url }) => {
                self.image_view = Some(url);
                self.image_zoom = 1.0;
                self.image_pan = egui::Vec2::ZERO;
            }
            Some(MsgAction::JumpTo {
                channel_id,
                message_id,
            }) => {
                let already = {
                    let s = self.shared.lock().unwrap();
                    s.selected_channel.as_deref() == Some(channel_id.as_str())
                };
                if !already {
                    {
                        let mut s = self.shared.lock().unwrap();
                        s.selected_channel = Some(channel_id.clone());
                        s.clear_unread(&channel_id);
                    }
                    let _ = self.cmd_tx.send(Command::AckChannel(channel_id.clone()));
                    let _ = self.cmd_tx.send(Command::FetchMessages(channel_id.clone()));
                }
                self.jump_to = Some(JumpTarget {
                    channel_id,
                    message_id,
                    tried: None,
                });
            }
            Some(MsgAction::PickReaction {
                channel_id,
                message_id,
            }) => {
                self.emoji_picker = Some(EmojiPicker {
                    target: EmojiTarget::Reaction {
                        channel_id,
                        message_id,
                    },
                    search: String::new(),
                    focused: false,
                });
            }
            Some(MsgAction::OpenDm { user_id }) => {
                let _ = self.cmd_tx.send(Command::OpenDm(user_id));
            }
            Some(MsgAction::Moderate {
                guild_id,
                user_id,
                user_name,
                action,
            }) => {
                let minutes = match action {
                    ModAction::Timeout(secs) if secs > 0 => (secs / 60) as u32,
                    ModAction::Timeout(_) => 10,
                    _ => 10,
                };
                self.mod_dialog = Some(ModDialog {
                    guild_id,
                    user_id,
                    user_name,
                    action,
                    minutes,
                    reason: String::new(),
                    focused: false,
                });
            }
            Some(MsgAction::ToggleReaction {
                channel_id,
                message_id,
                emoji,
            }) => {
                let key = emoji.key();
                let added = self
                    .shared
                    .lock()
                    .unwrap()
                    .toggle_reaction(&channel_id, &message_id, emoji);
                if let Some(added) = added {
                    let cmd = if added {
                        Command::AddReaction {
                            channel_id,
                            message_id,
                            emoji: key,
                        }
                    } else {
                        Command::RemoveReaction {
                            channel_id,
                            message_id,
                            emoji: key,
                        }
                    };
                    let _ = self.cmd_tx.send(cmd);
                }
            }
            None => {}
        }
    }

    fn input_bar(&mut self, ui: &mut egui::Ui) {
        let channel_id = self.shared.lock().unwrap().selected_channel.clone();
        let enabled = channel_id.is_some();
        let typing: Vec<String> = channel_id
            .as_ref()
            .map(|cid| self.shared.lock().unwrap().typing_names(cid))
            .unwrap_or_default();
        let mut submit = false;
        let mut cancel_edit = false;
        let mut cancel_reply = false;
        let mut open_picker = false;
        let mut suggestion: Option<String> = None;
        let mut want_paste = false;
        let mut pick_files = false;

        egui::Panel::bottom("input_bar").show(ui, |ui| {
            ui.add_space(4.0);

            if !typing.is_empty() {
                ui.horizontal(|ui| {
                    ui.weak(typing_label(&typing));
                });
            }

            if let Some(reply) = &self.replying_to {
                ui.horizontal(|ui| {
                    ui.weak(format!("Replying to {}: {}", reply.author, reply.preview));
                    if ui.small_button("cancel").clicked() {
                        cancel_reply = true;
                    }
                });
            }
            if self.editing.is_some() {
                ui.horizontal(|ui| {
                    ui.weak("Editing message");
                    if ui.small_button("cancel").clicked() {
                        cancel_edit = true;
                    }
                });
            }

            if !self.pending_files.is_empty() {
                let mut remove: Option<usize> = None;
                ui.horizontal_wrapped(|ui| {
                    for (i, f) in self.pending_files.iter().enumerate() {
                        let bytes = f.data.len() as f64;
                        let human = if bytes >= 1024.0 * 1024.0 {
                            format!("{:.1} MB", bytes / (1024.0 * 1024.0))
                        } else {
                            format!("{:.0} KB", bytes / 1024.0)
                        };
                        let big = bytes > 10.0 * 1024.0 * 1024.0;
                        let mut text =
                            RichText::new(format!("\u{1F4CE} {} ({human})  \u{2715}", f.filename));
                        if big {
                            text = text.color(egui::Color32::from_rgb(240, 140, 120));
                        }
                        let resp = ui.small_button(text);
                        let resp = if big {
                            resp.on_hover_text("Over the usual 10 MB limit; may be rejected")
                        } else {
                            resp.on_hover_text("Remove attachment")
                        };
                        if resp.clicked() {
                            remove = Some(i);
                        }
                    }
                });
                if let Some(i) = remove {
                    self.pending_files.remove(i);
                }
            }

            let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
            if escape {
                cancel_edit = true;
                cancel_reply = true;
            }
            let send_key =
                ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::V)) {
                want_paste = true;
            }

            let hint = if self.editing.is_some() {
                "Edit message (Enter to save)"
            } else {
                "Message"
            };
            let label = if self.editing.is_some() { "Save" } else { "Send" };

            ui.horizontal(|ui| {
                let spacing = ui.spacing().item_spacing.x;
                let button_w = 64.0;
                let emoji_w = 30.0;
                let clip_w = 30.0;
                let text_w = (ui.available_width()
                    - button_w
                    - emoji_w
                    - clip_w
                    - spacing * 3.0)
                    .max(80.0);
                if ui
                    .add_enabled(enabled, egui::Button::new("\u{1F4CE}").min_size(egui::vec2(clip_w, 24.0)))
                    .on_hover_text("Attach a file")
                    .clicked()
                {
                    pick_files = true;
                }
                let emoji_url = twemoji_url("\u{1F600}");
                let emoji_btn = if let Some(u) = &emoji_url {
                    egui::Button::image(
                        egui::Image::new(u).fit_to_exact_size(egui::vec2(20.0, 20.0)),
                    )
                    .min_size(egui::vec2(emoji_w, 24.0))
                } else {
                    egui::Button::new("\u{1F600}").min_size(egui::vec2(emoji_w, 24.0))
                };
                if ui
                    .add_enabled(enabled, emoji_btn)
                    .on_hover_text("Insert emoji")
                    .clicked()
                {
                    open_picker = true;
                }
                let resp = ui.add_enabled(
                    enabled,
                    egui::TextEdit::multiline(&mut self.draft)
                        .desired_rows(1)
                        .desired_width(text_w)
                        .hint_text(hint),
                );
                if self.focus_input {
                    resp.request_focus();
                    self.focus_input = false;
                }
                if enabled && resp.changed() {
                    if let Some(cid) = channel_id.clone() {
                        let now = Instant::now();
                        let due = self
                            .typing_sent
                            .map(|t| now.duration_since(t) > Duration::from_secs(6))
                            .unwrap_or(true);
                        if due {
                            self.typing_sent = Some(now);
                            let _ = self.cmd_tx.send(Command::TriggerTyping(cid));
                        }
                    }
                }
                let h = resp.rect.height();
                let can_send =
                    enabled && (!self.draft.trim().is_empty() || !self.pending_files.is_empty());
                let btn = egui::Button::new(label).min_size(egui::vec2(button_w, h));
                if ui.add_enabled(can_send, btn).clicked() || send_key {
                    submit = true;
                }
            });
            if enabled {
                if let Some((start, q)) = active_shortcode(&self.draft) {
                    let matches: Vec<&(&str, &str)> = EMOJIS
                        .iter()
                        .filter(|(n, _)| n.starts_with(&q))
                        .take(8)
                        .collect();
                    if !matches.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            for (name, ch) in matches {
                                if ui.button(format!(":{name}:")).clicked() {
                                    let mut d = self.draft.clone();
                                    d.replace_range(start.., ch);
                                    suggestion = Some(d);
                                }
                            }
                        });
                    }
                }
            }
            if enabled {
                if let Some((start, kind, query)) = active_autocomplete(&self.draft) {
                    let ql = query.to_lowercase();
                    let mut matches: Vec<(String, String, bool)> = Vec::new();
                    if kind == '@' {
                        for special in ["@everyone", "@here"] {
                            if query.is_empty() || special[1..].starts_with(&ql) {
                                matches.push((String::new(), special.to_string(), true));
                            }
                        }
                    }
                    let cands = if kind == '@' {
                        self.autocomplete_users()
                    } else {
                        self.autocomplete_channels()
                    };
                    for (id, label) in cands {
                        if query.is_empty() || label.to_lowercase().starts_with(&ql) {
                            matches.push((id, label, false));
                        }
                        if matches.len() >= 10 {
                            break;
                        }
                    }
                    matches.truncate(8);
                    if !matches.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            for (id, label, special) in matches {
                                let text = if special {
                                    label.clone()
                                } else if kind == '@' {
                                    format!("@{label}")
                                } else {
                                    format!("#{label}")
                                };
                                if ui.button(text).clicked() {
                                    let insert = if special {
                                        format!("{label} ")
                                    } else if kind == '@' {
                                        format!("<@{id}> ")
                                    } else {
                                        format!("<#{id}> ")
                                    };
                                    let mut d = self.draft.clone();
                                    d.replace_range(start.., &insert);
                                    suggestion = Some(d);
                                }
                            }
                        });
                    }
                }
            }
            ui.add_space(4.0);
        });

        if cancel_edit {
            if self.editing.take().is_some() {
                self.draft.clear();
            }
        }
        if cancel_reply {
            self.replying_to = None;
        }
        if open_picker {
            self.emoji_picker = Some(EmojiPicker {
                target: EmojiTarget::Composer,
                search: String::new(),
                focused: false,
            });
        }
        if let Some(d) = suggestion {
            self.draft = d;
            self.focus_input = true;
        }
        if want_paste && self.stage_clipboard_image() {
            self.focus_input = true;
        }
        if pick_files {
            let picked = rfd::FileDialog::new().pick_files().unwrap_or_default();
            self.stage_paths(picked);
            if !self.pending_files.is_empty() {
                self.focus_input = true;
            }
        }

        if submit && enabled {
            let content = self.draft.trim().to_string();
            let files = std::mem::take(&mut self.pending_files);
            if content.is_empty() && files.is_empty() {
                return;
            }
            let reply = self
                .replying_to
                .as_ref()
                .filter(|r| channel_id.as_deref() == Some(r.channel_id.as_str()))
                .map(|r| r.message_id.clone());
            if let Some(edit) = self.editing.take() {
                let _ = self.cmd_tx.send(Command::EditMessage {
                    channel_id: edit.channel_id,
                    message_id: edit.message_id,
                    content,
                });
                // Editing never carries attachments; keep them staged.
                self.pending_files = files;
            } else if let Some(cid) = channel_id {
                if files.is_empty() {
                    let _ = self.cmd_tx.send(Command::SendMessage {
                        channel_id: cid,
                        content,
                        reply_to: reply,
                    });
                } else {
                    let _ = self.cmd_tx.send(Command::SendAttachments {
                        channel_id: cid,
                        content,
                        files,
                        reply_to: reply,
                    });
                }
            }
            self.replying_to = None;
            self.draft.clear();
        }
    }

    /// If nothing is selected yet, pick the first guild and its first text channel.
    fn maybe_auto_select(&mut self) {
        if self.auto_selected {
            return;
        }
        let mut fetch_channels: Option<String> = None;
        let mut fetch_messages: Option<String> = None;
        {
            let mut s = self.shared.lock().unwrap();
            if !matches!(s.status, ConnectionStatus::Connected) || s.guilds.is_empty() {
                return;
            }
            let guild_id = s
                .selected_guild
                .clone()
                .unwrap_or_else(|| s.guilds[0].id.clone());
            s.selected_guild = Some(guild_id.clone());

            if !s.has_channels(&guild_id) {
                if !self.auto_channels_requested {
                    fetch_channels = Some(guild_id.clone());
                    self.auto_channels_requested = true;
                }
            } else {
                if s.selected_channel.is_none() {
                    if let Some(channel) = s.text_channels(&guild_id).into_iter().next() {
                        s.selected_channel = Some(channel.id.clone());
                        fetch_messages = Some(channel.id);
                    }
                }
                self.auto_selected = true;
            }
        }
        if let Some(id) = fetch_channels {
            let _ = self.cmd_tx.send(Command::FetchGuildChannels(id));
        }
        if let Some(id) = fetch_messages {
            let _ = self.cmd_tx.send(Command::FetchMessages(id));
        }
    }
}

impl App {
    fn needs_animation(&self) -> bool {
        if self.emoji_picker.is_some() || self.image_view.is_some() {
            return true;
        }
        let s = self.shared.lock().unwrap();
        if matches!(
            s.status,
            ConnectionStatus::Connecting | ConnectionStatus::Reconnecting
        ) {
            return true;
        }
        if !s.loading_older.is_empty() {
            return true;
        }
        let now = std::time::Instant::now();
        s.typing.iter().any(|(_, v)| v.iter().any(|t| t.until > now))
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.needs_animation() {
            let ms = if self.settings.performance_mode { 400 } else { 150 };
            ui.ctx().request_repaint_after(Duration::from_millis(ms));
        }
        {
            let mut s = self.shared.lock().unwrap();
            if s.repaint.is_none() {
                s.repaint = Some(ui.ctx().clone());
            }
        }
        self.shared
            .lock()
            .unwrap()
            .set_message_cap(if self.settings.performance_mode { 120 } else { 300 });
        let visuals = self.settings.theme_dark;
        if self.visuals_dark != Some(visuals) {
            self.visuals_dark = Some(visuals);
            if visuals {
                ui.ctx().set_visuals(egui::Visuals::dark());
            } else {
                ui.ctx().set_visuals(egui::Visuals::light());
            }
        }

        let logged_in = self.shared.lock().unwrap().me.is_some();
        if logged_in {
            let unread = self.shared.lock().unwrap().total_unread();
            let title = if unread > 0 {
                format!("Discord ({unread})")
            } else {
                "Discord".to_string()
            };
            // Only touch the OS window title when it actually changes.
            if title != self.last_title {
                self.last_title = title.clone();
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Title(title));
            }
            self.ui_chat(ui);
        } else {
            self.ui_login(ui);
        }
        if logged_in {
            self.handle_dropped_files(ui.ctx());
            self.reap_channel_memory(ui.ctx());
            self.pump_notices(ui.ctx());
            self.render_toasts(ui.ctx());
        }
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render_message(
    ui: &mut egui::Ui,
    message: &Message,
    prev: Option<&Message>,
    me_id: Option<&str>,
    s: &SharedState,
    settings: &config::Settings,
    action: &mut Option<MsgAction>,
    rs: &mut RenderState,
) {
    let time_fmt = if settings.time_24h { "%H:%M" } else { "%I:%M %p" };
    let ts = parse_ts(&message.timestamp);
    let new_day = match prev.and_then(|p| parse_ts(&p.timestamp)) {
        Some(p) => p.date_naive() != ts.map(|t| t.date_naive()).unwrap_or_default(),
        None => true,
    };
    if new_day {
        if let Some(t) = ts {
            ui.add_space(8.0);
            let pill = RichText::new(t.format("%B %d, %Y").to_string())
                .small()
                .color(egui::Color32::from_gray(170))
                .background_color(egui::Color32::from_gray(48));
            ui.vertical_centered(|ui| {
                ui.label(pill);
            });
            ui.add_space(6.0);
        }
    }

    let grouped = prev
        .map(|p| {
            p.author.id == message.author.id
                && p.message_reference.is_none()
                && message.message_reference.is_none()
                && match (parse_ts(&p.timestamp), ts) {
                    (Some(a), Some(b)) => {
                        a.date_naive() == b.date_naive() && (b - a).num_minutes().abs() < 5
                    }
                    _ => false,
                }
        })
        .unwrap_or(false);

    let scope = scope_for_channel(s, &message.channel_id);
    let author_name = display_name_for(s, settings, &scope, &message.author);
    let gap = if grouped { 1.0 } else { 6.0 };

    let mut open_profile: Option<String> = None;
    let mut avatar_resp: Option<egui::Response> = None;
    let mut name_resp: Option<egui::Response> = None;
    let mut open_image: Option<String> = None;

    let prev_rect = rs.rects.get(&message.id).copied();
    let radial_busy = rs.right_hold.as_ref().map(|h| h.activated).unwrap_or(false);
    // Right-/hold-click overlay. Registered *before* the message widgets so it
    // sits at the bottom of the z-order: hyperlinks, buttons and images placed
    // afterwards receive their clicks normally, while the overlay catches
    // right-clicks and presses on the empty parts of the row.
    let low = prev_rect.map(|p| {
        ui.interact(
            p,
            egui::Id::new(("msg_bg", &message.id)),
            egui::Sense::click(),
        )
    });
    let pane = ui.clip_rect();
    let right_edge = pane.right();
    let hovered = prev_rect
        .map(|r| {
            // The message is "hovered" for the full row width (through the right
            // edge of the pane), so the hover toolbar stays visible while the
            // cursor travels from the text out to the toolbar buttons.
            ui.rect_contains_pointer(egui::Rect::from_min_max(
                egui::pos2(pane.left(), r.min.y),
                egui::pos2(right_edge, r.max.y),
            ))
        })
        .unwrap_or(false);
    let is_mention = me_id != Some(message.author.id.as_str()) && s.mentions_me(message);
    if is_mention {
        if let Some(r) = prev_rect {
            let x = pane.left();
            let top = r.min.y - 3.0;
            let bottom = r.max.y + 3.0;
            ui.painter().rect_filled(
                egui::Rect::from_min_max(egui::pos2(x, top), egui::pos2(right_edge, bottom)),
                0.0,
                egui::Color32::from_rgba_unmultiplied(88, 101, 242, 22),
            );
            ui.painter().rect_filled(
                egui::Rect::from_min_max(egui::pos2(x, top), egui::pos2(x + 2.0, bottom)),
                0.0,
                egui::Color32::from_rgb(88, 101, 242),
            );
        }
    }
    if hovered && !radial_busy {
        if let Some(r) = prev_rect {
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(pane.left(), r.min.y - 3.0),
                    egui::pos2(pane.right(), r.max.y + 3.0),
                ),
                0.0,
                egui::Color32::from_black_alpha(40),
            );
        }
    }

    let inner = ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        if !grouped && !settings.compact {
            ui.horizontal(|ui| {
                if settings.show_avatars {
                    let url = message.author.avatar_url(64);
                    let st = s.presence_of(&message.author.id);
                    let ring_status = if st == "offline" { None } else { Some(st) };
                    let img = avatar_ring(ui, &url, 34.0, ring_status);
                    if img.clicked() {
                        open_profile = Some(message.author.id.clone());
                    }
                    avatar_resp = Some(img);
                    ui.add_space(4.0);
                }
                let nr = ui.add(
                    egui::Label::new(RichText::new(author_name.clone()).strong())
                        .sense(egui::Sense::click()),
                );
                if nr.clicked() {
                    open_profile = Some(message.author.id.clone());
                }
                name_resp = Some(nr);
                let time_resp =
                    ui.weak(ts.map(|t| t.format(time_fmt).to_string()).unwrap_or_default());
                if let Some(t) = ts {
                    time_resp.on_hover_text(t.format("%A, %B %d, %Y %I:%M %p").to_string());
                }
                if message.edited_timestamp.is_some() {
                    ui.weak("(edited)");
                }
            });
        } else if settings.compact {
            ui.horizontal(|ui| {
                ui.weak(
                    ts.map(|t| t.format(time_fmt).to_string())
                        .unwrap_or_default(),
                );
                let nr = ui.add(
                    egui::Label::new(RichText::new(author_name.clone()).strong())
                        .sense(egui::Sense::click()),
                );
                if nr.clicked() {
                    open_profile = Some(message.author.id.clone());
                }
                name_resp = Some(nr);
            });
        }

        let body_indent = if !settings.compact && settings.show_avatars {
            46
        } else {
            0
        };
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: body_indent,
                right: 0,
                top: 0,
                bottom: 0,
            })
            .show(ui, |ui| {
                if let Some((author, preview)) = reply_preview(message, s, settings, &scope) {
                    let resp = render_reply_line(ui, &author, &preview);
                    let replied_id = message
                        .message_reference
                        .as_ref()
                        .and_then(|r| r.message_id.clone())
                        .or_else(|| message.referenced_message.as_ref().map(|m| m.id.clone()));
                    if resp.clicked() {
                        if let Some(mid) = replied_id {
                            *action = Some(MsgAction::JumpTo {
                                channel_id: message.channel_id.clone(),
                                message_id: mid,
                            });
                        }
                    }
                }

                if !message.content.is_empty() {
                    render_content(ui, &message.content, s, &message.id, rs);
                }
                for attachment in &message.attachments {
                    render_attachment(ui, attachment, settings.load_images, &mut open_image);
                }
                for embed in &message.embeds {
                    render_embed(ui, embed, settings.load_images, &mut open_image);
                }
                if !message.reactions.is_empty() {
                    render_reactions(ui, message, action);
                }
            });
        ui.add_space(gap);
    });
    let msg_rect = inner.response.rect;

    let guild_id = s
        .channels
        .get(&message.channel_id)
        .and_then(|c| c.guild_id.clone());
    let menu = MenuCtx {
        channel_id: message.channel_id.clone(),
        message_id: message.id.clone(),
        author_id: message.author.id.clone(),
        author: author_name.clone(),
        scope: scope.clone(),
        current_alias: s.alias(&scope, &message.author.id).unwrap_or("").to_string(),
        preview: preview_text(&message.content),
        content: message.content.clone(),
        is_mine: Some(message.author.id.as_str()) == me_id,
        guild_id,
        allow_mod: settings.moderation,
    };

    if let Some(uid) = open_profile {
        if action.is_none() {
            *action = Some(MsgAction::OpenProfile { user_id: uid });
        }
    }
    if let Some(url) = open_image {
        if action.is_none() {
            *action = Some(MsgAction::OpenImage { url });
        }
    }

    // Secondary overlay for the right-hand rail (hover toolbar area), placed on
    // top so it owns clicks in the empty strip to the right of short messages.
    let low = match low {
        Some(r) => r,
        None => ui.interact(
            msg_rect,
            egui::Id::new(("msg_bg", &message.id)),
            egui::Sense::click(),
        ),
    };
    let right_edge = pane.right();
    let band = egui::Rect::from_min_max(
        egui::pos2(right_edge - 120.0, msg_rect.top()),
        egui::pos2(right_edge, msg_rect.bottom()),
    );
    let rail = ui.interact(
        band,
        egui::Id::new(("msg_rail", &message.id)),
        egui::Sense::click(),
    );

    let sec_down = ui.input(|i| i.pointer.button_down(egui::PointerButton::Secondary));
    let sec_pressed = ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary));
    let sec_released = ui.input(|i| i.pointer.button_released(egui::PointerButton::Secondary));
    // The whole row (full pane width) counts as "over the message" so the radial
    // menu and context menu open anywhere on the row, including the empty grey
    // area to the right of the text.
    let row_rect = egui::Rect::from_min_max(
        egui::pos2(pane.left(), msg_rect.top()),
        egui::pos2(right_edge, msg_rect.bottom()),
    );
    let over_msg = ui
        .input(|i| i.pointer.interact_pos())
        .map(|p| row_rect.expand(4.0).contains(p))
        .unwrap_or(false);

    if sec_pressed && over_msg {
        let pos = ui
            .input(|i| i.pointer.interact_pos())
            .unwrap_or(low.rect.center());
        rs.radial_hover = None;
        rs.right_hold = Some(RightHold {
            msg_id: message.id.clone(),
            pos,
            start: std::time::Instant::now(),
            activated: false,
        });
    }

    let mut consumed = false;
    if let Some(hold) = rs.right_hold.as_mut() {
        if hold.msg_id == message.id {
            // Activate the moment the button is held, no long-press required:
            // a quick right-click still falls back to the context menu because
            // releasing without picking a sector does nothing.
            if !hold.activated
                && settings.radial_menu
                && hold.start.elapsed() >= std::time::Duration::from_millis(150)
            {
                hold.activated = true;
            }
            let acts = radial_actions(&menu);
            if hold.activated && sec_down {
                let more = acts.iter().position(|a| *a == RadAct::More);
                let pointer = ui.input(|i| i.pointer.interact_pos());
                let (primary, sub) = match pointer {
                    Some(p) => radial_hit(hold.pos, p, &acts, more, rs.radial_sub),
                    None => (None, None),
                };
                rs.radial_hover = primary;
                rs.radial_sub = sub;
                radial_menu(ui, hold.pos, &menu, primary, sub);
            } else if sec_down {
                rs.radial_hover = None;
                rs.radial_sub = None;
            }
        }
    }

    // Hover toolbar is drawn with the painter only (never with widgets), so it
    // cannot change the message layout and cannot cause scroll feedback loops.
    // It is anchored to the right edge of the pane so even tiny messages show
    // all buttons. Clicks are routed through the `rail` overlay by hit-testing.
    let mut toolbar: Vec<(egui::Rect, RadAct)> = Vec::new();
    if hovered && !radial_busy {
        toolbar = paint_toolbar(ui, msg_rect, right_edge, &menu);
    }
    if rail.clicked() {
        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
            if let Some((_, a)) = toolbar.iter().find(|(r, _)| r.contains(pos)) {
                if action.is_none() {
                    *action = a.msg_action(&menu);
                }
            }
        }
    }

    if sec_released {
        let mut matched = false;
        if let Some(hold) = rs.right_hold.as_ref() {
            if hold.msg_id == message.id {
                matched = true;
            }
        }
        if matched {
            if let Some(hold) = rs.right_hold.take() {
                if hold.activated {
                    consumed = true;
                    let acts = radial_actions(&menu);
                    let mut pi = rs.radial_hover.take();
                    let mut si = rs.radial_sub.take();
                    if pi.is_none() {
                        if let Some(pos) = ui.input(|i| i.pointer.latest_pos()) {
                            let more = acts.iter().position(|a| *a == RadAct::More);
                            let (p, s) = radial_hit(hold.pos, pos, &acts, more, None);
                            pi = p;
                            si = s;
                        }
                    }
                    let chosen = pi.and_then(|ix| {
                        if ix >= acts.len() {
                            return None;
                        }
                        if acts[ix] == RadAct::More {
                            si.and_then(|j| submenu_actions().get(j).copied())
                        } else {
                            Some(acts[ix])
                        }
                    });
                    if let Some(a) = chosen {
                        if a.available(&menu) {
                            apply_radial(ui, &menu, a, action);
                        }
                    }
                }
            }
        }
        if !consumed && matched && over_msg {
            if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                let over_avatar = avatar_resp
                    .as_ref()
                    .map(|r| r.rect.expand(3.0).contains(pos))
                    .unwrap_or(false)
                    || name_resp
                        .as_ref()
                        .map(|r| r.rect.expand(3.0).contains(pos))
                        .unwrap_or(false);
                if !over_avatar {
                    rs.right_menu = Some((egui::pos2(pos.x, pos.y), menu.clone()));
                }
            }
        }
    }
    if hold_stale(rs, &message.id, sec_down) {
        rs.right_hold = None;
        rs.radial_hover = None;
        rs.radial_sub = None;
    }

    if let Some(r) = avatar_resp {
        r.context_menu(|ui| user_menu(ui, &menu, action));
    }
    if let Some(r) = name_resp {
        r.context_menu(|ui| user_menu(ui, &menu, action));
    }

    rs.rects.insert(message.id.clone(), msg_rect);
}

#[derive(Clone)]
struct MenuCtx {
    channel_id: String,
    message_id: String,
    author_id: String,
    author: String,
    scope: String,
    current_alias: String,
    preview: String,
    content: String,
    is_mine: bool,
    guild_id: Option<String>,
    allow_mod: bool,
}

struct RightHold {
    msg_id: String,
    pos: egui::Pos2,
    start: std::time::Instant,
    activated: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum RadAct {
    Reply,
    React,
    Edit,
    Delete,
    CopyLink,
    Profile,
    More,
    CopyText,
    CopyId,
    CopyAuthorId,
}

impl RadAct {
    fn icon(self) -> &'static str {
        match self {
            RadAct::Reply => "\u{21A9}",
            RadAct::React => "\u{263A}",
            RadAct::Edit => "\u{270E}",
            RadAct::Delete => "\u{1F5D1}",
            RadAct::CopyLink => "\u{1F517}",
            RadAct::Profile => "\u{1F464}",
            RadAct::More => "\u{2026}",
            RadAct::CopyText => "\u{1F4CB}",
            RadAct::CopyId => "\u{1F194}",
            RadAct::CopyAuthorId => "@",
        }
    }
    fn label(self) -> &'static str {
        match self {
            RadAct::Reply => "Reply",
            RadAct::React => "React",
            RadAct::Edit => "Edit",
            RadAct::Delete => "Delete",
            RadAct::CopyLink => "Copy link",
            RadAct::Profile => "Profile",
            RadAct::More => "More",
            RadAct::CopyText => "Copy text",
            RadAct::CopyId => "Copy ID",
            RadAct::CopyAuthorId => "Copy author",
        }
    }
    fn available(self, m: &MenuCtx) -> bool {
        match self {
            RadAct::Edit | RadAct::Delete => m.is_mine,
            RadAct::CopyText => !m.content.is_empty(),
            _ => true,
        }
    }
    fn msg_action(self, m: &MenuCtx) -> Option<MsgAction> {
        match self {
            RadAct::Reply => Some(MsgAction::Reply {
                channel_id: m.channel_id.clone(),
                message_id: m.message_id.clone(),
                author: m.author.clone(),
                preview: m.preview.clone(),
            }),
            RadAct::React => Some(MsgAction::PickReaction {
                channel_id: m.channel_id.clone(),
                message_id: m.message_id.clone(),
            }),
            RadAct::Edit if m.is_mine => Some(MsgAction::Edit {
                channel_id: m.channel_id.clone(),
                message_id: m.message_id.clone(),
                content: m.content.clone(),
            }),
            RadAct::Delete if m.is_mine => Some(MsgAction::Delete {
                channel_id: m.channel_id.clone(),
                message_id: m.message_id.clone(),
            }),
            _ => None,
        }
    }
}

#[derive(Clone)]
enum ModAction {
    Kick,
    Ban,
    Timeout(u64),
    RemoveTimeout,
}

/// A pending moderation action awaiting reason (+ duration for timeouts).
struct ModDialog {
    guild_id: String,
    user_id: String,
    user_name: String,
    action: ModAction,
    /// Duration in minutes for timeouts (ignored for kick/ban/remove).
    minutes: u32,
    reason: String,
    focused: bool,
}

#[derive(Default)]
struct RenderState {
    revealed: std::collections::HashSet<String>,
    rects: std::collections::HashMap<String, egui::Rect>,
    right_hold: Option<RightHold>,
    /// Sector index the pointer is over while a radial menu is being held.
    radial_hover: Option<usize>,
    /// Sub-sector index within the nested ring under `More`.
    radial_sub: Option<usize>,
    /// Quick right-click context menu opened at a screen position.
    right_menu: Option<(egui::Pos2, MenuCtx)>,
    /// Cached parsed segments: message id -> (raw content, segments).
    seg_cache: std::collections::HashMap<String, (String, Vec<Seg>)>,
}

fn member_display_name(
    s: &SharedState,
    use_custom_names: bool,
    use_names: bool,
    scope: &str,
    m: &Member,
) -> String {
    if use_custom_names {
        if let Some(a) = s.alias(scope, &m.user.id) {
            if !a.is_empty() {
                return a.to_string();
            }
        }
    }
    if !use_names {
        if let Some(n) = &m.nick {
            if !n.is_empty() {
                return n.clone();
            }
        }
    }
    m.user.display_name()
}

fn typing_label(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [a] => format!("{a} is typing\u{2026}"),
        [a, b] => format!("{a} and {b} are typing\u{2026}"),
        [a, b, c] => format!("{a}, {b} and {c} are typing\u{2026}"),
        [a, b, rest @ ..] => {
            format!("{a}, {b} and {} others are typing\u{2026}", rest.len())
        }
    }
}

fn render_channel_rows(
    ui: &mut egui::Ui,
    s: &SharedState,
    channels: &[&crate::model::Channel],
    clicked: &mut Option<String>,
    mark_read: &mut Option<String>,
) {
    for channel in channels {
        let selected = s.selected_channel.as_deref() == Some(channel.id.as_str());
        let unread = s.channel_unread(&channel.id);
        let mention = s.channel_mentions(&channel.id) > 0;
        let text = RichText::new(format!("# {}", channel.display_name()));
        let resp = ui.selectable_label(selected, if unread > 0 { text.strong() } else { text });
        if unread > 0 {
            badge(ui, resp.rect, unread, mention);
        }
        if resp.clicked() {
            *clicked = Some(channel.id.clone());
        }
        let cid = channel.id.clone();
        let gid = channel.guild_id.clone();
        resp.context_menu(|ui| {
            if unread > 0 && ui.button("Mark as Read").clicked() {
                *mark_read = Some(cid.clone());
                ui.close();
            }
            if ui.button("Copy Channel Link").clicked() {
                let g = gid.as_deref().unwrap_or("@me");
                ui.ctx()
                    .copy_text(format!("https://discord.com/channels/{g}/{cid}"));
                ui.close();
            }
            if ui.button("Copy Channel ID").clicked() {
                ui.ctx().copy_text(cid.clone());
                ui.close();
            }
        });
    }
}

fn render_voice_rows(ui: &mut egui::Ui, channels: &[&crate::model::Channel]) {
    for channel in channels {
        ui.weak(format!("\u{1F50A} {}", channel.display_name()));
    }
}

fn active_shortcode(s: &str) -> Option<(usize, String)> {
    let idx = s.rfind(':')?;
    let after = &s[idx + 1..];
    if after.is_empty() || after.len() > 32 {
        return None;
    }
    if !after
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '+')
    {
        return None;
    }
    if idx > 0 {
        let prev = s[..idx].chars().next_back().unwrap();
        if !prev.is_whitespace() {
            return None;
        }
    }
    Some((idx, after.to_string()))
}

/// Detect an in-progress `@user` or `#channel` token at the end of the draft.
/// Returns `(start_byte_index, kind, query)`.
fn active_autocomplete(s: &str) -> Option<(usize, char, String)> {
    let idx = s.rfind(['@', '#'])?;
    let kind = s[idx..].chars().next()?;
    let after = &s[idx + 1..];
    if after.len() > 32 || after.contains('\n') {
        return None;
    }
    if !after
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return None;
    }
    if idx > 0 {
        let prev = s[..idx].chars().next_back().unwrap();
        if !prev.is_whitespace() {
            return None;
        }
    }
    Some((idx, kind, after.to_string()))
}

fn encode_png(width: usize, height: usize, rgba: &[u8]) -> Option<Vec<u8>> {
    let img = image::RgbaImage::from_raw(width as u32, height as u32, rgba.to_vec())?;
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .ok()?;
    Some(out.into_inner())
}

fn is_image_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// Decode a Windows device-independent bitmap (the bytes of CF_DIB / CF_DIBV5)
/// into top-down RGBA8. Supports 24- and 32-bpp uncompressed/bitfield data,
/// which covers screenshots and most copied images.
#[cfg(target_os = "windows")]
fn dib_to_rgba(data: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    if data.len() < 40 {
        return None;
    }
    let rd_u32 = |o: usize| -> Option<u32> {
        data.get(o..o + 4)
            .and_then(|s| s.try_into().ok())
            .map(u32::from_le_bytes)
    };
    let rd_i32 = |o: usize| -> Option<i32> {
        data.get(o..o + 4)
            .and_then(|s| s.try_into().ok())
            .map(i32::from_le_bytes)
    };
    let rd_u16 = |o: usize| -> Option<u16> {
        data.get(o..o + 2)
            .and_then(|s| s.try_into().ok())
            .map(u16::from_le_bytes)
    };
    let bi_size = rd_u32(0)? as usize;
    let width = rd_i32(4)?;
    let height = rd_i32(8)?;
    let bpp = rd_u16(14)?;
    let compression = rd_u32(16)?;
    let clr_used = rd_u32(32)? as usize;
    if width <= 0 || height == 0 {
        return None;
    }
    let w = width as usize;
    let h = height.unsigned_abs() as usize;
    let top_down = height < 0;
    let mut offset = bi_size.max(40);
    if bpp <= 8 {
        let n = if clr_used != 0 {
            clr_used
        } else {
            1usize << bpp
        };
        offset += n * 4;
    }
    if compression == 3 && bi_size == 40 {
        offset += 12;
    }
    let mut rgba = vec![0u8; w * h * 4];
    match bpp {
        32 => {
            let row = w * 4;
            if data.len() < offset + row * h {
                return None;
            }
            for y in 0..h {
                let sy = if top_down { y } else { h - 1 - y };
                let s = offset + sy * row;
                let d = y * row;
                for x in 0..w {
                    let si = s + x * 4;
                    let di = d + x * 4;
                    rgba[di] = data[si + 2];
                    rgba[di + 1] = data[si + 1];
                    rgba[di + 2] = data[si];
                    let a = data[si + 3];
                    rgba[di + 3] = if compression == 0 && a == 0 { 255 } else { a };
                }
            }
        }
        24 => {
            let row = (w * 3).div_ceil(4) * 4;
            if data.len() < offset + row * h {
                return None;
            }
            for y in 0..h {
                let sy = if top_down { y } else { h - 1 - y };
                let s = offset + sy * row;
                let d = y * w * 4;
                for x in 0..w {
                    let si = s + x * 3;
                    let di = d + x * 4;
                    rgba[di] = data[si + 2];
                    rgba[di + 1] = data[si + 1];
                    rgba[di + 2] = data[si];
                    rgba[di + 3] = 255;
                }
            }
        }
        _ => return None,
    }
    Some((w, h, rgba))
}

fn open_url(url: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn();
    }
}

fn status_color(status: &str) -> egui::Color32 {
    match status {
        "online" => egui::Color32::from_rgb(59, 165, 93),
        "idle" => egui::Color32::from_rgb(250, 166, 26),
        "dnd" => egui::Color32::from_rgb(237, 66, 69),
        _ => egui::Color32::from_rgb(128, 132, 142),
    }
}

fn presence_rank(status: &str) -> u8 {
    match status {
        "online" => 0,
        "idle" => 1,
        "dnd" => 2,
        _ => 3,
    }
}

/// Draw a circular avatar with a presence ring around it (Discord-style).
/// Returns the clickable response covering the avatar + ring.
fn avatar_ring(
    ui: &mut egui::Ui,
    url: &str,
    size: f32,
    status: Option<&str>,
) -> egui::Response {
    let ring = (size * 0.09).clamp(2.0, 4.0);
    let total = size + ring * 2.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(total, total), egui::Sense::click());
    let center = rect.center();
    if let Some(st) = status {
        let color = if st == "offline" || st.is_empty() {
            egui::Color32::from_gray(55)
        } else {
            status_color(st)
        };
        ui.painter()
            .circle_stroke(center, size / 2.0 + ring * 0.6, egui::Stroke::new(ring, color));
    }
    let avatar_rect = egui::Rect::from_center_size(center, egui::vec2(size, size));
    egui::Image::new(url)
        .fit_to_exact_size(egui::vec2(size, size))
        .corner_radius(size / 2.0)
        .paint_at(ui, avatar_rect);
    resp
}

fn badge(ui: &egui::Ui, rect: egui::Rect, count: u32, mention: bool) {
    let painter = ui.painter();
    let center = egui::pos2(rect.right() - 4.0, rect.top() + 6.0);
    let color = if mention {
        egui::Color32::from_rgb(237, 66, 69)
    } else {
        egui::Color32::from_rgb(88, 92, 100)
    };
    painter.circle_filled(center, 9.0, color);
    let text = if count > 99 {
        "99+".to_string()
    } else {
        count.to_string()
    };
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(11.0),
        egui::Color32::WHITE,
    );
}

fn radial_actions(m: &MenuCtx) -> Vec<RadAct> {
    let mut v = vec![RadAct::Reply, RadAct::React];
    if m.is_mine {
        v.push(RadAct::Edit);
        v.push(RadAct::Delete);
    }
    v.push(RadAct::CopyLink);
    v.push(RadAct::Profile);
    v.push(RadAct::More);
    v
}

/// Items revealed by the nested sub-ring behind the `More` sector.
fn submenu_actions() -> Vec<RadAct> {
    vec![RadAct::CopyText, RadAct::CopyId, RadAct::CopyAuthorId]
}

const RADIAL_R_IN: f32 = 34.0;
const RADIAL_R_OUT: f32 = 150.0;
/// Thickness of the outer fan band that the `More` sector expands into when
/// hovered; its sub-slices are appended at the outer end of the `More` slice.
const RADIAL_SUB_BAND: f32 = 58.0;

/// Mid-angle (radians) of sector `i` of `n`, matching `radial_menu`'s layout.
fn radial_mid_angle(i: usize, n: usize) -> f32 {
    (i as f32 + 0.5) * std::f32::consts::TAU / n as f32 - std::f32::consts::FRAC_PI_2
}

fn radial_index(rel: egui::Vec2, n: usize) -> usize {
    let deg = (rel.y.atan2(rel.x).to_degrees() + 360.0) % 360.0;
    let sector = 360.0 / n as f32;
    let d = (deg + 90.0) % 360.0;
    ((d / sector) as usize) % n
}

/// Resolve the hovered sector (and, when the `More` sector is engaged, the
/// nested sub-sector) for the radial menu. Returns `(primary, sub)`.
fn radial_hit(
    center: egui::Pos2,
    p: egui::Pos2,
    acts: &[RadAct],
    more: Option<usize>,
    prev_sub: Option<usize>,
) -> (Option<usize>, Option<usize>) {
    let rel = p - center;
    let len = rel.length();
    let tau = std::f32::consts::TAU;
    if let Some(mi) = more {
        let n = acts.len();
        let step = tau / n as f32;
        let a0 = mi as f32 * step - std::f32::consts::FRAC_PI_2;
        // Outer fan band: sub-slices appended at the outer end of the More slice.
        if len >= RADIAL_R_OUT && len <= RADIAL_R_OUT + RADIAL_SUB_BAND {
            let d = (rel.y.atan2(rel.x) - a0).rem_euclid(tau);
            if d < step {
                let m = submenu_actions().len();
                let ix = (((d / step) * m as f32) as usize).min(m.saturating_sub(1));
                return (Some(mi), Some(ix));
            }
        }
        // Keep the fan engaged while the cursor is over the More slice body itself.
        if prev_sub.is_some() && len <= RADIAL_R_OUT {
            let ix = if len > RADIAL_R_IN {
                radial_index(rel, n)
            } else {
                mi
            };
            if ix == mi {
                return (Some(mi), prev_sub);
            }
        }
    }
    if len > RADIAL_R_IN && len < RADIAL_R_OUT + 6.0 {
        return (Some(radial_index(rel, acts.len())), None);
    }
    (None, None)
}

/// Perform a radial action (copy-to-clipboard actions run directly; the rest
/// are forwarded as `MsgAction`s for the app to handle).
fn apply_radial(ui: &egui::Ui, m: &MenuCtx, a: RadAct, action: &mut Option<MsgAction>) {
    match a {
        RadAct::CopyLink => {
            let link = format!(
                "https://discord.com/channels/{}/{}/{}",
                m.scope, m.channel_id, m.message_id
            );
            ui.ctx().copy_text(link);
        }
        RadAct::CopyText => ui.ctx().copy_text(m.content.clone()),
        RadAct::CopyId => ui.ctx().copy_text(m.message_id.clone()),
        RadAct::CopyAuthorId => ui.ctx().copy_text(m.author_id.clone()),
        RadAct::Profile => {
            if action.is_none() {
                *action = Some(MsgAction::OpenProfile {
                    user_id: m.author_id.clone(),
                });
            }
        }
        other => {
            if action.is_none() {
                *action = other.msg_action(m);
            }
        }
    }
}

/// Draw the hover toolbar (reply / react / edit / delete) with the painter only,
/// so it never affects layout. Anchored to the right edge of the pane. Returns
/// the button rects so the caller can hit-test clicks.
fn paint_toolbar(
    ui: &egui::Ui,
    msg_rect: egui::Rect,
    right_edge: f32,
    m: &MenuCtx,
) -> Vec<(egui::Rect, RadAct)> {
    let mut acts: Vec<RadAct> = Vec::new();
    if m.is_mine {
        acts.push(RadAct::Edit);
        acts.push(RadAct::Delete);
    }
    acts.push(RadAct::Reply);
    acts.push(RadAct::React);

    let bsize = egui::vec2(26.0, 22.0);
    let y = msg_rect.top() + 2.0;
    let mut x = right_edge - 4.0;
    let clip = ui.clip_rect();
    let painter = ui
        .ctx()
        .layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("msg_hover_toolbar"),
        ))
        .with_clip_rect(clip);
    let ptr = ui.input(|i| i.pointer.interact_pos());
    let mut out = Vec::with_capacity(acts.len());
    let mut hot: Vec<(egui::Rect, &str)> = Vec::new();
    for a in acts {
        let r = egui::Rect::from_min_size(egui::pos2(x - bsize.x, y), bsize);
        x -= bsize.x + 2.0;
        let is_hot = ptr.map(|p| r.contains(p)).unwrap_or(false);
        let fill = if is_hot {
            egui::Color32::from_gray(95)
        } else {
            egui::Color32::from_gray(58)
        };
        painter.rect_filled(r, 4.0, fill);
        painter.rect_stroke(
            r,
            4.0,
            egui::Stroke::new(1.0, egui::Color32::from_gray(125)),
            egui::StrokeKind::Inside,
        );
        painter.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            a.icon(),
            egui::FontId::proportional(13.0),
            egui::Color32::WHITE,
        );
        if is_hot {
            hot.push((r, a.label()));
        }
        out.push((r, a));
    }
    // Second pass draws tooltips above (or below, near the pane top) the
    // hovered button, never overlapping sibling icons.
    let pane = clip;
    for (r, label) in hot {
        let galley = painter.layout_no_wrap(
            label.to_string(),
            egui::FontId::proportional(11.0),
            egui::Color32::WHITE,
        );
        let tw = galley.size().x + 12.0;
        let th = galley.size().y + 6.0;
        let above = r.top() - th;
        let y = if above >= pane.top() { above } else { r.bottom() };
        // Keep the tooltip fully inside the message pane so it never spills into
        // the member list on the right.
        let x = (r.center().x - tw * 0.5)
            .max(pane.left())
            .min((pane.right() - tw).max(pane.left()));
        let lr = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(tw, th));
        painter.rect_filled(lr, 4.0, egui::Color32::from_gray(42));
        painter.galley(lr.center(), galley, egui::Color32::from_gray(220));
    }
    out
}

/// Radial quick-action menu drawn as a pie: hit-testing is purely angular
/// within the ring, matching the visual sectors. When the `More` sector is
/// hovered it grows outward and its sub-actions appear as extra slices fanned
/// onto the outer end of the `More` slice (not a separate circle).
fn radial_menu(
    ui: &egui::Ui,
    center: egui::Pos2,
    m: &MenuCtx,
    hover: Option<usize>,
    sub: Option<usize>,
) {
    let acts = radial_actions(m);
    let n = acts.len() as f32;
    let r_out = RADIAL_R_OUT;
    let r_in = RADIAL_R_IN;
    let more = acts.iter().position(|a| *a == RadAct::More);
    let subs = submenu_actions();
    let tau = std::f32::consts::TAU;
    egui::Area::new(egui::Id::new("radial_menu"))
        .order(egui::Order::Foreground)
        .interactable(true)
        .show(ui.ctx(), |ui| {
            let painter = ui.painter();
            let accent = egui::Color32::from_rgb(88, 101, 242);
            let more_hot = more.is_some() && hover == more;
            let bg = if more_hot {
                r_out + RADIAL_SUB_BAND + 6.0
            } else {
                r_out + 6.0
            };
            painter.circle_filled(center, bg, egui::Color32::from_black_alpha(175));
            for i in 0..acts.len() {
                let a0 = i as f32 * tau / n - std::f32::consts::FRAC_PI_2;
                let a1 = (i + 1) as f32 * tau / n - std::f32::consts::FRAC_PI_2;
                let is_more_hot = Some(i) == more && hover == more;
                let fill = if hover == Some(i) {
                    accent
                } else {
                    egui::Color32::from_gray(44)
                };
                let rout = if is_more_hot {
                    r_out + RADIAL_SUB_BAND
                } else {
                    r_out
                };
                let steps = 20;
                let mut poly: Vec<egui::Pos2> = Vec::with_capacity(steps + 3);
                poly.push(center);
                for k in 0..=steps {
                    let a = a0 + (a1 - a0) * k as f32 / steps as f32;
                    poly.push(center + egui::vec2(a.cos() * rout, a.sin() * rout));
                }
                painter.add(egui::Shape::convex_polygon(
                    poly,
                    fill,
                    egui::Stroke::new(1.0, egui::Color32::from_gray(85)),
                ));
            }

            // Outer fan for `More`: sub-slices appended at the outer end of the
            // (now enlarged) More slice.
            if let Some(mi) = more {
                if hover == Some(mi) {
                    let step = tau / n;
                    let a0 = mi as f32 * step - std::f32::consts::FRAC_PI_2;
                    let msubs = subs.len() as f32;
                    let steps = 12;
                    for j in 0..subs.len() {
                        let b0 = a0 + step * j as f32 / msubs;
                        let b1 = a0 + step * (j + 1) as f32 / msubs;
                        let fill = if sub == Some(j) {
                            accent
                        } else {
                            egui::Color32::from_gray(58)
                        };
                        let mut poly: Vec<egui::Pos2> = Vec::with_capacity(steps * 2 + 2);
                        for k in 0..=steps {
                            let a = b0 + (b1 - b0) * k as f32 / steps as f32;
                            poly.push(center + egui::vec2(a.cos() * (r_out + RADIAL_SUB_BAND), a.sin() * (r_out + RADIAL_SUB_BAND)));
                        }
                        for k in (0..=steps).rev() {
                            let a = b0 + (b1 - b0) * k as f32 / steps as f32;
                            poly.push(center + egui::vec2(a.cos() * r_out, a.sin() * r_out));
                        }
                        painter.add(egui::Shape::convex_polygon(
                            poly,
                            fill,
                            egui::Stroke::new(1.0, egui::Color32::from_gray(90)),
                        ));
                        let bmid = (b0 + b1) * 0.5;
                        let rr = r_out + RADIAL_SUB_BAND * 0.5;
                        let p = center + egui::vec2(bmid.cos() * rr, bmid.sin() * rr);
                        let a = subs[j];
                        painter.text(
                            p - egui::vec2(0.0, 8.0),
                            egui::Align2::CENTER_CENTER,
                            a.icon(),
                            egui::FontId::proportional(16.0),
                            egui::Color32::WHITE,
                        );
                        painter.text(
                            p + egui::vec2(0.0, 12.0),
                            egui::Align2::CENTER_CENTER,
                            a.label(),
                            egui::FontId::proportional(10.0),
                            egui::Color32::from_gray(205),
                        );
                    }
                }
            }

            painter.circle_filled(center, r_in - 4.0, egui::Color32::from_gray(52));
            for (i, a) in acts.iter().enumerate() {
                let amid = radial_mid_angle(i, acts.len());
                let p = center
                    + egui::vec2(
                        amid.cos() * (r_in + r_out) * 0.55,
                        amid.sin() * (r_in + r_out) * 0.55,
                    );
                painter.text(
                    p - egui::vec2(0.0, 9.0),
                    egui::Align2::CENTER_CENTER,
                    a.icon(),
                    egui::FontId::proportional(20.0),
                    egui::Color32::WHITE,
                );
                painter.text(
                    p + egui::vec2(0.0, 13.0),
                    egui::Align2::CENTER_CENTER,
                    a.label(),
                    egui::FontId::proportional(11.0),
                    egui::Color32::from_gray(205),
                );
            }
        });
}

fn hold_stale(rs: &RenderState, msg_id: &str, sec_down: bool) -> bool {
    match &rs.right_hold {
        Some(h) if h.msg_id == msg_id && !sec_down => true,
        _ => false,
    }
}

/// Make menu entries flat (no grey button backgrounds), keeping a subtle hover
/// highlight, matching the Discord context-menu look.
fn flatten_menu(ui: &mut egui::Ui) {
    let sel = ui.visuals().selection.bg_fill;
    let w = &mut ui.style_mut().visuals.widgets;
    w.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
    w.inactive.bg_stroke = egui::Stroke::NONE;
    w.noninteractive.weak_bg_fill = egui::Color32::TRANSPARENT;
    w.hovered.weak_bg_fill = sel.gamma_multiply(0.6);
    w.hovered.bg_stroke = egui::Stroke::NONE;
    w.active.weak_bg_fill = sel;
}

fn message_menu(ui: &mut egui::Ui, m: &MenuCtx, action: &mut Option<MsgAction>) {
    flatten_menu(ui);
    if ui.button("Reply").clicked() {
        *action = Some(MsgAction::Reply {
            channel_id: m.channel_id.clone(),
            message_id: m.message_id.clone(),
            author: m.author.clone(),
            preview: m.preview.clone(),
        });
        ui.close();
    }
    if ui.button("Add Reaction...").clicked() {
        *action = Some(MsgAction::PickReaction {
            channel_id: m.channel_id.clone(),
            message_id: m.message_id.clone(),
        });
        ui.close();
    }
    if m.is_mine {
        if ui.button("Edit").clicked() {
            *action = Some(MsgAction::Edit {
                channel_id: m.channel_id.clone(),
                message_id: m.message_id.clone(),
                content: m.content.clone(),
            });
            ui.close();
        }
        if ui.button("Delete").clicked() {
            *action = Some(MsgAction::Delete {
                channel_id: m.channel_id.clone(),
                message_id: m.message_id.clone(),
            });
            ui.close();
        }
    }
    ui.separator();
    if !m.content.is_empty() {
        if ui.button("Copy Text").clicked() {
            ui.ctx().copy_text(m.content.clone());
            ui.close();
        }
    }
    if ui.button("Copy Message ID").clicked() {
        ui.ctx().copy_text(m.message_id.clone());
        ui.close();
    }
    if ui.button("Copy Message Link").clicked() {
        let link = format!(
            "https://discord.com/channels/{}/{}/{}",
            m.scope, m.channel_id, m.message_id
        );
        ui.ctx().copy_text(link);
        ui.close();
    }
}

fn user_menu(ui: &mut egui::Ui, m: &MenuCtx, action: &mut Option<MsgAction>) {
    flatten_menu(ui);
    if ui.button("View Profile").clicked() {
        *action = Some(MsgAction::OpenProfile {
            user_id: m.author_id.clone(),
        });
        ui.close();
    }
    if ui.button("Message").clicked() {
        *action = Some(MsgAction::OpenDm {
            user_id: m.author_id.clone(),
        });
        ui.close();
    }
    if ui.button("Set Nickname...").clicked() {
        *action = Some(MsgAction::SetNickname {
            scope: m.scope.clone(),
            user_id: m.author_id.clone(),
            display: m.author.clone(),
            current: m.current_alias.clone(),
        });
        ui.close();
    }
    ui.separator();
    if ui.button("Copy User ID").clicked() {
        ui.ctx().copy_text(m.author_id.clone());
        ui.close();
    }
    if let Some(guild_id) = &m.guild_id {
        if !m.is_mine && m.allow_mod {
            ui.separator();
            if ui.button("Kick").clicked() {
                *action = Some(MsgAction::Moderate {
                    guild_id: guild_id.clone(),
                    user_id: m.author_id.clone(),
                    user_name: m.author.clone(),
                    action: ModAction::Kick,
                });
                ui.close();
            }
            if ui.button("Ban").clicked() {
                *action = Some(MsgAction::Moderate {
                    guild_id: guild_id.clone(),
                    user_id: m.author_id.clone(),
                    user_name: m.author.clone(),
                    action: ModAction::Ban,
                });
                ui.close();
            }
            if ui.button("Timeout...").clicked() {
                *action = Some(MsgAction::Moderate {
                    guild_id: guild_id.clone(),
                    user_id: m.author_id.clone(),
                    user_name: m.author.clone(),
                    action: ModAction::Timeout(0),
                });
                ui.close();
            }
            if ui.button("Remove Timeout").clicked() {
                *action = Some(MsgAction::Moderate {
                    guild_id: guild_id.clone(),
                    user_id: m.author_id.clone(),
                    user_name: m.author.clone(),
                    action: ModAction::RemoveTimeout,
                });
                ui.close();
            }
        }
    }
}

fn reply_preview(
    message: &Message,
    s: &SharedState,
    settings: &config::Settings,
    scope: &str,
) -> Option<(String, String)> {
    if let Some(referenced) = &message.referenced_message {
        return Some((
            display_name_for(s, settings, scope, &referenced.author),
            preview_text(&referenced.content),
        ));
    }
    if let Some(reference) = &message.message_reference {
        if let Some(id) = &reference.message_id {
            if let Some(orig) = s.message(&message.channel_id, id) {
                return Some((
                    display_name_for(s, settings, scope, &orig.author),
                    preview_text(&orig.content),
                ));
            }
            return Some(("original message".to_string(), String::new()));
        }
    }
    None
}

/// The alias scope for a channel: its guild id, or `DM_HOME` for DMs.
fn scope_for_channel(s: &SharedState, channel_id: &str) -> String {
    s.channels
        .get(channel_id)
        .and_then(|c| c.guild_id.clone())
        .unwrap_or_else(|| DM_HOME.to_string())
}

/// Preferred name for a user: raw username when the setting is on, otherwise
/// the (possibly aliased) display name for the scope.
fn display_name_for(
    s: &SharedState,
    settings: &config::Settings,
    scope: &str,
    user: &User,
) -> String {
    if settings.use_custom_names {
        if let Some(a) = s.alias(scope, &user.id) {
            if !a.is_empty() {
                return a.to_string();
            }
        }
    }
    if settings.use_names {
        user.username.clone()
    } else {
        user.display_name()
    }
}

/// Play the Windows notification sound. Best-effort and silent on failure.
#[cfg(windows)]
fn play_notification_sound() {
    const SND_ALIAS: u32 = 0x0001_0000;
    const SND_ASYNC: u32 = 0x0001;
    #[link(name = "winmm")]
    extern "system" {
        fn PlaySoundW(psz_sound: *const u16, hmod: *mut core::ffi::c_void, fdw_sound: u32) -> i32;
    }
    let alias: Vec<u16> = "SystemNotification\0".encode_utf16().collect();
    unsafe {
        if PlaySoundW(alias.as_ptr(), std::ptr::null_mut(), SND_ALIAS | SND_ASYNC) == 0 {
            const SND_ALIAS_ID: u32 = 0x0011_0000;
            let fallback: Vec<u16> = "SystemAsterisk\0".encode_utf16().collect();
            PlaySoundW(fallback.as_ptr(), std::ptr::null_mut(), SND_ALIAS | SND_ASYNC);
            let _ = SND_ALIAS_ID;
        }
    }
}

#[cfg(not(windows))]
fn play_notification_sound() {}

/// Compact window title bar: small title on the left, close button on the
/// right. Returns `true` when the user clicked close.
fn window_header(ui: &mut egui::Ui, title: &str) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).size(13.0).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("x").clicked() {
                close = true;
            }
        });
    });
    close
}

/// Discord-style reply indicator: a short accent bar followed by "author: preview".
fn render_reply_line(ui: &mut egui::Ui, author: &str, preview: &str) -> egui::Response {
    let resp = ui
        .horizontal(|ui| {
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(3.0, 16.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, 1.5, egui::Color32::from_gray(120));
            ui.add_space(4.0);
            let text = if preview.is_empty() {
                author.to_string()
            } else {
                format!("{author}: {preview}")
            };
            ui.weak(text);
        })
        .response;
    resp.interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn render_reactions(ui: &mut egui::Ui, message: &Message, action: &mut Option<MsgAction>) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        for r in &message.reactions {
            let resp = match r.emoji.image_url() {
                Some(url) => ui.add(egui::Button::selectable(
                    r.me,
                    (
                        egui::Image::new(&url).max_height(16.0).max_width(16.0),
                        format!(" {}", r.count),
                    ),
                )),
                None => {
                    let name = r.emoji.name.clone().unwrap_or_default();
                    match twemoji_url(&name) {
                        Some(url) => ui.add(egui::Button::selectable(
                            r.me,
                            (
                                egui::Image::new(&url)
                                    .fit_to_exact_size(egui::vec2(16.0, 16.0)),
                                format!(" {}", r.count),
                            ),
                        )),
                        None => ui.add(egui::Button::selectable(
                            r.me,
                            format!("{} {}", name, r.count),
                        )),
                    }
                }
            }
            .on_hover_text(r.emoji.label());
            if resp.clicked() && action.is_none() {
                *action = Some(MsgAction::ToggleReaction {
                    channel_id: message.channel_id.clone(),
                    message_id: message.id.clone(),
                    emoji: r.emoji.clone(),
                });
            }
        }
    });
}

/// Fit a (width, height) image into `max`, preserving aspect ratio.
fn image_fit(w: u32, h: u32, max: egui::Vec2) -> egui::Vec2 {
    let (w, h) = (w as f32, h as f32);
    if w <= 0.0 || h <= 0.0 {
        return egui::vec2(max.x.min(200.0), max.y.min(120.0));
    }
    let s = (max.x / w).min(max.y / h).min(1.0);
    egui::vec2(w * s, h * s)
}

/// Extract a YouTube video id from a broadcast URL, when possible.
fn youtube_id(url: &str) -> Option<String> {
    if !(url.contains("youtube.com") || url.contains("youtu.be")) {
        return None;
    }
    if let Some((_, rest)) = url.split_once("watch?v=") {
        let id: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if id.len() == 11 {
            return Some(id);
        }
    }
    for marker in ["youtu.be/", "/shorts/", "/embed/"] {
        if let Some((_, rest)) = url.split_once(marker) {
            let id: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if id.len() == 11 {
                return Some(id);
            }
        }
    }
    None
}

fn render_attachment(
    ui: &mut egui::Ui,
    attachment: &Attachment,
    load_images: bool,
    open_image: &mut Option<String>,
) {
    if attachment.is_image() {
        if !load_images {
            if ui
                .button(format!("\u{1F5BC} {} (click to view)", attachment.filename))
                .clicked()
            {
                *open_image = attachment.image_url(1600).or_else(|| attachment.url.clone());
            }
            return;
        }
        if let Some(url) = attachment.image_url(400) {
            let resp = if let (Some(w), Some(h)) = (attachment.width, attachment.height) {
                // Fixed size from the attachment metadata: the widget keeps its
                // size before the texture loads, so nothing reflows/jumps.
                let sz = image_fit(w, h, egui::vec2(400.0, 320.0));
                ui.add(
                    egui::Image::new(&url)
                        .fit_to_exact_size(sz)
                        .corner_radius(6.0)
                        .sense(egui::Sense::click()),
                )
            } else {
                ui.add(
                    egui::Image::new(&url)
                        .max_width(400.0)
                        .max_height(320.0)
                        .corner_radius(6.0)
                        .sense(egui::Sense::click()),
                )
            };
            if resp.clicked() {
                if let Some(full) = attachment.image_url(1600).or_else(|| attachment.url.clone()) {
                    *open_image = Some(full);
                }
            }
        }
    } else if let Some(url) = &attachment.url {
        ui.hyperlink_to(format!("\u{1F4CE} {}", attachment.filename), url);
    } else {
        ui.weak(format!("\u{1F4CE} {}", attachment.filename));
    }
}

fn render_embed(
    ui: &mut egui::Ui,
    embed: &Embed,
    load_images: bool,
    open_image: &mut Option<String>,
) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_max_width(440.0);
        if let Some(title) = &embed.title {
            if let Some(url) = &embed.url {
                ui.hyperlink_to(RichText::new(title).strong(), url);
            } else {
                ui.label(RichText::new(title).strong());
            }
        }
        if let Some(desc) = &embed.description {
            if !desc.is_empty() {
                ui.label(desc);
            }
        }
        if let Some(img) = &embed.image {
            if let Some(url) = img.best_url() {
                if load_images {
                    let sz = match (img.width, img.height) {
                        (Some(w), Some(h)) => image_fit(w, h, egui::vec2(420.0, 300.0)),
                        _ => egui::vec2(320.0, 220.0),
                    };
                    let resp = ui.add(
                        egui::Image::new(&url)
                            .fit_to_exact_size(sz)
                            .corner_radius(6.0)
                            .sense(egui::Sense::click()),
                    );
                    if resp.clicked() {
                        *open_image = Some(url);
                    }
                } else if ui.button("\u{1F5BC} embedded image (click to view)").clicked() {
                    *open_image = Some(url);
                }
            }
        }
        if let Some(thumb) = &embed.thumbnail {
            let is_video = embed.kind.as_deref() == Some("video") || embed.video.is_some();
            let mut thumbnail_url = thumb.best_url();
            if is_video {
                // YouTube thumbnails are flaky through Discord's proxy; build a
                // direct i.ytimg.com URL from the video id instead.
                if let Some(id) = embed.url.as_deref().and_then(youtube_id) {
                    thumbnail_url = Some(format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg"));
                }
            }
            if load_images {
                if let Some(url) = thumbnail_url {
                    let max = if is_video {
                        egui::vec2(380.0, 260.0)
                    } else {
                        egui::vec2(120.0, 120.0)
                    };
                    let sz = match (thumb.width, thumb.height) {
                        (Some(w), Some(h)) => image_fit(w, h, max),
                        _ => if is_video { egui::vec2(380.0, 214.0) } else { egui::vec2(120.0, 90.0) },
                    };
                    let mut resp = ui.add(
                        egui::Image::new(&url)
                            .fit_to_exact_size(sz)
                            .corner_radius(6.0)
                            .sense(egui::Sense::click()),
                    );
                    if resp.clicked() {
                        if let Some(u) = &embed.url {
                            open_url(u);
                        }
                    }
                    if is_video {
                        let label = match embed.provider.as_ref().and_then(|p| p.name.clone()) {
                            Some(n) => format!("Play on {n}"),
                            None => "Play video".to_string(),
                        };
                        resp = resp.on_hover_text(label);
                    } else if let Some(u) = &embed.url {
                        resp = resp.on_hover_text(u.clone());
                    }
                    let _ = resp;
                }
            } else if is_video {
                if let Some(u) = &embed.url {
                    if ui.button("\u{25B6} Watch").clicked() {
                        open_url(u);
                    }
                }
            }
        }
        if let Some(v) = &embed.video {
            if let Some(u) = &v.url {
                ui.hyperlink_to("Watch", u);
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Lightweight markdown / mention rendering
// ---------------------------------------------------------------------------

enum EmojiTok {
    Unicode(String),
    Custom(String),
}

fn is_emoji_char(c: char) -> bool {
    let u = c as u32;
    matches!(
        u,
        0x1F000..=0x1FAFF
            | 0x2600..=0x27BF
            | 0x2B00..=0x2BFF
            | 0x2190..=0x21FF
            | 0x2300..=0x23FF
            | 0x2100..=0x214F
            | 0x200D
            | 0xFE0F
            | 0x20E3
    )
}

fn is_emoji_base(c: char) -> bool {
    let u = c as u32;
    is_emoji_char(c) && !matches!(u, 0xFE0F | 0x200D | 0x20E3)
}

fn is_regional_indicator(c: char) -> bool {
    let u = c as u32;
    (0x1F1E6..=0x1F1FF).contains(&u)
}

fn twemoji_url(s: &str) -> Option<String> {
    if s.is_empty() {
        return None;
    }
    let mut out = String::new();
    for c in s.chars() {
        let cp = c as u32;
        if cp == 0xFE0F {
            continue;
        }
        if !out.is_empty() {
            out.push('-');
        }
        out.push_str(&format!("{cp:x}"));
    }
    if out.is_empty() {
        return None;
    }
    Some(format!(
        "https://cdn.jsdelivr.net/gh/jdecked/twemoji@15.1.0/assets/72x72/{out}.png"
    ))
}

enum EmojiPiece {
    Text(String),
    Emoji(String),
}

/// Split a string into runs of plain text and emoji sequences, so emoji can be
/// rendered as Twemoji images rather than relying on a system emoji font.
fn split_emoji(s: &str) -> Vec<EmojiPiece> {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut out = Vec::new();
    let mut text = String::new();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if is_regional_indicator(c) {
            if !text.is_empty() {
                out.push(EmojiPiece::Text(std::mem::take(&mut text)));
            }
            let mut e = String::new();
            e.push(c);
            i += 1;
            if i < n && is_regional_indicator(chars[i]) {
                e.push(chars[i]);
                i += 1;
            }
            out.push(EmojiPiece::Emoji(e));
        } else if is_emoji_base(c) {
            if !text.is_empty() {
                out.push(EmojiPiece::Text(std::mem::take(&mut text)));
            }
            let mut e = String::new();
            e.push(c);
            i += 1;
            loop {
                if i >= n {
                    break;
                }
                let nxt = chars[i] as u32;
                if nxt == 0x200D {
                    e.push(chars[i]);
                    i += 1;
                    if i < n {
                        e.push(chars[i]);
                        i += 1;
                    }
                } else if matches!(nxt, 0xFE0F | 0x20E3 | 0x1F3FB..=0x1F3FF) {
                    e.push(chars[i]);
                    i += 1;
                } else {
                    break;
                }
            }
            out.push(EmojiPiece::Emoji(e));
        } else {
            text.push(c);
            i += 1;
        }
    }
    if !text.is_empty() {
        out.push(EmojiPiece::Text(text));
    }
    out
}

fn render_plain(ui: &mut egui::Ui, text: &str) {
    if text.is_empty() {
        return;
    }
    for piece in split_emoji(text) {
        match piece {
            EmojiPiece::Text(t) => {
                ui.label(t);
            }
            EmojiPiece::Emoji(e) => match twemoji_url(&e) {
                Some(url) => {
                    ui.add(
                        egui::Image::new(&url)
                            .fit_to_exact_size(egui::vec2(17.0, 17.0)),
                    );
                }
                None => {
                    ui.label(e);
                }
            },
        }
    }
}

fn emoji_button(ui: &mut egui::Ui, ch: &str, size: f32) -> egui::Response {
    match twemoji_url(ch) {
        Some(url) => ui.add(egui::Button::image(
            egui::Image::new(&url).fit_to_exact_size(egui::vec2(size, size)),
        )),
        None => ui.add(egui::Button::new(ch)),
    }
}

fn custom_emoji_in(tok: &str) -> Option<String> {
    let inner = tok.strip_prefix('<')?.strip_suffix('>')?;
    let (animated, body) = match inner.strip_prefix('a') {
        Some(rest) if rest.starts_with(':') => (true, rest),
        _ => (false, inner),
    };
    let body = body.strip_prefix(':')?;
    let mut it = body.split(':');
    let name = it.next()?;
    let id = it.next()?;
    if it.next().is_some() || name.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let ext = if animated { "gif" } else { "png" };
    Some(format!("https://cdn.discordapp.com/emojis/{id}.{ext}"))
}

const EMOJIS: &[(&str, &str)] = &[
    ("grinning", "\u{1F600}"),
    ("smile", "\u{1F604}"),
    ("smiley", "\u{1F603}"),
    ("grin", "\u{1F601}"),
    ("laughing", "\u{1F606}"),
    ("sweat_smile", "\u{1F605}"),
    ("joy", "\u{1F602}"),
    ("rofl", "\u{1F923}"),
    ("wink", "\u{1F609}"),
    ("blush", "\u{1F60A}"),
    ("slightly_smiling", "\u{1F642}"),
    ("heart_eyes", "\u{1F60D}"),
    ("kissing_heart", "\u{1F618}"),
    ("thinking", "\u{1F914}"),
    ("neutral_face", "\u{1F610}"),
    ("expressionless", "\u{1F611}"),
    ("smirk", "\u{1F60F}"),
    ("unamused", "\u{1F612}"),
    ("rolling_eyes", "\u{1F644}"),
    ("flushed", "\u{1F633}"),
    ("sob", "\u{1F62D}"),
    ("cry", "\u{1F622}"),
    ("scream", "\u{1F631}"),
    ("rage", "\u{1F621}"),
    ("angry", "\u{1F620}"),
    ("sunglasses", "\u{1F60E}"),
    ("nerd", "\u{1F913}"),
    ("poop", "\u{1F4A9}"),
    ("skull", "\u{1F480}"),
    ("ghost", "\u{1F47B}"),
    ("alien", "\u{1F47D}"),
    ("robot", "\u{1F916}"),
    ("clown", "\u{1F921}"),
    ("thumbsup", "\u{1F44D}"),
    ("thumbsdown", "\u{1F44E}"),
    ("wave", "\u{1F44B}"),
    ("clap", "\u{1F44F}"),
    ("pray", "\u{1F64F}"),
    ("ok_hand", "\u{1F44C}"),
    ("muscle", "\u{1F4AA}"),
    ("point_up", "\u{261D}\u{FE0F}"),
    ("point_right", "\u{1F449}"),
    ("point_left", "\u{1F448}"),
    ("raised_hands", "\u{1F64C}"),
    ("handshake", "\u{1F91D}"),
    ("eyes", "\u{1F440}"),
    ("eye", "\u{1F441}\u{FE0F}"),
    ("fire", "\u{1F525}"),
    ("star", "\u{2B50}"),
    ("sparkles", "\u{2728}"),
    ("boom", "\u{1F4A5}"),
    ("zap", "\u{26A1}"),
    ("heart", "\u{2764}\u{FE0F}"),
    ("orange_heart", "\u{1F9E1}"),
    ("yellow_heart", "\u{1F49B}"),
    ("green_heart", "\u{1F49A}"),
    ("blue_heart", "\u{1F499}"),
    ("purple_heart", "\u{1F49C}"),
    ("broken_heart", "\u{1F494}"),
    ("100", "\u{1F4AF}"),
    ("tada", "\u{1F389}"),
    ("rocket", "\u{1F680}"),
    ("check", "\u{2705}"),
    ("x", "\u{274C}"),
    ("warning", "\u{26A0}\u{FE0F}"),
    ("question", "\u{2753}"),
    ("exclamation", "\u{2757}"),
    ("+1", "\u{1F44D}"),
    ("-1", "\u{1F44E}"),
    ("cat", "\u{1F431}"),
    ("dog", "\u{1F436}"),
    ("unicorn", "\u{1F984}"),
    ("dragon", "\u{1F409}"),
    ("pizza", "\u{1F355}"),
    ("burger", "\u{1F354}"),
    ("fries", "\u{1F35F}"),
    ("beer", "\u{1F37A}"),
    ("beers", "\u{1F37B}"),
    ("coffee", "\u{2615}"),
    ("cake", "\u{1F370}"),
    ("cookie", "\u{1F36A}"),
    ("apple", "\u{1F34E}"),
    ("soccer", "\u{26BD}"),
    ("basketball", "\u{1F3C0}"),
    ("trophy", "\u{1F3C6}"),
    ("medal", "\u{1F3C5}"),
    ("dice", "\u{1F3B2}"),
    ("crown", "\u{1F451}"),
    ("gem", "\u{1F48E}"),
    ("money", "\u{1F4B0}"),
    ("gift", "\u{1F381}"),
    ("bell", "\u{1F514}"),
    ("mega", "\u{1F4E3}"),
    ("book", "\u{1F4D6}"),
    ("pencil", "\u{270F}\u{FE0F}"),
    ("computer", "\u{1F4BB}"),
    ("phone", "\u{1F4F1}"),
    ("camera", "\u{1F4F7}"),
    ("music", "\u{1F3B5}"),
    ("headphones", "\u{1F3A7}"),
    ("game", "\u{1F3AE}"),
    ("sun", "\u{2600}\u{FE0F}"),
    ("moon", "\u{1F319}"),
    ("cloud", "\u{2601}\u{FE0F}"),
    ("rain", "\u{1F327}\u{FE0F}"),
    ("snowflake", "\u{2744}\u{FE0F}"),
    ("100_points", "\u{1F4AF}"),
    ("partying", "\u{1F973}"),
    ("shrug", "\u{1F937}"),
    ("facepalm", "\u{1F926}"),
    ("salute", "\u{1FAE1}"),
    ("melting", "\u{1FAE0}"),
];

fn shortcode(name: &str) -> Option<&'static str> {
    Some(match name {
        "smile" => "\u{1F604}",
        "smiley" => "\u{1F603}",
        "grin" => "\u{1F601}",
        "joy" => "\u{1F602}",
        "rofl" => "\u{1F923}",
        "wink" => "\u{1F609}",
        "blush" => "\u{1F60A}",
        "heart_eyes" => "\u{1F60D}",
        "thinking" | "thinking_face" => "\u{1F914}",
        "sob" | "cry" => "\u{1F62D}",
        "sunglasses" => "\u{1F60E}",
        "rage" | "angry" => "\u{1F621}",
        "scream" => "\u{1F631}",
        "thumbsup" | "+1" => "\u{1F44D}",
        "thumbsdown" | "-1" => "\u{1F44E}",
        "wave" => "\u{1F44B}",
        "clap" => "\u{1F44F}",
        "pray" => "\u{1F64F}",
        "ok_hand" => "\u{1F44C}",
        "eyes" => "\u{1F440}",
        "fire" => "\u{1F525}",
        "star" => "\u{2B50}",
        "sparkles" => "\u{2728}",
        "heart" => "\u{2764}\u{FE0F}",
        "broken_heart" => "\u{1F494}",
        "poop" | "shit" => "\u{1F4A9}",
        "skull" => "\u{1F480}",
        "tada" => "\u{1F389}",
        "rocket" => "\u{1F680}",
        "check" | "white_check_mark" => "\u{2705}",
        "x" => "\u{274C}",
        "100" => "\u{1F4AF}",
        "cat" => "\u{1F431}",
        "dog" => "\u{1F436}",
        "pizza" => "\u{1F355}",
        "beer" => "\u{1F37A}",
        "coffee" => "\u{2615}",
        _ => return None,
    })
}

fn big_emoji(content: &str) -> Option<Vec<EmojiTok>> {
    let trimmed = content.trim();
    if trimmed.is_empty() || trimmed.len() > 80 {
        return None;
    }
    let mut tokens = Vec::new();
    for tok in trimmed.split_whitespace() {
        if let Some(url) = custom_emoji_in(tok) {
            tokens.push(EmojiTok::Custom(url));
            continue;
        }
        if tok.len() >= 3 && tok.starts_with(':') && tok.ends_with(':') {
            let name = &tok[1..tok.len() - 1];
            match shortcode(name) {
                Some(u) => {
                    tokens.push(EmojiTok::Unicode(u.to_string()));
                    continue;
                }
                None => return None,
            }
        }
        if tok.chars().all(is_emoji_char) {
            tokens.push(EmojiTok::Unicode(tok.to_string()));
            continue;
        }
        return None;
    }
    if tokens.is_empty() || tokens.len() > 3 {
        return None;
    }
    Some(tokens)
}

#[derive(Clone)]
enum Seg {
    Plain(String),
    Bold(String),
    Italic(String),
    Strike(String),
    Code(String),
    Spoiler(String),
    Mention { text: String, me: bool },
    Channel(String),
    Emoji { name: String, id: String, animated: bool },
    Url(String),
}

fn render_content(
    ui: &mut egui::Ui,
    content: &str,
    s: &SharedState,
    msg_id: &str,
    rs: &mut RenderState,
) {
    if let Some(tokens) = big_emoji(content) {
        let size = if tokens.len() == 1 { 44.0 } else { 32.0 };
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for token in tokens {
                match token {
                    EmojiTok::Unicode(u) => match twemoji_url(&u) {
                        Some(url) => {
                            ui.add(
                                egui::Image::new(&url).fit_to_exact_size(egui::vec2(size, size)),
                            );
                        }
                        None => {
                            ui.label(RichText::new(u).size(size));
                        }
                    },
                    EmojiTok::Custom(url) => {
                        ui.add(egui::Image::new(&url).fit_to_exact_size(egui::vec2(size, size)));
                    }
                }
            }
        });
        return;
    }
    if content.contains("```") {
        render_fenced(ui, content, s, msg_id, rs);
        return;
    }
    let segs = match rs.seg_cache.get(msg_id) {
        Some((c, v)) if c == content => v.clone(),
        _ => {
            let v = parse_segments(content, s);
            rs.seg_cache
                .insert(msg_id.to_string(), (content.to_string(), v.clone()));
            v
        }
    };
    render_segments(ui, segs, msg_id, rs);
}

fn render_segments(
    ui: &mut egui::Ui,
    segments: Vec<Seg>,
    msg_id: &str,
    rs: &mut RenderState,
) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (idx, seg) in segments.into_iter().enumerate() {
            match seg {
                Seg::Plain(t) => {
                    render_plain(ui, &t);
                }
                Seg::Bold(t) => {
                    ui.label(RichText::new(t).strong());
                }
                Seg::Italic(t) => {
                    ui.label(RichText::new(t).italics());
                }
                Seg::Strike(t) => {
                    ui.label(RichText::new(t).strikethrough());
                }
                Seg::Code(t) => {
                    ui.label(RichText::new(t).code());
                }
                Seg::Spoiler(t) => {
                    let key = format!("{msg_id}:{idx}");
                    if rs.revealed.contains(&key) {
                        ui.label(
                            RichText::new(t)
                                .color(egui::Color32::from_gray(150))
                                .background_color(egui::Color32::from_gray(45)),
                        );
                    } else if ui
                        .add(egui::Button::new("spoiler").small())
                        .on_hover_text("Click to reveal")
                        .clicked()
                    {
                        rs.revealed.insert(key);
                    }
                }
                Seg::Mention { text, me } => {
                    if me {
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgb(88, 101, 242))
                            .corner_radius(egui::CornerRadius::same(4))
                            .inner_margin(egui::Margin::symmetric(5, 1))
                            .show(ui, |ui| {
                                ui.label(RichText::new(text).color(egui::Color32::WHITE));
                            });
                    } else {
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgb(60, 80, 130))
                            .corner_radius(egui::CornerRadius::same(4))
                            .inner_margin(egui::Margin::symmetric(5, 1))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(text).color(egui::Color32::from_rgb(200, 214, 255)),
                                );
                            });
                    }
                }
                Seg::Channel(t) => {
                    ui.label(
                        RichText::new(t)
                            .color(egui::Color32::from_rgb(180, 220, 255))
                            .background_color(egui::Color32::from_rgb(50, 90, 120)),
                    );
                }
                Seg::Emoji { name, id, animated } => {
                    let ext = if animated { "gif" } else { "png" };
                    let url = format!("https://cdn.discordapp.com/emojis/{id}.{ext}");
                    if ui
                        .add(egui::Image::new(&url).max_height(20.0).max_width(20.0))
                        .hovered()
                    {
                        // no-op; the image already renders a tooltip-less inline emoji
                    }
                    let _ = name;
                }
                Seg::Url(u) => {
                    ui.hyperlink_to(u.clone(), u.clone());
                }
            }
        }
    });
}

fn render_fenced(
    ui: &mut egui::Ui,
    content: &str,
    s: &SharedState,
    msg_id: &str,
    rs: &mut RenderState,
) {
    let mut in_code = false;
    let mut code = String::new();
    for line in content.lines() {
        if line.trim_start().starts_with("```") {
            if in_code {
                render_code_block(ui, &code);
                code.clear();
            }
            in_code = !in_code;
            continue;
        }
        if in_code {
            code.push_str(line);
            code.push('\n');
        } else if !line.is_empty() {
            render_segments(ui, parse_segments(line, s), msg_id, rs);
        }
    }
    if in_code && !code.is_empty() {
        render_code_block(ui, &code);
    }
}

fn render_code_block(ui: &mut egui::Ui, code: &str) {
    ui.add(
        egui::Label::new(
            RichText::new(code.trim_end())
                .monospace()
                .background_color(egui::Color32::from_gray(35)),
        )
        .wrap(),
    );
    ui.add_space(2.0);
}

fn parse_segments(text: &str, s: &SharedState) -> Vec<Seg> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut out: Vec<Seg> = Vec::new();
    let mut buf = String::new();
    let mut i = 0;

    while i < n {
        let c = chars[i];

        if c == '`' {
            if let Some(end) = find_char(&chars, i + 1, '`') {
                push_plain(&mut buf, &mut out);
                out.push(Seg::Code(chars[i + 1..end].iter().collect()));
                i = end + 1;
                continue;
            }
        }
        if c == '*' && i + 1 < n && chars[i + 1] == '*' {
            if let Some(end) = find_seq(&chars, i + 2, '*', '*') {
                push_plain(&mut buf, &mut out);
                out.push(Seg::Bold(chars[i + 2..end].iter().collect()));
                i = end + 2;
                continue;
            }
        }
        if c == '~' && i + 1 < n && chars[i + 1] == '~' {
            if let Some(end) = find_seq(&chars, i + 2, '~', '~') {
                push_plain(&mut buf, &mut out);
                out.push(Seg::Strike(chars[i + 2..end].iter().collect()));
                i = end + 2;
                continue;
            }
        }
        if c == '|' && i + 1 < n && chars[i + 1] == '|' {
            if let Some(end) = find_seq(&chars, i + 2, '|', '|') {
                push_plain(&mut buf, &mut out);
                out.push(Seg::Spoiler(chars[i + 2..end].iter().collect()));
                i = end + 2;
                continue;
            }
        }
        if c == '*' || c == '_' {
            // For `_`, only treat it as emphasis at a word boundary so that
            // identifiers like snake_case are not mangled.
            let boundary = c == '*'
                || (i == 0 || !chars[i - 1].is_alphanumeric())
                    && i + 1 < n
                    && !chars[i + 1].is_whitespace();
            if boundary {
                if let Some(end) = find_char(&chars, i + 1, c) {
                    if end > i + 1 && !chars[end - 1].is_whitespace() {
                        push_plain(&mut buf, &mut out);
                        out.push(Seg::Italic(chars[i + 1..end].iter().collect()));
                        i = end + 1;
                        continue;
                    }
                }
            }
        }
        if c == ':' {
            if let Some(end) = find_char(&chars, i + 1, ':') {
                if end > i + 1 && end - i <= 40 {
                    let name: String = chars[i + 1..end].iter().collect();
                    if name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '+' || c == '-')
                    {
                        if let Some(uni) = shortcode(&name) {
                            push_plain(&mut buf, &mut out);
                            out.push(Seg::Plain(uni.to_string()));
                            i = end + 1;
                            continue;
                        }
                    }
                }
            }
        }
        if c == '@' && (i == 0 || !chars[i - 1].is_alphanumeric()) {
            let word = if starts_word(&chars, i, "everyone") {
                Some("everyone")
            } else if starts_word(&chars, i, "here") {
                Some("here")
            } else {
                None
            };
            if let Some(w) = word {
                push_plain(&mut buf, &mut out);
                out.push(Seg::Mention {
                    text: format!("@{w}"),
                    me: true,
                });
                i += 1 + w.len();
                continue;
            }
        }
        if c == '<' {
            if let Some(end) = find_char(&chars, i + 1, '>') {
                let inner: String = chars[i + 1..end].iter().collect();
                if let Some(seg) = parse_angle(&inner, s) {
                    push_plain(&mut buf, &mut out);
                    out.push(seg);
                    i = end + 1;
                    continue;
                }
            }
        }
        if c == 'h' && matches_http(&chars, i) {
            let mut j = i;
            while j < n && !chars[j].is_whitespace() {
                j += 1;
            }
            let url: String = chars[i..j].iter().collect();
            push_plain(&mut buf, &mut out);
            out.push(Seg::Url(url));
            i = j;
            continue;
        }

        buf.push(c);
        i += 1;
    }

    push_plain(&mut buf, &mut out);
    out
}

fn parse_angle(inner: &str, s: &SharedState) -> Option<Seg> {
    let is_me = |id: &str| s.me.as_ref().map(|u| u.id == id).unwrap_or(false);
    if let Some(rest) = inner.strip_prefix("@!") {
        if rest.chars().all(|c| c.is_ascii_digit()) {
            let name = s.user_name(rest).unwrap_or_else(|| "user".to_string());
            return Some(Seg::Mention {
                text: format!("@{name}"),
                me: is_me(rest),
            });
        }
    }
    if let Some(rest) = inner.strip_prefix('@') {
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            let name = s.user_name(rest).unwrap_or_else(|| "user".to_string());
            return Some(Seg::Mention {
                text: format!("@{name}"),
                me: is_me(rest),
            });
        }
    }
    if let Some(rest) = inner.strip_prefix("#") {
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            let name = s.channel_name(rest).unwrap_or_else(|| "channel".to_string());
            return Some(Seg::Channel(format!("#{name}")));
        }
    }
    if let Some(rest) = inner.strip_prefix("@&") {
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            return Some(Seg::Mention {
                text: "@role".to_string(),
                me: false,
            });
        }
    }
    // Custom emoji:  <:name:id>  or  <a:name:id>
    let (animated, body) = match inner.strip_prefix('a') {
        Some(rest) if rest.starts_with(':') => (true, rest),
        _ => (false, inner),
    };
    if let Some(body) = body.strip_prefix(':') {
        let parts: Vec<&str> = body.split(':').collect();
        if parts.len() == 2 && !parts[1].is_empty() && parts[1].chars().all(|c| c.is_ascii_digit()) {
            return Some(Seg::Emoji {
                name: parts[0].to_string(),
                id: parts[1].to_string(),
                animated,
            });
        }
    }
    None
}

fn find_char(chars: &[char], from: usize, target: char) -> Option<usize> {
    (from..chars.len()).find(|&j| chars[j] == target)
}

fn find_seq(chars: &[char], from: usize, a: char, b: char) -> Option<usize> {
    (from..chars.len().saturating_sub(1)).find(|&j| chars[j] == a && chars[j + 1] == b)
}

fn matches_http(chars: &[char], i: usize) -> bool {
    let pat = ['h', 't', 't', 'p'];
    chars.len() >= i + 4 && chars[i..i + 4] == pat
}

fn starts_word(chars: &[char], i: usize, word: &str) -> bool {
    let w: Vec<char> = word.chars().collect();
    let start = i + 1;
    if start + w.len() > chars.len() {
        return false;
    }
    for (k, &wc) in w.iter().enumerate() {
        if chars[start + k] != wc {
            return false;
        }
    }
    match chars.get(start + w.len()) {
        Some(&next) => !next.is_alphanumeric() && next != '_',
        None => true,
    }
}

fn push_plain(buf: &mut String, out: &mut Vec<Seg>) {
    if !buf.is_empty() {
        out.push(Seg::Plain(std::mem::take(buf)));
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_ts(timestamp: &Option<String>) -> Option<DateTime<Local>> {
    timestamp
        .as_ref()
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .map(|dt| dt.with_timezone(&Local))
}

#[allow(dead_code)]
fn date_of(ts: Option<DateTime<Local>>) -> Option<NaiveDate> {
    ts.map(|t| t.date_naive())
}

fn preview_text(content: &str) -> String {
    let line = content.lines().next().unwrap_or("").trim();
    if line.chars().count() > 80 {
        let mut s: String = line.chars().take(80).collect();
        s.push('…');
        s
    } else if line.is_empty() {
        "[attachment]".to_string()
    } else {
        line.to_string()
    }
}
