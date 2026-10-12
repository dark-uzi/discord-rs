use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct User {
    pub id: String,
    pub username: String,
    #[serde(default)]
    pub global_name: Option<String>,
    #[serde(default)]
    pub discriminator: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub bot: Option<bool>,
    #[serde(default)]
    pub bio: Option<String>,
}

impl User {
    pub fn display_name(&self) -> String {
        self.global_name
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.username.clone())
    }

    /// The classic `username#1234` handle, or just `username` for the new system.
    pub fn handle(&self) -> String {
        match self.discriminator.as_deref() {
            Some(d) if d != "0" && !d.is_empty() => format!("{}#{d}", self.username),
            _ => self.username.clone(),
        }
    }

    pub fn avatar_url(&self, size: u32) -> String {
        match &self.avatar {
            Some(hash) => {
                let ext = if hash.starts_with("a_") { "gif" } else { "png" };
                format!(
                    "https://cdn.discordapp.com/avatars/{}/{}.{}?size={size}",
                    self.id, hash, ext
                )
            }
            None => {
                let index = match self.discriminator.as_deref() {
                    Some("0") | None => {
                        let id: u64 = self.id.parse().unwrap_or(0);
                        (id >> 22) % 6
                    }
                    Some(disc) => disc.parse::<u64>().unwrap_or(0) % 5,
                };
                format!("https://cdn.discordapp.com/embed/avatars/{index}.png")
            }
        }
    }
}

