//! Owned by: TS-SIZE
//! Deterministic procscan sample load and timeseries.db size report.

use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Component, Path, PathBuf},
    time::Duration,
};
use tidemark_core::{
    config::SampleRetention,
    live::{PhantomSample, WriterSample},
};
use tidemark_store::timeseries::TimeSeries;

const USAGE: &str = "usage: cargo xtask ts-size --profile v1|ci --out DIR [--processes N] [--files-per-process N] [--interval-secs N] [--duration-secs N] [--seed N] [--churn-permille N] [--path-bytes N,N,...] [--mount UTF-8] [--start-ts N] [--json]";

/// Process ids start here so the synthetic pids stay clear of kernel threads.
const PID_BASE: u32 = 1000;
const DEV: u64 = 1;
const MAX_PROCESSES: u32 = 100_000;
const MAX_FILES_PER_PROCESS: u32 = 1_024;
const MAX_SLOTS: u32 = 100_000;
const MAX_PASSES: i64 = 20_000;
/// Worst-case phantom plus writer rows, including the reuse day.
const MAX_SAMPLE_ROWS: u64 = 2_000_000;
const MIN_PATH_BYTES: usize = 32;
const MAX_PATH_BYTES: usize = 512;
const DAY_SECS: i64 = 86_400;
#[cfg(test)]
const CI_BYTE_CEILING: i64 = 8 * 1024 * 1024;

/// Insert order matches `tidemarkd` procscan: one pass, phantom batches, then writer batches.
const INSERT_ORDER: &str = "per_pass_phantom_then_writers";
const MIX_DESC: &str = "splitmix64(seed ^ pass*0xD1B54A32D192ED03 ^ pid*0x94D049BB133111EB ^ slot*0xBF58476D1CE4E5B9); churn when mix % 1000 < churn_permille";
const PATH_DESC: &str = "{mount}/srv/{comm}.service/pid-{pid}/slot-{slot}/gen-{generation}/app.log, padded with ASCII a to path_bytes[slot % classes]; mount `/` has no extra slash";
const UNIT_DESC: &str = "process_index % 10 == 0 => NULL, otherwise {comm}.service";
const DISK_DESC: &str = "writer size and disk are 4096 * (pass - opened_at + 1); phantom bytes are the previous writer disk for that generation, or 0 when it was never written";

const COMMS: &[&str] = &[
    "postgres",
    "mysqld",
    "nginx",
    "java",
    "python3",
    "redis-server",
    "tidemarkd",
    "cargo",
    "rustc",
    "node",
    "dockerd",
    "containerd",
    "rsyslogd",
    "sshd",
    "cron",
];

