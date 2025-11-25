//! Query DSL parsing and evaluation utilities.
//!
//! This module implements a minimal structured query language used
//! by the search engine and CLI. It supports fielded terms such as
//! `content:foo`, `name:bar`, `kind:function`, and simple AND/OR
//! composition:
//! - Space-separated groups are combined with AND.
//! - `A|B` within a group is treated as OR.
//! - When the first alternative in a group has a known field,
//!   subsequent bare alternatives inherit that field (e.g.
//!   `kind:function|method`).

use crate::models::{QueryExpr, QueryField, QueryTerm, Symbol, SymbolKind};

/// Parse a raw query string into a `QueryExpr`.
///
/// The parser is intentionally simple:
/// - Leading/trailing whitespace is ignored.
/// - When the pattern contains **no `field:` syntax at all**, the
///   whole pattern is treated as a generic content query:
///   - `foo bar` becomes a single `content:"foo bar"` term.
///   - `foo|bar` becomes `content:foo OR content:bar`.
/// - Otherwise, tokens are split on whitespace (honoring double
///   quotes) and within each token `|` separates OR alternatives.
/// - `field:value` syntax selects a field; bare tokens within a
///   structured query default to `name:` for backward compatibility.
pub fn parse_query_expr(input: &str) -> Option<QueryExpr> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Special-case patterns with no `field:` syntax at all. These are
    // interpreted as generic content queries, with `|` acting as OR between
    // alternatives and the full trimmed pattern (which may contain
    // spaces) preserved as-is per alternative.
    if !trimmed.contains(':') {
        let mut alts: Vec<QueryExpr> = Vec::new();
        for raw_alt in trimmed.split('|') {
            let alt = raw_alt.trim();
            if alt.is_empty() {
                continue;
            }
            let term = QueryTerm {
                field: QueryField::Content,
                value: alt.to_string(),
            };
            alts.push(QueryExpr::Term(term));
        }

        return match alts.len() {
            0 => None,
            1 => Some(alts.into_iter().next().unwrap()),
            _ => Some(QueryExpr::Or(alts)),
        };
    }

    let tokens = tokenize(trimmed);
    if tokens.is_empty() {
        return None;
    }

    // Each whitespace-separated token becomes an AND group; `|`
    // splits each group into OR alternatives. When the first
    // alternative in a group has an explicit field (e.g. `kind:`),
    // subsequent bare alternatives inherit that field:
    // - `kind:function|method` → kind:function OR kind:method
    // - `language:ts|js` → language:ts OR language:js
    let mut groups: Vec<Vec<QueryExpr>> = Vec::new();

    for token in tokens {
        let mut clauses = Vec::new();
        let mut default_field: Option<QueryField> = None;
        for raw_alt in token.split('|') {
            let alt = raw_alt.trim();
            if alt.is_empty() {
                continue;
            }
            let term = if alt.contains(':') || default_field.is_none() {
                let t = parse_term(alt);
                if default_field.is_none() {
                    default_field = Some(t.field);
                }
                t
            } else {
                let field = default_field.expect("default_field must be set");
                QueryTerm {
                    field,
                    value: alt.to_string(),
                }
            };
            clauses.push(QueryExpr::Term(term));
        }
        if !clauses.is_empty() {
            groups.push(clauses);
        }
    }

    if groups.is_empty() {
        return None;
    }

    let mut group_exprs = Vec::new();
    for clauses in groups {
        let expr = if clauses.len() == 1 {
            clauses.into_iter().next().unwrap()
        } else {
            QueryExpr::Or(clauses)
        };
        group_exprs.push(expr);
    }

    let expr = if group_exprs.len() == 1 {
        group_exprs.into_iter().next().unwrap()
    } else {
        QueryExpr::And(group_exprs)
    };

    Some(expr)
}

