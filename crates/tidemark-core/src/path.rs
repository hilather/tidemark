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
        display_lossy_bytes(&self.0)
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
    /// This is not filesystem canonicalization: callers must canonicalize live
    /// query paths first when the CLI promises `realpath`.
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
        display_lossy_bytes(&self.0)
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
// Shared sibling-field encoding for report DTOs. Ordinary RawPath serde remains
// byte-oriented for internal storage; enclosing reports choose the wire adapter.
#[derive(Serialize, Deserialize, Default)]
pub(crate) struct PathFields {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_string"
    )]
    path: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_string"
    )]
    path_b64: Option<String>,
}
impl PathFields {
    fn encode(bytes: Option<&[u8]>) -> Self {
        match bytes {
            None => Self::default(),
            Some(bytes) => match std::str::from_utf8(bytes) {
                Ok(path) => Self {
                    path: Some(path.into()),
                    path_b64: None,
                },
                Err(_) => Self {
                    path: None,
                    path_b64: Some(STANDARD.encode(bytes)),
                },
            },
        }
    }
    fn decode<E: serde::de::Error>(self) -> Result<Option<Vec<u8>>, E> {
        decode_fields(self.path, self.path_b64)
    }
}
pub(crate) fn present_string<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    String::deserialize(d).map(Some)
}
pub(crate) fn decode_fields<E: serde::de::Error>(
    text: Option<String>,
    b64: Option<String>,
) -> Result<Option<Vec<u8>>, E> {
    match (text, b64) {
        (Some(_), Some(_)) => Err(E::custom("conflicting path representations")),
        (Some(text), None) => Ok(Some(text.into_bytes())),
        (None, Some(b64)) => STANDARD.decode(b64).map(Some).map_err(E::custom),
        (None, None) => Ok(None),
    }
}
pub(crate) fn encode_fields(bytes: Option<&[u8]>) -> (Option<String>, Option<String>) {
    let fields = PathFields::encode(bytes);
    (fields.path, fields.path_b64)
}
pub(crate) mod report_bytes {
    use super::*;
    pub fn serialize<S: serde::Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        PathFields::encode(Some(bytes)).serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        PathFields::deserialize(d)?
            .decode()?
            .ok_or_else(|| serde::de::Error::custom("path or path_b64 is required"))
    }
}
pub(crate) mod report_path {
    use super::*;
    pub fn serialize<S: serde::Serializer>(path: &RawPath, s: S) -> Result<S::Ok, S::Error> {
        report_bytes::serialize(path.as_bytes(), s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<RawPath, D::Error> {
        RawPath::new(report_bytes::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}
pub(crate) mod report_optional_path {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        path: &Option<RawPath>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        PathFields::encode(path.as_ref().map(RawPath::as_bytes)).serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<RawPath>, D::Error> {
        PathFields::deserialize(d)?
            .decode()?
            .map(RawPath::new)
            .transpose()
            .map_err(serde::de::Error::custom)
    }
}
fn path_json(bytes: &[u8]) -> serde_json::Value {
    match std::str::from_utf8(bytes) {
        Ok(s) => serde_json::json!({"path": s}),
        Err(_) => serde_json::json!({"path_b64": STANDARD.encode(bytes)}),
    }
}
/// Escape bytes for terminal display: printable Unicode is kept; control characters, invalid
/// UTF-8 bytes and backslashes are written as uppercase `\xNN`.
pub fn display_lossy_bytes(bytes: &[u8]) -> String {
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

#[cfg(test)]
mod shared_display_tests {
    use super::*;
    #[test]
    fn three_display_entry_points_agree() {
        for bytes in [
            b"plain".as_slice(),
            "é\u{0085}".as_bytes(),
            b"a\xff\n\\",
            b"\xe2\x82",
        ] {
            let name = RawName::new(bytes.to_vec()).unwrap();
            let path = RawPath::new([b"/".as_slice(), bytes].concat()).unwrap();
            assert_eq!(name.display_lossy(), display_lossy_bytes(bytes));
            assert_eq!(
                path.display_lossy(),
                format!("/{}", display_lossy_bytes(bytes))
            );
        }
    }
}
