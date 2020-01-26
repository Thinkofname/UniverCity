
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let git_hash = Command::new("git")
        .args(&["rev-parse", "--short", "HEAD"])
        .output()
        .unwrap();

    let git_hash = String::from_utf8(git_hash.stdout).unwrap();
    println!("cargo:rustc-env=GAME_HASH={}", git_hash);
}