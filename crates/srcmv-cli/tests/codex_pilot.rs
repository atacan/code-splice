//! Phase 10 real-repository Codex workflow pilot.

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const PILOT_ROOT_NAME: &str = "phase10-pilot";

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("CLI crate should be beneath the repository root")
        .to_path_buf()
}

fn pilot_root() -> PathBuf {
    let root = PathBuf::from(
        std::env::var_os("SRCMV_PILOT_ROOT")
            .expect("SRCMV_PILOT_ROOT is required for the ignored pilot"),
    );
    assert_eq!(
        root.file_name().and_then(|name| name.to_str()),
        Some(PILOT_ROOT_NAME),
        "the pilot root must use the safety marker name"
    );
    fs::create_dir_all(&root).expect("pilot root should be created");
    root
}

fn reset_workspace(root: &Path, scenario: &str) -> PathBuf {
    let workspace = root.join(scenario);
    if workspace.exists() {
        fs::remove_dir_all(&workspace).expect("old scenario workspace should be removed");
    }
    fs::create_dir_all(workspace.join("src")).expect("scenario source directory should exist");
    fs::copy(
        repository_root().join("Cargo.toml"),
        workspace.join("Cargo.toml"),
    )
    .expect("real repository manifest should be copied");
    workspace
}

fn write(workspace: &Path, relative: &str, bytes: &[u8]) {
    let path = workspace.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("file parent should be created");
    }
    fs::write(path, bytes).expect("pilot file should be written");
}

fn real_file(relative: &str) -> Vec<u8> {
    fs::read(repository_root().join(relative)).expect("real repository file should read")
}

