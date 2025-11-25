use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use std::{fs, path::PathBuf};

fn fixture_dir() -> PathBuf {
    PathBuf::from("tests/fixtures/text_repo")
}

fn literal_fixture_dir() -> PathBuf {
    PathBuf::from("tests/fixtures/text_literal_repo")
}

fn symbol_literal_fixture_dir() -> PathBuf {
    PathBuf::from("tests/fixtures/symbol_literal_repo")
}

fn sort_matches(value: &mut Value) {
    if let Some(array) = value.get_mut("matches").and_then(|v| v.as_array_mut()) {
        array.sort_by(|a, b| {
            let path_a = a.get("path").and_then(|v| v.as_str()).unwrap_or_default();
            let path_b = b.get("path").and_then(|v| v.as_str()).unwrap_or_default();

            let line_a = a.get("line").and_then(|v| v.as_u64()).unwrap_or_default();
            let line_b = b.get("line").and_then(|v| v.as_u64()).unwrap_or_default();

            path_a.cmp(path_b).then_with(|| line_a.cmp(&line_b))
        });
    }
}

fn sort_symbols(value: &mut Value) {
    if let Some(array) = value.get_mut("symbols").and_then(|v| v.as_array_mut()) {
        array.sort_by(|a, b| {
            let file_a = a.get("file").and_then(|v| v.as_str()).unwrap_or_default();
            let file_b = b.get("file").and_then(|v| v.as_str()).unwrap_or_default();

            let line_a = a
                .get("range")
                .and_then(|r| r.get("start_line"))
                .and_then(|v| v.as_u64())
                .unwrap_or_default();
            let line_b = b
                .get("range")
                .and_then(|r| r.get("start_line"))
                .and_then(|v| v.as_u64())
                .unwrap_or_default();

            file_a.cmp(file_b).then_with(|| line_a.cmp(&line_b))
        });
    }
}

fn sort_contexts(value: &mut Value) {
    if let Some(array) = value.get_mut("contexts").and_then(|v| v.as_array_mut()) {
        array.sort_by(|a, b| {
            let file_a = a.get("file").and_then(|v| v.as_str()).unwrap_or_default();
            let file_b = b.get("file").and_then(|v| v.as_str()).unwrap_or_default();

            let line_a = a
                .get("range")
                .and_then(|r| r.get("start_line"))
                .and_then(|v| v.as_u64())
                .unwrap_or_default();
            let line_b = b
                .get("range")
                .and_then(|r| r.get("start_line"))
                .and_then(|v| v.as_u64())
                .unwrap_or_default();

            file_a.cmp(file_b).then_with(|| line_a.cmp(&line_b))
        });
    }
}

fn normalize_search_result(value: &mut Value) {
    sort_matches(value);
    sort_symbols(value);
    sort_contexts(value);
}

fn find_symbol_index_by_name<'a>(value: &'a Value, name: &str, language: &str) -> Option<usize> {
    let symbols = value.get("symbols")?.as_array()?;
    for (idx, symbol) in symbols.iter().enumerate() {
        if symbol.get("name") == Some(&Value::String(name.to_string()))
            && symbol.get("language") == Some(&Value::String(language.to_string()))
        {
            return Some(idx);
        }
    }
    None
}

fn tokenize_table_output(s: &str) -> Vec<Vec<String>> {
    s.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.split_whitespace().map(|t| t.to_string()).collect())
        .collect()
}

#[test]
fn cli_search_text_outputs_grep_like_lines() {
    let fixture_dir = fixture_dir();

    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "foo",
        "--path",
        fixture_dir.to_str().unwrap(),
        "--format",
        "text",
    ]);

    let assert = cmd.assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf-8");

    let mut lines: Vec<&str> = output.lines().collect();
    lines.sort();

    assert_eq!(
        lines,
        vec![
            "tests/fixtures/text_repo/a.txt:1:1: foo",
            "tests/fixtures/text_repo/b.txt:2:1: foo bar",
        ]
    );
}

#[test]
fn cli_search_text_literal_matches_whole_identifiers_only() {
    let fixture_dir = literal_fixture_dir();

    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "foo",
        "--path",
        fixture_dir.to_str().unwrap(),
        "--format",
        "text",
        "--literal",
    ]);

    let assert = cmd.assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf-8");

    let mut lines: Vec<&str> = output.lines().collect();
    lines.sort();

    // Expect matches for stand-alone `foo` and `foo()` but not
    // identifier substrings like `foobar`, `foo_bar`, or `bar_foo`.
    assert_eq!(
        lines,
        vec![
            "tests/fixtures/text_literal_repo/literal.txt:1:1: foo",
            "tests/fixtures/text_literal_repo/literal.txt:5:1: foo()",
        ]
    );
}

#[test]
fn cli_search_symbol_literal_enforces_exact_symbol_names() {
    let fixture_dir = symbol_literal_fixture_dir();

    // Without --literal, substring semantics should match both `add` and `adder`.
    let mut cmd_non_literal = cargo_bin_cmd!("symgrep");
    cmd_non_literal.args([
        "search",
        "name:add",
        "--path",
        fixture_dir.to_str().unwrap(),
        "--mode",
        "symbol",
        "--view",
        "meta",
        "--format",
        "json",
    ]);

    let assert_non_literal = cmd_non_literal.assert().success();
    let value_non_literal: Value =
        serde_json::from_slice(&assert_non_literal.get_output().stdout).expect("valid json output");

    let symbols_non_literal = value_non_literal["symbols"]
        .as_array()
        .expect("symbols array");
    let names_non_literal: Vec<&str> = symbols_non_literal
        .iter()
        .map(|s| s["name"].as_str().expect("name string"))
        .collect();

    assert!(
        names_non_literal.contains(&"add"),
        "expected non-literal search to include 'add'"
    );
    assert!(
        names_non_literal.contains(&"adder"),
        "expected non-literal search to include 'adder'"
    );

    // With --literal, only exact `add` symbols should match.
    let mut cmd_literal = cargo_bin_cmd!("symgrep");
    cmd_literal.args([
        "search",
        "name:add",
        "--path",
        fixture_dir.to_str().unwrap(),
        "--mode",
        "symbol",
        "--view",
        "meta",
        "--format",
        "json",
        "--literal",
    ]);

    let assert_literal = cmd_literal.assert().success();
    let value_literal: Value =
        serde_json::from_slice(&assert_literal.get_output().stdout).expect("valid json output");

    let symbols_literal = value_literal["symbols"].as_array().expect("symbols array");
    let names_literal: Vec<&str> = symbols_literal
        .iter()
        .map(|s| s["name"].as_str().expect("name string"))
        .collect();

    assert!(
        names_literal.iter().all(|name| *name == "add"),
        "expected literal search to return only 'add' symbols, got {:?}",
        names_literal
    );
}

