use std::cmp;

use anyhow::Result;

use crate::models::{ContextInfo, ContextNode, IndexSummary, SearchResult};

/// Internal representation of a row rendered by the CLI.
///
/// This is intentionally generic so that both text and table
/// formats can be derived from the same data.
struct DisplayRow {
    file: String,
    line: u32,
    column: Option<u32>,
    kind: String,
    name: String,
    context_name: Option<String>,
    snippet_lines: Vec<String>,
    is_symbol: bool,
}

/// Render a `SearchResult` in human-readable text form.
///
/// Symbol results are rendered as:
/// `path:line:col: kind name`
/// followed by indented snippet lines. Pure text results are
/// rendered with a similar header:
/// `path:line:col: text <pattern>`
/// followed by the matching line as an indented snippet.
pub fn print_text(result: &SearchResult) -> Result<()> {
    let rows = build_rows(result);

    for row in rows {
        let col_suffix = row.column.map(|c| format!(":{c}")).unwrap_or_default();

        if row.is_symbol {
            println!(
                "{}:{}{}: {} {}",
                row.file, row.line, col_suffix, row.kind, row.name
            );
            for snippet_line in row.snippet_lines {
                println!("    {snippet_line}");
            }
        } else {
            let snippet = row.snippet_lines.first().cloned().unwrap_or_default();
            println!("{}:{}{}: {}", row.file, row.line, col_suffix, snippet);
        }
    }

    Ok(())
}

/// Render an `IndexSummary` in human-readable text form.
pub fn print_index_summary_text(summary: &IndexSummary) -> Result<()> {
    let backend_str = match summary.backend {
        crate::models::IndexBackendKind::File => "file",
        crate::models::IndexBackendKind::Sqlite => "sqlite",
    };

    println!("backend      : {backend_str}");
    println!("index_path   : {}", summary.index_path.display());

    if let Some(root) = &summary.root_path {
        println!("root_path    : {root}");
    }
    if let Some(schema) = &summary.schema_version {
        println!("schema       : {schema}");
    }
    if let Some(tool) = &summary.tool_version {
        println!("tool_version : {tool}");
    }
    if let Some(created) = &summary.created_at {
        println!("created_at   : {created}");
    }
    if let Some(updated) = &summary.updated_at {
        println!("updated_at   : {updated}");
    }

    println!("files        : {}", summary.files_indexed);
    println!("symbols      : {}", summary.symbols_indexed);

    Ok(())
}

/// Render a `SearchResult` as a simple table.
///
/// Columns:
/// - FILE
/// - LINE
/// - KIND
/// - NAME
/// - CONTEXT (derived from parent_chain when available)
pub fn print_table(result: &SearchResult) -> Result<()> {
    let rows = build_rows(result);

    if rows.is_empty() {
        return Ok(());
    }

    const MAX_FILE_WIDTH: usize = 40;
    const MAX_NAME_WIDTH: usize = 30;
    const MAX_CONTEXT_WIDTH: usize = 40;

    let file_header = "FILE";
    let line_header = "LINE";
    let kind_header = "KIND";
    let name_header = "NAME";
    let context_header = "CONTEXT";

    let max_file_len = rows.iter().map(|r| r.file.len()).max().unwrap_or(0);
    let max_line_len = rows
        .iter()
        .map(|r| r.line.to_string().len())
        .max()
        .unwrap_or(0);
    let max_kind_len = rows.iter().map(|r| r.kind.len()).max().unwrap_or(0);
    let max_name_len = rows.iter().map(|r| r.name.len()).max().unwrap_or(0);
    let max_context_len = rows
        .iter()
        .map(|r| r.context_name.as_deref().unwrap_or("").len())
        .max()
        .unwrap_or(0);

    let file_width = cmp::min(cmp::max(file_header.len(), max_file_len), MAX_FILE_WIDTH);
    let line_width = cmp::max(line_header.len(), max_line_len);
    let kind_width = cmp::max(kind_header.len(), max_kind_len);
    let name_width = cmp::min(cmp::max(name_header.len(), max_name_len), MAX_NAME_WIDTH);
    let context_width = cmp::min(
        cmp::max(context_header.len(), max_context_len),
        MAX_CONTEXT_WIDTH,
    );

    println!(
        "{:<file_width$} {:>line_width$} {:<kind_width$} {:<name_width$} {:<context_width$}",
        file_header, line_header, kind_header, name_header, context_header
    );

    for row in rows {
        let file = truncate(&row.file, file_width);
        let line_str = row.line.to_string();
        let kind = truncate(&row.kind, kind_width);
        let name = truncate(&row.name, name_width);
        let context = truncate(row.context_name.as_deref().unwrap_or(""), context_width);

        println!(
            "{:<file_width$} {:>line_width$} {:<kind_width$} {:<name_width$} {:<context_width$}",
            file, line_str, kind, name, context
        );
    }

    Ok(())
}

