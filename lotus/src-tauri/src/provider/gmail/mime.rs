//! MIME parsing and normalization.
//!
//! v1 renders `text/plain`. When a message has no text part, the HTML part goes
//! through the crude stripper below. That fallback is the common path, not the
//! rare one: marketing and transactional mail is very often HTML-only.
//!
//! The stripper is deliberately cheap. Links lose their href, nested lists lose
//! their structure, and tables read as runs of text. A good HTML-to-text
//! extractor is 1 to 2 days of block-level awareness and link footnoting, and
//! Phase 7 replaces this whole path with real HTML rendering, so that work would
//! be thrown away.

use mail_parser::{MessageParser, MimeHeaders};

pub struct ParsedMessage {
    pub sender_name: String,
    pub sender_email: String,
    pub subject: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub reply_to: Vec<String>,
    pub body_paragraphs: Vec<String>,
}

/// Never fails. A malformed or unsupported part degrades that one message to an
/// empty body with its headers intact, rather than failing the whole sync.
pub fn parse(raw: &[u8]) -> ParsedMessage {
    let Some(message) = MessageParser::default().parse(raw) else {
        return ParsedMessage {
            sender_name: String::new(),
            sender_email: String::new(),
            subject: "(no subject)".into(),
            to: Vec::new(),
            cc: Vec::new(),
            reply_to: Vec::new(),
            body_paragraphs: Vec::new(),
        };
    };

    // mail-parser decodes RFC 2047 encoded words for us, so a
    // =?UTF-8?B?...?= subject arrives already readable.
    let subject = message.subject().unwrap_or("").trim().to_string();

    let (sender_name, sender_email) = message
        .from()
        .and_then(|list| list.first())
        .map(|address| {
            let email = address.address().unwrap_or("").to_lowercase();
            let name = address.name().unwrap_or("").trim().to_string();
            (name, email)
        })
        .unwrap_or_default();

    // A bare From address has no display name. Fall back to the local part so
    // the list pane shows something, and the NOT NULL default in SQLite is never
    // the thing filling this in.
    let sender_name = if sender_name.is_empty() {
        local_part(&sender_email)
    } else {
        sender_name
    };

    // `body_text` cannot be trusted to mean "there is a text part": for an
    // HTML-only message mail-parser synthesizes one by stripping tags, and its
    // stripper keeps `<script>` and `<style>` contents. So look for a real
    // `text/plain` part, and fall through to our own stripper otherwise.
    let body_paragraphs = match plain_text_part(&message) {
        Some(text) if !text.trim().is_empty() => paragraphs(&text),
        _ => match message.body_html(0) {
            Some(html) => paragraphs(&html_to_text(&html)),
            None => Vec::new(),
        },
    };

    ParsedMessage {
        sender_name,
        sender_email,
        subject: if subject.is_empty() {
            "(no subject)".into()
        } else {
            subject
        },
        to: addresses(message.to()),
        cc: addresses(message.cc()),
        reply_to: addresses(message.reply_to()),
        body_paragraphs,
    }
}

/// The first genuine `text/plain` part. A part with no Content-Type at all
/// counts: a bare RFC 5322 message with a body is plain text by default.
fn plain_text_part(message: &mail_parser::Message<'_>) -> Option<String> {
    message
        .parts
        .iter()
        .find(|part| match part.content_type() {
            Some(content_type) => {
                content_type.ctype().eq_ignore_ascii_case("text")
                    && content_type
                        .subtype()
                        .map(|subtype| subtype.eq_ignore_ascii_case("plain"))
                        .unwrap_or(false)
            }
            None => true,
        })
        .and_then(|part| part.text_contents().map(|text| text.to_string()))
}

fn addresses(list: Option<&mail_parser::Address<'_>>) -> Vec<String> {
    list.map(|address| {
        address
            .iter()
            .filter_map(|entry| entry.address().map(|value| value.to_lowercase()))
            .collect()
    })
    .unwrap_or_default()
}

fn local_part(email: &str) -> String {
    email
        .split('@')
        .next()
        .unwrap_or(email)
        .replace(['.', '_', '-'], " ")
        .trim()
        .to_string()
}