#[test]
fn cli_search_json_outputs_structured_result_with_version() {
    let fixture_dir = fixture_dir();

    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "foo",
        "--path",
        fixture_dir.to_str().unwrap(),
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    assert_eq!(value["version"], "1.2.0");
    assert_eq!(value["query"], "foo");

    let matches = value["matches"].as_array().expect("matches array");
    assert_eq!(matches.len(), 2);

    assert_eq!(value["summary"]["total_matches"], 2);
    assert_eq!(value["summary"]["truncated"], false);
}

#[test]
fn cli_search_json_respects_limit_and_truncated_flag() {
    let fixture_dir = fixture_dir();

    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "foo",
        "--path",
        fixture_dir.to_str().unwrap(),
        "--format",
        "json",
        "--limit",
        "1",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let matches = value["matches"].as_array().expect("matches array");
    assert_eq!(matches.len(), 1);

    assert_eq!(value["summary"]["total_matches"], 1);
    assert_eq!(value["summary"]["truncated"], true);
}

#[test]
fn cli_search_json_respects_max_lines_zero() {
    let fixture_dir = fixture_dir();

    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "foo",
        "--path",
        fixture_dir.to_str().unwrap(),
        "--format",
        "json",
        "--max-lines",
        "0",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let matches = value["matches"].as_array().expect("matches array");
    assert_eq!(matches.len(), 2);

    for m in matches {
        assert!(m["snippet"].is_null());
    }
}

#[test]
fn cli_search_text_respects_glob_inclusion() {
    let fixture_dir = fixture_dir();

    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "foo",
        "--path",
        fixture_dir.to_str().unwrap(),
        "--format",
        "text",
        "--glob",
        "*a.txt",
    ]);

    let assert = cmd.assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf-8");

    let lines: Vec<&str> = output.lines().collect();

    assert_eq!(lines, vec!["tests/fixtures/text_repo/a.txt:1:1: foo"]);
}

#[test]
fn cli_search_text_respects_exclude_globs() {
    let fixture_dir = fixture_dir();

    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "foo",
        "--path",
        fixture_dir.to_str().unwrap(),
        "--format",
        "text",
        "--exclude",
        "*a.txt",
    ]);

    let assert = cmd.assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf-8");

    let lines: Vec<&str> = output.lines().collect();

    assert_eq!(lines, vec!["tests/fixtures/text_repo/b.txt:2:1: foo bar"]);
}

#[test]
fn cli_search_json_supports_multiple_paths() {
    let fixture_dir = fixture_dir();
    let path_a = fixture_dir.join("a.txt");
    let path_b = fixture_dir.join("b.txt");

    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "foo",
        "--path",
        path_a.to_str().unwrap(),
        "--path",
        path_b.to_str().unwrap(),
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let matches = value["matches"].as_array().expect("matches array");
    assert_eq!(matches.len(), 2);

    let mut paths: Vec<&str> = matches
        .iter()
        .map(|m| m["path"].as_str().expect("path string"))
        .collect();
    paths.sort();

    assert_eq!(
        paths,
        vec![
            "tests/fixtures/text_repo/a.txt",
            "tests/fixtures/text_repo/b.txt"
        ]
    );
}

#[test]
fn cli_search_json_matches_snapshot_for_text_repo() {
    let fixture_dir = fixture_dir();

    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "foo",
        "--path",
        fixture_dir.to_str().unwrap(),
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let mut actual: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let snapshot =
        fs::read_to_string("tests/snapshots/text_search_foo.json").expect("snapshot file");
    let mut expected: Value = serde_json::from_str(&snapshot).expect("valid json snapshot");

    normalize_search_result(&mut actual);
    normalize_search_result(&mut expected);

    assert_eq!(actual, expected);
}

#[test]
fn cli_search_json_limit_and_max_lines_matches_snapshot_for_text_repo() {
    let fixture_dir = fixture_dir();

    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "foo",
        "--path",
        fixture_dir.to_str().unwrap(),
        "--format",
        "json",
        "--limit",
        "1",
        "--max-lines",
        "0",
    ]);

    let assert = cmd.assert().success();
    let mut actual: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let snapshot = fs::read_to_string("tests/snapshots/agent_text_limit_max_lines.json")
        .expect("snapshot file");
    let mut expected: Value = serde_json::from_str(&snapshot).expect("valid json snapshot");

    normalize_search_result(&mut actual);
    normalize_search_result(&mut expected);

    assert_eq!(actual, expected);
}

#[test]
fn cli_search_text_matches_snapshot_for_text_repo() {
    let fixture_dir = fixture_dir();

    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "foo",
        "--path",
        fixture_dir.to_str().unwrap(),
        "--format",
        "text",
    ]);

    let assert = cmd.assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf-8");

    let snapshot =
        fs::read_to_string("tests/snapshots/text_search_foo.txt").expect("snapshot file");

    let mut actual_lines: Vec<&str> = output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    actual_lines.sort();

    let mut expected_lines: Vec<&str> = snapshot
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    expected_lines.sort();

    assert_eq!(actual_lines, expected_lines);
}