fn build_rows(result: &SearchResult) -> Vec<DisplayRow> {
    if !result.symbols.is_empty() {
        build_symbol_rows(result)
    } else {
        build_text_rows(result)
    }
}

fn build_symbol_rows(result: &SearchResult) -> Vec<DisplayRow> {
    let mut rows = Vec::new();

    for (idx, symbol) in result.symbols.iter().enumerate() {
        let context = result.contexts.iter().find(|c| c.symbol_index == Some(idx));

        let snippet_lines = context_snippet_lines(context);
        let context_name = context.and_then(|c| derive_context_name(&c.parent_chain));

        rows.push(DisplayRow {
            file: symbol.file.display().to_string(),
            line: symbol.range.start_line,
            column: Some(symbol.range.start_column),
            kind: format!("{:?}", symbol.kind).to_lowercase(),
            name: symbol.name.clone(),
            context_name,
            snippet_lines,
            is_symbol: true,
        });
    }

    rows
}

fn build_text_rows(result: &SearchResult) -> Vec<DisplayRow> {
    result
        .matches
        .iter()
        .map(|m| DisplayRow {
            file: m.path.display().to_string(),
            line: m.line,
            column: m.column,
            kind: "text".to_string(),
            name: result.query.clone(),
            context_name: m.snippet.as_deref().map(|s| s.to_string()),
            snippet_lines: m
                .snippet
                .as_deref()
                .map(|s| vec![s.to_string()])
                .unwrap_or_default(),
            is_symbol: false,
        })
        .collect()
}

fn context_snippet_lines(context: Option<&ContextInfo>) -> Vec<String> {
    context
        .map(|c| c.snippet.lines().map(|s| s.to_string()).collect())
        .unwrap_or_default()
}

fn derive_context_name(chain: &[ContextNode]) -> Option<String> {
    if chain.is_empty() {
        None
    } else {
        // Skip purely file-level contexts (where the chain is a single
        // file-like node) to avoid duplicating the FILE column.
        if chain.len() == 1 && chain[0].kind.is_none() {
            None
        } else {
            Some(chain.last().expect("non-empty chain").name.clone())
        }
    }
}

fn truncate(s: &str, max_width: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_width {
        s.to_string()
    } else if max_width <= 1 {
        "…".to_string()
    } else {
        s.chars()
            .take(max_width.saturating_sub(1))
            .collect::<String>()
            + "…"
    }
}

#[cfg(test)]
mod tests {
    use super::truncate;

    #[test]
    fn truncate_leaves_short_strings_unchanged() {
        assert_eq!(truncate("abc", 5), "abc");
        assert_eq!(truncate("abc", 3), "abc");
    }

    #[test]
    fn truncate_ascii_strings_with_ellipsis() {
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("abcdef", 1), "…");
    }

    #[test]
    fn truncate_handles_unicode_characters() {
        let s = "éééé"; // multi-byte UTF-8 characters
        assert_eq!(truncate(s, 3), "éé…");
        assert_eq!(truncate(s, 2), "é…");
    }
}
