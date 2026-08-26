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
/// 3. Branch/tag: `git clone --reference cache --depth=1 --branch tag repo dest`
///    Commit SHA (from a pinned build.lock.toml): `git clone --branch` cannot
///    resolve raw commits, so clone without it, fetch the commit explicitly
///    and check it out detached.
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

    if is_commit_sha(tag) {
        runner.run(&format!(
            "git clone --reference {cache} {repo} {dest} && \
             git -C {dest} fetch origin {tag} && \
             git -C {dest} checkout --detach FETCH_HEAD"
        ))
    } else {
        runner.run(&format!(
            "git clone --reference {cache} --depth=1 --branch {tag} {repo} {dest}"
        ))
    }
}

/// True for a full 40-hex git commit SHA, as written into build.lock.toml
/// by `git rev-parse HEAD` and fed back through `image build --pinned`.
pub(crate) fn is_commit_sha(tag: &str) -> bool {
    tag.len() == 40 && tag.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Derive a unique cache directory name from repo URL.
/// "https://github.com/cyyself/opensbi" -> "cyyself-opensbi"
/// "https://gitee.com/bianbu-linux/linux-6.6.git" -> "bianbu-linux-linux-6.6"
fn repo_cache_name(repo: &str, fallback: &str) -> String {
    let stripped = repo.trim_end_matches('/').trim_end_matches(".git");
    if let Some(idx) = stripped.rfind("://") {
        let path = &stripped[idx + 3..];
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() >= 2 {
            return format!("{}-{}", parts[parts.len() - 2], parts[parts.len() - 1]);
        }
    }
    fallback.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_hex_sha_detected() {
        assert!(is_commit_sha("0123456789abcdef0123456789abcdef01234567"));
        assert!(is_commit_sha("0123456789ABCDEF0123456789ABCDEF01234567"));
    }

    #[test]
    fn refs_are_not_shas() {
        assert!(!is_commit_sha("v2.10.1"));
        assert!(!is_commit_sha("master"));
        assert!(!is_commit_sha("linux-6.6.y"));
        // abbreviated sha: build.lock.toml only ever contains full 40-hex
        assert!(!is_commit_sha("0123456789abcdef"));
        // right length, non-hex
        assert!(!is_commit_sha("branch-name-that-is-forty-characters-xyz"));
    }
}
