//! A Turtle reader for LV2 bundle data, written to RDF 1.1 Turtle (W3C Recommendation,
//! 25 February 2014). It reads the whole language that LV2 bundles use: `@prefix` / `@base`
//! and their SPARQL forms, IRIs relative to the file, prefixed names (with `\` escapes and
//! `%xx` in local names), `a`, `;` and `,` lists, blank nodes by label and as `[ … ]`,
//! collections `( … )`, strings in all four quotings with escapes, language tags, datatypes,
//! integers, decimals, doubles and booleans. The result is a flat list of triples; [`Graph`]
//! indexes them by subject for the few questions the host asks.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
pub const XSD: &str = "http://www.w3.org/2001/XMLSchema#";

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Node {
    Iri(String),
    /// A blank node, numbered uniquely within one [`Graph`].
    Blank(u32),
    Literal(Literal),
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Literal {
    pub value: String,
    /// The datatype IRI (`xsd:integer` for a bare `3`); `None` for a plain string.
    pub datatype: Option<String>,
    pub lang: Option<String>,
}
impl Node {
    pub fn iri(value: impl Into<String>) -> Self {
        Node::Iri(value.into())
    }
    pub fn as_iri(&self) -> Option<&str> {
        match self {
            Node::Iri(iri) => Some(iri),
            _ => None,
        }
    }
    pub fn as_literal(&self) -> Option<&Literal> {
        match self {
            Node::Literal(l) => Some(l),
            _ => None,
        }
    }
    /// The text of a literal, or an IRI.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Node::Literal(l) => Some(&l.value),
            Node::Iri(i) => Some(i),
            Node::Blank(_) => None,
        }
    }
    /// A numeric or boolean literal as a number. Plain strings that hold a number count too:
    /// some bundles write `lv2:default "0.5"`.
    pub fn as_f64(&self) -> Option<f64> {
        let l = self.as_literal()?;
        match l.value.trim() {
            "true" => Some(1.0),
            "false" => Some(0.0),
            text => text.parse::<f64>().ok().filter(|v| v.is_finite()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Triple {
    pub subject: Node,
    pub predicate: String,
    pub object: Node,
}

/// Triples from one or more files, indexed by subject.
#[derive(Default)]
pub struct Graph {
    triples: Vec<Triple>,
    by_subject: HashMap<Node, Vec<usize>>,
    next_blank: u32,
    loaded: Vec<PathBuf>,
}
impl Graph {
    pub fn new() -> Self {
        Self::default()
    }
    /// Parse `text` as a document at `base` (a `file:` IRI for files) into the graph.
    pub fn parse(&mut self, text: &str, base: &str) -> Result<(), String> {
        let mut out = vec![];
        Parser::new(text, base, &mut self.next_blank, &mut out).document()?;
        for triple in out {
            self.by_subject
                .entry(triple.subject.clone())
                .or_default()
                .push(self.triples.len());
            self.triples.push(triple);
        }
        Ok(())
    }
    /// Read a Turtle file into the graph once; a file already read is skipped.
    pub fn load(&mut self, path: &Path) -> Result<(), String> {
        if self.loaded.iter().any(|p| p == path) {
            return Ok(());
        }
        self.loaded.push(path.to_path_buf());
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
        self.parse(&text, &path_to_url(path))
            .map_err(|e| format!("{}: {e}", path.display()))
    }
    pub fn len(&self) -> usize {
        self.triples.len()
    }
    pub fn is_empty(&self) -> bool {
        self.triples.is_empty()
    }
    pub fn triples(&self) -> &[Triple] {
        &self.triples
    }
    /// Every (predicate, object) said about `subject`.
    pub fn about<'a>(&'a self, subject: &Node) -> impl Iterator<Item = &'a Triple> + 'a {
        self.by_subject
            .get(subject)
            .into_iter()
            .flatten()
            .map(|&i| &self.triples[i])
    }
    pub fn objects<'a>(
        &'a self,
        subject: &Node,
        predicate: &str,
    ) -> impl Iterator<Item = &'a Node> + 'a {
        let predicate = predicate.to_string();
        self.about(subject)
            .filter(move |t| t.predicate == predicate)
            .map(|t| &t.object)
    }
    pub fn object(&self, subject: &Node, predicate: &str) -> Option<&Node> {
        self.objects(subject, predicate).next()
    }
    pub fn has(&self, subject: &Node, predicate: &str, object: &str) -> bool {
        self.objects(subject, predicate)
            .any(|o| o.as_iri() == Some(object))
    }
    /// Subjects with `rdf:type class`, in document order.
    pub fn instances_of(&self, class: &str) -> Vec<Node> {
        let mut out: Vec<Node> = vec![];
        let ty = format!("{RDF}type");
        for t in &self.triples {
            if t.predicate == ty && t.object.as_iri() == Some(class) && !out.contains(&t.subject)
            {
                out.push(t.subject.clone());
            }
        }
        out
    }
    /// The items of an RDF collection (`( a b c )`), stopping at a malformed or cyclic list.
    pub fn list<'a>(&'a self, head: &'a Node) -> Vec<&'a Node> {
        let (first, rest, nil) = (format!("{RDF}first"), format!("{RDF}rest"), format!("{RDF}nil"));
        let mut out = vec![];
        let mut node = head;
        while node.as_iri() != Some(nil.as_str()) && out.len() < 100_000 {
            let Some(item) = self.object(node, &first) else {
                break;
            };
            out.push(item);
            match self.object(node, &rest) {
                Some(next) => node = next,
                None => break,
            }
        }
        out
    }
}

/// A `file:` IRI for a path, percent-encoding what an IRI cannot hold.
pub fn path_to_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file://");
    if !text.starts_with('/') {
        out.push('/');
    }
    for b in text.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'.'
            | b'_'
            | b'~'
            | b'/'
            | b':'
            | b'+'
            | b'@'
            | b'!'
            | b'$'
            | b'&'
            | b'\''
            | b'('
            | b')'
            | b'*'
            | b','
            | b';'
            | b'=' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
/// The path a `file:` IRI names, or `None` for any other IRI.
pub fn url_to_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let rest = rest.split(['#', '?']).next().unwrap_or(rest);
    let bytes = rest.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16);
            if let (Some(high), Some(low)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                decoded.push((high * 16 + low) as u8);
                i += 3;
                continue;
            }
        }
        decoded.push(bytes[i]);
        i += 1;
    }
    let text = String::from_utf8_lossy(&decoded).into_owned();
    // `file:///C:/x` is `C:/x` on Windows.
    if cfg!(windows) {
        let t = text.trim_start_matches('/');
        if t.len() > 1 && t.as_bytes()[1] == b':' {
            return Some(PathBuf::from(t));
        }
    }
    Some(PathBuf::from(text))
}

