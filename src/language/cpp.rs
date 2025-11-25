use std::path::Path;

use tree_sitter::{Node, Parser, TreeCursor};
use tree_sitter_cpp::LANGUAGE;

use crate::language::{
    collect_leading_comment, context_snippet_for_range, file_context_node, find_symbol_node,
    node_text_range, BackendError, BackendResult, LanguageBackend, ParsedFile,
};
use crate::models::{
    CallRef, ContextInfo, ContextKind, Symbol, SymbolAttributes, SymbolKind,
};

/// Tree-sitter backed language implementation for C++.
pub struct CppBackend;

/// Singleton instance used by the language registry.
pub static BACKEND: CppBackend = CppBackend;

fn cpp_symbol_name(file: &ParsedFile, node: Node) -> Option<String> {
    let source = file.source();

    if let Some(name_node) = node.child_by_field_name("name") {
        if let Ok(text) = name_node.utf8_text(source.as_bytes()) {
            return Some(text.to_string());
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "identifier" | "field_identifier" | "type_identifier" | "namespace_identifier" => {
                if let Ok(text) = child.utf8_text(source.as_bytes()) {
                    return Some(text.to_string());
                }
            }
            "function_declarator"
            | "pointer_declarator"
            | "reference_declarator"
            | "qualified_identifier" => {
                if let Some(name) = cpp_symbol_name(file, child) {
                    return Some(name);
                }
            }
            _ => {}
        }
    }

    None
}

fn cpp_is_method(node: Node) -> bool {
    let mut parent = node.parent();
    while let Some(p) = parent {
        match p.kind() {
            "class_specifier" | "struct_specifier" => return true,
            "translation_unit" => return false,
            _ => {
                parent = p.parent();
            }
        }
    }
    false
}

fn cpp_field_is_function(node: Node) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "function_declarator" {
            return true;
        }
    }
    false
}

fn cpp_decl_range(file: &ParsedFile, symbol_node: Node) -> Option<crate::models::TextRange> {
    let kind = symbol_node.kind();

    if kind == "function_definition" {
        let symbol_range = node_text_range(&symbol_node);
        let mut end_line = symbol_range.end_line;

        if let Some(body) = symbol_node.child_by_field_name("body") {
            let body_range = node_text_range(&body);
            if body_range.start_line > symbol_range.start_line {
                end_line = body_range.start_line.saturating_sub(1);
            } else {
                end_line = symbol_range.start_line;
            }
        }

        let lines: Vec<&str> = file.source().lines().collect();
        let end_idx = end_line
            .saturating_sub(1)
            .min(lines.len().saturating_sub(1) as u32) as usize;
        let end_text = lines.get(end_idx).copied().unwrap_or_default();
        let end_column = end_text.len() as u32 + 1;

        return Some(crate::models::TextRange {
            start_line: symbol_range.start_line,
            start_column: 1,
            end_line,
            end_column,
        });
    }

    if kind == "field_declaration" && cpp_field_is_function(symbol_node) {
        let symbol_range = node_text_range(&symbol_node);

        let lines: Vec<&str> = file.source().lines().collect();
        let end_idx = symbol_range
            .end_line
            .saturating_sub(1)
            .min(lines.len().saturating_sub(1) as u32) as usize;
        let end_text = lines.get(end_idx).copied().unwrap_or_default();
        let end_column = end_text.len() as u32 + 1;

        return Some(crate::models::TextRange {
            start_line: symbol_range.start_line,
            start_column: 1,
            end_line: symbol_range.end_line,
            end_column,
        });
    }

    None
}

