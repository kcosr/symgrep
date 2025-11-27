use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
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

fn index_path_for(tmp: &tempfile::TempDir) -> PathBuf {
    tmp.path().join(".symgrep")
}

fn run_index(path: &Path, index_root: &Path) {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "index",
        "--path",
        path.to_str().unwrap(),
        "--index-backend",
        "file",
        "--index-path",
        index_root.to_str().unwrap(),
    ]);

    cmd.assert().success();
}

fn run_index_sqlite(path: &Path, db_path: &Path) {
    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.current_dir(path);
    cmd.args([
        "index",
        "--path",
        ".",
        "--index-backend",
        "sqlite",
        "--index-path",
        db_path.to_str().unwrap(),
    ]);

    cmd.assert().success();
}

#[test]
fn cli_index_builds_file_backend_layout() {
    let (tmp, repo_root) = copy_fixture_repo("ts_js_repo");
    let index_root = index_path_for(&tmp);

    run_index(&repo_root, &index_root);

    let meta_path = index_root.join("meta.json");
    let files_path = index_root.join("files.jsonl");
    let symbols_path = index_root.join("symbols.jsonl");

    assert!(meta_path.exists(), "meta.json should exist after indexing");
    assert!(
        files_path.exists(),
        "files.jsonl should exist after indexing"
    );
    assert!(
        symbols_path.exists(),
        "symbols.jsonl should exist after indexing"
    );

    let meta_file = fs::File::open(&meta_path).expect("open meta.json");
    let meta: Value = serde_json::from_reader(meta_file).expect("parse meta.json");

    assert_eq!(meta["schema_version"], "2");
}

#[test]
fn cli_index_builds_sqlite_backend_layout() {
    let (_tmp, repo_root) = copy_fixture_repo("ts_js_repo");
    let db_path = repo_root.join(".symgrep").join("index.sqlite");

    run_index_sqlite(&repo_root, &db_path);

    assert!(
        db_path.exists(),
        "sqlite index file should exist after indexing"
    );
}

#[test]
fn cli_index_prints_human_readable_summary() {
    let (_tmp, repo_root) = copy_fixture_repo("ts_js_repo");
    let db_path = repo_root.join(".symgrep").join("index.sqlite");

    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.current_dir(&repo_root);
    cmd.args([
        "index",
        "--path",
        ".",
        "--index-backend",
        "sqlite",
        "--index-path",
        db_path.to_str().unwrap(),
    ]);

    let assert = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);

    assert!(
        stdout.contains("Indexed "),
        "expected summary line to mention 'Indexed', got:\n{stdout}"
    );
    assert!(
        stdout.contains(" files and "),
        "expected summary line to mention file and symbol counts, got:\n{stdout}"
    );
    assert!(
        stdout.contains(" symbols using "),
        "expected summary line to mention backend kind, got:\n{stdout}"
    );
    assert!(
        stdout.contains(" backend at "),
        "expected summary line to mention index path, got:\n{stdout}"
    );
}

#[test]
fn cli_search_symbol_ts_with_index_matches_without_index() {
    let (tmp, repo_root) = copy_fixture_repo("ts_js_repo");
    let index_root = index_path_for(&tmp);

    run_index(&repo_root, &index_root);

    // Baseline search without index.
    let mut base_cmd = cargo_bin_cmd!("symgrep");
    base_cmd.args([
        "search",
        "add",
        "--path",
        repo_root.to_str().unwrap(),
        "--language",
        "typescript",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
    ]);

    let base_assert = base_cmd.assert().success();
    let mut base_value: Value =
        serde_json::from_slice(&base_assert.get_output().stdout).expect("valid json output");

    // Search using the file-based index.
    let mut index_cmd = cargo_bin_cmd!("symgrep");
    index_cmd.args([
        "search",
        "add",
        "--path",
        repo_root.to_str().unwrap(),
        "--language",
        "typescript",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
        "--use-index",
        "--index-backend",
        "file",
        "--index-path",
        index_root.to_str().unwrap(),
    ]);

    let index_assert = index_cmd.assert().success();
    let mut index_value: Value =
        serde_json::from_slice(&index_assert.get_output().stdout).expect("valid json output");

    normalize_search_result(&mut base_value);
    normalize_search_result(&mut index_value);

    assert_eq!(
        base_value, index_value,
        "indexed TypeScript symbol search should match non-indexed search"
    );
}

