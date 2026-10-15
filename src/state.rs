use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::model::{
    Channel, Emoji, Guild, Member, Message, Relationship, User, UserProfile,
};

/// Minimum time a typing indicator lingers without a refresh.
const TYPING_TTL: Duration = Duration::from_secs(8);

/// Sentinel "guild id" used to represent the Direct Messages view.
pub const DM_HOME: &str = "@me";

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Error(String),
}

/// State shared between the UI thread and the tokio runtime.
pub struct SharedState {
    pub status: ConnectionStatus,
    pub me: Option<User>,
    pub guilds: Vec<Guild>,
    pub channels: HashMap<String, Channel>,
    pub messages: HashMap<String, Vec<Message>>,
    pub selected_guild: Option<String>,
    pub selected_channel: Option<String>,
    pub last_error: Option<String>,
    pub loading_older: HashSet<String>,
    pub reached_top: HashSet<String>,
    pub users: HashMap<String, User>,
    pub profiles: HashMap<String, User>,
    /// Channels whose history we have loaded (we only retain messages for these).
    pub known: HashSet<String>,
    /// Per-scope display-name overrides: "scope\x1fuser_id" -> custom name.
    pub aliases: HashMap<String, String>,
    /// Unread message counts, per channel.
    pub unread: HashMap<String, u32>,
    /// Unread messages that mention us (subset of `unread`), per channel.
    pub mentions: HashMap<String, u32>,
    /// Who is currently typing, per channel (with an expiry).
    pub typing: HashMap<String, Vec<TypingEntry>>,
    pub pinned_guilds: HashSet<String>,
    pub pinned_dms: HashSet<String>,
    /// Guilds whose full member list has been fetched (via gateway or REST).
    pub members_loaded: HashSet<String>,
    /// Guilds for which a member fetch from REST is currently in flight.
    pub members_pending: HashSet<String>,
    /// Pinned messages, per channel, once fetched.
    pub pins: HashMap<String, Vec<Message>>,
    /// Relationships (/friends). Sorted in fetch order.
    pub friends: Vec<Relationship>,
    /// Global presence per user id (friends, DMs): "online" / "idle" / "dnd" / ...
    pub presences: HashMap<String, String>,
    /// Recent message edits and deletions, newest first.
    pub message_log: Vec<LogEntry>,
    /// Pending desktop notifications (DMs + mentions), drained by the UI.
    pub notices: Vec<Notice>,
    /// Persistent history of notifications (newest last), shown in the UI.
    pub notice_log: Vec<Notice>,
    /// Pending error messages to surface as red toasts, drained by the UI.
    pub errors: Vec<String>,
    /// Maximum messages retained per channel (older ones are trimmed).
    pub message_cap: usize,
    /// UI context captured on the first frame so background tasks (gateway,
    /// REST, typing) can wake the interface when new data arrives.
    pub repaint: Option<eframe::egui::Context>,
}

/// A pending desktop notification for a DM or a message that mentions us.
#[derive(Debug, Clone)]
pub struct Notice {
    pub channel_id: String,
    pub channel_name: String,
    pub author: String,
    pub body: String,
    pub mention: bool,
}

/// A file staged for upload (from the file picker, drag-and-drop, or clipboard).
#[derive(Debug, Clone)]
pub struct PendingFile {
    pub filename: String,
    pub data: Vec<u8>,
    pub content_type: Option<String>,
}

impl PendingFile {
    pub fn from_bytes(filename: String, data: Vec<u8>) -> Self {
        let content_type = guess_content_type(&filename);
        Self {
            filename,
            data,
            content_type,
        }
    }
}

fn guess_content_type(filename: &str) -> Option<String> {
    let ext = filename.rsplit('.').next()?.to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "json" => "application/json",
        "zip" => "application/zip",
        _ => return None,
    };
    Some(mime.to_string())
}

#[derive(Debug, Clone, PartialEq)]
pub enum LogKind {
    Edited,
    Deleted,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub kind: LogKind,
    pub channel_name: String,
    pub author: String,
    pub before: String,
    pub after: String,
    pub at: String,
}

