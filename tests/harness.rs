//! End-to-end compiler tests: `tests/run` and `tests/compile-fail`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn tests_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tests")
        .canonicalize()
        .expect("tests directory")
}

fn sattlec() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_sattlec"))
}

fn satl_files(dir: &Path) -> Vec<PathBuf> {
    if !dir.exists() {
        return Vec::new();
    }
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension().is_some_and(|ext| ext == "satl") {
                Some(path)
            } else {
                None
            }
        })
        .collect();
    files.sort();
    files
}

fn sidecar(satl: &Path, ext: &str) -> PathBuf {
    satl.with_extension(ext)
}

fn read_optional(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

#[test]
fn run_cases() {
    let dir = tests_root().join("run");
    let cases = satl_files(&dir);
    assert!(
        !cases.is_empty(),
        "expected at least one .satl in {}",
        dir.display()
    );

    let mut failures = Vec::new();
    for satl in &cases {
        if let Err(msg) = run_case(satl) {
            failures.push(format!("{}: {msg}", satl.display()));
        }
    }
    assert!(
        failures.is_empty(),
        "run cases failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn compile_fail_cases() {
    let dir = tests_root().join("compile-fail");
    let mut failures = Vec::new();
    for satl in satl_files(&dir) {
        if let Err(msg) = compile_fail_case(&satl) {
            failures.push(format!("{}: {msg}", satl.display()));
        }
    }
    assert!(
        failures.is_empty(),
        "compile-fail cases failed:\n{}",
        failures.join("\n")
    );
}

fn run_case(satl: &Path) -> Result<(), String> {
    let tmp = std::env::temp_dir().join("sattle-harness").join(
        satl.file_stem()
            .ok_or_else(|| format!("no stem for {}", satl.display()))?,
    );
    fs::create_dir_all(&tmp).map_err(|e| format!("mkdir {}: {e}", tmp.display()))?;
    let exe = tmp.join("a");

    let compile = Command::new(sattlec())
        .arg(satl)
        .arg("-o")
        .arg(&exe)
        .output()
        .map_err(|e| format!("spawn sattlec: {e}"))?;
    if !compile.status.success() {
        return Err(format!(
            "compile failed (status {:?})\nstdout:\n{}\nstderr:\n{}",
            compile.status.code(),
            String::from_utf8_lossy(&compile.stdout),
            String::from_utf8_lossy(&compile.stderr)
        ));
    }

    let expected_exit: i32 = read_optional(&sidecar(satl, "exit"))
        .map(|s| {
            s.trim()
                .parse()
                .map_err(|e| format!("bad .exit: {e}"))
        })
        .transpose()?
        .unwrap_or(0);

    let mut cmd = Command::new(&exe);
    if let Some(stdin) = read_optional(&sidecar(satl, "stdin")) {
        cmd.stdin(Stdio::piped());
        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("spawn {}: {e}", exe.display()))?;
        if let Some(mut pipe) = child.stdin.take() {
            pipe.write_all(stdin.as_bytes())
                .map_err(|e| format!("write stdin: {e}"))?;
        }
        let output = child
            .wait_with_output()
            .map_err(|e| format!("wait {}: {e}", exe.display()))?;
        check_run_output(satl, &output, expected_exit)
    } else {
        let output = cmd
            .output()
            .map_err(|e| format!("run {}: {e}", exe.display()))?;
        check_run_output(satl, &output, expected_exit)
    }
}

fn check_run_output(
    satl: &Path,
    output: &std::process::Output,
    expected_exit: i32,
) -> Result<(), String> {
    let code = output.status.code().unwrap_or(-1);
    if code != expected_exit {
        return Err(format!(
            "exit {code}, expected {expected_exit}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    if let Some(expected) = read_optional(&sidecar(satl, "stdout")) {
        let actual = String::from_utf8_lossy(&output.stdout);
        if actual.as_ref() != expected {
            return Err(format!(
                "stdout mismatch\nexpected:\n{expected:?}\nactual:\n{actual:?}"
            ));
        }
    }

    if let Some(needle) = read_optional(&sidecar(satl, "stderr")) {
        let actual = String::from_utf8_lossy(&output.stderr);
        if !actual.contains(needle.trim_end()) && !actual.contains(&needle) {
            return Err(format!(
                "stderr missing {:?}\nactual:\n{actual}",
                needle.trim_end()
            ));
        }
    }

    Ok(())
}

fn compile_fail_case(satl: &Path) -> Result<(), String> {
    let tmp = std::env::temp_dir().join("sattle-harness-fail").join(
        satl.file_stem()
            .ok_or_else(|| format!("no stem for {}", satl.display()))?,
    );
    fs::create_dir_all(&tmp).map_err(|e| format!("mkdir {}: {e}", tmp.display()))?;
    let exe = tmp.join("a");

    let compile = Command::new(sattlec())
        .arg(satl)
        .arg("-o")
        .arg(&exe)
        .output()
        .map_err(|e| format!("spawn sattlec: {e}"))?;

    let code = compile.status.code().unwrap_or(-1);
    match read_optional(&sidecar(satl, "exit")) {
        Some(raw) => {
            let expected: i32 = raw.trim().parse().map_err(|e| format!("bad .exit: {e}"))?;
            if code != expected {
                return Err(format!("exit {code}, expected {expected}"));
            }
        }
        None if code == 0 => {
            return Err("compiler succeeded, expected failure".into());
        }
        None => {}
    }

    if let Some(needle) = read_optional(&sidecar(satl, "stderr")) {
        let actual = String::from_utf8_lossy(&compile.stderr);
        if !actual.contains(needle.trim_end()) && !actual.contains(&needle) {
            return Err(format!(
                "compiler stderr missing {:?}\nactual:\n{actual}",
                needle.trim_end()
            ));
        }
    }

    Ok(())
}