fn invoke(
    workspace: &Path,
    arguments: &[&str],
    request: Option<&Value>,
    failpoint: Option<&str>,
) -> Output {
    assert!(
        !arguments.contains(&"--accept-current-plan"),
        "the Phase 10 pilot forbids --accept-current-plan"
    );
    let mut command = Command::new(env!("CARGO_BIN_EXE_srcmv"));
    command.arg("--workspace").arg(workspace).args(arguments);
    if let Some(name) = failpoint {
        command
            .env("SRCMV_TEST_FAILPOINT", name)
            .env("SRCMV_TEST_FAILPOINT_ACTION", "exit");
    }
    if request.is_some() {
        command.stdin(Stdio::piped());
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().expect("srcmv should start");
    if let Some(request) = request {
        serde_json::to_writer(
            child.stdin.as_mut().expect("request stdin should be piped"),
            request,
        )
        .expect("request should serialize");
        child.stdin.take();
    }
    child.wait_with_output().expect("srcmv should exit")
}

fn report(output: &Output, expected_exit: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(expected_exit),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "JSON invocation wrote stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = std::str::from_utf8(&output.stdout).expect("JSON stdout should be UTF-8");
    assert!(stdout.ends_with('\n'), "JSON stdout should end in LF");
    serde_json::from_str(stdout).expect("stdout should contain one JSON value")
}

fn inspect(workspace: &Path, paths: &[&str]) -> Value {
    let mut arguments = vec!["inspect"];
    for path in paths {
        arguments.extend(["--path", path]);
    }
    arguments.push("--json");
    report(&invoke(workspace, &arguments, None, None), 0)
}

fn inspect_error(workspace: &Path, paths: &[&str], expected_code: &str, exit: i32) -> Value {
    let mut arguments = vec!["inspect"];
    for path in paths {
        arguments.extend(["--path", path]);
    }
    arguments.push("--json");
    let value = report(&invoke(workspace, &arguments, None, None), exit);
    assert_eq!(value["code"], expected_code);
    value
}

fn preview(workspace: &Path, request: &Value) -> (String, Value) {
    let value = report(
        &invoke(
            workspace,
            &["apply", "--request", "-", "--preview", "--json"],
            Some(request),
            None,
        ),
        0,
    );
    let plan = value["plan_sha256"]
        .as_str()
        .expect("preview should contain a plan digest")
        .to_owned();
    (plan, value)
}

fn preview_error(workspace: &Path, request: &Value, expected_code: &str, exit: i32) -> Value {
    let value = report(
        &invoke(
            workspace,
            &["apply", "--request", "-", "--preview", "--json"],
            Some(request),
            None,
        ),
        exit,
    );
    assert_eq!(value["code"], expected_code);
    value
}

fn commit(workspace: &Path, request: &Value, plan: &str) -> Value {
    report(
        &invoke(
            workspace,
            &[
                "apply",
                "--request",
                "-",
                "--commit",
                "--expect-plan",
                plan,
                "--json",
            ],
            Some(request),
            None,
        ),
        0,
    )
}

fn commit_error(
    workspace: &Path,
    request: &Value,
    plan: &str,
    expected_code: &str,
    exit: i32,
) -> Value {
    let value = report(
        &invoke(
            workspace,
            &[
                "apply",
                "--request",
                "-",
                "--commit",
                "--expect-plan",
                plan,
                "--json",
            ],
            Some(request),
            None,
        ),
        exit,
    );
    assert_eq!(value["code"], expected_code);
    value
}

fn crash_commit(workspace: &Path, request: &Value, plan: &str, failpoint: &str) {
    let output = invoke(
        workspace,
        &[
            "apply",
            "--request",
            "-",
            "--commit",
            "--expect-plan",
            plan,
            "--json",
        ],
        Some(request),
        Some(failpoint),
    );
    assert_eq!(
        output.status.code(),
        Some(86),
        "failpoint={failpoint} stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn recover(workspace: &Path, transaction_id: &str, action: &str, exit: i32) -> Value {
    report(
        &invoke(
            workspace,
            &["recover", transaction_id, action, "--json"],
            None,
            None,
        ),
        exit,
    )
}

fn transaction_id(workspace: &Path) -> String {
    let entries = fs::read_dir(workspace.join(".srcmv/transactions"))
        .expect("active transaction directory should exist")
        .collect::<Result<Vec<_>, _>>()
        .expect("transaction entries should read");
    assert_eq!(entries.len(), 1, "one active transaction should exist");
    entries[0]
        .file_name()
        .into_string()
        .expect("transaction ID should be UTF-8")
}

fn source(path: &str, selector: Value, bytes: &[u8]) -> Value {
    json!({
        "path": path,
        "selector": selector,
        "precondition": {"kind":"sha256", "value":digest(bytes)}
    })
}

fn existing_destination(path: &str, anchor: Value, bytes: &[u8]) -> Value {
    json!({
        "path": path,
        "anchor": anchor,
        "precondition": {"kind":"sha256", "value":digest(bytes)}
    })
}

fn new_destination(path: &str, anchor: Value) -> Value {
    json!({
        "path": path,
        "anchor": anchor,
        "precondition": {"kind":"must_not_exist"}
    })
}

fn operation(kind: &str, source: Value, destination: Value) -> Value {
    json!({"kind":kind, "source":source, "destination":destination})
}

fn batch(operations: Vec<Value>) -> Value {
    json!({"protocol_version":1, "operations":operations})
}

fn line_selector(start: u64, end: u64) -> Value {
    json!({"kind":"lines", "start":start, "end":end})
}

fn byte_selector(start: u64, end: u64) -> Value {
    json!({"kind":"bytes", "start":start, "end":end})
}

fn anchor(kind: &str) -> Value {
    json!({"kind":kind})
}

fn seeded_main(workspace: &Path) -> Vec<u8> {
    let bytes = real_file("crates/srcmv-cli/src/main.rs");
    write(workspace, "src/source.rs", &bytes);
    bytes
}

fn record(results: &mut Vec<Value>, id: u64, name: &str, demonstrated: &str) {
    results.push(json!({
        "id": id,
        "name": name,
        "status": "pass",
        "demonstrated": demonstrated
    }));
}

#[test]
#[ignore = "requires a qualified filesystem and a mounted second device; run scripts/run-codex-pilot.sh"]
fn codex_pilot_should_pass_all_fifteen_scenarios() {
    let root = pilot_root();
    let mut results = Vec::new();

    let workspace = reset_workspace(&root, "scenario-01");
    let main = seeded_main(&workspace);
    let destination = real_file("crates/srcmv-cli/src/preview.rs");
    write(&workspace, "src/destination.rs", &destination);
    inspect(&workspace, &["src/source.rs", "src/destination.rs"]);
    let selected = main
        .split_inclusive(|byte| *byte == b'\n')
        .skip(3)
        .flatten()
        .copied()
        .collect::<Vec<_>>();
    let request = batch(vec![operation(
        "move",
        source("src/source.rs", line_selector(4, 6), &main),
        existing_destination("src/destination.rs", anchor("file_end"), &destination),
    )]);
    let (plan, _) = preview(&workspace, &request);
    let committed = commit(&workspace, &request, &plan);
    assert_eq!(committed["transaction_state"], "committed");
    assert!(
        fs::read(workspace.join("src/destination.rs"))
            .unwrap()
            .ends_with(&selected)
    );
    record(
        &mut results,
        1,
        "move a function to an existing file",
        "real main function moved with exact selected and inserted bytes",
    );

    let workspace = reset_workspace(&root, "scenario-02");
    let main = seeded_main(&workspace);
    inspect(&workspace, &["src/source.rs", "src/new.rs"]);
    let request = batch(vec![operation(
        "move",
        source("src/source.rs", line_selector(4, 6), &main),
        new_destination("src/new.rs", anchor("file_start")),
    )]);
    let (plan, _) = preview(&workspace, &request);
    commit(&workspace, &request, &plan);
    assert!(
        fs::read(workspace.join("src/new.rs"))
            .unwrap()
            .starts_with(b"fn main")
    );
    record(
        &mut results,
        2,
        "move a function to a new file",
        "new target created from the previewed real function slice",
    );

    let workspace = reset_workspace(&root, "scenario-03");
    let source_bytes = real_file("crates/srcmv-core/src/plan_hash.rs");
    let destination = b"// copied declarations\n".to_vec();
    write(&workspace, "src/source.rs", &source_bytes);
    write(&workspace, "src/destination.rs", &destination);
    inspect(&workspace, &["src/source.rs", "src/destination.rs"]);
    let request = batch(vec![operation(
        "copy",
        source("src/source.rs", line_selector(12, 12), &source_bytes),
        existing_destination("src/destination.rs", anchor("file_end"), &destination),
    )]);
    let (plan, _) = preview(&workspace, &request);
    commit(&workspace, &request, &plan);
    assert!(
        String::from_utf8(fs::read(workspace.join("src/destination.rs")).unwrap())
            .unwrap()
            .contains("PLAN_HASH_VERSION")
    );
    record(
        &mut results,
        3,
        "copy a declaration",
        "the plan-hash version declaration was copied without reproducing it in the request",
    );

    let workspace = reset_workspace(&root, "scenario-04");
    let main = seeded_main(&workspace);
    inspect(&workspace, &["src/source.rs"]);
    let request = batch(vec![operation(
        "move",
        source("src/source.rs", line_selector(4, 6), &main),
        existing_destination("src/source.rs", anchor("file_start"), &main),
    )]);
    let (plan, _) = preview(&workspace, &request);
    commit(&workspace, &request, &plan);
    assert!(
        fs::read(workspace.join("src/source.rs"))
            .unwrap()
            .starts_with(b"fn main")
    );
    record(
        &mut results,
        4,
        "reorder code in one file",
        "same-file backward move used immutable initial coordinates",
    );

    let workspace = reset_workspace(&root, "scenario-05");
    let main = seeded_main(&workspace);
    inspect(&workspace, &["src/source.rs"]);
    let request = batch(vec![operation(
        "move",
        source("src/source.rs", line_selector(4, 6), &main),
        existing_destination(
            "src/source.rs",
            json!({"kind":"before_line", "line":4}),
            &main,
        ),
    )]);
    let (plan, preview_report) = preview(&workspace, &request);
    assert_eq!(preview_report["resolved_operations"][0]["effect"], "no_op");
    let committed = commit(&workspace, &request, &plan);
    assert!(committed["transaction_id"].is_null());
    assert_eq!(fs::read(workspace.join("src/source.rs")).unwrap(), main);
    assert!(!workspace.join(".srcmv").exists());
    record(
        &mut results,
        5,
        "execute a same-file no-op",
        "no transaction or control tree was created",
    );

    let workspace = reset_workspace(&root, "scenario-06");
    let main = seeded_main(&workspace);
    inspect(&workspace, &["src/source.rs", "src/split.rs"]);
    let request = batch(vec![operation(
        "move",
        source("src/source.rs", line_selector(1, 3), &main),
        new_destination("src/split.rs", anchor("file_start")),
    )]);
    let (plan, preview_report) = preview(&workspace, &request);
    assert_eq!(preview_report["outputs"].as_array().unwrap().len(), 2);
    commit(&workspace, &request, &plan);
    record(
        &mut results,
        6,
        "split one file into two outputs",
        "source and one new output committed in one transaction",
    );

    let workspace = reset_workspace(&root, "scenario-07");
    let source_bytes = real_file("crates/srcmv-core/src/plan_hash.rs");
    write(&workspace, "src/source.rs", &source_bytes);
    inspect(&workspace, &["src/source.rs", "src/one.rs", "src/two.rs"]);
    let request = batch(vec![
        operation(
            "move",
            source("src/source.rs", line_selector(12, 12), &source_bytes),
            new_destination("src/one.rs", anchor("file_start")),
        ),
        operation(
            "move",
            source("src/source.rs", line_selector(13, 13), &source_bytes),
            new_destination("src/two.rs", anchor("file_start")),
        ),
    ]);
    let (plan, preview_report) = preview(&workspace, &request);
    assert_eq!(preview_report["outputs"].as_array().unwrap().len(), 3);
    commit(&workspace, &request, &plan);
    record(
        &mut results,
        7,
        "split one file into three outputs",
        "three changed targets committed in deterministic order",
    );

    let workspace = reset_workspace(&root, "scenario-08");
    let mixed = b"alpha\r\nbeta\rgamma\n";
    let destination = b"prefix\r\n";
    write(&workspace, "src/source.rs", mixed);
    write(&workspace, "src/destination.rs", destination);
    inspect(&workspace, &["src/source.rs", "src/destination.rs"]);
    let request = batch(vec![operation(
        "move",
        source("src/source.rs", line_selector(1, 2), mixed),
        existing_destination("src/destination.rs", anchor("file_end"), destination),
    )]);
    let (plan, _) = preview(&workspace, &request);
    commit(&workspace, &request, &plan);
    assert_eq!(
        fs::read(workspace.join("src/source.rs")).unwrap(),
        b"gamma\n"
    );
    assert_eq!(
        fs::read(workspace.join("src/destination.rs")).unwrap(),
        b"prefix\r\nalpha\r\nbeta\r"
    );
    record(
        &mut results,
        8,
        "preserve CRLF and mixed terminators",
        "LF, CRLF, and lone-CR bytes remained exact",
    );

    let workspace = reset_workspace(&root, "scenario-09");
    let binary = b"A\xff\0\xfeB";
    let destination = b"binary:";
    write(&workspace, "src/source.bin", binary);
    write(&workspace, "src/destination.bin", destination);
    inspect(&workspace, &["src/source.bin", "src/destination.bin"]);
    let request = batch(vec![operation(
        "move",
        source("src/source.bin", byte_selector(1, 4), binary),
        existing_destination("src/destination.bin", anchor("file_end"), destination),
    )]);
    let (plan, preview_report) = preview(&workspace, &request);
    assert_eq!(preview_report["diff"]["kind"], "binary");
    commit(&workspace, &request, &plan);
    assert_eq!(
        fs::read(workspace.join("src/destination.bin")).unwrap(),
        b"binary:\xff\0\xfe"
    );
    record(
        &mut results,
        9,
        "move non-UTF-8 payload bytes",
        "binary payload and NUL byte preserved exactly",
    );

    let workspace = reset_workspace(&root, "scenario-10");
    let source_bytes = b"old source\n";
    let destination = b"target\n";
    write(&workspace, "src/source.rs", source_bytes);
    write(&workspace, "src/destination.rs", destination);
    inspect(&workspace, &["src/source.rs", "src/destination.rs"]);
    let request = batch(vec![operation(
        "copy",
        source("src/source.rs", line_selector(1, 1), source_bytes),
        existing_destination("src/destination.rs", anchor("file_end"), destination),
    )]);
    let (plan, _) = preview(&workspace, &request);
    write(&workspace, "src/source.rs", b"stale source\n");
    commit_error(&workspace, &request, &plan, "PRECONDITION_FAILED", 3);
    assert!(!workspace.join(".srcmv").exists());
    record(
        &mut results,
        10,
        "reject a stale source digest",
        "stale source was rejected before transaction creation",
    );

    let workspace = reset_workspace(&root, "scenario-11");
    let source_bytes = b"payload\n";
    let destination = b"target\n";
    write(&workspace, "src/source.rs", source_bytes);
    write(&workspace, "src/destination.rs", destination);
    inspect(&workspace, &["src/source.rs", "src/destination.rs"]);
    let request = batch(vec![operation(
        "copy",
        source("src/source.rs", line_selector(1, 1), source_bytes),
        existing_destination("src/destination.rs", anchor("file_end"), destination),
    )]);
    preview(&workspace, &request);
    let wrong = format!("sha256:{}", "0".repeat(64));
    commit_error(&workspace, &request, &wrong, "EXPECTED_PLAN_MISMATCH", 3);
    assert!(!workspace.join(".srcmv").exists());
    record(
        &mut results,
        11,
        "reject an expected-plan mismatch",
        "mismatched digest was rejected before transaction creation",
    );

    for (scenario, action, id, label) in [
        (
            "scenario-12",
            "--complete",
            12,
            "recover an interrupted multi-file commit by completion",
        ),
        (
            "scenario-13",
            "--rollback",
            13,
            "recover an interrupted multi-file commit by rollback",
        ),
    ] {
        let workspace = reset_workspace(&root, scenario);
        let source_bytes = b"abc";
        let target = b"XYZ";
        write(&workspace, "src/source.rs", source_bytes);
        write(&workspace, "src/target.rs", target);
        inspect(
            &workspace,
            &["src/source.rs", "src/target.rs", "src/new.rs"],
        );
        let request = batch(vec![
            operation(
                "move",
                source("src/source.rs", byte_selector(0, 1), source_bytes),
                existing_destination("src/target.rs", anchor("file_end"), target),
            ),
            operation(
                "copy",
                source("src/source.rs", byte_selector(1, 3), source_bytes),
                new_destination("src/new.rs", anchor("file_start")),
            ),
        ]);
        let (plan, _) = preview(&workspace, &request);
        crash_commit(
            &workspace,
            &request,
            &plan,
            "after_install_rename_target-00000000",
        );
        let transaction_id = transaction_id(&workspace);
        let status = recover(&workspace, &transaction_id, "--status", 0);
        assert_eq!(
            status["transaction"]["visibility"],
            "mixed_old_new_possible"
        );
        let recovered = recover(&workspace, &transaction_id, action, 0);
        if action == "--complete" {
            assert_eq!(recovered["transaction"]["visibility"], "all_planned");
            assert_eq!(fs::read(workspace.join("src/source.rs")).unwrap(), b"bc");
            assert_eq!(fs::read(workspace.join("src/target.rs")).unwrap(), b"XYZa");
            assert_eq!(fs::read(workspace.join("src/new.rs")).unwrap(), b"bc");
            record(
                &mut results,
                id,
                label,
                "fresh-process completion reached every planned digest",
            );
        } else {
            assert_eq!(recovered["transaction"]["visibility"], "all_original");
            assert_eq!(
                fs::read(workspace.join("src/source.rs")).unwrap(),
                source_bytes
            );
            assert_eq!(fs::read(workspace.join("src/target.rs")).unwrap(), target);
            assert!(!workspace.join("src/new.rs").exists());
            record(
                &mut results,
                id,
                label,
                "fresh-process rollback restored all original bytes and absence",
            );
        }
    }

    let workspace = reset_workspace(&root, "scenario-14");
    let source_bytes = b"payload";
    let target = b"target";
    write(&workspace, "src/source.rs", source_bytes);
    write(&workspace, "src/target.rs", target);
    fs::set_permissions(
        workspace.join("src/target.rs"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    inspect(&workspace, &["src/source.rs", "src/target.rs"]);
    let request = batch(vec![operation(
        "copy",
        source("src/source.rs", byte_selector(0, 7), source_bytes),
        existing_destination("src/target.rs", anchor("file_end"), target),
    )]);
    let (plan, _) = preview(&workspace, &request);
    fs::set_permissions(
        workspace.join("src/target.rs"),
        fs::Permissions::from_mode(0o640),
    )
    .unwrap();
    commit(&workspace, &request, &plan);
    assert_eq!(
        fs::metadata(workspace.join("src/target.rs"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
    record(
        &mut results,
        14,
        "preserve a permission change after preview",
        "locked commit snapshot preserved mode 0640",
    );

    let workspace = root.join("scenario-15");
    fs::create_dir_all(workspace.join("src")).expect("scenario 15 source should exist");
    if workspace.join(".srcmv").exists() {
        fs::remove_dir_all(workspace.join(".srcmv"))
            .expect("prior scenario 15 control tree should be removed");
    }
    if !workspace.join("Cargo.toml").exists() {
        fs::copy(
            repository_root().join("Cargo.toml"),
            workspace.join("Cargo.toml"),
        )
        .unwrap();
    }
    for entry in [
        "src/link",
        "src/hard-target",
        "src/hard-alias",
        "src/collision",
    ] {
        let path = workspace.join(entry);
        if path.is_symlink() || path.is_file() {
            fs::remove_file(path).unwrap();
        } else if path.is_dir() {
            fs::remove_dir_all(path).unwrap();
        }
    }
    write(&workspace, "src/source.rs", b"payload");
    fs::create_dir_all(workspace.join("real")).unwrap();
    write(&workspace, "real/file.rs", b"real");
    std::os::unix::fs::symlink("../real", workspace.join("src/link")).unwrap();
    inspect_error(&workspace, &["src/link/file.rs"], "SYMLINK_NOT_ALLOWED", 4);
    inspect_error(&workspace, &[".srcmv/forbidden"], "INVALID_REQUEST", 2);

    write(&workspace, "src/hard-target", b"target");
    fs::hard_link(
        workspace.join("src/hard-target"),
        workspace.join("src/hard-alias"),
    )
    .unwrap();
    inspect(&workspace, &["src/source.rs", "src/hard-target"]);
    let hard_request = batch(vec![operation(
        "copy",
        source("src/source.rs", byte_selector(0, 7), b"payload"),
        existing_destination("src/hard-target", anchor("file_end"), b"target"),
    )]);
    preview_error(&workspace, &hard_request, "HARD_LINK_NOT_SUPPORTED", 4);

    let cross_device = PathBuf::from(
        std::env::var_os("SRCMV_PILOT_CROSS_DEVICE")
            .expect("SRCMV_PILOT_CROSS_DEVICE must name the mounted second device"),
    );
    assert_eq!(cross_device, workspace.join("external"));
    assert!(cross_device.is_dir(), "cross-device mount should exist");
    inspect(&workspace, &["src/source.rs", "external/new.rs"]);
    let cross_request = batch(vec![operation(
        "copy",
        source("src/source.rs", byte_selector(0, 7), b"payload"),
        new_destination("external/new.rs", anchor("file_start")),
    )]);
    let (cross_plan, _) = preview(&workspace, &cross_request);
    commit_error(
        &workspace,
        &cross_request,
        &cross_plan,
        "CROSS_DEVICE_TRANSACTION",
        4,
    );

    inspect(&workspace, &["src/source.rs", "src/collision"]);
    let collision_request = batch(vec![operation(
        "copy",
        source("src/source.rs", byte_selector(0, 7), b"payload"),
        new_destination("src/collision", anchor("file_start")),
    )]);
    let (collision_plan, _) = preview(&workspace, &collision_request);
    crash_commit(
        &workspace,
        &collision_request,
        &collision_plan,
        "before_install_rename_target-00000000",
    );
    let collision_transaction = transaction_id(&workspace);
    write(&workspace, "src/collision", b"external");
    let conflict = recover(&workspace, &collision_transaction, "--complete", 3);
    assert_eq!(conflict["code"], "RECOVERY_CONFLICT");
    assert_eq!(
        fs::read(workspace.join("src/collision")).unwrap(),
        b"external"
    );
    record(
        &mut results,
        15,
        "reject unsafe path and overwrite cases",
        "symlink traversal, hard-link target, reserved tree, cross-device target, and external collision all failed closed",
    );

    assert_eq!(results.len(), 15);
    assert!(results.iter().all(|result| result["status"] == "pass"));
    let evidence = json!({
        "phase": 10,
        "baseline_commit": std::env::var("SRCMV_PILOT_BASELINE").expect("baseline commit is required"),
        "operating_system": std::env::var("SRCMV_PILOT_OS").expect("pilot OS is required"),
        "architecture": std::env::var("SRCMV_PILOT_ARCH").expect("pilot architecture is required"),
        "filesystem": std::env::var("SRCMV_PILOT_FILESYSTEM").expect("pilot filesystem is required"),
        "repository": repository_root(),
        "workflow": "inspect -> preview -> commit --expect-plan",
        "accept_current_plan_used": false,
        "scenarios": results
    });
    let evidence_path = root.join("evidence.json");
    let mut file = fs::File::create(&evidence_path).expect("evidence file should be created");
    serde_json::to_writer_pretty(&mut file, &evidence).expect("evidence should serialize");
    file.write_all(b"\n").expect("evidence should end in LF");
    println!("phase 10 pilot evidence: {}", evidence_path.display());
}