pub struct TypingEntry {
    pub user_id: String,
    pub name: String,
    pub until: Instant,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            status: ConnectionStatus::Disconnected,
            me: None,
            guilds: Vec::new(),
            channels: HashMap::new(),
            messages: HashMap::new(),
            selected_guild: None,
            selected_channel: None,
            last_error: None,
            loading_older: HashSet::new(),
            reached_top: HashSet::new(),
            users: HashMap::new(),
            profiles: HashMap::new(),
            known: HashSet::new(),
            aliases: crate::config::load_aliases(),
            unread: HashMap::new(),
            mentions: HashMap::new(),
            typing: HashMap::new(),
            pinned_guilds: crate::config::load_pins().guilds.into_iter().collect(),
            pinned_dms: crate::config::load_pins().dms.into_iter().collect(),
            members_loaded: HashSet::new(),
            members_pending: HashSet::new(),
            pins: HashMap::new(),
            friends: Vec::new(),
            presences: HashMap::new(),
            message_log: Vec::new(),
            notices: Vec::new(),
            notice_log: Vec::new(),
            errors: Vec::new(),
            message_cap: 300,
            repaint: None,
        }
    }
}

impl SharedState {
    pub fn reset(&mut self) {
        let aliases = std::mem::take(&mut self.aliases);
        let pinned_guilds = std::mem::take(&mut self.pinned_guilds);
        let pinned_dms = std::mem::take(&mut self.pinned_dms);
        *self = Self::default();
        self.aliases = aliases;
        self.pinned_guilds = pinned_guilds;
        self.pinned_dms = pinned_dms;
    }

    fn save_pins(&self) {
        let pins = crate::config::Pins {
            guilds: self.pinned_guilds.iter().cloned().collect(),
            dms: self.pinned_dms.iter().cloned().collect(),
        };
        crate::config::save_pins(&pins);
    }

    pub fn is_guild_pinned(&self, id: &str) -> bool {
        self.pinned_guilds.contains(id)
    }

    pub fn is_dm_pinned(&self, id: &str) -> bool {
        self.pinned_dms.contains(id)
    }

    pub fn toggle_pin_guild(&mut self, id: &str) {
        if !self.pinned_guilds.remove(id) {
            self.pinned_guilds.insert(id.to_string());
        }
        self.save_pins();
    }

    pub fn toggle_pin_dm(&mut self, id: &str) {
        if !self.pinned_dms.remove(id) {
            self.pinned_dms.insert(id.to_string());
        }
        self.save_pins();
    }

    pub fn clear_unread(&mut self, channel_id: &str) {
        self.unread.remove(channel_id);
        self.mentions.remove(channel_id);
    }

    pub fn channel_unread(&self, channel_id: &str) -> u32 {
        self.unread.get(channel_id).copied().unwrap_or(0)
    }

    pub fn channel_mentions(&self, channel_id: &str) -> u32 {
        self.mentions.get(channel_id).copied().unwrap_or(0)
    }

    /// Whether a message pings this user (`@you`, `@everyone`, `@here`).
    pub fn mentions_me(&self, message: &Message) -> bool {
        let Some(id) = self.me.as_ref().map(|u| u.id.as_str()) else {
            return false;
        };
        message.mentions.iter().any(|u| u.id == id)
            || message.content.contains(&format!("<@{id}>"))
            || message.content.contains(&format!("<@!{id}>"))
            || message.content.contains("@everyone")
            || message.content.contains("@here")
    }

    /// Newest message id in a channel, for building read receipts. Prefers the
    /// newest stored message, falling back to the channel's own last_message_id.
    pub fn channel_last_message_id(&self, channel_id: &str) -> Option<String> {
        if let Some(msgs) = self.messages.get(channel_id) {
            let max = msgs
                .iter()
                .map(|m| (m.id.clone(), m.id.parse::<u64>().unwrap_or(0)))
                .max_by_key(|(_, n)| *n)
                .map(|(id, _)| id);
            if max.is_some() {
                return max;
            }
        }
        self.channels
            .get(channel_id)
            .and_then(|c| c.last_message_id.clone())
            .filter(|id| !id.is_empty())
    }