/// Tokenize a query string, treating whitespace as separators and
/// allowing double-quoted segments to contain spaces.
fn tokenize(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in input.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
            }
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(current);
                    current = String::new();
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn parse_term(atom: &str) -> QueryTerm {
    let mut parts = atom.splitn(2, ':');
    let head = parts.next().unwrap_or_default();

    if let Some(rest) = parts.next() {
        let value = rest.to_string();
        match head.to_ascii_lowercase().as_str() {
            "name" => QueryTerm {
                field: QueryField::Name,
                value,
            },
            "kind" => QueryTerm {
                field: QueryField::Kind,
                value,
            },
            "file" => QueryTerm {
                field: QueryField::File,
                value,
            },
            "language" => QueryTerm {
                field: QueryField::Language,
                value,
            },
            "content" => QueryTerm {
                field: QueryField::Content,
                value,
            },
            "comment" => QueryTerm {
                field: QueryField::Comment,
                value,
            },
            "keyword" | "keywords" => QueryTerm {
                field: QueryField::Keyword,
                value,
            },
            "desc" | "description" => QueryTerm {
                field: QueryField::Description,
                value,
            },
            "calls" => QueryTerm {
                field: QueryField::Calls,
                value,
            },
            "called-by" | "called_by" | "callers" => QueryTerm {
                field: QueryField::CalledBy,
                value,
            },
            // Unknown field – treat the whole atom as a name filter
            // to stay backward compatible and conservative.
            _ => QueryTerm {
                field: QueryField::Name,
                value: atom.to_string(),
            },
        }
    } else {
        // Bare terms default to `name:` for symbol searches.
        QueryTerm {
            field: QueryField::Name,
            value: head.to_string(),
        }
    }
}

/// Whether the expression contains any content-like terms.
///
/// Content-like terms are those that require evaluating against
/// symbol content or attributes (content, comment, keyword,
/// description) rather than pure metadata.
pub fn expr_has_content_terms(expr: &QueryExpr) -> bool {
    match expr {
        QueryExpr::Term(term) => matches!(
            term.field,
            QueryField::Content
                | QueryField::Comment
                | QueryField::Keyword
                | QueryField::Description
        ),
        QueryExpr::And(clauses) | QueryExpr::Or(clauses) => {
            clauses.iter().any(expr_has_content_terms)
        }
    }
}

/// Whether the expression contains any call-graph related terms
/// (`calls:` / `called-by:` / `callers:`).
pub fn expr_has_call_terms(expr: &QueryExpr) -> bool {
    match expr {
        QueryExpr::Term(term) => matches!(term.field, QueryField::Calls | QueryField::CalledBy),
        QueryExpr::And(clauses) | QueryExpr::Or(clauses) => {
            clauses.iter().any(expr_has_call_terms)
        }
    }
}

/// Evaluate only the metadata portion of a query (name, kind, file,
/// language) against a symbol.
///
/// Content-like terms are treated as neutral here so that they can be
/// applied later once a context snippet is available.
///
/// The `literal` flag controls how `name:` terms are interpreted:
/// - When `literal == false`, name filters use substring matching.
/// - When `literal == true`, name filters require an exact match.
pub fn symbol_matches_metadata(expr: &QueryExpr, symbol: &Symbol, literal: bool) -> bool {
    match expr {
        QueryExpr::Term(term) => matches_term_metadata(term, symbol, literal),
        QueryExpr::And(clauses) => clauses
            .iter()
            .all(|c| symbol_matches_metadata(c, symbol, literal)),
        QueryExpr::Or(clauses) => clauses
            .iter()
            .any(|c| symbol_matches_metadata(c, symbol, literal)),
    }
}

fn matches_term_metadata(term: &QueryTerm, symbol: &Symbol, literal: bool) -> bool {
    match term.field {
        QueryField::Content
        | QueryField::Comment
        | QueryField::Keyword
        | QueryField::Description => true,
        QueryField::Name => {
            let value = term.value.as_str();
            if let Some(exact) = value.strip_prefix('=') {
                symbol.name == exact
            } else if literal {
                symbol.name == value
            } else {
                symbol.name.contains(value)
            }
        }
        QueryField::Kind => match parse_symbol_kind(&term.value) {
            Some(kind) => symbol.kind == kind,
            None => false,
        },
        QueryField::File => symbol
            .file
            .to_string_lossy()
            .to_string()
            .contains(&term.value),
        QueryField::Language => symbol.language.eq_ignore_ascii_case(term.value.as_str()),
        QueryField::Calls => {
            let value = term.value.as_str();
            if value.is_empty() {
                return true;
            }
            symbol.calls.iter().any(|call| {
                let target = call.name.as_str();
                if let Some(exact) = value.strip_prefix('=') {
                    target == exact
                } else {
                    target.contains(value)
                }
            })
        }
        QueryField::CalledBy => {
            let value = term.value.as_str();
            if value.is_empty() {
                return true;
            }
            symbol.called_by.iter().any(|caller| {
                let target = caller.name.as_str();
                if let Some(exact) = value.strip_prefix('=') {
                    target == exact
                } else {
                    target.contains(value)
                }
            })
        }
    }
}

