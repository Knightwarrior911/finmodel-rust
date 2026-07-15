//! Minimal namespace-resolving XML DOM built on `quick-xml`.
//!
//! The Python `pptx_*.py` toolchain navigates OOXML with lxml's
//! `find`/`findall`/`.//` and namespace-qualified tags. This module provides
//! just enough of that: parse bytes into an [`Element`] tree with namespace
//! URIs resolved, then query by `(namespace-uri, local-name)` exactly as the
//! reference code does with `_q(ns, tag)`.

use quick_xml::events::Event;
use quick_xml::name::ResolveResult;
use quick_xml::NsReader;

/// DrawingML main namespace (`a:`).
pub const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
/// PresentationML namespace (`p:`).
pub const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
/// Officedocument relationships namespace (`r:`).
pub const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
/// Package content-types namespace.
pub const CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
/// Package relationships namespace.
pub const PR: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

/// A resolved XML attribute.
#[derive(Debug, Clone)]
pub struct Attr {
    /// Namespace URI, or `None` for an unprefixed (no-namespace) attribute.
    pub ns: Option<String>,
    pub local: String,
    pub value: String,
}

/// A DOM element with namespace-resolved name, attributes and children.
#[derive(Debug, Clone)]
pub struct Element {
    /// Namespace URI of the element name.
    pub ns: Option<String>,
    pub local: String,
    pub attrs: Vec<Attr>,
    pub children: Vec<Element>,
    /// Concatenated direct text content of the element.
    pub text: String,
}

impl Element {
    /// Parse XML bytes into a DOM tree rooted at the document element.
    pub fn parse(bytes: &[u8]) -> Result<Element, String> {
        let mut reader = NsReader::from_reader(bytes);
        let mut stack: Vec<Element> = Vec::new();
        let mut root: Option<Element> = None;
        let mut buf = Vec::new();
        loop {
            let ev = reader
                .read_event_into(&mut buf)
                .map_err(|e| format!("xml parse error: {e}"))?;
            match ev {
                Event::Eof => break,
                Event::Start(e) => {
                    let el = elem_from_start(&reader, &e)?;
                    stack.push(el);
                }
                Event::Empty(e) => {
                    let el = elem_from_start(&reader, &e)?;
                    push_child(&mut stack, &mut root, el);
                }
                Event::End(_) => {
                    if let Some(el) = stack.pop() {
                        push_child(&mut stack, &mut root, el);
                    }
                }
                Event::Text(t) => {
                    let txt = t.unescape().map_err(|e| format!("text unescape: {e}"))?.into_owned();
                    if let Some(top) = stack.last_mut() {
                        top.text.push_str(&txt);
                    }
                }
                Event::CData(t) => {
                    let txt = String::from_utf8_lossy(&t).into_owned();
                    if let Some(top) = stack.last_mut() {
                        top.text.push_str(&txt);
                    }
                }
                _ => {}
            }
            buf.clear();
        }
        root.ok_or_else(|| "empty document".to_string())
    }

    /// First direct child matching `(ns, local)` (lxml `find("{ns}local")`).
    pub fn child(&self, ns: &str, local: &str) -> Option<&Element> {
        self.children
            .iter()
            .find(|c| c.local == local && c.ns.as_deref() == Some(ns))
    }