/// Resolve a (possibly relative) IRI reference against a base IRI (RFC 3986 section 5.2,
/// enough of it for the references bundles contain).
pub fn resolve(base: &str, reference: &str) -> String {
    let has_scheme = reference
        .find(':')
        .is_some_and(|i| i > 0 && reference[..i].bytes().all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b)) && reference.as_bytes()[0].is_ascii_alphabetic());
    if has_scheme {
        return reference.to_string();
    }
    let base_no_fragment = base.split('#').next().unwrap_or(base);
    if reference.is_empty() {
        return base_no_fragment.to_string();
    }
    if reference.starts_with('#') {
        return format!("{base_no_fragment}{reference}");
    }
    // scheme://authority
    let (scheme_authority, path) = match base_no_fragment.find("://") {
        Some(i) => {
            let after = &base_no_fragment[i + 3..];
            let end = after.find('/').map_or(base_no_fragment.len(), |j| i + 3 + j);
            (&base_no_fragment[..end], &base_no_fragment[end..])
        }
        None => match base_no_fragment.find(':') {
            Some(i) => (&base_no_fragment[..i + 1], &base_no_fragment[i + 1..]),
            None => ("", base_no_fragment),
        },
    };
    if reference.starts_with("//") {
        let scheme = scheme_authority.split(':').next().unwrap_or("");
        return format!("{scheme}:{reference}");
    }
    let (reference_path, suffix) = match reference.find(['?', '#']) {
        Some(i) => (&reference[..i], &reference[i..]),
        None => (reference, ""),
    };
    let merged = if reference_path.starts_with('/') {
        reference_path.to_string()
    } else if reference_path.is_empty() {
        path.split('?').next().unwrap_or(path).to_string()
    } else {
        let path = path.split('?').next().unwrap_or(path);
        let dir = match path.rfind('/') {
            Some(i) => &path[..=i],
            None => "/",
        };
        format!("{dir}{reference_path}")
    };
    format!("{scheme_authority}{}{suffix}", remove_dots(&merged))
}
fn remove_dots(path: &str) -> String {
    let mut out: Vec<&str> = vec![];
    let segments: Vec<&str> = path.split('/').collect();
    for (i, segment) in segments.iter().enumerate() {
        let last = i + 1 == segments.len();
        match *segment {
            "." => {
                if last {
                    out.push("");
                }
            }
            ".." => {
                if out.len() > 1 {
                    out.pop();
                }
                if last {
                    out.push("");
                }
            }
            s => out.push(s),
        }
    }
    let joined = out.join("/");
    if path.starts_with('/') && !joined.starts_with('/') {
        format!("/{joined}")
    } else {
        joined
    }
}

