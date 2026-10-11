//! Owned by: W2-A
//! Deterministic final-id notable selection.
use super::{count, AggConfig, AggError, FileRow};
pub(super) fn select(
    mut files: Vec<FileRow>,
    ids: &[u32],
    cfg: &AggConfig,
) -> Result<Vec<FileRow>, AggError> {
    for f in &mut files {
        f.dir_id = *ids
            .get(f.dir_id as usize)
            .filter(|&&n| n != u32::MAX)
            .ok_or(AggError::Tree)?;
    }
    files.sort_by(|a, b| {
        b.disk
            .cmp(&a.disk)
            .then(a.dir_id.cmp(&b.dir_id))
            .then(a.ino.cmp(&b.ino))
    });
    let keep = files
        .iter()
        .take_while(|f| f.disk >= cfg.big_file_threshold)
        .count()
        .max(cfg.top_files)
        .min(cfg.max_notable);
    files.truncate(keep);
    files.shrink_to_fit();
    for (id, f) in files.iter_mut().enumerate() {
        f.id = count(id)?;
    }
    Ok(files)
}