/// Split on blank lines, collapsing runs of whitespace inside each paragraph.
/// Caps at 400 paragraphs: a long quoted chain otherwise produces a wire payload
/// the reading pane cannot use anyway.
fn paragraphs(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current: Vec<&str> = Vec::new();

    for line in text.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                out.push(
                    current
                        .join(" ")
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" "),
                );
                current.clear();
            }
        } else {
            current.push(line.trim());
        }
        if out.len() >= 400 {
            return out;
        }
    }

    if !current.is_empty() {
        out.push(
            current
                .join(" ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
        );
    }

    out.retain(|paragraph| !paragraph.is_empty());
    out
}

/// Drop script and style subtrees, turn block-level closes into newlines, strip
/// remaining tags, unwrap entities.
///
/// Works on `char`s, not bytes. Byte indexing here has two failure modes and
/// real mail hits both: casting a UTF-8 byte to `char` turns `é` into `Ã©`, and
/// slicing at a byte offset panics when the offset lands mid-character.
pub fn html_to_text(html: &str) -> String {
    let characters: Vec<char> = html.chars().collect();
    let mut out = String::with_capacity(html.len());
    let mut index = 0usize;

    while index < characters.len() {
        if characters[index] != '<' {
            out.push(characters[index]);
            index += 1;
            continue;
        }

        let Some(close) = find_char(&characters, index + 1, '>') else {
            // An unterminated tag at the end. Everything after `<` is markup we
            // cannot parse, so stop rather than emit it as text.
            break;
        };

        let name = tag_name(&characters[index + 1..close]);

        if matches!(name.as_str(), "script" | "style" | "head") {
            // Skip the whole subtree, not just the open tag.
            match find_closing_tag(&characters, close + 1, &name) {
                Some(end) => {
                    index = end;
                    continue;
                }
                None => break,
            }
        }

        if is_block_tag(&name) {
            out.push('\n');
            // A block boundary is a paragraph boundary: emit the blank line that
            // `paragraphs` splits on.
            if matches!(
                name.as_str(),
                "p" | "div" | "tr" | "li" | "h1" | "h2" | "h3"
            ) {
                out.push('\n');
            }
        }

        index = close + 1;
    }

    unescape_entities(&out)
}

/// The element name from a tag's inner text, lowercased. `</DIV class=x>` yields
/// `div`.
fn tag_name(tag: &[char]) -> String {
    tag.iter()
        .skip_while(|character| **character == '/')
        .take_while(|character| {
            !character.is_whitespace() && **character != '/' && **character != '>'
        })
        .flat_map(|character| character.to_lowercase())
        .collect()
}

fn is_block_tag(name: &str) -> bool {
    matches!(
        name,
        "p" | "div"
            | "br"
            | "tr"
            | "td"
            | "th"
            | "li"
            | "ul"
            | "ol"
            | "table"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "blockquote"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "hr"
    )
}

fn find_char(characters: &[char], from: usize, needle: char) -> Option<usize> {
    characters
        .get(from..)?
        .iter()
        .position(|character| *character == needle)
        .map(|offset| from + offset)
}

/// Index just past the `>` of the matching close tag. Scans the char slice
/// directly: lowercasing changes byte length for 27 Unicode characters, so an
/// offset found in a lowered copy cannot index the original.
fn find_closing_tag(characters: &[char], from: usize, name: &str) -> Option<usize> {
    let needle: Vec<char> = format!("</{name}").chars().collect();
    let mut index = from;

    while index + needle.len() <= characters.len() {
        let matches_here = characters[index..index + needle.len()]
            .iter()
            .zip(needle.iter())
            .all(|(actual, expected)| actual.to_lowercase().eq(expected.to_lowercase()));

        if matches_here {
            return find_char(characters, index + needle.len(), '>').map(|end| end + 1);
        }
        index += 1;
    }

    None
}

