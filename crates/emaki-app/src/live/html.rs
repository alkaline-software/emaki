//! The HTML among a text's words, from Onyx's `onyx_core::html`: a tag
//! (`<br>`, `<u>`, `<mark>`, `<span style="color: red">`) read for what
//! it does to the words it holds. `read_tag` says what one tag means.
//!
//! A whole block of HTML is not read here: the editor shows its lines as
//! they are written. Nothing here runs anything.

use std::ops::Range;

/// A color as a tag names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// What a tag among the words does to the words it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Style {
    Strong,
    Em,
    Underline,
    Strike,
    Mark,
    Code,
    Sub,
    Sup,
    Small,
    Link(String),
    /// An ink, a wash, or both.
    Color { ink: Option<Rgb>, wash: Option<Rgb> },
    /// No look of its own: the words stand as they are.
    Plain,
}

/// One tag, as written between `<` and `>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tag {
    Comment,
    Break,
    /// A picture: its source, and where its alt text stands in the tag.
    Image { src: String, alt: Range<usize> },
    Open { name: String, style: Style },
    Close { name: String },
    /// Not a tag: `<` and words.
    Not,
}

/// A tag's name, lowercased, and where its attributes begin.
pub(crate) fn name_of(s: &str) -> (String, usize) {
    let end = s.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).unwrap_or(s.len());
    (s[..end].to_ascii_lowercase(), end)
}

/// A tag's attributes: each name, lowercased, and its value's bytes in
/// `s`. A name with no value has an empty range at its end.
pub(crate) fn attrs(s: &str, from: usize) -> Vec<(String, Range<usize>)> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = from;
    while i < bytes.len() {
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b'/') {
            i += 1;
        }
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && !matches!(bytes[i], b'=' | b'>' | b'/') {
            i += 1;
        }
        if i == start {
            break;
        }
        let name = s[start..i].to_ascii_lowercase();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if bytes.get(i) != Some(&b'=') {
            out.push((name, i..i));
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        match bytes.get(i) {
            Some(&q @ (b'"' | b'\'')) => {
                let at = i + 1;
                let end = s[at..].find(q as char).map_or(s.len(), |e| at + e);
                out.push((name, at..end));
                i = (end + 1).min(s.len());
            }
            _ => {
                let at = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
                    i += 1;
                }
                out.push((name, at..i));
            }
        }
    }
    out
}

pub(crate) fn attr<'a>(s: &'a str, list: &[(String, Range<usize>)], name: &str) -> Option<&'a str> {
    list.iter().find(|(n, _)| n == name).map(|(_, r)| &s[r.clone()])
}

