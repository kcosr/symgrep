//! Language backend registry and implementations.
//!
//! This module defines the `LanguageBackend` trait plus a small
//! registry that maps file extensions and logical language IDs to
//! backend implementations.
//!
//! Phase 2 introduces minimal tree-sitter based backends for
//! TypeScript/TSX and JavaScript/JSX. Later phases will extend these
//! backends with symbol indexing and richer context/snippet support.

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use tree_sitter::{Node, Point, Tree};

use crate::models::{ContextInfo, ContextKind, Symbol, TextRange};

mod cpp;
mod javascript;
mod rust;
mod typescript;

/// Minimal error type for language backends.
///
/// This keeps details internal to the language layer while allowing
/// callers to distinguish backend failures from other errors.
///
/// TODO(phase3): Consider enriching this into an enum that can
/// distinguish parse failures, syntax errors with locations, and I/O
/// errors so that search/index layers can report richer diagnostics
/// without leaking tree-sitter specifics.
#[derive(Debug)]
pub struct BackendError {
    message: String,
}

impl BackendError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "language backend error: {}", self.message)
    }
}

impl Error for BackendError {}

impl From<tree_sitter::LanguageError> for BackendError {
    fn from(err: tree_sitter::LanguageError) -> Self {
        BackendError::new(err.to_string())
    }
}

/// Convenience result type used throughout the language layer.
pub type BackendResult<T> = Result<T, BackendError>;

/// Parsed representation of a single source file.
///
/// For Phase 2 this is a thin wrapper around a `tree_sitter::Tree`
/// plus some basic metadata. Later phases may extend this struct with
/// symbol tables or indexing metadata.
#[derive(Debug)]
pub struct ParsedFile {
    /// Stable logical language identifier (e.g., "typescript").
    pub language_id: &'static str,
    /// Path of the parsed file, if known.
    pub path: PathBuf,
    /// The underlying tree-sitter syntax tree.
    pub tree: Tree,
    /// Full source text for this file.
    pub source: String,
}

impl ParsedFile {
    /// Helper to construct a new `ParsedFile`.
    pub fn new(language_id: &'static str, path: &Path, tree: Tree, source: String) -> Self {
        Self {
            language_id,
            path: path.to_path_buf(),
            tree,
            source,
        }
    }

    /// Kind of the root node in the syntax tree.
    pub fn root_kind(&self) -> String {
        self.tree.root_node().kind().to_string()
    }

    /// Whether the root node (or its descendants) contain parse errors.
    pub fn has_errors(&self) -> bool {
        self.tree.root_node().has_error()
    }

    /// Borrow the underlying source text.
    pub fn source(&self) -> &str {
        &self.source
    }
}

/// Convert a tree-sitter `Point` (0-based row/column) into a
/// user-facing 1-based line/column pair.
fn point_to_position(p: Point) -> (u32, u32) {
    (p.row as u32 + 1, p.column as u32 + 1)
}

/// Compute a `TextRange` for a given syntax node.
pub(crate) fn node_text_range(node: &Node) -> TextRange {
    let start = node.start_position();
    let end = node.end_position();
    let (start_line, start_column) = point_to_position(start);
    let (end_line, end_column) = point_to_position(end);

    TextRange {
        start_line,
        start_column,
        end_line,
        end_column,
    }
}

/// Convert a `TextRange` (1-based positions) back into tree-sitter
/// points (0-based row/column).
fn text_range_to_points(range: &TextRange) -> (Point, Point) {
    let start = Point {
        row: range.start_line.saturating_sub(1) as usize,
        column: range.start_column.saturating_sub(1) as usize,
    };
    let end = Point {
        row: range.end_line.saturating_sub(1) as usize,
        column: range.end_column.saturating_sub(1) as usize,
    };
    (start, end)
}

/// Locate the syntax node corresponding to a symbol's recorded
/// `TextRange` using `descendant_for_point_range`.
pub(crate) fn find_symbol_node<'a>(file: &'a ParsedFile, symbol: &Symbol) -> Option<Node<'a>> {
    let root = file.tree.root_node();
    let (start, end) = text_range_to_points(&symbol.range);
    root.descendant_for_point_range(start, end)
}

/// Construct a file-level context node for use as the outermost entry
/// in a parent chain.
pub(crate) fn file_context_node(file: &ParsedFile) -> crate::models::ContextNode {
    let name = file
        .path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    crate::models::ContextNode { name, kind: None }
}

/// Helper to build a `ContextInfo` from an arbitrary range in a file.
pub(crate) fn context_snippet_for_range(
    file: &ParsedFile,
    symbol_file: &Path,
    kind: ContextKind,
    range: TextRange,
) -> ContextInfo {
    let source = file.source();
    let lines: Vec<&str> = source.lines().collect();

    let start_line = range.start_line;
    let end_line = range.end_line;

    let start_idx = start_line.saturating_sub(1) as usize;
    let mut end_idx = end_line.saturating_sub(1) as usize;

    if lines.is_empty() || start_idx >= lines.len() {
        return ContextInfo {
            kind,
            file: symbol_file.to_path_buf(),
            range,
            snippet: String::new(),
            symbol_index: None,
            parent_chain: Vec::new(),
        };
    }

    if end_idx >= lines.len() {
        end_idx = lines.len() - 1;
    }

    let snippet = lines[start_idx..=end_idx].join("\n");

    ContextInfo {
        kind,
        file: symbol_file.to_path_buf(),
        range,
        snippet,
        symbol_index: None,
        parent_chain: Vec::new(),
    }
}

