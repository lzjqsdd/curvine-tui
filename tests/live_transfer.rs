//! Integration tests against a live Curvine cluster.
//!
//! Skipped unless `CURVINE_TUI_LIVE=1` is set, so plain `cargo test` stays
//! hermetic:
//!
//! ```sh
//! CURVINE_TUI_LIVE=1 CURVINE_CONF_FILE=/tmp/curvine-e2e/curvine-cluster.toml \
//!     cargo test --test live_transfer -- --test-threads=1 --nocapture
//! ```

use std::path::PathBuf;
use std::process::Command;

fn live() -> bool {
    std::env::var("CURVINE_TUI_LIVE").as_deref() == Ok("1")
}

macro_rules! skip_unless_live {
    () => {
        if !live() {
            eprintln!("skipped: set CURVINE_TUI_LIVE=1 with a running cluster");
            return;
        }
    };
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_curvine-tui")
}

fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("curvine-tui-live");
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

#[test]
fn probe_reports_a_healthy_cluster() {
    skip_unless_live!();

    let out = Command::new(bin()).arg("--probe").output().expect("probe");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(out.status.success(), "probe failed:\n{stdout}\n{stderr}");
    assert!(
        stdout.contains("all checks passed"),
        "unexpected probe output:\n{stdout}"
    );
    assert!(stdout.contains("cluster  : OK"), "cluster:\n{stdout}");
    assert!(stdout.contains("ls /     : OK"), "listing:\n{stdout}");
}

#[test]
fn upload_then_download_is_byte_identical() {
    skip_unless_live!();

    let dir = tmp_dir();
    // Deterministic, non-compressible pattern so a truncated or reordered
    // transfer cannot accidentally compare equal.
    let payload: Vec<u8> = (0..3_000_000u32).map(|i| ((i * 7 + 13) % 256) as u8).collect();
    let src = dir.join("roundtrip-src.bin");
    let dst = dir.join("roundtrip-dst.bin");
    std::fs::write(&src, &payload).expect("write payload");
    let _ = std::fs::remove_file(&dst);

    let remote = format!("/e2e/tui-roundtrip-{}.bin", std::process::id());

    let up = Command::new(bin())
        .args(["--put", src.to_str().unwrap(), &remote])
        .output()
        .expect("upload");
    assert!(
        up.status.success(),
        "upload failed: {}{}",
        String::from_utf8_lossy(&up.stdout),
        String::from_utf8_lossy(&up.stderr)
    );

    let down = Command::new(bin())
        .args(["--get", &remote, dst.to_str().unwrap()])
        .output()
        .expect("download");
    assert!(
        down.status.success(),
        "download failed: {}{}",
        String::from_utf8_lossy(&down.stdout),
        String::from_utf8_lossy(&down.stderr)
    );

    let fetched = std::fs::read(&dst).expect("read downloaded file");
    assert_eq!(
        fetched.len(),
        payload.len(),
        "size mismatch: got {} want {}",
        fetched.len(),
        payload.len()
    );
    assert_eq!(fetched, payload, "downloaded bytes differ from source");
}

#[test]
fn missing_remote_file_fails_cleanly() {
    skip_unless_live!();

    let dst = tmp_dir().join("should-not-exist.bin");
    let _ = std::fs::remove_file(&dst);

    let out = Command::new(bin())
        .args(["--get", "/e2e/definitely-absent.bin", dst.to_str().unwrap()])
        .output()
        .expect("download");

    assert!(
        !out.status.success(),
        "expected a non-zero exit for a missing remote file"
    );
    assert!(
        !dst.exists() || std::fs::read(&dst).unwrap().is_empty(),
        "a failed download must not leave partial content behind"
    );
}

#[test]
fn fsck_matches_the_reported_state() {
    skip_unless_live!();

    // The probe runs fsck over "/" and prints a health line; a healthy cluster
    // must report 100% and no warnings.
    let out = Command::new(bin())
        .arg("--probe")
        .env("CURVINE_TUI_PROBE_FSCK_PATH", "/")
        .output()
        .expect("probe");
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        out.status.success(),
        "probe failed:\n{}\n{}",
        stdout,
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("health        : 100.0%"), "fsck health:\n{stdout}");
    assert!(stdout.contains("warnings: false"), "fsck warnings:\n{stdout}");
}

#[test]
fn bench_produces_metrics() {
    skip_unless_live!();

    let out = Command::new(bin())
        .arg("--probe")
        .env("CURVINE_TUI_PROBE_BENCH", "1")
        .output()
        .expect("probe");
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        out.status.success(),
        "probe failed:\n{}\n{}",
        stdout,
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("bench    : OK"), "bench did not run:\n{stdout}");
    // The basic profile always reports these single-op suites.
    assert!(stdout.contains("Create"), "create metric missing:\n{stdout}");
    assert!(stdout.contains("ReadBig"), "read metric missing:\n{stdout}");
    assert!(stdout.contains("ops/s"), "ops unit missing:\n{stdout}");
}
