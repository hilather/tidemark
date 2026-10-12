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
    if cell.chars().count() <= width {
        return cell.to_owned();
    }
    if width < 3 {
        return ".".repeat(width);
    }
    let budget = width - 3;
    let last_component = cell
        .rsplit_once('/')
        .map_or(0, |(_, last)| last.chars().count() + 1);
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
        let n = token.chars().count();
        if used + n > left_budget {
            break;
        }
        left.push_str(token);
        used += n;
    }
    for token in tokens.iter().rev() {
        let n = token.chars().count();
        if used + n > budget {
            break;
        }
        right.insert_str(0, token);
        used += n;
    }
    format!("{left}...{right}")
}
/// Fit each table's cells to a shared width before aligning them.
pub fn aligned_width(rows: &[Vec<String>], width: usize) -> String {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    if columns == 0 {
        return String::new();
    }
    let mut widths = vec![0; columns];
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
    let available = width.saturating_sub(2 * columns.saturating_sub(1));
    while widths.iter().sum::<usize>() > available {
        let Some((index, largest)) = widths.iter().enumerate().max_by_key(|(_, n)| **n) else {
            break;
        };
        if *largest == 0 {
            break;
        }
        widths[index] -= 1;
    }
    aligned(
        &rows
            .iter()
            .map(|row| {
                row.iter()
                    .enumerate()
                    .map(|(i, cell)| shorten(cell, widths[i]))
                    .collect()
            })
            .collect::<Vec<_>>(),
    )
}
/// Wrap text at whitespace and shorten oversized tokens without splitting escapes.
pub fn wrap(text: &str, width: usize) -> String {
    let width = width.max(1);
    let mut out = String::new();
    for line in text.lines() {
        if line.chars().count() <= width {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let indent = line
            .chars()
            .take_while(|c| *c == ' ')
            .count()
            .min(width.saturating_sub(1));
        let mut current = " ".repeat(indent);
        for word in line.split_whitespace() {
            let word = shorten(word, width - indent);
            let space = usize::from(current.len() > indent);
            if current.chars().count() + space + word.chars().count() > width {
                out.push_str(&current);
                out.push('\n');
                current = " ".repeat(indent);
            }
            if current.len() > indent {
                current.push(' ');
            }
            current.push_str(&word);
        }
        out.push_str(&current);
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
