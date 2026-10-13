//! Owned by: W3-B
//! Byte-safe symlink text and JSON reports.
use std::io::Write;
use tidemark_core::path::{display_lossy_bytes, RawPath};
use tidemark_fs::symlinks::SymlinkReport;
/// Render counts and filtered examples with byte-preserving JSON paths and targets.
pub fn report(
    root: &RawPath,
    report: &SymlinkReport,
    json: bool,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    if json {
        let examples:Vec<_>=report.examples.iter().map(|(p,t,c)|serde_json::json!({"link":p.to_json(),"target":target_json(t),"class":c})).collect();
        crate::render::json(
            "tidemark.symlinks.v1",
            serde_json::json!({"root":root.to_json(),"total":report.total,"absolute":report.absolute,"relative":report.relative,"by_class":report.by_class,"examples":examples}),
            out,
        )?;
    } else {
        writeln!(
            out,
            "Symlinks under {}: {} (absolute {}, relative {})",
            root.display_lossy(),
            report.total,
            report.absolute,
            report.relative
        )?;
        for (class, count) in &report.by_class {
            writeln!(out, "{}: {count}", class_label(*class))?;
        }
        for (path, target, class) in &report.examples {
            writeln!(
                out,
                "{} -> {} ({})",
                path.display_lossy(),
                display_lossy_bytes(target),
                class_label(*class)
            )?;
        }
    }
    Ok(())
}
fn class_label(class: tidemark_fs::symlinks::SymlinkClass) -> &'static str {
    use tidemark_fs::symlinks::SymlinkClass::*;
    match class {
        InsideRoot => "inside root",
        OutsideRoot => "outside root",
        OtherFs => "leaves the mount",
        Dangling => "dangling",
        Loop => "loop",
        Unreadable => "unreadable",
    }
}
fn target_json(bytes: &[u8]) -> serde_json::Value {
    // Target text may contain slashes and dot-dot; preserve it without normalization.
    match std::str::from_utf8(bytes) {
        Ok(s) => serde_json::json!({"path":s}),
        Err(_) => {
            serde_json::json!({"path_b64":encode_base64(bytes)})
        }
    }
}

// The CLI has no base64 dependency; targets cannot use RawPath::to_json because
// lexical normalization would alter their exact bytes. Standard padded encoding.
fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        out.push(char::from(TABLE[usize::from(a >> 2)]));
        out.push(char::from(TABLE[usize::from(((a & 3) << 4) | (b >> 4))]));
        out.push(if chunk.len() > 1 {
            char::from(TABLE[usize::from(((b & 15) << 2) | (c >> 6))])
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            char::from(TABLE[usize::from(c & 63)])
        } else {
            '='
        });
    }
    out
}
#[cfg(test)]
mod tests {
    #[test]
    fn every_class_has_an_operator_label() {
        use tidemark_fs::symlinks::{SymlinkClass::*, SymlinkReport};
        let classes = [InsideRoot, OutsideRoot, OtherFs, Dangling, Loop, Unreadable];
        let labels = [
            "inside root",
            "outside root",
            "leaves the mount",
            "dangling",
            "loop",
            "unreadable",
        ];
        let root = tidemark_core::path::RawPath::new(b"/r".to_vec()).unwrap();
        let report = SymlinkReport {
            total: 6,
            absolute: 0,
            relative: 6,
            by_class: classes.iter().map(|c| (*c, 1)).collect(),
            examples: classes
                .iter()
                .map(|c| (root.clone(), b"target".to_vec(), *c))
                .collect(),
        };
        let mut out = Vec::new();
        super::report(&root, &report, false, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        for label in labels {
            assert!(text.contains(&format!("{label}: 1")), "{text}");
            assert!(text.contains(&format!("({label})")), "{text}");
        }
    }
    #[test]
    fn target_encoding_preserves_relative_bytes() {
        assert_eq!(
            super::target_json(b"../x"),
            serde_json::json!({"path":"../x"})
        );
        for (bytes, encoded) in [
            (b"".as_slice(), ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (&[255], "/w=="),
        ] {
            assert_eq!(super::encode_base64(bytes), encoded);
        }
        assert_eq!(
            super::target_json(&[255, b'/', b'.']),
            serde_json::json!({"path_b64":"/y8u"})
        );
    }
}
