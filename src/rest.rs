use std::sync::OnceLock;

use anyhow::{anyhow, Result};
use base64::Engine;
use reqwest::header::AUTHORIZATION;
use reqwest::Client;
use serde_json::json;

use crate::model::{Channel, Guild, Member, Message, Relationship, User, UserProfile};
use crate::state::PendingFile;

pub const API: &str = "https://discord.com/api/v10";
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

pub fn http_client() -> Client {
    Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .expect("failed to build reqwest client")
}

/// Base64-encoded `X-Super-Properties` header value, built once.
pub fn super_properties() -> &'static str {
    static SP: OnceLock<String> = OnceLock::new();
    SP.get_or_init(|| {
        let obj = json!({
            "os": "Windows",
            "browser": "Chrome",
            "device": "",
            "system_locale": "en-US",
            "has_client_mods": false,
            "client_version": "0.0.324",
            "os_version": "10",
            "referrer": "",
            "referring_domain": "",
            "referrer_current": "",
            "referring_domain_current": "",
            "release_channel": "stable",
            "client_build_number": 300000,
            "client_event_source": null
        });
        base64::engine::general_purpose::STANDARD.encode(obj.to_string())
    })
}

async fn ensure_ok(resp: reqwest::Response) -> Result<reqwest::Response> {
    if resp.status().is_success() {
        Ok(resp)
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(anyhow!("Discord API error {status}: {body}"))
    }
}

/// Fetch the currently authenticated user. Also validates the token.
pub async fn get_me(client: &Client, token: &str) -> Result<User> {
    let resp = client
        .get(format!("{API}/users/@me"))
        .header(AUTHORIZATION, token)
        .header("X-Super-Properties", super_properties())
        .header("X-Discord-Locale", "en-US")
        .header("X-Debug-Options", "bugReporterEnabled")
        .send()
        .await?;
    let resp = ensure_ok(resp).await?;
    Ok(resp.json::<User>().await?)
}

/// Fetch a user's full profile (includes bio). `guild_id` gives guild context
/// (roles, nickname, mutual info).
pub async fn get_user_profile(
    client: &Client,
    token: &str,
    user_id: &str,
    guild_id: Option<&str>,
) -> Result<UserProfile> {
    let mut query = vec![("with_mutual_guilds", "true".to_string())];
    if let Some(guild_id) = guild_id {
        query.push(("guild_id", guild_id.to_string()));
    }
    let resp = client
        .get(format!("{API}/users/{user_id}/profile"))
        .query(&query)
        .header(AUTHORIZATION, token)
        .header("X-Super-Properties", super_properties())
        .header("X-Discord-Locale", "en-US")
        .send()
        .await?;
    let resp = ensure_ok(resp).await?;
    Ok(resp.json::<UserProfile>().await?)
}

/// Open (or create) a DM channel with a user, returning the channel.
pub async fn open_dm(client: &Client, token: &str, user_id: &str) -> Result<Channel> {
    let resp = client
        .post(format!("{API}/users/@me/channels"))
        .header(AUTHORIZATION, token)
        .header("X-Super-Properties", super_properties())
        .header("X-Discord-Locale", "en-US")
        .json(&json!({ "recipient_id": user_id }))
        .send()
        .await?;
    let resp = ensure_ok(resp).await?;
    Ok(resp.json::<Channel>().await?)
}

/// Fetch the guilds the user is a member of (id + name for each).
pub async fn get_guilds(client: &Client, token: &str) -> Result<Vec<Guild>> {
    let resp = client
        .get(format!("{API}/users/@me/guilds"))
        .header(AUTHORIZATION, token)
        .header("X-Super-Properties", super_properties())
        .header("X-Discord-Locale", "en-US")
        .send()
        .await?;
    let resp = ensure_ok(resp).await?;
    Ok(resp.json::<Vec<Guild>>().await?)
}