struct Parser<'a> {
    s: &'a [u8],
    text: &'a str,
    pos: usize,
    base: String,
    prefixes: HashMap<String, String>,
    labels: HashMap<String, u32>,
    next_blank: &'a mut u32,
    out: &'a mut Vec<Triple>,
}

fn is_name_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b >= 0x80
}
fn is_name_char(b: u8) -> bool {
    is_name_start(b) || b.is_ascii_digit() || b == b'-'
}
/// Characters that may follow a backslash in a local name (PN_LOCAL_ESC).
const LOCAL_ESCAPES: &[u8] = b"_~.-!$&'()*+,;=/?#@%";

impl<'a> Parser<'a> {
    fn new(
        text: &'a str,
        base: &str,
        next_blank: &'a mut u32,
        out: &'a mut Vec<Triple>,
    ) -> Self {
        Self {
            s: text.as_bytes(),
            text,
            pos: 0,
            base: base.to_string(),
            prefixes: HashMap::new(),
            labels: HashMap::new(),
            next_blank,
            out,
        }
    }
    fn error(&self, what: &str) -> String {
        let line = self.s[..self.pos.min(self.s.len())]
            .iter()
            .filter(|&&b| b == b'\n')
            .count()
            + 1;
        format!("line {line}: {what}")
    }
    fn peek(&self) -> Option<u8> {
        self.s.get(self.pos).copied()
    }
    fn at(&self, offset: usize) -> Option<u8> {
        self.s.get(self.pos + offset).copied()
    }
    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if b.is_ascii_whitespace() {
                self.pos += 1;
            } else if b == b'#' {
                while let Some(b) = self.peek() {
                    self.pos += 1;
                    if b == b'\n' || b == b'\r' {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }
    fn eat(&mut self, b: u8) -> bool {
        self.skip_ws();
        if self.peek() == Some(b) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, b: u8) -> Result<(), String> {
        if self.eat(b) {
            Ok(())
        } else {
            Err(self.error(&format!(
                "expected `{}`, found {}",
                b as char,
                self.found()
            )))
        }
    }
    fn found(&self) -> String {
        match self.peek() {
            None => "the end of the file".into(),
            Some(_) => {
                let rest = &self.text[self.pos.min(self.text.len())..];
                let word: String = rest.chars().take_while(|c| !c.is_whitespace()).take(20).collect();
                format!("`{word}`")
            }
        }
    }
    /// A keyword (case-insensitive when `any_case`) followed by something that is not a name.
    fn keyword(&mut self, word: &str, any_case: bool) -> bool {
        let end = self.pos + word.len();
        let Some(slice) = self.s.get(self.pos..end) else {
            return false;
        };
        let same = if any_case {
            slice.eq_ignore_ascii_case(word.as_bytes())
        } else {
            slice == word.as_bytes()
        };
        let boundary = self
            .s
            .get(end)
            .is_none_or(|&b| !is_name_char(b) && b != b':');
        if same && boundary {
            self.pos = end;
            true
        } else {
            false
        }
    }
    fn fresh_blank(&mut self) -> Node {
        let id = *self.next_blank;
        *self.next_blank += 1;
        Node::Blank(id)
    }
    fn emit(&mut self, subject: &Node, predicate: &str, object: Node) {
        self.out.push(Triple {
            subject: subject.clone(),
            predicate: predicate.to_string(),
            object,
        });
    }

    fn document(&mut self) -> Result<(), String> {
        loop {
            self.skip_ws();
            let Some(b) = self.peek() else {
                return Ok(());
            };
            if b == b'@' {
                self.pos += 1;
                if self.keyword("prefix", false) {
                    self.prefix_directive()?;
                    self.expect(b'.')?;
                } else if self.keyword("base", false) {
                    self.base_directive()?;
                    self.expect(b'.')?;
                } else {
                    return Err(self.error(&format!("unknown directive @{}", self.found())));
                }
            } else if self.keyword("PREFIX", true) {
                self.prefix_directive()?;
            } else if self.keyword("BASE", true) {
                self.base_directive()?;
            } else {
                self.triples()?;
                self.expect(b'.')?;
            }
        }
    }
    fn prefix_directive(&mut self) -> Result<(), String> {
        self.skip_ws();
        let start = self.pos;
        while let Some(b) = self.peek() {
            if b == b':' {
                break;
            }
            if !(is_name_char(b) || b == b'.') {
                return Err(self.error("expected a prefix name ending in `:`"));
            }
            self.pos += 1;
        }
        let name = self.text[start..self.pos].to_string();
        self.expect(b':')?;
        let iri = self.iri_ref()?;
        self.prefixes.insert(name, iri);
        Ok(())
    }
    fn base_directive(&mut self) -> Result<(), String> {
        self.base = self.iri_ref()?;
        Ok(())
    }
    fn triples(&mut self) -> Result<(), String> {
        self.skip_ws();
        if self.peek() == Some(b'[') {
            let subject = self.blank_property_list()?;
            self.skip_ws();
            if self.peek() != Some(b'.') {
                self.predicate_object_list(&subject)?;
            }
            return Ok(());
        }
        let subject = match self.peek() {
            Some(b'(') => self.collection()?,
            _ => self.iri_or_blank()?,
        };
        self.predicate_object_list(&subject)
    }
    fn predicate_object_list(&mut self, subject: &Node) -> Result<(), String> {
        loop {
            let predicate = self.verb()?;
            self.object_list(subject, &predicate)?;
            if !self.eat(b';') {
                return Ok(());
            }
            // Repeated and trailing semicolons are allowed.
            loop {
                self.skip_ws();
                match self.peek() {
                    Some(b';') => self.pos += 1,
                    Some(b'.') | Some(b']') | None => return Ok(()),
                    _ => break,
                }
            }
        }
    }
    fn verb(&mut self) -> Result<String, String> {
        self.skip_ws();
        if self.peek() == Some(b'a') && self.keyword("a", false) {
            return Ok(format!("{RDF}type"));
        }
        match self.iri_or_blank()? {
            Node::Iri(iri) => Ok(iri),
            _ => Err(self.error("a predicate must be an IRI")),
        }
    }
    fn object_list(&mut self, subject: &Node, predicate: &str) -> Result<(), String> {
        loop {
            let object = self.object()?;
            self.emit(subject, predicate, object);
            if !self.eat(b',') {
                return Ok(());
            }
        }
    }
    fn object(&mut self) -> Result<Node, String> {
        self.skip_ws();
        match self.peek() {
            Some(b'[') => self.blank_property_list(),
            Some(b'(') => self.collection(),
            Some(b'"') | Some(b'\'') => self.string_literal(),
            Some(b) if b.is_ascii_digit() || b == b'+' || b == b'-' => self.number(),
            Some(b'.') if self.at(1).is_some_and(|b| b.is_ascii_digit()) => self.number(),
            Some(b't') if self.keyword("true", false) => Ok(Node::Literal(Literal {
                value: "true".into(),
                datatype: Some(format!("{XSD}boolean")),
                lang: None,
            })),
            Some(b'f') if self.keyword("false", false) => Ok(Node::Literal(Literal {
                value: "false".into(),
                datatype: Some(format!("{XSD}boolean")),
                lang: None,
            })),
            Some(_) => self.iri_or_blank(),
            None => Err(self.error("expected an object, found the end of the file")),
        }
    }
    fn blank_property_list(&mut self) -> Result<Node, String> {
        self.expect(b'[')?;
        let node = self.fresh_blank();
        self.skip_ws();
        if self.peek() != Some(b']') {
            self.predicate_object_list(&node)?;
        }
        self.expect(b']')?;
        Ok(node)
    }
    fn collection(&mut self) -> Result<Node, String> {
        self.expect(b'(')?;
        let mut items = vec![];
        loop {
            self.skip_ws();
            if self.peek() == Some(b')') {
                self.pos += 1;
                break;
            }
            if self.peek().is_none() {
                return Err(self.error("unclosed `(`"));
            }
            items.push(self.object()?);
        }
        let nil = Node::Iri(format!("{RDF}nil"));
        let mut head = nil.clone();
        let nodes: Vec<Node> = items.iter().map(|_| self.fresh_blank()).collect();
        for (i, item) in items.into_iter().enumerate() {
            let node = nodes[i].clone();
            self.emit(&node, &format!("{RDF}first"), item);
            let rest = nodes.get(i + 1).cloned().unwrap_or_else(|| nil.clone());
            self.emit(&node, &format!("{RDF}rest"), rest);
            if i == 0 {
                head = node;
            }
        }
        Ok(head)
    }
    fn iri_or_blank(&mut self) -> Result<Node, String> {
        self.skip_ws();
        match self.peek() {
            Some(b'<') => Ok(Node::Iri(self.iri_ref()?)),
            Some(b'_') if self.at(1) == Some(b':') => {
                self.pos += 2;
                let start = self.pos;
                while let Some(b) = self.peek() {
                    if is_name_char(b) || b == b'.' {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                while self.pos > start && self.s[self.pos - 1] == b'.' {
                    self.pos -= 1;
                }
                if self.pos == start {
                    return Err(self.error("empty blank node label"));
                }
                let label = self.text[start..self.pos].to_string();
                if let Some(&id) = self.labels.get(&label) {
                    return Ok(Node::Blank(id));
                }
                let node = self.fresh_blank();
                if let Node::Blank(id) = node {
                    self.labels.insert(label, id);
                }
                Ok(node)
            }
            Some(b'[') if self.at(1).is_some() => {
                // `[]` as a subject or object: an anonymous blank node.
                let save = self.pos;
                self.pos += 1;
                self.skip_ws();
                if self.peek() == Some(b']') {
                    self.pos += 1;
                    return Ok(self.fresh_blank());
                }
                self.pos = save;
                Err(self.error("a blank node with properties cannot be used here"))
            }
            Some(_) => self.prefixed_name(),
            None => Err(self.error("expected an IRI, found the end of the file")),
        }
    }
    fn iri_ref(&mut self) -> Result<String, String> {
        self.expect(b'<')?;
        let mut out = String::new();
        loop {
            let Some(b) = self.peek() else {
                return Err(self.error("unclosed IRI"));
            };
            self.pos += 1;
            match b {
                b'>' => break,
                b'\\' => out.push(self.unicode_escape()?),
                b'\n' | b'\r' | b' ' | b'"' | b'{' | b'}' | b'|' | b'^' | b'`' => {
                    return Err(self.error("invalid character in an IRI"));
                }
                _ => {
                    // Copy one UTF-8 character.
                    let start = self.pos - 1;
                    let len = utf8_len(b);
                    let end = (start + len).min(self.s.len());
                    out.push_str(&self.text[start..end]);
                    self.pos = end;
                }
            }
        }
        Ok(resolve(&self.base, &out))
    }
    fn unicode_escape(&mut self) -> Result<char, String> {
        let digits = match self.peek() {
            Some(b'u') => 4,
            Some(b'U') => 8,
            _ => return Err(self.error("invalid escape")),
        };
        self.pos += 1;
        let hex = self
            .text
            .get(self.pos..self.pos + digits)
            .ok_or_else(|| self.error("short unicode escape"))?;
        let code = u32::from_str_radix(hex, 16).map_err(|_| self.error("bad unicode escape"))?;
        self.pos += digits;
        char::from_u32(code).ok_or_else(|| self.error("escape is not a character"))
    }
    fn prefixed_name(&mut self) -> Result<Node, String> {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if is_name_char(b) || b == b'.' {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.peek() != Some(b':') {
            self.pos = start;
            return Err(self.error(&format!("expected an IRI or a prefixed name, found {}", self.found())));
        }
        let prefix = self.text[start..self.pos].to_string();
        self.pos += 1;
        let namespace = self
            .prefixes
            .get(&prefix)
            .cloned()
            .ok_or_else(|| self.error(&format!("undeclared prefix `{prefix}:`")))?;
        let mut local = String::new();
        loop {
            let Some(b) = self.peek() else { break };
            if is_name_char(b) || b == b':' || b == b'.' {
                let len = utf8_len(b);
                let end = (self.pos + len).min(self.s.len());
                local.push_str(&self.text[self.pos..end]);
                self.pos = end;
            } else if b == b'%'
                && self.at(1).is_some_and(|c| c.is_ascii_hexdigit())
                && self.at(2).is_some_and(|c| c.is_ascii_hexdigit())
            {
                local.push_str(&self.text[self.pos..self.pos + 3]);
                self.pos += 3;
            } else if b == b'\\' && self.at(1).is_some_and(|c| LOCAL_ESCAPES.contains(&c)) {
                local.push(self.s[self.pos + 1] as char);
                self.pos += 2;
            } else {
                break;
            }
        }
        // A local name never ends with a dot: that dot ends the statement.
        while local.ends_with('.') {
            local.pop();
            self.pos -= 1;
        }
        Ok(Node::Iri(format!("{namespace}{local}")))
    }
    fn number(&mut self) -> Result<Node, String> {
        let start = self.pos;
        if matches!(self.peek(), Some(b'+') | Some(b'-')) {
            self.pos += 1;
        }
        let digits = |p: &mut Self| {
            let from = p.pos;
            while p.peek().is_some_and(|b| b.is_ascii_digit()) {
                p.pos += 1;
            }
            p.pos - from
        };
        let whole = digits(self);
        let mut kind = "integer";
        if self.peek() == Some(b'.') && self.at(1).is_some_and(|b| b.is_ascii_digit()) {
            self.pos += 1;
            digits(self);
            kind = "decimal";
        } else if whole == 0 {
            return Err(self.error("expected a number"));
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            let save = self.pos;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.pos += 1;
            }
            if digits(self) == 0 {
                self.pos = save;
            } else {
                kind = "double";
            }
        }
        Ok(Node::Literal(Literal {
            value: self.text[start..self.pos].trim_start_matches('+').to_string(),
            datatype: Some(format!("{XSD}{kind}")),
            lang: None,
        }))
    }
    fn string_literal(&mut self) -> Result<Node, String> {
        let quote = self.peek().unwrap_or(b'"');
        let long = self.at(1) == Some(quote) && self.at(2) == Some(quote);
        self.pos += if long { 3 } else { 1 };
        let mut value = String::new();
        loop {
            let Some(b) = self.peek() else {
                return Err(self.error("unclosed string"));
            };
            if b == quote {
                if !long {
                    self.pos += 1;
                    break;
                }
                if self.at(1) == Some(quote) && self.at(2) == Some(quote) {
                    // Up to two quotes may end the content of a long string.
                    let mut end = self.pos + 3;
                    while self.s.get(end) == Some(&quote) && end - self.pos < 5 {
                        end += 1;
                    }
                    for _ in 0..(end - self.pos - 3) {
                        value.push(quote as char);
                    }
                    self.pos = end;
                    break;
                }
                value.push(quote as char);
                self.pos += 1;
                continue;
            }
            if !long && (b == b'\n' || b == b'\r') {
                return Err(self.error("line break in a short string"));
            }
            if b == b'\\' {
                self.pos += 1;
                let escaped = match self.peek() {
                    Some(b't') => '\t',
                    Some(b'b') => '\u{8}',
                    Some(b'n') => '\n',
                    Some(b'r') => '\r',
                    Some(b'f') => '\u{c}',
                    Some(b'"') => '"',
                    Some(b'\'') => '\'',
                    Some(b'\\') => '\\',
                    Some(b'u') | Some(b'U') => {
                        value.push(self.unicode_escape()?);
                        continue;
                    }
                    _ => return Err(self.error("invalid escape in a string")),
                };
                self.pos += 1;
                value.push(escaped);
                continue;
            }
            let len = utf8_len(b);
            let end = (self.pos + len).min(self.s.len());
            value.push_str(&self.text[self.pos..end]);
            self.pos = end;
        }
        let mut literal = Literal {
            value,
            datatype: None,
            lang: None,
        };
        if self.peek() == Some(b'@') {
            self.pos += 1;
            let start = self.pos;
            while self
                .peek()
                .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'-')
            {
                self.pos += 1;
            }
            literal.lang = Some(self.text[start..self.pos].to_lowercase());
        } else if self.peek() == Some(b'^') && self.at(1) == Some(b'^') {
            self.pos += 2;
            match self.iri_or_blank()? {
                Node::Iri(iri) => literal.datatype = Some(iri),
                _ => return Err(self.error("a datatype must be an IRI")),
            }
        }
        Ok(Node::Literal(literal))
    }
}
fn utf8_len(first: u8) -> usize {
    match first {
        0xF0..=0xFF => 4,
        0xE0..=0xEF => 3,
        0xC0..=0xDF => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(text: &str) -> Graph {
        let mut g = Graph::new();
        g.parse(text, "file:///lv2/x.lv2/manifest.ttl").unwrap();
        g
    }

    #[test]
    fn reads_what_lv2_bundles_write() {
        let g = graph(
            r#"
            @prefix : <http://lv2plug.in/ns/lv2core#> .
            @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
            PREFIX doap: <http://usefulinc.com/ns/doap#>
            # a comment with "quotes" and <brackets>
            <http://example.org/amp> a :Plugin, :AmplifierPlugin ;
                :binary <amp.so> ;
                rdfs:seeAlso <amp.ttl>, <../other.lv2/x.ttl> ;
                doap:name "Simple amp"@en, "Ampli"@fr ;
                :port [
                    a :InputPort , :ControlPort ;
                    :index 0 ; :symbol "gain" ;
                    :minimum -70 ; :maximum +70 ; :default 0.0 ;
                    :scalePoint [ rdfs:label "Unity" ; rdf:value 0 ] ;
                ] , [
                    a :OutputPort ; :index 1 ; :default 1.5e3 ; :toggled true
                ] ;
                :extra ( 1 "two" <three> ) ;
                :doc """Long "quoted"
text""" .
            "#
            .replace("rdf:value", "<http://www.w3.org/1999/02/22-rdf-syntax-ns#value>")
            .as_str(),
        );
        let amp = Node::iri("http://example.org/amp");
        let lv2 = "http://lv2plug.in/ns/lv2core#";
        assert!(g.has(&amp, &format!("{RDF}type"), &format!("{lv2}AmplifierPlugin")));
        assert_eq!(
            g.object(&amp, &format!("{lv2}binary")).unwrap().as_iri(),
            Some("file:///lv2/x.lv2/amp.so")
        );
        let see: Vec<_> = g
            .objects(&amp, "http://www.w3.org/2000/01/rdf-schema#seeAlso")
            .filter_map(Node::as_iri)
            .collect();
        assert_eq!(see, ["file:///lv2/x.lv2/amp.ttl", "file:///lv2/other.lv2/x.ttl"]);
        let names: Vec<_> = g
            .objects(&amp, "http://usefulinc.com/ns/doap#name")
            .map(|n| n.as_literal().unwrap().lang.clone())
            .collect();
        assert_eq!(names, [Some("en".to_string()), Some("fr".to_string())]);
        let ports: Vec<_> = g.objects(&amp, &format!("{lv2}port")).collect();
        assert_eq!(ports.len(), 2);
        let min = g.object(ports[0], &format!("{lv2}minimum")).unwrap();
        assert_eq!(min.as_f64(), Some(-70.0));
        assert_eq!(
            g.object(ports[0], &format!("{lv2}maximum")).unwrap().as_f64(),
            Some(70.0)
        );
        assert_eq!(
            g.object(ports[1], &format!("{lv2}default")).unwrap().as_literal().unwrap().datatype.as_deref(),
            Some("http://www.w3.org/2001/XMLSchema#double")
        );
        assert_eq!(g.object(ports[1], &format!("{lv2}toggled")).unwrap().as_f64(), Some(1.0));
        let point = g.object(ports[0], &format!("{lv2}scalePoint")).unwrap();
        assert_eq!(g.object(point, &format!("{RDF}value")).unwrap().as_f64(), Some(0.0));
        let list = g.object(&amp, &format!("{lv2}extra")).unwrap();
        let items: Vec<_> = g.list(list).into_iter().filter_map(Node::as_str).collect();
        assert_eq!(items, ["1", "two", "file:///lv2/x.lv2/three"]);
        assert_eq!(
            g.object(&amp, &format!("{lv2}doc")).unwrap().as_str(),
            Some("Long \"quoted\"\ntext")
        );
    }

    #[test]
    fn statement_dots_names_blank_labels_and_escapes() {
        let g = graph(
            r#"@prefix ex: <http://e.org/ns#> .
            @base <http://e.org/base/> .
            ex:a.b ex:p ex:c. ex:d ex:p 0. ex:e ex:p 1.5 .
            _:n ex:p _:n . _:n ex:q 'single' , '''long 'one''''.
            <rel> ex:p "tab\tand \u00e9" ; ex:t "1"^^ex:int , "x"^^<http://e.org/t> ;; .
            [] ex:p ex:local\,name , ex:pct%41 .
            [ ex:inner 1 ] ex:p true .
            ex:x ex:p () .
            "#,
        );
        let ab = Node::iri("http://e.org/ns#a.b");
        assert_eq!(g.object(&ab, "http://e.org/ns#p").unwrap().as_iri(), Some("http://e.org/ns#c"));
        let d = Node::iri("http://e.org/ns#d");
        assert_eq!(g.object(&d, "http://e.org/ns#p").unwrap().as_f64(), Some(0.0));
        let blank_subjects: Vec<_> = g
            .triples()
            .iter()
            .filter(|t| t.predicate == "http://e.org/ns#q")
            .map(|t| t.object.as_str().unwrap().to_string())
            .collect();
        assert_eq!(blank_subjects, ["single", "long 'one'"]);
        let rel = Node::iri("http://e.org/base/rel");
        assert_eq!(g.object(&rel, "http://e.org/ns#p").unwrap().as_str(), Some("tab\tand é"));
        let types: Vec<_> = g
            .objects(&rel, "http://e.org/ns#t")
            .map(|n| n.as_literal().unwrap().datatype.clone().unwrap())
            .collect();
        assert_eq!(types, ["http://e.org/ns#int", "http://e.org/t"]);
        assert!(g
            .triples()
            .iter()
            .any(|t| t.object.as_iri() == Some("http://e.org/ns#local,name")));
        assert!(g
            .triples()
            .iter()
            .any(|t| t.object.as_iri() == Some("http://e.org/ns#pct%41")));
        let x = Node::iri("http://e.org/ns#x");
        assert_eq!(
            g.object(&x, "http://e.org/ns#p").unwrap().as_iri(),
            Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#nil")
        );
    }

    #[test]
    fn errors_say_where() {
        let mut g = Graph::new();
        let e = g.parse("@prefix a: <x:> .\n\nb:c a:d a:e .", "file:///x").unwrap_err();
        assert!(e.contains("line 3") && e.contains("b:"), "{e}");
        let e = g.parse("<a> <b> \"open", "file:///x").unwrap_err();
        assert!(e.contains("unclosed string"), "{e}");
    }

    #[test]
    fn file_iris_and_resolution() {
        assert_eq!(resolve("file:///a/b/c.ttl", "d.so"), "file:///a/b/d.so");
        assert_eq!(resolve("file:///a/b/c.ttl", "../e/f.ttl"), "file:///a/e/f.ttl");
        assert_eq!(resolve("file:///a/b/c.ttl", ""), "file:///a/b/c.ttl");
        assert_eq!(resolve("file:///a/b/c.ttl", "#x"), "file:///a/b/c.ttl#x");
        assert_eq!(resolve("http://h.org/p/q", "/r"), "http://h.org/r");
        assert_eq!(resolve("http://h.org/p/q", "urn:x:y"), "urn:x:y");
        let path = Path::new("/tmp/my plugins/a.lv2/manifest.ttl");
        let url = path_to_url(path);
        assert_eq!(url, "file:///tmp/my%20plugins/a.lv2/manifest.ttl");
        assert_eq!(url_to_path(&url).unwrap(), path);
        assert_eq!(url_to_path("http://x"), None);
    }
}
