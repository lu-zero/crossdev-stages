use crate::container::SandboxRunner;
use crate::error::Result;

/// Clone a git repo into `dest`, using a bare repo cache to avoid repeated
/// network fetches.
///
/// Cache: `/cache/sources/{name}.git` (bare repo, bind-mounted from workspace).
///
/// 1. Bare cache missing -> `git clone --bare`
/// 2. Bare cache exists -> `git fetch`
/// 3. `git clone --reference cache --depth=1 --branch tag repo dest`
pub fn cached_clone(
    runner: &SandboxRunner,
    repo: &str,
    tag: &str,
    dest: &str,
    name: &str,
) -> Result<()> {
    let cache = format!("/cache/sources/{name}.git");

    runner.run(&format!(
        "if [ -d {cache} ]; then \
             git -C {cache} fetch --prune 2>/dev/null || true; \
         else \
             git clone --bare {repo} {cache}; \
         fi"
    ))?;

    runner.run(&format!(
        "git clone --reference {cache} --depth=1 --branch {tag} {repo} {dest}"
    ))
}
