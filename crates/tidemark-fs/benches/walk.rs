//! Owned by: W1-B
//! Warm metadata traversal throughput over 100,000 generated files.
use std::{fs, sync::atomic::AtomicBool, time::Instant};
use tidemark_core::{path::RawPath, scan::NoopGate};
use tidemark_fs::walk::{walk, WalkConfig};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    for d in 0..100 {
        let dir = temp.path().join(d.to_string());
        fs::create_dir(&dir)?;
        for f in 0..1000 {
            fs::File::create(dir.join(f.to_string()))?;
        }
    }
    let cfg = WalkConfig {
        root: RawPath::from_path(temp.path())?,
        threads: 8,
        stat_entries: true,
        exclude: globset::GlobSet::empty(),
        skip_mounts: Vec::new(),
        big_file_threshold: u64::MAX,
        top_files: 100,
        scan_start: 0,
        getdents_buf: 256 * 1024,
        io_idle: true,
        // Bound each directory record to 64 MiB for the generated benchmark tree.
        max_record_bytes: 64 * 1024 * 1024,
    };
    let (tx, rx) = crossbeam_channel::bounded(1024);
    let start = Instant::now();
    let summary = std::thread::scope(|scope| {
        let producer = scope.spawn(|| walk(&cfg, &NoopGate, &AtomicBool::new(false), tx));
        for _ in rx {}
        producer
            .join()
            .map_err(|_| std::io::Error::other("benchmark worker panicked"))?
            .map_err(std::io::Error::other)
    })?;
    println!(
        "{} entries in {:?}: {:.0} entries/s",
        summary.entries,
        start.elapsed(),
        summary.entries as f64 / start.elapsed().as_secs_f64()
    );
    Ok(())
}
