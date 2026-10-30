use std::io::Write;
use std::process::{Command, Stdio};

use diesel::prelude::*;
use wasd5::{db, pages};

fn command(database: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_pages"));
    command
        .env("DATABASE_URL", database)
        .env_remove("WASD_PAGE_EDITOR")
        .env_remove("EDITOR");
    command
}

#[test]
fn list_prompt_history_and_visibility() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("test.db");
    let markdown = dir.path().join("new.md");
    std::fs::write(&markdown, "# CLI article").unwrap();
    let mut child = command(&database)
        .args(["create", "nested/article"])
        .arg(&markdown)
        .arg("--hidden")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"Alice\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("WASD_PAGE_EDITOR"));
    assert!(stderr.contains("Who is editing"));
    let output = command(&database).arg("list").output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("1\tdraft\tnested/article\tAlice"));
    assert!(
        command(&database)
            .args(["publish", "1"])
            .env("WASD_PAGE_EDITOR", "Bob")
            .status()
            .unwrap()
            .success()
    );
    let output = command(&database).args(["history", "1"]).output().unwrap();
    assert!(output.status.success());
    let revisions: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(revisions[0]["editor"], "Bob");
    assert_eq!(revisions[1]["editor"], "Alice");
    assert_eq!(revisions[0]["content"], "# CLI article");
    assert_eq!(revisions[0]["hidden"], false);
    let output = command(&database)
        .args(["edit", "1", "--editor", "Alice"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("EDITOR is not set"));
    let output = command(&database)
        .args(["hide", "1", "--editor", " "])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn editor_command_arguments_save_and_failure() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("test.db");
    let mut conn = SqliteConnection::establish(database.to_str().unwrap()).unwrap();
    db::run_migrations(&mut conn).unwrap();
    let page = pages::create(&mut conn, "article", "before", "Alice", false).unwrap();
    let script = dir.path().join(if cfg!(windows) {
        "editor.ps1"
    } else {
        "editor.sh"
    });
    #[cfg(windows)]
    let (success, failure, program) = (
        "param([string]$path)\n$ErrorActionPreference = 'Stop'\n[System.IO.File]::WriteAllText($path, '# Edited')",
        "param([string]$path)\n$ErrorActionPreference = 'Stop'\n[System.IO.File]::WriteAllText($path, 'unsaved')\nexit 1",
        "powershell -NoProfile -File",
    );
    #[cfg(not(windows))]
    let (success, failure, program) = (
        "printf '# Edited' > \"$1\"",
        "printf unsaved > \"$1\"\nexit 1",
        "sh",
    );
    std::fs::write(&script, success).unwrap();
    let editor = format!("{program} '{}'", script.to_str().unwrap());
    let output = command(&database)
        .args(["edit", "1"])
        .env("EDITOR", &editor)
        .env("WASD_PAGE_EDITOR", "Bob")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        pages::by_id(&mut conn, page.id).unwrap().content,
        "# Edited",
        "stdout: {}; stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        pages::editors(&pages::history(&mut conn, page.id).unwrap()),
        vec!["Alice", "Bob"]
    );
    std::fs::write(&script, failure).unwrap();
    let output = command(&database)
        .args(["edit", "1", "--editor", "Carol"])
        .env("EDITOR", editor)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("SQL page was not changed"));
    let kept = stderr
        .lines()
        .find_map(|line| line.strip_prefix("Your temporary Markdown file was kept at "))
        .unwrap();
    assert_eq!(std::fs::read_to_string(kept).unwrap(), "unsaved");
    std::fs::remove_file(kept).unwrap();
    assert_eq!(
        pages::by_id(&mut conn, page.id).unwrap().content,
        "# Edited"
    );
    assert_eq!(pages::history(&mut conn, page.id).unwrap().len(), 2);
}
