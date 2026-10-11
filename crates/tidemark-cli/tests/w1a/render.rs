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

#[test]
fn parsed_mount_fields_cannot_forge_text_lines() {
    for (encoded, escaped) in [("\\012", "\\x0A"), ("\\033", "\\x1B"), ("\\134", "\\x5C")] {
        let input = format!("1 0 8:0 / /mnt/a{encoded}b rw,opt=a{encoded}b - xfs /dev/é{encoded}spoof rw,opt=a{encoded}b\n");
        let mounts = tidemark_fs::mount::parse_mountinfo(input.as_bytes()).unwrap();
        let mount = &mounts[0];
        let report = Report {
            filesystem: ProbeReport {
                host: Default::default(),
                roots: vec![tidemark_fs::probe::RootFacts {
                    root: mount.mount_point.clone(),
                    facts: [
                        (
                            "mount_options".into(),
                            Fact {
                                value: Some(mount.mount_opts.join(",")),
                                why: None,
                            },
                        ),
                        (
                            "super_options".into(),
                            Fact {
                                value: Some(mount.super_opts.join(",")),
                                why: None,
                            },
                        ),
                        (
                            "source".into(),
                            Fact {
                                value: Some(mount.source.clone()),
                                why: None,
                            },
                        ),
                        (
                            "mount_point".into(),
                            Fact {
                                value: Some(mount.mount_point.display_lossy()),
                                why: None,
                            },
                        ),
                    ]
                    .into(),
                    leaves: None,
                    mount_point: None,
                    other_mounts: Some(vec![]),
                }],
                recommendations: vec![],
            },
            postgres: Default::default(),
        };
        let mut output = Vec::new();
        write(&report, false, &mut output).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(
            text.contains(&format!("source: /dev/é{escaped}spoof")),
            "{text}"
        );
        assert!(
            text.contains(&format!("mount_point: /mnt/a{escaped}b")),
            "{text}"
        );
        assert!(text.contains(&format!("mount_options: rw,opt=a{escaped}b")));
        assert!(text.contains(&format!("super_options: rw,opt=a{escaped}b")));
        assert!(!text.contains('\u{1b}'));
        assert!(!text.lines().any(|line| line == "spoof"));
    }
}
