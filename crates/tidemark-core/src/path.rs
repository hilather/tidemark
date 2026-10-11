//! Owned by: W0-B1
//! Validated byte paths and terminal-safe display.

use crate::error::CoreError;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Deserializer, Serialize};
use std::{os::unix::ffi::OsStrExt, path::Path};

/// A non-empty directory-entry name without slash or NUL.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct RawName(Vec<u8>);
/// An absolute, lexically normalized path without NUL.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct RawPath(Vec<u8>);

impl RawName {
    /// Validate a name without converting its encoding.
    pub fn new(bytes: impl Into<Vec<u8>>) -> Result<Self, CoreError> {
        let bytes = bytes.into();
        if bytes.is_empty()
            || bytes.contains(&b'/')
            || bytes.contains(&0)
            || bytes == b"."
            || bytes == b".."
        {
            return Err(CoreError::Path(
                "name must be a non-empty component without slash, NUL, dot or dot-dot".into(),
            ));
        }
        Ok(Self(bytes))
    }
    /// Construct from an OS path containing one component.
    pub fn from_path(path: &Path) -> Result<Self, CoreError> {
        Self::new(path.as_os_str().as_bytes().to_vec())
    }
    /// Borrow the original byte name as an OS path.
    pub fn as_path(&self) -> &Path {
        Path::new(std::ffi::OsStr::from_bytes(&self.0))
    }
    /// Borrow the name bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
    /// Escape controls, backslashes and invalid UTF-8 for terminal display.
    pub fn display_lossy(&self) -> String {
        display(&self.0)
    }
    /// xxh3 bits reinterpreted as a signed SQLite integer.
    pub fn hash64(&self) -> i64 {
        xxhash_rust::xxh3::xxh3_64(&self.0) as i64
    }
    /// JSON path representation preserving every byte.
    pub fn to_json(&self) -> serde_json::Value {
        path_json(&self.0)
    }
}
impl RawPath {
    /// Normalize an absolute raw path lexically; no filesystem access occurs.
    pub fn new(bytes: impl Into<Vec<u8>>) -> Result<Self, CoreError> {
        let bytes = bytes.into();
        if !bytes.starts_with(b"/") || bytes.contains(&0) {
            return Err(CoreError::Path(
                "path must be absolute and contain no NUL".into(),
            ));
        }
        let mut out = Vec::with_capacity(bytes.len());
        out.push(b'/');
        for part in bytes.split(|&b| b == b'/') {
            if part.is_empty() || part == b"." {
                continue;
            }
            if part == b".." {
                if out.len() > 1 {
                    let end = out.iter().rposition(|&b| b == b'/').unwrap_or(0);
                    out.truncate(end.max(1));
                }
                continue;
            }
            if out.len() > 1 {
                out.push(b'/');
            }
            out.extend_from_slice(part);
        }
        Ok(Self(out))
    }
    /// Construct from an OS path without requiring UTF-8.
    pub fn from_path(path: &Path) -> Result<Self, CoreError> {
        Self::new(path.as_os_str().as_bytes().to_vec())
    }
    /// Borrow the normalized path as an OS path.
    pub fn as_path(&self) -> &Path {
        Path::new(std::ffi::OsStr::from_bytes(&self.0))
    }
    /// Borrow the normalized bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
    /// Append a validated name.
    pub fn join(&self, name: &RawName) -> Self {
        let mut bytes = self.0.clone();
        if bytes.len() > 1 {
            bytes.push(b'/');
        }
        bytes.extend_from_slice(&name.0);
        Self(bytes)
    }
    /// Parent path; the root has no parent.
    pub fn parent(&self) -> Option<Self> {
        if self.0.len() == 1 {
            return None;
        }
        let end = self.0.iter().rposition(|&b| b == b'/').unwrap_or(0);
        Some(Self(self.0[..end.max(1)].to_vec()))
    }
    /// Last component; the root has no name.
    pub fn file_name(&self) -> Option<RawName> {
        if self.0.len() == 1 {
            return None;
        }
        let start = self.0.iter().rposition(|&b| b == b'/').unwrap_or(0) + 1;
        Some(RawName(self.0[start..].to_vec()))
    }
    /// Whether this path is inside the given component-wise prefix.
    pub fn starts_with(&self, prefix: &Self) -> bool {
        prefix.0 == b"/"
            || self.0 == prefix.0
            || (self.0.starts_with(&prefix.0) && self.0.get(prefix.0.len()) == Some(&b'/'))
    }
    /// Escape controls, backslashes and invalid UTF-8 for terminal display.
    pub fn display_lossy(&self) -> String {
        display(&self.0)
    }
    /// xxh3 bits reinterpreted as a signed SQLite integer.
    pub fn hash64(&self) -> i64 {
        xxhash_rust::xxh3::xxh3_64(&self.0) as i64
    }
    /// JSON uses `path` for UTF-8 and `path_b64` otherwise.
    pub fn to_json(&self) -> serde_json::Value {
        path_json(&self.0)
    }
}
impl<'de> Deserialize<'de> for RawPath {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::new(Vec::<u8>::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}
impl<'de> Deserialize<'de> for RawName {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::new(Vec::<u8>::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}
fn path_json(bytes: &[u8]) -> serde_json::Value {
    match std::str::from_utf8(bytes) {
        Ok(s) => serde_json::json!({"path": s}),
        Err(_) => serde_json::json!({"path_b64": STANDARD.encode(bytes)}),
    }
}
fn display(bytes: &[u8]) -> String {
    let mut result = String::new();
    let mut remaining = bytes;
    while !remaining.is_empty() {
        let (valid, invalid) = match std::str::from_utf8(remaining) {
            Ok(s) => (s, 0),
            Err(e) => (
                std::str::from_utf8(&remaining[..e.valid_up_to()]).unwrap_or(""),
                e.error_len().unwrap_or(remaining.len() - e.valid_up_to()),
            ),
        };
        for c in valid.chars() {
            if c.is_control() || c == '\\' {
                let mut buf = [0; 4];
                for b in c.encode_utf8(&mut buf).as_bytes() {
                    result.push_str(&format!("\\x{b:02X}"));
                }
            } else {
                result.push(c);
            }
        }
        let offset = valid.len();
        for b in &remaining[offset..offset + invalid] {
            result.push_str(&format!("\\x{b:02X}"));
        }
        remaining = &remaining[offset + invalid..];
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStringExt;
    #[test]
    fn normalization_and_components() {
        let p = RawPath::new(b"//data/./a/../rep/".to_vec()).unwrap();
        assert_eq!(p.as_bytes(), b"/data/rep");
        assert!(!p.starts_with(&RawPath::new(b"/data/re".to_vec()).unwrap()));
        assert!(p.starts_with(&RawPath::new(b"/data".to_vec()).unwrap()));
        assert_eq!(p.parent().unwrap().join(&p.file_name().unwrap()), p);
        assert_eq!(RawPath::new(b"/../../".to_vec()).unwrap().as_bytes(), b"/");
    }
    #[test]
    fn names_validate_and_escape_unicode_controls() {
        for invalid in [b"".as_slice(), b"a/b", b"a\0b", b".", b".."] {
            assert!(RawName::new(invalid.to_vec()).is_err());
        }
        let name = RawName::new(b"a\xff".to_vec()).unwrap();
        assert_eq!(
            serde_json::from_str::<RawName>(&serde_json::to_string(&name).unwrap()).unwrap(),
            name
        );
        assert_eq!(
            RawPath::new("/é\u{0085}".as_bytes().to_vec())
                .unwrap()
                .display_lossy(),
            "/é\\xC2\\x85"
        );
        assert!(RawPath::new(b"/a\0b".to_vec()).is_err());
    }
    #[test]
    fn non_utf8_roundtrip_and_escaping() {
        let os = std::ffi::OsString::from_vec(b"/a\xff\n\\".to_vec());
        let p = RawPath::from_path(Path::new(&os)).unwrap();
        assert_eq!(p.as_path(), Path::new(&os));
        assert_eq!(p.display_lossy(), "/a\\xFF\\x0A\\x5C");
        assert_eq!(
            STANDARD
                .decode(p.to_json()["path_b64"].as_str().unwrap())
                .unwrap(),
            p.as_bytes()
        );
        assert_eq!(
            serde_json::from_str::<RawPath>(&serde_json::to_string(&p).unwrap()).unwrap(),
            p
        );
        assert!(serde_json::from_str::<RawPath>("[97]").is_err());
    }
}
