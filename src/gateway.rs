use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::{interval, sleep_until, Instant, MissedTickBehavior};
use tokio_tungstenite::tungstenite::Message as WsMessage;

use crate::config;
use crate::model::{Channel, Emoji, Guild, Message, User};
use crate::rest;
use crate::state::{Command, ConnectionStatus, SharedState};

const GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=10&encoding=json";

// GUILDS | GUILD_MEMBERS | GUILD_MESSAGES | GUILD_MESSAGE_REACTIONS |
// DIRECT_MESSAGES | DIRECT_MESSAGE_REACTIONS | MESSAGE_CONTENT
const INTENTS: u64 = 1 | 2 | 512 | 1024 | 2048 | 4096 | 8192 | 16384 | 32768;

pub enum SessionEnd {
    Logout,
    Shutdown,
}

#[derive(Default)]
struct Session {
    session_id: Option<String>,
    resume_url: Option<String>,
    seq: Option<u64>,
    auth_failed: bool,
    ready_this_run: bool,
}

enum GatewayExit {
    Disconnect,
    Reconnect,
    Shutdown,
}

fn set_status(shared: &Arc<Mutex<SharedState>>, status: ConnectionStatus, error: Option<String>) {
    let mut s = shared.lock().unwrap();
    s.status = status;
    s.last_error = error;
}

/// Runs a session: validates the token, then connects and stays connected,
/// reconnecting (with RESUME where possible) until the user logs out.
pub async fn run_session(
    shared: Arc<Mutex<SharedState>>,
    client: reqwest::Client,
    token: String,
    cmd_rx: &mut UnboundedReceiver<Command>,
) -> Result<SessionEnd> {
    set_status(&shared, ConnectionStatus::Connecting, None);

    let me = match rest::get_me(&client, &token).await {
        Ok(user) => user,
        Err(e) => {
            set_status(
                &shared,
                ConnectionStatus::Error(e.to_string()),
                Some(e.to_string()),
            );
            return Err(e);
        }
    };
    shared.lock().unwrap().set_me(me);
    config::save_token(&token);

    let mut session = Session::default();
    let mut backoff = Duration::from_secs(1);

    loop {
        let result = run_gateway(&shared, &client, &token, cmd_rx, &mut session).await;

        if session.auth_failed {
            set_status(
                &shared,
                ConnectionStatus::Error("Authentication failed".into()),
                Some("Authentication failed - the token was rejected".into()),
            );
            return Err(anyhow!("authentication failed"));
        }

        match result {
            Ok(GatewayExit::Disconnect) => return Ok(SessionEnd::Logout),
            Ok(GatewayExit::Shutdown) => return Ok(SessionEnd::Shutdown),
            Ok(GatewayExit::Reconnect) => {}
            Err(e) => eprintln!("gateway error: {e:#}"),
        }

        if session.ready_this_run {
            backoff = Duration::from_secs(1);
        } else {
            backoff = (backoff * 2).min(Duration::from_secs(30));
        }

        shared.lock().unwrap().status = ConnectionStatus::Reconnecting;

        let deadline = Instant::now() + backoff;
        loop {
            tokio::select! {
                _ = sleep_until(deadline) => break,
                cmd = cmd_rx.recv() => match cmd {
                    Some(Command::Disconnect) => return Ok(SessionEnd::Logout),
                    None => return Ok(SessionEnd::Shutdown),
                    Some(_) => {}
                },
            }
        }
    }
}