/// Resolved load. Defaults come from `--profile`; explicit flags replace one field.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Profile {
    processes: u32,
    files_per_process: u32,
    interval_secs: i64,
    duration_secs: i64,
    seed: u64,
    churn_permille: u32,
    path_bytes: Vec<usize>,
    mount: String,
    start_ts: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct Parameters {
    profile: String,
    seed: u64,
    processes: u32,
    files_per_process: u32,
    interval_secs: i64,
    duration_secs: i64,
    passes: i64,
    start_ts: i64,
    mount: String,
    churn_permille: u32,
    path_bytes: Vec<usize>,
    pid_base: u32,
    dev: u64,
    comms: Vec<String>,
    unit_rule: &'static str,
    path_rule: &'static str,
    disk_rule: &'static str,
    mix: &'static str,
    mix_vector_seed_42_pass_0_pid_1000_slot_0: String,
    chunk_rows: usize,
    insert_order: &'static str,
    sort: &'static str,
    /// `TimeSeries::open` sets these on the writer. They are not reread from the file.
    writer_journal_mode: &'static str,
    writer_synchronous: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ObjectBytes {
    name: String,
    bytes: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct TableCount {
    name: String,
    rows: i64,
}

/// Main-file size after a truncate checkpoint. `shm_bytes` is recorded and not compared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct Measurement {
    sqlite_version: String,
    journal_mode: String,
    auto_vacuum: i64,
    page_size: i64,
    page_count: i64,
    freelist_count: i64,
    main_file_len: u64,
    wal_bytes: u64,
    shm_bytes: u64,
    checkpoint_busy: i64,
    checkpoint_log: i64,
    checkpoint_frames: i64,
    objects: Vec<ObjectBytes>,
    rows: Vec<TableCount>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RetentionPhase {
    now: i64,
    hot_minutes_secs: u64,
    cutoff_ts: i64,
    extra_passes: i64,
    compact_deleted: u64,
    compact_inserted: u64,
    after_delete: Measurement,
    after_reuse: Measurement,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct PathClassCount {
    bytes: usize,
    rows: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct Report {
    parameters: Parameters,
    path_length_rows: Vec<PathClassCount>,
    /// First-week append. No `compact` and no deletes, so freelist stays empty.
    append_only: Measurement,
    /// Deletes the oldest day (or half the window), then inserts that many passes again.
    retention: RetentionPhase,
}

struct Proc {
    pid: u32,
    comm: String,
    unit: Option<String>,
}

#[derive(Clone)]
struct Slot {
    generation: u32,
    opened_at: u32,
    last_disk: u64,
}

struct State {
    procs: Vec<Proc>,
    slots: Vec<Slot>,
}

struct PassRows {
    phantom: Vec<PhantomSample>,
    writers: Vec<WriterSample>,
}

struct Cli {
    out: PathBuf,
    profile: Profile,
    profile_name: String,
    json: bool,
}

/// Run the size harness and write `OUT/report.json`.
pub fn run(args: impl IntoIterator<Item = std::ffi::OsString>) -> Result<()> {
    let cli = parse_args(args)?;
    let db = prepare_output(&cli.out)?;
    let passes = cli.profile.passes()?;
    let extra = extra_passes(&cli.profile)?;
    eprintln!(
        "ts-size: profile {} processes {} files {} passes {} extra {} db {}",
        cli.profile_name,
        cli.profile.processes,
        cli.profile.files_per_process,
        passes,
        extra,
        db.display()
    );
    let report = execute(&cli.profile, &cli.profile_name, &db, true)?;
    write_report(&cli.out, &report)?;
    render(&report);
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    }
    Ok(())
}

fn execute(profile: &Profile, profile_name: &str, db: &Path, verbose: bool) -> Result<Report> {
    let passes = profile.passes()?;
    let extra = extra_passes(profile)?;
    let total = passes.checked_add(extra).context("pass count overflow")?;
    profile.ensure_budget(total)?;
    profile.ensure_paths(u32::try_from(total).context("pass count")?)?;
    let mut lengths = BTreeMap::new();
    let mut state = State::new(profile)?;
    {
        let ts = TimeSeries::open(db, true).context("open timeseries for append")?;
        drive(&ts, &mut state, profile, 0, passes, verbose, &mut lengths)?;
    }
    let append_only = measure(db).context("measure append-only database")?;
    let now = profile
        .start_ts
        .checked_add(profile.duration_secs)
        .context("compact clock overflow")?;
    let cutoff_ts = profile
        .start_ts
        .checked_add(
            extra
                .checked_mul(profile.interval_secs)
                .context("cutoff overflow")?,
        )
        .context("cutoff overflow")?;
    if cutoff_ts > now {
        bail!("retention cutoff {cutoff_ts} is after compact clock {now}");
    }
    let hot_minutes_secs = u64::try_from(now - cutoff_ts).context("hot window")?;
    let (compact_deleted, compact_inserted) = {
        let ts = TimeSeries::open(db, true).context("reopen for compact")?;
        let policy = SampleRetention {
            hot_minutes: Duration::from_secs(hot_minutes_secs),
            ..SampleRetention::default()
        };
        let stats = ts
            .compact(now, &policy)
            .context("compact synthetic samples")?;
        (stats.deleted, stats.inserted)
    };
    if compact_deleted == 0 {
        bail!("compact deleted nothing; retention section would not show page reuse");
    }
    let after_delete = measure(db).context("measure after compact")?;
    {
        let ts = TimeSeries::open(db, true).context("reopen for reuse inserts")?;
        drive(
            &ts,
            &mut state,
            profile,
            passes,
            total,
            verbose,
            &mut lengths,
        )?;
    }
    let after_reuse = measure(db).context("measure after reuse inserts")?;
    Ok(Report {
        parameters: parameters(profile, profile_name, passes)?,
        path_length_rows: lengths
            .into_iter()
            .map(|(bytes, rows)| PathClassCount { bytes, rows })
            .collect(),
        append_only,
        retention: RetentionPhase {
            now,
            hot_minutes_secs,
            cutoff_ts,
            extra_passes: extra,
            compact_deleted,
            compact_inserted,
            after_delete,
            after_reuse,
        },
    })
}

#[cfg(test)]
fn append_only(profile: &Profile, db: &Path) -> Result<Measurement> {
    let passes = profile.passes()?;
    profile.ensure_budget(passes)?;
    profile.ensure_paths(u32::try_from(passes).context("pass count")?)?;
    let mut lengths = BTreeMap::new();
    {
        let ts = TimeSeries::open(db, true).context("open timeseries")?;
        let mut state = State::new(profile)?;
        drive(&ts, &mut state, profile, 0, passes, false, &mut lengths)?;
    }
    measure(db)
}

fn drive(
    ts: &TimeSeries,
    state: &mut State,
    profile: &Profile,
    from: i64,
    to: i64,
    verbose: bool,
    lengths: &mut BTreeMap<usize, u64>,
) -> Result<()> {
    let chunk = tidemarkd::services::tswriter::MAX_BATCH_ROWS;
    for pass in from..to {
        if verbose && (pass == from || pass % 500 == 0 || pass + 1 == to) {
            eprintln!("ts-size: pass {pass}/{to}");
        }
        let pass_u = u32::try_from(pass).context("pass")?;
        let rows = state.emit(pass_u, profile)?;
        note_lengths(&rows, lengths);
        write_phantom_chunks(ts, &rows.phantom, chunk, pass)?;
        write_writer_chunks(ts, &rows.writers, chunk, pass)?;
    }
    Ok(())
}

fn note_lengths(rows: &PassRows, lengths: &mut BTreeMap<usize, u64>) {
    for row in &rows.phantom {
        *lengths.entry(row.path.len()).or_insert(0) += 1;
    }
    for row in &rows.writers {
        *lengths.entry(row.path.len()).or_insert(0) += 1;
    }
}

fn write_phantom_chunks(
    ts: &TimeSeries,
    rows: &[PhantomSample],
    chunk: usize,
    pass: i64,
) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    for part in rows.chunks(chunk) {
        ts.insert_phantom(part)
            .with_context(|| format!("insert phantom pass {pass}"))?;
    }
    Ok(())
}

fn write_writer_chunks(
    ts: &TimeSeries,
    rows: &[WriterSample],
    chunk: usize,
    pass: i64,
) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    for part in rows.chunks(chunk) {
        ts.insert_writers(part)
            .with_context(|| format!("insert writers pass {pass}"))?;
    }
    Ok(())
}

fn measure(db: &Path) -> Result<Measurement> {
    let (checkpoint, stats) = {
        let conn = open_measure(db)?;
        let checkpoint = checkpoint(&conn)?;
        let stats = read_stats(&conn)?;
        (checkpoint, stats)
    };
    let wal_bytes = file_len(&sibling(db, "-wal")?)?;
    let shm_bytes = file_len(&sibling(db, "-shm")?)?;
    let main_file_len = file_len(db)?;
    let measurement = Measurement {
        sqlite_version: stats.sqlite_version,
        journal_mode: stats.journal_mode,
        auto_vacuum: stats.auto_vacuum,
        page_size: stats.page_size,
        page_count: stats.page_count,
        freelist_count: stats.freelist_count,
        main_file_len,
        wal_bytes,
        shm_bytes,
        checkpoint_busy: checkpoint.0,
        checkpoint_log: checkpoint.1,
        checkpoint_frames: checkpoint.2,
        objects: stats.objects,
        rows: stats.rows,
    };
    check_measurement(&measurement)?;
    Ok(measurement)
}

struct RawStats {
    sqlite_version: String,
    journal_mode: String,
    auto_vacuum: i64,
    page_size: i64,
    page_count: i64,
    freelist_count: i64,
    objects: Vec<ObjectBytes>,
    rows: Vec<TableCount>,
}

fn open_measure(db: &Path) -> Result<Connection> {
    if !db
        .try_exists()
        .with_context(|| format!("stat {}", db.display()))?
    {
        bail!("database {} is missing", db.display());
    }
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .with_context(|| format!("open {}", db.display()))?;
    conn.busy_timeout(Duration::ZERO)?;
    conn.execute_batch("PRAGMA temp_store=MEMORY;")?;
    Ok(conn)
}

fn checkpoint(conn: &Connection) -> Result<(i64, i64, i64)> {
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })
    .context("wal_checkpoint")
}