/// Response from `GET /users/{id}/profile`.
#[derive(Debug, Clone, Deserialize)]
pub struct UserProfile {
    pub user: User,
    #[serde(default)]
    pub user_profile: Option<ProfileBio>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ProfileBio {
    #[serde(default)]
    pub bio: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Guild {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub unavailable: bool,
    /// Server order as set in any Discord client (drag to reorder); synced via
    /// the guild's `position` field so mobile/desktop and this client agree.
    #[serde(default)]
    pub position: i32,
    #[serde(default)]
    pub channels: Vec<Channel>,
    #[serde(default)]
    pub members: Vec<Member>,
    #[serde(default)]
    pub presences: Vec<Presence>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Presence {
    pub user: PresenceUser,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PresenceUser {
    pub id: String,
}

/// A relationship from `GET /users/@me/relationships` (friends, blocked, ...).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Relationship {
    #[serde(default)]
    pub id: String,
    /// 1 = friend, 2 = blocked, 3 = incoming request, 4 = outgoing request.
    #[serde(default, rename = "type")]
    pub kind: u8,
    pub user: User,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Member {
    pub user: User,
    #[serde(default)]
    pub nick: Option<String>,
    #[serde(default)]
    pub roles: Vec<String>,
}

impl Member {
    pub fn display_name(&self) -> String {
        self.nick
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.user.display_name())
    }

    pub fn username(&self) -> String {
        self.user.username.clone()
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Emoji {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub animated: Option<bool>,
}

impl Emoji {
    /// The `name:id` (custom) or Unicode text (standard) used to render.
    pub fn key(&self) -> String {
        match &self.id {
            Some(id) => format!("{}:{}", self.name.clone().unwrap_or_default(), id),
            None => self.name.clone().unwrap_or_default(),
        }
    }

    /// The label shown next to a reaction count.
    pub fn label(&self) -> String {
        match &self.id {
            Some(_) => format!(":{}:", self.name.clone().unwrap_or_default()),
            None => self.name.clone().unwrap_or_default(),
        }
    }

    pub fn image_url(&self) -> Option<String> {
        let id = self.id.as_ref()?;
        let ext = if self.animated.unwrap_or(false) { "gif" } else { "png" };
        Some(format!("https://cdn.discordapp.com/emojis/{id}.{ext}"))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Reaction {
    pub count: u32,
    #[serde(default)]
    pub me: bool,
    pub emoji: Emoji,
}

impl Guild {
    pub fn display_name(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| "Unknown".to_string())
    }

    pub fn icon_url(&self, size: u32) -> Option<String> {
        let hash = self.icon.as_ref()?;
        let ext = if hash.starts_with("a_") { "gif" } else { "png" };
        Some(format!(
            "https://cdn.discordapp.com/icons/{}/{}.{}?size={size}",
            self.id, hash, ext
        ))
    }

    pub fn status_of(&self, user_id: &str) -> &str {
        self.presences
            .iter()
            .find(|p| p.user.id == user_id)
            .and_then(|p| p.status.as_deref())
            .unwrap_or("offline")
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Channel {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: u8,
    #[serde(default)]
    pub guild_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub position: Option<i64>,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub last_message_id: Option<String>,
    #[serde(default)]
    pub recipients: Vec<User>,
}

impl Channel {
    pub fn is_text(&self) -> bool {
        matches!(self.kind, 0 | 5)
    }

    pub fn is_dm(&self) -> bool {
        matches!(self.kind, 1 | 3)
    }

    pub fn display_name(&self) -> String {
        self.name.clone().unwrap_or_else(|| "unnamed".to_string())
    }

    /// Display name for a DM (kind 1) or group DM (kind 3).
    pub fn dm_display_name(&self) -> String {
        if self.kind == 3 {
            self.name
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| self.recipients_name())
        } else {
            self.recipients
                .first()
                .map(|u| u.display_name())
                .unwrap_or_else(|| "Direct Message".to_string())
        }
    }

    fn recipients_name(&self) -> String {
        let names: Vec<String> = self.recipients.iter().map(|u| u.display_name()).collect();
        if names.is_empty() {
            "Group DM".to_string()
        } else {
            names.join(", ")
        }
    }

    pub fn sort_key(&self) -> (i64, String) {
        (self.position.unwrap_or(0), self.id.clone())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Message {
    pub id: String,
    pub channel_id: String,
    pub author: User,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub timestamp: Option<String>,
    #[serde(default)]
    pub edited_timestamp: Option<String>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub embeds: Vec<Embed>,
    #[serde(default)]
    pub message_reference: Option<MessageReference>,
    #[serde(default)]
    pub referenced_message: Option<Box<Message>>,
    #[serde(default)]
    pub reactions: Vec<Reaction>,
    #[serde(default)]
    pub mentions: Vec<User>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MessageReference {
    #[serde(default)]
    pub message_id: Option<String>,
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub guild_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Attachment {
    pub id: String,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub proxy_url: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

impl Attachment {
    pub fn is_image(&self) -> bool {
        self.content_type
            .as_deref()
            .map(|c| c.starts_with("image/"))
            .unwrap_or(false)
    }

    pub fn image_url(&self, max: u32) -> Option<String> {
        // Animated images should not be resized through the CDN resizer.
        let animated = self
            .content_type
            .as_deref()
            .map(|c| c == "image/gif" || c == "image/apng")
            .unwrap_or(false);
        let base = self.proxy_url.clone().or_else(|| self.url.clone())?;
        if animated {
            Some(base)
        } else {
            // Requesting only `width` lets Discord's media proxy resize while
            // preserving the aspect ratio (width+height would crop to a square).
            Some(format!("{base}&width={max}"))
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Embed {
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub image: Option<EmbedImage>,
    #[serde(default)]
    pub thumbnail: Option<EmbedImage>,
    #[serde(default)]
    pub video: Option<EmbedImage>,
    #[serde(default)]
    pub provider: Option<EmbedProvider>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EmbedProvider {
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EmbedImage {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub proxy_url: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

impl EmbedImage {
    /// Prefer the direct URL over Discord's proxy; the proxy can fail for
    /// external sites (e.g. YouTube embed thumbnails).
    pub fn best_url(&self) -> Option<String> {
        self.url.clone().or_else(|| self.proxy_url.clone())
    }
}