async fn run_gateway(
    shared: &Arc<Mutex<SharedState>>,
    client: &reqwest::Client,
    token: &str,
    cmd_rx: &mut UnboundedReceiver<Command>,
    session: &mut Session,
) -> Result<GatewayExit> {
    session.ready_this_run = false;

    let can_resume = session.session_id.is_some() && session.seq.is_some();
    let url = match (&session.resume_url, can_resume) {
        (Some(url), true) if !url.contains('?') => format!("{url}/?v=10&encoding=json"),
        (Some(url), true) => url.clone(),
        _ => GATEWAY_URL.to_string(),
    };

    let (ws, _) = tokio_tungstenite::connect_async(&url).await?;
    let (mut sink, mut stream) = ws.split();

    // Wait for Hello.
    let hb_interval = loop {
        match stream.next().await {
            Some(Ok(WsMessage::Text(text))) => {
                let v: Value = serde_json::from_str(&text)?;
                if v["op"].as_u64() == Some(10) {
                    break v["d"]["heartbeat_interval"].as_u64().unwrap_or(41250);
                }
            }
            Some(Ok(_)) => continue,
            Some(Err(e)) => return Err(anyhow!(e)),
            None => return Err(anyhow!("gateway closed before Hello")),
        }
    };

    if can_resume {
        let resume = json!({
            "op": 6,
            "d": {
                "token": token,
                "session_id": session.session_id,
                "seq": session.seq,
            }
        });
        sink.send(WsMessage::Text(resume.to_string().into()))
            .await?;
    } else {
        let identify = json!({
            "op": 2,
            "d": {
                "token": token,
                "capabilities": 16381,
                "intents": INTENTS,
                "guild_subscriptions": true,
                "properties": {
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
                },
                "presence": { "status": "online", "since": 0, "activities": [], "afk": false },
                "compress": false,
                "client_state": {
                    "guild_versions": {},
                    "highest_last_message_id": "0",
                    "read_state_version": 0,
                    "user_guild_settings_version": -1,
                    "user_settings_version": -1,
                    "private_channels_version": "0",
                    "api_code_version": 0
                }
            }
        });
        sink.send(WsMessage::Text(identify.to_string().into()))
            .await?;
    }

    let mut heartbeat = interval(Duration::from_millis(hb_interval));
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
    heartbeat.tick().await; // consume the immediate first tick

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                let payload = json!({ "op": 1, "d": session.seq });
                sink.send(WsMessage::Text(payload.to_string().into())).await?;
            }
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(WsMessage::Text(text))) => {
                        let v: Value = match serde_json::from_str(&text) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                        if let Some(seq) = v["s"].as_u64() {
                            session.seq = Some(seq);
                        }
                        match v["op"].as_u64().unwrap_or(0) {
                            0 => {
                                let fresh_ready = handle_dispatch(shared, &v, session);
                                shared.lock().unwrap().wake();
                                if fresh_ready {
                                    spawn_fetch_guilds(client.clone(), token.to_string(), shared.clone());
                                    spawn_fetch_dms(client.clone(), token.to_string(), shared.clone());
                                    spawn_fetch_relationships(client.clone(), token.to_string(), shared.clone());
                                }
                            }
                            7 => {
                                eprintln!("gateway: reconnect requested");
                                return Ok(GatewayExit::Reconnect);
                            }
                            9 => {
                                eprintln!("gateway: invalid session, re-identifying");
                                session.session_id = None;
                                session.seq = None;
                                return Ok(GatewayExit::Reconnect);
                            }
                            _ => {}
                        }
                    }
                    Some(Ok(WsMessage::Close(frame))) => {
                        if let Some(f) = frame {
                            if u16::from(f.code) == 4004 {
                                session.auth_failed = true;
                            }
                        }
                        return Ok(GatewayExit::Reconnect);
                    }
                    None => return Ok(GatewayExit::Reconnect),
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return Err(anyhow!(e)),
                }
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(Command::SendMessage { channel_id, content, reply_to }) => {
                        let (client, token, shared) = (client.clone(), token.to_string(), shared.clone());
                        tokio::spawn(async move {
                            match rest::send_message(&client, &token, &channel_id, &content, reply_to.as_deref()).await {
                                Ok(m) => {
                                    shared.lock().unwrap().add_message(m.clone());
                                    // Mark our own message as read everywhere.
                                    if let Err(e) = rest::ack_message(&client, &token, &channel_id, &m.id).await {
                                        eprintln!("ack after send failed: {e:#}");
                                    }
                                }
                                Err(e) => {
                                    eprintln!("send message failed: {e:#}");
                                    shared.lock().unwrap().push_error(format!("Failed to send message: {e}"));
                                }
                            }
                        });
                    }
                    Some(Command::SendAttachments { channel_id, content, files, reply_to }) => {
                        let (client, token, shared) = (client.clone(), token.to_string(), shared.clone());
                        tokio::spawn(async move {
                            match rest::send_attachments(&client, &token, &channel_id, &content, &files, reply_to.as_deref()).await {
                                Ok(m) => {
                                    shared.lock().unwrap().add_message(m.clone());
                                    if let Err(e) = rest::ack_message(&client, &token, &channel_id, &m.id).await {
                                        eprintln!("ack after send failed: {e:#}");
                                    }
                                }
                                Err(e) => {
                                    eprintln!("send attachments failed: {e:#}");
                                    let msg = if e.to_string().contains("413") {
                                        "Upload failed: file is too large for this server.".to_string()
                                    } else {
                                        format!("Failed to upload attachment: {e}")
                                    };
                                    shared.lock().unwrap().push_error(msg);
                                }
                            }
                        });
                    }
                    Some(Command::EditMessage { channel_id, message_id, content }) => {
                        let (client, token, shared) = (client.clone(), token.to_string(), shared.clone());
                        tokio::spawn(async move {
                            match rest::edit_message(&client, &token, &channel_id, &message_id, &content).await {
                                Ok(m) => shared.lock().unwrap().add_message(m),
                                Err(e) => {
                                    eprintln!("edit message failed: {e:#}");
                                    shared.lock().unwrap().push_error(format!("Failed to edit message: {e}"));
                                }
                            }
                        });
                    }
                    Some(Command::DeleteMessage { channel_id, message_id }) => {
                        let (client, token, shared) = (client.clone(), token.to_string(), shared.clone());
                        tokio::spawn(async move {
                            match rest::delete_message(&client, &token, &channel_id, &message_id).await {
                                Ok(()) => shared.lock().unwrap().delete_message(&channel_id, &message_id),
                                Err(e) => {
                                    eprintln!("delete message failed: {e:#}");
                                    shared.lock().unwrap().push_error(format!("Failed to delete message: {e}"));
                                }
                            }
                        });
                    }
                    Some(Command::AddReaction { channel_id, message_id, emoji }) => {
                        let (client, token) = (client.clone(), token.to_string());
                        tokio::spawn(async move {
                            if let Err(e) = rest::add_reaction(&client, &token, &channel_id, &message_id, &emoji).await {
                                eprintln!("add reaction failed: {e:#}");
                            }
                        });
                    }
                    Some(Command::RemoveReaction { channel_id, message_id, emoji }) => {
                        let (client, token) = (client.clone(), token.to_string());
                        tokio::spawn(async move {
                            if let Err(e) = rest::remove_reaction(&client, &token, &channel_id, &message_id, &emoji).await {
                                eprintln!("remove reaction failed: {e:#}");
                            }
                        });
                    }
                    Some(Command::TriggerTyping(channel_id)) => {
                        let (client, token) = (client.clone(), token.to_string());
                        tokio::spawn(async move {
                            let _ = rest::trigger_typing(&client, &token, &channel_id).await;
                        });
                    }
                    Some(Command::FetchMessages(channel_id)) => {
                        spawn_fetch_messages(client.clone(), token.to_string(), shared.clone(), channel_id, None);
                    }
                    Some(Command::FetchOlder { channel_id, before }) => {
                        spawn_fetch_messages(client.clone(), token.to_string(), shared.clone(), channel_id, Some(before));
                    }
                    Some(Command::FetchGuildChannels(guild_id)) => {
                        spawn_fetch_channels(client.clone(), token.to_string(), shared.clone(), guild_id);
                    }
                    Some(Command::OpenChannel(channel_id)) => {
                        let (client, token, shared) = (client.clone(), token.to_string(), shared.clone());
                        tokio::spawn(async move {
                            match rest::get_channel(&client, &token, &channel_id).await {
                                Ok(ch) => {
                                    let guild_id = ch.guild_id.clone();
                                    {
                                        let mut s = shared.lock().unwrap();
                                        s.add_channel(ch);
                                        s.selected_channel = Some(channel_id.clone());
                                        match &guild_id {
                                            Some(g) => s.selected_guild = Some(g.clone()),
                                            None => {
                                                s.selected_guild =
                                                    Some(crate::state::DM_HOME.to_string())
                                            }
                                        }
                                    }
                                    if let Some(g) = guild_id {
                                        if !shared.lock().unwrap().has_channels(&g) {
                                            spawn_fetch_channels(
                                                client.clone(),
                                                token.to_string(),
                                                shared.clone(),
                                                g,
                                            );
                                        }
                                    }
                                }
                                Err(e) => eprintln!("open channel failed: {e:#}"),
                            }
                        });
                    }
                    Some(Command::FetchDms) => {
                        spawn_fetch_dms(client.clone(), token.to_string(), shared.clone());
                    }
                    Some(Command::FetchGuildMembers(guild_id)) => {
                        spawn_fetch_members(client.clone(), token.to_string(), shared.clone(), guild_id);
                    }
                    Some(Command::FetchPins(channel_id)) => {
                        spawn_fetch_pins(client.clone(), token.to_string(), shared.clone(), channel_id);
                    }
                    Some(Command::AckChannel(channel_id)) => {
                        let (client, token, shared) = (client.clone(), token.to_string(), shared.clone());
                        tokio::spawn(async move {
                            let last = {
                                let s = shared.lock().unwrap();
                                s.channel_last_message_id(&channel_id)
                            };
                            if let Some(last) = last {
                                if let Err(e) = rest::ack_message(&client, &token, &channel_id, &last).await {
                                    eprintln!("ack failed for {channel_id}: {e:#}");
                                }
                            }
                        });
                    }
                    Some(Command::KickUser { guild_id, user_id, reason }) => {
                        let (client, token) = (client.clone(), token.to_string());
                        tokio::spawn(async move {
                            if let Err(e) = rest::kick_member(&client, &token, &guild_id, &user_id, reason.as_deref()).await {
                                eprintln!("kick failed: {e:#}");
                            }
                        });
                    }
                    Some(Command::BanUser { guild_id, user_id, reason }) => {
                        let (client, token) = (client.clone(), token.to_string());
                        tokio::spawn(async move {
                            if let Err(e) = rest::ban_member(&client, &token, &guild_id, &user_id, reason.as_deref()).await {
                                eprintln!("ban failed: {e:#}");
                            }
                        });
                    }
                    Some(Command::TimeoutUser { guild_id, user_id, seconds, reason }) => {
                        let (client, token) = (client.clone(), token.to_string());
                        tokio::spawn(async move {
                            if let Err(e) = rest::timeout_member(&client, &token, &guild_id, &user_id, seconds, reason.as_deref()).await {
                                eprintln!("timeout failed: {e:#}");
                            }
                        });
                    }
                    Some(Command::FetchProfile { user_id, guild_id }) => {
                        spawn_fetch_profile(client.clone(), token.to_string(), shared.clone(), user_id, guild_id);
                    }
                    Some(Command::OpenDm(user_id)) => {
                        spawn_open_dm(client.clone(), token.to_string(), shared.clone(), user_id);
                    }
                    Some(Command::Disconnect) => {
                        let _ = sink.close().await;
                        return Ok(GatewayExit::Disconnect);
                    }
                    // A new Connect while already connected is ignored.
                    Some(Command::Connect(_)) => {}
                    None => return Ok(GatewayExit::Shutdown),
                }
            }
        }
    }
}

