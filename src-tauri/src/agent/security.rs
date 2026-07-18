//! Security boundaries for the agent loop: SSRF/egress validation, output-path
//! containment + write-risk classification, the structured confidential-query
//! egress guard, and secret redaction.
//!
//! Everything here is deterministic and unit-tested. The networking side
//! ([`validate_url_for_egress`]) resolves and classifies every candidate IP and
//! rejects loopback/private/link-local/unspecified/multicast/reserved addresses;
//! the caller pins the vetted IP to the socket for each redirect hop while
//! preserving Host/TLS (the driver does the actual connect in Phase C).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};
use std::path::{Component, Path, PathBuf};

use fm_agent::types::Risk;

/// Why an outbound request was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EgressError {
    NotHttp,
    HasCredentials,
    NoHost,
    UnresolvableHost,
    PrivateOrReserved(String),
}

impl std::fmt::Display for EgressError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EgressError::NotHttp => write!(f, "only http(s) is allowed"),
            EgressError::HasCredentials => write!(f, "url must not embed credentials"),
            EgressError::NoHost => write!(f, "url has no host"),
            EgressError::UnresolvableHost => write!(f, "host did not resolve"),
            EgressError::PrivateOrReserved(ip) => {
                write!(f, "resolved to a private/reserved address: {ip}")
            }
        }
    }
}

/// True only for globally-routable unicast addresses. Rejects loopback,
/// private, link-local, unspecified, multicast, documentation, and reserved
/// ranges for both IPv4 and IPv6 (incl. IPv4-mapped IPv6).
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_public_v4(mapped);
            }
            if let Some(compat) = v6.to_ipv4() {
                // Deprecated IPv4-compatible; treat conservatively.
                return is_public_v4(compat);
            }
            is_public_v6(v6)
        }
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    !(ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ip.is_broadcast()
        || ip.is_documentation()
        // 100.64.0.0/10 CGNAT
        || (o[0] == 100 && (64..=127).contains(&o[1]))
        // 192.0.0.0/24 IETF protocol assignments
        || (o[0] == 192 && o[1] == 0 && o[2] == 0)
        // 240.0.0.0/4 reserved
        || o[0] >= 240
        // 0.0.0.0/8 "this network"
        || o[0] == 0)
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let seg = ip.segments();
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        // unique local fc00::/7
        || (seg[0] & 0xfe00) == 0xfc00
        // link-local fe80::/10
        || (seg[0] & 0xffc0) == 0xfe80
        // documentation 2001:db8::/32
        || (seg[0] == 0x2001 && seg[1] == 0x0db8))
}

/// Validate an outbound URL and return the vetted, public socket addresses to
/// pin. HTTP(S) only, no embedded credentials, host resolves, and *every*
/// resolved address is public (so a DNS-rebind that returns a private IP is
/// rejected).
pub fn validate_url_for_egress(url: &str) -> Result<Vec<std::net::SocketAddr>, EgressError> {
    let lower = url.trim();
    let (scheme, rest) = lower.split_once("://").ok_or(EgressError::NotHttp)?;
    let default_port = match scheme.to_ascii_lowercase().as_str() {
        "http" => 80u16,
        "https" => 443u16,
        _ => return Err(EgressError::NotHttp),
    };
    // authority is up to the first '/', '?' or '#'
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.contains('@') {
        return Err(EgressError::HasCredentials);
    }
    if authority.is_empty() {
        return Err(EgressError::NoHost);
    }
    // Split host:port, handling bracketed IPv6.
    let (host, port) = parse_host_port(authority, default_port);
    if host.is_empty() {
        return Err(EgressError::NoHost);
    }
    let addrs: Vec<std::net::SocketAddr> = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|_| EgressError::UnresolvableHost)?
        .collect();
    if addrs.is_empty() {
        return Err(EgressError::UnresolvableHost);
    }
    for a in &addrs {
        if !is_public_ip(a.ip()) {
            return Err(EgressError::PrivateOrReserved(a.ip().to_string()));
        }
    }
    Ok(addrs)
}

fn parse_host_port(authority: &str, default_port: u16) -> (String, u16) {
    if let Some(rest) = authority.strip_prefix('[') {
        // [ipv6]:port
        if let Some(end) = rest.find(']') {
            let host = &rest[..end];
            let after = &rest[end + 1..];
            let port = after
                .strip_prefix(':')
                .and_then(|p| p.parse().ok())
                .unwrap_or(default_port);
            return (host.to_string(), port);
        }
    }
    match authority.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() => {
            (h.to_string(), p.parse().unwrap_or(default_port))
        }
        _ => (authority.to_string(), default_port),
    }
}

/// Why an output filename was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PathError {
    Empty,
    HasSeparator,
    HasDrivePrefix,
    ParentTraversal,
    ReservedName,
}

impl std::fmt::Display for PathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PathError::Empty => write!(f, "empty filename"),
            PathError::HasSeparator => write!(f, "filename must not contain path separators"),
            PathError::HasDrivePrefix => write!(f, "filename must not contain a drive prefix"),
            PathError::ParentTraversal => write!(f, "filename must not traverse parents"),
            PathError::ReservedName => write!(f, "reserved Windows filename"),
        }
    }
}

