//! Owned by: WCLI-format
//! Guard human rendering against accidental Rust Debug interpolation.

use std::path::Path;

fn scan(path: &Path, failures: &mut Vec<String>) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(path)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()?;
    entries.sort();
    for file in entries {
        if file.is_dir() {
            scan(&file, failures)?;
        } else if file.extension().is_some_and(|ext| ext == "rs") {
            let source = std::fs::read_to_string(&file)?;
            for (index, line) in source.lines().enumerate() {
                if !debug_field(line) {
                    continue;
                }
                // Existing deploy fixture serializes a temporary path into TOML in a test.
                let test_fixture = file.ends_with("cmd/deploy/stage.rs")
                    && line.trim() == "let bytes = format!(\"[general]\\nstate_dir = {:?}\\n\", dir.path().join(\"state\"));";
                // Existing shared helper deliberately converts enum Debug to lower-case labels;
                // private TUI enum types cannot be changed in this task's owned paths.
                let normalized_enum = file.ends_with("render/mod.rs")
                    && line.trim() == "format!(\"{value:?}\").to_ascii_lowercase()";
                if !test_fixture && !normalized_enum {
                    failures.push(format!("{}:{}: {line}", file.display(), index + 1));
                }
            }
        }
    }
    Ok(())
}

fn debug_field(line: &str) -> bool {
    line.split('{').skip(1).any(|field| {
        field.split_once('}').is_some_and(|(field, _)| {
            field
                .split_once(':')
                .is_some_and(|(_, spec)| spec.contains('?'))
        })
    })
}

#[test]
fn human_output_does_not_interpolate_debug_values() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut failures = Vec::new();
    scan(&root.join("render"), &mut failures).unwrap();
    scan(&root.join("cmd"), &mut failures).unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    for field in ["{:?}", "{value:?}", "{value:#?}", "{value:>10?}"] {
        assert!(debug_field(field));
    }
    assert!(!debug_field("{value:.2}%"));
}
