//! Core search and index entry points.
//!
//! These functions provide the "search as a function" API used by the
//! CLI and, in later phases, the daemon/server.

use std::fs::{self, File};
use std::io::{BufRead, BufReader};

use anyhow::{bail, Result};
use globset::{Glob, GlobSet};
use ignore::WalkBuilder;

use crate::index::models::SymbolQuery;
use crate::index::open_backend;
use crate::language::{backend_for_language, backend_for_path};
use crate::models::{
    ContextKind, IndexConfig, IndexSummary, SearchConfig, SearchMatch, SearchMode, SearchResult,
    SearchSummary, SymbolMatch, SymbolView, SEARCH_RESULT_VERSION,
};
use crate::search::query::{
    expr_has_call_terms, expr_has_content_terms, parse_query_expr, symbol_matches_metadata,
    symbol_matches_with_text,
};

/// Execute a search based on the provided configuration.
///
/// Text mode behaves like a traditional grep, while symbol and auto
/// modes use language backends when available.
pub fn run_search(mut config: SearchConfig) -> Result<SearchResult> {
    if config.pattern.is_empty() {
        bail!("search pattern must not be empty");
    }

    if config.query_expr.is_none() {
        config.query_expr = parse_query_expr(&config.pattern);
    }

    let has_call_terms = config
        .query_expr
        .as_ref()
        .is_some_and(|expr| expr_has_call_terms(expr));

    let mode = effective_mode(&config);

    if config.reindex_on_search && matches!(mode, SearchMode::Symbol) && !has_call_terms {
        if let Some(index_cfg) = config.index.as_ref() {
            let effective_index =
                resolve_effective_index_config(&config).unwrap_or_else(|| index_cfg.clone());
            crate::index::run_index(effective_index)?;
        }
    }

    match mode {
        SearchMode::Text => run_text_search(config),
        SearchMode::Symbol => {
            if config.index.is_some() && !has_call_terms {
                run_symbol_search_with_index(config)
            } else {
                run_symbol_search_without_index(config)
            }
        }
        SearchMode::Auto => unreachable!("effective_mode never returns Auto"),
    }
}

fn effective_mode(config: &SearchConfig) -> SearchMode {
    match config.mode {
        SearchMode::Text => SearchMode::Text,
        SearchMode::Symbol => SearchMode::Symbol,
        SearchMode::Auto => {
            if let Some(lang) = &config.language {
                if backend_for_language(lang).is_some() {
                    return SearchMode::Symbol;
                }
            }
            SearchMode::Text
        }
    }
}

fn run_text_search(config: SearchConfig) -> Result<SearchResult> {
    let include_globs = build_globset(&config.globs)?;
    let exclude_globs = build_globset(&config.exclude_globs)?;

    if config.paths.is_empty() {
        bail!("at least one search path is required");
    }

    for path in &config.paths {
        if !path.exists() {
            bail!("search path does not exist: {}", path.display());
        }
    }

    let mut builder = WalkBuilder::new(&config.paths[0]);
    for path in config.paths.iter().skip(1) {
        builder.add(path);
    }

    let walker = builder.build();

    let mut matches = Vec::new();
    let mut total_matches: u64 = 0;
    let mut truncated = false;

    let limit = config.limit.unwrap_or(usize::MAX);

    // For text mode, we optionally interpret the pattern via the DSL
    // when it parses into a content-only expression. This enables
    // `foo|bar` OR semantics while preserving legacy behavior for
    // more complex patterns.
    let query_expr = config
        .query_expr
        .clone()
        .or_else(|| parse_query_expr(&config.pattern));

    let text_only_expr = query_expr.filter(|expr| expr_is_text_only(expr));

    'walk: for entry_result in walker {
        let entry = match entry_result {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();

        // Only search regular files.
        if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            continue;
        }

        // Apply inclusion/exclusion globs.
        if let Some(set) = &include_globs {
            if !set.is_match(path) {
                continue;
            }
        }
        if let Some(set) = &exclude_globs {
            if set.is_match(path) {
                continue;
            }
        }

        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let reader = BufReader::new(file);

        for (idx, line_result) in reader.lines().enumerate() {
            let line_number = (idx + 1) as u32;
            let line = match line_result {
                Ok(l) => l,
                Err(_) => break,
            };

            let column = if let Some(expr) = &text_only_expr {
                find_in_line(expr, &line, config.literal).map(|idx| idx as u32 + 1)
            } else if config.literal {
                find_literal_identifier(&line, &config.pattern)
                    .map(|byte_index| byte_index as u32 + 1)
            } else {
                if !line.contains(&config.pattern) {
                    continue;
                }
                line.find(&config.pattern).map(|col| col as u32 + 1)
            };

            if column.is_none() {
                continue;
            }

            total_matches += 1;

            if matches.len() < limit {
                let snippet = match config.max_lines {
                    Some(0) => None,
                    _ => Some(line.clone()),
                };

                matches.push(SearchMatch {
                    path: path.to_path_buf(),
                    line: line_number,
                    column,
                    snippet,
                });
            }

            if matches.len() >= limit {
                truncated = config.limit.is_some();
                break 'walk;
            }
        }
    }

    let summary = SearchSummary {
        total_matches,
        truncated,
    };

    Ok(SearchResult {
        version: SEARCH_RESULT_VERSION.to_string(),
        query: config.pattern,
        matches,
        symbols: Vec::new(),
        contexts: Vec::new(),
        summary,
    })
}

