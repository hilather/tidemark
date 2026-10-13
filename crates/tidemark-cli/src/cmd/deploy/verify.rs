//! Owned by: W4-B
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Poll until healthy or this duration expires.
    #[arg(long, value_name = "DUR", value_parser = humantime::parse_duration)]
    pub wait: Option<std::time::Duration>,
}

/// Verify the service, staged report and API status, returning exit 6 until healthy.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    let api = crate::deploy::SocketPing {
        socket: context.config.general.socket.clone(),
        timeout: std::time::Duration::from_secs(10),
    };
    let mut result = crate::deploy::verify(
        &context.config.general.state_dir,
        args.wait.unwrap_or_default(),
        &crate::deploy::RealSystemctl,
        &api,
    )?;
    let detail = hash_detail(result.staged.as_ref(), result.firstboot.as_ref());
    if let Some(item) = result
        .items
        .iter_mut()
        .find(|item| item.name == "config_hash")
    {
        item.detail = detail;
    }
    super::render::verified(&result, context.json)?;
    if result.healthy {
        Ok(())
    } else {
        Err(crate::CliError::Unhealthy("deployment not healthy".into()).into())
    }
}

fn hash_detail(
    staged: Option<&serde_json::Value>,
    firstboot: Option<&serde_json::Value>,
) -> String {
    fn hash(record: Option<&serde_json::Value>) -> Option<&str> {
        record
            .and_then(|v| v.get("config_sha256"))
            .and_then(|v| v.as_str())
            .filter(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
    }
    let expected = hash(staged);
    let actual = hash(firstboot);
    fn label(value: Option<&str>) -> &str {
        value.map_or("missing", |v| &v[..12])
    }
    let state = match (expected, actual) {
        (Some(a), Some(b)) if a == b => "report matches staged config",
        (Some(_), Some(_)) => "firstboot stale relative to staged config",
        (None, Some(_)) => "staged hash missing or invalid; cannot determine freshness",
        (Some(_), None) => "firstboot hash missing or invalid",
        (None, None) => "staged and firstboot hashes missing or invalid",
    };
    format!(
        "{state}: staged={}, firstboot={}",
        label(expected),
        label(actual)
    )
}
#[cfg(test)]
mod tests {
    #[test]
    fn verify_hashes_show_both_prefixes_and_stale_side() {
        let staged = serde_json::json!({"config_sha256": "a".repeat(64)});
        let firstboot = serde_json::json!({"config_sha256": "b".repeat(64)});
        assert_eq!(super::hash_detail(Some(&staged), Some(&firstboot)), "firstboot stale relative to staged config: staged=aaaaaaaaaaaa, firstboot=bbbbbbbbbbbb");
        assert!(super::hash_detail(None, Some(&firstboot))
            .contains("staged=missing, firstboot=bbbbbbbbbbbb"));
        assert!(super::hash_detail(Some(&staged), None)
            .contains("staged=aaaaaaaaaaaa, firstboot=missing"));
        assert!(super::hash_detail(None, None).contains("staged=missing, firstboot=missing"));
        assert!(super::hash_detail(Some(&staged), Some(&staged)).contains("report matches"));
        assert!(
            super::hash_detail(Some(&serde_json::json!({"config_sha256":"garbage"})), None)
                .contains("staged=missing")
        );
    }
}