#[test]
fn cli_search_symbol_ts_with_sqlite_index_matches_without_index() {
    let (_tmp, repo_root) = copy_fixture_repo("ts_js_repo");
    let db_path = repo_root.join(".symgrep").join("index.sqlite");

    run_index_sqlite(&repo_root, &db_path);

    // Baseline search without index.
    let mut base_cmd = cargo_bin_cmd!("symgrep");
    base_cmd.current_dir(&repo_root);
    base_cmd.args([
        "search",
        "add",
        "--path",
        ".",
        "--language",
        "typescript",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
    ]);

    let base_assert = base_cmd.assert().success();
    let mut base_value: Value =
        serde_json::from_slice(&base_assert.get_output().stdout).expect("valid json output");

    // Search using the SQLite-based index.
    let mut index_cmd = cargo_bin_cmd!("symgrep");
    index_cmd.current_dir(&repo_root);
    index_cmd.args([
        "search",
        "add",
        "--path",
        ".",
        "--language",
        "typescript",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
        "--use-index",
        "--index-backend",
        "sqlite",
        "--index-path",
        ".symgrep/index.sqlite",
    ]);

    let index_assert = index_cmd.assert().success();
    let mut index_value: Value =
        serde_json::from_slice(&index_assert.get_output().stdout).expect("valid json output");

    normalize_search_result(&mut base_value);
    normalize_search_result(&mut index_value);

    assert_eq!(
        base_value, index_value,
        "SQLite indexed TypeScript symbol search should match non-indexed search"
    );
}

#[test]
fn cli_search_symbol_rust_with_index_matches_without_index() {
    let (tmp, repo_root) = copy_fixture_repo("rust_repo");
    let index_root = index_path_for(&tmp);

    run_index(&repo_root, &index_root);

    // Baseline search without index.
    let mut base_cmd = cargo_bin_cmd!("symgrep");
    base_cmd.args([
        "search",
        "add",
        "--path",
        repo_root.to_str().unwrap(),
        "--language",
        "rust",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
    ]);

    let base_assert = base_cmd.assert().success();
    let mut base_value: Value =
        serde_json::from_slice(&base_assert.get_output().stdout).expect("valid json output");

    // Search using the file-based index.
    let mut index_cmd = cargo_bin_cmd!("symgrep");
    index_cmd.args([
        "search",
        "add",
        "--path",
        repo_root.to_str().unwrap(),
        "--language",
        "rust",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
        "--use-index",
        "--index-backend",
        "file",
        "--index-path",
        index_root.to_str().unwrap(),
    ]);

    let index_assert = index_cmd.assert().success();
    let mut index_value: Value =
        serde_json::from_slice(&index_assert.get_output().stdout).expect("valid json output");

    normalize_search_result(&mut base_value);
    normalize_search_result(&mut index_value);

    assert_eq!(
        base_value, index_value,
        "indexed Rust symbol search should match non-indexed search"
    );
}

#[test]
fn cli_search_symbol_rust_with_sqlite_index_matches_without_index() {
    let (_tmp, repo_root) = copy_fixture_repo("rust_repo");
    let db_path = repo_root.join(".symgrep").join("index.sqlite");

    run_index_sqlite(&repo_root, &db_path);

    // Baseline search without index.
    let mut base_cmd = cargo_bin_cmd!("symgrep");
    base_cmd.current_dir(&repo_root);
    base_cmd.args([
        "search",
        "add",
        "--path",
        ".",
        "--language",
        "rust",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
    ]);

    let base_assert = base_cmd.assert().success();
    let mut base_value: Value =
        serde_json::from_slice(&base_assert.get_output().stdout).expect("valid json output");

    // Search using the SQLite-based index.
    let mut index_cmd = cargo_bin_cmd!("symgrep");
    index_cmd.current_dir(&repo_root);
    index_cmd.args([
        "search",
        "add",
        "--path",
        ".",
        "--language",
        "rust",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
        "--use-index",
        "--index-backend",
        "sqlite",
        "--index-path",
        ".symgrep/index.sqlite",
    ]);

    let index_assert = index_cmd.assert().success();
    let mut index_value: Value =
        serde_json::from_slice(&index_assert.get_output().stdout).expect("valid json output");

    normalize_search_result(&mut base_value);
    normalize_search_result(&mut index_value);

    assert_eq!(
        base_value, index_value,
        "SQLite indexed Rust symbol search should match non-indexed search"
    );
}

