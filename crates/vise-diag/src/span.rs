//! Source positions.
//!
//! Spans are byte offsets into a file, which is what the lexer naturally
//! produces. Converting to line and column happens only when a diagnostic is
//! rendered, so the hot path never pays for it.

/// Index of a file within a [`SourceMap`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(pub u32);

/// A half-open byte range `[start, end)` within a single file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub file: FileId,
    pub start: u32,
    pub end: u32,
}

impl Span {
    #[must_use]
    pub const fn new(file: FileId, start: u32, end: u32) -> Self {
        Self { file, start, end }
    }

    /// The smallest span covering both. Both must be in the same file.
    ///
    /// # Panics
    /// Panics if the spans are in different files.
    #[must_use]
    pub fn to(self, other: Self) -> Self {
        assert_eq!(self.file, other.file, "cannot join spans across files");
        Self {
            file: self.file,
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    #[must_use]
    pub const fn len(self) -> u32 {
        self.end - self.start
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }
}

/// A one-based line and column. The column counts characters, not bytes, so it
/// lines up with what a person or an editor sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCol {
    pub line: u32,
    pub col: u32,
}

/// One loaded source file, with a precomputed index of line start offsets.
#[derive(Debug)]
pub struct SourceFile {
    name: String,
    text: String,
    /// Byte offset of the first character of each line.
    line_starts: Vec<u32>,
}

impl SourceFile {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Number of lines. A file always has at least one.
    #[must_use]
    pub fn line_count(&self) -> u32 {
        u32::try_from(self.line_starts.len()).unwrap_or(u32::MAX)
    }

    /// Resolve a byte offset to a one-based line and column.
    ///
    /// Offsets past the end of the file clamp to the final position rather
    /// than panicking: a diagnostic should never be the thing that crashes.
    #[must_use]
    pub fn line_col(&self, offset: u32) -> LineCol {
        let offset = offset.min(u32::try_from(self.text.len()).unwrap_or(u32::MAX));
        // Index of the last line start that is <= offset.
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1, // i is never 0: line_starts[0] == 0 <= offset
        };
        let line_start = self.line_starts[line_idx] as usize;
        let col = self.text[line_start..offset as usize].chars().count() + 1;
        LineCol {
            line: u32::try_from(line_idx + 1).unwrap_or(u32::MAX),
            col: u32::try_from(col).unwrap_or(u32::MAX),
        }
    }

    /// The text of a one-based line, without its terminator.
    #[must_use]
    pub fn line_text(&self, line: u32) -> Option<&str> {
        let idx = (line as usize).checked_sub(1)?;
        let start = *self.line_starts.get(idx)? as usize;
        let end = self
            .line_starts
            .get(idx + 1)
            .map_or(self.text.len(), |&s| s as usize);
        Some(self.text[start..end].trim_end_matches(['\n', '\r']))
    }
}

/// Every file the compiler has loaded this run.
#[derive(Debug, Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Load a file and return its id.
    ///
    /// # Panics
    /// Panics if more than `u32::MAX` files are added.
    pub fn add(&mut self, name: impl Into<String>, text: impl Into<String>) -> FileId {
        let text = text.into();
        let mut line_starts = vec![0u32];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(u32::try_from(i + 1).expect("source file exceeds 4 GiB"));
            }
        }
        let id = FileId(u32::try_from(self.files.len()).expect("too many source files"));
        self.files.push(SourceFile {
            name: name.into(),
            text,
            line_starts,
        });
        id
    }

    #[must_use]
    pub fn file(&self, id: FileId) -> &SourceFile {
        &self.files[id.0 as usize]
    }

    /// The text a span covers.
    #[must_use]
    pub fn snippet(&self, span: Span) -> &str {
        let f = self.file(span.file);
        let start = (span.start as usize).min(f.text.len());
        let end = (span.end as usize).clamp(start, f.text.len());
        &f.text[start..end]
    }

    #[must_use]
    pub fn line_col(&self, span: Span) -> LineCol {
        self.file(span.file).line_col(span.start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(text: &str) -> (SourceMap, FileId) {
        let mut m = SourceMap::new();
        let id = m.add("t.vise", text);
        (m, id)
    }

    #[test]
    fn first_character_is_line_one_column_one() {
        let (m, f) = map("abc\ndef\n");
        assert_eq!(m.file(f).line_col(0), LineCol { line: 1, col: 1 });
    }

    #[test]
    fn offsets_resolve_across_lines() {
        let (m, f) = map("abc\ndef\n");
        assert_eq!(m.file(f).line_col(2), LineCol { line: 1, col: 3 });
        assert_eq!(m.file(f).line_col(4), LineCol { line: 2, col: 1 });
        assert_eq!(m.file(f).line_col(6), LineCol { line: 2, col: 3 });
    }

    #[test]
    fn newline_belongs_to_the_line_it_terminates() {
        let (m, f) = map("abc\ndef\n");
        assert_eq!(m.file(f).line_col(3), LineCol { line: 1, col: 4 });
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        // Bytes: h=0, é=1..3, l=3, l=4, o=5. Byte 3 is the first 'l',
        // which is the third *character*.
        let (m, f) = map("héllo");
        assert_eq!(m.file(f).line_col(3), LineCol { line: 1, col: 3 });
    }

    #[test]
    fn offsets_past_end_clamp_instead_of_panicking() {
        let (m, f) = map("abc");
        assert_eq!(m.file(f).line_col(999), LineCol { line: 1, col: 4 });
    }

    #[test]
    fn empty_file_has_one_line() {
        let (m, f) = map("");
        assert_eq!(m.file(f).line_count(), 1);
        assert_eq!(m.file(f).line_col(0), LineCol { line: 1, col: 1 });
    }

    #[test]
    fn line_text_strips_terminators_and_is_one_based() {
        let (m, f) = map("abc\r\ndef\n");
        assert_eq!(m.file(f).line_text(1), Some("abc"));
        assert_eq!(m.file(f).line_text(2), Some("def"));
        assert_eq!(m.file(f).line_text(0), None);
        assert_eq!(m.file(f).line_text(9), None);
    }

    #[test]
    fn snippet_returns_the_covered_text() {
        let (m, f) = map("module greet");
        assert_eq!(m.snippet(Span::new(f, 7, 12)), "greet");
    }

    #[test]
    fn snippet_clamps_an_out_of_range_span() {
        let (m, f) = map("abc");
        assert_eq!(m.snippet(Span::new(f, 1, 99)), "bc");
    }

    #[test]
    fn spans_join_to_the_smallest_cover() {
        let f = FileId(0);
        let joined = Span::new(f, 10, 12).to(Span::new(f, 3, 5));
        assert_eq!(joined, Span::new(f, 3, 12));
    }
}
