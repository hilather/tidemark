//! Owned by: W0-B2
//! Aligned tables and portable tree markers.

/// Render columns aligned to their longest cell, counting Unicode scalar values.
/// Callers bound their row count before supplying rows.
pub fn aligned(rows: &[Vec<String>]) -> String {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut widths = vec![0; columns];
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
    let mut output = String::new();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            output.push_str(cell);
            if i + 1 < row.len() {
                output.extend(std::iter::repeat_n(
                    ' ',
                    widths[i] - cell.chars().count() + 2,
                ));
            }
        }
        output.push('\n');
    }
    output
}
/// Tree branch marker with an explicit ASCII fallback.
pub fn branch(last: bool, ascii: bool) -> &'static str {
    match (last, ascii) {
        (false, false) => "├─ ",
        (true, false) => "└─ ",
        (false, true) => "|- ",
        (true, true) => "`- ",
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn columns_and_fallback() {
        assert_eq!(
            aligned(&[vec!["é".into(), "1".into()], vec!["abc".into(), "2".into()]]),
            "é    1\nabc  2\n"
        );
        assert_eq!(aligned(&[]), "");
        assert_eq!(branch(false, true), "|- ");
        assert_eq!(branch(true, false), "└─ ");
    }
}