fn parse_symbol_kind(value: &str) -> Option<SymbolKind> {
    match value.to_ascii_lowercase().as_str() {
        "function" | "func" => Some(SymbolKind::Function),
        "method" => Some(SymbolKind::Method),
        "class" | "struct" => Some(SymbolKind::Class),
        "interface" => Some(SymbolKind::Interface),
        "variable" | "var" => Some(SymbolKind::Variable),
        "namespace" | "ns" => Some(SymbolKind::Namespace),
        _ => None,
    }
}

/// Evaluate the full query expression against a symbol and optional
/// context snippet.
///
/// `content:` terms will match against the composite symbol surface
/// (name, signature, attributes, and optional context snippet). When
/// no snippet is provided they fall back to the symbol name and
/// attributes to avoid surprising behavior when no context is
/// requested.
///
/// The `literal` flag controls how `name:` terms are interpreted:
/// - When `literal == false`, name filters use substring matching.
/// - When `literal == true`, name filters require an exact match.
pub fn symbol_matches_with_text(
    expr: &QueryExpr,
    symbol: &Symbol,
    snippet: Option<&str>,
    literal: bool,
) -> bool {
    match expr {
        QueryExpr::Term(term) => matches_term_full(term, symbol, snippet, literal),
        QueryExpr::And(clauses) => clauses
            .iter()
            .all(|c| symbol_matches_with_text(c, symbol, snippet, literal)),
        QueryExpr::Or(clauses) => clauses
            .iter()
            .any(|c| symbol_matches_with_text(c, symbol, snippet, literal)),
    }
}

