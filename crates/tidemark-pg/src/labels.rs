//! Owned by: W5-C
//! Lexical path annotations without filesystem traversal.
use crate::collector::{PgError, PgRow};
use tidemark_core::path::RawPath;

/// Decode PostgreSQL paths into snapshot-compatible component paths.
pub fn decode(
    data: &str,
    databases: &[PgRow],
    tablespaces: &[PgRow],
) -> Result<Vec<(RawPath, String, String)>, PgError> {
    let root = RawPath::new(data.as_bytes()).map_err(|_| PgError::Decode)?;
    let mut result: Vec<(RawPath, String, String)> = vec![
        (root.clone(), "pg.data_directory".into(), "true".into()),
        (
            RawPath::new(format!("{data}/pg_wal").into_bytes()).map_err(|_| PgError::Decode)?,
            "pg.wal".into(),
            "true".into(),
        ),
    ];
    for row in databases {
        let oid = crate::collector::text(row, 0)?
            .parse::<u32>()
            .map_err(|_| PgError::Decode)?;
        result.push((
            RawPath::new(format!("{data}/base/{oid}").into_bytes()).map_err(|_| PgError::Decode)?,
            "pg.database".into(),
            crate::collector::text(row, 1)?.into(),
        ));
    }
    for row in tablespaces {
        let path = crate::collector::text(row, 1)?;
        if !path.is_empty() {
            result.push((
                RawPath::new(path.as_bytes()).map_err(|_| PgError::Decode)?,
                "pg.tablespace".into(),
                crate::collector::text(row, 0)?.into(),
            ));
        }
    }
    result.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    Ok(result)
}
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn maps_labels() {
        let rows = vec![vec![Some("42".into()), Some("db".into())]];
        let spaces = vec![vec![Some("extra".into()), Some("/disk/ts".into())]];
        let labels = decode("/pg", &rows, &spaces).unwrap();
        assert!(labels.iter().any(
            |(p, k, v)| p.as_path() == std::path::Path::new("/pg/base/42")
                && k == "pg.database"
                && v == "db"
        ));
        assert_eq!(labels.len(), 4);
        assert!(decode("relative", &[], &[]).is_err());
    }
}
