//! OOXML package (zip) read/write, order-preserving.
//!
//! Mirrors the Python `_read_zip` / `_write_zip` helpers in `pptx_editor.py`:
//! read every member into an ordered list, edit in place, write back with
//! `ZIP_DEFLATED`. Order preservation matters so control-file rewrites land in
//! the same package positions the reference implementation uses.

use std::io::{Read, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

/// An in-memory OOXML package: member names in original order + their bytes.
#[derive(Debug, Clone, Default)]
pub struct Package {
    /// Member names in archive order.
    pub names: Vec<String>,
    /// Member bytes, parallel to [`Package::names`].
    pub data: Vec<Vec<u8>>,
}

impl Package {
    /// Read a `.pptx` from disk into an ordered member map.
    pub fn read(path: &str) -> Result<Package, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
        Package::from_bytes(&bytes)
    }

    /// Parse an in-memory zip.
    pub fn from_bytes(bytes: &[u8]) -> Result<Package, String> {
        let cursor = std::io::Cursor::new(bytes);
        let mut zip = ZipArchive::new(cursor).map_err(|e| format!("open zip: {e}"))?;
        let mut names = Vec::new();
        let mut data = Vec::new();
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).map_err(|e| format!("zip entry {i}: {e}"))?;
            if !f.is_file() {
                continue;
            }
            let name = f.name().to_string();
            let mut buf = Vec::with_capacity(f.size() as usize);
            f.read_to_end(&mut buf)
                .map_err(|e| format!("read {name}: {e}"))?;
            names.push(name);
            data.push(buf);
        }
        Ok(Package { names, data })
    }

    /// Index of a member by exact name.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n == name)
    }

    /// Bytes of a member by name.
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.index_of(name).map(|i| self.data[i].as_slice())
    }

    /// Insert or replace a member, preserving position when it already exists.
    pub fn set(&mut self, name: &str, bytes: Vec<u8>) {
        match self.index_of(name) {
            Some(i) => self.data[i] = bytes,
            None => {
                self.names.push(name.to_string());
                self.data.push(bytes);
            }
        }
    }

    /// Remove a member by name (no-op if absent).
    pub fn remove(&mut self, name: &str) {
        if let Some(i) = self.index_of(name) {
            self.names.remove(i);
            self.data.remove(i);
        }
    }

    /// Write the package to disk with DEFLATE compression, in member order.
    pub fn write(&self, path: &str) -> Result<(), String> {
        let f = std::fs::File::create(path).map_err(|e| format!("create {path}: {e}"))?;
        let mut zw = ZipWriter::new(f);
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, bytes) in self.names.iter().zip(self.data.iter()) {
            zw.start_file(name.clone(), opts)
                .map_err(|e| format!("start {name}: {e}"))?;
            zw.write_all(bytes)
                .map_err(|e| format!("write {name}: {e}"))?;
        }
        zw.finish().map_err(|e| format!("finish zip: {e}"))?;
        Ok(())
    }
}