fn read_stats(conn: &Connection) -> Result<RawStats> {
    let sqlite_version: String = conn.query_row("SELECT sqlite_version()", [], |row| row.get(0))?;
    let journal_mode: String = conn.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
    let auto_vacuum: i64 = conn.query_row("PRAGMA auto_vacuum", [], |row| row.get(0))?;
    let page_size: i64 = conn.query_row("PRAGMA page_size", [], |row| row.get(0))?;
    let page_count: i64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0))?;
    let freelist_count: i64 = conn.query_row("PRAGMA freelist_count", [], |row| row.get(0))?;
    let mut object_stmt =
        conn.prepare("SELECT name, SUM(pgsize) FROM dbstat GROUP BY name ORDER BY name")?;
    let objects = object_stmt
        .query_map([], |row| {
            Ok(ObjectBytes {
                name: row.get(0)?,
                bytes: row.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut table_stmt = conn.prepare(
        "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )?;
    let names = table_stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut rows = Vec::with_capacity(names.len());
    for name in names {
        if !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            bail!("unexpected table name {name}");
        }
        let count: i64 =
            conn.query_row(&format!("SELECT COUNT(*) FROM \"{name}\""), [], |row| {
                row.get(0)
            })?;
        rows.push(TableCount { name, rows: count });
    }
    Ok(RawStats {
        sqlite_version,
        journal_mode,
        auto_vacuum,
        page_size,
        page_count,
        freelist_count,
        objects,
        rows,
    })
}

fn check_measurement(measurement: &Measurement) -> Result<()> {
    if measurement.checkpoint_busy != 0 {
        bail!(
            "wal_checkpoint was busy ({}, {}, {}); another connection still has the database",
            measurement.checkpoint_busy,
            measurement.checkpoint_log,
            measurement.checkpoint_frames
        );
    }
    if measurement.checkpoint_log != measurement.checkpoint_frames {
        bail!(
            "wal_checkpoint copied {} of {} frames",
            measurement.checkpoint_frames,
            measurement.checkpoint_log
        );
    }
    if measurement.wal_bytes != 0 {
        bail!(
            "timeseries wal is {} bytes after truncate checkpoint",
            measurement.wal_bytes
        );
    }
    if measurement.page_size <= 0 || measurement.page_count < 0 || measurement.freelist_count < 0 {
        bail!("negative sqlite size pragma");
    }
    let pages = u64::try_from(measurement.page_count)?
        .checked_mul(u64::try_from(measurement.page_size)?)
        .context("page bytes overflow")?;
    if measurement.main_file_len != pages {
        bail!(
            "main file is {} bytes but page_count*page_size is {pages}",
            measurement.main_file_len
        );
    }
    let summed: i64 = measurement.objects.iter().map(|object| object.bytes).sum();
    let accounted = summed
        .checked_add(
            measurement
                .freelist_count
                .checked_mul(measurement.page_size)
                .context("freelist bytes overflow")?,
        )
        .context("accounted bytes overflow")?;
    let logical = measurement
        .page_count
        .checked_mul(measurement.page_size)
        .context("logical bytes overflow")?;
    if accounted != logical {
        let lock_page = if measurement.main_file_len > (1 << 30) {
            measurement.page_size
        } else {
            0
        };
        if accounted.checked_add(lock_page) != Some(logical) {
            bail!("dbstat {summed} + freelist pages != page_count bytes {logical} (accounted {accounted})");
        }
    }
    Ok(())
}

fn file_len(path: &Path) -> Result<u64> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.len()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error).with_context(|| format!("stat {}", path.display())),
    }
}

