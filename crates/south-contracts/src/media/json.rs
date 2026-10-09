//! The order- and text-preserving JSON tree the elider and the template expander share.
//!
//! `serde_json::Value` is not usable here: without its `preserve_order` feature it re-sorts object
//! members, and it re-renders numbers, while the view a component sees must keep source order and
//! copy every number as its source text, byte for byte the same on two hosts (image record §6.2).
//! This module therefore parses RFC 8259 JSON into a small tree that keeps each scalar's source
//! text and each member's source position, and serializes it compactly.
//!
//! The grammar is strict: no comments, no trailing commas, no leading zeros, no lone surrogates
//! in escapes, and nothing after the one top-level value but whitespace. Depth is bounded so a
//! hostile document cannot exhaust the stack.

use std::fmt::Write as _;

/// The deepest nesting a document may have; deeper is refused rather than recursed into.
pub const MAX_MEDIA_JSON_DEPTH: usize = 128;

/// One parsed JSON value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// `null`, `true`, `false` or a number, kept as its exact source text.
    Scalar(String),
    /// A string: its source text between the quotes (escapes intact) and its decoded value.
    String(JsonString),
    /// An array, in source order.
    Array(Vec<Self>),
    /// An object, members in source order.
    Object(Vec<Member>),
}

/// A JSON string as written and as meant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonString {
    /// The text between the quotes, escapes intact.
    pub raw: String,
    /// The decoded value.
    pub decoded: String,
}

/// One object member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub key: JsonString,
    pub value: Node,
}

/// Why a document is not one strict JSON value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    /// Not UTF-8, or not one RFC 8259 value.
    Syntax,
    /// Nested deeper than [`MAX_MEDIA_JSON_DEPTH`].
    TooDeep,
}

/// Parses one strict JSON document.
pub fn parse(document: &[u8]) -> Result<Node, ParseError> {
    let text = std::str::from_utf8(document).map_err(|_| ParseError::Syntax)?;
    let mut parser = Parser { text, bytes: text.as_bytes(), at: 0 };
    parser.skip_whitespace();
    let node = parser.value(0)?;
    parser.skip_whitespace();
    if parser.at != parser.bytes.len() {
        return Err(ParseError::Syntax);
    }
    Ok(node)
}

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), ParseError> {
        if self.peek() == Some(byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(ParseError::Syntax)
        }
    }

    fn value(&mut self, depth: usize) -> Result<Node, ParseError> {
        match self.peek().ok_or(ParseError::Syntax)? {
            b'{' => self.object(depth + 1),
            b'[' => self.array(depth + 1),
            b'"' => self.string().map(Node::String),
            b't' => self.literal("true"),
            b'f' => self.literal("false"),
            b'n' => self.literal("null"),
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(ParseError::Syntax),
        }
    }

    fn literal(&mut self, word: &str) -> Result<Node, ParseError> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(Node::Scalar(word.to_owned()))
        } else {
            Err(ParseError::Syntax)
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.at;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        self.at - start
    }

    fn number(&mut self) -> Result<Node, ParseError> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        match self.peek() {
            Some(b'0') => self.at += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(ParseError::Syntax),
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            if self.digits() == 0 {
                return Err(ParseError::Syntax);
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if self.digits() == 0 {
                return Err(ParseError::Syntax);
            }
        }
        Ok(Node::Scalar(self.text[start..self.at].to_owned()))
    }

    fn string(&mut self) -> Result<JsonString, ParseError> {
        self.expect(b'"')?;
        let start = self.at;
        let mut decoded = String::new();
        loop {
            let byte = self.peek().ok_or(ParseError::Syntax)?;
            match byte {
                b'"' => {
                    let raw = self.text[start..self.at].to_owned();
                    self.at += 1;
                    return Ok(JsonString { raw, decoded });
                }
                b'\\' => {
                    self.at += 1;
                    let escape = self.peek().ok_or(ParseError::Syntax)?;
                    self.at += 1;
                    match escape {
                        b'"' => decoded.push('"'),
                        b'\\' => decoded.push('\\'),
                        b'/' => decoded.push('/'),
                        b'b' => decoded.push('\u{8}'),
                        b'f' => decoded.push('\u{c}'),
                        b'n' => decoded.push('\n'),
                        b'r' => decoded.push('\r'),
                        b't' => decoded.push('\t'),
                        b'u' => decoded.push(self.unicode_escape()?),
                        _ => return Err(ParseError::Syntax),
                    }
                }
                0x00..=0x1f => return Err(ParseError::Syntax),
                _ => {
                    // Copy the whole UTF-8 sequence; `text` is valid UTF-8, so the char boundary
                    // after `at` is the next one `chars()` yields.
                    let character =
                        self.text[self.at..].chars().next().ok_or(ParseError::Syntax)?;
                    decoded.push(character);
                    self.at += character.len_utf8();
                }
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, ParseError> {
        let digits = self.bytes.get(self.at..self.at + 4).ok_or(ParseError::Syntax)?;
        let mut value = 0_u32;
        for digit in digits {
            let nibble = char::from(*digit).to_digit(16).ok_or(ParseError::Syntax)?;
            value = (value << 4) | nibble;
        }
        self.at += 4;
        Ok(value)
    }

    fn unicode_escape(&mut self) -> Result<char, ParseError> {
        let first = self.hex4()?;
        let code = match first {
            0xd800..=0xdbff => {
                // A high surrogate must be followed by an escaped low surrogate.
                if !self.bytes[self.at..].starts_with(b"\\u") {
                    return Err(ParseError::Syntax);
                }
                self.at += 2;
                let second = self.hex4()?;
                if !(0xdc00..=0xdfff).contains(&second) {
                    return Err(ParseError::Syntax);
                }
                0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00)
            }
            0xdc00..=0xdfff => return Err(ParseError::Syntax),
            other => other,
        };
        char::from_u32(code).ok_or(ParseError::Syntax)
    }

    fn array(&mut self, depth: usize) -> Result<Node, ParseError> {
        if depth > MAX_MEDIA_JSON_DEPTH {
            return Err(ParseError::TooDeep);
        }
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Node::Array(items));
        }
        loop {
            self.skip_whitespace();
            items.push(self.value(depth)?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Node::Array(items));
                }
                _ => return Err(ParseError::Syntax),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Node, ParseError> {
        if depth > MAX_MEDIA_JSON_DEPTH {
            return Err(ParseError::TooDeep);
        }
        self.expect(b'{')?;
        let mut members = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Node::Object(members));
        }
        loop {
            self.skip_whitespace();
            let key = self.string()?;
            self.skip_whitespace();
            self.expect(b':')?;
            self.skip_whitespace();
            let value = self.value(depth)?;
            members.push(Member { key, value });
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Node::Object(members));
                }
                _ => return Err(ParseError::Syntax),
            }
        }
    }
}