    pub fn guild_unread(&self, guild_id: &str) -> u32 {
        self.channels
            .iter()
            .filter(|(_, c)| c.guild_id.as_deref() == Some(guild_id))
            .map(|(id, _)| self.channel_unread(id))
            .sum()
    }

    pub fn guild_mentions(&self, guild_id: &str) -> u32 {
        self.channels
            .iter()
            .filter(|(_, c)| c.guild_id.as_deref() == Some(guild_id))
            .map(|(id, _)| self.channel_mentions(id))
            .sum()
    }

    /// Unread mention count across the DM channels.
    pub fn dm_mentions_total(&self) -> u32 {
        self.channels
            .iter()
            .filter(|(_, c)| c.is_dm())
            .map(|(id, _)| self.channel_mentions(id))
            .sum()
    }

    pub fn total_unread(&self) -> u32 {
        self.unread.values().copied().sum()
    }

    pub fn dm_unread_total(&self) -> u32 {
        self.channels
            .iter()
            .filter(|(_, c)| c.is_dm())
            .map(|(id, _)| self.channel_unread(id))
            .sum()
    }

    /// A message arriving live from the gateway: count unread unless it lands
    /// in the channel currently on screen or was sent by this user from
    /// another client (never ping ourselves). Then store it (if we track it).
    pub fn receive_message(&mut self, message: Message) {
        let is_me = self
            .me
            .as_ref()
            .map(|u| u.id == message.author.id)
            .unwrap_or(false);
        let viewing = self.selected_channel.as_deref() == Some(message.channel_id.as_str());
        let mention = !is_me && self.mentions_me(&message);
        if !is_me && !viewing {
            *self.unread.entry(message.channel_id.clone()).or_insert(0) += 1;
            if mention {
                *self.mentions.entry(message.channel_id.clone()).or_insert(0) += 1;
            }
        }
        if !is_me && !viewing {
            self.queue_notice(&message);
        }
        self.add_message(message);
    }

    /// Build a desktop notification for a DM or a message that mentions us.
    fn queue_notice(&mut self, message: &Message) {
        let is_dm = self
            .channels
            .get(&message.channel_id)
            .map(|c| c.guild_id.is_none() && c.is_dm())
            .unwrap_or(false);
        let mention = self.mentions_me(message);
        if !is_dm && !mention {
            return;
        }
        let channel_name = match self.channels.get(&message.channel_id) {
            Some(c) if c.is_dm() => c.dm_display_name(),
            Some(c) => {
                let guild = c
                    .guild_id
                    .as_ref()
                    .and_then(|gid| self.guilds.iter().find(|g| &g.id == gid))
                    .map(|g| g.display_name());
                match guild {
                    Some(g) => format!("#{} in {}", c.display_name(), g),
                    None => format!("#{}", c.display_name()),
                }
            }
            None => "a channel".to_string(),
        };
        let mut body = message.content.clone();
        if body.is_empty() {
            if let Some(att) = message.attachments.first() {
                body = format!("[{}]", att.filename);
            } else {
                body = "[no content]".to_string();
            }
        }
        if body.chars().count() > 160 {
            body = body.chars().take(160).collect::<String>() + "\u{2026}";
        }
        let notice = Notice {
            channel_id: message.channel_id.clone(),
            channel_name,
            author: message.author.display_name(),
            body,
            mention,
        };
        self.notices.push(notice.clone());
        self.notice_log.push(notice);
        if self.notices.len() > 50 {
            let drop = self.notices.len() - 50;
            self.notices.drain(0..drop);
        }
        if self.notice_log.len() > 200 {
            let drop = self.notice_log.len() - 200;
            self.notice_log.drain(0..drop);
        }
    }

    /// Recent notification history (oldest first).
    pub fn notice_log(&self) -> &[Notice] {
        &self.notice_log
    }