fn sibling(path: &Path, suffix: &str) -> Result<PathBuf> {
    let name = path.file_name().context("database path has no file name")?;
    let mut owned = name.to_os_string();
    owned.push(suffix);
    Ok(path.with_file_name(owned))
}

fn prepare_output(out: &Path) -> Result<PathBuf> {
    if out.as_os_str().is_empty() {
        bail!("--out is required");
    }
    if out
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        bail!("--out must not contain '..'");
    }
    let abs = if out.is_absolute() {
        out.to_path_buf()
    } else {
        std::env::current_dir()?.join(out)
    };
    fs::create_dir_all(&abs).with_context(|| format!("create {}", abs.display()))?;
    fs::set_permissions(&abs, fs::Permissions::from_mode(0o700))?;
    let db = abs.join("timeseries.db");
    if db.try_exists()? {
        bail!(
            "{} already exists; refusing to append to or checkpoint an existing database",
            db.display()
        );
    }
    Ok(db)
}

fn write_report(out: &Path, report: &Report) -> Result<()> {
    let path = out.join("report.json");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .with_context(|| format!("create {}", path.display()))?;
    file.write_all(serde_json::to_string_pretty(report)?.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

fn render(report: &Report) {
    eprintln!("ts-size parameters: {}", report.parameters.profile);
    eprintln!(
        "  processes={} files_per_process={} passes={} interval={}s duration={}s seed={} churn_permille={} mount={} path_bytes={:?}",
        report.parameters.processes,
        report.parameters.files_per_process,
        report.parameters.passes,
        report.parameters.interval_secs,
        report.parameters.duration_secs,
        report.parameters.seed,
        report.parameters.churn_permille,
        report.parameters.mount,
        report.parameters.path_bytes
    );
    render_measurement("append-only", &report.append_only);
    eprintln!(
        "retention: now={} hot_minutes={}s cutoff={} extra_passes={} compact_deleted={} compact_inserted={}",
        report.retention.now,
        report.retention.hot_minutes_secs,
        report.retention.cutoff_ts,
        report.retention.extra_passes,
        report.retention.compact_deleted,
        report.retention.compact_inserted
    );
    render_measurement("after-delete", &report.retention.after_delete);
    render_measurement("after-reuse", &report.retention.after_reuse);
}

fn render_measurement(label: &str, measurement: &Measurement) {
    eprintln!(
        "{label}: page_size={} page_count={} freelist={} main_file={} wal={} sqlite={}",
        measurement.page_size,
        measurement.page_count,
        measurement.freelist_count,
        measurement.main_file_len,
        measurement.wal_bytes,
        measurement.sqlite_version
    );
    for object in &measurement.objects {
        let rows = measurement
            .rows
            .iter()
            .find(|table| table.name == object.name)
            .map(|table| table.rows.to_string())
            .unwrap_or_else(|| "-".into());
        eprintln!(
            "  {:<24} {:>12} bytes  rows {rows}",
            object.name, object.bytes
        );
    }
}

fn parameters(profile: &Profile, profile_name: &str, passes: i64) -> Result<Parameters> {
    Ok(Parameters {
        profile: profile_name.to_owned(),
        seed: profile.seed,
        processes: profile.processes,
        files_per_process: profile.files_per_process,
        interval_secs: profile.interval_secs,
        duration_secs: profile.duration_secs,
        passes,
        start_ts: profile.start_ts,
        mount: profile.mount.clone(),
        churn_permille: profile.churn_permille,
        path_bytes: profile.path_bytes.clone(),
        pid_base: PID_BASE,
        dev: DEV,
        comms: COMMS.iter().map(|comm| (*comm).to_owned()).collect(),
        unit_rule: UNIT_DESC,
        path_rule: PATH_DESC,
        disk_rule: DISK_DESC,
        mix: MIX_DESC,
        mix_vector_seed_42_pass_0_pid_1000_slot_0: format!(
            "{:x}",
            mix(42, 0, u64::from(PID_BASE), 0)
        ),
        chunk_rows: tidemarkd::services::tswriter::MAX_BATCH_ROWS,
        insert_order: INSERT_ORDER,
        sort: "(pid, slot) ascending within each pass",
        writer_journal_mode: "wal",
        writer_synchronous: "NORMAL",
    })
}

impl Profile {
    fn v1() -> Self {
        // 16*5 = 80 open files per pass, and every path-length class is used.
        Self {
            processes: 16,
            files_per_process: 5,
            interval_secs: 60,
            duration_secs: 604_800,
            seed: 42,
            churn_permille: 200,
            path_bytes: vec![96, 128, 160, 192, 256],
            mount: "/data".into(),
            start_ts: 1_780_000_000,
        }
    }

    fn ci() -> Self {
        Self {
            processes: 4,
            files_per_process: 3,
            interval_secs: 60,
            duration_secs: 300,
            seed: 42,
            churn_permille: 250,
            path_bytes: vec![96, 128, 160, 192, 256],
            mount: "/data".into(),
            start_ts: 1_780_000_000,
        }
    }

    fn passes(&self) -> Result<i64> {
        if self.interval_secs <= 0 {
            bail!("--interval-secs must be >= 1");
        }
        if self.duration_secs <= 0 {
            bail!("--duration-secs must be >= 1");
        }
        if self.duration_secs % self.interval_secs != 0 {
            bail!("--duration-secs must be divisible by --interval-secs");
        }
        let passes = self.duration_secs / self.interval_secs;
        if !(1..=MAX_PASSES).contains(&passes) {
            bail!("pass count {passes} is outside 1..={MAX_PASSES}");
        }
        self.validate_shape()?;
        Ok(passes)
    }

    fn validate_shape(&self) -> Result<()> {
        if !(1..=MAX_PROCESSES).contains(&self.processes) {
            bail!("--processes must be 1..={MAX_PROCESSES}");
        }
        if !(1..=MAX_FILES_PER_PROCESS).contains(&self.files_per_process) {
            bail!("--files-per-process must be 1..={MAX_FILES_PER_PROCESS}");
        }
        let slots = self
            .processes
            .checked_mul(self.files_per_process)
            .context("slot count overflow")?;
        if slots > MAX_SLOTS {
            bail!("processes * files-per-process {slots} exceeds {MAX_SLOTS}");
        }
        if self.churn_permille > 1000 {
            bail!("--churn-permille must be 0..=1000");
        }
        if self.path_bytes.is_empty() {
            bail!("--path-bytes must list at least one length");
        }
        for length in &self.path_bytes {
            if !(MIN_PATH_BYTES..=MAX_PATH_BYTES).contains(length) {
                bail!("path length {length} is outside {MIN_PATH_BYTES}..={MAX_PATH_BYTES}");
            }
        }
        validate_mount(&self.mount)?;
        Ok(())
    }

    fn ensure_budget(&self, total_passes: i64) -> Result<()> {
        let slots = u64::from(self.processes) * u64::from(self.files_per_process);
        let rows = slots
            .checked_mul(u64::try_from(total_passes).context("passes")?)
            .and_then(|value| value.checked_mul(2))
            .context("row budget overflow")?;
        if rows > MAX_SAMPLE_ROWS {
            bail!("worst-case phantom+writer rows {rows} exceeds {MAX_SAMPLE_ROWS}");
        }
        Ok(())
    }

    fn ensure_paths(&self, max_generation: u32) -> Result<()> {
        let procs = process_table(self)?;
        for proc in &procs {
            for slot in 0..self.files_per_process {
                let class = self.path_bytes[slot as usize % self.path_bytes.len()];
                let mut core = Vec::new();
                fill_path(
                    &mut core,
                    &self.mount,
                    &proc.comm,
                    proc.pid,
                    slot,
                    max_generation,
                    class,
                )
                .with_context(|| {
                    format!(
                        "path class {class} is shorter than the unique core for pid {} slot {slot}",
                        proc.pid
                    )
                })?;
            }
        }
        Ok(())
    }
}

fn extra_passes(profile: &Profile) -> Result<i64> {
    let passes = profile.passes()?;
    let extra = if profile.duration_secs >= 2 * DAY_SECS && DAY_SECS % profile.interval_secs == 0 {
        DAY_SECS / profile.interval_secs
    } else {
        (passes / 2).max(1)
    };
    if extra < 1 {
        bail!("reuse pass count must be at least 1");
    }
    Ok(extra)
}

fn validate_mount(mount: &str) -> Result<()> {
    if mount.is_empty() || mount.len() > 256 {
        bail!("--mount must be 1..=256 bytes");
    }
    if !mount.starts_with('/') {
        bail!("--mount must be absolute");
    }
    if mount.len() > 1 && mount.ends_with('/') {
        bail!("--mount must not end with '/'");
    }
    if mount.bytes().any(|byte| byte < 0x20) {
        bail!("--mount contains a control byte");
    }
    Ok(())
}

fn process_table(profile: &Profile) -> Result<Vec<Proc>> {
    for comm in COMMS {
        if comm.is_empty() || comm.len() > 15 || !comm.is_ascii() {
            bail!("comm list entry {comm:?} is outside the 15-byte TASK_COMM_LEN limit");
        }
    }
    let mut procs = Vec::with_capacity(profile.processes as usize);
    for index in 0..profile.processes {
        let pid = PID_BASE.checked_add(index).context("pid overflow")?;
        let comm = COMMS[index as usize % COMMS.len()];
        procs.push(Proc {
            pid,
            comm: comm.to_owned(),
            unit: if index % 10 == 0 {
                None
            } else {
                Some(format!("{comm}.service"))
            },
        });
    }
    Ok(procs)
}

impl State {
    fn new(profile: &Profile) -> Result<Self> {
        let procs = process_table(profile)?;
        let slots = vec![
            Slot {
                generation: 0,
                opened_at: 0,
                last_disk: 0,
            };
            procs.len() * profile.files_per_process as usize
        ];
        Ok(Self { procs, slots })
    }

    fn emit(&mut self, pass: u32, profile: &Profile) -> Result<PassRows> {
        let ts = profile
            .start_ts
            .checked_add(
                i64::from(pass)
                    .checked_mul(profile.interval_secs)
                    .context("timestamp overflow")?,
            )
            .context("timestamp overflow")?;
        let mut phantom = Vec::new();
        let mut writers = Vec::new();
        let mut path = Vec::new();
        for (index, proc) in self.procs.iter().enumerate() {
            for slot in 0..profile.files_per_process {
                let slot_index = index * profile.files_per_process as usize + slot as usize;
                let current = &mut self.slots[slot_index];
                let class = profile.path_bytes[slot as usize % profile.path_bytes.len()];
                if churns(
                    profile.seed,
                    u64::from(pass),
                    proc.pid,
                    slot,
                    profile.churn_permille,
                ) {
                    fill_path(
                        &mut path,
                        &profile.mount,
                        &proc.comm,
                        proc.pid,
                        slot,
                        current.generation,
                        class,
                    )?;
                    phantom.push(PhantomSample {
                        ts,
                        mount: profile.mount.clone(),
                        pid: proc.pid,
                        comm: proc.comm.clone(),
                        unit: proc.unit.clone(),
                        path: path.clone(),
                        dev: DEV,
                        ino: pack_ino(proc.pid, slot, current.generation)?,
                        bytes: current.last_disk,
                    });
                    current.generation = current
                        .generation
                        .checked_add(1)
                        .context("generation overflow")?;
                    current.opened_at = pass;
                    current.last_disk = 0;
                }
                let age = pass
                    .checked_sub(current.opened_at)
                    .and_then(|delta| delta.checked_add(1))
                    .context("writer age overflow")?;
                let disk = 4096u64
                    .checked_mul(u64::from(age))
                    .context("disk overflow")?;
                fill_path(
                    &mut path,
                    &profile.mount,
                    &proc.comm,
                    proc.pid,
                    slot,
                    current.generation,
                    class,
                )?;
                writers.push(WriterSample {
                    ts,
                    mount: profile.mount.clone(),
                    pid: proc.pid,
                    comm: proc.comm.clone(),
                    unit: proc.unit.clone(),
                    path: path.clone(),
                    dev: DEV,
                    ino: pack_ino(proc.pid, slot, current.generation)?,
                    size: disk,
                    disk,
                });
                current.last_disk = disk;
            }
        }
        Ok(PassRows { phantom, writers })
    }
}

fn churns(seed: u64, pass: u64, pid: u32, slot: u32, churn_permille: u32) -> bool {
    mix(seed, pass, u64::from(pid), u64::from(slot)) % 1000 < u64::from(churn_permille)
}

fn mix(seed: u64, pass: u64, pid: u64, slot: u64) -> u64 {
    let mixed = seed
        ^ pass.wrapping_mul(0xD1B5_4A32_D192_ED03)
        ^ pid.wrapping_mul(0x94D0_49BB_1331_11EB)
        ^ slot.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    splitmix64(mixed)
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut mixed = value;
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    mixed ^ (mixed >> 31)
}

fn pack_ino(pid: u32, slot: u32, generation: u32) -> Result<u64> {
    if pid >= 1 << 24 || slot >= 1 << 20 || generation >= 1 << 20 {
        bail!("inode fields overlap or exceed sqlite integer range");
    }
    let ino = (u64::from(pid) << 40) | (u64::from(slot) << 20) | u64::from(generation);
    i64::try_from(ino).context("inode does not fit in a sqlite integer")?;
    Ok(ino)
}

fn fill_path(
    buf: &mut Vec<u8>,
    mount: &str,
    comm: &str,
    pid: u32,
    slot: u32,
    generation: u32,
    class: usize,
) -> Result<()> {
    buf.clear();
    if mount == "/" {
        buf.push(b'/');
    } else {
        buf.extend_from_slice(mount.as_bytes());
        buf.push(b'/');
    }
    let rest = format!("srv/{comm}.service/pid-{pid}/slot-{slot}/gen-{generation}/app.log");
    buf.extend_from_slice(rest.as_bytes());
    if buf.len() > class {
        bail!(
            "path core is {} bytes and length class is {class}",
            buf.len()
        );
    }
    buf.resize(class, b'a');
    Ok(())
}

fn parse_args(args: impl IntoIterator<Item = std::ffi::OsString>) -> Result<Cli> {
    let args: Vec<_> = args.into_iter().collect();
    let mut profile_name: Option<String> = None;
    let mut out: Option<PathBuf> = None;
    let mut json = false;
    let mut processes = None;
    let mut files = None;
    let mut interval = None;
    let mut duration = None;
    let mut seed = None;
    let mut churn = None;
    let mut path_bytes = None;
    let mut mount = None;
    let mut start_ts = None;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--json" {
            if json {
                bail!("duplicate --json\n{USAGE}");
            }
            json = true;
            index += 1;
            continue;
        }
        let value = args
            .get(index + 1)
            .with_context(|| format!("missing value for {}\n{USAGE}", arg.to_string_lossy()))?;
        let text = value.to_str().with_context(|| {
            format!("value for {} must be UTF-8\n{USAGE}", arg.to_string_lossy())
        })?;
        match arg.to_str() {
            Some("--profile") => assign(&mut profile_name, text.to_owned(), "--profile")?,
            Some("--out") => {
                if out.is_some() {
                    bail!("duplicate --out\n{USAGE}");
                }
                out = Some(PathBuf::from(value));
            }
            Some("--processes") => assign(&mut processes, text.parse()?, "--processes")?,
            Some("--files-per-process") => {
                assign(&mut files, text.parse()?, "--files-per-process")?;
            }
            Some("--interval-secs") => assign(&mut interval, text.parse()?, "--interval-secs")?,
            Some("--duration-secs") => assign(&mut duration, text.parse()?, "--duration-secs")?,
            Some("--seed") => assign(&mut seed, text.parse()?, "--seed")?,
            Some("--churn-permille") => assign(&mut churn, text.parse()?, "--churn-permille")?,
            Some("--path-bytes") => {
                if path_bytes.is_some() {
                    bail!("duplicate --path-bytes\n{USAGE}");
                }
                path_bytes = Some(parse_path_bytes(text)?);
            }
            Some("--mount") => assign(&mut mount, text.to_owned(), "--mount")?,
            Some("--start-ts") => assign(&mut start_ts, text.parse()?, "--start-ts")?,
            _ => bail!("unknown argument {}\n{USAGE}", arg.to_string_lossy()),
        }
        index += 2;
    }
    let profile_name = profile_name.context(USAGE)?;
    let out = out.context(USAGE)?;
    let mut profile = match profile_name.as_str() {
        "v1" => Profile::v1(),
        "ci" => Profile::ci(),
        _ => bail!("--profile must be v1 or ci\n{USAGE}"),
    };
    if let Some(value) = processes {
        profile.processes = value;
    }
    if let Some(value) = files {
        profile.files_per_process = value;
    }
    if let Some(value) = interval {
        profile.interval_secs = value;
    }
    if let Some(value) = duration {
        profile.duration_secs = value;
    }
    if let Some(value) = seed {
        profile.seed = value;
    }
    if let Some(value) = churn {
        profile.churn_permille = value;
    }
    if let Some(value) = path_bytes {
        profile.path_bytes = value;
    }
    if let Some(value) = mount {
        profile.mount = value;
    }
    if let Some(value) = start_ts {
        profile.start_ts = value;
    }
    let passes = profile.passes()?;
    let extra = extra_passes(&profile)?;
    let total = passes.checked_add(extra).context("pass count overflow")?;
    profile.ensure_budget(total)?;
    profile.ensure_paths(u32::try_from(total).context("pass count")?)?;
    Ok(Cli {
        out,
        profile,
        profile_name,
        json,
    })
}