#[test]
fn cli_search_text_with_context_prints_context_blocks() {
    let fixture_dir = fixture_dir();

    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "foo",
        "--path",
        fixture_dir.to_str().unwrap(),
        "--format",
        "text",
        "--context",
        "1",
    ]);

    let assert = cmd.assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf-8");

    let snapshot = fs::read_to_string("tests/snapshots/text_search_foo_context_1.txt")
        .expect("snapshot file");

    assert_eq!(output.trim(), snapshot.trim());
}

#[test]
fn cli_search_text_mixed_repo_matches_all_languages() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "name:add kind:function",
        "--path",
        "tests/fixtures/mixed_repo",
        "--mode",
        "symbol",
        "--format",
        "text",
    ]);

    let assert = cmd.assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf-8");

    let mut lines: Vec<&str> = output.lines().collect();
    lines.sort();

    // Expect exactly one match per file in the mixed repo.
    assert_eq!(
        lines.len(),
        3,
        "expected exactly three matches (one per language) in mixed_repo"
    );

    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("tests/fixtures/mixed_repo/sample.cpp:") && l.contains("add")),
        "expected C++ match in mixed_repo"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("tests/fixtures/mixed_repo/simple.ts:") && l.contains("add")),
        "expected TypeScript match in mixed_repo"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("tests/fixtures/mixed_repo/simple.js:") && l.contains("add")),
        "expected JavaScript match in mixed_repo"
    );
}

#[test]
fn cli_search_table_output_matches_snapshot_for_single_text_match() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "foo",
        "--path",
        "tests/fixtures/text_repo/a.txt",
        "--format",
        "table",
    ]);

    let assert = cmd.assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf-8");

    let snapshot =
        fs::read_to_string("tests/snapshots/text_search_foo_table.txt").expect("snapshot file");

    let actual_tokens = tokenize_table_output(&output);
    let expected_tokens = tokenize_table_output(&snapshot);

    assert_eq!(actual_tokens, expected_tokens);
}

#[test]
fn cli_search_table_output_for_symbol_includes_context_name() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "name:increment kind:method",
        "--path",
        "tests/fixtures/cpp_repo",
        "--language",
        "cpp",
        "--mode",
        "symbol",
        "--view",
        "parent",
        "--format",
        "table",
    ]);

    let assert = cmd.assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf-8");
    let tokens = tokenize_table_output(&output);

    assert!(
        tokens.len() >= 2,
        "expected header and at least one data row in table output"
    );

    let row = &tokens[1];
    assert!(
        row.get(0)
            .map(|v| v.ends_with("sample.cpp"))
            .unwrap_or(false),
        "expected FILE column to end with sample.cpp"
    );
    assert_eq!(row.get(2).map(String::as_str), Some("method"));
    assert_eq!(row.get(3).map(String::as_str), Some("increment"));
    assert_eq!(row.get(4).map(String::as_str), Some("Widget"));
}

#[test]
fn cli_search_symbol_ts_json_includes_symbols_and_decl_context() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "name:=add kind:function",
        "--path",
        "tests/fixtures/ts_js_repo",
        "--language",
        "typescript",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let symbols = value["symbols"].as_array().expect("symbols array");
    assert_eq!(symbols.len(), 1);

    let symbol = &symbols[0];
    assert_eq!(symbol["name"], "add");
    assert_eq!(symbol["language"], "typescript");

    let contexts = value["contexts"].as_array().expect("contexts array");
    assert_eq!(contexts.len(), 1);
    assert!(contexts[0]["snippet"]
        .as_str()
        .unwrap_or_default()
        .contains("export function add"));
}

#[test]
fn cli_search_symbol_ts_multiline_decl_includes_full_signature() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "name:multilineAdd kind:function",
        "--path",
        "tests/fixtures/ts_js_repo",
        "--language",
        "typescript",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let symbols = value["symbols"].as_array().expect("symbols array");
    assert_eq!(symbols.len(), 1);

    let symbol = &symbols[0];
    assert_eq!(symbol["name"], "multilineAdd");
    assert_eq!(symbol["language"], "typescript");

    let contexts = value["contexts"].as_array().expect("contexts array");
    assert_eq!(contexts.len(), 1);
    let snippet = contexts[0]["snippet"].as_str().expect("snippet string");

    assert!(
        snippet.contains("export function multilineAdd"),
        "expected decl snippet to include function header"
    );
    assert!(
        snippet.contains("a: number") && snippet.contains("b: number"),
        "expected decl snippet to include parameters across lines"
    );
    assert!(
        snippet.lines().count() >= 2,
        "expected multi-line decl snippet for TypeScript"
    );
}

#[test]
fn cli_search_symbol_ts_def_populates_def_line_count_in_json() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "name:multilineAdd kind:function",
        "--path",
        "tests/fixtures/ts_js_repo",
        "--language",
        "typescript",
        "--mode",
        "symbol",
        "--view",
        "def",
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let symbols = value["symbols"].as_array().expect("symbols array");
    assert_eq!(symbols.len(), 1);

    let symbol = &symbols[0];
    assert_eq!(symbol["name"], "multilineAdd");
    assert_eq!(symbol["language"], "typescript");

    let def_line_count = symbol["def_line_count"]
        .as_u64()
        .expect("def_line_count present");

    let contexts = value["contexts"].as_array().expect("contexts array");
    assert_eq!(contexts.len(), 1);
    let snippet = contexts[0]["snippet"].as_str().expect("snippet string");

    let snippet_lines = snippet.lines().count() as u64;
    assert_eq!(
        def_line_count, snippet_lines,
        "def_line_count should match number of lines in Def snippet"
    );
}

