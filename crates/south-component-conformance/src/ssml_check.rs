//! A strict, minimal XML well-formedness reader for the `ssml_escaping` row (speech record §5,
//! §11).
//!
//! It reads what an SSML request body is made of and nothing more: an optional XML declaration,
//! one root element, nested elements with single- or double-quoted attributes, character data,
//! comments, the five predefined entities and numeric character references. Anything else — a
//! DOCTYPE, a CDATA section, a processing instruction after the declaration, an undefined entity,
//! a raw `<` or `&` in text or an attribute, a repeated attribute, a mismatched end tag, content
//! after the root — is not well-formed, which is the point: an SSML body a component failed to
//! escape fails here instead of at the upstream. Hand-written because the conformance crate is
//! linked into every component, and an XML dependency would enter every lockfile (§16 Q47).
//!
//! It holds every character to XML 1.0's `Char` production (speech world batch V4 §4.4, SV-Q11):
//! a character the production forbids — U+0000 to U+0008, U+000B, U+000C, U+000E to U+001F,
//! U+FFFE, U+FFFF; surrogates cannot occur in a Rust string — is refused anywhere in the document,
//! raw or as a numeric character reference, so a component that embeds one instead of dropping it
//! fails.

/// What a well-formed document carries that the row compares.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct XmlDocumentV1 {
    /// Each element's own character data (its text children joined, references decoded), in
    /// document order of the elements' start tags.
    pub texts: Vec<String>,
    /// Every attribute value, references decoded, in document order.
    pub attributes: Vec<String>,
}

/// Whether XML 1.0's `Char` production admits `character`: tab, line feed, carriage return and
/// everything from U+0020 except U+FFFE and U+FFFF (a Rust `char` is never a surrogate).
#[must_use]
pub const fn is_xml_char(character: char) -> bool {
    matches!(character, '\t' | '\n' | '\r')
        || (character >= ' ' && character != '\u{FFFE}' && character != '\u{FFFF}')
}

/// `text` without the characters [`is_xml_char`] refuses: what a well-formed SSML document can
/// carry of a client string.
#[must_use]
pub fn without_forbidden(text: &str) -> String {
    text.chars().filter(|character| is_xml_char(*character)).collect()
}

struct Reader<'a> {
    text: &'a str,
    at: usize,
}

const fn is_name_start(character: char) -> bool {
    character.is_ascii_alphabetic() || matches!(character, '_' | ':') || !character.is_ascii()
}

const fn is_name_char(character: char) -> bool {
    is_name_start(character) || character.is_ascii_digit() || matches!(character, '-' | '.')
}