fn cpp_visit_symbols(file: &ParsedFile, cursor: &mut TreeCursor, symbols: &mut Vec<Symbol>) {
    loop {
        let node = cursor.node();
        let kind = node.kind();

        let symbol_kind = match kind {
            "function_definition" => {
                if cpp_is_method(node) {
                    Some(SymbolKind::Method)
                } else {
                    Some(SymbolKind::Function)
                }
            }
            "field_declaration" if cpp_field_is_function(node) => Some(SymbolKind::Method),
            "class_specifier" | "struct_specifier" => Some(SymbolKind::Class),
            "namespace_definition" => Some(SymbolKind::Namespace),
            _ => None,
        };

        if let Some(kind) = symbol_kind {
            if let Some(name) = cpp_symbol_name(file, node) {
                let range = node_text_range(&node);
                let comment = collect_leading_comment(file.source(), range.start_line, |line| {
                    let trimmed = line.trim_start();
                    trimmed.starts_with("[[")
                });
                let attributes = comment.map(|(text, comment_range)| SymbolAttributes {
                    comment: Some(text),
                    comment_range: Some(comment_range),
                    keywords: Vec::new(),
                    description: None,
                });
                symbols.push(Symbol {
                    name,
                    kind,
                    language: file.language_id.to_string(),
                    file: file.path.clone(),
                    range,
                    signature: None,
                    attributes,
                    def_line_count: None,
                    matches: Vec::new(),
                    calls: Vec::new(),
                    called_by: Vec::new(),
                });
            }
        }

        if cursor.goto_first_child() {
            cpp_visit_symbols(file, cursor, symbols);
            cursor.goto_parent();
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

fn cpp_enclosing_symbol_index(
    symbols: &[Symbol],
    file_path: &Path,
    range: crate::models::TextRange,
) -> Option<usize> {
    let mut best: Option<(usize, u32)> = None;

    for (idx, symbol) in symbols.iter().enumerate() {
        if symbol.file != file_path {
            continue;
        }

        if !matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method) {
            continue;
        }

        if symbol.range.start_line <= range.start_line && symbol.range.end_line >= range.end_line {
            let span = symbol.range.end_line.saturating_sub(symbol.range.start_line);
            match best {
                None => best = Some((idx, span)),
                Some((_, best_span)) => {
                    if span <= best_span {
                        best = Some((idx, span));
                    }
                }
            }
        }
    }

    best.map(|(idx, _)| idx)
}

fn cpp_callee_name(file: &ParsedFile, call_node: Node) -> Option<String> {
    let source = file.source();
    let function = call_node.child_by_field_name("function")?;

    match function.kind() {
        "identifier" => function
            .utf8_text(source.as_bytes())
            .ok()
            .map(|s| s.to_string()),
        _ => None,
    }
}

fn cpp_attach_call_metadata(file: &ParsedFile, symbols: &mut [Symbol]) {
    if symbols.is_empty() {
        return;
    }

    for symbol in symbols.iter_mut() {
        symbol.calls.clear();
        symbol.called_by.clear();
    }

    let root = file.tree.root_node();
    let mut cursor = root.walk();
    let mut edges: Vec<(usize, String, u32)> = Vec::new();

    fn visit(
        file: &ParsedFile,
        symbols: &[Symbol],
        cursor: &mut TreeCursor,
        edges: &mut Vec<(usize, String, u32)>,
    ) {
        loop {
            let node = cursor.node();
            if node.kind() == "call_expression" {
                let range = crate::language::node_text_range(&node);
                if let Some(caller_idx) =
                    cpp_enclosing_symbol_index(symbols, &file.path, range)
                {
                    if let Some(callee) = cpp_callee_name(file, node) {
                        edges.push((caller_idx, callee, range.start_line));
                    }
                }
            }

            if cursor.goto_first_child() {
                visit(file, symbols, cursor, edges);
                cursor.goto_parent();
            }

            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }

    visit(file, symbols, &mut cursor, &mut edges);

    for (caller_idx, callee_name, line) in edges {
        if caller_idx >= symbols.len() {
            continue;
        }

        let caller_name = symbols[caller_idx].name.clone();
        let caller_file = symbols[caller_idx].file.clone();

        symbols[caller_idx].calls.push(CallRef {
            name: callee_name.clone(),
            file: caller_file.clone(),
            line: Some(line),
            kind: None,
        });

        for symbol in symbols.iter_mut() {
            if symbol.name == callee_name {
                symbol.called_by.push(CallRef {
                    name: caller_name.clone(),
                    file: caller_file.clone(),
                    line: Some(line),
                    kind: None,
                });
            }
        }
    }
}

fn cpp_context_node_for_ancestor(
    file: &ParsedFile,
    node: Node,
) -> Option<crate::models::ContextNode> {
    let kind = match node.kind() {
        "namespace_definition" => Some(SymbolKind::Namespace),
        "class_specifier" | "struct_specifier" => Some(SymbolKind::Class),
        _ => None,
    }?;

    let name = cpp_symbol_name(file, node)?;
    Some(crate::models::ContextNode {
        name,
        kind: Some(kind),
    })
}

fn cpp_parent_info<'a>(
    file: &'a ParsedFile,
    symbol_node: Node<'a>,
) -> (Option<Node<'a>>, Vec<crate::models::ContextNode>) {
    let root = file.tree.root_node();
    let mut parent_ctx_node: Option<Node> = None;
    let mut chain_rev: Vec<crate::models::ContextNode> = Vec::new();

    let mut current = symbol_node.parent();
    while let Some(node) = current {
        if let Some(ctx) = cpp_context_node_for_ancestor(file, node) {
            if parent_ctx_node.is_none() {
                parent_ctx_node = Some(node);
            }
            chain_rev.push(ctx);
        }
        current = node.parent();
    }

    let mut chain = Vec::new();
    chain.push(file_context_node(file));
    chain_rev.reverse();
    chain.extend(chain_rev);

    let parent_node = parent_ctx_node.or(Some(root));
    (parent_node, chain)
}

impl LanguageBackend for CppBackend {
    fn id(&self) -> &'static str {
        "cpp"
    }

    fn file_extensions(&self) -> &'static [&'static str] {
        &["cpp", "cc", "cxx", "hpp", "hh", "h", "hxx"]
    }

    fn parse_file(&self, path: &Path, source: &str) -> BackendResult<ParsedFile> {
        let mut parser = Parser::new();
        let language = LANGUAGE.into();
        parser.set_language(&language)?;

        let tree = parser
            .parse(source, None)
            .ok_or_else(|| BackendError::new("failed to parse C++ source"))?;

        if tree.root_node().has_error() {
            return Err(BackendError::new(
                "tree-sitter reported errors while parsing C++ source",
            ));
        }

        Ok(ParsedFile::new(self.id(), path, tree, source.to_string()))
    }

    fn index_symbols(&self, file: &ParsedFile) -> BackendResult<Vec<Symbol>> {
        let mut symbols = Vec::new();
        let mut cursor: TreeCursor = file.tree.root_node().walk();
        cpp_visit_symbols(file, &mut cursor, &mut symbols);
        cpp_attach_call_metadata(file, &mut symbols);
        Ok(symbols)
    }

    fn get_context_snippet(
        &self,
        file: &ParsedFile,
        symbol: &Symbol,
        kind: ContextKind,
    ) -> BackendResult<ContextInfo> {
        let symbol_node = match find_symbol_node(file, symbol) {
            Some(node) => node,
            None => return Ok(crate::language::basic_context_snippet(file, symbol, kind)),
        };

        let (parent_node, parent_chain) = cpp_parent_info(file, symbol_node);

        let mut context = match kind {
            ContextKind::Decl => {
                if let Some(range) = cpp_decl_range(file, symbol_node) {
                    context_snippet_for_range(file, &symbol.file, ContextKind::Decl, range)
                } else {
                    crate::language::basic_context_snippet(file, symbol, ContextKind::Decl)
                }
            }
            ContextKind::Def => crate::language::basic_context_snippet(file, symbol, ContextKind::Def),
            ContextKind::Parent => {
                if let Some(parent) = parent_node {
                    let range = node_text_range(&parent);
                    context_snippet_for_range(file, &symbol.file, ContextKind::Parent, range)
                } else {
                    crate::language::basic_context_snippet(file, symbol, ContextKind::Parent)
                }
            }
        };

        context.parent_chain = parent_chain;
        Ok(context)
    }
}