#[test]
fn cli_search_symbol_js_respects_context_none() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "name:=add kind:function",
        "--path",
        "tests/fixtures/ts_js_repo",
        "--language",
        "javascript",
        "--mode",
        "symbol",
        "--view",
        "meta",
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let symbols = value["symbols"].as_array().expect("symbols array");
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0]["name"], "add");
    assert_eq!(symbols[0]["language"], "javascript");

    let contexts = value["contexts"].as_array().expect("contexts array");
    assert_eq!(contexts.len(), 0);
}

#[test]
fn cli_search_symbol_js_multiline_decl_includes_full_signature() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "name:multilineAdd kind:function",
        "--path",
        "tests/fixtures/ts_js_repo",
        "--language",
        "javascript",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let symbols = value["symbols"].as_array().expect("symbols array");
    assert_eq!(symbols.len(), 1);

    let symbol = &symbols[0];
    assert_eq!(symbol["name"], "multilineAdd");
    assert_eq!(symbol["language"], "javascript");

    let contexts = value["contexts"].as_array().expect("contexts array");
    assert_eq!(contexts.len(), 1);
    let snippet = contexts[0]["snippet"].as_str().expect("snippet string");

    assert!(
        snippet.contains("function multilineAdd"),
        "expected decl snippet to include function header"
    );
    assert!(
        snippet.contains("a") && snippet.contains("b"),
        "expected decl snippet to include parameters across lines"
    );
    assert!(
        snippet.lines().count() >= 2,
        "expected multi-line decl snippet for JavaScript"
    );
}

#[test]
fn cli_search_symbol_cpp_json_includes_symbols_and_decl_context() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "add",
        "--path",
        "tests/fixtures/cpp_repo",
        "--language",
        "cpp",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let symbols = value["symbols"].as_array().expect("symbols array");
    assert!(!symbols.is_empty());

    let add_index = find_symbol_index_by_name(&value, "add", "cpp").expect("add symbol");
    let contexts = value["contexts"].as_array().expect("contexts array");

    let context = contexts
        .iter()
        .find(|c| c.get("symbol_index").and_then(|v| v.as_u64()) == Some(add_index as u64))
        .expect("context for add");

    let snippet = context["snippet"].as_str().expect("snippet string for add");
    assert!(snippet.contains("int add"));
}

#[test]
fn cli_search_symbol_rust_parent_context_includes_module_and_type() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "name:increment kind:method language:rust",
        "--path",
        "tests/fixtures/rust_repo",
        "--mode",
        "symbol",
        "--view",
        "parent",
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let symbols = value["symbols"].as_array().expect("symbols array");
    assert_eq!(symbols.len(), 1);

    let symbol = &symbols[0];
    assert_eq!(symbol["name"], "increment");
    assert_eq!(symbol["kind"], "method");
    assert_eq!(symbol["language"], "rust");

    let contexts = value["contexts"].as_array().expect("contexts array");
    assert_eq!(contexts.len(), 1);
    let context = &contexts[0];

    let chain = context["parent_chain"]
        .as_array()
        .expect("parent_chain array");
    assert!(
        chain.len() >= 3,
        "expected file, module and type in parent_chain"
    );

    let names: Vec<&str> = chain
        .iter()
        .map(|n| n["name"].as_str().expect("name string"))
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

    let snippet = context["snippet"].as_str().expect("snippet string");
    assert!(snippet.contains("impl Widget"));
    assert!(snippet.contains("fn increment"));
}

#[test]
fn cli_search_symbol_rust_trait_method_decl_includes_signature() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "name:greet kind:method language:rust",
        "--path",
        "tests/fixtures/rust_repo",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let symbols = value["symbols"].as_array().expect("symbols array");
    assert_eq!(symbols.len(), 1);

    let symbol = &symbols[0];
    assert_eq!(symbol["name"], "greet");
    assert_eq!(symbol["kind"], "method");
    assert_eq!(symbol["language"], "rust");

    let contexts = value["contexts"].as_array().expect("contexts array");
    assert_eq!(contexts.len(), 1);
    let context = &contexts[0];

    let snippet = context["snippet"].as_str().expect("snippet string");
    assert!(
        snippet.contains("fn greet(&self)"),
        "expected decl snippet to contain trait method signature"
    );
}

#[test]
fn cli_search_symbol_rust_deeply_nested_modules_build_full_parent_chain() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "name:depth kind:method language:rust",
        "--path",
        "tests/fixtures/rust_repo",
        "--mode",
        "symbol",
        "--view",
        "parent",
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let symbols = value["symbols"].as_array().expect("symbols array");
    assert_eq!(symbols.len(), 1);

    let symbol = &symbols[0];
    assert_eq!(symbol["name"], "depth");
    assert_eq!(symbol["kind"], "method");
    assert_eq!(symbol["language"], "rust");

    let contexts = value["contexts"].as_array().expect("contexts array");
    assert_eq!(contexts.len(), 1);
    let context = &contexts[0];

    let chain = context["parent_chain"]
        .as_array()
        .expect("parent_chain array");
    assert!(
        chain.len() >= 5,
        "expected file and deep module/type chain in parent_chain"
    );

    let names: Vec<&str> = chain
        .iter()
        .map(|n| n["name"].as_str().expect("name string"))
        .collect();

    assert_eq!(names[0], "lib.rs");
    assert!(
        names.iter().any(|n| *n == "deep"),
        "expected module 'deep' in parent_chain"
    );
    assert!(
        names.iter().any(|n| *n == "level1"),
        "expected module 'level1' in parent_chain"
    );
    assert!(
        names.iter().any(|n| *n == "level2"),
        "expected module 'level2' in parent_chain"
    );
    assert!(
        names.iter().any(|n| *n == "DeepWidget"),
        "expected type 'DeepWidget' in parent_chain"
    );

    let snippet = context["snippet"].as_str().expect("snippet string");
    assert!(snippet.contains("impl DeepWidget"));
    assert!(snippet.contains("fn depth"));
}