/// Serializes `node` compactly: no insignificant whitespace, members in source order, scalars
/// and strings copied as their source text.
pub fn write_compact(node: &Node, out: &mut String) {
    match node {
        Node::Scalar(text) => out.push_str(text),
        Node::String(string) => {
            out.push('"');
            out.push_str(&string.raw);
            out.push('"');
        }
        Node::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_compact(item, out);
            }
            out.push(']');
        }
        Node::Object(members) => {
            out.push('{');
            for (index, member) in members.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push('"');
                out.push_str(&member.key.raw);
                out.push_str("\":");
                write_compact(&member.value, out);
            }
            out.push('}');
        }
    }
}

/// Writes `value` as a JSON string with the escaping every host must reproduce: `"` and `\`
/// escaped, `\b \f \n \r \t` by name, every other control character as `\u00xx` (lower-case
/// hex), and everything else, non-ASCII included, as itself. This is `serde_json`'s escaping.
pub fn write_string(value: &str, out: &mut String) {
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if u32::from(control) < 0x20 => {
                // Writing to a `String` cannot fail.
                let _ = write!(out, "\\u{:04x}", u32::from(control));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(input: &str) -> String {
        let mut out = String::new();
        write_compact(&parse(input.as_bytes()).expect("valid"), &mut out);
        out
    }

    #[test]
    fn keeps_order_number_text_and_escapes() {
        assert_eq!(
            round_trip(r#" { "b" : 1.50e+3 , "a" : [ "x\u0041\n" , -0 , true , null ] } "#),
            r#"{"b":1.50e+3,"a":["x\u0041\n",-0,true,null]}"#
        );
    }

    #[test]
    fn decodes_escapes_and_surrogate_pairs() {
        let Node::String(string) = parse(br#""\ud83d\ude00\t\/""#).expect("valid") else {
            panic!("a string");
        };
        assert_eq!(string.decoded, "\u{1f600}\t/");
    }

    #[test]
    fn refuses_what_rfc_8259_refuses() {
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\":1,}",
            "01",
            "1.",
            "-",
            "1e",
            ".5",
            "tru",
            "\"\\x\"",
            "\"\u{1}\"",
            "\"\\ud800\"",
            "\"\\udc00\"",
            "1 2",
            "{'a':1}",
            "NaN",
            "// x\n1",
        ] {
            assert_eq!(parse(bad.as_bytes()), Err(ParseError::Syntax), "{bad:?}");
        }
        assert_eq!(parse(&[0xff]), Err(ParseError::Syntax));
    }

    #[test]
    fn depth_is_bounded() {
        let deep = "[".repeat(MAX_MEDIA_JSON_DEPTH) + &"]".repeat(MAX_MEDIA_JSON_DEPTH);
        assert!(parse(deep.as_bytes()).is_ok());
        let deeper = "[".repeat(MAX_MEDIA_JSON_DEPTH + 1) + &"]".repeat(MAX_MEDIA_JSON_DEPTH + 1);
        assert_eq!(parse(deeper.as_bytes()), Err(ParseError::TooDeep));
    }

    #[test]
    fn string_escaping_is_serde_json_escaping() {
        let value = "a\"b\\c\u{8}\u{c}\n\r\t\u{1}\u{1f}é\u{1f600}";
        let mut ours = String::new();
        write_string(value, &mut ours);
        assert_eq!(ours, serde_json::to_string(value).expect("serializable"));
    }
}
