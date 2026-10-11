//! Owned by: W1-A
//! Stable probe wire schema and complete text sections.
use super::*;
#[test]
fn json_snapshot_and_text_unknowns() {
    let report = Report {
        filesystem: ProbeReport {
            host: tidemark_fs::probe::HostFacts {
                facts: [(
                    "kernel".into(),
                    Fact {
                        value: Some("4.18.0-test".into()),
                        why: None,
                    },
                )]
                .into(),
            },
            roots: vec![tidemark_fs::probe::RootFacts {
                root: tidemark_core::path::RawPath::new(b"/data/\xff".to_vec()).unwrap(),
                facts: [
                    (
                        "fanotify_available".into(),
                        Fact {
                            value: None,
                            why: Some("NotYetImplemented".into()),
                        },
                    ),
                    (
                        "lvm".into(),
                        Fact {
                            value: Some("n/a".into()),
                            why: Some("awaits W4-A".into()),
                        },
                    ),
                ]
                .into(),
                leaves: None,
                mount_point: None,
                other_mounts: Some(vec![serde_json::json!({"path":"/bind"})]),
            }],
            recommendations: vec![],
        },
        postgres: [(
            "reachable".into(),
            Fact {
                value: None,
                why: Some("NotYetImplemented".into()),
            },
        )]
        .into(),
    };
    let mut output = Vec::new();
    write(&report, true, &mut output).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    insta::assert_json_snapshot!("probe_v1", value);
    output.clear();
    write(&report, false, &mut output).unwrap();
    let text = String::from_utf8(output).unwrap();
    for section in [
        "Host",
        "Root /data/\\xFF",
        "PostgreSQL",
        "Recommendations",
        "? reachable: unknown (NotYetImplemented)",
    ] {
        assert!(text.contains(section), "{text}");
    }
}