/// Whether the expression is composed only of content-like terms that
/// can be evaluated directly against a single line of text.
fn expr_is_text_only(expr: &crate::models::QueryExpr) -> bool {
    use crate::models::QueryExpr::*;
    match expr {
        Term(term) => matches!(term.field, crate::models::QueryField::Content),
        And(clauses) | Or(clauses) => clauses.iter().all(expr_is_text_only),
    }
}

/// Find the first match column (0-based) for a content-only query
/// expression within a single line, honoring `--literal` for
/// identifier-style matching.
fn find_in_line(expr: &crate::models::QueryExpr, line: &str, literal: bool) -> Option<usize> {
    use crate::models::QueryExpr::*;
    match expr {
        Term(term) => {
            let value = term.value.as_str();
            if let Some(exact) = value.strip_prefix('=') {
                if line == exact {
                    return Some(0);
                }
                return None;
            }

            if literal {
                find_literal_identifier(line, value)
            } else {
                line.find(value)
            }
        }
        And(clauses) => {
            let mut best: Option<usize> = None;
            for clause in clauses {
                let idx = find_in_line(clause, line, literal)?;
                best = Some(match best {
                    Some(current) => current.min(idx),
                    None => idx,
                });
            }
            best
        }
        Or(clauses) => {
            let mut best: Option<usize> = None;
            for clause in clauses {
                if let Some(idx) = find_in_line(clause, line, literal) {
                    best = Some(match best {
                        Some(current) => current.min(idx),
                        None => idx,
                    });
                }
            }
            best
        }
    }
}