fn matches_term_full(
    term: &QueryTerm,
    symbol: &Symbol,
    snippet: Option<&str>,
    literal: bool,
) -> bool {
    match term.field {
        QueryField::Content => {
            let value = term.value.as_str();
            let mut parts: Vec<String> = Vec::new();
            parts.push(symbol.name.clone());
            if let Some(signature) = &symbol.signature {
                parts.push(signature.clone());
            }
            if let Some(attrs) = &symbol.attributes {
                if let Some(comment) = &attrs.comment {
                    parts.push(comment.clone());
                }
                if !attrs.keywords.is_empty() {
                    parts.push(attrs.keywords.join(" "));
                }
                if let Some(desc) = &attrs.description {
                    parts.push(desc.clone());
                }
            }
            if let Some(snippet) = snippet {
                parts.push(snippet.to_string());
            }
            let surface = parts.join(" ");

            if let Some(exact) = value.strip_prefix('=') {
                surface == exact
            } else {
                surface.contains(value)
            }
        }
        QueryField::Name => {
            let value = term.value.as_str();
            if let Some(exact) = value.strip_prefix('=') {
                symbol.name == exact
            } else if literal {
                symbol.name == value
            } else {
                symbol.name.contains(value)
            }
        }
        QueryField::Comment => {
            let value = term.value.as_str();
            let comment = match symbol
                .attributes
                .as_ref()
                .and_then(|attrs| attrs.comment.as_ref())
            {
                Some(c) => c,
                None => return false,
            };
            if let Some(exact) = value.strip_prefix('=') {
                comment == exact
            } else {
                comment.contains(value)
            }
        }
        QueryField::Keyword => {
            let value = term.value.as_str();
            let attrs = match symbol.attributes.as_ref() {
                Some(a) => a,
                None => return false,
            };
            if attrs.keywords.is_empty() {
                return false;
            }

            // Default semantics: exact keyword membership; a leading
            // `~` enables substring matching within keywords.
            if let Some(substr) = value.strip_prefix('~') {
                let target = substr;
                attrs
                    .keywords
                    .iter()
                    .any(|kw| kw.contains(target))
            } else if let Some(exact) = value.strip_prefix('=') {
                attrs.keywords.iter().any(|kw| kw == exact)
            } else {
                attrs.keywords.iter().any(|kw| kw == value)
            }
        }
        QueryField::Description => {
            let value = term.value.as_str();
            let desc = match symbol
                .attributes
                .as_ref()
                .and_then(|attrs| attrs.description.as_ref())
            {
                Some(d) => d,
                None => return false,
            };
            if let Some(exact) = value.strip_prefix('=') {
                desc == exact
            } else {
                desc.contains(value)
            }
        }
        QueryField::Kind
        | QueryField::File
        | QueryField::Language
        | QueryField::Calls
        | QueryField::CalledBy => {
            matches_term_metadata(term, symbol, literal)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SymbolAttributes;

    fn term(field: QueryField, value: &str) -> QueryExpr {
        QueryExpr::Term(QueryTerm {
            field,
            value: value.to_string(),
        })
    }

    #[test]
    fn parse_simple_content_term_for_bare_pattern() {
        let expr = parse_query_expr("foo").expect("expr");
        assert!(matches!(
            expr,
            QueryExpr::Term(QueryTerm {
                field: QueryField::Content,
                value
            }) if value == "foo"
        ));
    }

    #[test]
    fn parse_bare_pattern_with_or_as_text_terms() {
        let expr = parse_query_expr("foo|bar").expect("expr");
        match expr {
            QueryExpr::Or(alts) => {
                assert_eq!(alts.len(), 2);
                assert!(matches!(
                    &alts[0],
                    QueryExpr::Term(QueryTerm {
                        field: QueryField::Content,
                        value
                    }) if value == "foo"
                ));
                assert!(matches!(
                    &alts[1],
                    QueryExpr::Term(QueryTerm {
                        field: QueryField::Content,
                        value
                    }) if value == "bar"
                ));
            }
            _ => panic!("expected top-level OR expression"),
        }
    }

    #[test]
    fn parse_field_or_group_normalizes_field() {
        let expr = parse_query_expr("kind:function|method").expect("expr");
        match expr {
            QueryExpr::Or(alts) => {
                assert_eq!(alts.len(), 2);
                for alt in alts {
                    match alt {
                        QueryExpr::Term(QueryTerm { field, value }) => {
                            assert_eq!(field, QueryField::Kind);
                            assert!(value == "function" || value == "method");
                        }
                        _ => panic!("expected term in OR group"),
                    }
                }
            }
            _ => panic!("expected top-level OR expression for kind"),
        }
    }

    #[test]
    fn parse_name_and_kind_with_or() {
        let expr = parse_query_expr("name:foo|bar kind:function").expect("expr");
        match expr {
            QueryExpr::And(groups) => {
                assert_eq!(groups.len(), 2);
                match &groups[0] {
                    QueryExpr::Or(alts) => {
                        assert_eq!(alts.len(), 2);
                        for alt in alts {
                            match alt {
                                QueryExpr::Term(QueryTerm { field, value }) => {
                                    assert_eq!(*field, QueryField::Name);
                                    assert!(value == "foo" || value == "bar");
                                }
                                _ => panic!("expected name term in OR group"),
                            }
                        }
                    }
                    _ => panic!("expected OR group for name"),
                }
                match &groups[1] {
                    QueryExpr::Term(QueryTerm { field, value }) => {
                        assert_eq!(*field, QueryField::Kind);
                        assert_eq!(value, "function");
                    }
                    _ => panic!("expected simple kind term"),
                }
            }
            _ => panic!("expected top-level AND expression"),
        }
    }

    #[test]
    fn tokenize_respects_quotes() {
        let tokens = tokenize(r#"content:"rate limit" name:foo"#);
        assert_eq!(tokens, vec!["content:rate limit", "name:foo"]);
    }

    #[test]
    fn metadata_matching_respects_kind_and_language() {
        let symbol = Symbol {
            name: "add".to_string(),
            kind: SymbolKind::Function,
            language: "TypeScript".to_string(),
            file: "src/lib.ts".into(),
            range: crate::models::TextRange {
                start_line: 1,
                start_column: 1,
                end_line: 1,
                end_column: 1,
            },
            signature: None,
            attributes: None,
            def_line_count: None,
            matches: Vec::new(),
            calls: Vec::new(),
            called_by: Vec::new(),
        };

        let expr = QueryExpr::And(vec![
            term(QueryField::Kind, "function"),
            term(QueryField::Language, "typescript"),
            term(QueryField::Name, "add"),
        ]);

        assert!(symbol_matches_metadata(&expr, &symbol, false));
    }

    #[test]
    fn literal_name_matching_uses_exact_symbol_name() {
        let symbol = Symbol {
            name: "add".to_string(),
            kind: SymbolKind::Function,
            language: "typescript".to_string(),
            file: "src/lib.ts".into(),
            range: crate::models::TextRange {
                start_line: 1,
                start_column: 1,
                end_line: 1,
                end_column: 1,
            },
            signature: None,
            attributes: None,
            def_line_count: None,
            matches: Vec::new(),
            calls: Vec::new(),
            called_by: Vec::new(),
        };

        let expr = term(QueryField::Name, "add");
        let expr_other = term(QueryField::Name, "adder");

        assert!(symbol_matches_metadata(&expr, &symbol, true));
        assert!(!symbol_matches_metadata(&expr_other, &symbol, true));
    }

    #[test]
    fn parse_comment_and_description_fields() {
        let expr =
            parse_query_expr("comment:auth keyword:jwt desc:\"issues tokens\"").expect("expr");
        match expr {
            QueryExpr::And(groups) => {
                assert_eq!(groups.len(), 3);
                assert!(matches!(
                    &groups[0],
                    QueryExpr::Term(QueryTerm {
                        field: QueryField::Comment,
                        value
                    }) if value == "auth"
                ));
                assert!(matches!(
                    &groups[1],
                    QueryExpr::Term(QueryTerm {
                        field: QueryField::Keyword,
                        value
                    }) if value == "jwt"
                ));
                assert!(matches!(
                    &groups[2],
                    QueryExpr::Term(QueryTerm {
                        field: QueryField::Description,
                        value
                    }) if value == "issues tokens"
                ));
            }
            _ => panic!("expected AND expression"),
        }
    }

    #[test]
    fn comment_field_matches_symbol_comment() {
        let symbol = Symbol {
            name: "loginUser".to_string(),
            kind: SymbolKind::Function,
            language: "typescript".to_string(),
            file: "src/auth.ts".into(),
            range: crate::models::TextRange {
                start_line: 1,
                start_column: 1,
                end_line: 10,
                end_column: 1,
            },
            signature: None,
            attributes: Some(SymbolAttributes {
                comment: Some("Handles authentication and JWT issuance.".to_string()),
                comment_range: None,
                keywords: Vec::new(),
                description: None,
            }),
            def_line_count: None,
            matches: Vec::new(),
            calls: Vec::new(),
            called_by: Vec::new(),
        };

        let expr = term(QueryField::Comment, "authentication");
        assert!(symbol_matches_with_text(&expr, &symbol, None, false));
    }

    #[test]
    fn keyword_field_matches_exact_keyword_and_tilde_substring() {
        let symbol = Symbol {
            name: "loginUser".to_string(),
            kind: SymbolKind::Function,
            language: "typescript".to_string(),
            file: "src/auth.ts".into(),
            range: crate::models::TextRange {
                start_line: 1,
                start_column: 1,
                end_line: 10,
                end_column: 1,
            },
            signature: None,
            attributes: Some(SymbolAttributes {
                comment: None,
                comment_range: None,
                keywords: vec!["auth".to_string(), "jwt-token".to_string()],
                description: None,
            }),
            def_line_count: None,
            matches: Vec::new(),
            calls: Vec::new(),
            called_by: Vec::new(),
        };

        let expr_exact = term(QueryField::Keyword, "auth");
        let expr_sub = term(QueryField::Keyword, "~jwt");

        assert!(symbol_matches_with_text(&expr_exact, &symbol, None, false));
        assert!(symbol_matches_with_text(&expr_sub, &symbol, None, false));
    }

    #[test]
    fn description_field_matches_symbol_description() {
        let symbol = Symbol {
            name: "loginUser".to_string(),
            kind: SymbolKind::Function,
            language: "typescript".to_string(),
            file: "src/auth.ts".into(),
            range: crate::models::TextRange {
                start_line: 1,
                start_column: 1,
                end_line: 10,
                end_column: 1,
            },
            signature: None,
            attributes: Some(SymbolAttributes {
                comment: None,
                comment_range: None,
                keywords: Vec::new(),
                description: Some(
                    "Performs user authentication and issues JWTs".to_string(),
                ),
            }),
            def_line_count: None,
            matches: Vec::new(),
            calls: Vec::new(),
            called_by: Vec::new(),
        };

        let expr = term(QueryField::Description, "issues JWTs");
        assert!(symbol_matches_with_text(&expr, &symbol, None, false));
    }

    #[test]
    fn parse_calls_and_called_by_fields() {
        let expr = parse_query_expr("calls:foo called-by:bar|baz").expect("expr");
        match expr {
            QueryExpr::And(groups) => {
                assert_eq!(groups.len(), 2);
                assert!(matches!(
                    &groups[0],
                    QueryExpr::Term(QueryTerm {
                        field: QueryField::Calls,
                        value
                    }) if value == "foo"
                ));
                match &groups[1] {
                    QueryExpr::Or(alts) => {
                        assert_eq!(alts.len(), 2);
                        for alt in alts {
                            match alt {
                                QueryExpr::Term(QueryTerm { field, value }) => {
                                    assert_eq!(*field, QueryField::CalledBy);
                                    assert!(value == "bar" || value == "baz");
                                }
                                _ => panic!("expected called-by term in OR group"),
                            }
                        }
                    }
                    _ => panic!("expected OR group for called-by"),
                }
            }
            _ => panic!("expected AND expression for calls/called-by"),
        }
    }

    #[test]
    fn calls_and_called_by_terms_match_call_metadata() {
        use crate::models::CallRef;

        let mut symbol = Symbol {
            name: "foo".to_string(),
            kind: SymbolKind::Function,
            language: "typescript".to_string(),
            file: "src/lib.ts".into(),
            range: crate::models::TextRange {
                start_line: 1,
                start_column: 1,
                end_line: 5,
                end_column: 1,
            },
            signature: None,
            attributes: None,
            def_line_count: None,
            matches: Vec::new(),
            calls: Vec::new(),
            called_by: Vec::new(),
        };

        symbol.calls.push(CallRef {
            name: "bar".to_string(),
            file: "src/lib.ts".into(),
            line: Some(3),
            kind: Some(SymbolKind::Function),
        });

        symbol.called_by.push(CallRef {
            name: "qux".to_string(),
            file: "src/lib.ts".into(),
            line: Some(7),
            kind: Some(SymbolKind::Function),
        });

        let expr_calls = term(QueryField::Calls, "bar");
        let expr_calls_exact = term(QueryField::Calls, "=bar");
        let expr_calls_other = term(QueryField::Calls, "baz");

        assert!(symbol_matches_metadata(&expr_calls, &symbol, false));
        assert!(symbol_matches_metadata(&expr_calls_exact, &symbol, false));
        assert!(!symbol_matches_metadata(&expr_calls_other, &symbol, false));

        let expr_called_by = term(QueryField::CalledBy, "qux");
        let expr_called_by_exact = term(QueryField::CalledBy, "=qux");
        let expr_called_by_other = term(QueryField::CalledBy, "other");

        assert!(symbol_matches_metadata(&expr_called_by, &symbol, false));
        assert!(symbol_matches_metadata(&expr_called_by_exact, &symbol, false));
        assert!(!symbol_matches_metadata(&expr_called_by_other, &symbol, false));
    }
}