/// Fetch the current user's relationships (friends, blocked, pending).
pub async fn get_relationships(client: &Client, token: &str) -> Result<Vec<Relationship>> {
    let resp = client
        .get(format!("{API}/users/@me/relationships"))
        .header(AUTHORIZATION, token)
        .header("X-Super-Properties", super_properties())
        .header("X-Discord-Locale", "en-US")
        .send()
        .await?;
    let resp = ensure_ok(resp).await?;
    Ok(resp.json::<Vec<Relationship>>().await?)
}

/// Fetch all channels in a guild.
pub async fn get_guild_channels(
    client: &Client,
    token: &str,
    guild_id: &str,
) -> Result<Vec<Channel>> {
    let resp = client
        .get(format!("{API}/guilds/{guild_id}/channels"))
        .header(AUTHORIZATION, token)
        .header("X-Super-Properties", super_properties())
        .header("X-Discord-Locale", "en-US")
        .send()
        .await?;
    let resp = ensure_ok(resp).await?;
    Ok(resp.json::<Vec<Channel>>().await?)
}

/// Fetch a single channel by id (used to resolve its guild when jumping from a
/// notification to a channel we haven't cached yet).
pub async fn get_channel(client: &Client, token: &str, channel_id: &str) -> Result<Channel> {
    let resp = client
        .get(format!("{API}/channels/{channel_id}"))
        .header(AUTHORIZATION, token)
        .header("X-Super-Properties", super_properties())
        .header("X-Discord-Locale", "en-US")
        .send()
        .await?;
    let resp = ensure_ok(resp).await?;
    Ok(resp.json::<Channel>().await?)
}

/// Fetch the pinned messages in a channel.
pub async fn get_pins(client: &Client, token: &str, channel_id: &str) -> Result<Vec<Message>> {
    let resp = client
        .get(format!("{API}/channels/{channel_id}/pins"))
        .header(AUTHORIZATION, token)
        .header("X-Super-Properties", super_properties())
        .header("X-Discord-Locale", "en-US")
        .send()
        .await?;
    let resp = ensure_ok(resp).await?;
    Ok(resp.json::<Vec<Message>>().await?)
}

/// Fetch a page of guild members (max 1000 per call). Pass the last member id
/// from the previous page as `after` to paginate.
pub async fn get_guild_members(
    client: &Client,
    token: &str,
    guild_id: &str,
    after: Option<&str>,
    limit: u32,
) -> Result<Vec<Member>> {
    let mut query = vec![("limit", limit.to_string())];
    if let Some(after) = after {
        query.push(("after", after.to_string()));
    }
    let resp = client
        .get(format!("{API}/guilds/{guild_id}/members"))
        .query(&query)
        .header(AUTHORIZATION, token)
        .header("X-Super-Properties", super_properties())
        .header("X-Discord-Locale", "en-US")
        .send()
        .await?;
    let resp = ensure_ok(resp).await?;
    Ok(resp.json::<Vec<Member>>().await?)
}

/// Fetch the user's private channels (DMs and group DMs).
pub async fn get_dm_channels(client: &Client, token: &str) -> Result<Vec<Channel>> {
    let resp = client
        .get(format!("{API}/users/@me/channels"))
        .header(AUTHORIZATION, token)
        .header("X-Super-Properties", super_properties())
        .header("X-Discord-Locale", "en-US")
        .send()
        .await?;
    let resp = ensure_ok(resp).await?;
    Ok(resp.json::<Vec<Channel>>().await?)
}

/// Fetch the most recent messages for a channel (returned oldest-first).
pub async fn get_messages(
    client: &Client,
    token: &str,
    channel_id: &str,
    limit: u32,
    before: Option<&str>,
) -> Result<Vec<Message>> {
    let mut query = vec![("limit", limit.to_string())];
    if let Some(before) = before {
        query.push(("before", before.to_string()));
    }
    let resp = client
        .get(format!("{API}/channels/{channel_id}/messages"))
        .query(&query)
        .header(AUTHORIZATION, token)
        .header("X-Super-Properties", super_properties())
        .header("X-Discord-Locale", "en-US")
        .send()
        .await?;
    let resp = ensure_ok(resp).await?;
    let mut messages = resp.json::<Vec<Message>>().await?;
    messages.reverse();
    Ok(messages)
}

