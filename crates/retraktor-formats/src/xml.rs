//! Minimal lossless XML DOM used for Traktor `.nml` and `.tsi` documents.
//!
//! Traktor writes every element as `<TAG attr="v"></TAG>` (never self-closing), with a newline
//! after each closing tag. Attribute values may contain raw newlines. This module preserves
//! element order, attribute order and non-whitespace text so documents can be edited and
//! re-serialized without dropping fields ReTraktor does not model yet.

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct XmlNode {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<XmlNode>,
    /// Concatenated non-whitespace character data directly inside this element.
    pub text: String,
}

impl XmlNode {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn set_attr(&mut self, key: &str, value: impl Into<String>) {
        let value = value.into();
        if let Some(slot) = self.attrs.iter_mut().find(|(k, _)| k == key) {
            slot.1 = value;
        } else {
            self.attrs.push((key.to_string(), value));
        }
    }

    pub fn child(&self, name: &str) -> Option<&XmlNode> {
        self.children.iter().find(|c| c.name == name)
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a XmlNode> {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// Counts this node and all descendants.
    pub fn count_nodes(&self) -> usize {
        1 + self.children.iter().map(|c| c.count_nodes()).sum::<usize>()
    }
}

/// Parsed document: the XML declaration (verbatim) plus a single root element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlDocument {
    pub declaration: Option<String>,
    pub root: XmlNode,
}

impl XmlDocument {
    pub fn parse(input: &str) -> Result<Self, String> {
        let bytes = input.as_bytes();
        let mut i = 0usize;
        let mut declaration = None;
        let mut stack: Vec<XmlNode> = Vec::new();
        let mut root: Option<XmlNode> = None;

        while i < bytes.len() {
            if bytes[i] != b'<' {
                // character data
                let start = i;
                while i < bytes.len() && bytes[i] != b'<' {
                    i += 1;
                }
                let raw = &input[start..i];
                if !raw.trim().is_empty() {
                    if let Some(top) = stack.last_mut() {
                        top.text.push_str(&unescape(raw.trim()));
                    }
                }
                continue;
            }
            if input[i..].starts_with("<?") {
                let end = input[i..].find("?>").ok_or("Unterminated XML declaration")? + i + 2;
                declaration = Some(input[i..end].to_string());
                i = end;
                continue;
            }
            if input[i..].starts_with("<!--") {
                let end = input[i..].find("-->").ok_or("Unterminated comment")? + i + 3;
                i = end;
                continue;
            }
            // find end of tag, honoring quotes
            let tag_start = i;
            let mut in_q = false;
            i += 1;
            while i < bytes.len() {
                match bytes[i] {
                    b'"' => in_q = !in_q,
                    b'>' if !in_q => break,
                    _ => {}
                }
                i += 1;
            }
            if i >= bytes.len() {
                return Err(format!("Unterminated tag at byte {tag_start}"));
            }
            let inner = &input[tag_start + 1..i];
            i += 1;

            if let Some(name) = inner.strip_prefix('/') {
                let name = name.trim();
                let node = stack
                    .pop()
                    .ok_or_else(|| format!("Unexpected closing tag </{name}>"))?;
                if node.name != name {
                    return Err(format!("Mismatched </{name}>, expected </{}>", node.name));
                }
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => root = Some(node),
                }
                continue;
            }

            let self_closing = inner.ends_with('/');
            let inner = inner.trim_end_matches('/');
            let node = parse_start_tag(inner)?;
            if self_closing {
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => root = Some(node),
                }
            } else {
                stack.push(node);
            }
        }

        if let Some(open) = stack.last() {
            return Err(format!("Unclosed element <{}>", open.name));
        }
        Ok(Self {
            declaration,
            root: root.ok_or("Document has no root element")?,
        })
    }

    /// Serializes in Traktor's style: explicit close tags, `\n` after each close tag.
    pub fn to_string_traktor(&self) -> String {
        let mut out = String::new();
        if let Some(d) = &self.declaration {
            out.push_str(d);
            out.push('\n');
        }
        write_node(&mut out, &self.root);
        out
    }
}

fn parse_start_tag(inner: &str) -> Result<XmlNode, String> {
    let name_end = inner
        .find(|c: char| c.is_whitespace())
        .unwrap_or(inner.len());
    let mut node = XmlNode::new(&inner[..name_end]);
    let mut rest = &inner[name_end..];
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let eq = rest.find('=').ok_or_else(|| format!("Malformed attribute in <{inner}>"))?;
        let key = rest[..eq].trim().to_string();
        let after = rest[eq + 1..].trim_start();
        let after = after
            .strip_prefix('"')
            .ok_or_else(|| format!("Unquoted attribute {key}"))?;
        let close = after.find('"').ok_or_else(|| format!("Unterminated attribute {key}"))?;
        node.attrs.push((key, unescape(&after[..close])));
        rest = &after[close + 1..];
    }
    Ok(node)
}

fn write_node(out: &mut String, node: &XmlNode) {
    out.push('<');
    out.push_str(&node.name);
    for (k, v) in &node.attrs {
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        out.push_str(&escape(v));
        out.push('"');
    }
    out.push('>');
    if !node.text.is_empty() {
        out.push_str(&escape(&node.text));
    }
    for c in &node.children {
        write_node(out, c);
    }
    out.push_str("</");
    out.push_str(&node.name);
    out.push_str(">\n");
}

pub fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            c => o.push(c),
        }
    }
    o
}

pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut o = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('&') {
        o.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        let semi = tail.find(';');
        let ent = semi.map(|e| &tail[1..e]);
        let decoded = match ent {
            Some("amp") => Some('&'),
            Some("lt") => Some('<'),
            Some("gt") => Some('>'),
            Some("quot") => Some('"'),
            Some("apos") => Some('\''),
            Some(e) if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
            Some(e) if e.starts_with('#') => e[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        match (decoded, semi) {
            (Some(c), Some(e)) => {
                o.push(c);
                rest = &tail[e + 1..];
            }
            _ => {
                o.push('&');
                rest = &tail[1..];
            }
        }
    }
    o.push_str(rest);
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_traktor_style() {
        let src = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n<NML VERSION=\"20\"><HEAD COMPANY=\"x\" PROGRAM=\"Traktor Pro 4\"></HEAD>\n<COLLECTION ENTRIES=\"1\"><ENTRY TITLE=\"R&amp;B\" LYR=\"a\nb\"><LOCATION DIR=\"/:a/:\" FILE=\"f.flac\"></LOCATION>\n</ENTRY>\n</COLLECTION>\n</NML>\n";
        let doc = XmlDocument::parse(src).unwrap();
        assert_eq!(doc.root.child("COLLECTION").unwrap().children.len(), 1);
        assert_eq!(
            doc.root.child("COLLECTION").unwrap().children[0].attr("TITLE"),
            Some("R&B")
        );
        assert_eq!(doc.to_string_traktor(), src);
    }
}
