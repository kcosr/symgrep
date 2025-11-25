use std::cmp;
use std::collections::BTreeMap;

use anyhow::Result;

use crate::cli::args::{SearchArgs, SymbolViewArg};
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
/// When symbol views are specified via `--view`, symbol-mode output
/// is driven by those views (decl/def/parent/comment/matches).
/// Otherwise, legacy context-based behavior is used.
pub fn print_text(result: &SearchResult, args: &SearchArgs) -> Result<()> {
    if !result.symbols.is_empty() && !args.view.is_empty() {
        print_symbol_text_with_views(result, args)
    } else if result.symbols.is_empty()
        && args.context.unwrap_or(0) > 0
        && matches!(args.format, crate::cli::args::OutputFormat::Text)
    {
        print_text_mode_with_context(result, args)
    } else {
        let rows = build_rows(result);
        let max_lines = args.max_lines.unwrap_or(usize::MAX);

        for row in rows {
            let col_suffix = row.column.map(|c| format!(":{c}")).unwrap_or_default();

            if row.is_symbol {
                println!(
                    "{}:{}{}: {} {}",
                    row.file, row.line, col_suffix, row.kind, row.name
                );
                if max_lines > 0 {
                    for snippet_line in row.snippet_lines.iter().take(max_lines) {
                        println!("{snippet_line}");
                    }
                }
            } else {
                let snippet = row.snippet_lines.first().cloned().unwrap_or_default();
                println!("{}:{}{}: {}", row.file, row.line, col_suffix, snippet);
            }
        }

        Ok(())
    }
}