/// One declaration of a `style` attribute.
fn declared<'a>(style: &'a str, name: &str) -> Option<&'a str> {
    style.split(';').find_map(|d| {
        let (k, v) = d.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// A color as CSS writes it.
pub fn color(css: &str) -> Option<Rgb> {
    let css = css.trim().trim_end_matches("!important").trim().to_ascii_lowercase();
    if let Some(hex) = css.strip_prefix('#') {
        if !hex.is_ascii() {
            return None;
        }
        let pair = |a: usize| u8::from_str_radix(hex.get(a..a + 2)?, 16).ok();
        let one = |a: usize| u8::from_str_radix(hex.get(a..a + 1)?, 16).ok().map(|v| v * 17);
        return match hex.len() {
            3 | 4 => Some(Rgb(one(0)?, one(1)?, one(2)?)),
            6 | 8 => Some(Rgb(pair(0)?, pair(2)?, pair(4)?)),
            _ => None,
        };
    }
    if let Some(rest) = css.strip_prefix("rgb(").or_else(|| css.strip_prefix("rgba(")) {
        let parts: Vec<u8> = rest.trim_end_matches(')').split(|c: char| c == ',' || c == '/' || c.is_whitespace()).filter(|p| !p.is_empty()).take(3).filter_map(|p| p.parse::<f32>().ok()).map(|v| v.clamp(0., 255.) as u8).collect();
        return (parts.len() == 3).then(|| Rgb(parts[0], parts[1], parts[2]));
    }
    const NAMES: &[(&str, u32)] = &[
        ("black", 0x000000), ("silver", 0xc0c0c0), ("gray", 0x808080), ("grey", 0x808080), ("white", 0xffffff), ("maroon", 0x800000), ("red", 0xff0000), ("purple", 0x800080), ("fuchsia", 0xff00ff), ("magenta", 0xff00ff), ("green", 0x008000), ("lime", 0x00ff00), ("olive", 0x808000), ("yellow", 0xffff00), ("navy", 0x000080), ("blue", 0x0000ff), ("teal", 0x008080), ("aqua", 0x00ffff), ("cyan", 0x00ffff), ("orange", 0xffa500), ("pink", 0xffc0cb), ("brown", 0xa52a2a), ("gold", 0xffd700), ("indigo", 0x4b0082), ("violet", 0xee82ee), ("crimson", 0xdc143c), ("coral", 0xff7f50), ("salmon", 0xfa8072), ("tomato", 0xff6347), ("orangered", 0xff4500), ("skyblue", 0x87ceeb), ("steelblue", 0x4682b4), ("royalblue", 0x4169e1), ("seagreen", 0x2e8b57), ("forestgreen", 0x228b22), ("lightgray", 0xd3d3d3), ("lightgrey", 0xd3d3d3), ("darkgray", 0xa9a9a9), ("darkgrey", 0xa9a9a9), ("lightblue", 0xadd8e6), ("lightgreen", 0x90ee90), ("lightyellow", 0xffffe0),
    ];
    NAMES.iter().find(|(n, _)| *n == css).map(|(_, v)| Rgb((v >> 16) as u8, (v >> 8) as u8, *v as u8))
}

/// What one tag means, written whole with its `<` and `>`.
pub fn read_tag(tag: &str) -> Tag {
    if tag.starts_with("<!--") {
        return Tag::Comment;
    }
    let Some(inner) = tag.strip_prefix('<') else { return Tag::Not };
    if inner.starts_with('!') || inner.starts_with('?') {
        return Tag::Comment;
    }
    if let Some(close) = inner.strip_prefix('/') {
        let (name, _) = name_of(close);
        return if name.is_empty() { Tag::Not } else { Tag::Close { name } };
    }
    let (name, end) = name_of(inner);
    if name.is_empty() || !inner[..end].starts_with(|c: char| c.is_ascii_alphabetic()) {
        return Tag::Not;
    }
    let list = attrs(inner, end);
    let at = |name: &str| attr(inner, &list, name);
    let style = match name.as_str() {
        "br" | "wbr" => return Tag::Break,
        // An embedded file's words are its title, as a picture's are its alt.
        "img" | "embed" => {
            let words = if name == "img" { "alt" } else { "title" };
            let alt = list.iter().find(|(n, _)| n == words).map_or(0..0, |(_, r)| r.start + 1..r.end + 1);
            return Tag::Image { src: at("src").unwrap_or("").to_string(), alt };
        }
        "b" | "strong" => Style::Strong,
        "i" | "em" | "cite" | "dfn" | "var" => Style::Em,
        "u" | "ins" => Style::Underline,
        "s" | "del" | "strike" => Style::Strike,
        "mark" => Style::Mark,
        "code" | "kbd" | "samp" | "tt" => Style::Code,
        "sub" => Style::Sub,
        "sup" => Style::Sup,
        "small" => Style::Small,
        "a" => at("href").map_or(Style::Plain, |h| Style::Link(entities(h))),
        "font" => colored(at("color").and_then(color), None),
        _ => match at("style") {
            Some(css) => colored(declared(css, "color").and_then(color), declared(css, "background-color").or_else(|| declared(css, "background")).and_then(color)),
            None => Style::Plain,
        },
    };
    Tag::Open { name, style }
}

fn colored(ink: Option<Rgb>, wash: Option<Rgb>) -> Style {
    if ink.is_some() || wash.is_some() {
        Style::Color { ink, wash }
    } else {
        Style::Plain
    }
}

/// A text with HTML's entities written out.
pub fn entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let known = rest.find(';').filter(|e| *e <= 10).and_then(|e| entity(&rest[1..e]).map(|c| (c, e)));
        match known {
            Some((c, e)) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    if let Some(num) = name.strip_prefix('#') {
        let code = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse().ok()?,
        };
        return char::from_u32(code);
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "ndash" => '–',
        "mdash" => '—',
        "hellip" => '…',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "middot" => '·',
        "bull" => '•',
        "times" => '×',
        "rarr" => '→',
        "larr" => '←',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tag_says_what_it_does() {
        assert_eq!(read_tag("<br>"), Tag::Break);
        assert_eq!(read_tag("<BR />"), Tag::Break);
        assert_eq!(read_tag("<!-- note -->"), Tag::Comment);
        assert_eq!(read_tag("</u>"), Tag::Close { name: "u".into() });
        assert_eq!(read_tag("<u>"), Tag::Open { name: "u".into(), style: Style::Underline });
        assert_eq!(read_tag(r#"<a href="https://a.b/c?d=1&amp;e=2">"#), Tag::Open { name: "a".into(), style: Style::Link("https://a.b/c?d=1&e=2".into()) });
        assert_eq!(read_tag(r#"<span style="color: #f00; background-color: yellow">"#), Tag::Open { name: "span".into(), style: Style::Color { ink: Some(Rgb(255, 0, 0)), wash: Some(Rgb(255, 255, 0)) } });
        assert_eq!(read_tag(r#"<font color=red>"#), Tag::Open { name: "font".into(), style: Style::Color { ink: Some(Rgb(255, 0, 0)), wash: None } });
        assert_eq!(read_tag("<custom-thing>"), Tag::Open { name: "custom-thing".into(), style: Style::Plain });
        let tag = r#"<img src="a b.png" alt="A cat" width="300">"#;
        let Tag::Image { src, alt } = read_tag(tag) else { panic!() };
        assert_eq!((src.as_str(), &tag[alt]), ("a b.png", "A cat"));
        assert_eq!(read_tag("< x"), Tag::Not);
    }

    #[test]
    fn a_color_reads_as_css_writes_it() {
        assert_eq!(color("#abc"), Some(Rgb(0xaa, 0xbb, 0xcc)));
        assert_eq!(color("#a1b2c3"), Some(Rgb(0xa1, 0xb2, 0xc3)));
        assert_eq!(color("rgb(1, 2, 3)"), Some(Rgb(1, 2, 3)));
        assert_eq!(color("rgba(1 2 3 / 50%)"), Some(Rgb(1, 2, 3)));
        assert_eq!(color("Crimson"), Some(Rgb(0xdc, 0x14, 0x3c)));
        assert_eq!(color("var(--x)"), None);
        assert_eq!(color("#中"), None);
    }

    #[test]
    fn entities_are_written_out() {
        assert_eq!(entities("a &amp; b &lt;c&gt; &#65;&#x42; &nope; & d"), "a & b <c> AB &nope; & d");
    }

    #[test]
    fn a_broken_tag_never_panics() {
        for s in ["<", "<a", "<a href=\"", "<!--", "</", "<>", "<é>", "<a é=é>", "&", "&#xffffffff;", "<td>", "</td>", "<img alt=é", "<a href='é"] {
            let _ = read_tag(s);
            let _ = entities(s);
        }
    }
}
