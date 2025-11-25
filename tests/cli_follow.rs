use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;

#[test]
fn cli_follow_json_callees_for_foo_ts() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "follow",
        "name:foo kind:function",
        "--path",
        "tests/fixtures/call_graph_repo/ts_calls.ts",
        "--language",
        "typescript",
        "--direction",
        "callees",
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    assert_eq!(value["direction"], "callees");
    assert_eq!(value["query"], "name:foo kind:function");

    let targets = value["targets"].as_array().expect("targets array");
    assert_eq!(targets.len(), 1, "expected exactly one target symbol");

    let target = &targets[0];
    assert_eq!(target["symbol"]["name"], "foo");
    assert_eq!(target["symbol"]["language"], "typescript");

    let callees = target["callees"].as_array().expect("callees array");
    let mut callee_names: Vec<&str> = callees
        .iter()
        .map(|edge| edge["symbol"]["name"].as_str().expect("callee name"))
        .collect();
    callee_names.sort();
    assert_eq!(callee_names, vec!["bar", "baz"]);

    for edge in callees {
        let sites = edge["call_sites"].as_array().expect("call_sites array");
        assert!(
            !sites.is_empty(),
            "expected at least one call_site per callee"
        );
        for site in sites {
            assert_eq!(
                site["file"],
                Value::String("tests/fixtures/call_graph_repo/ts_calls.ts".to_string())
            );
            assert!(
                site["line"].as_u64().is_some(),
                "call_site.line should be an integer"
            );
        }
    }
}

#[test]
fn cli_follow_json_callers_for_foo_ts() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "follow",
        "name:foo kind:function",
        "--path",
        "tests/fixtures/call_graph_repo/ts_calls.ts",
        "--language",
        "typescript",
        "--direction",
        "callers",
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    assert_eq!(value["direction"], "callers");

    let targets = value["targets"].as_array().expect("targets array");
    assert_eq!(targets.len(), 1, "expected exactly one target symbol");

    let target = &targets[0];
    assert_eq!(target["symbol"]["name"], "foo");

    let callers = target["callers"].as_array().expect("callers array");
    assert_eq!(
        callers.len(),
        1,
        "expected exactly one direct caller of foo"
    );

    let caller = &callers[0];
    assert_eq!(caller["symbol"]["name"], "qux");

    let sites = caller["call_sites"].as_array().expect("call_sites array");
    assert!(
        !sites.is_empty(),
        "expected at least one call_site for qux -> foo"
    );
}

#[test]
fn cli_follow_text_callees_with_context_prints_call_site_blocks() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "follow",
        "name:foo kind:function",
        "--path",
        "tests/fixtures/call_graph_repo/ts_calls.ts",
        "--language",
        "typescript",
        "--direction",
        "callees",
        "--format",
        "text",
        "--context",
        "1",
    ]);

    let assert = cmd.assert().success();
    let output =
        String::from_utf8(assert.get_output().stdout.clone()).expect("utf-8 follow output");

    assert!(
        output.contains("Target: foo (function)"),
        "expected target header for foo in output"
    );
    assert!(
        output.contains("Callee: bar (symbol)"),
        "expected callee header for bar in output"
    );
    assert!(
        output.contains("Callee: baz (symbol)"),
        "expected callee header for baz in output"
    );

    // Expect context around the call sites within foo's body.
    assert!(
        output.contains("export function foo(): void {"),
        "expected context line for foo() body"
    );
    assert!(
        output.contains("bar();"),
        "expected call line for bar() within foo() body"
    );
    assert!(
        output.contains("baz();"),
        "expected call line for baz() within foo() body"
    );
}

#[test]
fn cli_follow_json_both_directions_for_foo_ts() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "follow",
        "name:foo kind:function",
        "--path",
        "tests/fixtures/call_graph_repo/ts_calls.ts",
        "--language",
        "typescript",
        "--direction",
        "both",
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    assert_eq!(value["direction"], "both");

    let targets = value["targets"].as_array().expect("targets array");
    assert_eq!(targets.len(), 1);
    let target = &targets[0];

    let callers = target["callers"].as_array().expect("callers array");
    assert!(
        callers
            .iter()
            .any(|c| c["symbol"]["name"] == "qux"),
        "expected qux to appear in callers for foo"
    );

    let callees = target["callees"].as_array().expect("callees array");
    let mut callee_names: Vec<&str> = callees
        .iter()
        .map(|edge| edge["symbol"]["name"].as_str().expect("callee name"))
        .collect();
    callee_names.sort();
    assert_eq!(callee_names, vec!["bar", "baz"]);
}