fn run_symbol_search_without_index(config: SearchConfig) -> Result<SearchResult> {
    let include_globs = build_globset(&config.globs)?;
    let exclude_globs = build_globset(&config.exclude_globs)?;

    if config.paths.is_empty() {
        bail!("at least one search path is required");
    }

    for path in &config.paths {
        if !path.exists() {
            bail!("search path does not exist: {}", path.display());
        }
    }

    let selected_backend = if let Some(lang) = &config.language {
        Some(
            backend_for_language(lang).ok_or_else(|| {
                anyhow::anyhow!(
                    "symbol search is only supported for known languages (e.g., typescript, javascript); got {}",
                    lang
                )
            })?,
        )
    } else {
        None
    };

    let mut builder = WalkBuilder::new(&config.paths[0]);
    for path in config.paths.iter().skip(1) {
        builder.add(path);
    }

    let walker = builder.build();

    let query_expr = config
        .query_expr
        .clone()
        .or_else(|| parse_query_expr(&config.pattern));

    let has_content_terms = query_expr.as_ref().is_some_and(expr_has_content_terms);
    let primary_context = primary_context_kind_from_views(&config.symbol_views);
    let want_matches = config
        .symbol_views
        .iter()
        .any(|v| matches!(v, SymbolView::Matches));
    let views_meta_only = views_are_meta_only(&config.symbol_views);

    let mut symbols = Vec::new();
    let mut contexts = Vec::new();
    let mut total_matches: u64 = 0;
    let mut truncated = false;

    let limit = config.limit.unwrap_or(usize::MAX);

    'walk: for entry_result in walker {
        let entry = match entry_result {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();

        if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            continue;
        }

        if let Some(set) = &include_globs {
            if !set.is_match(path) {
                continue;
            }
        }
        if let Some(set) = &exclude_globs {
            if set.is_match(path) {
                continue;
            }
        }

        let backend = match selected_backend {
            Some(backend) => {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if !backend
                    .file_extensions()
                    .iter()
                    .any(|e| e.eq_ignore_ascii_case(ext))
                {
                    continue;
                }
                backend
            }
            None => match backend_for_path(path) {
                Some(b) => b,
                None => continue,
            },
        };

        let source = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let parsed = match backend.parse_file(path, &source) {
            Ok(p) => p,
            Err(_) => continue,
        };

        let indexed_symbols = match backend.index_symbols(&parsed) {
            Ok(syms) => syms,
            Err(_) => continue,
        };

        for mut symbol in indexed_symbols {
            // First apply metadata-only filters (name/kind/file/language).
            let metadata_matches = if let Some(expr) = &query_expr {
                symbol_matches_metadata(expr, &symbol, config.literal)
            } else if config.literal {
                symbol.name == config.pattern
            } else {
                symbol.name.contains(&config.pattern)
            };

            if !metadata_matches {
                continue;
            }

            // Only materialize a context snippet when either requested
            // by views or required for content-like terms. When views
            // are `meta`-only, we intentionally avoid fetching any
            // context.
            let mut context_for_result = None;

            if let Some(expr) = &query_expr {
                if !views_meta_only
                    && (has_content_terms
                        || primary_context.is_some()
                        || (want_matches && !config.symbol_views.is_empty()))
                {
                    let kind_for_snippet = primary_context.unwrap_or(ContextKind::Def);

                    let context = backend
                        .get_context_snippet(&parsed, &symbol, kind_for_snippet)
                        .map_err(|err| {
                            anyhow::anyhow!(
                                "failed to get context snippet for symbol {} in {}: {}",
                                symbol.name,
                                symbol.file.display(),
                                err
                            )
                        })?;

                    let context_snippet = context.snippet.clone();

                    context_for_result = Some(context);

                    if has_content_terms
                        && !symbol_matches_with_text(
                            expr,
                            &symbol,
                            Some(context_snippet.as_str()),
                            config.literal,
                        )
                    {
                        continue;
                    }
                } else if !symbol_matches_with_text(expr, &symbol, None, config.literal) {
                    // Expression without content-like terms or context/snippet
                    // requirements – evaluate against metadata only.
                    continue;
                }
            }

            let symbol_matches = if want_matches && !views_meta_only {
                compute_symbol_matches(
                    query_expr.as_ref(),
                    &symbol,
                    context_for_result.as_ref(),
                )
            } else {
                Vec::new()
            };

            total_matches += 1;

            if symbols.len() < limit {
                let symbol_index = symbols.len();

                if let Some(mut context) = context_for_result {
                    if matches!(context.kind, ContextKind::Def) {
                        let range = context.range;
                        let lines = range.end_line.saturating_sub(range.start_line) + 1;
                        symbol.def_line_count = Some(lines);
                    }
                    context.symbol_index = Some(symbol_index);
                    contexts.push(context);
                }

                let mut symbol_with_matches = symbol;
                symbol_with_matches.matches = symbol_matches;
                symbols.push(symbol_with_matches);
            }

            if symbols.len() >= limit {
                truncated = config.limit.is_some();
                break 'walk;
            }
        }
    }

    let summary = SearchSummary {
        total_matches,
        truncated,
    };

    Ok(SearchResult {
        version: SEARCH_RESULT_VERSION.to_string(),
        query: config.pattern,
        matches: Vec::new(),
        symbols,
        contexts,
        summary,
    })
}