/// Internal classification of a single line when collecting leading
/// comments for a symbol.
enum CommentLineKind {
    /// Actual content line within a comment block.
    Content(String),
    /// Comment delimiter or decorative line (e.g. `/**`, `*/`, `*`).
    Delimiter,
    /// Non-comment line.
    NotComment,
}

fn classify_comment_line(line: &str) -> CommentLineKind {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return CommentLineKind::NotComment;
    }

    if trimmed.starts_with("//") {
        let body = trimmed.trim_start_matches('/').trim_start_matches('/').trim();
        if body.is_empty() {
            CommentLineKind::Delimiter
        } else {
            CommentLineKind::Content(body.to_string())
        }
    } else if trimmed.starts_with("/*") {
        let mut body = trimmed.trim_start_matches("/*").trim_start_matches('*').trim();
        if body.ends_with("*/") {
            body = body.trim_end_matches("*/").trim();
        }
        if body.is_empty() {
            CommentLineKind::Delimiter
        } else {
            CommentLineKind::Content(body.to_string())
        }
    } else if trimmed.starts_with('*') {
        let body = trimmed.trim_start_matches('*').trim();
        if body.is_empty() {
            CommentLineKind::Delimiter
        } else {
            CommentLineKind::Content(body.to_string())
        }
    } else {
        CommentLineKind::NotComment
    }
}

/// Collect leading comment lines immediately preceding a symbol,
/// returning both normalized text and the original source range.
///
/// This walks upward from `start_line - 1`, skipping over decorator
/// or attribute lines as defined by `is_decorator_line`, and
/// aggregating contiguous comment lines. The walk stops on the first
/// blank line or non-comment, non-decorator line.
///
/// The returned `TextRange` covers the full comment block in the
/// original source (including delimiters and indentation), while the
/// normalized text strips comment delimiters for use in queries and
/// JSON APIs.
pub(crate) fn collect_leading_comment<F>(
    source: &str,
    start_line: u32,
    is_decorator_line: F,
) -> Option<(String, TextRange)>
where
    F: Fn(&str) -> bool,
{
    if start_line <= 1 {
        return None;
    }

    let lines: Vec<&str> = source.lines().collect();
    if lines.is_empty() {
        return None;
    }

    let mut idx = start_line.saturating_sub(1) as usize;
    if idx == 0 {
        return None;
    }
    idx = idx.saturating_sub(1);

    let mut collected: Vec<String> = Vec::new();
    let mut saw_any = false;
    let mut min_idx: Option<usize> = None;
    let mut max_idx: Option<usize> = None;

    loop {
        if idx >= lines.len() {
            break;
        }

        let line = lines[idx];
        let trimmed = line.trim_end();

        if trimmed.trim().is_empty() {
            if saw_any {
                break;
            } else {
                // Blank line immediately above the symbol with no
                // comments/decorators – treat this as separating
                // header comments from symbol comments.
                break;
            }
        }

        if is_decorator_line(trimmed) {
            saw_any = true;
            if idx == 0 {
                break;
            }
            idx = idx.saturating_sub(1);
            continue;
        }

        match classify_comment_line(trimmed) {
            CommentLineKind::Content(text) => {
                saw_any = true;
                collected.push(text);
                min_idx = Some(min_idx.map_or(idx, |current| current.min(idx)));
                max_idx = Some(max_idx.map_or(idx, |current| current.max(idx)));
                if idx == 0 {
                    break;
                }
                idx = idx.saturating_sub(1);
            }
            CommentLineKind::Delimiter => {
                saw_any = true;
                min_idx = Some(min_idx.map_or(idx, |current| current.min(idx)));
                max_idx = Some(max_idx.map_or(idx, |current| current.max(idx)));
                if idx == 0 {
                    break;
                }
                idx = idx.saturating_sub(1);
            }
            CommentLineKind::NotComment => break,
        }
    }

    if collected.is_empty() {
        None
    } else {
        collected.reverse();
        let text = collected.join("\n");

        let start_idx = min_idx.unwrap_or(0);
        let end_idx = max_idx.unwrap_or(start_idx);
        let start_line_range = start_idx as u32 + 1;
        let end_line_range = end_idx as u32 + 1;
        let end_text = lines.get(end_idx).copied().unwrap_or_default();
        let end_column = end_text.len() as u32 + 1;

        let range = TextRange {
            start_line: start_line_range,
            start_column: 1,
            end_line: end_line_range,
            end_column,
        };

        Some((text, range))
    }
}

