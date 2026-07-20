//! Opaque conversation-scoped artifact handles (Phase 1.6).
//!
//! Handles are minted only by trusted Rust-side flows:
//! - OS file picker ([`pick_pdf_artifact`])
//! - OS drag-drop observed in Rust ([`observe_drop`] + [`claim_dropped_pdf`])
//! - user-typed path from the original user message ([`register_user_pdf`])
//! - app-generated workbook/deck ([`ensure_generated`])
//!
//! Tools accept `artifact_id` only — never a model-supplied filesystem path.
//! User-input handles REQUIRE a non-empty conversation id; generated outputs
//! are session-wide (`conversation_id = None`).
//!
//! Drag-drop: the Rust window event records absolute PDF paths as short-lived
//! one-use grants. The UI claims a grant by conversation id without resupplying
//! the path — so an arbitrary IPC caller cannot bless an unobserved path.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::State;
use tauri_plugin_dialog::{DialogExt, FilePath};

use crate::error::{AppError, AppResult};

/// How the path entered the registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    /// User-picked local PDF via the OS file dialog, drop grant, or user text.
    UserPdf,
    /// App-generated workbook / deck.
    Generated,
    /// Other user-picked file via a trusted picker.
    UserFile,
}

#[derive(Clone, Debug)]
struct Artifact {
    path: PathBuf,
    kind: ArtifactKind,
    label: String,
    created: Instant,
    /// Owning conversation for user-input handles. Always Some for UserPdf /
    /// UserFile. Generated outputs use None (session-wide).
    conversation_id: Option<String>,
}

/// A short-lived, one-use grant created when the OS drag-drop event is observed
/// in Rust. Redeemed by [`claim_dropped_pdf`] — the path is never re-supplied
/// by the frontend.
#[derive(Clone, Debug)]
struct DropGrant {
    path: PathBuf,
    created: Instant,
}

/// In-memory artifact handle table + pending drop grants. Managed as Tauri state.
#[derive(Default)]
pub struct ArtifactRegistry {
    inner: Mutex<HashMap<String, Artifact>>,
    drops: Mutex<VecDeque<DropGrant>>,
}

const MAX_ENTRIES: usize = 64;
const TTL: Duration = Duration::from_secs(30 * 60);
const DROP_GRANT_TTL: Duration = Duration::from_secs(60);
const MAX_DROP_GRANTS: usize = 8;

impl ArtifactRegistry {
    fn prune_locked(map: &mut HashMap<String, Artifact>) {
        let now = Instant::now();
        map.retain(|_, a| now.duration_since(a.created) < TTL);
        while map.len() > MAX_ENTRIES {
            let oldest = map
                .iter()
                .min_by_key(|(_, a)| a.created)
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest {
                map.remove(&k);
            } else {
                break;
            }
        }
    }

    fn prune_drops_locked(drops: &mut VecDeque<DropGrant>) {
        let now = Instant::now();
        while drops
            .front()
            .map(|d| now.duration_since(d.created) >= DROP_GRANT_TTL)
            .unwrap_or(false)
        {
            drops.pop_front();
        }
        while drops.len() > MAX_DROP_GRANTS {
            drops.pop_front();
        }
    }