fn spawn_fetch_messages(
    client: reqwest::Client,
    token: String,
    shared: Arc<Mutex<SharedState>>,
    channel_id: String,
    before: Option<String>,
) {
    {
        let mut s = shared.lock().unwrap();
        s.loading_older.insert(channel_id.clone());
        s.mark_known(&channel_id);
    }
    tokio::spawn(async move {
        let result = rest::get_messages(&client, &token, &channel_id, 50, before.as_deref()).await;
        let mut s = shared.lock().unwrap();
        match result {
            Ok(messages) => {
                if messages.is_empty() {
                    s.reached_top.insert(channel_id.clone());
                } else {
                    for m in messages {
                        s.add_message(m);
                    }
                }
            }
            Err(e) => eprintln!("fetch messages failed: {e:#}"),
        }
        s.loading_older.remove(&channel_id);
    });
}

fn spawn_fetch_guilds(client: reqwest::Client, token: String, shared: Arc<Mutex<SharedState>>) {
    tokio::spawn(async move {
        match rest::get_guilds(&client, &token).await {
            Ok(guilds) => {
                let mut s = shared.lock().unwrap();
                for guild in guilds {
                    s.upsert_guild(guild);
                }
                s.sort_guilds();
                s.wake();
                eprintln!("loaded {} guilds via REST", s.guilds.len());
            }
            Err(e) => eprintln!("fetch guilds failed: {e:#}"),
        }
    });
}