#[test]
fn cli_search_symbol_cpp_with_index_matches_without_index() {
    let (tmp, repo_root) = copy_fixture_repo("cpp_repo");
    let index_root = index_path_for(&tmp);

    run_index(&repo_root, &index_root);

    // Baseline search without index.
    let mut base_cmd = cargo_bin_cmd!("symgrep");
    base_cmd.args([
        "search",
        "add",
        "--path",
        repo_root.to_str().unwrap(),
        "--language",
        "cpp",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
    ]);

    let base_assert = base_cmd.assert().success();
    let mut base_value: Value =
        serde_json::from_slice(&base_assert.get_output().stdout).expect("valid json output");

    // Search using the file-based index.
    let mut index_cmd = cargo_bin_cmd!("symgrep");
    index_cmd.args([
        "search",
        "add",
        "--path",
        repo_root.to_str().unwrap(),
        "--language",
        "cpp",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
        "--use-index",
        "--index-backend",
        "file",
        "--index-path",
        index_root.to_str().unwrap(),
    ]);

    let index_assert = index_cmd.assert().success();
    let mut index_value: Value =
        serde_json::from_slice(&index_assert.get_output().stdout).expect("valid json output");

    normalize_search_result(&mut base_value);
    normalize_search_result(&mut index_value);

    assert_eq!(
        base_value, index_value,
        "indexed C++ symbol search should match non-indexed search"
    );
}

#[test]
fn cli_search_symbol_cpp_with_sqlite_index_matches_without_index() {
    let (_tmp, repo_root) = copy_fixture_repo("cpp_repo");
    let db_path = repo_root.join(".symgrep").join("index.sqlite");

    run_index_sqlite(&repo_root, &db_path);

    // Baseline search without index.
    let mut base_cmd = cargo_bin_cmd!("symgrep");
    base_cmd.current_dir(&repo_root);
    base_cmd.args([
        "search",
        "add",
        "--path",
        ".",
        "--language",
        "cpp",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
    ]);

    let base_assert = base_cmd.assert().success();
    let mut base_value: Value =
        serde_json::from_slice(&base_assert.get_output().stdout).expect("valid json output");

    // Search using the SQLite-based index.
    let mut index_cmd = cargo_bin_cmd!("symgrep");
    index_cmd.current_dir(&repo_root);
    index_cmd.args([
        "search",
        "add",
        "--path",
        ".",
        "--language",
        "cpp",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
        "--use-index",
        "--index-backend",
        "sqlite",
        "--index-path",
        ".symgrep/index.sqlite",
    ]);

    let index_assert = index_cmd.assert().success();
    let mut index_value: Value =
        serde_json::from_slice(&index_assert.get_output().stdout).expect("valid json output");

    normalize_search_result(&mut base_value);
    normalize_search_result(&mut index_value);

    assert_eq!(
        base_value, index_value,
        "SQLite indexed C++ symbol search should match non-indexed search"
    );
}