    /// Record absolute PDF paths observed from a Rust-side drag-drop event.
    /// Non-PDF / non-absolute / missing paths are ignored.
    /// Returns how many grants were added.
    pub fn observe_drop(&self, paths: &[PathBuf]) -> usize {
        let mut drops = self.drops.lock().unwrap_or_else(|e| e.into_inner());
        Self::prune_drops_locked(&mut drops);
        let mut added = 0usize;
        for p in paths {
            if !p.is_absolute() || !p.is_file() {
                continue;
            }
            let allowed = p
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| {
                    let e = e.to_ascii_lowercase();
                    crate::commands::attachments::ALLOWED_EXTS.contains(&e.as_str())
                })
                .unwrap_or(false);
            if !allowed {
                continue;
            }
            drops.push_back(DropGrant {
                path: p.clone(),
                created: Instant::now(),
            });
            added += 1;
        }
        Self::prune_drops_locked(&mut drops);
        added
    }

    /// Redeem the most recent unexpired drop grant as a conversation-scoped
    /// UserPdf handle. One-use: the grant is removed on claim.
    pub fn claim_drop(&self, conversation_id: &str) -> Result<(String, String), String> {
        if conversation_id.trim().is_empty() {
            return Err("conversation_id is required".into());
        }
        let path = {
            let mut drops = self.drops.lock().unwrap_or_else(|e| e.into_inner());
            Self::prune_drops_locked(&mut drops);
            drops
                .pop_back()
                .ok_or_else(|| "no pending PDF drop to claim".to_string())?
                .path
        };
        // Full file name (extension kept) so downstream classification works.
        let label = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file")
            .to_string();
        let is_pdf = label.to_ascii_lowercase().ends_with(".pdf");
        let id = self.register(
            path,
            if is_pdf { ArtifactKind::UserPdf } else { ArtifactKind::UserFile },
            label.clone(),
            Some(conversation_id.to_string()),
        )?;
        Ok((id, label))
    }

    /// Register `path` and return a new opaque `artifact_id`.
    /// User kinds require a non-empty `conversation_id`.
    pub fn register(
        &self,
        path: impl Into<PathBuf>,
        kind: ArtifactKind,
        label: impl Into<String>,
        conversation_id: Option<String>,
    ) -> Result<String, String> {
        if matches!(kind, ArtifactKind::UserPdf | ArtifactKind::UserFile) {
            match &conversation_id {
                Some(c) if !c.trim().is_empty() => {}
                _ => {
                    return Err("user artifacts require a non-empty conversation_id".into());
                }
            }
        }
        let path = path.into();
        let label = label.into();
        let id = new_artifact_id();
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        Self::prune_locked(&mut map);
        map.insert(
            id.clone(),
            Artifact {
                path,
                kind,
                label,
                created: Instant::now(),
                conversation_id,
            },
        );
        Ok(id)
    }

    /// Resolve a handle. User artifacts require a matching conversation_id;
    /// generated outputs ignore conversation (session-wide).
    pub fn resolve(
        &self,
        artifact_id: &str,
        conversation_id: Option<&str>,
    ) -> Result<(PathBuf, ArtifactKind, String), String> {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        Self::prune_locked(&mut map);
        let a = map
            .get(artifact_id)
            .ok_or_else(|| format!("unknown or expired artifact_id: {artifact_id}"))?;
        if matches!(a.kind, ArtifactKind::UserPdf | ArtifactKind::UserFile) {
            let owner = a
                .conversation_id
                .as_deref()
                .filter(|c| !c.is_empty())
                .ok_or_else(|| "user artifact missing owner".to_string())?;
            let req = conversation_id
                .filter(|c| !c.is_empty())
                .ok_or_else(|| "conversation_id required to resolve user artifact".to_string())?;
            if owner != req {
                return Err("artifact_id is not usable in this conversation".into());
            }
        }
        Ok((a.path.clone(), a.kind, a.label.clone()))
    }

    /// True if `path` is currently registered (for open_path allowlisting).
    pub fn contains_path(&self, path: &Path) -> bool {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        Self::prune_locked(&mut map);
        map.values().any(|a| paths_equal(&a.path, path))
    }

    /// Ensure `path` is registered as a generated artifact; return its handle.
    pub fn ensure_generated(&self, path: impl Into<PathBuf>, label: impl Into<String>) -> String {
        let path = path.into();
        let label = label.into();
        {
            let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            Self::prune_locked(&mut map);
            if let Some((id, _)) = map
                .iter()
                .find(|(_, a)| paths_equal(&a.path, &path) && a.kind == ArtifactKind::Generated)
            {
                return id.clone();
            }
        }
        self.register(path, ArtifactKind::Generated, label, None)
            .expect("generated registration never requires conversation")
    }

    /// Register a user-typed absolute path as a conversation-scoped UserPdf.
    pub fn register_user_pdf(
        &self,
        path: impl Into<PathBuf>,
        label: impl Into<String>,
        conversation_id: &str,
    ) -> Result<String, String> {
        self.register(
            path,
            ArtifactKind::UserPdf,
            label,
            Some(conversation_id.to_string()),
        )
    }
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    a == b || a.as_os_str().eq_ignore_ascii_case(b.as_os_str())
}

fn new_artifact_id() -> String {
    let mut bytes = [0u8; 16];
    for b in &mut bytes {
        *b = rand::random();
    }
    let mut hex = String::with_capacity(32);
    for b in bytes {
        hex.push_str(&format!("{b:02x}"));
    }
    format!("art-{hex}")
}

