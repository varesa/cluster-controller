use std::process::Command;
use std::fs;
use regex::Regex;

fn main() {
    let output = Command::new("git").args(&["rev-parse", "HEAD"]).output().unwrap();
    let git_hash = String::from_utf8(output.stdout).map_or("undetermined".into(), |o| o);

    let output = Command::new("git").args(&["rev-list", "--count", "HEAD"]).output().unwrap();
    let git_count = String::from_utf8(output.stdout).map_or("undetermined".into(), |o| o);

    println!("cargo:rustc-env=GIT_HASH={}", git_hash);
    println!("cargo:rustc-env=GIT_COUNT={}", git_count);
    println!("cargo:rustc-rerun-if-changed=.git/HEAD");

    if let Ok(head) = fs::read_to_string(".git/HEAD") {
        let re = Regex::new(r"ref: (.*)").unwrap();
        if let Some(captures) = re.captures(&head) {
            println!("cargo:rustc-rerun-if-changed=.git/{}",
                     captures.get(1).map_or("", |m| m.as_str()));
        }
    }
}