    /// Take the queued desktop notifications (called once per frame by the UI).
    pub fn take_notices(&mut self) -> Vec<Notice> {
        std::mem::take(&mut self.notices)
    }

    /// Wake the UI thread (called by background tasks when state changes).
    pub fn wake(&self) {
        if let Some(ctx) = &self.repaint {
            ctx.request_repaint();
        }
    }

    /// Queue an error message to be shown as a red toast.
    pub fn push_error(&mut self, message: impl Into<String>) {
        self.errors.push(message.into());
        if self.errors.len() > 20 {
            let drop = self.errors.len() - 20;
            self.errors.drain(0..drop);
        }
        self.wake();
    }

    /// Take the queued error messages (called once per frame by the UI).
    pub fn take_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut self.errors)
    }

    /// Apply the read state that Discord ships in READY (what other clients
    /// read): seed unread/mention counts so channels read on mobile/desktop do
    /// not show up as new here.
    pub fn seed_read_state(&mut self, entries: impl IntoIterator<Item = (String, u32)>) {
        for (channel_id, mentions) in entries {
            if mentions == 0 {
                continue;
            }
            let e = self.unread.entry(channel_id.clone()).or_insert(0);
            if mentions > *e {
                *e = mentions;
            }
            let m = self.mentions.entry(channel_id).or_insert(0);
            if mentions > *m {
                *m = mentions;
            }
        }
    }

    pub fn note_typing(&mut self, channel_id: &str, user_id: &str, name: String) {
        let list = self.typing.entry(channel_id.to_string()).or_default();
        if let Some(entry) = list.iter_mut().find(|e| e.user_id == user_id) {
            entry.name = name;
            entry.until = Instant::now() + TYPING_TTL;
        } else {
            list.push(TypingEntry {
                user_id: user_id.to_string(),
                name,
                until: Instant::now() + TYPING_TTL,
            });
        }
        self.wake();
    }

    /// Names of users typing in a channel (expired entries are pruned).
    pub fn typing_names(&mut self, channel_id: &str) -> Vec<String> {
        let me = self.me.as_ref().map(|u| u.id.clone());
        let Some(list) = self.typing.get_mut(channel_id) else {
            return Vec::new();
        };
        let now = Instant::now();
        list.retain(|e| e.until > now);
        list.iter()
            .filter(|e| Some(&e.user_id) != me.as_ref())
            .map(|e| e.name.clone())
            .collect()
    }

    pub fn react_add(&mut self, channel_id: &str, message_id: &str, emoji: Emoji, from_me: bool) {
        if let Some(list) = self.messages.get_mut(channel_id) {
            if let Some(m) = list.iter_mut().find(|m| m.id == message_id) {
                if let Some(r) = m
                    .reactions
                    .iter_mut()
                    .find(|r| r.emoji.key() == emoji.key())
                {
                    // Idempotent for our own optimistic update: the gateway echo
                    // of a reaction we already applied must not double-count.
                    if from_me && r.me {
                        return;
                    }
                    r.count += 1;
                    if from_me {
                        r.me = true;
                    }
                } else {
                    m.reactions.push(crate::model::Reaction {
                        count: 1,
                        me: from_me,
                        emoji,
                    });
                }
            }
        }
    }

    pub fn react_remove(&mut self, channel_id: &str, message_id: &str, emoji: Emoji, from_me: bool) {
        if let Some(list) = self.messages.get_mut(channel_id) {
            if let Some(m) = list.iter_mut().find(|m| m.id == message_id) {
                if let Some(pos) = m.reactions.iter().position(|r| r.emoji.key() == emoji.key()) {
                    // Idempotent for our own optimistic removal.
                    if from_me && !m.reactions[pos].me {
                        return;
                    }
                    let r = &mut m.reactions[pos];
                    if from_me {
                        r.me = false;
                    }
                    r.count = r.count.saturating_sub(1);
                    if r.count == 0 {
                        m.reactions.remove(pos);
                    }
                }
            }
        }
    }

    /// Toggle the current user's reaction locally. Returns `true` if a reaction
    /// should now be sent to Discord (i.e. it was added), `false` for removal.
    pub fn toggle_reaction(
        &mut self,
        channel_id: &str,
        message_id: &str,
        emoji: Emoji,
    ) -> Option<bool> {
        let currently_reacted = self
            .messages
            .get(channel_id)
            .and_then(|l| l.iter().find(|m| m.id == message_id))
            .and_then(|m| m.reactions.iter().find(|r| r.emoji.key() == emoji.key()))
            .map(|r| r.me)
            .unwrap_or(false);
        if currently_reacted {
            self.react_remove(channel_id, message_id, emoji, true);
            Some(false)
        } else {
            self.react_add(channel_id, message_id, emoji, true);
            Some(true)
        }
    }

    pub fn upsert_guild(&mut self, guild: Guild) {
        if let Some(existing) = self.guilds.iter_mut().find(|g| g.id == guild.id) {
            if guild.name.is_some() {
                existing.name = guild.name;
            }
            if guild.icon.is_some() {
                existing.icon = guild.icon;
            }
            if !guild.channels.is_empty() {
                existing.channels = guild.channels;
            }
            if !guild.members.is_empty() {
                existing.members = guild.members;
            }
            if !guild.presences.is_empty() {
                for p in guild.presences {
                    if let Some(slot) = existing
                        .presences
                        .iter_mut()
                        .find(|e| e.user.id == p.user.id)
                    {
                        *slot = p;
                    } else {
                        existing.presences.push(p);
                    }
                }
            }
            // GUILD_UPDATE payloads omit `position` (serde default 0); don't let
            // a missing field clobber the client-set server order.
            if guild.position != 0 {
                existing.position = guild.position;
            }
            existing.unavailable = guild.unavailable;
        } else {
            self.guilds.push(guild);
        }
    }

    /// Sort the guild list by the client-set server order (Discord `position`).
    pub fn sort_guilds(&mut self) {
        self.guilds.sort_by_key(|g| g.position);
    }

    pub fn upsert_member(&mut self, guild_id: &str, member: Member) {
        if let Some(guild) = self.guilds.iter_mut().find(|g| g.id == guild_id) {
            if let Some(existing) = guild
                .members
                .iter_mut()
                .find(|m| m.user.id == member.user.id)
            {
                *existing = member;
            } else {
                guild.members.push(member);
            }
        }
    }

    pub fn set_presence(&mut self, guild_id: &str, user_id: &str, status: &str) {
        self.presences
            .insert(user_id.to_string(), status.to_string());
        if let Some(guild) = self.guilds.iter_mut().find(|g| g.id == guild_id) {
            if let Some(slot) = guild.presences.iter_mut().find(|p| p.user.id == user_id) {
                slot.status = Some(status.to_string());
            } else {
                guild.presences.push(crate::model::Presence {
                    user: crate::model::PresenceUser {
                        id: user_id.to_string(),
                    },
                    status: Some(status.to_string()),
                });
            }
        }
        self.wake();
    }

    /// Replace the friend list (keeps friend requests elsewhere too).
    pub fn set_friends(&mut self, relationships: Vec<Relationship>) {
        self.friends = relationships;
        self.wake();
    }

    pub fn friend_presence(&self, user_id: &str) -> &str {
        self.presence_of(user_id)
    }

    /// Global presence status for a user ("online"/"idle"/"dnd"/"offline").
    pub fn presence_of(&self, user_id: &str) -> &str {
        self.presences
            .get(user_id)
            .map(String::as_str)
            .unwrap_or("offline")
    }

    pub fn guild_members(&self, guild_id: &str) -> Vec<Member> {
        let mut members = self
            .guilds
            .iter()
            .find(|g| g.id == guild_id)
            .map(|g| g.members.clone())
            .unwrap_or_default();
        members.sort_by(|a, b| {
            a.display_name()
                .to_lowercase()
                .cmp(&b.display_name().to_lowercase())
        });
        members
    }

    pub fn members_loaded(&self, guild_id: &str) -> bool {
        self.members_loaded.contains(guild_id)
    }

    pub fn members_pending(&self, guild_id: &str) -> bool {
        self.members_pending.contains(guild_id)
    }

    pub fn begin_members_fetch(&mut self, guild_id: &str) -> bool {
        if self.members_loaded.contains(guild_id) || self.members_pending.contains(guild_id) {
            return false;
        }
        self.members_pending.insert(guild_id.to_string());
        true
    }

    pub fn finish_members_fetch(&mut self, guild_id: &str) {
        self.members_pending.remove(guild_id);
        self.members_loaded.insert(guild_id.to_string());
    }

    pub fn add_channel(&mut self, channel: Channel) {
        for user in &channel.recipients {
            self.users.insert(user.id.clone(), user.clone());
        }
        self.channels.insert(channel.id.clone(), channel);
        self.wake();
    }

    pub fn mark_known(&mut self, channel_id: &str) {
        self.known.insert(channel_id.to_string());
    }

    pub fn add_message(&mut self, message: Message) {
        // Only retain messages for channels whose history we have loaded; this
        // keeps us from accumulating data for servers the user hasn't opened.
        if !self.known.contains(&message.channel_id) {
            return;
        }
        self.users
            .insert(message.author.id.clone(), message.author.clone());
        let list = self.messages.entry(message.channel_id.clone()).or_default();
        if !list.iter().any(|m| m.id == message.id) {
            list.push(message);
            list.sort_by(|a, b| a.id.cmp(&b.id));
            if self.message_cap > 0 && list.len() > self.message_cap {
                let drop = list.len() - self.message_cap;
                list.drain(0..drop);
            }
        }
        self.wake();
    }

    /// Set the per-channel message cap, trimming existing history to match.
    pub fn set_message_cap(&mut self, cap: usize) {
        if self.message_cap == cap {
            return;
        }
        self.message_cap = cap;
        if cap == 0 {
            return;
        }
        for list in self.messages.values_mut() {
            if list.len() > cap {
                let drop = list.len() - cap;
                list.drain(0..drop);
            }
        }
    }

    /// Trim a channel's history down to its newest `keep` messages, returning
    /// the image URIs of the dropped ones. Used when the user is pinned to the
    /// bottom so that text scrolled past (and its pictures) is released.
    pub fn trim_channel_history(&mut self, channel_id: &str, keep: usize) -> Vec<String> {
        let mut uris: Vec<String> = Vec::new();
        if let Some(list) = self.messages.get_mut(channel_id) {
            if list.len() > keep {
                let drop = list.len() - keep;
                let removed: Vec<Message> = list.drain(0..drop).collect();
                for m in &removed {
                    collect_message_image_uris(m, &mut uris);
                }
            }
        }
        uris.sort();
        uris.dedup();
        uris
    }

    /// Drop stored history for every channel except `keep`, returning the image
    /// URIs referenced by the dropped messages so the UI can release their
    /// decoded bytes and textures. This keeps RAM bounded when hopping between
    /// channels instead of retaining every chat (and its pictures) forever.
    pub fn drop_history_except(&mut self, keep: Option<&str>) -> Vec<String> {
        let drop_ids: Vec<String> = self
            .messages
            .keys()
            .filter(|k| Some(k.as_str()) != keep)
            .cloned()
            .collect();
        let mut uris: Vec<String> = Vec::new();
        for id in drop_ids {
            if let Some(list) = self.messages.remove(&id) {
                for m in &list {
                    collect_message_image_uris(m, &mut uris);
                }
            }
            self.known.remove(&id);
            self.loading_older.remove(&id);
            self.reached_top.remove(&id);
        }
        uris.sort();
        uris.dedup();
        uris
    }

    fn alias_key(scope: &str, user_id: &str) -> String {
        format!("{scope}\u{1f}{user_id}")
    }

    pub fn alias(&self, scope: &str, user_id: &str) -> Option<&str> {
        self.aliases
            .get(&Self::alias_key(scope, user_id))
            .map(|s| s.as_str())
    }

    pub fn set_alias(&mut self, scope: &str, user_id: &str, name: &str) {
        let key = Self::alias_key(scope, user_id);
        if name.trim().is_empty() {
            self.aliases.remove(&key);
        } else {
            self.aliases.insert(key, name.trim().to_string());
        }
        crate::config::save_aliases(&self.aliases);
    }

    /// Preferred display name for a user within a scope (guild id or `DM_HOME`).
    pub fn name_in(&self, scope: &str, user: &User) -> String {
        self.alias(scope, &user.id)
            .map(|s| s.to_string())
            .unwrap_or_else(|| user.display_name())
    }

    pub fn set_me(&mut self, user: User) {
        self.users.insert(user.id.clone(), user.clone());
        self.me = Some(user);
    }

    pub fn user_name(&self, id: &str) -> Option<String> {
        self.users.get(id).map(|u| u.display_name())
    }

    pub fn set_profile(&mut self, profile: UserProfile) {
        let mut user = profile.user;
        if let Some(bio) = profile.user_profile.and_then(|p| p.bio) {
            user.bio = Some(bio);
        }
        self.users.insert(user.id.clone(), user.clone());
        self.profiles.insert(user.id.clone(), user);
    }

    pub fn profile(&self, id: &str) -> Option<&User> {
        self.profiles.get(id).or_else(|| self.users.get(id))
    }

    pub fn channel_name(&self, id: &str) -> Option<String> {
        self.channels.get(id).map(|c| c.display_name())
    }

    pub fn update_message(
        &mut self,
        channel_id: &str,
        message_id: &str,
        content: String,
        edited_timestamp: Option<String>,
    ) {
        let mut pending = None;
        if let Some(list) = self.messages.get_mut(channel_id) {
            if let Some(existing) = list.iter_mut().find(|m| m.id == message_id) {
                let before = existing.content.clone();
                if before != content {
                    pending = Some((existing.author.id.clone(), before, existing.timestamp.clone()));
                }
                existing.content = content.clone();
                existing.edited_timestamp = edited_timestamp.clone();
            }
        }
        if let Some((user_id, before, ts)) = pending {
            let author = self.display_author(&user_id);
            let channel_name = self.channel_name(channel_id).unwrap_or_else(|| "unknown".into());
            let at = edited_timestamp.or(ts).unwrap_or_default();
            self.push_log(LogEntry {
                kind: LogKind::Edited,
                channel_name,
                author,
                before,
                after: content,
                at,
            });
        }
    }

    pub fn delete_message(&mut self, channel_id: &str, message_id: &str) {
        let mut pending = None;
        if let Some(list) = self.messages.get_mut(channel_id) {
            if let Some(pos) = list.iter().position(|m| m.id == message_id) {
                let m = list.remove(pos);
                pending = Some((m.author.id.clone(), m.content, m.timestamp));
            }
        }
        if let Some((user_id, before, at)) = pending {
            let author = self.display_author(&user_id);
            let channel_name = self.channel_name(channel_id).unwrap_or_else(|| "unknown".into());
            self.push_log(LogEntry {
                kind: LogKind::Deleted,
                channel_name,
                author,
                before,
                after: String::new(),
                at: at.unwrap_or_default(),
            });
        }
        self.wake();
    }

    fn display_author(&self, user_id: &str) -> String {
        if let Some(u) = self.users.get(user_id) {
            return u.display_name().to_string();
        }
        if let Some(u) = self.profiles.get(user_id) {
            return u.display_name().to_string();
        }
        user_id.to_string()
    }

    fn push_log(&mut self, entry: LogEntry) {
        self.message_log.insert(0, entry);
        if self.message_log.len() > 300 {
            self.message_log.truncate(300);
        }
    }

    pub fn message(&self, channel_id: &str, message_id: &str) -> Option<&Message> {
        self.messages
            .get(channel_id)
            .and_then(|list| list.iter().find(|m| m.id == message_id))
    }

    pub fn text_channels(&self, guild_id: &str) -> Vec<Channel> {
        let mut chans: Vec<Channel> = self
            .channels
            .values()
            .filter(|c| c.guild_id.as_deref() == Some(guild_id) && c.is_text())
            .cloned()
            .collect();
        chans.sort_by_key(|c| c.sort_key());
        chans
    }

    pub fn voice_channels(&self, guild_id: &str) -> Vec<Channel> {
        let mut chans: Vec<Channel> = self
            .channels
            .values()
            .filter(|c| c.guild_id.as_deref() == Some(guild_id) && c.kind == 2)
            .cloned()
            .collect();
        chans.sort_by_key(|c| c.sort_key());
        chans
    }

    pub fn categories(&self, guild_id: &str) -> Vec<Channel> {
        let mut cats: Vec<Channel> = self
            .channels
            .values()
            .filter(|c| c.guild_id.as_deref() == Some(guild_id) && c.kind == 4)
            .cloned()
            .collect();
        cats.sort_by_key(|c| c.sort_key());
        cats
    }

    pub fn has_channels(&self, guild_id: &str) -> bool {
        self.channels
            .values()
            .any(|c| c.guild_id.as_deref() == Some(guild_id))
    }

    /// DM and group-DM channels, most recently active first.
    pub fn dm_channels(&self) -> Vec<Channel> {
        let mut chans: Vec<Channel> = self
            .channels
            .values()
            .filter(|c| c.guild_id.is_none() && c.is_dm())
            .cloned()
            .collect();
        chans.sort_by(|a, b| {
            let ak = a.last_message_id.clone().unwrap_or_default();
            let bk = b.last_message_id.clone().unwrap_or_default();
            bk.cmp(&ak)
        });
        chans
    }
}

