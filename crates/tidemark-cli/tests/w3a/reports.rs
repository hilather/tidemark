//! Owned by: W3-A
//! Deterministic text and JSON goldens including invalid UTF-8.
use tidemark_core::path::RawPath;
use tidemark_store::diff::*;
#[test]
fn text_and_json() {
    let root = RawPath::new(b"/fixture".to_vec()).unwrap();
    let path = RawPath::new(b"/fixture/a\xff".to_vec()).unwrap();
    let rows = vec![
        DiffRow {
            path: root.clone(),
            depth_rel: 0,
            new: None,
            old: Some(2048),
            delta: -2048,
            class: DiffClass::Removed,
        },
        DiffRow {
            path: path.clone(),
            depth_rel: 1,
            new: None,
            old: Some(1024),
            delta: -1024,
            class: DiffClass::Removed,
        },
    ];
    let explanation = Explanation {
        target_delta: -2048,
        steps: vec![ExplainStep {
            path,
            delta: -1024,
            share_of_parent: 0.5,
        }],
        own_delta: -1024,
        spread: vec![],
        spread_children: 0,
    };
    for json in [false, true] {
        let mut out = Vec::new();
        tidemark_cli::render::diff::report(
            &root,
            &rows,
            Some(&explanation),
            false,
            Some("comparison snapshot actual age: 2h"),
            json,
            &mut out,
        )
        .unwrap();
        insta::with_settings!({snapshot_path => "snapshots"}, {
            if json { let doc:serde_json::Value=serde_json::from_slice(&out).unwrap();
                assert!(doc["data"]["diff"][1].get("path").is_none());
                assert_eq!(doc["data"]["diff"][1]["path_b64"],"L2ZpeHR1cmUvYf8=");
                insta::assert_json_snapshot!("diff_json",doc);
            } else { insta::assert_snapshot!("diff_text",String::from_utf8(out).unwrap()); }
        });
    }
}
#[test]
fn files_are_counts() {
    let root = RawPath::new(b"/fixture".to_vec()).unwrap();
    let rows = vec![DiffRow {
        path: root.clone(),
        depth_rel: 0,
        new: Some(2000),
        old: Some(1000),
        delta: 1000,
        class: DiffClass::Grown,
    }];
    let mut out = Vec::new();
    tidemark_cli::render::diff::report(&root, &rows, None, true, None, false, &mut out).unwrap();
    assert!(String::from_utf8(out).unwrap().contains("+1000"));
}