const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Validate a server-owned output filename: no separators, drive prefixes,
/// reserved Windows names, or `..`. Returns the contained path under `root`.
pub fn contain_output_filename(root: &Path, filename: &str) -> Result<PathBuf, PathError> {
    if filename.trim().is_empty() {
        return Err(PathError::Empty);
    }
    if filename.contains('/') || filename.contains('\\') {
        return Err(PathError::HasSeparator);
    }
    if filename.contains(':') {
        return Err(PathError::HasDrivePrefix);
    }
    if filename == ".." || filename == "." {
        return Err(PathError::ParentTraversal);
    }
    let stem = filename
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    if WINDOWS_RESERVED.contains(&stem.as_str()) {
        return Err(PathError::ReservedName);
    }
    Ok(root.join(filename))
}

/// Classify the risk of writing to `target`, given the configured output root.
/// A new path inside the root is [`Risk::LocalCreate`]; overwriting an existing
/// path inside the root is [`Risk::LocalOverwrite`]; anything outside the root is
/// [`Risk::Export`]. Paths are compared after lexical normalization so a
/// `..`-escape resolves to `Export`, never a false `LocalCreate`.
pub fn classify_write_risk(root: &Path, target: &Path) -> Risk {
    let root_n = normalize(root);
    let target_n = normalize(target);
    let inside = target_n.starts_with(&root_n);
    if !inside {
        Risk::Export
    } else if target.exists() {
        Risk::LocalOverwrite
    } else {
        Risk::LocalCreate
    }
}

/// Lexically normalize a path (resolve `.`/`..` without touching the FS) for
/// containment comparison. Does not follow symlinks/reparse points — the driver
/// additionally canonicalizes and opens with reparse-safe flags before use.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The structured-query egress guard for `Confidential`/`Restricted` workspaces.
/// Only allowlisted public entity ids and enumerated public intent/period fields
/// may leave; any free-form literal requires disclosure approval.
pub struct EgressGuard<'a> {
    pub allowed_entities: &'a [String],
    pub allowed_fields: &'a [String],
}

/// Outcome of vetting a structured query term.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryVerdict {
    /// Allowed to leave without approval.
    Allowed,
    /// Free-form / non-allowlisted content; requires disclosure approval.
    RequiresApproval(String),
}

impl EgressGuard<'_> {
    /// Vet a structured query built from `(entity_id, field)` pairs. Any entity
    /// not on the allowlist, or any field not enumerated, requires approval.
    pub fn vet(&self, entity_id: &str, field: &str) -> QueryVerdict {
        if !self.allowed_entities.iter().any(|e| e == entity_id) {
            return QueryVerdict::RequiresApproval(format!("entity `{entity_id}` not allowlisted"));
        }
        if !self.allowed_fields.iter().any(|f| f == field) {
            return QueryVerdict::RequiresApproval(format!("field `{field}` not enumerated"));
        }
        QueryVerdict::Allowed
    }

    /// Any free-form literal term always requires approval in these tiers.
    pub fn vet_literal(&self, _term: &str) -> QueryVerdict {
        QueryVerdict::RequiresApproval("free-form literal requires disclosure approval".into())
    }
}

/// Redact secrets from text before it is persisted, logged, or sent to
/// telemetry: OpenRouter/OpenAI-style keys, bearer tokens, and generic long
/// hex/base64 secrets.
pub fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for token in split_keep(text) {
        if looks_secret(&token) {
            out.push_str("[REDACTED]");
        } else {
            out.push_str(&token);
        }
    }
    out
}

