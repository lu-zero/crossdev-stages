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
///    resolve raw commits, so the commit is put in the bare cache and the
///    checkout is cloned from the cache instead of from the network.
///
/// `dest` is replaced, not reused.  A checkout is derived state: the tree
/// left by a run that stopped after cloning and before its patches applied
/// is at some other tag, or half patched, and `git clone` into it only
/// fails with "already exists and is not an empty directory".
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
        // `--shared` borrows the cache's objects, so the pin costs no
        // transfer and no default-branch checkout for the pinned commit to
        // overwrite.  The network is touched only on a cache miss, where
        // `git fetch <sha>` asks for an object no ref advertises: that needs
        // protocol v2 (git's default, `protocol.version`) or the server's
        // uploadpack.allowReachableSHA1InWant -- git.kernel.org refuses it
        // over v0.  A fetched commit is unreferenced in a bare repo, so
        // refs/pins/ is what keeps the cache's own gc off it.
        runner.run(&format!(
            "rm -rf {dest} && \
             if ! git -C {cache} cat-file -e {tag} 2>/dev/null; then \
                 git -C {cache} fetch origin {tag} && \
                 git -C {cache} update-ref refs/pins/{tag} {tag}; \
             fi && \
             git clone --shared --no-checkout {cache} {dest} && \
             git -C {dest} remote set-url origin {repo} && \
             git -C {dest} checkout --detach {tag}"
        ))
    } else {
        runner.run(&format!(
            "rm -rf {dest} && \
             git clone --reference {cache} --depth=1 --branch {tag} {repo} {dest}"
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