#[test]
fn cli_follow_json_no_matches_returns_empty_targets() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "follow",
        "name:doesNotExist kind:function",
        "--path",
        "tests/fixtures/call_graph_repo/ts_calls.ts",
        "--language",
        "typescript",
        "--direction",
        "callers",
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let targets = value["targets"].as_array().expect("targets array");
    assert!(
        targets.is_empty(),
        "expected no targets for non-matching pattern"
    );
}

#[test]
fn cli_follow_text_respects_max_lines_truncation() {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "follow",
        "name:foo kind:function",
        "--path",
        "tests/fixtures/call_graph_repo/ts_calls.ts",
        "--language",
        "typescript",
        "--direction",
        "callees",
        "--format",
        "text",
        "--context",
        "2",
        "--max-lines",
        "2",
    ]);

    let assert = cmd.assert().success();
    let output =
        String::from_utf8(assert.get_output().stdout.clone()).expect("utf-8 follow output");

    // Ensure that each callee block prints at most 2 context lines.
    let mut in_block = false;
    let mut current_block_lines = 0usize;

    for line in output.lines() {
        let trimmed = line.trim_start();

        if trimmed.starts_with("Callee:") {
            if in_block {
                assert!(
                    current_block_lines <= 2,
                    "expected at most 2 context lines per callee block, got {}:\n{}",
                    current_block_lines,
                    output
                );
            }
            in_block = true;
            current_block_lines = 0;
            continue;
        }

        if in_block {
            if trimmed.is_empty() {
                assert!(
                    current_block_lines <= 2,
                    "expected at most 2 context lines per callee block, got {}:\n{}",
                    current_block_lines,
                    output
                );
                in_block = false;
                current_block_lines = 0;
                continue;
            }

            if trimmed
                .chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
            {
                current_block_lines += 1;
            }
        }
    }

    if in_block {
        assert!(
            current_block_lines <= 2,
            "expected at most 2 context lines per callee block at EOF, got {}:\n{}",
            current_block_lines,
            output
        );
    }
}

#[test]
fn cli_follow_literal_requires_exact_name_match() {
    let mut cmd_non_literal = cargo_bin_cmd!("symgrep");
    cmd_non_literal.args([
        "follow",
        "name:add kind:function",
        "--path",
        "tests/fixtures/symbol_literal_repo",
        "--language",
        "typescript",
        "--direction",
        "callers",
        "--format",
        "json",
    ]);

    let assert_non_literal = cmd_non_literal.assert().success();
    let value_non_literal: Value =
        serde_json::from_slice(&assert_non_literal.get_output().stdout).expect("valid json");

    let targets_non_literal = value_non_literal["targets"]
        .as_array()
        .expect("targets array");
    let mut names_non_literal: Vec<&str> = targets_non_literal
        .iter()
        .map(|t| t["symbol"]["name"].as_str().expect("name string"))
        .collect();
    names_non_literal.sort();
    assert!(
        names_non_literal.contains(&"add")
            && names_non_literal.contains(&"adder"),
        "expected non-literal follow to include both add and adder, got {:?}",
        names_non_literal
    );

    let mut cmd_literal = cargo_bin_cmd!("symgrep");
    cmd_literal.args([
        "follow",
        "name:add",
        "--path",
        "tests/fixtures/symbol_literal_repo",
        "--language",
        "typescript",
        "--direction",
        "callers",
        "--format",
        "json",
        "--literal",
    ]);

    let assert_literal = cmd_literal.assert().success();
    let value_literal: Value =
        serde_json::from_slice(&assert_literal.get_output().stdout).expect("valid json");

    let targets_literal = value_literal["targets"]
        .as_array()
        .expect("targets array");
    let names_literal: Vec<&str> = targets_literal
        .iter()
        .map(|t| t["symbol"]["name"].as_str().expect("name string"))
        .collect();

    assert!(
        !names_literal.is_empty(),
        "expected at least one target for literal follow"
    );
    assert!(
        names_literal.iter().all(|name| *name == "add"),
        "expected literal follow to return only 'add' symbols, got {:?}",
        names_literal
    );
}