fn split_keep(text: &str) -> Vec<String> {
    // Split on whitespace but keep the separators so redaction is lossless.
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            out.push(ch.to_string());
        } else {
            cur.push(ch);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn looks_secret(tok: &str) -> bool {
    let t = tok.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_');
    if t.starts_with("sk-") && t.len() >= 20 {
        return true;
    }
    if t.starts_with("Bearer") {
        return true;
    }
    // Long opaque token: >=32 chars of hex/base64-ish.
    if t.len() >= 32
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '+' || c == '/')
    {
        // require a mix (not a plain English word) — has a digit.
        return t.chars().any(|c| c.is_ascii_digit());
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn private_and_reserved_ipv4_rejected() {
        for ip in [
            "127.0.0.1",
            "10.0.0.5",
            "192.168.1.1",
            "172.16.0.1",
            "169.254.1.1",
            "0.0.0.0",
            "100.64.0.1",
            "192.0.0.1",
            "240.0.0.1",
            "255.255.255.255",
            "224.0.0.1",
        ] {
            let a: Ipv4Addr = ip.parse().unwrap();
            assert!(!is_public_ip(IpAddr::V4(a)), "{ip} must be non-public");
        }
    }

    #[test]
    fn public_ipv4_allowed() {
        for ip in ["8.8.8.8", "1.1.1.1", "93.184.216.34"] {
            let a: Ipv4Addr = ip.parse().unwrap();
            assert!(is_public_ip(IpAddr::V4(a)), "{ip} must be public");
        }
    }

    #[test]
    fn private_ipv6_rejected() {
        for ip in ["::1", "::", "fc00::1", "fe80::1", "2001:db8::1", "ff02::1"] {
            let a: Ipv6Addr = ip.parse().unwrap();
            assert!(!is_public_ip(IpAddr::V6(a)), "{ip} must be non-public");
        }
        // IPv4-mapped private
        let mapped: Ipv6Addr = "::ffff:192.168.0.1".parse().unwrap();
        assert!(!is_public_ip(IpAddr::V6(mapped)));
    }

    #[test]
    fn egress_rejects_non_http_and_credentials() {
        assert_eq!(
            validate_url_for_egress("ftp://example.com/x"),
            Err(EgressError::NotHttp)
        );
        assert_eq!(
            validate_url_for_egress("file:///etc/passwd"),
            Err(EgressError::NotHttp)
        );
        assert_eq!(
            validate_url_for_egress("http://user:pass@example.com/"),
            Err(EgressError::HasCredentials)
        );
    }

    #[test]
    fn egress_rejects_literal_private_hosts() {
        assert!(matches!(
            validate_url_for_egress("http://127.0.0.1:8080/admin"),
            Err(EgressError::PrivateOrReserved(_))
        ));
        assert!(matches!(
            validate_url_for_egress("http://[::1]/"),
            Err(EgressError::PrivateOrReserved(_))
        ));
        assert!(matches!(
            validate_url_for_egress("http://169.254.169.254/latest/meta-data/"),
            Err(EgressError::PrivateOrReserved(_))
        ));
    }

    #[test]
    fn egress_allows_public_literal() {
        // 8.8.8.8 resolves trivially (literal); must be accepted.
        let addrs = validate_url_for_egress("https://8.8.8.8/").unwrap();
        assert!(addrs.iter().all(|a| is_public_ip(a.ip())));
    }

    #[test]
    fn output_filename_containment() {
        let root = Path::new("C:/out");
        assert!(contain_output_filename(root, "model.xlsx").is_ok());
        assert_eq!(
            contain_output_filename(root, "a/b.xlsx"),
            Err(PathError::HasSeparator)
        );
        assert_eq!(
            contain_output_filename(root, "a\\b.xlsx"),
            Err(PathError::HasSeparator)
        );
        assert_eq!(
            contain_output_filename(root, "C:evil.xlsx"),
            Err(PathError::HasDrivePrefix)
        );
        assert_eq!(
            contain_output_filename(root, ".."),
            Err(PathError::ParentTraversal)
        );
        assert_eq!(
            contain_output_filename(root, "CON.txt"),
            Err(PathError::ReservedName)
        );
        assert_eq!(
            contain_output_filename(root, "nul"),
            Err(PathError::ReservedName)
        );
        assert_eq!(contain_output_filename(root, ""), Err(PathError::Empty));
    }

    #[test]
    fn write_risk_classification() {
        let root = Path::new("C:/out");
        // A path escaping the root via .. is Export, never LocalCreate.
        assert_eq!(
            classify_write_risk(root, Path::new("C:/out/../secret/x.xlsx")),
            Risk::Export
        );
        assert_eq!(
            classify_write_risk(root, Path::new("D:/elsewhere/x.xlsx")),
            Risk::Export
        );
        // A fresh path inside the root is LocalCreate.
        assert_eq!(
            classify_write_risk(root, Path::new("C:/out/new_model.xlsx")),
            Risk::LocalCreate
        );
    }

    #[test]
    fn egress_guard_allowlist() {
        let entities = vec!["nvda".to_string(), "amd".to_string()];
        let fields = vec!["revenue".to_string(), "period".to_string()];
        let g = EgressGuard {
            allowed_entities: &entities,
            allowed_fields: &fields,
        };
        assert_eq!(g.vet("nvda", "revenue"), QueryVerdict::Allowed);
        assert!(matches!(
            g.vet("tsla", "revenue"),
            QueryVerdict::RequiresApproval(_)
        ));
        assert!(matches!(
            g.vet("nvda", "secret_field"),
            QueryVerdict::RequiresApproval(_)
        ));
        assert!(matches!(
            g.vet_literal("anything"),
            QueryVerdict::RequiresApproval(_)
        ));
    }

    #[test]
    fn redaction_hides_keys_but_keeps_prose() {
        let text = "use key sk-abcdef0123456789ABCDEF here and Bearer tok12345 too";
        let red = redact(text);
        assert!(!red.contains("sk-abcdef0123456789ABCDEF"), "{red}");
        assert!(red.contains("use key"));
        assert!(red.contains("here and"));
        // Ordinary words are preserved.
        let plain = redact("the quarterly revenue grew 12 percent");
        assert_eq!(plain, "the quarterly revenue grew 12 percent");
    }
}