fn run_symbol_search_with_index(config: SearchConfig) -> Result<SearchResult> {
    if config.paths.is_empty() {
        bail!("at least one search path is required");
    }

    for path in &config.paths {
        if !path.exists() {
            bail!("search path does not exist: {}", path.display());
        }
    }

    let index_config = match resolve_effective_index_config(&config) {
        Some(cfg) => cfg,
        None => return run_symbol_search_without_index(config),
    };

    // If the backend cannot be opened (e.g., index does not exist yet),
    // fall back to the non-indexed implementation to preserve behavior.
    let backend = match open_backend(&index_config) {
        Ok(b) => b,
        Err(_) => return run_symbol_search_without_index(config),
    };

    let query_expr = config
        .query_expr
        .clone()
        .or_else(|| parse_query_expr(&config.pattern));
    let has_content_terms = query_expr.as_ref().is_some_and(expr_has_content_terms);
    let primary_context = primary_context_kind_from_views(&config.symbol_views);
    let want_matches = config
        .symbol_views
        .iter()
        .any(|v| matches!(v, SymbolView::Matches));
    let views_meta_only = views_are_meta_only(&config.symbol_views);

    // Use the index to retrieve candidate symbols, filtering only by
    // language and path-level constraints. The full DSL evaluation is
    // still performed in-memory to keep behavior identical to the
    // non-indexed search.
    let symbol_query = SymbolQuery {
        name_substring: None,
        language: config.language.clone(),
        paths: config.paths.clone(),
        globs: config.globs.clone(),
        exclude_globs: config.exclude_globs.clone(),
    };

    let indexed_symbols = backend.query_symbols(&symbol_query)?;

    if indexed_symbols.is_empty() {
        // No index data for the requested paths/language; fall back
        // to the non-indexed engine.
        return run_symbol_search_without_index(config);
    }

    let mut symbols = Vec::new();
    let mut contexts = Vec::new();
    let mut total_matches: u64 = 0;
    let mut truncated = false;

    let limit = config.limit.unwrap_or(usize::MAX);

    use std::collections::HashMap;
    use std::path::PathBuf;

    let mut parsed_cache: HashMap<PathBuf, crate::language::ParsedFile> = HashMap::new();

    for record in indexed_symbols {
        let file_record = match backend.get_file_by_id(record.file_id)? {
            Some(f) => f,
            None => continue,
        };

        let path = file_record.path.clone();

        // Reconstruct the core `Symbol` type from the indexed record,
        // hydrating attributes from the persisted `extra` payload.
        let attributes = crate::index::symbol_attributes_from_extra(&record.extra);
        let mut symbol = crate::models::Symbol {
            name: record.name.clone(),
            kind: record.kind,
            language: record.language.clone(),
            file: path.clone(),
            range: record.range,
            signature: record.signature.clone(),
            attributes,
            def_line_count: None,
            matches: Vec::new(),
            calls: Vec::new(),
            called_by: Vec::new(),
        };

        // First apply metadata-only filters (name/kind/file/language).
        let metadata_matches = if let Some(expr) = &query_expr {
            symbol_matches_metadata(expr, &symbol, config.literal)
        } else if config.literal {
            symbol.name == config.pattern
        } else {
            symbol.name.contains(&config.pattern)
        };

        if !metadata_matches {
            continue;
        }

        let mut context_for_result = None;

        if let Some(expr) = &query_expr {
            if !views_meta_only
                && (has_content_terms
                    || primary_context.is_some()
                    || (want_matches && !config.symbol_views.is_empty()))
            {
                let parsed = if let Some(existing) = parsed_cache.get(&path) {
                    existing
                } else {
                    let language_backend = if let Some(lang) = &config.language {
                        backend_for_language(lang).ok_or_else(|| {
                            anyhow::anyhow!(
                                "symbol search is only supported for known languages (e.g., typescript, javascript); got {}",
                                lang
                            )
                        })?
                    } else {
                        backend_for_path(&path).ok_or_else(|| {
                            anyhow::anyhow!(
                                "symbol search is only supported for known languages (e.g., typescript, javascript, cpp); got path {}",
                                path.display()
                            )
                        })?
                    };

                    let source = fs::read_to_string(&path)?;
                    let parsed = language_backend.parse_file(&path, &source)?;
                    parsed_cache.insert(path.clone(), parsed);
                    parsed_cache
                        .get(&path)
                        .expect("parsed file should be present in cache")
                };

                let kind_for_snippet = primary_context.unwrap_or(ContextKind::Def);

                let language_backend = backend_for_language(&symbol.language)
                    .or_else(|| backend_for_path(&symbol.file))
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "symbol search is only supported for known languages; got {}",
                            symbol.language
                        )
                    })?;

                let context = language_backend
                    .get_context_snippet(parsed, &symbol, kind_for_snippet)
                    .map_err(|err| {
                        anyhow::anyhow!(
                            "failed to get context snippet for symbol {} in {}: {}",
                            symbol.name,
                            symbol.file.display(),
                            err
                        )
                    })?;

                let context_snippet = context.snippet.clone();

                if has_content_terms
                    && !symbol_matches_with_text(
                        expr,
                        &symbol,
                        Some(context_snippet.as_str()),
                        config.literal,
                    )
                {
                    continue;
                }

                context_for_result = Some(context);
        } else if !symbol_matches_with_text(expr, &symbol, None, config.literal) {
                // Expression without content-like terms or context/snippet
                // requirements – evaluate against metadata only.
                continue;
            }
        }

        let symbol_matches = if want_matches && !views_meta_only {
            compute_symbol_matches(
                query_expr.as_ref(),
                &symbol,
                context_for_result.as_ref(),
            )
        } else {
            Vec::new()
        };

        total_matches += 1;

        if symbols.len() < limit {
            let symbol_index = symbols.len();

            if let Some(mut context) = context_for_result {
                if matches!(context.kind, ContextKind::Def) {
                    let range = context.range;
                    let lines = range.end_line.saturating_sub(range.start_line) + 1;
                    symbol.def_line_count = Some(lines);
                }
                context.symbol_index = Some(symbol_index);
                contexts.push(context);
            }

            let mut symbol_with_matches = symbol;
            symbol_with_matches.matches = symbol_matches;
            symbols.push(symbol_with_matches);
        }

        if symbols.len() >= limit {
            truncated = config.limit.is_some();
            break;
        }
    }

    Ok(SearchResult {
        version: SEARCH_RESULT_VERSION.to_string(),
        query: config.pattern,
        matches: Vec::new(),
        symbols,
        contexts,
        summary: SearchSummary {
            total_matches,
            truncated,
        },
    })
}