/// Unwrap HTML entities. Scans `char`s: the window after an `&` is bounded by
/// character count, not byte count, so a bare ampersand near accented text
/// cannot slice mid-character.
fn unescape_entities(value: &str) -> String {
    // The longest entity we recognize is `&hellip;`, and numeric forms run to
    // `&#x1F600;`. Ten characters after the `&` covers both.
    const MAX_ENTITY_CHARS: usize = 10;

    let characters: Vec<char> = value.chars().collect();
    let mut out = String::with_capacity(value.len());
    let mut index = 0usize;

    while index < characters.len() {
        if characters[index] != '&' {
            out.push(characters[index]);
            index += 1;
            continue;
        }

        let window_end = (index + 1 + MAX_ENTITY_CHARS).min(characters.len());
        let semicolon = characters[index + 1..window_end]
            .iter()
            .position(|character| *character == ';')
            .map(|offset| index + 1 + offset);

        let Some(end) = semicolon else {
            out.push('&');
            index += 1;
            continue;
        };

        let entity: String = characters[index + 1..end].iter().collect();
        let replacement = match entity.as_str() {
            "amp" => Some("&".to_string()),
            "lt" => Some("<".to_string()),
            "gt" => Some(">".to_string()),
            "quot" => Some("\"".to_string()),
            "apos" | "#39" => Some("'".to_string()),
            "nbsp" => Some(" ".to_string()),
            "mdash" => Some("-".to_string()),
            "ndash" => Some("-".to_string()),
            "hellip" => Some("...".to_string()),
            other => other
                .strip_prefix('#')
                .and_then(|digits| {
                    if let Some(hex) = digits
                        .strip_prefix('x')
                        .or_else(|| digits.strip_prefix('X'))
                    {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        digits.parse::<u32>().ok()
                    }
                })
                .and_then(char::from_u32)
                .map(|character| character.to_string()),
        };

        match replacement {
            Some(text) => {
                out.push_str(&text);
                index = end + 1;
            }
            // An unrecognized entity is left alone rather than swallowed.
            None => {
                out.push('&');
                index += 1;
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_message_yields_paragraphs_and_addresses() {
        let raw = b"From: Ada Lovelace <ada@example.com>\r\n\
                    To: reader@example.com\r\n\
                    Cc: cc@example.com\r\n\
                    Subject: Analytical Engine\r\n\
                    Content-Type: text/plain; charset=utf-8\r\n\r\n\
                    First paragraph.\r\n\r\nSecond paragraph.\r\n";
        let parsed = parse(raw);

        assert_eq!(parsed.sender_name, "Ada Lovelace");
        assert_eq!(parsed.sender_email, "ada@example.com");
        assert_eq!(parsed.subject, "Analytical Engine");
        assert_eq!(parsed.to, vec!["reader@example.com"]);
        assert_eq!(parsed.cc, vec!["cc@example.com"]);
        assert_eq!(
            parsed.body_paragraphs,
            vec!["First paragraph.", "Second paragraph."]
        );
    }

    #[test]
    fn rfc2047_encoded_subject_decodes() {
        let raw = b"From: a@b.com\r\n\
                    Subject: =?UTF-8?B?SGVsbG8g5LiA5LqM?=\r\n\
                    Content-Type: text/plain\r\n\r\nbody\r\n";
        assert_eq!(parse(raw).subject, "Hello 一二");
    }

    #[test]
    fn quoted_printable_eight_bit_body_decodes() {
        let raw = b"From: a@b.com\r\n\
                    Subject: Caf\xC3\xA9\r\n\
                    Content-Type: text/plain; charset=utf-8\r\n\
                    Content-Transfer-Encoding: quoted-printable\r\n\r\n\
                    Caf=C3=A9 na=C3=AFve\r\n";
        let parsed = parse(raw);
        assert!(parsed.body_paragraphs[0].contains("Café"));
        assert!(parsed.body_paragraphs[0].contains("naïve"));
    }

    #[test]
    fn bare_address_sender_falls_back_to_the_local_part() {
        let raw = b"From: no-reply@notifications.example.com\r\n\
                    Subject: Alert\r\n\
                    Content-Type: text/plain\r\n\r\nbody\r\n";
        let parsed = parse(raw);
        assert_eq!(parsed.sender_email, "no-reply@notifications.example.com");
        assert_eq!(parsed.sender_name, "no reply");
    }

    #[test]
    fn multipart_alternative_prefers_the_text_part() {
        let raw = b"From: a@b.com\r\n\
                    Subject: Both\r\n\
                    Content-Type: multipart/alternative; boundary=X\r\n\r\n\
                    --X\r\nContent-Type: text/plain\r\n\r\nThe plain part.\r\n\
                    --X\r\nContent-Type: text/html\r\n\r\n<p>The HTML part.</p>\r\n\
                    --X--\r\n";
        assert_eq!(parse(raw).body_paragraphs, vec!["The plain part."]);
    }

    #[test]
    fn html_only_message_falls_back_to_stripped_text() {
        let raw = b"From: a@b.com\r\n\
                    Subject: Marketing\r\n\
                    Content-Type: text/html; charset=utf-8\r\n\r\n\
                    <html><head><style>p{color:red}</style></head><body>\
                    <p>Big &amp; bold news</p><p>Second block</p>\
                    <script>track()</script></body></html>\r\n";
        let parsed = parse(raw);
        assert_eq!(
            parsed.body_paragraphs,
            vec!["Big & bold news", "Second block"]
        );
        // The point of dropping the subtrees: no CSS or JS in the reading pane.
        assert!(!parsed.body_paragraphs.join(" ").contains("color"));
        assert!(!parsed.body_paragraphs.join(" ").contains("track"));
    }

    #[test]
    fn message_with_no_body_keeps_its_subject() {
        let raw = b"From: a@b.com\r\nSubject: Header only\r\n\r\n";
        let parsed = parse(raw);
        assert_eq!(parsed.subject, "Header only");
        assert!(parsed.body_paragraphs.is_empty());
    }

    #[test]
    fn garbage_input_degrades_rather_than_panicking() {
        let parsed = parse(b"\x00\x01\x02 not a message at all");
        assert!(parsed.body_paragraphs.is_empty() || !parsed.subject.is_empty());
    }

    #[test]
    fn missing_subject_gets_a_placeholder() {
        let raw = b"From: a@b.com\r\nContent-Type: text/plain\r\n\r\nbody\r\n";
        assert_eq!(parse(raw).subject, "(no subject)");
    }

    #[test]
    fn numeric_and_hex_entities_unescape() {
        assert_eq!(unescape_entities("a&#65;b&#x42;c"), "aAbBc");
        assert_eq!(
            unescape_entities("5 &lt; 7 &amp;&amp; 8 &gt; 6"),
            "5 < 7 && 8 > 6"
        );
        // An unknown entity is left alone rather than swallowed.
        assert_eq!(unescape_entities("&notreal; x"), "&notreal; x");
    }

    /// Non-ASCII in an HTML-only body. Two things must hold: the text must not
    /// be corrupted, and the entity scanner must not slice mid-character.
    #[test]
    fn non_ascii_html_only_body_survives_intact() {
        let raw = "From: a@b.com\r\nSubject: X\r\n\
                   Content-Type: text/html; charset=utf-8\r\n\r\n\
                   <p>Dupont &amp; Fr\u{e8}re \u{2014} caf\u{e9} na\u{ef}ve</p>\r\n";
        let parsed = parse(raw.as_bytes());
        let body = parsed.body_paragraphs.join(" ");
        assert!(body.contains("Fr\u{e8}re"), "got: {body}");
        assert!(body.contains("caf\u{e9}"), "got: {body}");
        assert!(body.contains("na\u{ef}ve"), "got: {body}");
        assert!(body.contains('&'));
    }

    /// A bare ampersand followed closely by a multibyte character used to slice
    /// the entity-scan window mid-character.
    #[test]
    fn a_bare_ampersand_next_to_multibyte_text_does_not_panic() {
        assert_eq!(unescape_entities("a & \u{e9}b"), "a & \u{e9}b");
        assert_eq!(
            unescape_entities("&\u{2014}\u{2014}\u{2014}\u{2014}x"),
            "&\u{2014}\u{2014}\u{2014}\u{2014}x"
        );
        assert_eq!(
            unescape_entities("Dupont & Fr\u{e8}re"),
            "Dupont & Fr\u{e8}re"
        );
    }

    /// Lowercasing can change byte length, so an offset found in the lowered
    /// copy cannot index the original.
    #[test]
    fn subtree_skip_survives_characters_that_change_length_when_lowercased() {
        let html = "<style>\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}\u{130}</style>\u{20ac}ok";
        let text = html_to_text(html);
        assert!(text.contains("ok"), "got: {text:?}");
        assert!(!text.contains("\u{130}"), "style contents leaked: {text:?}");
    }

    #[test]
    fn unterminated_tag_does_not_loop_forever() {
        let text = html_to_text("<p>ok</p><div class=\"unclosed");
        assert!(text.contains("ok"));
    }
}