#[test]
fn cli_search_symbol_ts_auto_prefers_existing_sqlite_index() {
    let (_tmp, repo_root) = copy_fixture_repo("ts_js_repo");

    // Build both file and SQLite indexes at their default locations.
    {
        let mut file_cmd = cargo_bin_cmd!("symgrep");
        file_cmd.current_dir(&repo_root);
        file_cmd.args(["index", "--path", ".", "--index-backend", "file"]);
        file_cmd.assert().success();
    }

    {
        let mut sqlite_cmd = cargo_bin_cmd!("symgrep");
        sqlite_cmd.current_dir(&repo_root);
        sqlite_cmd.args([
            "index",
            "--path",
            ".",
            "--index-backend",
            "sqlite",
            "--index-path",
            ".symgrep/index.sqlite",
        ]);
        sqlite_cmd.assert().success();
    }

    // Baseline search without index.
    let mut base_cmd = cargo_bin_cmd!("symgrep");
    base_cmd.current_dir(&repo_root);
    base_cmd.args([
        "search",
        "add",
        "--path",
        ".",
        "--language",
        "typescript",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
    ]);

    let base_assert = base_cmd.assert().success();
    let mut base_value: Value =
        serde_json::from_slice(&base_assert.get_output().stdout).expect("valid json output");

    // Search with `--use-index` but no explicit backend or path;
    // engine should select an existing SQLite index by preference.
    let mut index_cmd = cargo_bin_cmd!("symgrep");
    index_cmd.current_dir(&repo_root);
    index_cmd.args([
        "search",
        "add",
        "--path",
        ".",
        "--language",
        "typescript",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
        "--use-index",
    ]);

    let index_assert = index_cmd.assert().success();
    let mut index_value: Value =
        serde_json::from_slice(&index_assert.get_output().stdout).expect("valid json output");

    normalize_search_result(&mut base_value);
    normalize_search_result(&mut index_value);

    assert_eq!(
        base_value, index_value,
        "auto backend selection with existing SQLite index should preserve search semantics"
    );
}

#[test]
fn agent_guide_ts_symbol_search_with_explicit_sqlite_index_matches_snapshot() {
    let repo_root = PathBuf::from("tests/fixtures/ts_js_repo");
    let index_path = PathBuf::from("target/symgrep/index.sqlite");

    if let Some(parent) = index_path.parent() {
        fs::create_dir_all(parent).expect("create index directory");
    }

    // Build a SQLite index at a stable path under target/.
    let mut index_cmd = cargo_bin_cmd!("symgrep");
    index_cmd.args([
        "index",
        "--path",
        repo_root.to_str().unwrap(),
        "--index-backend",
        "sqlite",
        "--index-path",
        index_path.to_str().unwrap(),
    ]);
    index_cmd.assert().success();

    // Run a symbol search using the explicit SQLite index.
    let mut search_cmd = cargo_bin_cmd!("symgrep");
    search_cmd.args([
        "search",
        "add",
        "--path",
        repo_root.to_str().unwrap(),
        "--language",
        "typescript",
        "--mode",
        "symbol",
        "--view",
        "decl",
        "--format",
        "json",
        "--use-index",
        "--index-backend",
        "sqlite",
        "--index-path",
        index_path.to_str().unwrap(),
    ]);

    let assert = search_cmd.assert().success();
    let mut actual: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    let snapshot = fs::read_to_string("tests/snapshots/agent_ts_add_decl_with_index.json")
        .expect("snapshot file");
    let mut expected: Value = serde_json::from_str(&snapshot).expect("valid json snapshot");

    normalize_search_result(&mut actual);
    normalize_search_result(&mut expected);

    assert_eq!(actual, expected);
}