/// Helper to construct a basic context snippet for a symbol using its
/// recorded `TextRange`.
pub(crate) fn basic_context_snippet(
    file: &ParsedFile,
    symbol: &Symbol,
    kind: ContextKind,
) -> ContextInfo {
    let source = file.source();
    let lines: Vec<&str> = source.lines().collect();

    let (start_line, end_line) = match kind {
        ContextKind::Decl => {
            let line = symbol.range.start_line;
            (line, line)
        }
        ContextKind::Def | ContextKind::Parent => (symbol.range.start_line, symbol.range.end_line),
    };

    let start_idx = start_line.saturating_sub(1) as usize;
    let mut end_idx = end_line.saturating_sub(1) as usize;

    if lines.is_empty() || start_idx >= lines.len() {
        return ContextInfo {
            kind,
            file: symbol.file.clone(),
            range: symbol.range,
            snippet: String::new(),
            symbol_index: None,
            parent_chain: Vec::new(),
        };
    }

    if end_idx >= lines.len() {
        end_idx = lines.len() - 1;
    }

    let snippet = lines[start_idx..=end_idx].join("\n");

    let range = if matches!(kind, ContextKind::Decl) {
        // Narrow the range to a single line for declarations.
        let line_text = lines.get(start_idx).copied().unwrap_or_default();
        crate::models::TextRange {
            start_line,
            start_column: 1,
            end_line: start_line,
            end_column: line_text.len() as u32 + 1,
        }
    } else {
        symbol.range
    };

    ContextInfo {
        kind,
        file: symbol.file.clone(),
        range,
        snippet,
        symbol_index: None,
        parent_chain: Vec::new(),
    }
}

/// Common interface implemented by all language backends.
///
/// The trait is intentionally minimal in Phase 2: it focuses on
/// identifying the backend, advertising supported file extensions,
/// and parsing a file into a tree-sitter syntax tree. Symbol
/// extraction and richer context APIs will be added in later phases.
pub trait LanguageBackend: Sync + Send {
    /// Stable language identifier (e.g., "typescript", "javascript").
    fn id(&self) -> &'static str;

    /// File extensions (without leading dots) handled by this backend.
    ///
    /// Examples: `["ts", "tsx"]`, `["js", "jsx"]`.
    fn file_extensions(&self) -> &'static [&'static str];

    /// Parse a file's source into a `ParsedFile`.
    ///
    /// Implementations should return an error if tree-sitter fails to
    /// produce a tree or if the resulting tree contains parse errors.
    fn parse_file(&self, path: &Path, source: &str) -> BackendResult<ParsedFile>;

    /// Index symbols for a parsed file.
    ///
    /// The default implementation returns an empty list so that
    /// backends can opt into symbol support incrementally.
    fn index_symbols(&self, _file: &ParsedFile) -> BackendResult<Vec<Symbol>> {
        Ok(Vec::new())
    }

    /// Return a context snippet for a given symbol.
    ///
    /// By default this returns an error; backends that implement
    /// symbol extraction should also provide appropriate context
    /// snippets for the symbol kinds they support.
    fn get_context_snippet(
        &self,
        _file: &ParsedFile,
        _symbol: &Symbol,
        _kind: ContextKind,
    ) -> BackendResult<ContextInfo> {
        Err(BackendError::new(
            "context snippets are not implemented for this language",
        ))
    }
}

/// All statically-registered backends.
///
/// This array is used by the registry helpers below; new language
/// backends should be added here.
static BACKENDS: [&'static dyn LanguageBackend; 4] = [
    &typescript::BACKEND,
    &javascript::BACKEND,
    &cpp::BACKEND,
    &rust::BACKEND,
];

/// Look up a backend by file path, using the extension to infer
/// language.
///
/// The lookup is case-insensitive and only considers the last
/// component of the file name.
pub fn backend_for_path(path: &Path) -> Option<&'static dyn LanguageBackend> {
    let ext = path.extension()?.to_str()?;
    let ext = ext.to_ascii_lowercase();

    BACKENDS.iter().copied().find(|backend| {
        backend
            .file_extensions()
            .iter()
            .any(|e| e.eq_ignore_ascii_case(&ext))
    })
}

