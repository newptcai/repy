use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;

#[test]
fn test_history_flag() {
    let mut cmd = Command::cargo_bin("repy").unwrap();
    cmd.arg("-r");
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("history: true"));
}

#[test]
fn test_dump_flag() {
    let mut cmd = Command::cargo_bin("repy").unwrap();
    cmd.arg("--dump");
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("dump: true"));
}

#[test]
fn test_ebook_arg() {
    let mut cmd = Command::cargo_bin("repy").unwrap();
    cmd.arg("my_book.epub");
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("ebook: [\"my_book.epub\"]"));
}