fn spawn_fetch_relationships(
    client: reqwest::Client,
    token: String,
    shared: Arc<Mutex<SharedState>>,
) {
    tokio::spawn(async move {
        match rest::get_relationships(&client, &token).await {
            Ok(relationships) => {
                let mut s = shared.lock().unwrap();
                s.set_friends(relationships);
                let friends = s.friends.iter().filter(|r| r.kind == 1).count();
                eprintln!("loaded {} friends", friends);
            }
            Err(e) => eprintln!("fetch relationships failed: {e:#}"),
        }
    });
}

fn spawn_fetch_channels(
    client: reqwest::Client,
    token: String,
    shared: Arc<Mutex<SharedState>>,
    guild_id: String,
) {
    tokio::spawn(async move {
        match rest::get_guild_channels(&client, &token, &guild_id).await {
            Ok(channels) => {
                let mut s = shared.lock().unwrap();
                for channel in channels {
                    s.add_channel(channel);
                }
            }
            Err(e) => eprintln!("fetch channels failed: {e:#}"),
        }
    });
}

fn spawn_fetch_members(
    client: reqwest::Client,
    token: String,
    shared: Arc<Mutex<SharedState>>,
    guild_id: String,
) {
    tokio::spawn(async move {
        let mut after: Option<String> = None;
        let mut after_id: u64 = 0;
        let mut fetched = 0usize;
        loop {
            match rest::get_guild_members(&client, &token, &guild_id, after.as_deref(), 1000).await
            {
                Ok(members) => {
                    let n = members.len();
                    let page_max = members
                        .iter()
                        .filter_map(|m| m.user.id.parse::<u64>().ok())
                        .max()
                        .unwrap_or(after_id);
                    {
                        let mut s = shared.lock().unwrap();
                        for member in members {
                            s.users.insert(member.user.id.clone(), member.user.clone());
                            s.upsert_member(&guild_id, member);
                        }
                    }
                    fetched += n;
                    if n < 1000 || page_max <= after_id {
                        break;
                    }
                    after_id = page_max;
                    after = Some(page_max.to_string());
                }
                Err(e) => {
                    eprintln!("member list via REST unavailable for user tokens ({e})");
                    break;
                }
            }
        }
        let mut s = shared.lock().unwrap();
        s.finish_members_fetch(&guild_id);
        eprintln!("loaded {fetched} members for guild {guild_id}");
    });
}

