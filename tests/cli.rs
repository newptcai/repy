use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;

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
fn test_history_number_out_of_range_fails() {
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("repy"));
    cmd.env("XDG_CONFIG_HOME", dir.path());
    cmd.arg("7");
    cmd.assert()
        .failure()
        .stderr(predicates::str::contains("out of range"));
}