/// Send a message to a channel, optionally as a reply.
pub async fn send_message(
    client: &Client,
    token: &str,
    channel_id: &str,
    content: &str,
    reply_to: Option<&str>,
) -> Result<Message> {
    let mut body = json!({ "content": content, "tts": false });
    if let Some(message_id) = reply_to {
        body["message_reference"] = json!({ "message_id": message_id });
    }
    let resp = client
        .post(format!("{API}/channels/{channel_id}/messages"))
        .header(AUTHORIZATION, token)
        .header("X-Super-Properties", super_properties())
        .header("X-Discord-Locale", "en-US")
        .json(&body)
        .send()
        .await?;
    let resp = ensure_ok(resp).await?;
    Ok(resp.json::<Message>().await?)
}

/// Upload one or more files as a new message (multipart form).
pub async fn send_attachments(
    client: &Client,
    token: &str,
    channel_id: &str,
    content: &str,
    files: &[PendingFile],
    reply_to: Option<&str>,
) -> Result<Message> {
    let mut payload = json!({ "content": content, "tts": false });
    if let Some(message_id) = reply_to {
        payload["message_reference"] = json!({ "message_id": message_id });
    }
    let mut form = reqwest::multipart::Form::new().text("payload_json", payload.to_string());
    for (i, file) in files.iter().enumerate() {
        let mut part =
            reqwest::multipart::Part::bytes(file.data.clone()).file_name(file.filename.clone());
        if let Some(ct) = &file.content_type {
            part = part.mime_str(ct)?;
        }
        form = form.part(format!("files[{i}]"), part);
    }
    let resp = client
        .post(format!("{API}/channels/{channel_id}/messages"))
        .header(AUTHORIZATION, token)
        .header("X-Super-Properties", super_properties())
        .header("X-Discord-Locale", "en-US")
        .multipart(form)
        .send()
        .await?;
    let resp = ensure_ok(resp).await?;
    Ok(resp.json::<Message>().await?)
}

/// Edit an existing message's content.
pub async fn edit_message(
    client: &Client,
    token: &str,
    channel_id: &str,
    message_id: &str,
    content: &str,
) -> Result<Message> {
    let resp = client
        .patch(format!("{API}/channels/{channel_id}/messages/{message_id}"))
        .header(AUTHORIZATION, token)
        .header("X-Super-Properties", super_properties())
        .header("X-Discord-Locale", "en-US")
        .json(&json!({ "content": content }))
        .send()
        .await?;
    let resp = ensure_ok(resp).await?;
    Ok(resp.json::<Message>().await?)
}

