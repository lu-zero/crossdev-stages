use std::process::Command;

fn main() {
    // Embed the git commit of this source tree so manifest emission can
    // record which crossdev-stages built a given image. Re-runs on any
    // change to the git state.
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/refs");
    println!("cargo:rerun-if-changed=../.git/index");

    let mut commit = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string());

    // Append -dirty when the working tree has uncommitted changes so a
    // build from a modified checkout can be told apart in the lock file.
    let dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .map(|out| !out.stdout.is_empty())
        .unwrap_or(false);
    if dirty {
        commit.push_str("-dirty");
    }

    println!("cargo:rustc-env=CROSSDEV_STAGES_GIT_COMMIT={commit}");
}