/// Resolve the effective index configuration to use for a symbol search.
///
/// Backend selection rules:
/// - If `config.index` is `None`, indexing is disabled.
/// - If an explicit backend and path have been configured (e.g.
///   via `--index-backend` or a non-default `--index-path`),
///   that backend is used as-is.
/// - When `config.index` uses the default file backend at
///   `.symgrep` (the CLI default when `--index-backend` is
///   omitted), this is treated as "auto" selection:
///   - Prefer an existing SQLite index at `.symgrep/index.sqlite`.
///   - Else, use the file backend if `.symgrep/` exists.
///   - Else, fall back to non-indexed search.
fn resolve_effective_index_config(config: &SearchConfig) -> Option<IndexConfig> {
    let index = match &config.index {
        Some(cfg) => cfg.clone(),
        None => return None,
    };

    let default_root = std::path::PathBuf::from(".symgrep");

    if index.backend == crate::models::IndexBackendKind::File && index.index_path == default_root {
        let sqlite_path = default_root.join("index.sqlite");

        if sqlite_path.exists() {
            return Some(IndexConfig {
                backend: crate::models::IndexBackendKind::Sqlite,
                index_path: sqlite_path,
                ..index
            });
        }

        if default_root.exists() {
            return Some(index);
        }

        return None;
    }

    Some(index)
}

fn is_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn find_literal_identifier(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }

    let mut search_start = 0;

    while let Some(rel_idx) = haystack[search_start..].find(needle) {
        let start = search_start + rel_idx;
        let end = start + needle.len();

        let prev_char = haystack[..start].chars().rev().next();
        let next_char = haystack[end..].chars().next();

        let left_ok = prev_char.map_or(true, |ch| !is_identifier_char(ch));
        let right_ok = next_char.map_or(true, |ch| !is_identifier_char(ch));

        if left_ok && right_ok {
            return Some(start);
        }

        search_start = end;
    }

    None
}

fn primary_context_kind_from_views(views: &[SymbolView]) -> Option<ContextKind> {
    if views.iter().any(|v| matches!(v, SymbolView::Def)) {
        Some(ContextKind::Def)
    } else if views.iter().any(|v| matches!(v, SymbolView::Decl)) {
        Some(ContextKind::Decl)
    } else if views.iter().any(|v| matches!(v, SymbolView::Parent)) {
        Some(ContextKind::Parent)
    } else {
        None
    }
}

fn views_are_meta_only(views: &[SymbolView]) -> bool {
    !views.is_empty() && views.iter().all(|v| matches!(v, SymbolView::Meta))
}

fn compute_symbol_matches(
    expr: Option<&crate::models::QueryExpr>,
    symbol: &crate::models::Symbol,
    context: Option<&crate::models::ContextInfo>,
) -> Vec<SymbolMatch> {
    let mut result = Vec::new();
    let Some(expr) = expr else {
        return result;
    };

    use crate::models::QueryExpr::*;

    // Collect content-like terms (content/comment/description).
    fn collect_terms<'a>(
        expr: &'a crate::models::QueryExpr,
        content_terms: &mut Vec<&'a crate::models::QueryTerm>,
        comment_terms: &mut Vec<&'a crate::models::QueryTerm>,
        desc_terms: &mut Vec<&'a crate::models::QueryTerm>,
    ) {
        match expr {
            Term(term) => match term.field {
                crate::models::QueryField::Content => content_terms.push(term),
                crate::models::QueryField::Comment => comment_terms.push(term),
                crate::models::QueryField::Description => desc_terms.push(term),
                _ => {}
            },
            And(clauses) | Or(clauses) => {
                for c in clauses {
                    collect_terms(c, content_terms, comment_terms, desc_terms);
                }
            }
        }
    }

    let mut content_terms = Vec::new();
    let mut comment_terms = Vec::new();
    let mut desc_terms = Vec::new();
    collect_terms(expr, &mut content_terms, &mut comment_terms, &mut desc_terms);

    // Matches from the primary context snippet.
    if let (Some(ctx), true) = (context, !content_terms.is_empty()) {
        let base_line = ctx.range.start_line;
        for (idx, line) in ctx.snippet.lines().enumerate() {
            let line_no = base_line + idx as u32;
            for term in &content_terms {
                let raw = term.value.as_str();
                let needle = raw.strip_prefix('=').unwrap_or(raw);
                if needle.is_empty() {
                    continue;
                }
                if let Some(col) = line.find(needle) {
                    result.push(SymbolMatch {
                        line: line_no,
                        column: Some(col as u32 + 1),
                        snippet: line.to_string(),
                    });
                    break;
                }
            }
        }
    }

    // Matches from comment/description attributes.
    if let Some(attrs) = &symbol.attributes {
        if let Some(comment) = &attrs.comment {
            for term in &comment_terms {
                let raw = term.value.as_str();
                let needle = raw.strip_prefix('=').unwrap_or(raw);
                if !needle.is_empty() && comment.contains(needle) {
                    result.push(SymbolMatch {
                        line: symbol.range.start_line,
                        column: Some(1),
                        snippet: comment.clone(),
                    });
                    break;
                }
            }
        }

        if let Some(desc) = &attrs.description {
            for term in &desc_terms {
                let raw = term.value.as_str();
                let needle = raw.strip_prefix('=').unwrap_or(raw);
                if !needle.is_empty() && desc.contains(needle) {
                    result.push(SymbolMatch {
                        line: symbol.range.start_line,
                        column: Some(1),
                        snippet: desc.clone(),
                    });
                    break;
                }
            }
        }
    }

    result
}

