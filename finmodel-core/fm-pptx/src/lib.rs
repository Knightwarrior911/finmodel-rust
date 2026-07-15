//! `fm-pptx` — PowerPoint subsystem, a direct zip+XML OOXML/DrawingML port of
//! the Python `src/research/pptx_*.py` toolchain.
//!
//! Modules, in the dependency order of the port:
//! - [`inspect`]  — `inspect_pptx` structural reverse-engineering (6.1)
//! - [`edit`]     — slide-structure + theme + text zip/XML editing (6.2, 6.5 primitives)
//! - [`writer`]   — pure archetype helpers + DrawingML deck emission (6.3, 6.4)
//! - [`render`]   — render-to-image via soffice/pdftoppm subprocess (6.5)

pub mod edit;
pub mod inspect;
pub mod pkg;
pub mod render;
pub mod writer;
pub mod xmldom;
