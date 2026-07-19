use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;

fn sample_identity(book_id: &str) -> repy::models::BookIdentity {
    repy::models::BookIdentity {
        book_id: book_id.into(),
        identifier: None,
        title: Some("CLI Test Book".into()),
        creator: Some("Test Author".into()),
        spine_hrefs_hash: "spines".into(),
        content_fingerprints_hash: "content".into(),
    }
}

#[test]
fn test_history_flag() {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("repy"));
    cmd.env("REPY_CLI_ECHO", "1");
    cmd.arg("-r");
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("history: true"));
}

#[test]
fn test_dump_flag() {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("repy"));
    cmd.env("REPY_CLI_ECHO", "1");
    cmd.arg("--dump");
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("dump: true"));
}

#[test]
fn test_ebook_arg() {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("repy"));
    cmd.env("REPY_CLI_ECHO", "1");
    cmd.arg("my_book.epub");
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("ebook: [\"my_book.epub\"]"));
}

#[test]
fn test_bash_completions() {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("repy"));
    cmd.arg("--completions").arg("bash");
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("repy"));
}

#[test]
fn test_zsh_completions() {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("repy"));
    cmd.arg("--completions").arg("zsh");
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("repy"));
}

#[test]
fn test_fish_completions() {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("repy"));
    cmd.arg("--completions").arg("fish");
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("repy"));
}

#[test]
fn test_powershell_completions() {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("repy"));
    cmd.arg("--completions").arg("powershell");
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("repy"));
}

#[test]
fn test_invalid_completion_shell_fails() {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("repy"));
    cmd.arg("--completions").arg("invalid");
    cmd.assert()
        .failure()
        .stderr(predicates::str::contains("invalid value"));
}

#[test]
fn test_history_empty() {
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("repy"));
    cmd.env("XDG_CONFIG_HOME", dir.path());
    cmd.arg("-r");
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("Reading history is empty."));
}

#[test]
fn test_dump_fixture_epub() {
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("repy"));
    cmd.env("XDG_CONFIG_HOME", dir.path());
    cmd.arg("--dump").arg("tests/fixtures/small.epub");
    cmd.assert()
        .success()
        .stdout(predicates::str::is_empty().not());
}

#[test]
fn test_dump_uses_defaults_when_config_is_invalid() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("broken.json");
    let broken = "{ invalid json }";
    std::fs::write(&config_path, broken).unwrap();

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("repy"));
    cmd.arg("--config")
        .arg(&config_path)
        .arg("--dump")
        .arg("tests/fixtures/small.epub");
    cmd.assert()
        .success()
        .stdout(predicates::str::is_empty().not())
        .stderr(predicates::str::contains("Config invalid, using defaults"));

    assert_eq!(std::fs::read_to_string(config_path).unwrap(), broken);
}

#[test]
fn test_dump_fixture_markdown() {
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("repy"));
    cmd.env("XDG_CONFIG_HOME", dir.path());
    cmd.arg("--dump").arg("tests/fixtures/sample.md");
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("The Little Markdown Book"))
        .stdout(predicates::str::contains("Call me Ishmael."));
}

#[test]
fn test_dump_fixture_plain_text() {
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("repy"));
    cmd.env("XDG_CONFIG_HOME", dir.path());
    cmd.arg("--dump").arg("tests/fixtures/sample.txt");
    cmd.assert().success().stdout(predicates::str::contains(
        "This paragraph is hard-wrapped across several short lines",
    ));
}

#[test]
fn test_dump_without_ebook_fails() {
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("repy"));
    cmd.env("XDG_CONFIG_HOME", dir.path());
    cmd.arg("--dump");
    cmd.assert()
        .failure()
        .stderr(predicates::str::contains("provide an ebook"));
}

#[test]
fn test_unmatched_pattern_fails() {
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("repy"));
    cmd.env("XDG_CONFIG_HOME", dir.path());
    cmd.arg("no-such-book-xyz");
    cmd.assert()
        .failure()
        .stderr(predicates::str::contains("no history entry matches"));
}

#[test]
fn test_export_highlights_markdown() {
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("repy"));
    cmd.env("XDG_CONFIG_HOME", dir.path());
    cmd.arg("--export-highlights")
        .arg("tests/fixtures/small.epub")
        .arg("--format")
        .arg("md");
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("# Highlights: Accessible EPUB 3"));
}

#[test]
fn test_export_stats_json() {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().join("repy");
    let state = repy::state::State::new_at(data_dir.join("states.db")).unwrap();
    let identity = sample_identity("cli-stats");
    state.upsert_book_record(&identity).unwrap();
    let ended = chrono::Utc::now();
    state
        .insert_reading_session(
            &identity.book_id,
            ended - chrono::Duration::minutes(20),
            ended,
            12,
            400,
        )
        .unwrap();
    state
        .insert_reading_session(
            &identity.book_id,
            ended - chrono::Duration::minutes(10),
            ended,
            8,
            200,
        )
        .unwrap();
    drop(state);

    let output = dir.path().join("stats.json");
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("repy"));
    cmd.env("XDG_CONFIG_HOME", dir.path())
        .arg("--export-stats")
        .arg(&output);
    cmd.assert().success();
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(output).unwrap()).unwrap();
    assert_eq!(value["global"]["sessions"], 2);
    assert_eq!(value["books"][0]["title"], "CLI Test Book");
}

#[test]
fn test_export_stats_errors_when_empty() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("stats.json");
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("repy"));
    cmd.env("XDG_CONFIG_HOME", dir.path())
        .arg("--export-stats")
        .arg(&output);
    cmd.assert().failure().stderr(predicates::str::contains(
        "statistics database is missing or empty",
    ));
    assert!(!output.exists());
}

#[test]
fn test_history_number_out_of_range_fails() {
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("repy"));
    cmd.env("XDG_CONFIG_HOME", dir.path());
    cmd.arg("7");
    cmd.assert()
        .failure()
        .stderr(predicates::str::contains("out of range"));
}
