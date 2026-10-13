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

/// Standard output's positive terminal width, or 100 for redirected output.
pub fn terminal_width() -> usize {
    use std::io::IsTerminal;
    let stdout = std::io::stdout();
    select_width(stdout.is_terminal(), || {
        rustix::termios::tcgetwinsize(&stdout)
            .ok()
            .map(|s| s.ws_col)
    })
}
fn select_width(is_terminal: bool, query: impl FnOnce() -> Option<u16>) -> usize {
    if is_terminal {
        query().filter(|width| *width > 0).map_or(100, usize::from)
    } else {
        100
    }
}
/// Conservative terminal-column budget: ASCII occupies one, other scalars at most two.
/// Counting combining/ambiguous scalars as two avoids overflowing differing terminal locales.
pub fn display_width(text: &str) -> usize {
    text.chars()
        .map(|ch| if ch.is_ascii() { 1 } else { 2 })
        .sum()
}
/// Shorten the middle of a display cell, retaining complete escaped-byte tokens.
pub fn shorten(cell: &str, width: usize) -> String {
    let mut tokens = Vec::new();
    let mut chars = cell.char_indices().peekable();
    while let Some((start, ch)) = chars.next() {
        if ch == '\\' && chars.peek().is_some_and(|(_, c)| *c == 'x') {
            for _ in 0..3 {
                let _ = chars.next();
            }
        }
        let end = chars.peek().map_or(cell.len(), |(i, _)| *i);
        tokens.push(&cell[start..end]);
    }
    if display_width(cell) <= width {
        return cell.to_owned();
    }
    if width < 3 {
        return ".".repeat(width);
    }
    let budget = width - 3;
    let last_component = cell
        .rsplit_once('/')
        .map_or(0, |(_, last)| display_width(last) + 1);
    let right_budget = if last_component < budget {
        (budget - budget / 2).max(last_component)
    } else {
        budget - budget / 2
    };
    let left_budget = budget - right_budget;
    let mut left = String::new();
    let mut right = String::new();
    let mut used = 0;
    for token in &tokens {
        let n = display_width(token);
        if used + n > left_budget {
            break;
        }
        left.push_str(token);
        used += n;
    }
    for token in tokens.iter().rev() {
        let n = display_width(token);
        if used + n > budget {
            break;
        }
        right.insert_str(0, token);
        used += n;
    }
    format!("{left}...{right}")
}
/// Fit path/unit cells to a shared width before aligning them.
/// For tables containing prose, use `aligned_wrapped` with explicit short columns.
pub fn aligned_width(rows: &[Vec<String>], width: usize) -> String {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    aligned_wrapped(rows, width, &(0..columns).collect::<Vec<_>>())
}
/// Align a table, wrapping free text and shortening only the named path/unit columns.
/// Continuations begin at each cell's column. Oversized words remain whole.
pub fn aligned_wrapped(rows: &[Vec<String>], width: usize, short_columns: &[usize]) -> String {
    aligned_wrapped_right(rows, width, short_columns, &[])
}
/// Align and wrap a table, right-aligning the named numeric data columns.
pub fn aligned_wrapped_right(
    rows: &[Vec<String>],
    width: usize,
    short_columns: &[usize],
    right_columns: &[usize],
) -> String {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    if columns == 0 {
        return String::new();
    }
    let mut widths = vec![0; columns];
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(display_width(cell));
        }
    }
    let available = width.saturating_sub(2 * columns.saturating_sub(1));
    let minimum: Vec<usize> = (0..columns)
        .map(|i| {
            if short_columns.contains(&i) {
                rows.first()
                    .and_then(|r| r.get(i))
                    .map_or(1, |s| display_width(s))
            } else {
                rows.iter()
                    .filter_map(|r| r.get(i))
                    .flat_map(|s| s.split_whitespace())
                    .map(display_width)
                    .filter(|n| *n < width)
                    .max()
                    .unwrap_or(1)
                    .min(available)
            }
        })
        .collect();
    while widths.iter().sum::<usize>() > available {
        let eligible = |i: usize| widths[i] > minimum[i];
        // Give prose its longest-word budget, then borrow space from paths/units
        // before wrapping fixed metadata columns such as timestamps.
        let candidate = if eligible(columns - 1) {
            Some((columns - 1, &widths[columns - 1]))
        } else {
            widths
                .iter()
                .enumerate()
                .filter(|(i, _)| short_columns.contains(i) && eligible(*i))
                .max_by_key(|(_, n)| **n)
                .or_else(|| {
                    widths
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| eligible(*i))
                        .max_by_key(|(_, n)| **n)
                })
        };
        let candidate = candidate.or_else(|| {
            widths
                .iter()
                .enumerate()
                .filter(|(_, n)| **n > 0)
                .max_by_key(|(_, n)| **n)
        });
        let Some((index, _)) = candidate else {
            break;
        };
        widths[index] -= 1;
    }
    // Multiple unbreakable cells may not fit side by side. Keep their column
    // starts within the page; the row writer puts overlapping cells on separate lines.
    for i in 1..columns {
        let longest = rows
            .iter()
            .filter_map(|r| r.get(i))
            .flat_map(|s| s.split_whitespace())
            .map(display_width)
            .filter(|n| *n < width)
            .max()
            .unwrap_or(0);
        while widths[..i].iter().sum::<usize>() + 2 * i + longest > width {
            let Some((index, largest)) = widths[..i].iter().enumerate().max_by_key(|(_, n)| **n)
            else {
                break;
            };
            if *largest == 0 {
                break;
            }
            widths[index] -= 1;
        }
    }
    let mut out = String::new();
    for (row_index, row) in rows.iter().enumerate() {
        let cells: Vec<Vec<String>> = row
            .iter()
            .enumerate()
            .map(|(i, cell)| {
                if row_index > 0 && short_columns.contains(&i) {
                    vec![shorten(cell, widths[i])]
                } else {
                    word_lines(cell, widths[i].max(1))
                }
            })
            .collect();
        let height = cells.iter().map(Vec::len).max().unwrap_or(0);
        for line in 0..height {
            let mut position = 0;
            for (i, cell) in cells.iter().enumerate() {
                let text = cell.get(line).map_or("", String::as_str);
                let padding = if row_index > 0 && right_columns.contains(&i) {
                    widths[i].saturating_sub(display_width(text))
                } else {
                    0
                };
                let start: usize = widths[..i].iter().sum::<usize>() + 2 * i + padding;
                if text.is_empty() {
                    continue;
                }
                // An oversized word gets its own line rather than overlapping a later cell.
                if position > 0 && (position + 2 > start || start + display_width(text) > width) {
                    out.push('\n');
                    position = 0;
                }
                out.extend(std::iter::repeat_n(' ', start.saturating_sub(position)));
                out.push_str(text);
                position = start + display_width(text);
            }
            out.push('\n');
        }
    }
    out
}
fn word_lines(text: &str, width: usize) -> Vec<String> {
    if display_width(text) <= width {
        return vec![text.to_owned()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && display_width(&current) + 1 + display_width(word) > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}
/// Wrap prose at word boundaries without dropping oversized words.
pub fn wrap(text: &str, width: usize) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let indent = line.chars().take_while(|c| *c == ' ').count();
        out.push_str(&wrap_prefixed(
            &" ".repeat(indent),
            line.trim_start(),
            width,
        ));
    }
    out
}
/// Wrap a prefixed item, indenting every continuation by the prefix's display width.
pub fn wrap_prefixed(prefix: &str, text: &str, width: usize) -> String {
    let indent = display_width(prefix);
    let lines = word_lines(text, width.saturating_sub(indent).max(1));
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i == 0 {
            if display_width(line) > width.saturating_sub(indent) && !prefix.trim().is_empty() {
                out.push_str(prefix.trim_end());
                out.push('\n');
                out.extend(std::iter::repeat_n(' ', indent));
            } else {
                out.push_str(prefix);
            }
        } else {
            out.extend(std::iter::repeat_n(' ', indent));
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod fitting_tests {
    #[test]
    fn terminal_selection_uses_positive_width_or_default() {
        assert_eq!(
            super::select_width(false, || panic!("nonterminal queried")),
            100
        );
        assert_eq!(super::select_width(true, || Some(132)), 132);
        assert_eq!(super::select_width(true, || Some(0)), 100);
        assert_eq!(super::select_width(true, || None), 100);
    }

    #[test]
    fn suffix_and_escape_tokens_survive_shortening() {
        let cell = format!("/begin/{}/last-component", "long".repeat(50));
        let text = super::shorten(&cell, 30);
        assert!(text.starts_with("/begin/") && text.ends_with("/last-component"));
        for width in 3..40 {
            let text = super::shorten("\\xFF".repeat(20).as_str(), width);
            assert!(text.chars().count() <= width);
            for chunk in text.split("...") {
                assert!(chunk.len().is_multiple_of(4));
            }
        }
    }
}

/// Escape control characters in one human diagnostic cell, preserving prior path escapes.
/// Layout newlines are emitted by the caller separately from untrusted message text.
pub fn diagnostic(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_control() {
            let mut bytes = [0; 4];
            output.push_str(&tidemark_core::path::display_lossy_bytes(
                ch.encode_utf8(&mut bytes).as_bytes(),
            ));
        } else {
            output.push(ch);
        }
    }
    output
}
