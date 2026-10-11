//! Owned by: W5-C
//! Synchronous, bounded and read-only PostgreSQL metadata collection.
use crate::{labels, queries};
use std::{
    collections::BTreeMap,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    time::Duration,
};
use tidemark_core::{
    config::PostgresConfig,
    live::{PgNow, PgSample, PgSlot},
    path::RawPath,
};
/// PostgreSQL collector contract.
pub struct PgProbe {
    /// Contract field.
    pub reachable: bool,
    /// Contract field.
    pub server_version_num: Option<u32>,
    /// Contract field.
    pub in_recovery: Option<bool>,
    /// Contract field.
    pub data_directory: Option<String>,
    /// Contract field.
    pub missing_privileges: Vec<String>,
    /// Contract field.
    pub error: Option<String>,
}
/// PostgreSQL collector contract.
pub fn probe_from_config(cfg: &PostgresConfig) -> Result<PgProbe, PgError> {
    probe_with_connector(cfg, &RealConnector(cfg.clone()))
}
fn probe_with_connector(
    cfg: &PostgresConfig,
    connector: &dyn PgConnector,
) -> Result<PgProbe, PgError> {
    let mut probe = PgProbe {
        reachable: false,
        server_version_num: None,
        in_recovery: None,
        data_directory: None,
        missing_privileges: Vec::new(),
        error: None,
    };
    let mut conn = match connector.connect("") {
        Ok(c) => c,
        Err(e) => {
            probe.error = Some(e.to_string());
            return Ok(probe);
        }
    };
    probe.reachable = true;
    initialize(&mut *conn, cfg.statement_timeout)?;
    match server_info(&mut *conn) {
        Ok((v, r)) => {
            probe.server_version_num = Some(v);
            probe.in_recovery = Some(r);
            if v < 100000 {
                probe.error = Some(PgError::Unsupported.to_string());
                return Ok(probe);
            }
        }
        Err(e) => {
            probe.error = Some(e.to_string());
            return Ok(probe);
        }
    }
    for (name, sql) in [
        ("pg_ls_waldir()", queries::WAL),
        ("pg_database_size", queries::PROBE_DATABASE),
        ("data_directory", queries::DATA),
    ] {
        match conn.query(sql, &[]) {
            Ok(rows) if name == "data_directory" => {
                probe.data_directory = Some(text(first(&rows)?, 0)?.into());
            }
            Ok(_) => {}
            Err(_) => probe.missing_privileges.push(name.into()),
        }
    }
    if !probe.missing_privileges.is_empty() {
        probe.error = Some("Missing monitoring privileges; GRANT pg_monitor TO tidemark;".into());
    }
    Ok(probe)
}

