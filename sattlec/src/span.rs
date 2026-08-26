//! Source locations: byte offsets ↔ line/column.

use crate::lexer::SpannedToken;

/// Maps byte offsets to 1-based `(line, column)` positions.
///
/// Column counts Unicode scalar values (chars), not UTF-8 bytes.
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offset of the start of each line (line 1 at index 0).
    line_starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self { line_starts }
    }

    pub fn line_col(&self, source: &str, offset: usize) -> (usize, usize) {
        let offset = offset.min(source.len());
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let line_start = self.line_starts[line_idx];
        let col = source[line_start..offset].chars().count() + 1;
        (line_idx + 1, col)
    }
}

/// Format tokens one per line: `KIND @ line:col` (1-based, token start).
pub fn format_tokens(source: &str, tokens: &[SpannedToken<'_>]) -> String {
    let index = LineIndex::new(source);
    let mut out = String::new();
    for t in tokens {
        let (line, col) = index.line_col(source, t.span.start);
        out.push_str(&format!("{} @ {line}:{col}\n", t.kind));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;

    #[test]
    fn format_tokens_uses_line_col() {
        let src = "fn main";
        let tokens = lex(src).unwrap();
        assert_eq!(
            format_tokens(src, &tokens),
            "fn @ 1:1\nIdent(main) @ 1:4\n"
        );
    }

    #[test]
    fn format_tokens_tracks_newlines() {
        let src = "fn\nmain";
        let tokens = lex(src).unwrap();
        assert_eq!(
            format_tokens(src, &tokens),
            "fn @ 1:1\nIdent(main) @ 2:1\n"
        );
    }

    #[test]
    fn line_index_end_of_file() {
        let src = "a\nb";
        let index = LineIndex::new(src);
        assert_eq!(index.line_col(src, 0), (1, 1));
        assert_eq!(index.line_col(src, 2), (2, 1));
        assert_eq!(index.line_col(src, src.len()), (2, 2));
    }
}