/// Percent-encode a single URL path segment (used for emoji keys).
fn encode_segment(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Add the current user's reaction to a message. `emoji` is a unicode emoji or
/// a `name:id` custom emoji key.
pub async fn add_reaction(
    client: &Client,
    token: &str,
    channel_id: &str,
    message_id: &str,
    emoji: &str,
) -> Result<()> {
    let emoji = encode_segment(emoji);
    let resp = client
        .put(format!(
            "{API}/channels/{channel_id}/messages/{message_id}/reactions/{emoji}/@me"
        ))
        .header(AUTHORIZATION, token)
        .header("X-Super-Properties", super_properties())
        .header("X-Discord-Locale", "en-US")
        .send()
        .await?;
    ensure_ok(resp).await?;
    Ok(())
}

/// Remove the current user's reaction from a message.
pub async fn remove_reaction(
    client: &Client,
    token: &str,
    channel_id: &str,
    message_id: &str,
    emoji: &str,
) -> Result<()> {
    let emoji = encode_segment(emoji);
    let resp = client
        .delete(format!(
            "{API}/channels/{channel_id}/messages/{message_id}/reactions/{emoji}/@me"
        ))
        .header(AUTHORIZATION, token)
        .header("X-Super-Properties", super_properties())
        .header("X-Discord-Locale", "en-US")
        .send()
        .await?;
    ensure_ok(resp).await?;
    Ok(())
}

/// Fire the "user is typing" indicator in a channel.
pub async fn trigger_typing(client: &Client, token: &str, channel_id: &str) -> Result<()> {
    let resp = client
        .post(format!("{API}/channels/{channel_id}/typing"))
        .header(AUTHORIZATION, token)
        .header("X-Super-Properties", super_properties())
        .header("X-Discord-Locale", "en-US")
        .send()
        .await?;
    ensure_ok(resp).await?;
    Ok(())
}

/// Mark a channel read up to `message_id` (the newest message). This is what
/// makes reads show up as consumed in every other Discord client.
pub async fn ack_message(
    client: &Client,
    token: &str,
    channel_id: &str,
    message_id: &str,
) -> Result<()> {
    let resp = client
        .post(format!("{API}/channels/{channel_id}/messages/{message_id}/ack"))
        .header(AUTHORIZATION, token)
        .header("X-Super-Properties", super_properties())
        .header("X-Discord-Locale", "en-US")
        .json(&json!({ "token": null }))
        .send()
        .await?;
    ensure_ok(resp).await?;
    Ok(())
}

/// Percent-encode an audit-log reason for the `X-Audit-Log-Reason` header.
fn encode_reason(reason: &str) -> Option<String> {
    let reason = reason.trim();
    if reason.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(reason.len());
    for &b in reason.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    Some(out)
}

/// Kick a member from a guild.
pub async fn kick_member(
    client: &Client,
    token: &str,
    guild_id: &str,
    user_id: &str,
    reason: Option<&str>,
) -> Result<()> {
    let mut req = client
        .delete(format!("{API}/guilds/{guild_id}/members/{user_id}"))
        .header(AUTHORIZATION, token)
        .header("X-Super-Properties", super_properties())
        .header("X-Discord-Locale", "en-US");
    if let Some(r) = reason.and_then(encode_reason) {
        req = req.header("X-Audit-Log-Reason", r);
    }
    let resp = req.send().await?;
    ensure_ok(resp).await?;
    Ok(())
}

/// Ban a user from a guild.
pub async fn ban_member(
    client: &Client,
    token: &str,
    guild_id: &str,
    user_id: &str,
    reason: Option<&str>,
) -> Result<()> {
    let mut req = client
        .put(format!("{API}/guilds/{guild_id}/bans/{user_id}"))
        .header(AUTHORIZATION, token)
        .header("X-Super-Properties", super_properties())
        .header("X-Discord-Locale", "en-US")
        .json(&json!({ "delete_message_seconds": 0 }));
    if let Some(r) = reason.and_then(encode_reason) {
        req = req.header("X-Audit-Log-Reason", r);
    }
    let resp = req.send().await?;
    ensure_ok(resp).await?;
    Ok(())
}

/// Timeout a member for `seconds` (0 clears the timeout).
pub async fn timeout_member(
    client: &Client,
    token: &str,
    guild_id: &str,
    user_id: &str,
    seconds: u64,
    reason: Option<&str>,
) -> Result<()> {
    let body = if seconds == 0 {
        json!({ "communication_disabled_until": serde_json::Value::Null })
    } else {
        let until = chrono::Utc::now() + chrono::Duration::seconds(seconds as i64);
        let s = until.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        json!({ "communication_disabled_until": s })
    };
    let mut req = client
        .patch(format!("{API}/guilds/{guild_id}/members/{user_id}"))
        .header(AUTHORIZATION, token)
        .header("X-Super-Properties", super_properties())
        .header("X-Discord-Locale", "en-US")
        .json(&body);
    if let Some(r) = reason.and_then(encode_reason) {
        req = req.header("X-Audit-Log-Reason", r);
    }
    let resp = req.send().await?;
    ensure_ok(resp).await?;
    Ok(())
}

/// Delete a message.
pub async fn delete_message(
    client: &Client,
    token: &str,
    channel_id: &str,
    message_id: &str,
) -> Result<()> {
    let resp = client
        .delete(format!("{API}/channels/{channel_id}/messages/{message_id}"))
        .header(AUTHORIZATION, token)
        .header("X-Super-Properties", super_properties())
        .header("X-Discord-Locale", "en-US")
        .send()
        .await?;
    ensure_ok(resp).await?;
    Ok(())
}