fn assign<T>(slot: &mut Option<T>, value: T, flag: &str) -> Result<()> {
    if slot.is_some() {
        bail!("duplicate {flag}\n{USAGE}");
    }
    *slot = Some(value);
    Ok(())
}

fn parse_path_bytes(text: &str) -> Result<Vec<usize>> {
    if text.is_empty() {
        bail!("--path-bytes is empty\n{USAGE}");
    }
    let mut lengths = Vec::new();
    for part in text.split(',') {
        if part.is_empty() {
            bail!("--path-bytes contains an empty length\n{USAGE}");
        }
        lengths.push(
            part.parse()
                .with_context(|| format!("invalid --path-bytes entry {part}"))?,
        );
    }
    Ok(lengths)
}

#[cfg(test)]
fn rows_of(measurement: &Measurement, name: &str) -> i64 {
    measurement
        .rows
        .iter()
        .find(|table| table.name == name)
        .map(|table| table.rows)
        .unwrap_or(0)
}

#[cfg(test)]
fn bytes_of(measurement: &Measurement, name: &str) -> i64 {
    measurement
        .objects
        .iter()
        .find(|object| object.name == name)
        .map(|object| object.bytes)
        .unwrap_or(0)
}

#[cfg(test)]
fn storage_eq(left: &Measurement, right: &Measurement) -> bool {
    let mut left = left.clone();
    let mut right = right.clone();
    left.shm_bytes = 0;
    right.shm_bytes = 0;
    left == right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_vector_is_stable() {
        assert_eq!(mix(42, 0, 1000, 0), 0x4a94_7683_533f_72ef);
        assert_eq!(mix(42, 0, 1000, 0) % 1000, 815);
    }

    #[test]
    fn inode_pack_widens_before_shifting() {
        let ino = pack_ino(100_999, 1023, 20_000).unwrap();
        assert_eq!(
            ino,
            (u64::from(100_999u32) << 40) | (u64::from(1023u32) << 20) | u64::from(20_000u32)
        );
        assert!(i64::try_from(ino).is_ok());
        assert!(pack_ino(1 << 24, 0, 0).is_err());
    }

    #[test]
    fn batches_follow_the_daemon_cap() {
        let cap = tidemarkd::services::tswriter::MAX_BATCH_ROWS;
        assert_eq!(cap, 20_000);
        let sizes: Vec<_> = vec![0u8; cap + 1].chunks(cap).map(<[_]>::len).collect();
        assert_eq!(sizes, vec![cap, 1]);
    }

    #[test]
    fn comm_names_fit_task_comm_len() {
        assert!(COMMS
            .iter()
            .all(|comm| !comm.is_empty() && comm.len() <= 15));
    }

    #[test]
    fn v1_uses_every_configured_path_length() {
        let profile = Profile::v1();
        let mut state = State::new(&profile).unwrap();
        let rows = state.emit(0, &profile).unwrap();
        let mut seen: Vec<usize> = rows.writers.iter().map(|row| row.path.len()).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen, profile.path_bytes);
        assert_eq!(rows.writers.len(), 80);
    }

    #[test]
    fn v1_budget_includes_the_reuse_day() {
        let profile = Profile::v1();
        assert_eq!(profile.passes().unwrap(), 10_080);
        assert_eq!(extra_passes(&profile).unwrap(), 1_440);
        profile.ensure_budget(10_080 + 1_440).unwrap();
    }

    #[test]
    fn ci_predicate_counts_are_fixed() {
        let profile = Profile::ci();
        let mut state = State::new(&profile).unwrap();
        let mut phantoms = 0;
        let mut writers = 0;
        for pass in 0..5 {
            let rows = state.emit(pass, &profile).unwrap();
            phantoms += rows.phantom.len();
            writers += rows.writers.len();
        }
        assert_eq!(writers, 60);
        assert_eq!(phantoms, 14);
    }

    #[test]
    fn churn_keeps_the_previous_generation_on_the_phantom_row() {
        let mut profile = Profile::ci();
        profile.churn_permille = 1000;
        profile.processes = 1;
        profile.files_per_process = 1;
        let mut state = State::new(&profile).unwrap();
        let first = state.emit(0, &profile).unwrap();
        assert!(first.writers[0].unit.is_none());
        assert_eq!(first.phantom.len(), 1);
        assert_eq!(first.phantom[0].bytes, 0);
        assert_eq!(first.phantom[0].ino, pack_ino(1000, 0, 0).unwrap());
        assert_eq!(first.writers[0].disk, 4096);
        assert_eq!(first.writers[0].size, 4096);
        assert_eq!(first.writers[0].ino, pack_ino(1000, 0, 1).unwrap());
        assert_eq!(first.writers[0].path.len(), 96);
        let second = state.emit(1, &profile).unwrap();
        assert_eq!(second.phantom[0].bytes, 4096);
        assert_eq!(second.phantom[0].ino, pack_ino(1000, 0, 1).unwrap());
        assert_eq!(second.writers[0].disk, 4096);
        assert_eq!(second.writers[0].ino, pack_ino(1000, 0, 2).unwrap());
        assert!(first.phantom[0]
            .path
            .windows(6)
            .any(|window| window == b"/gen-0"));
        assert!(second.phantom[0]
            .path
            .windows(6)
            .any(|window| window == b"/gen-1"));
        assert!(second.writers[0]
            .path
            .windows(6)
            .any(|window| window == b"/gen-2"));
    }

    #[test]
    fn quiet_slots_grow_without_phantoms() {
        let mut profile = Profile::ci();
        profile.churn_permille = 0;
        profile.processes = 1;
        profile.files_per_process = 1;
        let mut state = State::new(&profile).unwrap();
        let first = state.emit(0, &profile).unwrap();
        assert!(first.phantom.is_empty());
        assert_eq!(first.writers[0].disk, 4096);
        let second = state.emit(1, &profile).unwrap();
        assert!(second.phantom.is_empty());
        assert_eq!(second.writers[0].disk, 8192);
        assert_eq!(second.writers[0].ino, first.writers[0].ino);
    }

    #[test]
    fn short_path_class_is_rejected_before_insert() {
        let mut profile = Profile::ci();
        profile.path_bytes = vec![32];
        assert!(profile.ensure_paths(5).is_err());
    }

    #[test]
    fn parser_requires_profile_and_applies_overrides() {
        assert!(parse_args(Vec::<std::ffi::OsString>::new()).is_err());
        let cli = parse_args([
            "--profile".into(),
            "ci".into(),
            "--out".into(),
            "out".into(),
            "--processes".into(),
            "2".into(),
            "--json".into(),
        ])
        .unwrap();
        assert!(cli.json);
        assert_eq!(cli.profile.processes, 2);
        assert_eq!(
            cli.profile.files_per_process,
            Profile::ci().files_per_process
        );
        assert!(parse_args([
            "--profile".into(),
            "v1".into(),
            "--out".into(),
            "out".into(),
            "--duration-secs".into(),
            "90".into(),
        ])
        .is_err());
    }

    #[test]
    fn row_cap_rejects_an_extra_zero() {
        let mut profile = Profile::v1();
        profile.duration_secs *= 10;
        assert!(profile.passes().is_err());
    }

    #[test]
    fn ci_append_only_is_deterministic_and_bounded() {
        let profile = Profile::ci();
        let left = run_append(&profile);
        let right = run_append(&profile);
        assert!(storage_eq(&left, &right));
        assert_eq!(left.page_size, 4096);
        assert_eq!(left.freelist_count, 0);
        assert_eq!(left.wal_bytes, 0);
        assert_eq!(left.journal_mode, "wal");
        assert_eq!(left.auto_vacuum, 0);
        assert!(left.page_count * left.page_size < CI_BYTE_CEILING);
        assert_eq!(rows_of(&left, "writer_samples"), 60);
        assert_eq!(rows_of(&left, "phantom_samples"), 14);
        assert!(bytes_of(&left, "writer_samples") >= 2 * left.page_size);
        assert!(bytes_of(&left, "writer_ts") >= left.page_size);
        assert!(bytes_of(&left, "phantom_samples") >= left.page_size);
        assert!(bytes_of(&left, "phantom_ts") >= left.page_size);
        for table in &left.rows {
            if table.name != "writer_samples" && table.name != "phantom_samples" {
                assert_eq!(table.rows, 0, "{}", table.name);
            }
        }
    }

    #[test]
    fn ci_retention_deletes_and_reuses_rows() {
        let dir = test_support::private_tempdir().unwrap();
        let db = dir.path().join("timeseries.db");
        let report = execute(&Profile::ci(), "ci", &db, false).unwrap();
        assert!(report.retention.compact_deleted > 0);
        assert!(rows_of(&report.retention.after_delete, "writer_samples") < 60);
        assert!(
            rows_of(&report.retention.after_reuse, "writer_samples")
                > rows_of(&report.retention.after_delete, "writer_samples")
        );
        assert!(
            report.retention.after_reuse.page_count * report.retention.after_reuse.page_size
                < CI_BYTE_CEILING
        );
        assert_eq!(report.retention.after_delete.wal_bytes, 0);
        assert_eq!(report.retention.after_reuse.wal_bytes, 0);
    }

    #[test]
    fn measure_refuses_a_busy_checkpoint() {
        let dir = test_support::private_tempdir().unwrap();
        let db = dir.path().join("timeseries.db");
        drop(TimeSeries::open(&db, true).unwrap());
        let holder = Connection::open(&db).unwrap();
        holder.busy_timeout(Duration::ZERO).unwrap();
        holder.execute_batch("BEGIN IMMEDIATE").unwrap();
        let error = measure(&db).unwrap_err();
        let text = format!("{error:#}");
        assert!(text.contains("busy"), "{text}");
    }

    fn run_append(profile: &Profile) -> Measurement {
        let dir = test_support::private_tempdir().unwrap();
        append_only(profile, &dir.path().join("timeseries.db")).unwrap()
    }
}
