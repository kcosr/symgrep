use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

fn copy_fixture_repo(name: &str) -> (tempfile::TempDir, PathBuf) {
    let src_root = PathBuf::from("tests/fixtures").join(name);
    let tmp = tempdir().expect("tempdir");
    let dst_root = tmp.path().join(name);
    fs::create_dir_all(&dst_root).expect("create dst_root");

    for entry in fs::read_dir(&src_root).expect("read src_root") {
        let entry = entry.expect("entry");
        let file_type = entry.file_type().expect("file_type");
        if file_type.is_file() {
            let file_name = entry.file_name();
            let dst_path = dst_root.join(file_name);
            fs::copy(entry.path(), &dst_path).expect("copy file");
        }
    }

    (tmp, dst_root)
}

#[test]
fn cli_search_uses_project_config_default_view_for_symbol_mode() {
    let (_tmp, repo_root) = copy_fixture_repo("cpp_repo");
    let symgrep_dir = repo_root.join(".symgrep");
    fs::create_dir_all(&symgrep_dir).expect("create .symgrep directory");

    let config_toml = r#"
[search]
paths = ["."]
language = "cpp"
mode = "symbol"
view = ["def", "matches"]
"#;
    fs::write(symgrep_dir.join("config.toml"), config_toml).expect("write config.toml");

    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.current_dir(&repo_root);
    cmd.args([
        "search",
        "name:increment kind:method content:delta",
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let symbols = value["symbols"].as_array().expect("symbols array");
    assert!(
        !symbols.is_empty(),
        "expected at least one symbol result in symbol mode"
    );

    let contexts = value["contexts"].as_array().expect("contexts array");
    assert!(
        !contexts.is_empty(),
        "expected at least one context snippet when using def view"
    );

    for context in contexts {
        assert_eq!(
            context["kind"], "def",
            "expected def context when config view includes def"
        );
    }

    let has_matches = symbols.iter().any(|s| {
        s["matches"]
            .as_array()
            .map(|arr| !arr.is_empty())
            .unwrap_or(false)
    });
    assert!(
        has_matches,
        "expected per-symbol matches when config view includes matches"
    );
}

#[test]
fn cli_search_cli_view_overrides_project_config_view() {
    let (_tmp, repo_root) = copy_fixture_repo("cpp_repo");
    let symgrep_dir = repo_root.join(".symgrep");
    fs::create_dir_all(&symgrep_dir).expect("create .symgrep directory");

    let config_toml = r#"
[search]
paths = ["."]
language = "cpp"
mode = "symbol"
view = ["def", "matches"]
"#;
    fs::write(symgrep_dir.join("config.toml"), config_toml).expect("write config.toml");

    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.current_dir(&repo_root);
    cmd.args([
        "search",
        "name:increment kind:method content:delta",
        "--mode",
        "symbol",
        "--language",
        "cpp",
        "--view",
        "decl",
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let contexts = value["contexts"].as_array().expect("contexts array");
    assert!(
        !contexts.is_empty(),
        "expected at least one context snippet when using symbol view"
    );

    for context in contexts {
        assert_eq!(
            context["kind"], "decl",
            "expected CLI --view decl to override config def,matches"
        );
    }
}

#[test]
fn cli_search_uses_project_config_defaults_for_paths_and_format() {
    let (_tmp, repo_root) = copy_fixture_repo("text_repo");
    let symgrep_dir = repo_root.join(".symgrep");
    fs::create_dir_all(&symgrep_dir).expect("create .symgrep directory");

    let config_toml = r#"
[search]
paths = ["."]
format = "json"
"#;
    fs::write(symgrep_dir.join("config.toml"), config_toml).expect("write config.toml");

    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.current_dir(&repo_root);
    cmd.args(["search", "foo"]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    assert_eq!(value["query"], "foo");

    let matches = value["matches"].as_array().expect("matches array");
    assert!(
        !matches.is_empty(),
        "expected at least one match when using config defaults"
    );
}

#[test]
fn cli_search_config_can_disable_server_even_with_env() {
    let (_tmp, repo_root) = copy_fixture_repo("text_repo");
    let symgrep_dir = repo_root.join(".symgrep");
    fs::create_dir_all(&symgrep_dir).expect("create .symgrep directory");

    let config_toml = r#"
[search]
paths = ["."]
format = "text"
no_server = true
"#;
    fs::write(symgrep_dir.join("config.toml"), config_toml).expect("write config.toml");

    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.current_dir(&repo_root);
    cmd.env("SYMGREP_SERVER_URL", "http://127.0.0.1:9");
    cmd.args(["search", "foo"]);

    let assert = cmd.assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf-8");

    assert!(
        output.contains("foo"),
        "expected local search output when config sets no_server = true"
    );
}