    /// All direct children matching `(ns, local)` (lxml `findall`).
    pub fn children_named<'a>(
        &'a self,
        ns: &'a str,
        local: &'a str,
    ) -> impl Iterator<Item = &'a Element> {
        self.children
            .iter()
            .filter(move |c| c.local == local && c.ns.as_deref() == Some(ns))
    }

    /// First descendant (self excluded) in document order matching `(ns, local)`
    /// (lxml `.//{ns}local`).
    pub fn descendant(&self, ns: &str, local: &str) -> Option<&Element> {
        for c in &self.children {
            if c.local == local && c.ns.as_deref() == Some(ns) {
                return Some(c);
            }
            if let Some(found) = c.descendant(ns, local) {
                return Some(found);
            }
        }
        None
    }

    /// All descendants (self excluded) matching `(ns, local)`, document order.
    pub fn descendants<'a>(&'a self, ns: &'a str, local: &'a str, out: &mut Vec<&'a Element>) {
        for c in &self.children {
            if c.local == local && c.ns.as_deref() == Some(ns) {
                out.push(c);
            }
            c.descendants(ns, local, out);
        }
    }

    /// Every descendant (self excluded) in document order, any name.
    pub fn iter_all<'a>(&'a self, out: &mut Vec<&'a Element>) {
        for c in &self.children {
            out.push(c);
            c.iter_all(out);
        }
    }

    /// Value of an unprefixed (no-namespace) attribute.
    pub fn attr(&self, local: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.ns.is_none() && a.local == local)
            .map(|a| a.value.as_str())
    }

    /// Value of a namespaced attribute.
    pub fn attr_ns(&self, ns: &str, local: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.ns.as_deref() == Some(ns) && a.local == local)
            .map(|a| a.value.as_str())
    }

    /// Deterministic, namespace-qualified canonical string for structural
    /// equality. Element names as `{ns}local`, attributes sorted by
    /// `(ns, local)`, text trimmed. Ignores prefix/attribute-order/whitespace
    /// differences between lxml and this writer so two structurally-identical
    /// documents compare equal.
    pub fn canonical(&self) -> String {
        let mut s = String::new();
        self.write_canonical(&mut s);
        s
    }

    fn write_canonical(&self, out: &mut String) {
        out.push('<');
        out.push_str(&qname(self.ns.as_deref(), &self.local));
        let mut attrs: Vec<&Attr> = self.attrs.iter().collect();
        attrs.sort_by(|a, b| (a.ns.as_deref(), a.local.as_str()).cmp(&(b.ns.as_deref(), b.local.as_str())));
        for a in attrs {
            out.push(' ');
            out.push_str(&qname(a.ns.as_deref(), &a.local));
            out.push_str("=\"");
            out.push_str(&a.value);
            out.push('"');
        }
        let text = self.text.trim();
        if self.children.is_empty() && text.is_empty() {
            out.push_str("/>");
            return;
        }
        out.push('>');
        if !text.is_empty() {
            out.push_str(text);
        }
        for c in &self.children {
            c.write_canonical(out);
        }
        out.push_str("</");
        out.push_str(&qname(self.ns.as_deref(), &self.local));
        out.push('>');
    }

    /// Serialize to valid namespaced XML bytes with an XML declaration.
    ///
    /// The root element's namespace becomes the default namespace; every other
    /// namespace in the subtree gets a conventional prefix (`a`/`p`/`r`/`ct`/
    /// `pr`) or a generated one. Prefixes differ from lxml's but the documents
    /// are structurally identical (see [`Element::canonical`]).
    pub fn to_xml_bytes(&self) -> Vec<u8> {
        let mut prefixes: Vec<(String, String)> = Vec::new(); // (ns_uri, prefix)
        let root_ns = self.ns.clone();
        self.collect_ns(&root_ns, &mut prefixes);
        let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
        self.write_xml(&root_ns, &prefixes, true, &mut s);
        s.into_bytes()
    }

    fn collect_ns(&self, root_ns: &Option<String>, out: &mut Vec<(String, String)>) {
        let consider = |ns: &Option<String>, out: &mut Vec<(String, String)>| {
            if let Some(uri) = ns {
                if root_ns.as_deref() != Some(uri.as_str())
                    && !out.iter().any(|(u, _)| u == uri)
                {
                    let pfx = conventional_prefix(uri).unwrap_or_else(|| format!("ns{}", out.len()));
                    out.push((uri.clone(), pfx));
                }
            }
        };
        consider(&self.ns, out);
        for a in &self.attrs {
            consider(&a.ns, out);
        }
        for c in &self.children {
            c.collect_ns(root_ns, out);
        }
    }

    fn write_xml(
        &self,
        root_ns: &Option<String>,
        prefixes: &[(String, String)],
        is_root: bool,
        out: &mut String,
    ) {
        let tag = prefixed(&self.ns, root_ns, prefixes, &self.local);
        out.push('<');
        out.push_str(&tag);
        if is_root {
            if let Some(r) = root_ns {
                out.push_str(&format!(" xmlns=\"{r}\""));
            }
            for (uri, pfx) in prefixes {
                out.push_str(&format!(" xmlns:{pfx}=\"{uri}\""));
            }
        }
        for a in &self.attrs {
            let an = prefixed(&a.ns, root_ns, prefixes, &a.local);
            out.push_str(&format!(" {an}=\"{}\"", escape_attr(&a.value)));
        }
        let text = &self.text;
        if self.children.is_empty() && text.is_empty() {
            out.push_str("/>");
            return;
        }
        out.push('>');
        if !text.is_empty() {
            out.push_str(&escape_text(text));
        }
        for c in &self.children {
            c.write_xml(root_ns, prefixes, false, out);
        }
        out.push_str(&format!("</{tag}>"));
    }
}