fn spawn_fetch_pins(
    client: reqwest::Client,
    token: String,
    shared: Arc<Mutex<SharedState>>,
    channel_id: String,
) {
    tokio::spawn(async move {
        match rest::get_pins(&client, &token, &channel_id).await {
            Ok(pins) => {
                let mut s = shared.lock().unwrap();
                s.pins.insert(channel_id, pins);
            }
            Err(e) => eprintln!("fetch pins failed: {e:#}"),
        }
    });
}

fn spawn_fetch_profile(
    client: reqwest::Client,
    token: String,
    shared: Arc<Mutex<SharedState>>,
    user_id: String,
    guild_id: Option<String>,
) {
    tokio::spawn(async move {
        match rest::get_user_profile(&client, &token, &user_id, guild_id.as_deref()).await {
            Ok(profile) => shared.lock().unwrap().set_profile(profile),
            Err(e) => eprintln!("fetch profile failed: {e:#}"),
        }
    });
}

fn spawn_open_dm(
    client: reqwest::Client,
    token: String,
    shared: Arc<Mutex<SharedState>>,
    user_id: String,
) {
    tokio::spawn(async move {
        match rest::open_dm(&client, &token, &user_id).await {
            Ok(channel) => {
                let channel_id = channel.id.clone();
                {
                    let mut s = shared.lock().unwrap();
                    s.add_channel(channel);
                    s.selected_guild = Some(crate::state::DM_HOME.to_string());
                    s.selected_channel = Some(channel_id.clone());
                }
                spawn_fetch_messages(client, token, shared, channel_id, None);
            }
            Err(e) => eprintln!("open DM failed: {e:#}"),
        }
    });
}