fn build_globset(patterns: &[String]) -> Result<Option<GlobSet>> {
    if patterns.is_empty() {
        return Ok(None);
    }

    let mut builder = globset::GlobSetBuilder::new();
    for pat in patterns {
        builder.add(Glob::new(pat)?);
    }
    Ok(Some(builder.build()?))
}

/// Build or update an index based on the provided configuration.
///
/// The concrete indexing behavior will be implemented in later phases.
pub fn run_index(_config: IndexConfig) -> Result<IndexSummary> {
    crate::index::run_index(_config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{IndexBackendKind, IndexConfig, SearchConfig, SearchMode, SymbolView};
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn base_config(root: PathBuf) -> SearchConfig {
        SearchConfig {
            pattern: "foo".to_string(),
            paths: vec![root],
            globs: Vec::new(),
            exclude_globs: Vec::new(),
            language: None,
            mode: SearchMode::Text,
            literal: false,
            symbol_views: Vec::new(),
            limit: None,
            max_lines: None,
            reindex_on_search: false,
            query_expr: None,
            index: None,
        }
    }

    #[test]
    fn run_search_finds_simple_matches() {
        let dir = tempdir().expect("tempdir");
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "foo\nbar\nfoo bar\n").expect("write");

        let config = base_config(dir.path().to_path_buf());
        let result = run_search(config).expect("search result");

        assert_eq!(result.query, "foo");
        assert_eq!(result.summary.total_matches, 2);
        assert!(!result.summary.truncated);
        assert_eq!(result.matches.len(), 2);
        assert!(result.matches.iter().all(|m| m.path == file_path));
    }

    #[test]
    fn run_search_honors_limit_and_truncated_flag() {
        let dir = tempdir().expect("tempdir");
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "foo1\nfoo2\nfoo3\n").expect("write");

        let mut config = base_config(dir.path().to_path_buf());
        config.limit = Some(1);

        let result = run_search(config).expect("search result");

        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.summary.total_matches, 1);
        assert!(result.summary.truncated);
    }

    #[test]
    fn run_search_omits_snippet_when_max_lines_is_zero() {
        let dir = tempdir().expect("tempdir");
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "foo\n").expect("write");

        let mut config = base_config(dir.path().to_path_buf());
        config.max_lines = Some(0);

        let result = run_search(config).expect("search result");
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].snippet, None);
    }

    #[test]
    fn run_search_respects_glob_inclusion_and_exclusion() {
        let dir = tempdir().expect("tempdir");
        let include_file = dir.path().join("keep.rs");
        let exclude_file = dir.path().join("skip.txt");
        std::fs::write(&include_file, "foo\n").expect("write");
        std::fs::write(&exclude_file, "foo\n").expect("write");

        let mut config = base_config(dir.path().to_path_buf());
        config.globs = vec!["*.rs".to_string()];
        config.exclude_globs = vec!["*skip*".to_string()];

        let result = run_search(config).expect("search result");
        assert_eq!(result.matches.len(), 1);
        assert!(result.matches.iter().all(|m| m.path == include_file));
    }

    #[test]
    fn run_search_supports_multiple_paths() {
        let root = tempdir().expect("tempdir");
        let dir_a = tempfile::tempdir_in(root.path()).expect("dir a");
        let dir_b = tempfile::tempdir_in(root.path()).expect("dir b");

        let file_a = dir_a.path().join("a.txt");
        let file_b = dir_b.path().join("b.txt");
        std::fs::write(&file_a, "foo\n").expect("write");
        std::fs::write(&file_b, "foo\n").expect("write");

        let config = SearchConfig {
            pattern: "foo".to_string(),
            paths: vec![dir_a.path().to_path_buf(), dir_b.path().to_path_buf()],
            globs: Vec::new(),
            exclude_globs: Vec::new(),
            language: None,
            mode: SearchMode::Text,
            literal: false,
            symbol_views: Vec::new(),
            limit: None,
            max_lines: None,
            reindex_on_search: false,
            query_expr: None,
            index: None,
        };

        let result = run_search(config).expect("search result");
        assert_eq!(result.matches.len(), 2);
        let paths: Vec<_> = result.matches.iter().map(|m| m.path.clone()).collect();
        assert!(paths.contains(&file_a));
        assert!(paths.contains(&file_b));
    }

    #[test]
    fn find_literal_identifier_respects_word_boundaries() {
        assert_eq!(find_literal_identifier("foo", "foo"), Some(0));
        assert_eq!(find_literal_identifier("foobar", "foo"), None);
        assert_eq!(find_literal_identifier("foo_bar", "foo"), None);
        assert_eq!(find_literal_identifier("bar_foo", "foo"), None);
        assert_eq!(find_literal_identifier("foo()", "foo"), Some(0));
    }

    #[test]
    fn run_search_errors_on_nonexistent_path() {
        let config = SearchConfig {
            pattern: "foo".to_string(),
            paths: vec![PathBuf::from("definitely/does/not/exist")],
            globs: Vec::new(),
            exclude_globs: Vec::new(),
            language: None,
            mode: SearchMode::Text,
            literal: false,
            symbol_views: Vec::new(),
            limit: None,
            max_lines: None,
            reindex_on_search: false,
            query_expr: None,
            index: None,
        };

        let err = run_search(config).expect_err("expected error");
        let msg = format!("{err}");
        assert!(msg.contains("search path does not exist"));
    }

    #[test]
    fn symbol_mode_searches_ts_symbols_by_name() {
        let config = SearchConfig {
            pattern: "add".to_string(),
            paths: vec![PathBuf::from("tests/fixtures/ts_js_repo")],
            globs: Vec::new(),
            exclude_globs: Vec::new(),
            language: Some("typescript".to_string()),
            mode: SearchMode::Symbol,
            literal: false,
            symbol_views: vec![SymbolView::Decl],
            limit: None,
            max_lines: None,
            reindex_on_search: false,
            query_expr: None,
            index: None,
        };

        let result = run_search(config).expect("search result");

        assert!(
            result.summary.total_matches >= 1,
            "expected at least one TypeScript symbol match for 'add'"
        );
        assert!(!result.summary.truncated);
        assert_eq!(result.matches.len(), 0);
        assert!(
            result
                .symbols
                .iter()
                .any(|s| s.name == "add" && s.language == "typescript"),
            "expected at least one symbol named 'add' in TypeScript results"
        );
        assert!(
            result
                .contexts
                .iter()
                .any(|c| c.snippet.contains("export function add")),
            "expected at least one context snippet containing 'export function add'"
        );
    }

    #[test]
    fn auto_mode_uses_symbol_search_for_supported_language() {
        let config = SearchConfig {
            pattern: "add".to_string(),
            paths: vec![PathBuf::from("tests/fixtures/ts_js_repo")],
            globs: Vec::new(),
            exclude_globs: Vec::new(),
            language: Some("typescript".to_string()),
            mode: SearchMode::Auto,
            literal: false,
            symbol_views: vec![SymbolView::Def],
            limit: Some(1),
            max_lines: None,
            reindex_on_search: false,
            query_expr: None,
            index: None,
        };

        let result = run_search(config).expect("search result");

        assert_eq!(result.summary.total_matches, 1);
        assert!(result.summary.truncated);
        assert_eq!(result.matches.len(), 0);
        assert_eq!(result.symbols.len(), 1);
        assert!(
            result.symbols[0].name == "add"
                || result.symbols[0].name == "addWithDoc",
            "expected a single symbol whose name contains 'add'"
        );
    }

    #[test]
    fn symbol_mode_populates_def_line_count_from_def_context() {
        let config = SearchConfig {
            pattern: "name:multilineAdd kind:function".to_string(),
            paths: vec![PathBuf::from("tests/fixtures/ts_js_repo")],
            globs: Vec::new(),
            exclude_globs: Vec::new(),
            language: Some("typescript".to_string()),
            mode: SearchMode::Symbol,
            literal: false,
            symbol_views: vec![SymbolView::Def],
            limit: None,
            max_lines: None,
            reindex_on_search: false,
            query_expr: None,
            index: None,
        };

        let result = run_search(config).expect("search result");

        let idx = result
            .symbols
            .iter()
            .position(|s| s.name == "multilineAdd" && s.language == "typescript")
            .expect("multilineAdd symbol");
        let symbol = &result.symbols[idx];

        assert!(
            symbol.def_line_count.is_some(),
            "expected def_line_count to be populated for multilineAdd"
        );

        let context = result
            .contexts
            .iter()
            .find(|c| c.symbol_index == Some(idx))
            .expect("context for multilineAdd");

        let snippet_line_count = context.snippet.lines().count() as u32;
        assert_eq!(
            symbol.def_line_count,
            Some(snippet_line_count),
            "def_line_count should match Def context snippet line count"
        );
    }

    #[test]
    fn symbol_mode_def_context_ignores_max_lines_in_json() {
        let config = SearchConfig {
            pattern: "name:multilineAdd kind:function".to_string(),
            paths: vec![PathBuf::from("tests/fixtures/ts_js_repo")],
            globs: Vec::new(),
            exclude_globs: Vec::new(),
            language: Some("typescript".to_string()),
            mode: SearchMode::Symbol,
            literal: false,
            symbol_views: vec![SymbolView::Def],
            limit: None,
            max_lines: Some(1),
            reindex_on_search: false,
            query_expr: None,
            index: None,
        };

        let result = run_search(config).expect("search result");

        let idx = result
            .symbols
            .iter()
            .position(|s| s.name == "multilineAdd" && s.language == "typescript")
            .expect("multilineAdd symbol");
        let symbol = &result.symbols[idx];

        let context = result
            .contexts
            .iter()
            .find(|c| c.symbol_index == Some(idx))
            .expect("context for multilineAdd");

        let snippet_line_count = context.snippet.lines().count() as u32;
        assert!(
            snippet_line_count >= 2,
            "expected multi-line Def snippet for multilineAdd even when max_lines is set"
        );
        assert_eq!(
            symbol.def_line_count,
            Some(snippet_line_count),
            "def_line_count should still match full Def context snippet line count when max_lines is set"
        );
    }

    #[test]
    fn reindex_on_search_refreshes_indexed_symbols() {
        use tempfile::tempdir;

        let tmp = tempdir().expect("tempdir");
        let repo_root = tmp.path().join("repo");
        std::fs::create_dir_all(&repo_root).expect("create repo root");

        let file_path = repo_root.join("sample.ts");
        std::fs::write(
            &file_path,
            "export function add(a: number, b: number): number { return a + b; }",
        )
        .expect("write sample");

        let index_root = tmp.path().join(".symgrep");

        let index_config = IndexConfig {
            paths: vec![repo_root.clone()],
            globs: Vec::new(),
            exclude_globs: Vec::new(),
            backend: IndexBackendKind::File,
            index_path: index_root,
            language: Some("typescript".to_string()),
        };

        // Build an initial index so subsequent searches can use it.
        crate::index::run_index(index_config.clone()).expect("initial index");

        let initial_search = SearchConfig {
            pattern: "name:add kind:function".to_string(),
            paths: vec![repo_root.clone()],
            globs: Vec::new(),
            exclude_globs: Vec::new(),
            language: Some("typescript".to_string()),
            mode: SearchMode::Symbol,
            literal: false,
            symbol_views: vec![SymbolView::Decl],
            limit: None,
            max_lines: None,
            reindex_on_search: false,
            query_expr: None,
            index: Some(index_config.clone()),
        };

        let initial_result = run_search(initial_search).expect("initial search result");
        assert!(
            initial_result
                .symbols
                .iter()
                .any(|s| s.name == "add"),
            "expected to find symbol 'add' before rename"
        );

        // Rename the function in source without manually re-running `symgrep index`.
        std::fs::write(
            &file_path,
            "export function add2(a: number, b: number): number { return a + b; }",
        )
        .expect("rewrite sample");

        let refreshed_search = SearchConfig {
            pattern: "name:add2 kind:function".to_string(),
            paths: vec![repo_root],
            globs: Vec::new(),
            exclude_globs: Vec::new(),
            language: Some("typescript".to_string()),
            mode: SearchMode::Symbol,
            literal: false,
            symbol_views: vec![SymbolView::Decl],
            limit: None,
            max_lines: None,
            reindex_on_search: true,
            query_expr: None,
            index: Some(index_config),
        };

        let refreshed_result = run_search(refreshed_search).expect("refreshed search result");
        assert!(
            refreshed_result
                .symbols
                .iter()
                .any(|s| s.name == "add2"),
            "expected to find symbol 'add2' after reindex_on_search"
        );
    }
}