impl Reader<'_> {
    fn rest(&self) -> &str {
        &self.text[self.at..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn eat(&mut self, prefix: &str) -> bool {
        if self.rest().starts_with(prefix) {
            self.at += prefix.len();
            true
        } else {
            false
        }
    }

    fn skip_space(&mut self) -> bool {
        let before = self.at;
        while let Some(character) = self.peek().filter(|c| matches!(*c, ' ' | '\t' | '\r' | '\n')) {
            self.at += character.len_utf8();
        }
        self.at > before
    }

    fn name(&mut self) -> Result<String, String> {
        let start = self.at;
        match self.peek() {
            Some(character) if is_name_start(character) => self.at += character.len_utf8(),
            _ => return Err(format!("a name is expected at byte {start}")),
        }
        while let Some(character) = self.peek().filter(|c| is_name_char(*c)) {
            self.at += character.len_utf8();
        }
        Ok(self.text[start..self.at].to_owned())
    }

    /// A reference after its `&`: a predefined entity or a numeric character reference.
    fn reference(&mut self) -> Result<char, String> {
        let end = self.rest().find(';').ok_or("an unterminated reference")?;
        let body = &self.rest()[..end];
        let character = match body {
            "lt" => '<',
            "gt" => '>',
            "amp" => '&',
            "apos" => '\'',
            "quot" => '"',
            _ => {
                let code = body.strip_prefix("#x").map_or_else(
                    || body.strip_prefix('#').and_then(|decimal| decimal.parse::<u32>().ok()),
                    |hex| u32::from_str_radix(hex, 16).ok(),
                );
                let character = code.and_then(char::from_u32).ok_or("an undefined reference")?;
                if !is_xml_char(character) {
                    return Err("a reference to a character XML 1.0 forbids".into());
                }
                character
            }
        };
        self.at += end + 1;
        Ok(character)
    }

    fn comment(&mut self) -> Result<(), String> {
        let end = self.rest().find("-->").ok_or("an unterminated comment")?;
        if self.rest()[..end].contains("--") {
            return Err("`--` inside a comment".into());
        }
        self.at += end + 3;
        Ok(())
    }

    fn misc(&mut self) -> Result<(), String> {
        loop {
            self.skip_space();
            if self.eat("<!--") {
                self.comment()?;
            } else {
                return Ok(());
            }
        }
    }

    fn attribute_value(&mut self) -> Result<String, String> {
        let Some(quote @ ('"' | '\'')) = self.peek() else {
            return Err("an attribute value must be quoted".into());
        };
        self.at += 1;
        let mut value = String::new();
        loop {
            match self.peek() {
                None => return Err("an unterminated attribute value".into()),
                Some(character) if character == quote => {
                    self.at += 1;
                    return Ok(value);
                }
                Some('<') => return Err("a raw `<` in an attribute value".into()),
                Some('&') => {
                    self.at += 1;
                    value.push(self.reference()?);
                }
                Some(character) => {
                    value.push(character);
                    self.at += character.len_utf8();
                }
            }
        }
    }

    fn element(&mut self, document: &mut XmlDocumentV1) -> Result<(), String> {
        if !self.eat("<") {
            return Err("an element is expected".into());
        }
        let name = self.name()?;
        let mut seen = Vec::new();
        loop {
            let spaced = self.skip_space();
            if self.eat("/>") {
                document.texts.push(String::new());
                return Ok(());
            }
            if self.eat(">") {
                break;
            }
            if !spaced {
                return Err("attributes must be separated by white space".into());
            }
            let attribute = self.name()?;
            if seen.contains(&attribute) {
                return Err("a repeated attribute".into());
            }
            seen.push(attribute);
            self.skip_space();
            if !self.eat("=") {
                return Err("an attribute without `=`".into());
            }
            self.skip_space();
            let value = self.attribute_value()?;
            document.attributes.push(value);
        }
        let slot = document.texts.len();
        document.texts.push(String::new());
        let mut own = String::new();
        loop {
            if self.eat("</") {
                if self.name()? != name {
                    return Err("a mismatched end tag".into());
                }
                self.skip_space();
                if !self.eat(">") {
                    return Err("an unterminated end tag".into());
                }
                document.texts[slot] = own;
                return Ok(());
            }
            if self.eat("<!--") {
                self.comment()?;
                continue;
            }
            match self.peek() {
                None => return Err("an unclosed element".into()),
                Some('<') => {
                    if self.rest().starts_with("<!") || self.rest().starts_with("<?") {
                        return Err("a declaration or instruction inside an element".into());
                    }
                    self.element(document)?;
                }
                Some('&') => {
                    self.at += 1;
                    own.push(self.reference()?);
                }
                Some(character) => {
                    if self.rest().starts_with("]]>") {
                        return Err("`]]>` in character data".into());
                    }
                    own.push(character);
                    self.at += character.len_utf8();
                }
            }
        }
    }
}

/// Reads `text` as one well-formed XML document.
///
/// # Errors
/// Returns why the document is not well-formed (or uses a construct this reader does not admit).
pub fn parse_xml(text: &str) -> Result<XmlDocumentV1, String> {
    if let Some(at) = text.find(|character: char| !is_xml_char(character)) {
        return Err(format!("a character XML 1.0 forbids at byte {at}"));
    }
    let mut reader = Reader { text, at: 0 };
    if reader.eat("<?xml") {
        let end = reader.rest().find("?>").ok_or("an unterminated XML declaration")?;
        reader.at += end + 2;
    }
    reader.misc()?;
    let mut document = XmlDocumentV1::default();
    reader.element(&mut document)?;
    reader.misc()?;
    if reader.at == text.len() {
        Ok(document)
    } else {
        Err("content after the root element".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaped_text_and_attributes_decode() {
        let document = parse_xml(
            "<?xml version='1.0'?><speak version='1.0' xml:lang=\"en-US\"><voice \
             name='a&lt;b&amp;c&apos;d&quot;'>x &lt; y &amp; z&#39;s &#x22;q&quot;</voice></speak>",
        )
        .expect("well-formed");
        assert_eq!(document.attributes, ["1.0", "en-US", "a<b&c'd\""]);
        assert_eq!(document.texts, ["", "x < y & z's \"q\""]);
    }

    #[test]
    fn unescaped_or_malformed_documents_are_refused() {
        for bad in [
            "<speak>a < b</speak>",
            "<speak>a & b</speak>",
            "<speak><voice name='a'b'>x</voice></speak>",
            "<speak><voice name='a<b'>x</voice></speak>",
            "<speak a='1' a='2'/>",
            "<speak><voice>x</speak></voice>",
            "<speak>x</speak><speak/>",
            "<speak>&nbsp;</speak>",
            "<!DOCTYPE speak><speak/>",
            "<speak><![CDATA[x]]></speak>",
            "<speak",
            "",
        ] {
            assert!(parse_xml(bad).is_err(), "{bad}");
        }
    }

    /// SV-Q11: a character the `Char` production forbids is refused raw (in text, an attribute or
    /// a comment) and as a decimal or hexadecimal reference; tab, line feed and carriage return
    /// are admitted both ways.
    #[test]
    fn characters_xml_forbids_are_refused_raw_and_as_references() {
        for bad in [
            "<speak>a\u{7}b</speak>",
            "<speak>\u{0}</speak>",
            "<speak a='\u{1f}'/>",
            "<speak>\u{fffe}</speak>",
            "<speak>\u{ffff}</speak>",
            "<speak><!-- \u{b} --></speak>",
            "<speak>&#7;</speak>",
            "<speak>&#x0;</speak>",
            "<speak a='&#x1F;'/>",
            "<speak>&#xC;</speak>",
            "<speak>&#65534;</speak>",
            "<speak>&#xFFFF;</speak>",
        ] {
            assert!(parse_xml(bad).is_err(), "{bad:?}");
        }
        let document =
            parse_xml("<speak a='&#9;&#xA;&#13;'>x\ty\nz\r&#x9;\u{fffd}\u{10000}</speak>")
                .expect("every character here is a Char");
        assert_eq!(document.attributes, ["\t\n\r"]);
        assert_eq!(document.texts, ["x\ty\nz\r\t\u{fffd}\u{10000}"]);
        assert_eq!(without_forbidden("a\u{7}b\u{0}\tc\u{fffe}\u{ffff}d\u{c}"), "ab\tcd");
    }
}