fn conventional_prefix(uri: &str) -> Option<String> {
    Some(match uri {
        A => "a",
        P => "p",
        R => "r",
        CT => "ct",
        PR => "pr",
        _ => return None,
    }
    .to_string())
}

fn prefixed(
    ns: &Option<String>,
    root_ns: &Option<String>,
    prefixes: &[(String, String)],
    local: &str,
) -> String {
    match ns {
        None => local.to_string(),
        Some(uri) => {
            if root_ns.as_deref() == Some(uri.as_str()) {
                local.to_string()
            } else if let Some((_, p)) = prefixes.iter().find(|(u, _)| u == uri) {
                format!("{p}:{local}")
            } else {
                local.to_string()
            }
        }
    }
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn qname(ns: Option<&str>, local: &str) -> String {
    match ns {
        Some(n) => format!("{{{n}}}{local}"),
        None => local.to_string(),
    }
}

fn push_child(stack: &mut [Element], root: &mut Option<Element>, el: Element) {
    if let Some(top) = stack.last_mut() {
        top.children.push(el);
    } else {
        *root = Some(el);
    }
}

fn elem_from_start(
    reader: &NsReader<&[u8]>,
    e: &quick_xml::events::BytesStart,
) -> Result<Element, String> {
    let local = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
    let (ns_res, _) = reader.resolve_element(e.name());
    let ns = match ns_res {
        ResolveResult::Bound(n) => Some(String::from_utf8_lossy(n.as_ref()).into_owned()),
        _ => None,
    };
    let mut attrs = Vec::new();
    for a in e.attributes() {
        let a = a.map_err(|e| format!("attr error: {e}"))?;
        // Skip namespace declarations (xmlns / xmlns:prefix).
        let key = a.key;
        if key.as_ref() == b"xmlns" || key.as_ref().starts_with(b"xmlns:") {
            continue;
        }
        let (attr_ns, attr_local) = reader.resolve_attribute(key);
        let ns_uri = match attr_ns {
            ResolveResult::Bound(n) => Some(String::from_utf8_lossy(n.as_ref()).into_owned()),
            _ => None,
        };
        let local = String::from_utf8_lossy(attr_local.as_ref()).into_owned();
        let value = a
            .decode_and_unescape_value(reader)
            .map_err(|e| format!("attr value: {e}"))?
            .into_owned();
        attrs.push(Attr {
            ns: ns_uri,
            local,
            value,
        });
    }
    Ok(Element {
        ns,
        local,
        attrs,
        children: Vec::new(),
        text: String::new(),
    })
}