/// PostgreSQL collector contract.
pub type PgRow = Vec<Option<String>>;
/// PostgreSQL collector contract.
pub trait PgConn {
    /// PostgreSQL collector contract.
    fn query(&mut self, sql: &str, params: &[&str]) -> Result<Vec<PgRow>, PgError>;
}
/// PostgreSQL collector contract.
pub trait PgConnector: Send {
    /// PostgreSQL collector contract.
    fn connect(&self, dbname: &str) -> Result<Box<dyn PgConn>, PgError>;
}
/// Collector state; a failed pass never publishes partial values.
pub struct PgCollector {
    cfg: PostgresConfig,
    connector: Box<dyn PgConnector>,
    latest: Option<PgNow>,
    failures: u32,
    backoff: Option<i64>,
    unreachable: Option<i64>,
    temp: Option<(i64, BTreeMap<String, i64>)>,
}
impl PgCollector {
    /// Construct the synchronous PostgreSQL connector.
    pub fn new(cfg: PostgresConfig) -> Self {
        Self::with_connector(cfg.clone(), Box::new(RealConnector(cfg)))
    }
    /// Inject a connector for deterministic tests.
    pub fn with_connector(cfg: PostgresConfig, c: Box<dyn PgConnector>) -> Self {
        Self {
            cfg,
            connector: c,
            latest: None,
            failures: 0,
            backoff: None,
            unreachable: None,
            temp: None,
        }
    }
    fn ready(&self, now: i64) -> Result<(), PgError> {
        if self.backoff.is_some_and(|t| now < t) {
            Err(PgError::Backoff)
        } else {
            Ok(())
        }
    }
    fn finish<T>(&mut self, now: i64, result: Result<T, PgError>) -> Result<T, PgError> {
        if result.is_err() {
            let minutes = (1u32 << self.failures.min(5)).min(30);
            self.failures = self.failures.saturating_add(1);
            self.backoff = Some(now.saturating_add(i64::from(minutes) * 60));
            self.unreachable.get_or_insert(now);
            let latest = self.latest.get_or_insert_with(empty_state);
            latest.unreachable_since = self.unreachable;
        } else {
            self.failures = 0;
            self.backoff = None;
            self.unreachable = None;
            if let Some(latest) = &mut self.latest {
                latest.unreachable_since = None;
            }
        }
        result
    }
    /// Collect the five-minute metadata set atomically.
    pub fn collect_fast(&mut self, now: i64) -> Result<Vec<PgSample>, PgError> {
        self.ready(now)?;
        let result = self.fast(now);
        self.finish(now, result)
    }
    fn fast(&mut self, now: i64) -> Result<Vec<PgSample>, PgError> {
        let mut conn = self.connector.connect("")?;
        initialize(&mut *conn, self.cfg.statement_timeout)?;
        let (v, recovery) = version(&mut *conn)?;
        let mut state = PgNow {
            sampled_at: Some(now),
            unreachable_since: None,
            server_version_num: Some(v),
            databases: Vec::new(),
            wal_dir_bytes: None,
            slots: Vec::new(),
            archiver_failing: false,
            archiver_last_failed: None,
            temp_bytes_rate: None,
            top_relations: self
                .latest
                .as_ref()
                .map(|p| p.top_relations.clone())
                .unwrap_or_default(),
        };
        let mut samples = Vec::new();
        for row in conn.query(queries::DATABASES, &[])? {
            let name = text(&row, 0)?.to_owned();
            let size = number(&row, 1)?;
            state.databases.push((name.clone(), size as u64));
            samples.push(sample(now, "database", name, size));
        }
        state
            .databases
            .sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let wal = conn.query(queries::WAL, &[])?;
        let size = number(first(&wal)?, 0)?;
        state.wal_dir_bytes = Some(size as u64);
        samples.push(sample(now, "wal_dir", String::new(), size));
        if let Some(sql) = queries::slots(v, recovery) {
            for row in conn.query(sql, &[])? {
                let retained = optional_number(&row, 3)?;
                let name = text(&row, 0)?.to_owned();
                if let Some(n) = retained {
                    samples.push(sample(now, "slot_retained", name.clone(), n));
                }
                state.slots.push(PgSlot {
                    name,
                    slot_type: text(&row, 1)?.into(),
                    active: boolean(text(&row, 2)?)?,
                    retained_bytes: retained.map(|n| n as u64),
                    wal_status: if v >= 130000 {
                        row.get(4).ok_or(PgError::Decode)?.clone()
                    } else {
                        None
                    },
                });
            }
        }
        let archive = conn.query(queries::ARCHIVER, &[])?;
        let row = first(&archive)?;
        let failed = number(row, 1)?;
        state.archiver_last_failed = optional_timestamp(row, 2)?;
        let archived = optional_timestamp(row, 3)?;
        state.archiver_failing = state
            .archiver_last_failed
            .is_some_and(|f| archived.is_none_or(|a| f > a));
        samples.push(sample(now, "archiver_failed_total", String::new(), failed));
        let mut temp = BTreeMap::new();
        for row in conn.query(queries::TEMP, &[])? {
            let name = text(&row, 0)?.to_owned();
            let bytes = number(&row, 1)?;
            samples.push(sample(now, "temp_bytes_total", name.clone(), bytes));
            temp.insert(name, bytes);
        }
        state.temp_bytes_rate = self
            .temp
            .as_ref()
            .and_then(|previous| counter_rate(previous, &temp, now));
        self.temp = Some((now, temp));
        self.latest = Some(state);
        Ok(samples)
    }
    /// Collect each database sequentially with a thirty-second maximum timeout.
    pub fn collect_relations(&mut self, now: i64) -> Result<Vec<PgSample>, PgError> {
        self.ready(now)?;
        let result = self.relations(now, None);
        self.finish(now, result)
    }
    /// Collect relation metadata only for the selected database (CLI filter).
    pub fn collect_relations_for_database(
        &mut self,
        now: i64,
        database: &str,
    ) -> Result<Vec<PgSample>, PgError> {
        self.ready(now)?;
        let result = self.relations(now, Some(database));
        self.finish(now, result)
    }
    fn relations(&mut self, now: i64, database: Option<&str>) -> Result<Vec<PgSample>, PgError> {
        let databases = {
            let mut c = self.connector.connect("")?;
            initialize(&mut *c, self.cfg.statement_timeout)?;
            version(&mut *c)?;
            c.query(queries::CONNECT_DATABASES, &[])?
        };
        let mut samples = Vec::new();
        let mut relations = Vec::new();
        for row in databases {
            let db = text(&row, 1)?;
            if database.is_some_and(|selected| selected != db) {
                continue;
            }
            let mut c = self.connector.connect(db)?;
            initialize(
                &mut *c,
                self.cfg.statement_timeout.min(Duration::from_secs(30)),
            )?;
            for row in c.query(&queries::relations(self.cfg.top_relations), &[])? {
                let name = format!("{db}.{}.{}", text(&row, 0)?, text(&row, 1)?);
                let size = number(&row, 3)?;
                if samples.len() >= 10000 {
                    return Err(PgError::Limit);
                }
                relations.push((name.clone(), size as u64));
                samples.push(sample(now, "relation", name, size));
            }
            for row in c.query(queries::DEAD, &[])? {
                let name = format!("{db}.{}.{}", text(&row, 0)?, text(&row, 1)?);
                if samples.len() >= 10000 {
                    return Err(PgError::Limit);
                }
                samples.push(sample(now, "dead_tuples", name, number(&row, 3)?));
            }
        }
        relations.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        relations.truncate(self.cfg.top_relations.min(1000));
        if let Some(latest) = &mut self.latest {
            latest.top_relations = relations;
        }
        Ok(samples)
    }
    /// Fetch lexical PGDATA, database, WAL and tablespace labels.
    pub fn path_labels(&mut self) -> Result<Vec<(RawPath, String, String)>, PgError> {
        let mut c = self.connector.connect("")?;
        initialize(&mut *c, self.cfg.statement_timeout)?;
        version(&mut *c)?;
        let data = c.query(queries::DATA, &[])?;
        labels::decode(
            text(first(&data)?, 0)?,
            &c.query(queries::DATABASE_LABELS, &[])?,
            &c.query(queries::TABLESPACES, &[])?,
        )
    }
    /// Last complete fast pass, retained across failures.
    pub fn latest(&self) -> Option<PgNow> {
        self.latest.clone()
    }
    /// Earliest retry time after connection or query failure.
    pub fn backoff_until(&self) -> Option<i64> {
        self.backoff
    }
}
fn counter_rate(
    previous: &(i64, BTreeMap<String, i64>),
    current: &BTreeMap<String, i64>,
    now: i64,
) -> Option<f64> {
    let (then, old) = previous;
    if now <= *then {
        return None;
    }
    let delta: f64 = current
        .iter()
        .filter_map(|(name, value)| {
            old.get(name)
                .filter(|old| value >= *old)
                .map(|old| (value - old) as f64)
        })
        .sum();
    Some(delta / (i128::from(now) - i128::from(*then)) as f64)
}
fn empty_state() -> PgNow {
    PgNow {
        sampled_at: None,
        unreachable_since: None,
        server_version_num: None,
        databases: Vec::new(),
        wal_dir_bytes: None,
        slots: Vec::new(),
        archiver_failing: false,
        archiver_last_failed: None,
        temp_bytes_rate: None,
        top_relations: Vec::new(),
    }
}
fn sample(ts: i64, kind: &str, name: String, value: i64) -> PgSample {
    PgSample {
        ts,
        kind: kind.into(),
        name,
        value,
    }
}
pub(crate) fn text(row: &PgRow, index: usize) -> Result<&str, PgError> {
    row.get(index)
        .and_then(|s| s.as_deref())
        .ok_or(PgError::Decode)
}
fn first(rows: &[PgRow]) -> Result<&PgRow, PgError> {
    rows.first().ok_or(PgError::Decode)
}
fn number(row: &PgRow, index: usize) -> Result<i64, PgError> {
    let n = text(row, index)?
        .parse::<i64>()
        .map_err(|_| PgError::Decode)?;
    if n < 0 {
        Err(PgError::Decode)
    } else {
        Ok(n)
    }
}
fn optional_number(row: &PgRow, index: usize) -> Result<Option<i64>, PgError> {
    if row.get(index).ok_or(PgError::Decode)?.is_none() {
        Ok(None)
    } else {
        number(row, index).map(Some)
    }
}
fn optional_timestamp(row: &PgRow, index: usize) -> Result<Option<i64>, PgError> {
    row.get(index)
        .ok_or(PgError::Decode)?
        .as_deref()
        .map(|value| value.parse().map_err(|_| PgError::Decode))
        .transpose()
}
fn boolean(s: &str) -> Result<bool, PgError> {
    match s {
        "true" | "t" => Ok(true),
        "false" | "f" => Ok(false),
        _ => Err(PgError::Decode),
    }
}
fn version(c: &mut dyn PgConn) -> Result<(u32, bool), PgError> {
    let (v, recovery) = server_info(c)?;
    if v < 100000 {
        return Err(PgError::Unsupported);
    }
    Ok((v, recovery))
}
fn server_info(c: &mut dyn PgConn) -> Result<(u32, bool), PgError> {
    let rows = c.query(queries::VERSION, &[])?;
    let row = first(&rows)?;
    let v = text(row, 0)?.parse().map_err(|_| PgError::Decode)?;
    Ok((v, boolean(text(row, 1)?)?))
}
fn initialize(c: &mut dyn PgConn, timeout: Duration) -> Result<(), PgError> {
    for sql in [
        "SET default_transaction_read_only = on".to_owned(),
        "SET application_name = 'tidemark'".into(),
        format!(
            "SET statement_timeout = '{}'",
            timeout.as_millis().clamp(1, i32::MAX as u128)
        ),
        "SET lock_timeout = '1s'".into(),
    ] {
        c.query(&sql, &[])?;
    }
    Ok(())
}
struct RealConnector(PostgresConfig);
struct RealConn(postgres::Client);
impl PgConnector for RealConnector {
    fn connect(&self, dbname: &str) -> Result<Box<dyn PgConn>, PgError> {
        let (dsn, passfile) = password_dsn(&self.0.dsn)?;
        let mut config: postgres::Config = dsn.parse().map_err(|_| PgError::Connection)?;
        if !dbname.is_empty() {
            config.dbname(dbname);
        }
        config.connect_timeout(Duration::from_secs(5));
        // Startup options protect the session before its first SQL statement.
        config.options(
            "-c default_transaction_read_only=on -c lock_timeout=1000 -c statement_timeout=30000",
        );
        if config.get_password().is_some() {
            return Err(PgError::PasswordPermissions);
        }
        if let Some(path) = passfile {
            let bytes = password_file(&path)?;
            config.password(select_password(&bytes, &config)?);
        }
        let client = config
            .connect(postgres::NoTls)
            .map_err(|_| PgError::Connection)?;
        let mut conn = RealConn(client);
        initialize(&mut conn, self.0.statement_timeout)?;
        Ok(Box::new(conn))
    }
}
// Extract libpq's passfile keyword before handing the remaining DSN to the Rust client.
// Preserve original quoting for all other options, and never report DSN fragments in errors.
fn password_dsn(dsn: &str) -> Result<(String, Option<std::path::PathBuf>), PgError> {
    if dsn.starts_with("postgres://") || dsn.starts_with("postgresql://") {
        return Ok((dsn.to_owned(), None));
    }
    let bytes = dsn.as_bytes();
    let mut i = 0;
    let mut parts = Vec::new();
    let mut passfile = None;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i == bytes.len() {
            break;
        }
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        let key = &dsn[start..i];
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if key.is_empty() || bytes.get(i) != Some(&b'=') {
            return Err(PgError::Credentials);
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let quoted = bytes.get(i) == Some(&b'\'');
        if quoted {
            i += 1;
        }
        let mut value = Vec::new();
        let mut closed = !quoted;
        while i < bytes.len() {
            let byte = bytes[i];
            if quoted && byte == b'\'' {
                i += 1;
                closed = true;
                break;
            }
            if !quoted && byte.is_ascii_whitespace() {
                break;
            }
            i += 1;
            if byte == b'\\' {
                value.push(*bytes.get(i).ok_or(PgError::Credentials)?);
                i += 1;
            } else {
                value.push(byte);
            }
        }
        if !closed || bytes.get(i).is_some_and(|b| !b.is_ascii_whitespace()) {
            return Err(PgError::Credentials);
        }
        if key == "passfile" {
            if passfile.is_some() {
                return Err(PgError::Credentials);
            }
            let path = std::path::PathBuf::from(
                String::from_utf8(value).map_err(|_| PgError::Credentials)?,
            );
            if !path.is_absolute() {
                return Err(PgError::Credentials);
            }
            passfile = Some(path);
        } else {
            parts.push(&dsn[start..i]);
        }
    }
    Ok((parts.join(" "), passfile))
}
fn select_password(bytes: &[u8], config: &postgres::Config) -> Result<Vec<u8>, PgError> {
    // Accept a raw one-line password as well as libpq's five-field pgpass records.
    if !bytes.contains(&b':') && !bytes.contains(&b'\n') && !bytes.contains(&b'\r') {
        return Ok(bytes.to_vec());
    }
    if config.get_hosts().len() > 1 || config.get_ports().len() > 1 {
        return Err(PgError::Credentials);
    }
    let host = match config.get_hosts().first() {
        Some(postgres::config::Host::Tcp(host)) => host.as_str(),
        Some(postgres::config::Host::Unix(_)) | None => "localhost",
    };
    let port = config
        .get_ports()
        .first()
        .copied()
        .unwrap_or(5432)
        .to_string();
    let user = config.get_user().ok_or(PgError::Credentials)?;
    let db = config.get_dbname().unwrap_or(user);
    for line in bytes.split(|b| *b == b'\n') {
        if line.is_empty() || line.first() == Some(&b'#') {
            continue;
        }
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let mut fields = vec![Vec::new()];
        let mut escaped = false;
        for &byte in line {
            if escaped {
                fields.last_mut().ok_or(PgError::Credentials)?.push(byte);
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b':' {
                if fields.len() == 5 {
                    return Err(PgError::Credentials);
                }
                fields.push(Vec::new());
            } else {
                fields.last_mut().ok_or(PgError::Credentials)?.push(byte);
            }
        }
        if escaped || fields.len() != 5 {
            return Err(PgError::Credentials);
        }
        if fields[..4]
            .iter()
            .zip([host, &port, db, user])
            .all(|(field, value)| field == b"*" || field == value.as_bytes())
        {
            return Ok(fields[4].clone());
        }
    }
    Err(PgError::Credentials)
}
fn password_file(path: &std::path::Path) -> Result<Vec<u8>, PgError> {
    // Linux O_NOFOLLOW | O_NONBLOCK: refuse symlinks and inspect the opened descriptor to avoid a metadata/open race.
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(0x20000 | 0x800)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.uid() != 0 || metadata.mode() & 0o777 != 0o600 {
        return Err(PgError::PasswordPermissions);
    }
    let mut bytes = Vec::new();
    file.by_ref().take(4097).read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err(PgError::PasswordPermissions);
    }
    while matches!(bytes.last(), Some(b'\n' | b'\r')) {
        bytes.pop();
    }
    Ok(bytes)
}
impl PgConn for RealConn {
    fn query(&mut self, sql: &str, params: &[&str]) -> Result<Vec<PgRow>, PgError> {
        let mut rows = Vec::new();
        // Row streaming bounds client memory even if the catalog is unexpectedly large.
        let iter = self
            .0
            .query_raw(sql, params.iter().copied())
            .map_err(|_| PgError::Query)?;
        use postgres::fallible_iterator::FallibleIterator;
        let mut iter = iter;
        while let Some(row) = iter.next().map_err(|_| PgError::Query)? {
            if rows.len() >= 10000 {
                return Err(PgError::Limit);
            }
            let mut columns = Vec::with_capacity(row.len());
            for i in 0..row.len() {
                columns.push(
                    row.try_get::<_, Option<String>>(i)
                        .map_err(|_| PgError::Decode)?,
                );
            }
            rows.push(columns);
        }
        Ok(rows)
    }
}
/// Sanitized collector failures; connection errors never expose DSN secrets.
#[derive(Debug, thiserror::Error)]
pub enum PgError {
    /// Retained compatibility variant for preparation callers.
    #[error("NotYetImplemented")]
    NotYetImplemented,
    /// Connection failed.
    #[error("PostgreSQL connection failed")]
    Connection,
    /// Query failed.
    #[error("PostgreSQL metadata query failed")]
    Query,
    /// Invalid or overflowing text row.
    #[error("invalid PostgreSQL metadata row")]
    Decode,
    /// PostgreSQL before version 10.
    #[error("PostgreSQL versions before 10 are unsupported")]
    Unsupported,
    /// Retry deferred.
    #[error("PostgreSQL collection is backing off")]
    Backoff,
    /// Catalog cap exceeded.
    #[error("PostgreSQL catalog exceeds 10000 rows")]
    Limit,
    /// Credential file rejected.
    #[error("password file must be a root-owned regular file with mode 0600, at most 4096 bytes")]
    PasswordPermissions,
    /// Malformed credential configuration or no matching pgpass record.
    #[error("invalid PostgreSQL credential configuration (use an absolute passfile path and explicit user)")]
    Credentials,
    /// File I/O error without credential contents.
    #[error("password file I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    struct Fake {
        version: u32,
        recovery: bool,
        fail: Arc<Mutex<bool>>,
        log: Arc<Mutex<Vec<String>>>,
    }
    struct Conn {
        version: u32,
        recovery: bool,
        fail: Arc<Mutex<bool>>,
        log: Arc<Mutex<Vec<String>>>,
    }
    fn row(values: &[&str]) -> PgRow {
        values.iter().map(|v| Some((*v).into())).collect()
    }
    impl PgConnector for Fake {
        fn connect(&self, db: &str) -> Result<Box<dyn PgConn>, PgError> {
            self.log.lock().unwrap().push(format!("CONNECT {db}"));
            if *self.fail.lock().unwrap() {
                return Err(PgError::Connection);
            }
            Ok(Box::new(Conn {
                version: self.version,
                recovery: self.recovery,
                fail: self.fail.clone(),
                log: self.log.clone(),
            }))
        }
    }
    impl PgConn for Conn {
        fn query(&mut self, sql: &str, _: &[&str]) -> Result<Vec<PgRow>, PgError> {
            self.log.lock().unwrap().push(sql.into());
            if *self.fail.lock().unwrap() {
                return Err(PgError::Query);
            }
            Ok(if sql.starts_with("SET ") {
                vec![]
            } else if sql == queries::VERSION {
                vec![row(&[
                    &self.version.to_string(),
                    if self.recovery { "true" } else { "false" },
                ])]
            } else if sql == queries::PROBE_DATABASE {
                vec![row(&["123"])]
            } else if sql == queries::DATABASES {
                vec![row(&["db", "123"])]
            } else if sql == queries::WAL {
                vec![row(&["456"])]
            } else if sql == queries::ARCHIVER {
                vec![vec![
                    Some("10".into()),
                    Some("2".into()),
                    Some("50".into()),
                    None,
                ]]
            } else if sql == queries::TEMP {
                vec![row(&["db", "100", "1"])]
            } else if sql == queries::DATA {
                vec![row(&["/pg"])]
            } else if sql == queries::DATABASE_LABELS || sql == queries::CONNECT_DATABASES {
                vec![row(&["42", "db"])]
            } else if sql == queries::TABLESPACES {
                vec![row(&["space", "/space"])]
            } else if sql == queries::DEAD {
                vec![row(&["public", "table", "10", "3", "0"])]
            } else if sql.starts_with("SELECT n.nspname") {
                vec![row(&["public", "table", "r", "20"])]
            } else if sql.contains("pg_replication_slots") {
                let mut r = row(&["slot", "physical", "false", "0"]);
                if self.version >= 130000 {
                    r.push(Some("reserved".into()));
                }
                vec![r]
            } else {
                return Err(PgError::Query);
            })
        }
    }
    type FakeState = (PgCollector, Arc<Mutex<bool>>, Arc<Mutex<Vec<String>>>);
    fn collector(v: u32, r: bool) -> FakeState {
        let fail = Arc::new(Mutex::new(false));
        let log = Arc::new(Mutex::new(Vec::new()));
        (
            PgCollector::with_connector(
                PostgresConfig::default(),
                Box::new(Fake {
                    version: v,
                    recovery: r,
                    fail: fail.clone(),
                    log: log.clone(),
                }),
            ),
            fail,
            log,
        )
    }
    #[test]
    fn versions_decode_and_guard_sessions() {
        for v in [100000, 120000, 130000, 160000] {
            let (mut c, _, log) = collector(v, false);
            assert_eq!(c.collect_fast(10).unwrap().len(), 5);
            let latest = c.latest().unwrap();
            assert_eq!(latest.wal_dir_bytes, Some(456));
            assert_eq!(
                latest.slots[0].wal_status,
                if v >= 130000 {
                    Some("reserved".into())
                } else {
                    None
                }
            );
            assert!(latest.archiver_failing);
            assert_eq!(latest.databases, vec![("db".into(), 123)]);
            assert!(log
                .lock()
                .unwrap()
                .iter()
                .any(|s| s == "SET default_transaction_read_only = on"));
            c.collect_fast(20).unwrap();
            assert_eq!(c.latest().unwrap().temp_bytes_rate, Some(0.0));
            assert_eq!(c.path_labels().unwrap().len(), 4);
        }
    }
    #[test]
    fn standby_skips_slots_and_old_versions_fail() {
        let (mut c, _, log) = collector(160000, true);
        c.collect_fast(0).unwrap();
        assert!(c.latest().unwrap().slots.is_empty());
        assert!(!log
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.contains("pg_replication_slots")));
        let (mut c, _, _) = collector(90600, false);
        assert!(matches!(c.collect_fast(0), Err(PgError::Unsupported)));
    }
    #[test]
    fn backoff_retains_last_good_and_recovers() {
        let (mut c, fail, log) = collector(160000, false);
        c.collect_fast(0).unwrap();
        *fail.lock().unwrap() = true;
        let mut now = 10;
        for delay in [60, 120, 240, 480, 960, 1800, 1800] {
            assert!(c.collect_fast(now).is_err());
            assert_eq!(c.backoff_until(), Some(now + delay));
            assert_eq!(c.latest().unwrap().unreachable_since, Some(10));
            let n = log.lock().unwrap().len();
            assert!(matches!(c.collect_fast(now + 1), Err(PgError::Backoff)));
            assert_eq!(log.lock().unwrap().len(), n);
            now += delay;
        }
        *fail.lock().unwrap() = false;
        c.collect_fast(now).unwrap();
        assert_eq!(c.latest().unwrap().unreachable_since, None);
        assert_eq!(c.backoff_until(), None);
    }
    #[test]
    fn relations_are_qualified_and_sequential() {
        let (mut c, _, log) = collector(160000, false);
        c.collect_fast(0).unwrap();
        let samples = c.collect_relations(1).unwrap();
        assert_eq!(samples[0].name, "db.public.table");
        assert_eq!(samples[1].kind, "dead_tuples");
        assert_eq!(
            c.latest().unwrap().top_relations,
            vec![("db.public.table".into(), 20)]
        );
        assert!(log.lock().unwrap().iter().any(|s| s == "CONNECT db"));
    }
    struct FailAtConnector {
        inner: Fake,
        sql: Arc<Mutex<Option<&'static str>>>,
    }
    struct FailAtConn {
        inner: Box<dyn PgConn>,
        sql: Arc<Mutex<Option<&'static str>>>,
    }
    impl PgConnector for FailAtConnector {
        fn connect(&self, db: &str) -> Result<Box<dyn PgConn>, PgError> {
            Ok(Box::new(FailAtConn {
                inner: self.inner.connect(db)?,
                sql: self.sql.clone(),
            }))
        }
    }
    impl PgConn for FailAtConn {
        fn query(&mut self, sql: &str, params: &[&str]) -> Result<Vec<PgRow>, PgError> {
            if self.sql.lock().unwrap().is_some_and(|s| s == sql) {
                return Err(PgError::Query);
            }
            self.inner.query(sql, params)
        }
    }
    #[test]
    fn query_failure_publishes_no_partial_pass() {
        let sql = Arc::new(Mutex::new(None));
        let fake = Fake {
            version: 160000,
            recovery: false,
            fail: Arc::new(Mutex::new(false)),
            log: Arc::new(Mutex::new(Vec::new())),
        };
        let mut c = PgCollector::with_connector(
            PostgresConfig::default(),
            Box::new(FailAtConnector {
                inner: fake,
                sql: sql.clone(),
            }),
        );
        c.collect_fast(0).unwrap();
        c.collect_relations(0).unwrap();
        let mut previous = c.latest().unwrap();
        previous.unreachable_since = Some(10);
        *sql.lock().unwrap() = Some(queries::ARCHIVER);
        assert!(matches!(c.collect_fast(10), Err(PgError::Query)));
        assert_eq!(c.latest().unwrap(), previous);
        *sql.lock().unwrap() = Some(queries::DEAD);
        assert!(matches!(c.collect_relations(70), Err(PgError::Query)));
        assert_eq!(c.latest().unwrap(), previous);
        *sql.lock().unwrap() = None;
        c.collect_fast(190).unwrap();
        assert_eq!(c.latest().unwrap().temp_bytes_rate, Some(0.0));
    }
    #[test]
    fn first_failure_has_unreachable_state_and_selection_skips_other_databases() {
        let (mut c, fail, _) = collector(160000, false);
        *fail.lock().unwrap() = true;
        assert!(c.collect_fast(1).is_err());
        let state = c.latest().unwrap();
        assert_eq!(state.sampled_at, None);
        assert_eq!(state.unreachable_since, Some(1));
        *fail.lock().unwrap() = false;
        assert!(c
            .collect_relations_for_database(61, "other")
            .unwrap()
            .is_empty());
    }
    #[test]
    fn counter_rates_handle_growth_resets_new_databases_and_clock_edges() {
        let old = BTreeMap::from([("db".into(), 100), ("reset".into(), 1000)]);
        let current = BTreeMap::from([
            ("db".into(), 200),
            ("reset".into(), 1),
            ("new".into(), 5000),
        ]);
        assert_eq!(counter_rate(&(10, old.clone()), &current, 20), Some(10.0));
        assert_eq!(counter_rate(&(10, old.clone()), &current, 10), None);
        assert_eq!(counter_rate(&(10, old.clone()), &current, 9), None);
        assert_eq!(
            counter_rate(&(i64::MAX - 1, old), &current, i64::MAX),
            Some(100.0)
        );
    }
    #[test]
    fn malformed_rows_are_errors() {
        for r in [
            vec![],
            vec![None],
            row(&["-1"]),
            row(&["9223372036854775808"]),
            row(&["NaN"]),
        ] {
            assert!(number(&r, 0).is_err());
        }
        assert!(optional_number(&vec![], 0).is_err());
        assert_eq!(optional_number(&vec![None], 0).unwrap(), None);
        assert!(boolean("yes").is_err());
        assert_eq!(optional_timestamp(&row(&["-1"]), 0).unwrap(), Some(-1));
    }
    #[test]
    fn configured_password_paths_and_pgpass_matching() {
        let (dsn, path) =
            password_dsn("host=/socket user=tidemark passfile='/private/pg pass' dbname=main")
                .unwrap();
        assert_eq!(path.unwrap(), std::path::PathBuf::from("/private/pg pass"));
        assert_eq!(dsn, "host=/socket user=tidemark dbname=main");
        let config: postgres::Config = dsn.parse().unwrap();
        assert_eq!(
            select_password(b"raw-password", &config).unwrap(),
            b"raw-password"
        );
        assert_eq!(
            select_password(
                b"# ignored\nlocalhost:5432:main:tidemark:secret\\:with\\\\slash\n",
                &config
            )
            .unwrap(),
            b"secret:with\\slash"
        );
        assert_eq!(
            select_password(b"*:5432:*:tidemark:wildcard", &config).unwrap(),
            b"wildcard"
        );
        assert!(select_password(b"localhost:5432:other:tidemark:secret", &config).is_err());
        for dsn in [
            "passfile=relative",
            "passfile='/private/unclosed",
            "passfile=/a passfile=/b",
            "user=tidemark passfile='/a'junk",
        ] {
            assert!(password_dsn(dsn).is_err());
        }
        assert_eq!(
            password_dsn("postgresql://localhost/main").unwrap().0,
            "postgresql://localhost/main"
        );
        let (dsn, path) = password_dsn("passfile='/a\\'b' user=tidemark").unwrap();
        assert_eq!(dsn, "user=tidemark");
        assert_eq!(path.unwrap(), std::path::PathBuf::from("/a'b"));
    }
    #[test]
    fn probe_reports_versions_missing_privileges_and_unreachable() {
        let sql = Arc::new(Mutex::new(Some(queries::DATA)));
        let fake = Fake {
            version: 160000,
            recovery: false,
            fail: Arc::new(Mutex::new(false)),
            log: Arc::new(Mutex::new(Vec::new())),
        };
        let connector = FailAtConnector { inner: fake, sql };
        let probe = probe_with_connector(&PostgresConfig::default(), &connector).unwrap();
        assert!(probe.reachable);
        assert_eq!(probe.missing_privileges, vec!["data_directory"]);
        assert!(probe.error.unwrap().contains("GRANT pg_monitor"));
        let old = Fake {
            version: 90600,
            recovery: false,
            fail: Arc::new(Mutex::new(false)),
            log: Arc::new(Mutex::new(Vec::new())),
        };
        let probe = probe_with_connector(&PostgresConfig::default(), &old).unwrap();
        assert_eq!(probe.server_version_num, Some(90600));
        assert!(probe.error.unwrap().contains("unsupported"));
        *old.fail.lock().unwrap() = true;
        let probe = probe_with_connector(&PostgresConfig::default(), &old).unwrap();
        assert!(!probe.reachable);
    }
    #[test]
    fn password_rejects_wide_permissions_and_symlink() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("password");
        std::fs::write(&path, b"secret").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            password_file(&path),
            Err(PgError::PasswordPermissions)
        ));
        let link = dir.path().join("link");
        symlink(&path, &link).unwrap();
        assert!(password_file(&link).is_err());
    }
}
