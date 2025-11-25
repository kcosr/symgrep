use std::path::Path;

use tree_sitter::{Node, Parser, TreeCursor};
use tree_sitter_rust::LANGUAGE;

use crate::language::{
    collect_leading_comment, context_snippet_for_range, file_context_node, find_symbol_node,
    node_text_range, BackendError, BackendResult, LanguageBackend, ParsedFile,
};
use crate::models::{ContextInfo, ContextKind, Symbol, SymbolAttributes, SymbolKind};

/// Tree-sitter backed language implementation for Rust.
pub struct RustBackend;

/// Singleton instance used by the language registry.
pub static BACKEND: RustBackend = RustBackend;

fn rust_symbol_name(file: &ParsedFile, node: Node) -> Option<String> {
    let source = file.source();

    if let Some(name_node) = node.child_by_field_name("name") {
        if let Ok(text) = name_node.utf8_text(source.as_bytes()) {
            return Some(text.to_string());
        }
    }

    None
}

fn rust_has_self_parameter(node: Node) -> bool {
    if let Some(params) = node.child_by_field_name("parameters") {
        let mut cursor = params.walk();
        for child in params.children(&mut cursor) {
            if child.kind() == "self_parameter" {
                return true;
            }
        }
    }
    false
}

fn rust_function_kind(node: Node) -> SymbolKind {
    let mut in_impl = false;
    let mut in_trait = false;

    let mut parent = node.parent();
    while let Some(p) = parent {
        match p.kind() {
            "impl_item" => in_impl = true,
            "trait_item" => in_trait = true,
            _ => {}
        }
        parent = p.parent();
    }

    let has_self = rust_has_self_parameter(node);

    if (in_impl || in_trait) && has_self {
        SymbolKind::Method
    } else {
        SymbolKind::Function
    }
}

fn rust_decl_range(file: &ParsedFile, symbol_node: Node) -> Option<crate::models::TextRange> {
    let kind = symbol_node.kind();
    if kind != "function_item" && kind != "function_signature_item" {
        return None;
    }

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

    Some(crate::models::TextRange {
        start_line: symbol_range.start_line,
        start_column: 1,
        end_line,
        end_column,
    })
}

fn rust_type_name_from_type_node(file: &ParsedFile, type_node: Node) -> Option<String> {
    let source = file.source();

    match type_node.kind() {
        "type_identifier" => type_node
            .utf8_text(source.as_bytes())
            .ok()
            .map(|s| s.to_string()),
        "generic_type" => {
            if let Some(inner) = type_node.child_by_field_name("type") {
                rust_type_name_from_type_node(file, inner)
            } else {
                None
            }
        }
        "scoped_type_identifier" | "scoped_identifier" => {
            let mut cursor = type_node.walk();
            let mut last_ident: Option<String> = None;
            for child in type_node.children(&mut cursor) {
                if child.kind() == "type_identifier" || child.kind() == "identifier" {
                    if let Ok(text) = child.utf8_text(source.as_bytes()) {
                        last_ident = Some(text.to_string());
                    }
                }
            }
            last_ident
        }
        _ => {
            let mut cursor = type_node.walk();
            for child in type_node.children(&mut cursor) {
                if let Some(name) = rust_type_name_from_type_node(file, child) {
                    return Some(name);
                }
            }
            None
        }
    }
}

fn rust_impl_type_name(file: &ParsedFile, node: Node) -> Option<String> {
    if node.kind() != "impl_item" {
        return None;
    }
    let type_node = node.child_by_field_name("type")?;
    rust_type_name_from_type_node(file, type_node)
}

fn rust_visit_symbols(file: &ParsedFile, cursor: &mut TreeCursor, symbols: &mut Vec<Symbol>) {
    loop {
        let node = cursor.node();

        let symbol_kind = match node.kind() {
            "function_item" | "function_signature_item" => Some(rust_function_kind(node)),
            "struct_item" | "enum_item" | "union_item" | "type_item" => Some(SymbolKind::Class),
            "trait_item" => Some(SymbolKind::Interface),
            "mod_item" => Some(SymbolKind::Namespace),
            _ => None,
        };

        if let Some(kind) = symbol_kind {
            if let Some(name) = rust_symbol_name(file, node) {
                let range = node_text_range(&node);
                let comment = collect_leading_comment(file.source(), range.start_line, |line| {
                    let trimmed = line.trim_start();
                    trimmed.starts_with("#[") || trimmed.starts_with("#![")
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
            rust_visit_symbols(file, cursor, symbols);
            cursor.goto_parent();
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

fn rust_context_node_for_ancestor(
    file: &ParsedFile,
    node: Node,
) -> Option<crate::models::ContextNode> {
    let kind = match node.kind() {
        "mod_item" => Some(SymbolKind::Namespace),
        "struct_item" | "enum_item" | "union_item" | "type_item" => Some(SymbolKind::Class),
        "trait_item" => Some(SymbolKind::Interface),
        "function_item" | "function_signature_item" => Some(rust_function_kind(node)),
        "impl_item" => Some(SymbolKind::Class),
        _ => None,
    }?;

    let name = if node.kind() == "impl_item" {
        rust_impl_type_name(file, node)?
    } else {
        rust_symbol_name(file, node)?
    };

    Some(crate::models::ContextNode {
        name,
        kind: Some(kind),
    })
}

fn rust_parent_info<'a>(
    file: &'a ParsedFile,
    symbol_node: Node<'a>,
) -> (Option<Node<'a>>, Vec<crate::models::ContextNode>) {
    let root = file.tree.root_node();
    let mut parent_ctx_node: Option<Node> = None;
    let mut chain_rev: Vec<crate::models::ContextNode> = Vec::new();

    let mut current = symbol_node.parent();
    while let Some(node) = current {
        if let Some(ctx) = rust_context_node_for_ancestor(file, node) {
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

impl LanguageBackend for RustBackend {
    fn id(&self) -> &'static str {
        "rust"
    }

    fn file_extensions(&self) -> &'static [&'static str] {
        &["rs"]
    }

    fn parse_file(&self, path: &Path, source: &str) -> BackendResult<ParsedFile> {
        let mut parser = Parser::new();
        let language = LANGUAGE.into();
        parser.set_language(&language)?;

        let tree = parser
            .parse(source, None)
            .ok_or_else(|| BackendError::new("failed to parse Rust source"))?;

        if tree.root_node().has_error() {
            return Err(BackendError::new(
                "tree-sitter reported errors while parsing Rust source",
            ));
        }

        Ok(ParsedFile::new(self.id(), path, tree, source.to_string()))
    }

    fn index_symbols(&self, file: &ParsedFile) -> BackendResult<Vec<Symbol>> {
        let mut symbols = Vec::new();
        let mut cursor: TreeCursor = file.tree.root_node().walk();
        rust_visit_symbols(file, &mut cursor, &mut symbols);
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

        let (parent_node, parent_chain) = rust_parent_info(file, symbol_node);

        let mut context = match kind {
            ContextKind::Decl => {
                if let Some(range) = rust_decl_range(file, symbol_node) {
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
