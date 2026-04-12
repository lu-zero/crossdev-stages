use crate::container::SandboxRunner;
use crate::error::Result;

/// Clone a git repo into `dest`, using a bare repo cache to avoid repeated
/// network fetches.
///
/// Cache: `/cache/sources/{cache_name}.git` where cache_name is derived from
/// the repo URL to avoid collisions between different repos with the same
/// component name (e.g. K1 opensbi vs K230 opensbi).
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
    let cache_name = repo_cache_name(repo, name);
    let cache = format!("/cache/sources/{cache_name}.git");

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

/// Derive a unique cache directory name from repo URL.
/// "https://github.com/cyyself/opensbi" -> "cyyself-opensbi"
/// "https://gitee.com/bianbu-linux/linux-6.6.git" -> "bianbu-linux-linux-6.6"
fn repo_cache_name(repo: &str, fallback: &str) -> String {
    let stripped = repo
        .trim_end_matches('/')
        .trim_end_matches(".git");
    if let Some(idx) = stripped.rfind("://") {
        let path = &stripped[idx + 3..];
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() >= 2 {
            return format!("{}-{}", parts[parts.len() - 2], parts[parts.len() - 1]);
        }
    }
    fallback.to_string()
}
