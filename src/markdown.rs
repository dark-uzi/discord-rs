use crate::format::shortcode;
use crate::state::SharedState;

/// A rendered message segment produced by [`parse`].
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Seg {
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

/// Provides the id→name lookups needed while parsing mentions. Implemented by
/// [`SharedState`] for the real app and by a mock in unit tests.
pub(crate) trait NameResolver {
    fn user_name(&self, id: &str) -> Option<String>;
    fn channel_name(&self, id: &str) -> Option<String>;
    fn me_id(&self) -> Option<&str>;
}

impl NameResolver for SharedState {
    fn user_name(&self, id: &str) -> Option<String> {
        self.user_name(id)
    }
    fn channel_name(&self, id: &str) -> Option<String> {
        self.channel_name(id)
    }
    fn me_id(&self) -> Option<&str> {
        self.me.as_ref().map(|u| u.id.as_str())
    }
}

/// Tokenize a message body into a sequence of styled segments.
pub(crate) fn parse(text: &str, r: &impl NameResolver) -> Vec<Seg> {
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
                if let Some(seg) = parse_angle(&inner, r) {
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

fn parse_angle(inner: &str, r: &impl NameResolver) -> Option<Seg> {
    let is_me = |id: &str| r.me_id() == Some(id);
    if let Some(rest) = inner.strip_prefix("@!") {
        if rest.chars().all(|c| c.is_ascii_digit()) {
            let name = r.user_name(rest).unwrap_or_else(|| "user".to_string());
            return Some(Seg::Mention {
                text: format!("@{name}"),
                me: is_me(rest),
            });
        }
    }
    if let Some(rest) = inner.strip_prefix('@') {
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            let name = r.user_name(rest).unwrap_or_else(|| "user".to_string());
            return Some(Seg::Mention {
                text: format!("@{name}"),
                me: is_me(rest),
            });
        }
    }
    if let Some(rest) = inner.strip_prefix("#") {
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            let name = r.channel_name(rest).unwrap_or_else(|| "channel".to_string());
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

#[cfg(test)]
mod tests {
    use super::*;

    struct Mock;

    impl NameResolver for Mock {
        fn user_name(&self, id: &str) -> Option<String> {
            match id {
                "111" => Some("Alice".to_string()),
                "222" => Some("Bob".to_string()),
                _ => None,
            }
        }
        fn channel_name(&self, id: &str) -> Option<String> {
            match id {
                "333" => Some("general".to_string()),
                _ => None,
            }
        }
        fn me_id(&self) -> Option<&str> {
            Some("222")
        }
    }

    fn segs(text: &str) -> Vec<Seg> {
        parse(text, &Mock)
    }

    #[test]
    fn plain_text() {
        assert_eq!(segs("hello world"), vec![Seg::Plain("hello world".into())]);
    }

    #[test]
    fn bold_italic_strike_spoiler() {
        assert_eq!(segs("a **b** c"), vec![
            Seg::Plain("a ".into()),
            Seg::Bold("b".into()),
            Seg::Plain(" c".into()),
        ]);
        assert_eq!(segs("*x*"), vec![Seg::Italic("x".into())]);
        assert_eq!(segs("~~x~~"), vec![Seg::Strike("x".into())]);
        assert_eq!(segs("||x||"), vec![Seg::Spoiler("x".into())]);
    }

    #[test]
    fn underscore_only_at_word_boundary() {
        assert_eq!(segs("snake_case"), vec![Seg::Plain("snake_case".into())]);
        assert_eq!(segs("say _hi_"), vec![Seg::Plain("say ".into()), Seg::Italic("hi".into())]);
    }

    #[test]
    fn inline_code() {
        assert_eq!(segs("run `cargo test` now"), vec![
            Seg::Plain("run ".into()),
            Seg::Code("cargo test".into()),
            Seg::Plain(" now".into()),
        ]);
    }

    #[test]
    fn shortcodes_expanded() {
        assert_eq!(segs(":joy:"), vec![Seg::Plain("\u{1F602}".into())]);
        assert_eq!(segs(":nope:"), vec![Seg::Plain(":nope:".into())]);
    }

    #[test]
    fn user_mention_resolved() {
        assert_eq!(segs("<@111>"), vec![Seg::Mention { text: "@Alice".into(), me: false }]);
        assert_eq!(segs("<@222>"), vec![Seg::Mention { text: "@Bob".into(), me: true }]);
        assert_eq!(segs("<@!111>"), vec![Seg::Mention { text: "@Alice".into(), me: false }]);
        assert_eq!(segs("<@999>"), vec![Seg::Mention { text: "@user".into(), me: false }]);
    }

    #[test]
    fn channel_mention_resolved() {
        assert_eq!(segs("<#333>"), vec![Seg::Channel("#general".into())]);
        assert_eq!(segs("<#999>"), vec![Seg::Channel("#channel".into())]);
    }

    #[test]
    fn role_mention() {
        assert_eq!(segs("<@&789>"), vec![Seg::Mention { text: "@role".into(), me: false }]);
    }

    #[test]
    fn custom_emoji_parsed() {
        assert_eq!(segs("<:pepe:42>"), vec![Seg::Emoji { name: "pepe".into(), id: "42".into(), animated: false }]);
        assert_eq!(segs("<a:wave:7>"), vec![Seg::Emoji { name: "wave".into(), id: "7".into(), animated: true }]);
        // A malformed tag stays literal.
        assert_eq!(segs("<:no:id:extra>"), vec![Seg::Plain("<:no:id:extra>".into())]);
    }

    #[test]
    fn everyone_and_here() {
        assert_eq!(segs("hi @everyone"), vec![Seg::Plain("hi ".into()), Seg::Mention { text: "@everyone".into(), me: true }]);
        assert_eq!(segs("@here"), vec![Seg::Mention { text: "@here".into(), me: true }]);
        assert_eq!(segs("@everyoneelse"), vec![Seg::Plain("@everyoneelse".into())]);
    }

    #[test]
    fn urls_detected() {
        assert_eq!(segs("go http://x.example/a"), vec![
            Seg::Plain("go ".into()),
            Seg::Url("http://x.example/a".into()),
        ]);
        assert_eq!(segs("https://y.example"), vec![Seg::Url("https://y.example".into())]);
        assert_eq!(segs("not a link"), vec![Seg::Plain("not a link".into())]);
    }

    #[test]
    fn mixed_message() {
        let v = segs("<@111> **hi** :joy: https://x.example");
        assert_eq!(v[0], Seg::Mention { text: "@Alice".into(), me: false });
        assert_eq!(v[2], Seg::Bold("hi".into()));
        assert_eq!(v[4], Seg::Plain("\u{1F602}".into()));
        assert_eq!(v[6], Seg::Url("https://x.example".into()));
    }
}