#[test]
fn cli_search_symbol_cpp_decl_text_includes_multiline_signature() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "name:multiline_function kind:function",
        "--path",
        "tests/fixtures/cpp_repo",
        "--language",
        "cpp",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "text",
    ]);

    let assert = cmd.assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf-8");

    assert!(
        output.contains("function multiline_function"),
        "expected header line for multiline_function"
    );
    assert!(
        output.contains("void multiline_function("),
        "expected decl snippet to include return type and name on first line"
    );
    assert!(
        output.contains("    int a,"),
        "expected decl snippet to include first parameter line"
    );
    assert!(
        output.contains("    int b"),
        "expected decl snippet to include second parameter line"
    );
}

#[test]
fn cli_search_symbol_cpp_def_context_includes_function_body() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "add",
        "--path",
        "tests/fixtures/cpp_repo",
        "--language",
        "cpp",
        "--mode",
        "symbol",
        "--view",
        "def",
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let add_index = find_symbol_index_by_name(&value, "add", "cpp").expect("add symbol");
    let contexts = value["contexts"].as_array().expect("contexts array");

    let context = contexts
        .iter()
        .find(|c| c.get("symbol_index").and_then(|v| v.as_u64()) == Some(add_index as u64))
        .expect("context for add");

    let snippet = context["snippet"].as_str().expect("snippet string for add");
    assert!(snippet.contains("return a + b;"));
    assert!(snippet.lines().count() >= 2);
}

#[test]
fn cli_search_symbol_def_text_header_includes_def_line_count() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "name:multilineAdd kind:function",
        "--path",
        "tests/fixtures/ts_js_repo",
        "--language",
        "typescript",
        "--mode",
        "symbol",
        "--view",
        "def",
        "--format",
        "text",
    ]);

    let assert = cmd.assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf-8");

    let header_line = output.lines().next().unwrap_or_default();
    assert!(
        header_line.contains("multilineAdd"),
        "expected header line for multilineAdd, got: {header_line}"
    );
    assert!(
        header_line.contains("(def:") && header_line.contains("lines)"),
        "expected header to include def line count suffix, got: {header_line}"
    );
}

#[test]
fn cli_search_symbol_def_text_truncates_body_with_max_lines() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "name:multilineAdd kind:function",
        "--path",
        "tests/fixtures/ts_js_repo",
        "--language",
        "typescript",
        "--mode",
        "symbol",
        "--view",
        "def",
        "--format",
        "text",
        "--max-lines",
        "2",
    ]);

    let assert = cmd.assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf-8");

    let mut lines = output.lines();
    let header_line = lines.next().unwrap_or_default();
    assert!(
        header_line.contains("multilineAdd"),
        "expected header line for multilineAdd, got: {header_line}"
    );

    let body_lines: Vec<&str> = lines.collect();
    assert_eq!(
        body_lines.len(),
        2,
        "expected exactly two body lines under header, got {}",
        body_lines.len()
    );
}

#[test]
fn cli_search_symbol_def_text_hides_body_when_max_lines_is_zero() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "name:multilineAdd kind:function",
        "--path",
        "tests/fixtures/ts_js_repo",
        "--language",
        "typescript",
        "--mode",
        "symbol",
        "--view",
        "def",
        "--format",
        "text",
        "--max-lines",
        "0",
    ]);

    let assert = cmd.assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf-8");

    let mut lines = output.lines();
    let header_line = lines.next().unwrap_or_default();
    assert!(
        header_line.contains("multilineAdd"),
        "expected header line for multilineAdd, got: {header_line}"
    );
    assert!(
        lines.next().is_none(),
        "expected no body lines when max-lines is 0"
    );
}

#[test]
fn cli_search_symbol_def_matches_respects_max_lines_for_match_lines() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "name:increment kind:method content:delta",
        "--path",
        "tests/fixtures/cpp_repo",
        "--language",
        "cpp",
        "--mode",
        "symbol",
        "--view",
        "def,matches",
        "--format",
        "text",
        "--max-lines",
        "1",
    ]);

    let assert = cmd.assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf-8");

    let mut lines = output.lines();
    let header_line = lines.next().unwrap_or_default();
    assert!(
        header_line.contains("increment"),
        "expected header line for increment, got: {header_line}"
    );

    let body_lines: Vec<&str> = lines.collect();
    assert_eq!(
        body_lines.len(),
        1,
        "expected exactly one match line under header when max-lines is 1, got {}",
        body_lines.len()
    );
}

#[test]
fn cli_search_symbol_def_json_ignores_max_lines() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "name:multilineAdd kind:function",
        "--path",
        "tests/fixtures/ts_js_repo",
        "--language",
        "typescript",
        "--mode",
        "symbol",
        "--view",
        "def",
        "--format",
        "json",
        "--max-lines",
        "1",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let symbols = value["symbols"].as_array().expect("symbols array");
    assert_eq!(symbols.len(), 1);

    let symbol = &symbols[0];
    assert_eq!(symbol["name"], "multilineAdd");
    assert_eq!(symbol["language"], "typescript");

    let def_line_count = symbol["def_line_count"]
        .as_u64()
        .expect("def_line_count present");

    let contexts = value["contexts"].as_array().expect("contexts array");
    assert_eq!(contexts.len(), 1);
    let snippet = contexts[0]["snippet"].as_str().expect("snippet string");

    let snippet_lines = snippet.lines().count() as u64;
    assert!(
        snippet_lines >= 2,
        "expected multi-line def snippet for multilineAdd"
    );
    assert_eq!(
        def_line_count, snippet_lines,
        "def_line_count should match full Def snippet line count even when max-lines is set"
    );
}