fn spawn_fetch_dms(client: reqwest::Client, token: String, shared: Arc<Mutex<SharedState>>) {
    tokio::spawn(async move {
        match rest::get_dm_channels(&client, &token).await {
            Ok(channels) => {
                let mut s = shared.lock().unwrap();
                for channel in channels {
                    s.add_channel(channel);
                }
                eprintln!("loaded {} DM channels via REST", s.dm_channels().len());
            }
            Err(e) => eprintln!("fetch DMs failed: {e:#}"),
        }
    });
}

/// Append a dispatch event name to a debug log (best-effort).
fn log_event(event: &str) {
    use std::io::Write;
    use std::sync::Once;
    static INIT: Once = Once::new();
    let path = std::env::temp_dir().join("discord-rs-events.log");
    INIT.call_once(|| {
        let _ = std::fs::write(&path, b"");
    });
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{event}");
    }
}

/// Handles a gateway dispatch (op 0) event. Returns `true` when a fresh READY
/// was received (so callers can (re)load guilds/DMs over REST).
fn handle_dispatch(shared: &Arc<Mutex<SharedState>>, v: &Value, session: &mut Session) -> bool {
    let event = v["t"].as_str().unwrap_or_default();
    let data = &v["d"];
    log_event(event);

    match event {
"READY" => {
            let mut s = shared.lock().unwrap();
            s.status = ConnectionStatus::Connected;
            if let Some(sid) = data["session_id"].as_str() {
                session.session_id = Some(sid.to_string());
            }
            if let Some(url) = data["resume_gateway_url"].as_str() {
                session.resume_url = Some(url.to_string());
            }
            if let Ok(user) = serde_json::from_value::<User>(data["user"].clone()) {
                s.set_me(user);
            }
            if let Some(guilds) = data["guilds"].as_array() {
                for g in guilds {
                    if let Ok(guild) = serde_json::from_value::<Guild>(g.clone()) {
                        s.upsert_guild(guild);
                    }
                }
            }
            s.sort_guilds();
            // Seed unread/mention counts from the last time other clients read.
            if let Some(rs) = data["read_state"].as_array() {
                let entries = rs
                    .iter()
                    .filter_map(|e| {
                        let id = e["id"].as_str()?.to_string();
                        let mentions = e["mention_count"].as_u64().unwrap_or(0) as u32;
                        Some((id, mentions))
                    });
                s.seed_read_state(entries);
            }
            // Friend / DM presence snapshot (only some payloads include it).
            if let Some(presences) = data["presences"].as_array() {
                for p in presences {
                    if let (Some(uid), Some(status)) =
                        (p["user"]["id"].as_str(), p["status"].as_str())
                    {
                        s.presences.insert(uid.to_string(), status.to_string());
                    }
                }
            }
            session.ready_this_run = true;
            eprintln!("gateway: READY with {} guilds", s.guilds.len());
            return true;
        }
        "RESUMED" => {
            shared.lock().unwrap().status = ConnectionStatus::Connected;
            session.ready_this_run = true;
            eprintln!("gateway: RESUMED");
        }
        "GUILD_CREATE" => {
            if let Ok(guild) = serde_json::from_value::<Guild>(data.clone()) {
                let guild_id = guild.id.clone();
                let has_members = !guild.members.is_empty();
                let mut s = shared.lock().unwrap();
                for channel in &guild.channels {
                    s.add_channel(channel.clone());
                }
                for member in &guild.members {
                    s.users.insert(member.user.id.clone(), member.user.clone());
                }
                s.upsert_guild(guild);
                s.sort_guilds();
                if has_members {
                    s.finish_members_fetch(&guild_id);
                }
            }
        }
        "GUILD_POSITION_UPDATE" => {
            // Server reordered in another client; reflect it immediately.
            if let (Some(guild_id), Some(pos)) = (
                data["guild_id"].as_str(),
                data["position"].as_i64(),
            ) {
                let mut s = shared.lock().unwrap();
                if let Some(g) = s.guilds.iter_mut().find(|g| g.id == guild_id) {
                    g.position = pos as i32;
                }
                s.sort_guilds();
            }
        }
        "GUILD_MEMBER_ADD" | "GUILD_MEMBER_UPDATE" => {
            if let (Some(guild_id), Ok(member)) = (
                data["guild_id"].as_str(),
                serde_json::from_value::<crate::model::Member>(data.clone()),
            ) {
                let mut s = shared.lock().unwrap();
                s.users.insert(member.user.id.clone(), member.user.clone());
                s.upsert_member(guild_id, member);
            }
        }
        "PRESENCE_UPDATE" => {
            if let Some(user_id) = data["user"]["id"].as_str() {
                let status = data["status"].as_str().unwrap_or("offline");
                let guild_id = data["guild_id"].as_str().unwrap_or("");
                shared.lock().unwrap().set_presence(guild_id, user_id, status);
            }
        }
        "CHANNEL_CREATE" | "CHANNEL_UPDATE" => {
            if let Ok(channel) = serde_json::from_value::<Channel>(data.clone()) {
                shared.lock().unwrap().add_channel(channel);
            }
        }
        "MESSAGE_CREATE" => {
            if let Ok(message) = serde_json::from_value::<Message>(data.clone()) {
                shared.lock().unwrap().receive_message(message);
            }
            return false;
        }
        "TYPING_START" => {
            if let (Some(channel_id), Some(user_id)) =
                (data["channel_id"].as_str(), data["user_id"].as_str())
            {
                let mut s = shared.lock().unwrap();
                let is_me = s.me.as_ref().map(|u| u.id.as_str()) == Some(user_id);
                if !is_me {
                    let name = data["member"]["user"]["global_name"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .or_else(|| data["member"]["user"]["username"].as_str())
                        .map(|s| s.to_string())
                        .or_else(|| s.users.get(user_id).map(|u| u.display_name()))
                        .or_else(|| {
                            s.channels
                                .get(channel_id)
                                .and_then(|c| c.recipients.iter().find(|r| r.id == user_id))
                                .map(|r| r.display_name())
                        })
                        .unwrap_or_else(|| "Someone".to_string());
                    s.note_typing(channel_id, user_id, name);
                }
            }
        }
        "MESSAGE_REACTION_ADD" => {
            if let (Some(channel_id), Some(message_id), Some(user_id), Ok(emoji)) = (
                data["channel_id"].as_str(),
                data["message_id"].as_str(),
                data["user_id"].as_str(),
                serde_json::from_value::<Emoji>(data["emoji"].clone()),
            ) {
                let mut s = shared.lock().unwrap();
                let from_me = s.me.as_ref().map(|u| u.id.as_str()) == Some(user_id);
                s.react_add(channel_id, message_id, emoji, from_me);
            }
        }
        "MESSAGE_REACTION_REMOVE" => {
            if let (Some(channel_id), Some(message_id), Some(user_id), Ok(emoji)) = (
                data["channel_id"].as_str(),
                data["message_id"].as_str(),
                data["user_id"].as_str(),
                serde_json::from_value::<Emoji>(data["emoji"].clone()),
            ) {
                let mut s = shared.lock().unwrap();
                let from_me = s.me.as_ref().map(|u| u.id.as_str()) == Some(user_id);
                s.react_remove(channel_id, message_id, emoji, from_me);
            }
        }
        "MESSAGE_UPDATE" => {
            if let (Some(channel_id), Some(message_id)) =
                (data["channel_id"].as_str(), data["id"].as_str())
            {
                if let Some(content) = data["content"].as_str() {
                    let edited = data["edited_timestamp"].as_str().map(|s| s.to_string());
                    shared
                        .lock()
                        .unwrap()
                        .update_message(channel_id, message_id, content.to_string(), edited);
                }
            }
        }
        "MESSAGE_DELETE" => {
            if let (Some(channel_id), Some(message_id)) =
                (data["channel_id"].as_str(), data["id"].as_str())
            {
                shared.lock().unwrap().delete_message(channel_id, message_id);
            }
        }
        _ => {}
    }

    false
}