/// Commands sent from the UI to the async Discord backend.
#[derive(Debug, Clone)]
pub enum Command {
    Connect(String),
    FetchGuildChannels(String),
    OpenChannel(String),
    FetchDms,
    FetchProfile { user_id: String, guild_id: Option<String> },
    OpenDm(String),
    FetchMessages(String),
    FetchOlder { channel_id: String, before: String },
    SendMessage { channel_id: String, content: String, reply_to: Option<String> },
    SendAttachments {
        channel_id: String,
        content: String,
        files: Vec<PendingFile>,
        reply_to: Option<String>,
    },
    EditMessage { channel_id: String, message_id: String, content: String },
    DeleteMessage { channel_id: String, message_id: String },
    AddReaction { channel_id: String, message_id: String, emoji: String },
    RemoveReaction { channel_id: String, message_id: String, emoji: String },
    TriggerTyping(String),
    FetchGuildMembers(String),
    FetchPins(String),
    AckChannel(String),
    KickUser { guild_id: String, user_id: String, reason: Option<String> },
    BanUser { guild_id: String, user_id: String, reason: Option<String> },
    TimeoutUser { guild_id: String, user_id: String, seconds: u64, reason: Option<String> },
    Disconnect,
}

/// Collect the image URIs a message may have loaded (attachment previews at the
/// sizes the UI requests, plus embed images) so they can be released.
fn collect_message_image_uris(message: &Message, out: &mut Vec<String>) {
    for att in &message.attachments {
        if att.is_image() {
            if let Some(u) = att.image_url(400) {
                out.push(u);
            }
            if let Some(u) = att.image_url(1600) {
                out.push(u);
            }
        }
        if let Some(u) = &att.url {
            out.push(u.clone());
        }
        if let Some(u) = &att.proxy_url {
            out.push(u.clone());
        }
    }
    for e in &message.embeds {
        if let Some(u) = e.image.as_ref().and_then(|i| i.best_url()) {
            out.push(u);
        }
        if let Some(u) = e.thumbnail.as_ref().and_then(|i| i.best_url()) {
            out.push(u);
        }
        if let Some(u) = e.video.as_ref().and_then(|i| i.best_url()) {
            out.push(u);
        }
    }
    crate::format::collect_content_image_uris(&message.content, out);
}