#[test]
fn cli_search_symbol_mixed_repo_auto_detects_all_languages() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "add",
        "--path",
        "tests/fixtures/mixed_repo",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let symbols = value["symbols"].as_array().expect("symbols array");
    assert_eq!(symbols.len(), 3);

    let mut languages: Vec<&str> = symbols
        .iter()
        .map(|s| s["language"].as_str().expect("language string"))
        .collect();
    languages.sort();
    assert_eq!(languages, vec!["cpp", "javascript", "typescript"]);

    let contexts = value["contexts"].as_array().expect("contexts array");
    assert_eq!(contexts.len(), 3);

    for lang in ["cpp", "javascript", "typescript"] {
        let idx =
            find_symbol_index_by_name(&value, "add", lang).expect("symbol index for language");
        let context = contexts
            .iter()
            .find(|c| c.get("symbol_index").and_then(|v| v.as_u64()) == Some(idx as u64))
            .expect("context for symbol");
        let snippet = context["snippet"].as_str().expect("snippet string");
        assert!(
            snippet.contains("add"),
            "expected snippet for {lang} symbol to contain 'add'"
        );
    }
}

#[test]
fn cli_search_symbol_mixed_repo_respects_language_filter() {
    let cases = [
        ("typescript", "typescript"),
        ("javascript", "javascript"),
        ("cpp", "cpp"),
    ];

    for (language_flag, expected_language) in cases {
        let mut cmd = cargo_bin_cmd!("symgrep");
        cmd.args([
            "search",
            "add",
            "--path",
            "tests/fixtures/mixed_repo",
            "--language",
            language_flag,
            "--mode",
            "symbol",
            "--view",
            "decl",
            "--format",
            "json",
        ]);

        let assert = cmd.assert().success();
        let value: Value =
            serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

        let symbols = value["symbols"].as_array().expect("symbols array");
        assert_eq!(
            symbols.len(),
            1,
            "expected exactly one symbol for language {language_flag}"
        );

        let symbol = &symbols[0];
        assert_eq!(symbol["name"], "add");
        assert_eq!(symbol["language"], expected_language);

        let contexts = value["contexts"].as_array().expect("contexts array");
        assert_eq!(
            contexts.len(),
            1,
            "expected exactly one context for language {language_flag}"
        );
        let snippet = contexts[0]["snippet"].as_str().expect("snippet string");
        assert!(
            snippet.contains("add"),
            "expected context snippet for {language_flag} to contain 'add'"
        );
    }
}

#[test]
fn cli_search_symbol_ts_auto_detection_matches_explicit_language() {
    let mut cmd_auto = cargo_bin_cmd!("symgrep");
    cmd_auto.args([
        "search",
        "add",
        "--path",
        "tests/fixtures/ts_js_repo/simple.ts",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
    ]);

    let assert_auto = cmd_auto.assert().success();
    let auto_value: Value =
        serde_json::from_slice(&assert_auto.get_output().stdout).expect("valid json output");

    let mut cmd_explicit = cargo_bin_cmd!("symgrep");
    cmd_explicit.args([
        "search",
        "add",
        "--path",
        "tests/fixtures/ts_js_repo/simple.ts",
        "--language",
        "typescript",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
    ]);

    let assert_explicit = cmd_explicit.assert().success();
    let explicit_value: Value =
        serde_json::from_slice(&assert_explicit.get_output().stdout).expect("valid json output");

    assert_eq!(
        auto_value, explicit_value,
        "auto-detected TypeScript results should match explicit --language"
    );
}

#[test]
fn cli_search_symbol_mixed_repo_supports_query_dsl_name_and_kind() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "name:add kind:function",
        "--path",
        "tests/fixtures/mixed_repo",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let symbols = value["symbols"].as_array().expect("symbols array");
    assert_eq!(symbols.len(), 3);

    let mut languages: Vec<&str> = symbols
        .iter()
        .map(|s| s["language"].as_str().expect("language string"))
        .collect();
    languages.sort();
    assert_eq!(languages, vec!["cpp", "javascript", "typescript"]);
}

#[test]
fn cli_search_symbol_mixed_repo_add_decl_matches_snapshot() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "name:add kind:function",
        "--path",
        "tests/fixtures/mixed_repo",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let mut actual: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let snapshot = fs::read_to_string("tests/snapshots/agent_symbol_add_mixed_decl.json")
        .expect("snapshot file");
    let mut expected: Value = serde_json::from_str(&snapshot).expect("valid json snapshot");

    normalize_search_result(&mut actual);
    normalize_search_result(&mut expected);

    assert_eq!(actual, expected);
}

#[test]
fn cli_search_symbol_mixed_repo_parent_context_exposes_parent_chain() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "name:add kind:function",
        "--path",
        "tests/fixtures/mixed_repo",
        "--mode",
        "symbol",
        "--view",
        "parent",
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let symbols = value["symbols"].as_array().expect("symbols array");
    assert_eq!(symbols.len(), 3);

    let contexts = value["contexts"].as_array().expect("contexts array");
    assert_eq!(contexts.len(), 3);

    for (language, file_name) in [
        ("cpp", "sample.cpp"),
        ("javascript", "simple.js"),
        ("typescript", "simple.ts"),
    ] {
        let idx =
            find_symbol_index_by_name(&value, "add", language).expect("symbol index for language");
        let context = contexts
            .iter()
            .find(|c| c.get("symbol_index").and_then(|v| v.as_u64()) == Some(idx as u64))
            .expect("context for symbol");

        let chain = context["parent_chain"]
            .as_array()
            .expect("parent_chain array");
        assert!(
            !chain.is_empty(),
            "parent_chain should not be empty for {language}"
        );
        assert_eq!(
            chain[0]["name"],
            Value::String(file_name.to_string()),
            "outermost parent should be file-level node for {language}"
        );

        let snippet = context["snippet"].as_str().expect("snippet string");
        assert!(
            snippet.contains("add"),
            "parent context snippet for {language} should contain 'add'"
        );
    }
}