/// Look up a backend by logical language identifier.
///
/// Identifiers are compared case-insensitively. Common aliases like
/// `"ts"`/`"tsx"` and `"js"`/`"jsx"` are normalized to their canonical
/// backend IDs.
pub fn backend_for_language(id: &str) -> Option<&'static dyn LanguageBackend> {
    let id = id.to_ascii_lowercase();
    let canonical = match id.as_str() {
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "cpp" | "c++" => "cpp",
        "rs" => "rust",
        other => other,
    };

    BACKENDS
        .iter()
        .copied()
        .find(|backend| backend.id().eq_ignore_ascii_case(canonical))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ContextKind, SymbolKind};
    use std::fs;
    use std::path::PathBuf;

    fn fixture(name: &str) -> (PathBuf, String) {
        let path = PathBuf::from("tests/fixtures/ts_js_repo").join(name);
        let source = fs::read_to_string(&path).expect("fixture source");
        (path, source)
    }

    fn cpp_fixture(name: &str) -> (PathBuf, String) {
        let path = PathBuf::from("tests/fixtures/cpp_repo").join(name);
        let source = fs::read_to_string(&path).expect("fixture source");
        (path, source)
    }

    fn rust_fixture(name: &str) -> (PathBuf, String) {
        let path = PathBuf::from("tests/fixtures/rust_repo").join(name);
        let source = fs::read_to_string(&path).expect("fixture source");
        (path, source)
    }

    fn call_fixture(name: &str) -> (PathBuf, String) {
        let path = PathBuf::from("tests/fixtures/call_graph_repo").join(name);
        let source = fs::read_to_string(&path).expect("fixture source");
        (path, source)
    }

    #[test]
    fn registry_maps_extensions_to_backends() {
        let ts_path = Path::new("src/lib.ts");
        let tsx_path = Path::new("src/component.tsx");
        let js_path = Path::new("src/index.js");
        let jsx_path = Path::new("src/app.jsx");
        let rs_path = Path::new("src/lib.rs");
        let cpp_path = Path::new("src/main.cpp");
        let cc_path = Path::new("src/main.cc");
        let cxx_path = Path::new("src/main.cxx");
        let hpp_path = Path::new("src/main.hpp");
        let hh_path = Path::new("src/main.hh");
        let hxx_path = Path::new("src/main.hxx");

        assert_eq!(backend_for_path(ts_path).unwrap().id(), "typescript");
        assert_eq!(backend_for_path(tsx_path).unwrap().id(), "typescript");
        assert_eq!(backend_for_path(js_path).unwrap().id(), "javascript");
        assert_eq!(backend_for_path(jsx_path).unwrap().id(), "javascript");
        assert_eq!(backend_for_path(rs_path).unwrap().id(), "rust");
        assert_eq!(backend_for_path(cpp_path).unwrap().id(), "cpp");
        assert_eq!(backend_for_path(cc_path).unwrap().id(), "cpp");
        assert_eq!(backend_for_path(cxx_path).unwrap().id(), "cpp");
        assert_eq!(backend_for_path(hpp_path).unwrap().id(), "cpp");
        assert_eq!(backend_for_path(hh_path).unwrap().id(), "cpp");
        assert_eq!(backend_for_path(hxx_path).unwrap().id(), "cpp");
    }

    #[test]
    fn registry_maps_language_ids_to_backends() {
        assert_eq!(
            backend_for_language("typescript").unwrap().id(),
            "typescript"
        );
        assert_eq!(backend_for_language("ts").unwrap().id(), "typescript");
        assert_eq!(backend_for_language("tsx").unwrap().id(), "typescript");

        assert_eq!(
            backend_for_language("javascript").unwrap().id(),
            "javascript"
        );
        assert_eq!(backend_for_language("js").unwrap().id(), "javascript");
        assert_eq!(backend_for_language("jsx").unwrap().id(), "javascript");

        assert_eq!(backend_for_language("cpp").unwrap().id(), "cpp");
        assert_eq!(backend_for_language("c++").unwrap().id(), "cpp");

        assert_eq!(backend_for_language("rust").unwrap().id(), "rust");
        assert_eq!(backend_for_language("rs").unwrap().id(), "rust");
    }

    #[test]
    fn cpp_backend_parses_fixture() {
        let (path, source) = cpp_fixture("sample.cpp");
        let backend = backend_for_language("cpp").unwrap();
        let parsed = backend.parse_file(&path, &source).expect("parsed");

        assert_eq!(parsed.language_id, "cpp");
        assert!(!parsed.has_errors());
        assert!(!parsed.root_kind().is_empty());
    }

    #[test]
    fn cpp_backend_indexes_basic_symbols() {
        let (path, source) = cpp_fixture("sample.cpp");
        let backend = backend_for_language("cpp").unwrap();
        let parsed = backend.parse_file(&path, &source).expect("parsed");

        let symbols = backend.index_symbols(&parsed).expect("symbols");

        assert!(symbols
            .iter()
            .any(|s| s.name == "add" && s.kind == SymbolKind::Function));
        assert!(symbols
            .iter()
            .any(|s| s.name == "Widget" && s.kind == SymbolKind::Class));
        assert!(symbols
            .iter()
            .any(|s| s.name == "util" && s.kind == SymbolKind::Namespace));
        assert!(symbols.iter().all(|s| s.language == "cpp"));
    }

    #[test]
    fn cpp_backend_context_snippets_for_decl_and_def() {
        let (path, source) = cpp_fixture("sample.cpp");
        let backend = backend_for_language("cpp").unwrap();
        let parsed = backend.parse_file(&path, &source).expect("parsed");

        let symbols = backend.index_symbols(&parsed).expect("symbols");
        let symbol = symbols
            .iter()
            .find(|s| s.name == "add")
            .expect("add symbol");

        let decl = backend
            .get_context_snippet(&parsed, symbol, ContextKind::Decl)
            .expect("decl context");
        let def = backend
            .get_context_snippet(&parsed, symbol, ContextKind::Def)
            .expect("def context");

        assert!(decl.snippet.contains("int add"));
        assert!(def.snippet.contains("return a + b;"));
        assert!(def.snippet.lines().count() >= decl.snippet.lines().count());
    }

    #[test]
    fn cpp_backend_decl_context_includes_multiline_signature() {
        let (path, source) = cpp_fixture("sample.cpp");
        let backend = backend_for_language("cpp").unwrap();
        let parsed = backend.parse_file(&path, &source).expect("parsed");

        let symbols = backend.index_symbols(&parsed).expect("symbols");
        let symbol = symbols
            .iter()
            .find(|s| s.name == "multiline_function")
            .expect("multiline_function symbol");

        let decl = backend
            .get_context_snippet(&parsed, symbol, ContextKind::Decl)
            .expect("decl context");

        let snippet = decl.snippet.as_str();
        assert!(
            snippet.lines().count() >= 3,
            "expected multi-line signature in decl snippet"
        );
        assert!(
            snippet.contains("void"),
            "expected decl snippet to include return type"
        );
        assert!(
            snippet.contains("multiline_function"),
            "expected decl snippet to include function name"
        );
        assert!(
            snippet.contains("int a") && snippet.contains("int b"),
            "expected decl snippet to include parameters"
        );
    }

    #[test]
    fn typescript_backend_parses_fixture() {
        let (path, source) = fixture("simple.ts");
        let backend = backend_for_language("typescript").unwrap();
        let parsed = backend.parse_file(&path, &source).expect("parsed");

        assert_eq!(parsed.language_id, "typescript");
        assert!(!parsed.has_errors());
        assert!(!parsed.root_kind().is_empty());
    }

    #[test]
    fn javascript_backend_parses_fixture() {
        let (path, source) = fixture("simple.js");
        let backend = backend_for_language("javascript").unwrap();
        let parsed = backend.parse_file(&path, &source).expect("parsed");

        assert_eq!(parsed.language_id, "javascript");
        assert!(!parsed.has_errors());
        assert!(!parsed.root_kind().is_empty());
    }

    #[test]
    fn typescript_backend_parses_tsx_fixture() {
        let (path, source) = fixture("component.tsx");
        let backend = backend_for_language("typescript").unwrap();
        let parsed = backend.parse_file(&path, &source).expect("parsed");

        assert_eq!(parsed.language_id, "typescript");
        assert!(!parsed.has_errors());
    }

    #[test]
    fn javascript_backend_parses_jsx_fixture() {
        let (path, source) = fixture("component.jsx");
        let backend = backend_for_language("javascript").unwrap();
        let parsed = backend.parse_file(&path, &source).expect("parsed");

        assert_eq!(parsed.language_id, "javascript");
        assert!(!parsed.has_errors());
    }

    #[test]
    fn rust_backend_parses_fixture() {
        let (path, source) = rust_fixture("lib.rs");
        let backend = backend_for_language("rust").unwrap();
        let parsed = backend.parse_file(&path, &source).expect("parsed");

        assert_eq!(parsed.language_id, "rust");
        assert!(!parsed.has_errors());
        assert!(!parsed.root_kind().is_empty());
    }

    #[test]
    fn typescript_backend_indexes_basic_symbols() {
        let (path, source) = fixture("simple.ts");
        let backend = backend_for_language("typescript").unwrap();
        let parsed = backend.parse_file(&path, &source).expect("parsed");

        let symbols = backend.index_symbols(&parsed).expect("symbols");

        assert!(symbols
            .iter()
            .any(|s| s.name == "add" && s.kind == SymbolKind::Function));
        assert!(symbols.iter().all(|s| s.language == "typescript"));
    }

    #[test]
    fn javascript_backend_indexes_basic_symbols() {
        let (path, source) = fixture("simple.js");
        let backend = backend_for_language("javascript").unwrap();
        let parsed = backend.parse_file(&path, &source).expect("parsed");

        let symbols = backend.index_symbols(&parsed).expect("symbols");

        assert!(symbols
            .iter()
            .any(|s| s.name == "add" && s.kind == SymbolKind::Function));
        assert!(symbols.iter().all(|s| s.language == "javascript"));
    }

    #[test]
    fn rust_backend_indexes_basic_symbols() {
        let (path, source) = rust_fixture("lib.rs");
        let backend = backend_for_language("rust").unwrap();
        let parsed = backend.parse_file(&path, &source).expect("parsed");

        let symbols = backend.index_symbols(&parsed).expect("symbols");

        assert!(symbols
            .iter()
            .any(|s| s.name == "add" && s.kind == SymbolKind::Function));
        assert!(symbols
            .iter()
            .any(|s| s.name == "new" && s.kind == SymbolKind::Function));
        assert!(symbols
            .iter()
            .any(|s| s.name == "increment" && s.kind == SymbolKind::Method));
        assert!(symbols
            .iter()
            .any(|s| s.name == "Widget" && s.kind == SymbolKind::Class));
        assert!(symbols
            .iter()
            .any(|s| s.name == "Greeter" && s.kind == SymbolKind::Interface));
        assert!(symbols
            .iter()
            .any(|s| s.name == "my_mod" && s.kind == SymbolKind::Namespace));
        assert!(symbols.iter().all(|s| s.language == "rust"));
    }

    #[test]
    fn typescript_backend_context_snippets_for_decl_and_def() {
        let (path, source) = fixture("simple.ts");
        let backend = backend_for_language("typescript").unwrap();
        let parsed = backend.parse_file(&path, &source).expect("parsed");

        let symbols = backend.index_symbols(&parsed).expect("symbols");
        let symbol = symbols
            .iter()
            .find(|s| s.name == "add")
            .expect("add symbol");

        let decl = backend
            .get_context_snippet(&parsed, symbol, ContextKind::Decl)
            .expect("decl context");
        let def = backend
            .get_context_snippet(&parsed, symbol, ContextKind::Def)
            .expect("def context");

        assert!(decl.snippet.contains("export function add"));
        assert!(def.snippet.contains("return a + b;"));
        assert!(def.snippet.lines().count() >= decl.snippet.lines().count());
    }

    #[test]
    fn javascript_backend_context_snippets_for_decl_and_def() {
        let (path, source) = fixture("simple.js");
        let backend = backend_for_language("javascript").unwrap();
        let parsed = backend.parse_file(&path, &source).expect("parsed");

        let symbols = backend.index_symbols(&parsed).expect("symbols");
        let symbol = symbols
            .iter()
            .find(|s| s.name == "add")
            .expect("add symbol");

        let decl = backend
            .get_context_snippet(&parsed, symbol, ContextKind::Decl)
            .expect("decl context");
        let def = backend
            .get_context_snippet(&parsed, symbol, ContextKind::Def)
            .expect("def context");

        assert!(decl.snippet.contains("function add"));
        assert!(def.snippet.contains("return a + b;"));
        assert!(def.snippet.lines().count() >= decl.snippet.lines().count());
    }

    #[test]
    fn typescript_backend_parent_context_includes_file_in_parent_chain() {
        let (path, source) = fixture("simple.ts");
        let backend = backend_for_language("typescript").unwrap();
        let parsed = backend.parse_file(&path, &source).expect("parsed");

        let symbols = backend.index_symbols(&parsed).expect("symbols");
        let symbol = symbols
            .iter()
            .find(|s| s.name == "add")
            .expect("add symbol");

        let context = backend
            .get_context_snippet(&parsed, symbol, ContextKind::Parent)
            .expect("parent context");

        assert!(
            !context.parent_chain.is_empty(),
            "expected non-empty parent_chain for TypeScript symbol"
        );
        assert_eq!(context.parent_chain[0].name, "simple.ts");
        assert!(context.snippet.contains("export function add"));
    }

    #[test]
    fn javascript_backend_parent_context_includes_file_in_parent_chain() {
        let (path, source) = fixture("simple.js");
        let backend = backend_for_language("javascript").unwrap();
        let parsed = backend.parse_file(&path, &source).expect("parsed");

        let symbols = backend.index_symbols(&parsed).expect("symbols");
        let symbol = symbols
            .iter()
            .find(|s| s.name == "add")
            .expect("add symbol");

        let context = backend
            .get_context_snippet(&parsed, symbol, ContextKind::Parent)
            .expect("parent context");

        assert!(
            !context.parent_chain.is_empty(),
            "expected non-empty parent_chain for JavaScript symbol"
        );
        assert_eq!(context.parent_chain[0].name, "simple.js");
        assert!(context.snippet.contains("function add"));
    }

    #[test]
    fn cpp_backend_parent_context_builds_namespace_and_class_chain() {
        let (path, source) = cpp_fixture("sample.cpp");
        let backend = backend_for_language("cpp").unwrap();
        let parsed = backend.parse_file(&path, &source).expect("parsed");

        let symbols = backend.index_symbols(&parsed).expect("symbols");
        let symbol = symbols
            .iter()
            .find(|s| s.name == "increment")
            .expect("increment symbol");

        let context = backend
            .get_context_snippet(&parsed, symbol, ContextKind::Parent)
            .expect("parent context");

        assert!(
            context.parent_chain.len() >= 3,
            "expected file, namespace and class in parent_chain"
        );

        let names: Vec<&str> = context
            .parent_chain
            .iter()
            .map(|n| n.name.as_str())
            .collect();

        assert_eq!(names[0], "sample.cpp");
        assert!(
            names.iter().any(|n| *n == "util"),
            "expected namespace 'util' in parent_chain"
        );
        assert!(
            names.iter().any(|n| *n == "Widget"),
            "expected class 'Widget' in parent_chain"
        );

        assert!(context.snippet.contains("struct Widget"));
        assert!(context.snippet.contains("int increment"));
    }

    #[test]
    fn rust_backend_parent_context_builds_module_and_type_chain() {
        let (path, source) = rust_fixture("lib.rs");
        let backend = backend_for_language("rust").unwrap();
        let parsed = backend.parse_file(&path, &source).expect("parsed");

        let symbols = backend.index_symbols(&parsed).expect("symbols");
        let symbol = symbols
            .iter()
            .find(|s| s.name == "increment" && s.kind == SymbolKind::Method)
            .expect("increment symbol");

        let context = backend
            .get_context_snippet(&parsed, symbol, ContextKind::Parent)
            .expect("parent context");

        assert!(
            context.parent_chain.len() >= 3,
            "expected file, module and type in parent_chain"
        );

        let names: Vec<&str> = context
            .parent_chain
            .iter()
            .map(|n| n.name.as_str())
            .collect();

        assert_eq!(names[0], "lib.rs");
        assert!(
            names.iter().any(|n| *n == "my_mod"),
            "expected module 'my_mod' in parent_chain"
        );
        assert!(
            names.iter().any(|n| *n == "Widget"),
            "expected type 'Widget' in parent_chain"
        );

        assert!(context.snippet.contains("impl Widget"));
        assert!(context.snippet.contains("fn increment"));
    }

    #[test]
    fn typescript_backend_attaches_leading_doc_comment() {
        let (path, source) = fixture("doc_comments.ts");
        let backend = backend_for_language("typescript").unwrap();
        let parsed = backend.parse_file(&path, &source).expect("parsed");

        let symbols = backend.index_symbols(&parsed).expect("symbols");
        let symbol = symbols
            .iter()
            .find(|s| s.name == "addWithDoc")
            .expect("addWithDoc symbol");

        let attrs = symbol.attributes.as_ref().expect("attributes");
        let comment = attrs.comment.as_ref().expect("comment");
        assert!(
            comment.contains("Adds two numbers"),
            "expected extracted comment to include doc text, got: {comment}"
        );
    }

    #[test]
    fn javascript_backend_attaches_leading_doc_comment() {
        let (path, source) = fixture("doc_comments.js");
        let backend = backend_for_language("javascript").unwrap();
        let parsed = backend.parse_file(&path, &source).expect("parsed");

        let symbols = backend.index_symbols(&parsed).expect("symbols");
        let symbol = symbols
            .iter()
            .find(|s| s.name == "addWithDoc")
            .expect("addWithDoc symbol");

        let attrs = symbol.attributes.as_ref().expect("attributes");
        let comment = attrs.comment.as_ref().expect("comment");
        assert!(
            comment.contains("Adds two numbers"),
            "expected extracted comment to include doc text, got: {comment}"
        );
    }

    #[test]
    fn cpp_backend_attaches_leading_doc_comment() {
        let (path, source) = cpp_fixture("doc_comments.cpp");
        let backend = backend_for_language("cpp").unwrap();
        let parsed = backend.parse_file(&path, &source).expect("parsed");

        let symbols = backend.index_symbols(&parsed).expect("symbols");
        let symbol = symbols
            .iter()
            .find(|s| s.name == "add_with_doc")
            .expect("add_with_doc symbol");

        let attrs = symbol.attributes.as_ref().expect("attributes");
        let comment = attrs.comment.as_ref().expect("comment");
        assert!(
            comment.contains("Adds two integers"),
            "expected extracted comment to include doc text, got: {comment}"
        );
    }

    #[test]
    fn rust_backend_attaches_leading_doc_comment() {
        let (path, source) = rust_fixture("lib.rs");
        let backend = backend_for_language("rust").unwrap();
        let parsed = backend.parse_file(&path, &source).expect("parsed");

        let symbols = backend.index_symbols(&parsed).expect("symbols");
        let symbol = symbols
            .iter()
            .find(|s| s.name == "add_with_doc")
            .expect("add_with_doc symbol");

        let attrs = symbol.attributes.as_ref().expect("attributes");
        let comment = attrs.comment.as_ref().expect("comment");
        assert!(
            comment.contains("Adds two integers"),
            "expected extracted comment to include doc text, got: {comment}"
        );
    }

    #[test]
    fn typescript_backend_populates_call_relationships() {
        let (path, source) = call_fixture("ts_calls.ts");
        let backend = backend_for_language("typescript").unwrap();
        let parsed = backend.parse_file(&path, &source).expect("parsed");

        let symbols = backend.index_symbols(&parsed).expect("symbols");

        let foo = symbols
            .iter()
            .find(|s| s.name == "foo" && s.kind == SymbolKind::Function)
            .expect("foo symbol");
        let qux = symbols
            .iter()
            .find(|s| s.name == "qux" && s.kind == SymbolKind::Function)
            .expect("qux symbol");
        let bar = symbols
            .iter()
            .find(|s| s.name == "bar" && s.kind == SymbolKind::Function)
            .expect("bar symbol");
        let baz = symbols
            .iter()
            .find(|s| s.name == "baz" && s.kind == SymbolKind::Function)
            .expect("baz symbol");

        let foo_calls: Vec<&str> = foo.calls.iter().map(|c| c.name.as_str()).collect();
        assert!(
            foo_calls.contains(&"bar") && foo_calls.contains(&"baz"),
            "expected foo to call bar and baz"
        );

        let bar_callers: Vec<&str> = bar.called_by.iter().map(|c| c.name.as_str()).collect();
        let baz_callers: Vec<&str> = baz.called_by.iter().map(|c| c.name.as_str()).collect();
        assert!(
            bar_callers.contains(&"foo") && baz_callers.contains(&"foo"),
            "expected bar and baz to be called by foo"
        );

        let qux_calls: Vec<&str> = qux.calls.iter().map(|c| c.name.as_str()).collect();
        assert!(
            qux_calls.contains(&"foo"),
            "expected qux to call foo"
        );

        let foo_callers: Vec<&str> = foo.called_by.iter().map(|c| c.name.as_str()).collect();
        assert!(
            foo_callers.contains(&"qux"),
            "expected foo to be called by qux"
        );
    }

    #[test]
    fn javascript_backend_populates_call_relationships() {
        let (path, source) = call_fixture("js_calls.js");
        let backend = backend_for_language("javascript").unwrap();
        let parsed = backend.parse_file(&path, &source).expect("parsed");

        let symbols = backend.index_symbols(&parsed).expect("symbols");

        let foo = symbols
            .iter()
            .find(|s| s.name == "foo" && s.kind == SymbolKind::Function)
            .expect("foo symbol");
        let qux = symbols
            .iter()
            .find(|s| s.name == "qux" && s.kind == SymbolKind::Function)
            .expect("qux symbol");
        let bar = symbols
            .iter()
            .find(|s| s.name == "bar" && s.kind == SymbolKind::Function)
            .expect("bar symbol");
        let baz = symbols
            .iter()
            .find(|s| s.name == "baz" && s.kind == SymbolKind::Function)
            .expect("baz symbol");

        let foo_calls: Vec<&str> = foo.calls.iter().map(|c| c.name.as_str()).collect();
        assert!(
            foo_calls.contains(&"bar") && foo_calls.contains(&"baz"),
            "expected foo to call bar and baz"
        );

        let bar_callers: Vec<&str> = bar.called_by.iter().map(|c| c.name.as_str()).collect();
        let baz_callers: Vec<&str> = baz.called_by.iter().map(|c| c.name.as_str()).collect();
        assert!(
            bar_callers.contains(&"foo") && baz_callers.contains(&"foo"),
            "expected bar and baz to be called by foo"
        );

        let qux_calls: Vec<&str> = qux.calls.iter().map(|c| c.name.as_str()).collect();
        assert!(
            qux_calls.contains(&"foo"),
            "expected qux to call foo"
        );

        let foo_callers: Vec<&str> = foo.called_by.iter().map(|c| c.name.as_str()).collect();
        assert!(
            foo_callers.contains(&"qux"),
            "expected foo to be called by qux"
        );
    }

    #[test]
    fn cpp_backend_populates_call_relationships() {
        let (path, source) = call_fixture("cpp_calls.cpp");
        let backend = backend_for_language("cpp").unwrap();
        let parsed = backend.parse_file(&path, &source).expect("parsed");

        let symbols = backend.index_symbols(&parsed).expect("symbols");

        let foo = symbols
            .iter()
            .find(|s| s.name == "foo" && s.kind == SymbolKind::Function)
            .expect("foo symbol");
        let qux = symbols
            .iter()
            .find(|s| s.name == "qux" && s.kind == SymbolKind::Function)
            .expect("qux symbol");
        let bar = symbols
            .iter()
            .find(|s| s.name == "bar" && s.kind == SymbolKind::Function)
            .expect("bar symbol");
        let baz = symbols
            .iter()
            .find(|s| s.name == "baz" && s.kind == SymbolKind::Function)
            .expect("baz symbol");

        let foo_calls: Vec<&str> = foo.calls.iter().map(|c| c.name.as_str()).collect();
        assert!(
            foo_calls.contains(&"bar") && foo_calls.contains(&"baz"),
            "expected foo to call bar and baz"
        );

        let bar_callers: Vec<&str> = bar.called_by.iter().map(|c| c.name.as_str()).collect();
        let baz_callers: Vec<&str> = baz.called_by.iter().map(|c| c.name.as_str()).collect();
        assert!(
            bar_callers.contains(&"foo") && baz_callers.contains(&"foo"),
            "expected bar and baz to be called by foo"
        );

        let qux_calls: Vec<&str> = qux.calls.iter().map(|c| c.name.as_str()).collect();
        assert!(
            qux_calls.contains(&"foo"),
            "expected qux to call foo"
        );

        let foo_callers: Vec<&str> = foo.called_by.iter().map(|c| c.name.as_str()).collect();
        assert!(
            foo_callers.contains(&"qux"),
            "expected foo to be called by qux"
        );
    }
}