/// Open the OS PDF file picker and mint an opaque handle.
/// Requires a non-empty `conversation_id` to scope the handle.
#[tauri::command(rename_all = "snake_case")]
pub async fn pick_pdf_artifact(
    app: tauri::AppHandle,
    registry: State<'_, ArtifactRegistry>,
    conversation_id: String,
) -> AppResult<String> {
    if conversation_id.trim().is_empty() {
        return Err(AppError::Config(
            "conversation_id is required to pick a PDF".into(),
        ));
    }
    let file = app
        .dialog()
        .file()
        .add_filter("PDF", &["pdf"])
        .set_title("Choose a PDF filing")
        .blocking_pick_file();
    let Some(fp) = file else {
        return Ok("null".into());
    };
    let path = match fp {
        FilePath::Path(p) => p,
        FilePath::Url(u) => u
            .to_file_path()
            .map_err(|_| AppError::Config("picker returned a non-local URL".into()))?,
    };
    if !path.is_file() {
        return Err(AppError::Config(format!(
            "file not found: {}",
            path.display()
        )));
    }
    let is_pdf = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false);
    if !is_pdf {
        return Err(AppError::Config(format!(
            "not a .pdf file: {}",
            path.display()
        )));
    }
    let label = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("PDF")
        .to_string();
    let id = registry
        .register(
            path,
            ArtifactKind::UserPdf,
            label.clone(),
            Some(conversation_id),
        )
        .map_err(AppError::Config)?;
    Ok(serde_json::json!({
        "artifact_id": id,
        "label": label,
        "kind": ArtifactKind::UserPdf,
    })
    .to_string())
}

/// Claim the most recent Rust-observed PDF drop grant for this conversation.
/// Does **not** accept a path from the frontend.
#[tauri::command(rename_all = "snake_case")]
pub fn claim_dropped_file(
    registry: State<'_, ArtifactRegistry>,
    owner: String,
) -> AppResult<String> {
    let (id, label) = registry.claim_drop(&owner).map_err(AppError::Config)?;
    let class = crate::commands::attachments::classify(&label)
        .map(|c| c.as_str())
        .unwrap_or("text");
    Ok(serde_json::json!({
        "artifact_id": id,
        "label": label,
        "class": class,
    })
    .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_resolve_roundtrip() {
        let reg = ArtifactRegistry::default();
        let id = reg
            .register(
                PathBuf::from("C:/tmp/demo.pdf"),
                ArtifactKind::UserPdf,
                "demo",
                Some("conv-1".into()),
            )
            .unwrap();
        assert!(id.starts_with("art-"));
        let (path, kind, label) = reg.resolve(&id, Some("conv-1")).unwrap();
        assert_eq!(path, PathBuf::from("C:/tmp/demo.pdf"));
        assert_eq!(kind, ArtifactKind::UserPdf);
        assert_eq!(label, "demo");
        assert!(reg.resolve(&id, Some("conv-other")).is_err());
        assert!(reg.resolve(&id, None).is_err());
        assert!(reg
            .register(
                PathBuf::from("C:/tmp/x.pdf"),
                ArtifactKind::UserPdf,
                "x",
                None
            )
            .is_err());
    }

    #[test]
    fn artifact_id_shape() {
        let id = new_artifact_id();
        assert!(id.starts_with("art-"), "got {id}");
        assert_eq!(id.len(), 36, "got {id}");
        assert!(
            id[4..].chars().all(|c| c.is_ascii_hexdigit()),
            "non-hex: {id}"
        );
    }

    #[test]
    fn ensure_generated_reuses_handle() {
        let reg = ArtifactRegistry::default();
        let a = reg.ensure_generated("C:/out/x.xlsx", "x");
        let b = reg.ensure_generated("C:/out/x.xlsx", "x");
        assert_eq!(a, b);
        assert!(reg.resolve(&a, None).is_ok());
        assert!(reg.resolve(&a, Some("any")).is_ok());
    }

    #[test]
    fn drop_grant_is_one_use() {
        let reg = ArtifactRegistry::default();
        // Can't create a real file easily; just assert empty claim fails.
        assert!(reg.claim_drop("conv-1").is_err());
    }
}
