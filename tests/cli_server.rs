use assert_cmd::cargo::{cargo_bin_cmd, CommandCargoExt};
use reqwest::blocking::Client;
use serde_json::Value;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;
use tempfile::tempdir;

struct TestDaemon {
    base_url: String,
    child: Child,
}

impl TestDaemon {
    fn spawn() -> Self {
        // Bind an ephemeral port first so we know which port to pass
        // to the CLI `symgrep serve` subcommand.
        let listener =
            TcpListener::bind("127.0.0.1:0").expect("bind ephemeral TCP listener for daemon");
        let addr = listener
            .local_addr()
            .expect("local_addr for daemon listener");
        let port = addr.port();
        drop(listener);

        let addr_arg = format!("127.0.0.1:{port}");
        let base_url = format!("http://{addr_arg}");

        // Capture daemon stdout/stderr to temp files for easier
        // debugging when tests fail.
        let log_dir = std::env::temp_dir();
        let stdout_path = log_dir.join(format!("symgrep_daemon_{port}_stdout.log"));
        let stderr_path = log_dir.join(format!("symgrep_daemon_{port}_stderr.log"));

        let stdout_file =
            std::fs::File::create(&stdout_path).expect("create daemon stdout log file");
        let stderr_file =
            std::fs::File::create(&stderr_path).expect("create daemon stderr log file");

        let mut cmd = Command::cargo_bin("symgrep").expect("locate symgrep binary");
        cmd.args(["serve", "--addr", &addr_arg])
            .stdout(stdout_file)
            .stderr(stderr_file);
        let child = cmd.spawn().expect("spawn symgrep serve daemon");

        wait_for_health(&base_url);

        Self { base_url, child }
    }
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn wait_for_health(base_url: &str) {
    let client = Client::new();
    let url = format!("{}/v1/health", base_url);

    let mut last_err = None;
    for _ in 0..150 {
        match client.get(&url).send() {
            Ok(resp) if resp.status().is_success() => return,
            Err(e) => {
                last_err = Some(format!("HTTP error: {}", e));
                thread::sleep(Duration::from_millis(100));
            }
            Ok(resp) => {
                last_err = Some(format!("unexpected status: {}", resp.status()));
                thread::sleep(Duration::from_millis(100));
            }
        }
    }

    panic!(
        "symgrep HTTP daemon did not become healthy in time. Last error: {}",
        last_err.unwrap_or_else(|| "unknown".to_string())
    );
}

fn normalize_search_result(value: &mut Value) {
    if let Some(array) = value.get_mut("matches").and_then(|v| v.as_array_mut()) {
        array.sort_by(|a, b| {
            let path_a = a.get("path").and_then(|v| v.as_str()).unwrap_or_default();
            let path_b = b.get("path").and_then(|v| v.as_str()).unwrap_or_default();

            let line_a = a.get("line").and_then(|v| v.as_u64()).unwrap_or_default();
            let line_b = b.get("line").and_then(|v| v.as_u64()).unwrap_or_default();

            path_a.cmp(path_b).then_with(|| line_a.cmp(&line_b))
        });
    }

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

#[test]
fn cli_search_via_server_matches_local_search() {
    let daemon = TestDaemon::spawn();

    let fixture_dir = "tests/fixtures/text_repo";

    // Baseline local search.
    let mut local_cmd = cargo_bin_cmd!("symgrep");
    local_cmd.args(["search", "foo", "--path", fixture_dir, "--format", "json"]);

    let local_assert = local_cmd.assert().success();
    let mut local_value: Value =
        serde_json::from_slice(&local_assert.get_output().stdout).expect("valid local json");

    // Search via HTTP daemon.
    let mut server_cmd = cargo_bin_cmd!("symgrep");
    server_cmd.args([
        "search",
        "foo",
        "--path",
        fixture_dir,
        "--format",
        "json",
        "--server",
        &daemon.base_url,
    ]);

    let server_assert = server_cmd.assert().success();
    let mut server_value: Value =
        serde_json::from_slice(&server_assert.get_output().stdout).expect("valid server json");

    normalize_search_result(&mut local_value);
    normalize_search_result(&mut server_value);

    assert_eq!(
        local_value, server_value,
        "daemon-backed search should match local CLI search"
    );
}

#[test]
fn cli_serve_health_endpoint_reports_ok_status() {
    let daemon = TestDaemon::spawn();
    let client = Client::new();
    let url = format!("{}/v1/health", daemon.base_url);

    let resp = client.get(&url).send().expect("health response");
    assert!(
        resp.status().is_success(),
        "health endpoint should return success status"
    );

    let value: Value = resp.json().expect("valid health JSON body");
    assert_eq!(value["status"], "ok");
}

#[test]
fn cli_symbol_search_via_server_matches_local_search_for_key_languages() {
    let daemon = TestDaemon::spawn();

    let cases = [
        ("tests/fixtures/ts_js_repo", "typescript"),
        ("tests/fixtures/ts_js_repo", "javascript"),
        ("tests/fixtures/cpp_repo", "cpp"),
    ];

    for (fixture_dir, language) in cases {
        // Baseline local symbol search.
        let mut local_cmd = cargo_bin_cmd!("symgrep");
        local_cmd.args([
            "search",
            "name:add kind:function",
            "--path",
            fixture_dir,
            "--language",
            language,
            "--mode",
            "symbol",
            "--context",
            "def",
            "--format",
            "json",
        ]);

        let local_assert = local_cmd.assert().success();
        let mut local_value: Value =
            serde_json::from_slice(&local_assert.get_output().stdout).expect("valid local json");

        // Symbol search via HTTP daemon.
        let mut server_cmd = cargo_bin_cmd!("symgrep");
        server_cmd.args([
            "search",
            "name:add kind:function",
            "--path",
            fixture_dir,
            "--language",
            language,
            "--mode",
            "symbol",
            "--context",
            "def",
            "--format",
            "json",
            "--server",
            &daemon.base_url,
        ]);

        let server_assert = server_cmd.assert().success();
        let mut server_value: Value =
            serde_json::from_slice(&server_assert.get_output().stdout).expect("valid server json");

        normalize_search_result(&mut local_value);
        normalize_search_result(&mut server_value);

        assert_eq!(
            local_value, server_value,
            "daemon-backed symbol search should match local CLI search for language {language}"
        );
    }
}

fn assert_index_parity_via_daemon(backend: &str, index_path: &Path) {
    let daemon = TestDaemon::spawn();

    let repo_root = PathBuf::from("tests/fixtures/ts_js_repo");

    // Baseline non-indexed search.
    let mut base_cmd = cargo_bin_cmd!("symgrep");
    base_cmd.args([
        "search",
        "name:add kind:function",
        "--path",
        repo_root.to_str().unwrap(),
        "--language",
        "typescript",
        "--mode",
        "symbol",
        "--context",
        "def",
        "--format",
        "json",
    ]);

    let base_assert = base_cmd.assert().success();
    let mut base_value: Value =
        serde_json::from_slice(&base_assert.get_output().stdout).expect("valid base json");

    // Build an index via the HTTP daemon.
    let mut index_cmd = cargo_bin_cmd!("symgrep");
    index_cmd.args([
        "index",
        "--path",
        repo_root.to_str().unwrap(),
        "--index-backend",
        backend,
        "--index-path",
        index_path.to_str().unwrap(),
        "--server",
        &daemon.base_url,
    ]);
    index_cmd.assert().success();

    // Perform an indexed search via the HTTP daemon.
    let mut server_cmd = cargo_bin_cmd!("symgrep");
    server_cmd.args([
        "search",
        "name:add kind:function",
        "--path",
        repo_root.to_str().unwrap(),
        "--language",
        "typescript",
        "--mode",
        "symbol",
        "--context",
        "def",
        "--format",
        "json",
        "--use-index",
        "--index-backend",
        backend,
        "--index-path",
        index_path.to_str().unwrap(),
        "--server",
        &daemon.base_url,
    ]);

    let server_assert = server_cmd.assert().success();
    let mut server_value: Value =
        serde_json::from_slice(&server_assert.get_output().stdout).expect("valid server json");

    normalize_search_result(&mut base_value);
    normalize_search_result(&mut server_value);

    assert_eq!(
        base_value, server_value,
        "indexed search via daemon should match non-indexed local search for backend {backend}"
    );
}

#[test]
fn cli_index_and_search_via_server_file_backend_matches_local_search() {
    let tmp = tempdir().expect("tempdir");
    let index_root = tmp.path().join("file_index");
    assert_index_parity_via_daemon("file", &index_root);
}

#[test]
fn cli_index_and_search_via_server_sqlite_backend_matches_local_search() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("index.sqlite");
    assert_index_parity_via_daemon("sqlite", &db_path);
}

#[test]
fn cli_search_via_server_surfaces_http_errors() {
    let daemon = TestDaemon::spawn();

    let mut cmd = cargo_bin_cmd!("symgrep");
    cmd.args([
        "search",
        "foo",
        "--path",
        "definitely/does/not/exist",
        "--format",
        "json",
        "--server",
        &daemon.base_url,
    ]);

    let assert = cmd.assert().failure();
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).expect("utf-8 stderr");

    assert!(
        stderr.contains("server returned error for"),
        "expected CLI error output to mention server-side HTTP error, got: {stderr}"
    );
}
