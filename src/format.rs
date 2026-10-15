use chrono::{DateTime, Local, NaiveDate};

pub(crate) enum EmojiTok {
    Unicode(String),
    Custom(String),
}

pub(crate) fn is_emoji_char(c: char) -> bool {
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

pub(crate) fn is_emoji_base(c: char) -> bool {
    let u = c as u32;
    is_emoji_char(c) && !matches!(u, 0xFE0F | 0x200D | 0x20E3)
}

pub(crate) fn is_regional_indicator(c: char) -> bool {
    let u = c as u32;
    (0x1F1E6..=0x1F1FF).contains(&u)
}

pub(crate) fn twemoji_url(s: &str) -> Option<String> {
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

pub(crate) enum EmojiPiece {
    Text(String),
    Emoji(String),
}

/// Split a string into runs of plain text and emoji sequences, so emoji can be
/// rendered as Twemoji images rather than relying on a system emoji font.
pub(crate) fn split_emoji(s: &str) -> Vec<EmojiPiece> {
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

/// Parse Discord's inline custom-emoji syntax `<:name:id>` / `<a:name:id>` and
/// return the CDN image URL. Returns `None` for anything else.
pub(crate) fn custom_emoji_in(tok: &str) -> Option<String> {
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

pub(crate) const EMOJIS: &[(&str, &str)] = &[
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

pub(crate) fn shortcode(name: &str) -> Option<&'static str> {
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

/// When a message is short and emoji-only this returns up to three tokens so
/// the message can be rendered at a large size.
pub(crate) fn big_emoji(content: &str) -> Option<Vec<EmojiTok>> {
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
            let u = shortcode(name)?;
            tokens.push(EmojiTok::Unicode(u.to_string()));
            continue;
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

pub(crate) fn typing_label(names: &[String]) -> String {
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

pub(crate) fn is_image_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

pub(crate) fn youtube_id(url: &str) -> Option<String> {
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

pub(crate) fn parse_ts(timestamp: &Option<String>) -> Option<DateTime<Local>> {
    timestamp
        .as_ref()
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .map(|dt| dt.with_timezone(&Local))
}

#[allow(dead_code)]
pub(crate) fn date_of(ts: Option<DateTime<Local>>) -> Option<NaiveDate> {
    ts.map(|t| t.date_naive())
}

pub(crate) fn collect_content_image_uris(content: &str, out: &mut Vec<String>) {
    if content.is_empty() {
        return;
    }
    for piece in split_emoji(content) {
        if let EmojiPiece::Emoji(e) = piece {
            if let Some(url) = twemoji_url(&e) {
                out.push(url);
            }
        }
    }
    let mut rest = content;
    while let Some(start) = rest.find('<') {
        rest = &rest[start..];
        if let Some(end) = rest.find('>') {
            let tok = &rest[..=end];
            if let Some(url) = custom_emoji_in(tok) {
                out.push(url);
            }
            rest = &rest[end + 1..];
        } else {
            break;
        }
    }
}

pub(crate) fn preview_text(content: &str) -> String {
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

pub(crate) fn active_shortcode(s: &str) -> Option<(usize, String)> {
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
pub(crate) fn active_autocomplete(s: &str) -> Option<(usize, char, String)> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_emoji_flags() {
        let pieces = split_emoji("hi 🇺🇸");
        assert_eq!(pieces.len(), 2);
        match &pieces[0] {
            EmojiPiece::Text(t) => assert_eq!(t, "hi "),
            _ => panic!("expected text"),
        }
        match &pieces[1] {
            EmojiPiece::Emoji(e) => assert_eq!(e, "🇺🇸"),
            _ => panic!("expected emoji"),
        }
    }

    #[test]
    fn split_emoji_zwj_family() {
        let family = "👨\u{200D}👩\u{200D}👧";
        let pieces = split_emoji(family);
        assert_eq!(pieces.len(), 1);
        assert!(matches!(&pieces[0], EmojiPiece::Emoji(e) if e == family));
    }

    #[test]
    fn split_emoji_plain_text() {
        let pieces = split_emoji("just text");
        assert_eq!(pieces.len(), 1);
        assert!(matches!(&pieces[0], EmojiPiece::Text(t) if t == "just text"));
    }

    #[test]
    fn split_emoji_skin_tone() {
        let ok_hand = "👌\u{1F3FD}";
        let pieces = split_emoji(ok_hand);
        assert_eq!(pieces.len(), 1);
        assert!(matches!(&pieces[0], EmojiPiece::Emoji(e) if e == ok_hand));
    }

    #[test]
    fn twemoji_strips_variation_selector() {
        let heart = "\u{2764}\u{FE0F}";
        let url = twemoji_url(heart).expect("url");
        assert!(url.contains("/2764.png"), "{url}");
    }

    #[test]
    fn twemoji_empty_is_none() {
        assert!(twemoji_url("").is_none());
    }

    #[test]
    fn shortcode_lookup() {
        assert_eq!(shortcode("joy"), Some("\u{1F602}"));
        assert_eq!(shortcode("+1"), Some("\u{1F44D}"));
        assert_eq!(shortcode("not_a_real_emoji"), None);
    }

    #[test]
    fn custom_emoji_parser() {
        let gif = custom_emoji_in("<a:wave:123456>").expect("animated");
        assert!(gif.ends_with("/123456.gif"), "{gif}");
        let png = custom_emoji_in("<:ok:42>").expect("static");
        assert!(png.ends_with("/42.png"), "{png}");
        assert!(custom_emoji_in(":wave:").is_none());
        assert!(custom_emoji_in("<:bad:notnum>").is_none());
        assert!(custom_emoji_in("plain").is_none());
    }

    #[test]
    fn big_emoji_shortcodes() {
        let toks = big_emoji(":joy: :fire:").expect("tokens");
        assert_eq!(toks.len(), 2);
        assert!(matches!(&toks[0], EmojiTok::Unicode(u) if u == "\u{1F602}"));
    }

    #[test]
    fn big_emoji_custom() {
        let toks = big_emoji("<:ok:42>").expect("tokens");
        assert_eq!(toks.len(), 1);
        assert!(matches!(&toks[0], EmojiTok::Custom(url) if url.ends_with("/42.png")));
    }

    #[test]
    fn big_emoji_rejects_text() {
        assert!(big_emoji("hello world").is_none());
        assert!(big_emoji("").is_none());
        assert!(big_emoji(":joy: :fire: :heart: :sob:").is_none());
        let long = "x".repeat(81);
        assert!(big_emoji(&long).is_none());
    }

    #[test]
    fn is_image_path_extensions() {
        assert!(is_image_path("photo.PNG"));
        assert!(is_image_path("img.webp"));
        assert!(is_image_path("a/b/c.gif"));
        assert!(!is_image_path("notes.txt"));
        assert!(!is_image_path("image.png.exe"));
    }

    #[test]
    fn youtube_ids() {
        let id = "dQw4w9WgXcQ";
        assert_eq!(youtube_id(&format!("https://youtube.com/watch?v={id}")), Some(id.into()));
        assert_eq!(youtube_id(&format!("https://youtu.be/{id}")), Some(id.into()));
        assert_eq!(youtube_id(&format!("https://youtube.com/shorts/{id}")), Some(id.into()));
        assert!(youtube_id("https://example.com/watch?v=123").is_none());
        assert!(youtube_id("https://youtube.com/watch?v=short").is_none());
    }

    #[test]
    fn preview_text_rules() {
        assert_eq!(preview_text(""), "[attachment]");
        assert_eq!(preview_text("  hi  "), "hi");
        let long = "a".repeat(90);
        let p = preview_text(&long);
        assert_eq!(p.chars().count(), 81);
        assert!(p.ends_with('…'));
    }

    #[test]
    fn parse_timestamps() {
        assert!(parse_ts(&None).is_none());
        assert!(parse_ts(&Some("garbage".to_string())).is_none());
        assert!(parse_ts(&Some("2026-01-31T12:00:00Z".to_string())).is_some());
        assert!(parse_ts(&Some("2026-01-31T12:00:00+02:00".to_string())).is_some());
    }

    #[test]
    fn date_of_handles_none_and_some() {
        assert!(date_of(None).is_none());
        let ts = parse_ts(&Some("2026-01-31T12:00:00Z".to_string()));
        assert!(date_of(ts).is_some());
    }

    #[test]
    fn typing_labels() {
        assert_eq!(typing_label(&[]), "");
        assert_eq!(typing_label(&["Alice".into()]), "Alice is typing\u{2026}");
        assert_eq!(
            typing_label(&["Alice".into(), "Bob".into()]),
            "Alice and Bob are typing\u{2026}"
        );
        let many = vec!["A".into(), "B".into(), "C".into(), "D".into()];
        assert_eq!(typing_label(&many), "A, B and 2 others are typing\u{2026}");
    }

    #[test]
    fn active_shortcode_detection() {
        assert_eq!(active_shortcode("hello :jo"), Some((6, "jo".to_string())));
        assert_eq!(active_shortcode(":joy:"), None);
        assert_eq!(active_shortcode("noleading"), None);
        assert_eq!(active_shortcode("x:y"), None);
    }

    #[test]
    fn active_autocomplete_detection() {
        assert_eq!(active_autocomplete("hi @ali"), Some((3, '@', "ali".to_string())));
        assert_eq!(active_autocomplete("see #gen"), Some((4, '#', "gen".to_string())));
        assert_eq!(active_autocomplete("email@x"), None);
        assert_eq!(active_autocomplete("done "), None);
    }
}