#[test]
fn cli_search_symbol_cpp_parent_context_includes_namespace_and_class() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "name:increment kind:method",
        "--path",
        "tests/fixtures/cpp_repo",
        "--language",
        "cpp",
        "--mode",
        "symbol",
        "--view",
        "parent",
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let symbols = value["symbols"].as_array().expect("symbols array");
    assert_eq!(symbols.len(), 1);
    let symbol = &symbols[0];
    assert_eq!(symbol["name"], "increment");
    assert_eq!(symbol["kind"], "method");
    assert_eq!(symbol["language"], "cpp");

    let contexts = value["contexts"].as_array().expect("contexts array");
    assert_eq!(contexts.len(), 1);
    let context = &contexts[0];

    let chain = context["parent_chain"]
        .as_array()
        .expect("parent_chain array");
    assert!(
        chain.len() >= 3,
        "expected file, namespace and class in parent_chain"
    );

    let names: Vec<&str> = chain
        .iter()
        .map(|n| n["name"].as_str().expect("name string"))
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

    let snippet = context["snippet"].as_str().expect("snippet string");
    assert!(snippet.contains("struct Widget"));
    assert!(snippet.contains("int increment"));
}

#[test]
fn cli_search_symbol_cpp_parent_context_matches_snapshot() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "name:increment kind:method",
        "--path",
        "tests/fixtures/cpp_repo",
        "--language",
        "cpp",
        "--mode",
        "symbol",
        "--view",
        "parent",
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let mut actual: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let snapshot = fs::read_to_string("tests/snapshots/agent_cpp_increment_parent.json")
        .expect("snapshot file");
    let mut expected: Value = serde_json::from_str(&snapshot).expect("valid json snapshot");

    normalize_search_result(&mut actual);
    normalize_search_result(&mut expected);

    assert_eq!(actual, expected);
}

#[test]
fn cli_schema_version_flag_prints_current_version() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args(["--schema-version"]);

    let assert = cmd.assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf-8");

    assert!(
        output.contains("1.2.0"),
        "schema version output should include 1.2.0"
    );
}

#[test]
fn cli_search_symbol_comment_field_matches_doc_comments() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "comment:Adds",
        "--path",
        "tests/fixtures/ts_js_repo",
        "--language",
        "typescript",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let symbols = value["symbols"].as_array().expect("symbols array");
    assert!(
        !symbols.is_empty(),
        "expected at least one symbol for comment: query"
    );
    assert!(
        symbols
            .iter()
            .all(|s| s["name"] == "addWithDoc"),
        "expected only addWithDoc symbols for comment: query"
    );
}

#[test]
fn cli_search_symbol_view_decl_comment_shows_signature_and_comment() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "name:addWithDoc kind:function",
        "--path",
        "tests/fixtures/ts_js_repo",
        "--language",
        "typescript",
        "--mode",
        "symbol",
        "--view",
        "decl,comment",
        "--format",
        "text",
    ]);

    let assert = cmd.assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf-8");

    assert!(
        output.contains("tests/fixtures/ts_js_repo/doc_comments.ts"),
        "expected header for doc_comments.ts"
    );
    assert!(
        output.contains("export function addWithDoc"),
        "expected declaration line in output"
    );
    assert!(
        output.contains("/**"),
        "expected original doc comment delimiter in output"
    );

    let idx_comment = output
        .find("/**")
        .expect("expected comment block to be present");
    let idx_decl = output
        .find("export function addWithDoc")
        .expect("expected declaration to be present");
    assert!(
        idx_comment < idx_decl,
        "expected comment block to appear before declaration in output"
    );
}

#[test]
fn cli_search_symbol_view_def_matches_shows_matching_lines_only() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "name:increment kind:method content:delta",
        "--path",
        "tests/fixtures/cpp_repo",
        "--language",
        "cpp",
        "--mode",
        "symbol",
        "--view",
        "def,matches",
        "--format",
        "text",
    ]);

    let assert = cmd.assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf-8");

    // Ensure the matching line is present and uses a simple `line:  code`
    // prefix without an extra column offset.
    assert!(
        output.contains("return value + delta;"),
        "expected matching line for content:delta"
    );
    assert!(
        output
            .lines()
            .filter(|line| line.contains("return value + delta;"))
            .all(|line| !line.contains(":1:") && !line.contains(":2:") && !line.contains(":3:")
                && !line.contains(":4:") && !line.contains(":5:") && !line.contains(":6:")
                && !line.contains(":7:") && !line.contains(":8:") && !line.contains(":9:")),
        "expected def,matches snippet lines to omit column offsets"
    );
    assert!(
        !output.contains("struct Widget {"),
        "def,matches view should not print enclosing struct body"
    );
}

#[test]
fn cli_search_symbol_def_matches_with_context_prints_match_context() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "name:increment kind:method content:delta",
        "--path",
        "tests/fixtures/cpp_repo",
        "--language",
        "cpp",
        "--mode",
        "symbol",
        "--view",
        "def,matches",
        "--format",
        "text",
        "--context",
        "1",
    ]);

    let assert = cmd.assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf-8");

    assert!(
        output.contains("int increment(int delta) {"),
        "expected context line for increment body"
    );
    assert!(
        output.contains("return value + delta;"),
        "expected matching line for content:delta"
    );
    assert!(
        !output.contains("struct Widget {"),
        "def,matches context should not include enclosing struct when using --context"
    );
}