fn print_text_mode_with_context(
    result: &SearchResult,
    args: &SearchArgs,
) -> Result<()> {
    if result.matches.is_empty() {
        return Ok(());
    }

    let context = args.context.unwrap_or(0) as u32;
    let max_lines_per_file = args.max_lines.unwrap_or(usize::MAX);

    let mut matches_by_file: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for m in &result.matches {
        let path = m.path.display().to_string();
        matches_by_file.entry(path).or_default().push(m.line);
    }

    let mut first_file = true;

    for (file, lines) in matches_by_file {
        let source = match std::fs::read_to_string(&file) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let all_lines: Vec<&str> = source.lines().collect();
        if all_lines.is_empty() {
            continue;
        }

        let mut windows: Vec<(u32, u32)> = Vec::new();
        for line in lines {
            let start = line.saturating_sub(context).max(1);
            let end = (line + context).min(all_lines.len() as u32);
            windows.push((start, end));
        }

        if windows.is_empty() {
            continue;
        }

        windows.sort_by_key(|(start, _)| *start);
        let mut merged: Vec<(u32, u32)> = Vec::new();
        for (start, end) in windows {
            if let Some(last) = merged.last_mut() {
                if start <= last.1 + 1 {
                    last.1 = last.1.max(end);
                } else {
                    merged.push((start, end));
                }
            } else {
                merged.push((start, end));
            }
        }

        if !first_file {
            println!();
        }
        first_file = false;

        println!("{file}");

        if max_lines_per_file == 0 {
            continue;
        }

        let mut printed = 0usize;

        for (start, end) in merged {
            for line_no in start..=end {
                if printed >= max_lines_per_file {
                    break;
                }

                let idx = (line_no - 1) as usize;
                if idx >= all_lines.len() {
                    break;
                }

                let text = all_lines[idx];
                println!("{line_no}:  {text}");
                printed += 1;
            }

            if printed >= max_lines_per_file {
                break;
            }
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

fn print_symbol_text_with_views(result: &SearchResult, args: &SearchArgs) -> Result<()> {
    let show_comment = args
        .view
        .iter()
        .any(|v| matches!(v, SymbolViewArg::Comment));
    let show_matches = args
        .view
        .iter()
        .any(|v| matches!(v, SymbolViewArg::Matches));
    let has_region_view = args.view.iter().any(|v| {
        matches!(
            v,
            SymbolViewArg::Decl | SymbolViewArg::Def | SymbolViewArg::Parent
        )
    });
    let meta_only = args
        .view
        .iter()
        .any(|v| matches!(v, SymbolViewArg::Meta))
        && !has_region_view
        && !show_comment
        && !show_matches;
    let show_context =
        has_region_view || (!show_comment && !show_matches && !meta_only);
    let max_lines = args.max_lines.unwrap_or(usize::MAX);
    let context_lines = args.context.unwrap_or(0);

    for (idx, symbol) in result.symbols.iter().enumerate() {
        let file = symbol.file.display().to_string();
        let line = symbol.range.start_line;
        let col = Some(symbol.range.start_column);
        let col_suffix = col.map(|c| format!(":{c}")).unwrap_or_default();
        let kind = format!("{:?}", symbol.kind).to_lowercase();
        let def_suffix = symbol
            .def_line_count
            .map(|n| format!(" (def: {n} lines)"))
            .unwrap_or_default();

        println!("{file}:{line}{col_suffix}: {kind} {}{def_suffix}", symbol.name);

        if show_comment {
            if let Some(attrs) = &symbol.attributes {
                if let Some(range) = attrs.comment_range {
                    if let Ok(source) = std::fs::read_to_string(&symbol.file) {
                        let lines: Vec<&str> = source.lines().collect();
                        if !lines.is_empty() {
                            let start_idx =
                                range.start_line.saturating_sub(1) as usize;
                            let end_idx =
                                range.end_line.saturating_sub(1) as usize;
                            if start_idx < lines.len() && end_idx < lines.len() {
                                for idx in start_idx..=end_idx {
                                    println!("{}", lines[idx]);
                                }
                            }
                        }
                    }
                } else if let Some(comment) = &attrs.comment {
                    for line in comment.lines() {
                        println!("{line}");
                    }
                }
            }
        }

        let context = result
            .contexts
            .iter()
            .find(|c| c.symbol_index == Some(idx));

        if show_matches && context_lines > 0 {
            if let Some(ctx) = context {
                let snippet_lines: Vec<&str> = ctx.snippet.lines().collect();
                if !snippet_lines.is_empty() && !symbol.matches.is_empty() {
                    let base_line = ctx.range.start_line;
                    let last_index = snippet_lines.len().saturating_sub(1);

                    let mut windows: Vec<(usize, usize)> = Vec::new();
                    for m in &symbol.matches {
                        if m.line < base_line {
                            continue;
                        }
                        let rel = (m.line - base_line) as usize;
                        if rel > last_index {
                            continue;
                        }
                        let start = rel.saturating_sub(context_lines);
                        let end = cmp::min(rel + context_lines, last_index);
                        windows.push((start, end));
                    }

                    if !windows.is_empty() {
                        windows.sort_by_key(|(start, _)| *start);
                        let mut merged: Vec<(usize, usize)> = Vec::new();
                        for (start, end) in windows {
                            if let Some(last) = merged.last_mut() {
                                if start <= last.1 + 1 {
                                    last.1 = last.1.max(end);
                                } else {
                                    merged.push((start, end));
                                }
                            } else {
                                merged.push((start, end));
                            }
                        }

                        let mut printed = 0usize;
                        for (start, end) in merged {
                            for rel in start..=end {
                                if printed >= max_lines {
                                    break;
                                }

                                let abs_line = base_line + rel as u32;
                                let snippet_line = snippet_lines
                                    .get(rel)
                                    .copied()
                                    .unwrap_or_default();
                                println!("{abs_line}:  {snippet_line}");

                                printed += 1;
                            }

                            if printed >= max_lines {
                                break;
                            }
                        }
                    } else {
                        // No usable windows; fall back to legacy behavior.
                        for (printed, m) in symbol.matches.iter().enumerate() {
                            if printed >= max_lines {
                                break;
                            }
                            println!("{}:  {}", m.line, m.snippet);
                        }
                    }
                } else {
                    // No matches but a primary context is available; fall back
                    // to printing the context snippet, honoring max_lines.
                    for (idx, snippet_line) in ctx.snippet.lines().enumerate() {
                        if idx >= max_lines {
                            break;
                        }
                        println!("{snippet_line}");
                    }
                }
            } else if !symbol.matches.is_empty() {
                // No primary context; fall back to legacy match-only behavior.
                for (printed, m) in symbol.matches.iter().enumerate() {
                    if printed >= max_lines {
                        break;
                    }
                    println!("{}:  {}", m.line, m.snippet);
                }
            }
        } else if show_matches {
            if !symbol.matches.is_empty() {
                for (printed, m) in symbol.matches.iter().enumerate() {
                    if printed >= max_lines {
                        break;
                    }
                    println!("{}:  {}", m.line, m.snippet);
                }
            } else if let Some(ctx) = context {
                // Fallback: when no per-symbol matches are available,
                // print the primary context snippet instead of leaving
                // the body empty.
                for (idx, snippet_line) in ctx.snippet.lines().enumerate() {
                    if idx >= max_lines {
                        break;
                    }
                    println!("{snippet_line}");
                }
            }
        } else if show_context {
            if let Some(ctx) = context {
                for (idx, snippet_line) in ctx.snippet.lines().enumerate() {
                    if idx >= max_lines {
                        break;
                    }
                    println!("{snippet_line}");
                }
            }
        }

    }

    Ok(())
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