#[test]
fn cli_annotate_updates_attributes_and_reindex_preserves_them() {
    let (tmp, repo_root) = copy_fixture_repo("ts_js_repo");
    let index_root = index_path_for(&tmp);

    run_index(&repo_root, &index_root);

    // Locate the symbol to annotate using the index-backed search.
    let mut search_cmd = cargo_bin_cmd!("symgrep");
    search_cmd.args([
        "search",
        "name:addWithDoc kind:function",
        "--path",
        repo_root.to_str().unwrap(),
        "--language",
        "typescript",
        "--mode",
        "symbol",
        "--view",
        "meta",
        "--format",
        "json",
        "--use-index",
        "--index-backend",
        "file",
        "--index-path",
        index_root.to_str().unwrap(),
    ]);

    let search_assert = search_cmd.assert().success();
    let search_value: Value =
        serde_json::from_slice(&search_assert.get_output().stdout).expect("valid json output");

    let symbols = search_value["symbols"].as_array().expect("symbols array");
    let target = symbols
        .iter()
        .find(|s| s["name"] == "addWithDoc")
        .expect("addWithDoc symbol");

    let file = target["file"].as_str().expect("file string");
    let start_line = target["range"]["start_line"]
        .as_u64()
        .expect("start_line");
    let end_line = target["range"]["end_line"].as_u64().expect("end_line");

    // Annotate the symbol with keywords and a description.
    let mut annotate_cmd = cargo_bin_cmd!("symgrep");
    annotate_cmd.args([
        "annotate",
        "--file",
        file,
        "--language",
        "typescript",
        "--kind",
        "function",
        "--name",
        "addWithDoc",
        "--start-line",
        &start_line.to_string(),
        "--end-line",
        &end_line.to_string(),
        "--keywords",
        "auth,login,jwt",
        "--description",
        "Performs user authentication and issues JWTs",
        "--index-backend",
        "file",
        "--index-path",
        index_root.to_str().unwrap(),
    ]);

    let annotate_assert = annotate_cmd.assert().success();
    let annotate_value: Value =
        serde_json::from_slice(&annotate_assert.get_output().stdout).expect("valid json output");

    let attrs = &annotate_value["symbol"]["attributes"];
    assert_eq!(
        attrs["keywords"],
        serde_json::json!(["auth", "login", "jwt"])
    );
    assert_eq!(
        attrs["description"],
        serde_json::json!("Performs user authentication and issues JWTs")
    );

    // Re-run indexing and ensure attributes are preserved.
    run_index(&repo_root, &index_root);

    let mut search_after_cmd = cargo_bin_cmd!("symgrep");
    search_after_cmd.args([
        "search",
        "name:addWithDoc kind:function",
        "--path",
        repo_root.to_str().unwrap(),
        "--language",
        "typescript",
        "--mode",
        "symbol",
        "--view",
        "meta",
        "--format",
        "json",
        "--use-index",
        "--index-backend",
        "file",
        "--index-path",
        index_root.to_str().unwrap(),
    ]);

    let search_after_assert = search_after_cmd.assert().success();
    let search_after_value: Value = serde_json::from_slice(
        &search_after_assert.get_output().stdout,
    )
    .expect("valid json output");

    let symbols_after = search_after_value["symbols"]
        .as_array()
        .expect("symbols array");
    let target_after = symbols_after
        .iter()
        .find(|s| s["name"] == "addWithDoc")
        .expect("addWithDoc symbol");

    let attrs_after = &target_after["attributes"];
    assert_eq!(attrs_after["keywords"], attrs["keywords"]);
    assert_eq!(attrs_after["description"], attrs["description"]);
}

#[test]
fn cli_index_info_file_backend_text_output() {
    let (tmp, repo_root) = copy_fixture_repo("ts_js_repo");
    let index_root = index_path_for(&tmp);

    run_index(&repo_root, &index_root);

    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "index-info",
        "--path",
        repo_root.to_str().unwrap(),
        "--index-backend",
        "file",
        "--index-path",
        index_root.to_str().unwrap(),
        "--format",
        "text",
    ]);

    let assert = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);

    assert!(
        stdout.contains("backend      : file"),
        "expected backend line in output, got:\n{stdout}"
    );
    assert!(
        stdout.contains("files        :"),
        "expected files count line in output, got:\n{stdout}"
    );
    assert!(
        stdout.contains("symbols      :"),
        "expected symbols count line in output, got:\n{stdout}"
    );
}

#[test]
fn cli_index_info_sqlite_backend_json_output() {
    let (_tmp, repo_root) = copy_fixture_repo("ts_js_repo");
    let db_path = repo_root.join(".symgrep").join("index.sqlite");

    run_index_sqlite(&repo_root, &db_path);

    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.current_dir(&repo_root);
    cmd.args([
        "index-info",
        "--path",
        ".",
        "--index-backend",
        "sqlite",
        "--index-path",
        ".symgrep/index.sqlite",
        "--format",
        "json",
    ]);

    let assert = cmd.assert().success();
    let value: Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("valid json output");

    assert_eq!(value["backend"], "sqlite");
    assert_eq!(value["index_path"], ".symgrep/index.sqlite");
    assert!(value["files_indexed"].as_u64().unwrap_or(0) >= 1);
    assert!(value["symbols_indexed"].as_u64().unwrap_or(0) >= 1);
    assert!(
        value.get("root_path").and_then(|v| v.as_str()).is_some(),
        "expected root_path field in index-info JSON"
    );
    assert!(
        value.get("created_at").and_then(|v| v.as_str()).is_some(),
        "expected created_at field in index-info JSON"
    );
    assert!(
        value.get("updated_at").and_then(|v| v.as_str()).is_some(),
        "expected updated_at field in index-info JSON"
    );
}