#[test]
fn cli_search_symbol_ts_call_metadata_populates_calls_and_called_by() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "kind:function",
        "--path",
        "tests/fixtures/call_graph_repo/ts_calls.ts",
        "--language",
        "typescript",
        "--mode",
        "symbol",
        "--view",
        "meta",
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let symbols = value["symbols"].as_array().expect("symbols array");
    assert_eq!(symbols.len(), 4, "expected four functions in ts_calls.ts");

    let foo = symbols
        .iter()
        .find(|s| s["name"] == "foo")
        .expect("foo symbol");
    let bar = symbols
        .iter()
        .find(|s| s["name"] == "bar")
        .expect("bar symbol");
    let baz = symbols
        .iter()
        .find(|s| s["name"] == "baz")
        .expect("baz symbol");
    let qux = symbols
        .iter()
        .find(|s| s["name"] == "qux")
        .expect("qux symbol");

    let foo_calls = foo["calls"].as_array().expect("foo.calls array");
    let foo_call_names: Vec<&str> = foo_calls
        .iter()
        .map(|c| c["name"].as_str().expect("call name"))
        .collect();
    assert!(
        foo_call_names.contains(&"bar") && foo_call_names.contains(&"baz"),
        "expected foo.calls to contain bar and baz, got {:?}",
        foo_call_names
    );

    let bar_called_by = bar["called_by"]
        .as_array()
        .expect("bar.called_by array");
    let bar_caller_names: Vec<&str> = bar_called_by
        .iter()
        .map(|c| c["name"].as_str().expect("caller name"))
        .collect();
    assert!(
        bar_caller_names.contains(&"foo"),
        "expected bar.called_by to contain foo, got {:?}",
        bar_caller_names
    );

    let baz_called_by = baz["called_by"]
        .as_array()
        .expect("baz.called_by array");
    let baz_caller_names: Vec<&str> = baz_called_by
        .iter()
        .map(|c| c["name"].as_str().expect("caller name"))
        .collect();
    assert!(
        baz_caller_names.contains(&"foo"),
        "expected baz.called_by to contain foo, got {:?}",
        baz_caller_names
    );

    let qux_calls = qux["calls"].as_array().expect("qux.calls array");
    let qux_call_names: Vec<&str> = qux_calls
        .iter()
        .map(|c| c["name"].as_str().expect("call name"))
        .collect();
    assert!(
        qux_call_names.contains(&"foo"),
        "expected qux.calls to contain foo, got {:?}",
        qux_call_names
    );

    let foo_called_by = foo["called_by"]
        .as_array()
        .expect("foo.called_by array");
    let foo_caller_names: Vec<&str> = foo_called_by
        .iter()
        .map(|c| c["name"].as_str().expect("caller name"))
        .collect();
    assert!(
        foo_caller_names.contains(&"qux"),
        "expected foo.called_by to contain qux, got {:?}",
        foo_caller_names
    );
}

#[test]
fn cli_search_symbol_ts_calls_and_called_by_filters() {
    // calls:foo should return the caller qux.
    let mut cmd_calls = cargo_bin_cmd!("symgrep");
    cmd_calls.args([
        "search",
        "calls:foo",
        "--path",
        "tests/fixtures/call_graph_repo/ts_calls.ts",
        "--language",
        "typescript",
        "--mode",
        "symbol",
        "--view",
        "meta",
        "--format",
        "json",
    ]);

    let assert_calls = cmd_calls.assert().success();
    let value_calls: Value =
        serde_json::from_slice(&assert_calls.get_output().stdout).expect("valid json output");

    let symbols_calls = value_calls["symbols"].as_array().expect("symbols array");
    assert_eq!(
        symbols_calls.len(),
        1,
        "expected exactly one symbol for calls:foo"
    );
    assert_eq!(symbols_calls[0]["name"], "qux");

    // called-by:foo should return the callees bar and baz.
    let mut cmd_called_by = cargo_bin_cmd!("symgrep");
    cmd_called_by.args([
        "search",
        "called-by:foo",
        "--path",
        "tests/fixtures/call_graph_repo/ts_calls.ts",
        "--language",
        "typescript",
        "--mode",
        "symbol",
        "--view",
        "meta",
        "--format",
        "json",
    ]);

    let assert_called_by = cmd_called_by.assert().success();
    let value_called_by: Value = serde_json::from_slice(&assert_called_by.get_output().stdout)
        .expect("valid json output");

    let symbols_called_by = value_called_by["symbols"]
        .as_array()
        .expect("symbols array");
    assert_eq!(
        symbols_called_by.len(),
        2,
        "expected exactly two symbols for called-by:foo"
    );

    let mut names: Vec<&str> = symbols_called_by
        .iter()
        .map(|s| s["name"].as_str().expect("name string"))
        .collect();
    names.sort();
    assert_eq!(names, vec!["bar", "baz"]);

    // callers:foo should behave like called-by:foo.
    let mut cmd_callers = cargo_bin_cmd!("symgrep");
    cmd_callers.args([
        "search",
        "callers:foo",
        "--path",
        "tests/fixtures/call_graph_repo/ts_calls.ts",
        "--language",
        "typescript",
        "--mode",
        "symbol",
        "--view",
        "meta",
        "--format",
        "json",
    ]);

    let assert_callers = cmd_callers.assert().success();
    let value_callers: Value =
        serde_json::from_slice(&assert_callers.get_output().stdout).expect("valid json output");

    let symbols_callers = value_callers["symbols"].as_array().expect("symbols array");
    let mut caller_names: Vec<&str> = symbols_callers
        .iter()
        .map(|s| s["name"].as_str().expect("name string"))
        .collect();
    caller_names.sort();
    assert_eq!(
        caller_names, names,
        "callers:foo should return the same symbols as called-by:foo"
    );
}
