use camino::{Utf8Path, Utf8PathBuf};

use chrono::Utc;

use crate::board::BoardConfig;
use crate::container::SandboxRunner;
use crate::error::{Error, Result};
use crate::portage::Portage;
use crate::provider::RootfsProvider;
use crate::sandbox::Sandbox;
use crate::target::Target;
use crate::workspace::Workspace;

/// Where `boards/<board>/` is bind-mounted read-only, so config the cross
/// prefix needs can be copied in from inside the container.
const BOARD_DIR_IN_CONTAINER: &str = "/.board-config";

fn project_root(boards_root: &Utf8Path) -> Utf8PathBuf {
    boards_root.parent().unwrap_or(boards_root).to_path_buf()
}

// ── Build directory ─────────────────────────────────────────────────────────

pub struct Build {
    pub dir: Utf8PathBuf,
    pub board: String,
}

impl Build {
    pub fn create(ws: &Workspace, board: &str) -> Result<Self> {
        // Layout is builds/<board>/<timestamp>/, one fresh leaf per build.
        // A flat pre-nesting dir (builds/<board>/ carrying the .board
        // marker itself) would swallow new leaves, so migrate it into a
        // nested leaf first.
        migrate_legacy_build(ws, board)?;

        // Resume the newest unpacked leaf for this board; steps resume
        // via the .{step} markers inside the leaf.
        if let Ok(builds) = ws.list_builds() {
            for dir in builds {
                if let Some(b) = Self::open(dir.clone()) {
                    if b.board == board && !b.is_done("packed") {
                        tracing::info!("Resuming build: {}", dir);
                        return Ok(b);
                    }
                }
            }
        }
        let ts = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let dir = ws.builds_dir().join(board).join(&ts);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(".board"), board)?;
        Ok(Self {
            dir,
            board: board.to_string(),
        })
    }

    pub fn open(dir: Utf8PathBuf) -> Option<Self> {
        let board = std::fs::read_to_string(dir.join(".board"))
            .ok()
            .map(|s| s.trim().to_string())?;
        Some(Self { dir, board })
    }

    /// Wall-clock build timestamp embedded in the produced image filename.
    /// The leaf directory name is the timestamp (builds/<board>/<ts>/) —
    /// single source of truth, stable across resume.
    pub fn timestamp(&self) -> String {
        self.dir
            .file_name()
            .map(str::to_string)
            .unwrap_or_else(|| Utc::now().format("%Y%m%dT%H%M%SZ").to_string())
    }

    fn marker(&self, step: &str) -> Utf8PathBuf {
        self.dir.join(format!(".{step}"))
    }

    fn is_done(&self, step: &str) -> bool {
        self.marker(step).exists()
    }

    fn mark_done(&self, step: &str) -> Result<()> {
        std::fs::write(self.marker(step), Utc::now().to_rfc3339())?;
        Ok(())
    }
}

/// Move a flat pre-nesting build (builds/<board>/ containing .board) into
/// the nested layout: builds/<board>/<ts>/.  The timestamp comes from the
/// legacy .timestamp file when present.  Without this, a new leaf created
/// under the legacy dir would be invisible to list_builds(), which treats
/// any first-level dir with a .board marker as an opaque legacy leaf.
fn migrate_legacy_build(ws: &Workspace, board: &str) -> Result<()> {
    let board_dir = ws.builds_dir().join(board);
    if !board_dir.join(".board").exists() {
        return Ok(());
    }
    let ts = std::fs::read_to_string(board_dir.join(".timestamp"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "legacy".to_string());
    let staging = ws.builds_dir().join(format!(".migrate-{board}"));
    std::fs::rename(&board_dir, &staging)?;
    std::fs::create_dir_all(&board_dir)?;
    let leaf = board_dir.join(&ts);
    std::fs::rename(&staging, &leaf)?;
    // Leaf name is the timestamp now; the marker file is retired.
    let _ = std::fs::remove_file(leaf.join(".timestamp"));
    tracing::info!("Migrated legacy build dir to {}", leaf);
    Ok(())
}

// ── Step runner with file-convention hooks ───────────────────────────────────
//
// For each step, check boards/<name>/ for:
//   override-{step}.sh  →  replaces Rust default entirely
//   pre-{step}.sh       →  runs before Rust default
//   post-{step}.sh      →  runs after Rust default

fn run_step(
    step: &str,
    marker: &str,
    build: &Build,
    runner: &SandboxRunner,
    boards_root: &Utf8Path,
    board: &BoardConfig,
    default_fn: impl FnOnce(&SandboxRunner) -> Result<()>,
) -> Result<()> {
    if build.is_done(marker) {
        return Ok(());
    }

    let board_dir = boards_root.join(&board.name);

    let override_sh = format!("override-{step}.sh");
    if board_dir.join(&override_sh).exists() {
        runner.run(&run_board_script(board, &override_sh))?;
        return build.mark_done(marker);
    }

    let pre_sh = format!("pre-{step}.sh");
    if board_dir.join(&pre_sh).exists() {
        runner.run(&run_board_script(board, &pre_sh))?;
    }

    default_fn(runner)?;

    let post_sh = format!("post-{step}.sh");
    if board_dir.join(&post_sh).exists() {
        runner.run(&run_board_script(board, &post_sh))?;
    }

    build.mark_done(marker)
}

fn run_board_script(board: &BoardConfig, script: &str) -> String {
    // Same order the Rust loader uses: includes first, board last, so a hook
    // sees exactly the values `board info` reports.  Sourcing only board.conf
    // here would leave the shell blind to everything the includes provide.
    let family: String = board
        .includes
        .iter()
        .map(|name| format!("source /scripts/boards/include/{name}.conf\n"))
        .collect();
    format!(
        "set -e\nexport LDCONFIG=/usr/local/bin/ldconfig\n{disk}{family}\
         source /scripts/boards/{name}/board.conf\n\
         source /scripts/boards/{name}/{script}",
        disk = DiskId::of(board).exports(),
        name = board.name,
    )
}

/// Identifiers for the partition table, derived from the board rather than
/// written out by hand or drawn at random.
///
/// A board needs a stable name for its root filesystem that does not depend on
/// which slot the card ends up in.  The kernel only understands `PARTUUID=`
/// without an initramfs (`block/early-lookup.c`), so that name has to come from
/// the partition table, which means it has to be decided before `assemble`
/// writes the boot config -- genimage's own `disk-signature = random` happens a
/// step too late to be of any use.
///
/// Deriving it from the board keeps both properties that matter: two boards
/// never collide, and building the same board twice gives the same image.  The
/// input is what actually decides the contents, not the whole file, so editing
/// a comment in board.conf does not renumber the disk.
struct DiskId {
    /// MBR disk signature, never zero: the kernel spells the PARTUUID of slot
    /// N as "{sig:08x}-{N:02x}" (`block/partitions/msdos.c`).
    sig: u32,
    /// GPT UUID, with the final byte left as the partition index.
    uuid: [u8; 16],
}

impl DiskId {
    fn of(board: &BoardConfig) -> Self {
        let identity = format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n",
            board.name,
            board.arch,
            board.chost(),
            board.kernel_repo,
            board.kernel_tag,
            board.effective_cflags(),
        );
        let hi = fnv1a_64(identity.as_bytes());
        let lo = fnv1a_64(format!("{identity}uuid\n").as_bytes());

        let mut uuid = [0u8; 16];
        uuid[..8].copy_from_slice(&hi.to_be_bytes());
        uuid[8..].copy_from_slice(&lo.to_be_bytes());

        // A zero signature is what genimage writes when none is configured, so
        // every unsigned card in the world already claims it.
        let sig = match (hi as u32) ^ ((hi >> 32) as u32) {
            0 => 1,
            n => n,
        };
        Self { sig, uuid }
    }

    /// `uuid` with its last byte set to `part`, so a disk and its partitions
    /// read as an obviously related set.
    fn part_uuid(&self, part: u8) -> String {
        let mut b = self.uuid;
        b[15] = part;
        format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-\
             {:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            b[0],
            b[1],
            b[2],
            b[3],
            b[4],
            b[5],
            b[6],
            b[7],
            b[8],
            b[9],
            b[10],
            b[11],
            b[12],
            b[13],
            b[14],
            b[15],
        )
    }

    /// Shell `export` lines, consumed both by board scripts (which source
    /// board.conf after these, so `BOOT_ROOT_DEV` can refer to them) and by
    /// genimage, whose config parser expands `${VAR}` through getenv.
    fn exports(&self) -> String {
        let mut out = format!(
            "export BOOT_DISK_ID={:08x}\nexport BOOT_DISK_SIG=0x{:08x}\n\
             export BOOT_DISK_UUID={}\n",
            self.sig,
            self.sig,
            self.part_uuid(0),
        );
        for part in 1..=4u8 {
            out.push_str(&format!(
                "export BOOT_PART_UUID_{part}={}\n",
                self.part_uuid(part)
            ));
        }
        out
    }
}

/// FNV-1a, 64-bit.  The same hash the CFLAGS tooling uses, and short enough to
/// keep here rather than take a dependency for six lines.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Ebuild repositories the sandbox will search, as host paths.
fn ebuild_repos(sandbox_dir: &Utf8Path) -> Vec<Utf8PathBuf> {
    let mut repos: Vec<Utf8PathBuf> = Vec::new();
    let mut push = |dir: Utf8PathBuf| {
        if dir.join("profiles").is_dir() && !repos.contains(&dir) {
            repos.push(dir);
        }
    };

    // repos.conf may be a file or a directory of files.
    let conf = sandbox_dir.join("etc/portage/repos.conf");
    let mut files = vec![conf.clone()];
    if let Ok(entries) = std::fs::read_dir(&conf) {
        files.extend(entries.filter_map(|e| Utf8PathBuf::from_path_buf(e.ok()?.path()).ok()));
    }
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for line in text.lines() {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim() == "location" {
                    push(sandbox_dir.join(value.trim().trim_start_matches('/')));
                }
            }
        }
    }
    // `eselect repository` drops an overlay here with no repos.conf entry.
    if let Ok(entries) = std::fs::read_dir(sandbox_dir.join("var/db/repos")) {
        for entry in entries.filter_map(|e| e.ok()) {
            if let Ok(dir) = Utf8PathBuf::from_path_buf(entry.path()) {
                push(dir);
            }
        }
    }
    repos
}

/// The category and package of an emerge atom, with everything a directory
/// name does not carry stripped: the comparison operator, the slot, and the
/// version glued onto the package name.
fn atom_cpn(atom: &str) -> Option<(&str, &str)> {
    let cpn = atom
        .trim_start_matches(['=', '<', '>', '~', '!'])
        .split(':')
        .next()?;
    let (category, rest) = cpn.split_once('/')?;
    // A dash followed by a digit starts the version; anything else is part of
    // the name, which is why sys-apps and gst-plugins-base survive this.
    let package = match rest.rsplit_once('-') {
        Some((name, tail)) if tail.starts_with(|c: char| c.is_ascii_digit()) => name,
        _ => rest,
    };
    Some((category, package))
}

/// Reject atoms that name nothing in any repo, before anything is built.
///
/// Only the category/package part is checked.  Version ranges, slots and USE
/// deps are emerge's business; this is here to catch a package that was
/// renamed or never existed, which is the failure that costs a whole build.
fn check_package_lists(
    sandbox: &Sandbox,
    board: &BoardConfig,
    board_dir: &Utf8Path,
    defaults_root: &Utf8Path,
) -> Result<()> {
    let repos = ebuild_repos(&sandbox.dir);
    if repos.is_empty() {
        // No synced tree yet: the sandbox has not been prepared, and emerge
        // will say so far more clearly than we could.
        return Ok(());
    }

    let mut bad: Vec<String> = Vec::new();
    for list in [
        defaults_root.join("sandbox-packages.txt"),
        defaults_root.join("target-packages.txt"),
        board_dir.join("sandbox-packages.txt"),
        board_dir.join("target-packages.txt"),
    ] {
        let Ok(content) = std::fs::read_to_string(&list) else {
            continue;
        };
        for (number, line) in content.lines().enumerate() {
            let line = line.trim();
            // A `-atom` line takes something out of the defaults rather than
            // naming something to build, so there is nothing to look up.
            if line.is_empty() || line.starts_with('#') || line.starts_with('-') {
                continue;
            }
            let Some(atom) = line.split_whitespace().next() else {
                continue;
            };
            let Some((category, package)) = atom_cpn(atom) else {
                continue;
            };
            let found = repos
                .iter()
                .any(|repo| repo.join(category).join(package).symlink_metadata().is_ok());
            if !found {
                bad.push(format!("{list}:{}: {atom}", number + 1));
            }
        }
    }

    if bad.is_empty() {
        return Ok(());
    }
    Err(crate::error::Error::BoardConfigParse {
        file: board.name.clone(),
        msg: format!("no such package:\n  {}", bad.join("\n  ")),
    })
}

// ── Default implementations ─────────────────────────────────────────────────

/// Per-(chost, cflags-hash) binpkg cache dir for a board's target packages.
/// Single source for both the build-step runners and default_deps.
fn board_binpkgs_dir(ws: &Workspace, board: &BoardConfig) -> Result<Utf8PathBuf> {
    let hash = crate::cflags::binpkg_key(board);
    let dir = ws.binpkgs_dir().join(board.chost()).join(hash);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Host-side sandbox extras from boards/<name>/sandbox-packages.txt.
/// Provider-independent: provider bodies and hook scripts alike need
/// their host tools (grub, debootstrap, …) present in the sandbox.
fn install_sandbox_extras(
    sandbox: &Sandbox,
    board: &BoardConfig,
    boards_root: &Utf8Path,
) -> Result<()> {
    // Defaults are already installed during prepare; only the board's own
    // extras (e.g. grub for pentium-mmx) need emerging here.  merge() with
    // an empty base means a `-atom` line here can only cancel the board's
    // own extras, never uninstall a prepare-time default.
    let board_dir = boards_root.join(&board.name);
    let board_sandbox = crate::package_list::merge(
        Vec::new(),
        crate::package_list::read_optional(&board_dir.join("sandbox-packages.txt"))?,
    );
    if !board_sandbox.is_empty() {
        let portage_dir = sandbox.dir.join("etc/portage");
        crate::package_list::write_accept_keywords(&board_sandbox, &portage_dir)?;
        // USE overrides from sandbox-packages.use, e.g. "sys-boot/grub grub_platforms_pc"
        crate::package_list::write_package_use(
            &board_dir.join("sandbox-packages.use"),
            &portage_dir,
        )?;
        // Host-side packages don't touch the cross toolchain or the shared
        // target binpkg cache; a plain sandbox runner is enough.
        let host_runner = sandbox.runner();
        let portage = Portage::new(&host_runner);
        portage.emerge(&crate::package_list::atoms(&board_sandbox))?;
    }
    Ok(())
}

fn default_deps(
    _runner: &SandboxRunner,
    ws: &Workspace,
    sandbox: &Sandbox,
    target: &Target,
    board: &BoardConfig,
    boards_root: &Utf8Path,
    defaults_root: &Utf8Path,
) -> Result<()> {
    let board_dir = boards_root.join(&board.name);

    // Two portage config roots see a board's target packages: the cross
    // prefix's, which `{chost}-emerge` reads (PORTAGE_CONFIGROOT=/usr/<chost>),
    // and the target's, which portage reads once the image boots.  Board
    // portage config goes into both, so it does not depend on which one is in
    // play.  The cross prefix is an overlay mount, so its half is written
    // through the runner: a host-side write into sandbox/usr/<chost> lands
    // under the mount, where nothing in the container can read it.
    let cross_portage = format!("/usr/{}/etc/portage", board.chost());
    let target_portage = target.dir.join("etc/portage");
    let target_runner = sandbox
        .runner_for_board(ws, &board.arch, board)?
        .with_target(&target.dir)
        .with_binpkgs(&board_binpkgs_dir(ws, board)?)
        .with_extra_ro(&board_dir, BOARD_DIR_IN_CONTAINER);

    let board_patches = board_dir.join("portage-patches");
    if board_patches.is_dir() {
        crate::sandbox::copy_tree(&board_patches, &target_portage.join("patches"))?;
        target_runner.run(&format!(
            "mkdir -p {cross_portage}/patches && \
             cp -a {BOARD_DIR_IN_CONTAINER}/portage-patches/. {cross_portage}/patches/"
        ))?;
    }

    // package.provided cuts a dep chain off at an atom that does not
    // cross-build for the target: virtual/udev pulling in
    // sys-apps/systemd-utils on rv32-musl, say.
    let board_provided = board_dir.join("package.provided");
    if board_provided.is_file() {
        let profile_dir = target_portage.join("profile");
        std::fs::create_dir_all(&profile_dir)?;
        std::fs::copy(&board_provided, profile_dir.join("package.provided"))?;
        target_runner.run(&format!(
            "mkdir -p {cross_portage}/profile && \
             cp {BOARD_DIR_IN_CONTAINER}/package.provided \
             {cross_portage}/profile/package.provided"
        ))?;
    }

    // Target packages: defaults UNION board extras MINUS board `-atom` lines.
    let target_pkgs = crate::package_list::merge(
        crate::package_list::read_required(&defaults_root.join("target-packages.txt"))?,
        crate::package_list::read_optional(&board_dir.join("target-packages.txt"))?,
    );
    if !target_pkgs.is_empty() {
        let board_use = board_dir.join("target-packages.use");
        crate::package_list::write_accept_keywords(&target_pkgs, &target_portage)?;
        crate::package_list::write_package_use(&board_use, &target_portage)?;
        // The keyword a line asks for has to be written where the cross emerge
        // will look for it.  Without this a line like `sys-boot/syslinux **`
        // gets its atom emerged and its keyword silently dropped, and the
        // emerge fails on a masked package.
        if let Some(script) =
            crate::package_list::accept_keywords_script(&target_pkgs, &cross_portage)
        {
            target_runner.run(&script)?;
        }
        if board_use.is_file() {
            target_runner.run(&format!(
                "mkdir -p {cross_portage}/package.use && \
                 cp {BOARD_DIR_IN_CONTAINER}/target-packages.use \
                 {cross_portage}/package.use/board"
            ))?;
        }

        let portage = Portage::new(&target_runner);
        portage.cross_emerge(&board.chost(), &crate::package_list::atoms(&target_pkgs))?;
    }

    Ok(())
}

/// Read a `<distro>-packages.txt`: one package name per line, `#`
/// comments, blanks ignored.  Deliberately not the portage-atom parser --
/// that one takes a second whitespace-separated token as a KEYWORDS
/// request and would silently drop it here.
fn read_distro_packages(path: &Utf8Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    if !path.exists() {
        return Ok(out);
    }
    for line in std::fs::read_to_string(path)?.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.split_whitespace().nth(1).is_some() {
            return Err(crate::error::Error::BoardConfigParse {
                file: path.to_string(),
                msg: format!("one package per line: '{line}'"),
            });
        }
        out.push(line.to_string());
    }
    Ok(out)
}

/// Provision and populate a Debian or Ubuntu rootfs in /target via
/// debootstrap.  Runs inside the sandbox: stage 1 (`--foreign`)
/// downloads every .deb and unpacks the Priority:required set without
/// executing target binaries.
///
/// The second stage runs in a chroot by default, which is what every
/// other image builder does and what makes the shipped image a finished
/// system; a foreign arch needs qemu-user binfmt (F flag) on the host.
/// `ROOTFS_SECOND_STAGE="first-boot"` stops after stage 1 instead and
/// leaves the work to the board (see `SecondStage::FirstBoot` for what
/// that costs).
///
/// Extra packages come from the board's `<provider>-packages.txt` via
/// `--include`; they are installed during the second stage, so changing
/// the list means recreating the target.
fn debootstrap_deps(
    runner: &SandboxRunner,
    target: &Target,
    board: &BoardConfig,
    boards_root: &Utf8Path,
    deb: crate::provider::Debootstrap,
) -> Result<()> {
    let provider = board.rootfs_provider.name();
    let mode = board.second_stage;
    let bad_board = |msg: String| crate::error::Error::BoardConfigParse {
        file: board.name.clone(),
        msg,
    };

    // The marker records which second-stage mode produced the tree: a
    // stage-1-only target still carries /debootstrap and no init, so
    // reusing it for a chroot build (or the reverse) would ship a rootfs
    // the board.conf no longer describes.
    let done = target.dir.join(".debootstrap-done");
    if let Ok(prev) = std::fs::read_to_string(&done) {
        if prev.split_whitespace().next() == Some(mode.name()) {
            tracing::info!("{provider} rootfs already provisioned, skipping debootstrap.");
            return Ok(());
        }
        tracing::info!("Target was bootstrapped for a different ROOTFS_SECOND_STAGE, redoing it.");
    }

    let arch = crate::provider::dpkg_arch(&board.arch)
        .ok_or_else(|| bad_board(format!("no {provider} port for arch '{}'", board.arch)))?;
    // Ubuntu dropped i386 as a release architecture after 19.10.
    if board.rootfs_provider == crate::provider::RootfsProvider::Ubuntu && arch == "i386" {
        return Err(bad_board(
            "Ubuntu has had no i386 port since 19.10; use the debian provider".into(),
        ));
    }

    let suite = match (board.suite.as_deref(), deb.default_suite) {
        (Some(s), _) => s,
        (None, Some(d)) => d,
        (None, None) => {
            return Err(bad_board(format!(
                "{} has no rolling suite alias, so {} must name a codename \
                 (e.g. noble)",
                provider, deb.suite_key
            )))
        }
    };
    if deb.is_moving_alias(suite) {
        tracing::warn!(
            "{} '{suite}' is a moving alias; pin a codename \
             (e.g. trixie) for reproducible builds",
            deb.suite_key
        );
    }
    let mirror = board
        .mirror
        .as_deref()
        .unwrap_or_else(|| deb.default_mirror(arch));

    let extra = read_distro_packages(
        &boards_root
            .join(&board.name)
            .join(format!("{provider}-packages.txt")),
    )?;
    let include = if extra.is_empty() {
        String::new()
    } else {
        format!("--include={} ", extra.join(","))
    };

    let portage = Portage::new(runner);
    portage.emerge(&["--noreplace", "dev-util/debootstrap", deb.keyring_pkg])?;
    // A failed run leaves a partial tree, and debootstrap unpacks with
    // tar -k which hard-errors (FILEEXIST) on existing files -- wipe
    // everything but the workspace markers so a retry starts clean.
    runner.run(
        "find /target -mindepth 1 -maxdepth 1 \
         ! -name '.arch' ! -name '.stage3' ! -name '.provider' -exec rm -rf {} +",
    )?;
    // --keyring is always passed, never left to the suite script: Ubuntu's
    // picks its keyring only after an online end-of-life lookup, and falls
    // back to the removed-keys keyring when that lookup fails.  Naming the
    // mirror explicitly sidesteps the same lookup's mirror fallback.
    runner.run(&format!(
        "debootstrap --foreign --arch={arch} --keyring={keyring} \
         {include}{suite} /target {mirror}",
        keyring = deb.keyring,
    ))?;
    if mode == crate::provider::SecondStage::Chroot {
        runner
            .run("chroot /target /debootstrap/debootstrap --second-stage")
            .inspect_err(|_| {
                tracing::error!(
                    "debootstrap second stage failed -- a foreign-arch chroot \
                     needs qemu-user binfmt (registered with the F flag) on the \
                     host, or ROOTFS_SECOND_STAGE=\"first-boot\" to defer it"
                );
            })?;
    } else {
        tracing::warn!(
            "Second stage deferred to first boot: /debootstrap and every \
             downloaded .deb ship in the image, and the board finishes the \
             bootstrap itself."
        );
    }
    // debootstrap's device setup cannot mknod in a userns and leaves a
    // regular file at dev/null; empty /dev entirely -- devtmpfs mounts
    // over it at boot (the Gentoo path ships an empty /dev the same way).
    runner.run("find /target/dev -mindepth 1 -delete")?;
    std::fs::write(
        done,
        format!("{} {}\n", mode.name(), chrono::Utc::now().to_rfc3339()),
    )?;
    Ok(())
}

/// Alpine's default branch.  A codename-free distro, so unlike
/// DEBIAN_SUITE this is a real version and pinning it is the default.
const ALPINE_BRANCH_DEFAULT: &str = "v3.24";

/// Packages whose install scriptlets `alpine_deps` reproduces by hand
/// after `--no-scripts`, keyed by package name.  Anything else with an
/// unrun scriptlet is reported to the user instead.
const ALPINE_SCRIPTS_REPLICATED: [&str; 3] = ["busybox", "alpine-baselayout", "openrc"];

/// Provision and populate an Alpine rootfs in /target with apk.
///
/// The one provider that fills a foreign-arch root with no emulation at
/// all: apk is a host-arch binary that only reads and writes files, and
/// `--no-scripts` stops it exec'ing the packages' own shell scriptlets --
/// upstream documents that flag for exactly this ("useful for extracting
/// a system image for different architecture on alternative ROOT").
/// Contrast the debian provider, whose debootstrap second stage runs
/// target binaries in a chroot and so needs qemu-user binfmt on the host.
///
/// Signatures are checked.  `defaults/alpine-keys/<arch>/` is copied into
/// the root before the first `apk add`, so apk has the target arch's
/// signing keys and `--allow-untrusted` is never needed.
///
/// What `--no-scripts` costs is paid back below where it is structural
/// (busybox's applet symlinks are the whole userland, `/sbin/init`
/// included) and reported where it is not.
fn alpine_deps(
    runner: &SandboxRunner,
    target: &Target,
    board: &BoardConfig,
    boards_root: &Utf8Path,
) -> Result<()> {
    let done = target.dir.join(".apk-done");
    if done.exists() {
        tracing::info!("Alpine rootfs already provisioned, skipping apk.");
        return Ok(());
    }
    let arch = crate::provider::alpine_arch(&board.arch).ok_or_else(|| {
        crate::error::Error::BoardConfigParse {
            file: board.name.clone(),
            msg: format!("no Alpine port for arch '{}'", board.arch),
        }
    })?;
    let branch = board
        .alpine_branch
        .as_deref()
        .unwrap_or(ALPINE_BRANCH_DEFAULT);
    if matches!(branch, "edge" | "latest-stable") {
        tracing::warn!(
            "ALPINE_BRANCH '{branch}' is a moving target; pin a release \
             (e.g. {ALPINE_BRANCH_DEFAULT}) for reproducible builds"
        );
    }
    if !crate::provider::alpine_branch_serves(branch, arch) {
        return Err(crate::error::Error::BoardConfigParse {
            file: board.name.clone(),
            msg: format!(
                "Alpine {branch} has no {arch} repository -- riscv64 is a \
                 supported architecture from Alpine 3.20 on"
            ),
        });
    }
    let mirror = board
        .alpine_mirror
        .as_deref()
        .unwrap_or("https://dl-cdn.alpinelinux.org/alpine");
    let repos = board.alpine_repos.as_deref().unwrap_or("main community");
    let extra = read_distro_packages(&boards_root.join(&board.name).join("alpine-packages.txt"))?;

    // apk-tools is not in ::gentoo (checked: no apk, no apk-tools, in any
    // category), so the crossdev-stages overlay carries the ebuild.  An
    // ebuild rather than a pinned binary: portage checks the release
    // tarball against the Manifest, records the build in the sandbox VDB
    // and caches a binpkg, so nothing here is a blob nothing accounts for.
    let portage = Portage::new(runner);
    portage.emerge(&["--noreplace", "app-arch/apk-tools"])?;

    // A failed run leaves a half-unpacked tree; wipe everything but the
    // workspace markers so a retry starts from nothing.
    runner.run(
        "find /target -mindepth 1 -maxdepth 1 \
         ! -name '.arch' ! -name '.stage3' ! -name '.provider' -exec rm -rf {} +",
    )?;

    // Keys first: apk reads them from <root>/etc/apk/keys, so they have to
    // be in place before the first package is verified.  Per-arch, because
    // Alpine signs each architecture with its own builder key.
    runner.run(&format!(
        "set -e\n\
         mkdir -p /target/etc/apk/keys\n\
         cp /scripts/defaults/alpine-keys/{arch}/*.rsa.pub /target/etc/apk/keys/\n\
         : > /target/etc/apk/repositories\n\
         for r in {repos}; do echo '{mirror}/{branch}/'\"$r\" \
             >> /target/etc/apk/repositories; done"
    ))?;

    // --initdb also records the arch in /target/etc/apk/arch, so apk on the
    // booted board keeps installing for the right architecture.
    let add = std::iter::once("alpine-base".to_string())
        .chain(extra)
        .collect::<Vec<_>>()
        .join(" ");
    runner.run(&format!(
        "apk --arch {arch} --root /target --initdb --no-scripts add {add}"
    ))?;

    // busybox ships one binary and a list of the paths its applets answer
    // to; the post-install scriptlet turns that list into symlinks.  Without
    // them the root has no /bin/ls and no /sbin/init.  The list is a plain
    // file (/etc/busybox-paths.d/<binary>), so recreate them here -- same
    // rule as `busybox --install -s`, which never overwrites an existing
    // path.  -L as well as -e, because a link to a target that only
    // resolves once the image is the root reads as absent to -e.
    // bbsuid then claims its eight applets, as its own --install does.
    runner.run(
        "set -e\n\
         for list in /target/etc/busybox-paths.d/*; do\n\
         \x20   [ -f \"$list\" ] || continue\n\
         \x20   bin=/bin/${list##*/}\n\
         \x20   [ -e \"/target$bin\" ] || continue\n\
         \x20   while read -r p; do\n\
         \x20       [ -n \"$p\" ] || continue\n\
         \x20       mkdir -p \"/target/${p%/*}\"\n\
         \x20       [ -e \"/target/$p\" ] || [ -L \"/target/$p\" ] || \\\n\
         \x20           ln -s \"$bin\" \"/target/$p\"\n\
         \x20   done < \"$list\"\n\
         done\n\
         if [ -e /target/bin/bbsuid ]; then\n\
         \x20   for a in bin/mount bin/umount bin/su usr/bin/crontab usr/bin/passwd \\\n\
         \x20            usr/bin/traceroute usr/bin/traceroute6 usr/bin/vlock; do\n\
         \x20       ln -sf /bin/bbsuid \"/target/$a\"\n\
         \x20   done\n\
         fi",
    )?;

    // alpine-baselayout's post-install, the part that matters: /etc/group is
    // written after /etc/shadow, so the shadow group (gid 42) only gets
    // applied afterwards.
    runner.run("[ -f /target/etc/shadow ] && chgrp 42 /target/etc/shadow; :")?;

    report_unrun_alpine_scripts(runner);

    // No /dev cleanup: apk never mknods here (with --no-scripts it does not
    // reach its device-node setup at all) and alpine-base carries no device
    // nodes, so /target/dev is already just the pts and shm mountpoints.
    std::fs::write(done, chrono::Utc::now().to_rfc3339())?;
    Ok(())
}

/// apk stores every package's scriptlets in the root even when it is told
/// not to run them, so the image can say exactly which ones did not run.
/// Advisory: the three whose effects `alpine_deps` reproduces are dropped,
/// and what is left is a board's to handle from post-assemble.sh.
fn report_unrun_alpine_scripts(runner: &SandboxRunner) {
    let Ok(listing) = runner.run_output(
        "tar tzf /target/lib/apk/db/scripts.tar.gz 2>/dev/null \
         | grep -E '\\.(pre|post)-install$' \
         | sed -E 's/\\.X1[0-9a-f]+\\.(pre|post)-install$//' | sort -u",
    ) else {
        return;
    };
    let mut rest: Vec<&str> = listing
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        // "alpine-baselayout-3.7.2-r1" -> "alpine-baselayout": strip the
        // -r<rel> and the version, both of which move independently.
        .filter(|s| {
            let name = s.rsplitn(3, '-').nth(2).unwrap_or(s);
            !ALPINE_SCRIPTS_REPLICATED.contains(&name)
        })
        .collect();
    rest.sort_unstable();
    if !rest.is_empty() {
        tracing::warn!(
            "apk ran with --no-scripts (no emulation); install scriptlets \
             did not run for: {}",
            rest.join(" ")
        );
    }
}

/// Where the Fedora composes are published.  One host, two trees: see
/// `FedoraImage::url`.
const FEDORA_MIRROR_DEFAULT: &str = "https://dl.fedoraproject.org/pub";

/// Provision a Fedora rootfs in /target from a published container base
/// image.
///
/// Seeding needs no emulation.  Fedora composes one OCI archive per
/// architecture per release, so this part is a checksummed download and
/// two `tar` calls: nothing target-arch is executed, nothing is
/// resolved, and no scriptlet is skipped, because none ever runs.
///
/// The archive is an OCI image layout, not a flat rootfs tarball.  The
/// outer `.tar.xz` unpacks to `index.json` plus `blobs/sha256/*`, and
/// the tree is the single layer blob inside.  Fedora's base images have
/// exactly one layer, which is why there is no whiteout handling below;
/// a second layer is refused rather than merged wrong.
///
/// Board extras come from `fedora-packages.txt` and are installed by
/// `fedora_install` below, on top of the image's own rpmdb, the same
/// shape as stage3 + cross-emerge on the gentoo side.
fn fedora_deps(
    runner: &SandboxRunner,
    target: &Target,
    board: &BoardConfig,
    boards_root: &Utf8Path,
) -> Result<()> {
    let done = target.dir.join(".fedora-done");
    if done.exists() {
        tracing::info!("Fedora rootfs already provisioned, skipping unpack.");
        return Ok(());
    }
    let arch = crate::provider::fedora_arch(&board.arch).ok_or_else(|| {
        crate::error::Error::BoardConfigParse {
            file: board.name.clone(),
            msg: format!(
                "no Fedora port for arch '{}': fedora serves aarch64, riscv64 \
                 and x86_64.  ARMv7 was retired in Fedora 37 and 32-bit x86 \
                 has had no installable tree since 30",
                board.arch
            ),
        }
    })?;
    let release = board
        .fedora_release
        .as_deref()
        .unwrap_or(crate::provider::FEDORA_RELEASE_DEFAULT);
    let image = crate::provider::fedora_image(release, arch).ok_or_else(|| {
        crate::error::Error::BoardConfigParse {
            file: board.name.clone(),
            msg: format!(
                "no pinned Fedora image for release {release} on {arch}; \
                 pinned: {}",
                crate::provider::fedora_pinned()
            ),
        }
    })?;
    let extra = read_distro_packages(&boards_root.join(&board.name).join("fedora-packages.txt"))?;
    let mirror = board
        .fedora_mirror
        .as_deref()
        .unwrap_or(FEDORA_MIRROR_DEFAULT);

    // A failed run leaves a half-unpacked tree; wipe everything but the
    // workspace markers so a retry starts from nothing.
    runner.run(
        "find /target -mindepth 1 -maxdepth 1 \
         ! -name '.arch' ! -name '.stage3' ! -name '.provider' -exec rm -rf {} +",
    )?;

    // Pinned URL, pinned sha256, cached in the workspace so the second
    // build is offline.  For riscv64 that checksum is the whole trust
    // anchor, because Fedora signs the SIG composes with nothing; for
    // aarch64 and x86_64 the same value is also in a clearsigned
    // CHECKSUM file next to the image.
    runner.run(&format!(
        "set -e\n\
         mkdir -p /cache/sources\n\
         img=/cache/sources/{name}\n\
         if ! echo '{sha}  '\"$img\" | sha256sum -c --status 2>/dev/null; then\n\
         \x20   wget -O \"$img.tmp\" '{url}'\n\
         \x20   echo '{sha}  '\"$img.tmp\" | sha256sum -c\n\
         \x20   mv -f \"$img.tmp\" \"$img\"\n\
         fi\n\
         rm -rf /build/fedora-oci\n\
         mkdir -p /build/fedora-oci\n\
         tar -xJf \"$img\" -C /build/fedora-oci",
        name = image.file_name(),
        sha = image.sha256,
        url = image.url(mirror),
    ))?;

    // index.json -> manifest -> config, so the architecture is read out
    // of the image rather than trusted from the file name, and ->
    // layers, whose one blob is the rootfs.
    runner.run(&format!(
        "python3 - > /build/fedora-layer <<'PY'\n\
         import json, os, sys\n\
         d = '/build/fedora-oci'\n\
         def blob(digest):\n\
         \x20   return os.path.join(d, 'blobs', *digest.split(':'))\n\
         idx = json.load(open(os.path.join(d, 'index.json')))\n\
         man = json.load(open(blob(idx['manifests'][0]['digest'])))\n\
         cfg = json.load(open(blob(man['config']['digest'])))\n\
         if cfg.get('architecture') != '{arch}':\n\
         \x20   sys.exit('image is ' + str(cfg.get('architecture')) + ', not {arch}')\n\
         if len(man['layers']) != 1:\n\
         \x20   sys.exit('expected a single-layer image, got ' + str(len(man['layers'])) +\n\
         \x20            '; merging whiteouts is not implemented')\n\
         print(blob(man['layers'][0]['digest']))\n\
         PY"
    ))?;

    // Same flags as the stage3 unpack: ownership and capability xattrs
    // are part of the rootfs, since Fedora ships ping and friends with
    // security.capability instead of a suid bit.
    // The unpacked OCI layout is 70-odd MB of scratch and the image
    // itself is already cached under /cache/sources; drop it once the
    // tree is out rather than leaving it in the build dir.
    runner.run(
        "set -e\n\
         layer=$(cat /build/fedora-layer)\n\
         tar --overwrite -xpf \"$layer\" \
           --xattrs-include='*.*' --numeric-owner -C /target\n\
         rm -rf /build/fedora-oci /build/fedora-layer",
    )?;

    if !extra.is_empty() {
        fedora_install(runner, arch, release, &extra)?;
    }

    report_fedora_missing_init(runner);

    // No /dev cleanup: the layer ships /dev as an empty directory,
    // which is exactly the mountpoint devtmpfs needs at boot.
    std::fs::write(done, chrono::Utc::now().to_rfc3339())?;
    Ok(())
}

/// Install a board's `fedora-packages.txt` into the unpacked root.
///
/// dnf5 is not in ::gentoo, and neither are two of its libraries, so
/// the crossdev-stages overlay carries `sys-apps/dnf5`, `dev-libs/libsolv` and
/// `dev-libs/librepo`.  Those three are the whole gap: everything else
/// dnf5 wants (app-arch/rpm, dev-cpp/sdbus-c++, sys-libs/libmodulemd,
/// app-arch/zchunk, dev-cpp/toml11, dev-libs/libfmt, json-c, glib) is
/// already in the tree.
///
/// No repository configuration is written here, and none is needed.
/// The container base image ships Fedora's own /etc/yum.repos.d, dnf5
/// reads its configuration from the installroot rather than the host,
/// and what is there is already per-arch correct: the riscv64 image
/// enables `[fedora-riscv]`, the SIG's koji dist-repo, and disables the
/// primary metalink; the primary images do the opposite.
///
/// `--nogpgcheck` is not passed either.  Fedora's own config decides,
/// the release key is already in the image's rpmdb as a `gpg-pubkey`,
/// and on aarch64 and x86_64 signatures are therefore checked.
///
/// riscv64 is the exception, and it is Fedora's exception rather than
/// one invented here.  The SIG's packages carry no OpenPGP signature at
/// all, its repo file says `gpgcheck=0`, and Fedora's own rpm ships
/// `%_pkgverify_level digest` in /usr/lib/rpm/macros.  ::gentoo's rpm
/// keeps upstream's stricter `all`, so without matching Fedora's value
/// the transaction aborts with "does not verify: no signature" for
/// every package.  So for riscv64, and only riscv64, that same value is
/// set for the length of the transaction, with the warning above naming
/// what it costs.  `digest` still checks the header and payload SHA256,
/// so corruption is still caught; what cannot be checked is authorship,
/// because Fedora published none.
///
/// This is also the step that needs qemu-user binfmt with the F flag,
/// for the same reason the debian provider does: rpm runs each
/// package's scriptlets inside the installroot, and glibc's own file
/// trigger execs ldconfig there.  Unpacking the image needs none of it.
fn fedora_install(
    runner: &SandboxRunner,
    arch: &str,
    release: &str,
    packages: &[String],
) -> Result<()> {
    if arch == "riscv64" {
        tracing::warn!(
            "Fedora's riscv64 repository is unsigned: its own \
             fedora-riscv.repo sets gpgcheck=0, no \
             RPM-GPG-KEY-fedora-{release}-riscv64 exists, and the RPMs carry \
             no OpenPGP signature.  Packages installed here are trusted on \
             TLS alone.  Its `latest` is also a moving koji symlink, so this \
             step is not reproducible the way the pinned base image is"
        );
    }

    let portage = Portage::new(runner);
    portage.emerge(&["--noreplace", "sys-apps/dnf5"])?;

    let forcearch = if arch == std::env::consts::ARCH {
        String::new()
    } else {
        format!("--forcearch={arch} ")
    };
    // Only for the architecture Fedora publishes unsigned, only while the
    // transaction runs, and removed on the way out either way: the
    // sandbox outlives this step and must not keep a relaxed rpm.
    let pkgverify = if arch == "riscv64" {
        "echo '%_pkgverify_level digest' > /etc/rpm/macros.crossdev-stages-fedora\n\
         trap 'rm -f /etc/rpm/macros.crossdev-stages-fedora' EXIT\n"
    } else {
        ""
    };

    // rpm chroots into the installroot to run the scriptlets and they
    // read /proc, so the kernel filesystems have to be visible where the
    // chroot looks for them.  Mounts and transaction go in one run: each
    // runner.run is its own container with its own mount namespace, so
    // they vanish when it exits and `assemble` never sees a live bind
    // under /target.
    runner
        .run(&format!(
            "set -e\n\
             mkdir -p /etc/rpm\n\
             {pkgverify}\
             for d in proc sys dev; do mkdir -p /target/$d; \
                 mount --bind /$d /target/$d; done\n\
             dnf5 --installroot=/target --releasever={release} {forcearch}\
                 --assumeyes install {packages}\n",
            packages = packages.join(" "),
        ))
        .inspect_err(|_| {
            tracing::error!(
                "dnf5 install failed.  Installing a foreign-arch root runs \
                 the packages' rpm scriptlets under the target architecture, \
                 so the host needs qemu-user binfmt registered with the F \
                 flag, the same requirement the debian provider has.  On \
                 Debian: apt install qemu-user-binfmt"
            );
        })?;
    Ok(())
}

/// The container base image is a userland, not an operating system.
///
/// Fedora builds it from a KIWI profile that ignores `kernel` and takes
/// `systemd-standalone-sysusers` instead of `systemd`, so the tree has
/// systemd's shared libraries and no PID 1; the OCI config's `Cmd` is
/// `/bin/bash`.  There is a systemd-carrying profile in the same file
/// (`Container-Base-Generic-Init`) but Fedora does not publish it, and
/// the artifacts that do boot are Cloud qcow2 and Server raw disk
/// images, neither of which is a tarball.  So say it here, where it is
/// cheap to fix, instead of letting the board find out at the panic.
fn report_fedora_missing_init(runner: &SandboxRunner) {
    let has_init = runner
        .run_output("[ -e /target/usr/lib/systemd/systemd ] && echo yes || echo no")
        .map(|s| s.trim() == "yes")
        .unwrap_or(false);
    if has_init {
        return;
    }
    tracing::warn!(
        "the Fedora container base image carries no init: no \
         /usr/lib/systemd/systemd, no /sbin/init.  The rootfs is otherwise \
         complete (bash, coreutils, glibc, rpm, dnf5) but will not boot \
         until a board hook puts an init in place"
    );
}

/// Upstream buildroot git.  A board overrides it with BUILDROOT_REPO,
/// which is how a vendor fork gets used.
const BUILDROOT_REPO: &str = "https://gitlab.com/buildroot.org/buildroot.git";

/// Buildroot's source tree and its out-of-tree output, both inside the
/// build dir so they are per-build and go when the build does.
const BUILDROOT_SRC: &str = "/build/buildroot";
const BUILDROOT_OUT: &str = "/build/buildroot-out";

/// Run buildroot and unpack the rootfs it produced into `/target`.
///
/// `/target` is the seam, not `gen/root`: the whole point of the
/// provider split is that everything after `deps` is provider-agnostic,
/// and `assemble` already copies `/target` into `gen/root`, strips the
/// workspace markers and recreates the mount points.  Handing buildroot
/// its own path into `gen/root` would buy nothing and cost `chroot
/// --board`, the target-provider guard, and the ABI and ISA checks,
/// which all read the target or the tree assemble builds from it.
///
/// Buildroot's kernel, DTBs and bootloader stay where buildroot put them
/// (`BUILDROOT_OUT/images/`); [`buildroot_boot_artifacts`] installs the
/// ones the board named.
fn buildroot_deps(runner: &SandboxRunner, target: &Target, board: &BoardConfig) -> Result<()> {
    let done = target.dir.join(".buildroot-done");
    if done.exists() {
        tracing::info!("Buildroot rootfs already provisioned, skipping.");
        return Ok(());
    }
    let defconfig = board.buildroot_defconfig.as_deref().ok_or_else(|| {
        crate::error::Error::BoardConfigParse {
            file: board.name.clone(),
            msg: "the buildroot provider needs BUILDROOT_DEFCONFIG".into(),
        }
    })?;
    // A defconfig name is a make target and a make variable value; a
    // shell metacharacter in it would run as one.
    if defconfig.contains(|c: char| c.is_whitespace() || "$`'\"\\;&|<>()".contains(c)) {
        return Err(crate::error::Error::BoardConfigParse {
            file: board.name.clone(),
            msg: format!("BUILDROOT_DEFCONFIG '{defconfig}' is not a plain file name"),
        });
    }

    // Everything buildroot's support/dependencies/dependencies.sh calls
    // mandatory that this project's own defaults do not already install
    // (bc and git are in defaults/sandbox-packages.txt).  All of these
    // are in a stage3 today, most of them as somebody else's dependency
    // -- --noreplace makes saying so cost nothing and stops it being an
    // assumption.
    let portage = Portage::new(runner);
    portage.emerge(&[
        "--noreplace",
        "app-arch/cpio",
        "app-arch/unzip",
        "net-misc/rsync",
        "net-misc/wget",
        "sys-apps/which",
    ])?;

    // A buildroot build that fails leaves its clone behind and does not
    // mark `deps` done, so the retry comes back through here and
    // cached_clone would fall over on the existing directory.  The other
    // clone sites never see this: they run in `checkout`, which has
    // nothing after it that can fail.
    let cloned = runner.run_output(&format!(
        "[ -d {BUILDROOT_SRC}/.git ] && echo yes || echo no"
    ))? == "yes";
    if !cloned {
        let repo = board.buildroot_repo.as_deref().unwrap_or(BUILDROOT_REPO);
        let tag = board.buildroot_tag.as_deref().unwrap_or("master");
        crate::source_cache::cached_clone(runner, repo, tag, BUILDROOT_SRC, "buildroot")?;
    }

    runner.run(&buildroot_build_script(&board.name, defconfig))?;

    // Same reason debian_deps clears the tree: a half-unpacked rootfs
    // from a failed run would collide with this one.
    runner.run(
        "find /target -mindepth 1 -maxdepth 1 \
         ! -name '.arch' ! -name '.stage3' ! -name '.provider' -exec rm -rf {} +",
    )?;
    // ./dev is excluded for the reason the stage3 unpack excludes it:
    // the tar carries real device nodes, mknod is denied in a user
    // namespace, and devtmpfs mounts over the directory at boot anyway.
    // assemble recreates the empty mount points.
    runner.run(&format!(
        "tar -xpf {out}/images/rootfs.tar --numeric-owner --exclude='./dev' -C /target",
        out = BUILDROOT_OUT,
    ))?;
    std::fs::write(done, chrono::Utc::now().to_rfc3339())?;
    Ok(())
}

/// Shell that configures and runs buildroot out of tree.
///
/// The defconfig is looked up board file first, then buildroot's own
/// `configs/` -- the same order `KERNEL_CONFIG_FRAGMENTS` uses.  The
/// lookup happens in the shell rather than on the host so there is one
/// answer instead of a host-side guess and a sandbox-side one.
///
/// `BR2_DL_DIR` points into the workspace cache.  Buildroot fetches
/// every package tarball itself, and without this each build starts from
/// an empty download dir and refetches the lot.
///
/// No `-j`, which every other build step in this project passes:
/// buildroot does not support a parallel top-level make and says so.
/// `BR2_JLEVEL` in the defconfig (default: nproc) is where a board asks
/// for parallelism.
fn buildroot_build_script(board_name: &str, defconfig: &str) -> String {
    format!(
        "set -e\n\
         cd {src}\n\
         mkdir -p /cache/buildroot-dl\n\
         export BR2_DL_DIR=/cache/buildroot-dl\n\
         board_defconfig=/scripts/boards/{board_name}/{defconfig}\n\
         if [ -f \"$board_defconfig\" ]; then\n\
             echo \"defconfig: $board_defconfig\"\n\
             make O={out} BR2_DEFCONFIG=\"$board_defconfig\" defconfig\n\
         elif [ -f configs/{defconfig} ]; then\n\
             echo \"defconfig: buildroot configs/{defconfig}\"\n\
             make O={out} {defconfig}\n\
         else\n\
             echo \"no BUILDROOT_DEFCONFIG {defconfig}: not in \
                   boards/{board_name}/ and not in buildroot's configs/\" >&2\n\
             exit 1\n\
         fi\n\
         {tar_rootfs}\
         make O={out}",
        src = BUILDROOT_SRC,
        out = BUILDROOT_OUT,
        tar_rootfs = buildroot_force_tar_rootfs(),
    )
}

/// Shell that makes buildroot emit `images/rootfs.tar` whatever else the
/// defconfig asked for.
///
/// A tar is the one output shape that unpacks into `/target`, and almost
/// no stock defconfig sets it -- `qemu_riscv64_virt_defconfig` asks for
/// ext2 and nothing else.  Appending the symbol and re-running
/// olddefconfig is what the kernel step already does with its config
/// fragments, and so is checking afterwards that the symbol survived:
/// olddefconfig drops a symbol whose dependencies are unmet without a
/// word, and the failure would otherwise surface as a missing file after
/// an hour of building.
fn buildroot_force_tar_rootfs() -> String {
    format!(
        "echo 'BR2_TARGET_ROOTFS_TAR=y' >> {out}/.config\n\
         make O={out} olddefconfig\n\
         grep -qx 'BR2_TARGET_ROOTFS_TAR=y' {out}/.config || {{\n\
             echo 'buildroot refused BR2_TARGET_ROOTFS_TAR; \
                   this defconfig cannot emit a rootfs tar' >&2\n\
             exit 1\n\
         }}\n",
        out = BUILDROOT_OUT,
    )
}

fn default_checkout(
    runner: &SandboxRunner,
    board: &BoardConfig,
    boards_root: &Utf8Path,
) -> Result<()> {
    crate::bootloader::clone_pipeline(runner, board)?;
    if let Some(repo) = &board.firmware_repo {
        let tag = board.effective_firmware_tag(); // FIRMWARE_TAG → U_BOOT_TAG → "main"
        crate::source_cache::cached_clone(runner, repo, &tag, "/build/firmware", "firmware")?;
    }
    crate::source_cache::cached_clone(
        runner,
        &board.kernel_repo,
        &board.kernel_tag,
        "/build/linux",
        &format!("linux-{}", board.name),
    )?;
    apply_board_patches(runner, board, boards_root)
}

/// Apply `boards/<board>/patches/<source>/*.patch` to `/build/<source>`, in
/// filename order.  `<source>` is a checkout directory name (linux, u-boot,
/// opensbi, tfa, firmware), so a board declares which tree a patch belongs to
/// by where it puts the file, and no board script is involved.
///
/// Patches for Gentoo packages do not belong here: those go under
/// `portage-patches/` and are applied by portage's own `eapply_user`.
fn apply_board_patches(
    runner: &SandboxRunner,
    board: &BoardConfig,
    boards_root: &Utf8Path,
) -> Result<()> {
    let patches = boards_root.join(&board.name).join("patches");
    if !patches.is_dir() {
        return Ok(());
    }
    let mut sources: Vec<String> = std::fs::read_dir(&patches)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    sources.sort();

    for source in sources {
        let dir = patches.join(&source);
        let mut files: Vec<String> = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.ends_with(".patch") || n.ends_with(".diff"))
            .collect();
        files.sort();
        if files.is_empty() {
            continue;
        }
        for file in files {
            tracing::info!("Applying {source}/{file}…");
            // Three states, not two.  `patch --forward` exits 1 both for a
            // patch that is already applied and for one that does not apply at
            // all, so it cannot make a rerun safe -- it only makes a real
            // conflict look like one.  git tells them apart: --check says it
            // would apply, --reverse --check says it is already in, and
            // neither means the tree has moved and the build must stop.
            runner.run(&format!(
                "set -e\n\
                 cd /build/{source}\n\
                 p=/scripts/boards/{board_name}/patches/{source}/{file}\n\
                 if git apply --check \"$p\" 2>/dev/null; then\n\
                     git apply \"$p\"\n\
                 elif git apply --reverse --check \"$p\" 2>/dev/null; then\n\
                     echo \"already applied: {file}\"\n\
                 else\n\
                     echo \"does not apply to this tree: {file}\" >&2\n\
                     exit 1\n\
                 fi",
                board_name = board.name,
            ))?;
        }
    }
    Ok(())
}

fn default_bootloader(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    crate::bootloader::build_pipeline(runner, board)
}

fn default_kernel(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    let karch =
        board
            .kernel_arch
            .as_deref()
            .ok_or_else(|| crate::error::Error::BoardConfigParse {
                file: board.name.clone(),
                msg: "KERNEL_ARCH required for kernel build".into(),
            })?;
    // Without these the kernel embeds the wall clock, this machine's hostname
    // and whoever ran the build, so the same source and config produce a
    // different image every time.  The date comes from the commit the tree is
    // checked out at, which is the only timestamp that is a property of the
    // input rather than of the run.
    runner.run(&format!(
        "set -e\n\
         cd /build/linux\n\
         epoch=$(git log -1 --pretty=%ct 2>/dev/null || echo 0)\n\
         export SOURCE_DATE_EPOCH=$epoch\n\
         export KBUILD_BUILD_TIMESTAMP=$(date -u -d \"@$epoch\" 2>/dev/null || echo)\n\
         export KBUILD_BUILD_USER=crossdev-stages\n\
         export KBUILD_BUILD_HOST={board_name}\n\
         make -C /build/linux ARCH={karch} CROSS_COMPILE={cc} {defconfig}\n\
         {fragments}\
         make -C /build/linux ARCH={karch} CROSS_COMPILE={cc} WERROR=0 -j$(nproc)",
        cc = board.cross_compile,
        defconfig = board.kernel_defconfig,
        board_name = board.name,
        fragments = kernel_config_fragments(board),
    ))
}

/// Shell that appends each fragment to .config, reruns olddefconfig, and then
/// checks that every line the fragments asked for actually survived.
///
/// The check is the point.  olddefconfig drops a symbol whose dependencies are
/// unmet and can hand back a module where a builtin was asked for, both
/// silently, and a board that says `# CONFIG_X is not set` has no way to find
/// out that X came back on.  Checking the fragments themselves means the whole
/// request is covered rather than whichever symbols someone remembered to list.
fn kernel_config_fragments(board: &BoardConfig) -> String {
    if board.kernel_config_fragments.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for name in &board.kernel_config_fragments {
        out.push_str(&format!(
            "f=/scripts/boards/{board}/kernel-config/{name}\n\
             [ -f \"$f\" ] || f=/scripts/defaults/kernel-config/{name}\n\
             [ -f \"$f\" ] || {{ echo \"no kernel config fragment {name}\" >&2; exit 1; }}\n\
             cat \"$f\" >> /build/linux/.config\n\
             frags=\"$frags $f\"\n",
            board = board.name,
        ));
    }
    format!(
        "frags=\n\
         {out}\
         make -C /build/linux ARCH=$ARCH CROSS_COMPILE=$CROSS_COMPILE olddefconfig\n\
         for f in $frags; do\n\
             while read -r line; do\n\
                 case \"$line\" in\n\
                     '#'*' is not set')\n\
                         sym=${{line#\\# }}; sym=${{sym%% *}}\n\
                         if grep -q \"^$sym=\" /build/linux/.config; then\n\
                             echo \"$sym came back on, fragment asked for it off\" >&2\n\
                             exit 1\n\
                         fi ;;\n\
                     CONFIG_*=*)\n\
                         grep -qx \"$line\" /build/linux/.config || {{\n\
                             echo \"kernel config lost: $line\" >&2\n\
                             exit 1\n\
                         }} ;;\n\
                 esac\n\
             done < \"$f\"\n\
         done\n"
    )
}

/// The device tree an extlinux entry should load.
///
/// `BOARD_DTB_GLOB` already names what `assemble` copies into /boot, so a
/// board that copies exactly one file has already said which DTB it boots and
/// repeating the name in `BOOT_DTB_NAME` would only give it somewhere to
/// drift.  A board that copies a whole directory has to name one.
fn extlinux_fdt(board: &BoardConfig) -> Result<Option<String>> {
    if let Some(name) = &board.dtb_name {
        return Ok(Some(name.clone()));
    }
    let Some(glob) = &board.kernel_dtb_glob else {
        return Ok(None);
    };
    if glob.contains(['*', '?', '[']) {
        return Err(Error::BoardConfigParse {
            file: board.name.clone(),
            msg: "BOARD_DTB_GLOB matches more than one file, \
                  so BOOT_EXTLINUX needs BOOT_DTB_NAME to say which one boots"
                .into(),
        });
    }
    Ok(Some(
        glob.rsplit('/').next().unwrap_or(glob.as_str()).to_string(),
    ))
}

/// Shell that writes the board's extlinux.conf.
///
/// Every board that boots this way writes the same file with three values
/// changed, so the file is built here and the board states only the values.
/// The heredoc is unquoted and carries the disk identifiers in front of it for
/// the same reason a board hook does: `BOOT_ROOT_DEV` is written as
/// `PARTUUID=${BOOT_PART_UUID_2}` and only means anything once expanded.
fn extlinux_conf(board: &BoardConfig) -> Result<String> {
    let missing = |key: &str| Error::BoardConfigParse {
        file: board.name.clone(),
        msg: format!("BOOT_EXTLINUX needs {key}"),
    };
    let kernel = board
        .kernel_name
        .as_deref()
        .ok_or(missing("BOOT_KERNEL_NAME"))?;
    let root = board.root_dev.as_deref().ok_or(missing("BOOT_ROOT_DEV"))?;

    let mut append = format!("root={root} rw rootwait rootfstype=ext4");
    if let Some(console) = &board.console {
        append.push_str(&format!(" console={console}"));
    }
    if let Some(extra) = &board.append {
        append.push(' ');
        append.push_str(extra);
    }

    let fdt = match extlinux_fdt(board)? {
        Some(name) => format!("    FDT /{name}\n"),
        None => String::new(),
    };

    Ok(format!(
        "{exports}mkdir -p /build/gen/boot/extlinux\n\
         cat > /build/gen/boot/extlinux/extlinux.conf <<EOF\n\
         DEFAULT linux\n\
         TIMEOUT 30\n\
         LABEL linux\n\
         \x20   MENU LABEL {label}\n\
         \x20   LINUX /{kernel}\n\
         {fdt}\x20   APPEND {append}\n\
         EOF\n",
        exports = DiskId::of(board).exports(),
        // The rootfs is not always Gentoo, and the boot menu is the first
        // thing anyone reads off a serial console.
        label = board.description.as_deref().unwrap_or(&board.name),
    ))
}

fn default_assemble(
    runner: &SandboxRunner,
    board: &BoardConfig,
    build: &Build,
    ws: &Workspace,
    kernel_built: bool,
) -> Result<()> {
    // Start from nothing.  `cp -a /target/.` merges, so anything an earlier
    // build of this same tree put here outlives being taken back out -- a
    // service dropped from the board's list, a package.mask deleted from the
    // target, a firmware directory renamed.  The whole tree is rebuilt from
    // /target and this step's own work, so there is nothing here worth
    // keeping, and the copy that follows costs the same either way.
    crate::container::destroy_dir(&build.dir.join("gen"), ws.base())?;

    runner.run("mkdir -p /build/gen/root /build/gen/boot")?;
    runner.run("cp -a /target/. /build/gen/root/")?;
    // Workspace bookkeeping markers must not ship in the image.
    runner.run(
        "rm -f /build/gen/root/.arch /build/gen/root/.stage3 \
         /build/gen/root/.provider /build/gen/root/.stage1 \
         /build/gen/root/.updated /build/gen/root/.debootstrap-done \
         /build/gen/root/.apk-done /build/gen/root/.fedora-done \
         /build/gen/root/.buildroot-done /build/gen/root/.openwrt-done",
    )?;
    // unpack_tarball excludes ./dev to avoid permission errors in rootless containers.
    // Recreate the empty mount-point directories so the kernel can mount devtmpfs,
    // procfs, sysfs and tmpfs at boot.
    runner.run(
        "mkdir -p /build/gen/root/{dev,proc,sys,run,tmp,mnt,media} && \
         chmod 1777 /build/gen/root/tmp",
    )?;

    // Rootfs-only pipelines (no kernel step, no /build/linux) skip the
    // kernel install; a kernel built in an earlier `--steps kernel`
    // invocation of the same build dir still installs.
    if kernel_built {
        let karch =
            board
                .kernel_arch
                .as_deref()
                .ok_or_else(|| crate::error::Error::BoardConfigParse {
                    file: board.name.clone(),
                    msg: "KERNEL_ARCH required for assemble".into(),
                })?;

        runner.run(&format!(
            "make -C /build/linux ARCH={karch} CROSS_COMPILE={cc} \
             INSTALL_MOD_PATH=/build/gen/root modules_install",
            cc = board.cross_compile,
        ))?;

        if let Some(dtb_glob) = &board.kernel_dtb_glob {
            runner.run(&format!("cp /build/linux/{dtb_glob} /build/gen/boot/"))?;
        }

        if let Some(kname) = &board.kernel_name {
            runner.run(&format!(
                "cp /build/linux/arch/{karch}/boot/{kname} /build/gen/boot/"
            ))?;
        }
    } else if board.rootfs_provider == RootfsProvider::Buildroot {
        buildroot_boot_artifacts(runner, board)?;
    }

    // Firmware, in the two shapes a board actually has.  Every board carrying
    // any firmware at all used to write these same few lines into its own
    // post-assemble hook.
    //
    // Both forms fail if the directory is not there.  The version that lived in
    // the hooks copied host paths with `2>/dev/null || true`, which meant a
    // board could name firmware it never got: the copy runs inside the sandbox,
    // whose rootfs has no /lib/firmware at all, so it silently did nothing on
    // every board that used it.
    if let Some(overlay) = &board.firmware_overlay {
        // The overlay path ends in lib/firmware, so its *contents* land in
        // /lib/firmware -- the vendor tree already lays out rtw89/, rtl_bt/ and
        // the rest under the names the drivers request.
        runner.run(&format!(
            "[ -d /build/firmware/{overlay} ] || {{ \
                 echo 'BOARD_FIRMWARE_OVERLAY: no {overlay} in the firmware repo' >&2; exit 1; }}; \
             mkdir -p /build/gen/root/lib/firmware && \
             cp -a /build/firmware/{overlay}/. /build/gen/root/lib/firmware/"
        ))?;
    }
    for dir in &board.firmware_dirs {
        // Path preserved: panthor asks for arm/mali/arch<N>.<M>/mali_csffw.bin
        // and the network drivers are just as particular about their directory.
        runner.run(&format!(
            "[ -d /build/firmware/{dir} ] || {{ \
                 echo 'FIRMWARE_DIRS: no {dir} in the firmware repo' >&2; exit 1; }}; \
             mkdir -p /build/gen/root/lib/firmware/{dir} && \
             cp -a /build/firmware/{dir}/. /build/gen/root/lib/firmware/{dir}/"
        ))?;
    }

    if board.extlinux {
        runner.run(&extlinux_conf(board)?)?;
    }

    match board.rootfs_provider {
        RootfsProvider::Gentoo => os_config_openrc(runner, board)?,
        RootfsProvider::Debian | RootfsProvider::Ubuntu | RootfsProvider::Fedora => {
            os_config_systemd(runner, board)?
        }
        RootfsProvider::Alpine => os_config_alpine(runner, board)?,
        RootfsProvider::Buildroot => os_config_buildroot(board),
        RootfsProvider::OpenWrt => os_config_openwrt(runner, board)?,
        RootfsProvider::None => {}
    }

    if let Some(dracut_modules) = &board.dracut_modules {
        runner.run(&format!(
            "kver=$(ls /build/gen/root/lib/modules/ | head -1) && \
             [ -n \"$kver\" ] && \
             dracutbasedir=/usr/lib/dracut \
             DRACUT_INSTALL=/usr/lib/dracut/dracut-install \
               dracut -f --no-early-microcode --no-kernel \
                 -m '{dracut_modules}' --gzip \
                 --sysroot /build/gen/root \
                 --tmpdir /tmp \
                 /build/gen/boot/initramfs.img \"$kver\""
        ))?;
    }

    runner.run("/usr/local/bin/ldconfig -v -r /build/gen/root")
}

/// Install the kernel and device trees buildroot built into `/boot`.
///
/// The board says which files with the two fields it would use anyway,
/// `BOOT_KERNEL_NAME` and `BOARD_DTB_GLOB`; only the directory they are
/// read from changes, from the kernel tree this project built to the
/// one buildroot filled.  `extlinux.conf` is written from the same two
/// fields either way, so a buildroot board boots through the same path
/// as every other extlinux board.
///
/// Kernel modules need nothing here: buildroot installed them into the
/// rootfs it handed over, which is already in `gen/root`.
fn buildroot_boot_artifacts(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    let images = format!("{BUILDROOT_OUT}/images");
    if let Some(dtb_glob) = &board.kernel_dtb_glob {
        runner.run(&format!("cp {images}/{dtb_glob} /build/gen/boot/"))?;
    }
    if let Some(kname) = &board.kernel_name {
        runner.run(&format!("cp {images}/{kname} /build/gen/boot/"))?;
    }
    Ok(())
}

/// What the buildroot provider does not configure, and why.
///
/// Everything `os_config_openrc` and `os_config_systemd` write is a
/// defconfig symbol in buildroot: `BR2_TARGET_GENERIC_HOSTNAME`,
/// `BR2_TARGET_GENERIC_GETTY_PORT`, `BR2_TARGET_GENERIC_ROOT_PASSWD`,
/// and the init system itself.  Buildroot is the first provider that
/// owns this seam outright, so the provider writes nothing rather than
/// arguing with the defconfig -- but a board that set the keys and got
/// silence would have no way to find that out.
fn os_config_buildroot(board: &BoardConfig) {
    let mut ignored: Vec<&str> = Vec::new();
    if board.serial_tty.is_some() || board.serial_baud.is_some() {
        ignored.push("BOOT_SERIAL_TTY/BOOT_SERIAL_BAUD");
    }
    if !board.services.is_empty() {
        ignored.push("BOOT_SERVICES");
    }
    if !ignored.is_empty() {
        tracing::warn!(
            "{} configure the rootfs, which buildroot owns: set \
             BR2_TARGET_GENERIC_GETTY_* and the init system in the defconfig instead",
            ignored.join(" and ")
        );
    }
}

/// The OpenRC parts Gentoo and Alpine genuinely share: the runlevel
/// directories, the first-boot grow-rootfs oneshot, and BOOT_SERVICES.
/// Both distros use `/etc/init.d/<svc>` and `/etc/runlevels/<level>/<svc>`
/// and neither needs the target arch to run for any of it.
///
/// grow-rootfs itself calls findmnt, sfdisk, partx and resize2fs; on
/// Gentoo those are all in @system, on Alpine they are extra packages
/// (util-linux-misc, e2fsprogs-extra).  It exits with a warning rather
/// than failing the boot when they are missing.
fn os_config_openrc_common(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    runner
        .run("mkdir -p /build/gen/root/etc/runlevels/{boot,default,nonetwork,shutdown,sysinit}")?;

    runner.run(
        "install -m 0755 /scripts/defaults/scripts/grow-rootfs.initd \
           /build/gen/root/etc/init.d/grow-rootfs && \
         ln -sf /etc/init.d/grow-rootfs \
           /build/gen/root/etc/runlevels/boot/grow-rootfs",
    )?;

    for svc in &board.services {
        if let Some((name, runlevel)) = svc.split_once(':') {
            runner.run(&format!(
                "ln -sf /etc/init.d/{name} /build/gen/root/etc/runlevels/{runlevel}/{name}"
            ))?;
        }
    }
    Ok(())
}

/// OpenRC configuration of the assembled rootfs: runlevels, first-boot
/// grow-rootfs service, BOOT_SERVICES symlinks, hostname, serial getty,
/// empty root password, permissive sshd.  Gentoo-provider only; other
/// rootfs providers configure their OS in their own way.
fn os_config_openrc(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    os_config_openrc_common(runner, board)?;

    runner.run(&format!(
        "mkdir -p /build/gen/root/etc/conf.d && \
         printf 'hostname=\"{}\"\n' > /build/gen/root/etc/conf.d/hostname",
        board.hostname
    ))?;

    // baselayout's inittab starts a getty on every port some board somewhere
    // has: `s0`/`s1` on ttyS0/ttyS1, and under "Architecture specific
    // features" a live `f0` on ttyAMA0.  On a board without that port agetty
    // exits at once and init respawns it until it gives up ("INIT: Id \"s0\"
    // respawning too fast", every five minutes, forever).  On a board that
    // does have it, the stock getty and the board's own both open it and both
    // print /etc/issue, so the login banner repeats down the screen.
    //
    // Disable every getty that is not a virtual terminal.  The VTs stay: tty1
    // is the framebuffer console on a board with a display, and it costs
    // nothing on one without.  The board's own serial line is appended below,
    // and this pass comments out the copy an earlier run of assemble left, so
    // repeating the step cannot stack up gettys.
    runner.run(
        "sed -i -E '/^[^#].*agetty/{/agetty.* tty[0-9]/! s/^/#/}' \
         /build/gen/root/etc/inittab",
    )?;

    if let (Some(tty), Some(baud)) = (&board.serial_tty, &board.serial_baud) {
        // -L for the same reason baselayout's own serial lines carry it: a
        // debug header has no modem control lines, so without CLOCAL the tty
        // layer hangs the port up on the missing carrier and agetty dies and
        // respawns once a second, reprinting /etc/issue each time.
        runner.run(&format!(
            "echo 's0:12345:respawn:/sbin/agetty -L {baud} {tty} linux' \
             >> /build/gen/root/etc/inittab"
        ))?;
    }

    // The target stage is shared by every board of an arch, so anything a
    // single board wants in portage's config has to be applied to its own copy
    // here.  Appended verbatim: this is the file where a board says what its
    // own hardware needs -- MAKEOPTS for its core count, USE, VIDEO_CARDS -- and
    // portage takes the last assignment of a variable.
    runner.run(&format!(
        "conf=/scripts/boards/{}/make.conf; \
         if [ -f \"$conf\" ]; then \
             mkdir -p /build/gen/root/etc/portage && \
             cat \"$conf\" >> /build/gen/root/etc/portage/make.conf; \
         fi",
        board.name
    ))?;

    runner.run("sed -i -e 's/root:x:/root::/' /build/gen/root/etc/passwd")?;
    runner.run(
        "mkdir -p /build/gen/root/etc/ssh && \
         printf 'PermitRootLogin yes\nPermitEmptyPasswords yes\nStrictModes yes\n' \
         >> /build/gen/root/etc/ssh/sshd_config",
    )
}

/// OpenRC configuration of an assembled Alpine rootfs.
///
/// Alpine runs OpenRC, so the runlevel machinery above is shared with the
/// Gentoo provider verbatim.  The rest of `os_config_openrc` is not
/// reusable and is redone here: Alpine's hostname service reads
/// /etc/hostname in preference to /etc/conf.d/hostname; init is busybox
/// init, whose inittab lines are `<tty>::<action>:<cmd>` and whose getty
/// is busybox's, not agetty; root's password lives in /etc/shadow, not
/// /etc/passwd; and there is no /etc/portage to append a make.conf to.
fn os_config_alpine(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    os_config_openrc_common(runner, board)?;

    runner.run(&format!(
        "printf '{}\n' > /build/gen/root/etc/hostname",
        board.hostname
    ))?;

    if let (Some(tty), Some(baud)) = (&board.serial_tty, &board.serial_baud) {
        // Alpine ships this exact line commented out; uncommenting is not
        // enough because the board picks the port and the speed.  Drop any
        // line an earlier assemble added first, so repeating the step cannot
        // stack up gettys on one port.  -L for the same reason the Gentoo
        // path passes it: a debug header has no carrier detect.
        runner.run(&format!(
            "sed -i -E '/^{tty}::respawn:/d' /build/gen/root/etc/inittab && \
             echo '{tty}::respawn:/sbin/getty -L {baud} {tty} vt100' \
             >> /build/gen/root/etc/inittab"
        ))?;
    }

    runner.run("sed -i -e 's/^root:[^:]*:/root::/' /build/gen/root/etc/shadow")?;
    // Alpine's sshd_config carries no Include, so append rather than drop a
    // file into sshd_config.d.  Harmless when openssh isn't installed.
    runner.run(
        "mkdir -p /build/gen/root/etc/ssh && \
         printf 'PermitRootLogin yes\nPermitEmptyPasswords yes\n' \
         >> /build/gen/root/etc/ssh/sshd_config",
    )
}

/// systemd configuration of an assembled rootfs: the files the image
/// cannot boot without (fstab, hostname, hosts, a serial getty, a root
/// password) plus the permissive sshd drop-in the Gentoo image also
/// ships.  All plain file edits and symlinks, no target-arch execution.
///
/// Shared by the debian, ubuntu and fedora providers, which differ in how
/// their root is filled and not at all in how systemd is configured: all
/// keep units under /lib/systemd/system (a symlink into /usr everywhere,
/// so the path in the enable symlink resolves either way), all read
/// /etc/hostname, and all ship an sshd_config with an Include of
/// sshd_config.d.
///
/// The init the debootstrap providers produce is systemd, which is not a
/// guess: debootstrap's default variant installs Priority:required plus
/// Priority:important, and `systemd-sysv` (which owns /sbin/init) is
/// Priority:important on both Debian trixie and Ubuntu noble, as is the
/// `init` metapackage whose Pre-Depends puts systemd-sysv first.
fn os_config_systemd(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    let deferred = board.second_stage == crate::provider::SecondStage::FirstBoot;
    let root = "/build/gen/root";

    // debootstrap leaves an "UNCONFIGURED FSTAB FOR BASE SYSTEM" stub with
    // no entries.  systemd mounts the API filesystems itself and the kernel
    // has already mounted the root, so the one line that earns its place is
    // the root entry: it is what fsck-at-boot and `mount -o remount` read.
    // Written through a shell heredoc for the same reason extlinux.conf is:
    // BOOT_ROOT_DEV is spelled PARTUUID=${BOOT_PART_UUID_2} and only means
    // something once expanded.
    if let Some(root_dev) = &board.root_dev {
        runner.run(&format!(
            "{exports}cat > {root}/etc/fstab <<EOF\n\
             # <file system> <mount point> <type> <options> <dump> <pass>\n\
             {root_dev}  /  ext4  defaults,noatime  0  1\n\
             EOF\n",
            exports = DiskId::of(board).exports(),
        ))?;
    }

    runner.run(&format!(
        "printf '{}\n' > {root}/etc/hostname",
        board.hostname
    ))?;
    // Without the second line `sudo`, `hostname -f` and anything else that
    // resolves the machine's own name wait for a DNS timeout that never
    // comes on a board with no network.
    runner.run(&format!(
        "printf '127.0.0.1\tlocalhost\n127.0.1.1\t{host}\n\n\
         ::1\tlocalhost ip6-localhost ip6-loopback\nff02::1\tip6-allnodes\n\
         ff02::2\tip6-allrouters\n' > {root}/etc/hosts",
        host = board.hostname,
    ))?;

    if let (Some(tty), Some(_baud)) = (&board.serial_tty, &board.serial_baud) {
        // Offline enable: serial-getty@ carries its own agetty invocation,
        // the baud rate comes from the kernel console= parameter.  The link
        // target is only unpacked by the second stage, so with a deferred
        // one this dangles until first boot finishes, which is exactly when
        // systemd first reads it.
        runner.run(&format!(
            "mkdir -p {root}/etc/systemd/system/getty.target.wants && \
             ln -sf /lib/systemd/system/serial-getty@.service \
               {root}/etc/systemd/system/getty.target.wants/serial-getty@{tty}.service"
        ))?;
    }

    if !board.services.is_empty() {
        tracing::warn!(
            "BOOT_SERVICES is OpenRC-only and ignored for the {} \
             provider; enable units from post-assemble.sh instead",
            board.rootfs_provider.name()
        );
    }

    if deferred {
        // Stage 1 unpacks only Priority:required, which contains no init on
        // either distribution, so /sbin/init is free and the kernel finds
        // this shim there and nothing else.  It runs the second stage from
        // the .debs already in the image, then hands over to the real init.
        runner.run(&format!(
            "install -m 0755 /scripts/defaults/scripts/debootstrap-second-stage.init \
               {root}/sbin/init"
        ))?;
    } else {
        // /etc/shadow is written by base-passwd's postinst, i.e. by the
        // second stage; with a deferred one it does not exist yet and the
        // shim applies this same policy on the board instead.
        runner.run(&format!(
            "sed -i -e 's/^root:[^:]*:/root::/' {root}/etc/shadow"
        ))?;
    }

    // Same permissive-ssh policy the Gentoo image ships; the stock config
    // on all three distros (prohibit-password) would lock out the only
    // account.  Harmless when sshd isn't installed.
    runner.run(&format!(
        "mkdir -p {root}/etc/ssh/sshd_config.d && \
         printf 'PermitRootLogin yes\nPermitEmptyPasswords yes\n' \
         > {root}/etc/ssh/sshd_config.d/90-crossdev-stages.conf",
    ))
}

/// procd configuration of an assembled OpenWrt rootfs: hostname and the
/// console login.  Deliberately short, because OpenWrt ships the rest.
///
/// Not written here, and why:
///   * root password.  OpenWrt's own /etc/shadow already has `root:::`,
///     the same empty password the other two providers arrange for.
///   * network.  /etc/board.d/99-default_network puts `lan` on eth0 when
///     no board.d script claims the machine, and /bin/config_generate
///     turns that into /etc/config/network on first boot.  A board that
///     needs anything else says so from post-assemble.sh.
///   * sshd.  OpenWrt's ssh is dropbear, configured through UCI, and its
///     shipped /etc/config/dropbear already permits root.
fn os_config_openwrt(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    // /bin/config_generate writes /etc/config/system on first boot, but
    // only while the file is missing -- shipping one from here would take
    // OpenWrt's timezone and ntp defaults down with it.  A uci-defaults
    // script runs afterwards (preinit generates, /etc/init.d/boot applies),
    // sets the one value and deletes itself.
    runner.run(&format!(
        "mkdir -p /build/gen/root/etc/uci-defaults && \
         printf 'uci -q set \"system.@system[0].hostname={}\"\nexit 0\n' \
           > /build/gen/root/etc/uci-defaults/99-crossdev-hostname",
        board.hostname
    ))?;

    // Every login line in the shipped inittab belongs to the OpenWrt
    // target we borrowed the userspace from, not to this board: the
    // sifiveu images say ttySIF0, and base-files on its own says
    // `::askconsole:`.  Drop them all and state the board's own port.
    // Virtual terminals stay, for the same reason the OpenRC path keeps
    // them: tty1 is the display console where there is a display and
    // costs nothing where there is not.  Deleting first is what keeps a
    // second assemble from stacking a second login on one port.
    //
    // BOOT_SERIAL_BAUD has nothing here to read it.  OpenWrt builds
    // busybox without getty and login.sh never touches the line speed;
    // console=<tty>,<baud> on the kernel command line sets it, which is
    // what BOOT_CONSOLE already writes.
    let login = match &board.serial_tty {
        Some(tty) => format!("{tty}::askfirst:/usr/libexec/login.sh"),
        // No named port: let busybox init follow the kernel's console.
        None => "::askconsole:/usr/libexec/login.sh".to_string(),
    };
    runner.run(&format!(
        "sed -i -E '/askfirst|askconsole/{{/^tty[0-9]+::/!d}}' \
           /build/gen/root/etc/inittab && \
         echo '{login}' >> /build/gen/root/etc/inittab"
    ))?;

    if !board.services.is_empty() {
        tracing::warn!(
            "BOOT_SERVICES is OpenRC-only and ignored for the openwrt \
             provider; procd services are enabled with \
             '/etc/init.d/<name> enable' from post-assemble.sh"
        );
    }
    Ok(())
}

fn default_pack(
    runner: &SandboxRunner,
    board: &BoardConfig,
    build: &Build,
    boards_root: &Utf8Path,
) -> Result<()> {
    let board_cfg = boards_root.join(&board.name).join("genimage.cfg");
    let cfg_path = if board_cfg.exists() {
        format!("/scripts/boards/{}/genimage.cfg", board.name)
    } else {
        "/scripts/genimage.cfg".to_string()
    };

    let cfg_name = board
        .image_name
        .clone()
        .unwrap_or_else(|| format!("gentoo-linux-{}_dev-sdcard.img", board.name));

    // genimage's config parser expands ${VAR} through getenv, so a board can
    // write `disk-signature = "${BOOT_DISK_SIG}"` and stay in step with the
    // root= that assemble already wrote.
    runner.run(&format!(
        "set -e\n{disk}\
         rm -rf /build/tmp && cd /build && \
         genimage --config {cfg_path} \
         --mkdosfs mkfs.vfat \
         --inputpath /build --outputpath /build --rootpath /build/gen",
        disk = DiskId::of(board).exports(),
    ))?;

    // Stamp the build timestamp into the image filename so successive builds
    // don't shadow each other when the user copies them out, e.g.
    //   gentoo-linux-premier-p550_dev-sdcard-20260622T031736Z.img.xz
    let ts = build.timestamp();
    let img_name = match cfg_name.strip_suffix(".img") {
        Some(stem) => format!("{stem}-{ts}.img"),
        None => format!("{cfg_name}-{ts}"),
    };
    runner.run(&format!("mv /build/{cfg_name} /build/{img_name}"))?;

    // Emit the sidecar manifest after the timestamped mv (so it records the
    // final image name) and BEFORE compression: partition sources are still
    // in-place and the sha256 covers the uncompressed bytes users will dd.
    let host_cfg = if board_cfg.exists() {
        board_cfg.clone()
    } else {
        project_root(boards_root).join("genimage.cfg")
    };
    crate::manifest::write_image_sidecar(
        &build.dir,
        &board.name,
        &img_name,
        host_cfg.exists().then_some(host_cfg.as_path()),
    )?;

    let compression = board.compression.as_deref().unwrap_or("xz");
    let final_name = match compression {
        "none" => {
            println!("Image ready: {}/{img_name}", build.dir);
            img_name.clone()
        }
        "gz" | "gzip" => {
            runner.run(&format!("gzip -fv -9 /build/{img_name}"))?;
            let name = format!("{img_name}.gz");
            println!("Image ready: {}/{name}", build.dir);
            name
        }
        _ => {
            runner.run(&format!("xz -fv -T0 -9 /build/{img_name}"))?;
            let name = format!("{img_name}.xz");
            println!("Image ready: {}/{name}", build.dir);
            name
        }
    };

    std::fs::write(build.dir.join(".image"), &final_name)?;
    Ok(())
}

// ── Pipeline ────────────────────────────────────────────────────────────────

pub fn build(
    ws: &Workspace,
    sandbox: &Sandbox,
    target: &Target,
    board: &BoardConfig,
    boards_root: &Utf8Path,
    defaults_root: &Utf8Path,
    steps: Option<&[String]>,
) -> Result<()> {
    let bld = Build::create(ws, &board.name)?;
    let mut manifest = crate::manifest::ManifestBuilder::new(board);
    warn_unpinned_sources(board);

    let provider = board.rootfs_provider;
    if provider == RootfsProvider::Gentoo {
        // Refresh the target's make.conf with the board's CFLAGS before any
        // step runs so cross-emerges in `deps`, `kernel`, etc. all see the
        // same flags the board declares.  Cheap and idempotent; recovers from
        // targets unpacked elsewhere or built against a different board.
        // Other providers own /target -- no portage config is written into it.
        let board_cflags = board.effective_cflags();
        let gcc_spec = sandbox.gcc_spec_for(board, None)?;
        target.prepare_portage_with_cflags(ws, &board.chost(), &board_cflags, &gcc_spec)?;
        let (canonical, _) = crate::cflags::canonicalize(&board_cflags);
        let hash = crate::cflags::toolchain_key(board);
        tracing::info!("Target make.conf CFLAGS={canonical:?} (key {hash})");

        // Say what the binary package cache is and how the toolchain has moved
        // since it was last written to.  Advisory: nothing here invalidates the
        // cache, because a stamp that triggers rebuilds is only a cache key with
        // worse ergonomics.  If the drift produced something the board cannot
        // load, the ABI check at the end of assemble says so against the image.
        let _ = crate::binpkg_meta::report(
            &board_binpkgs_dir(ws, board)?,
            &ws.store_dir().join(crate::workspace::store_key(
                &board.chost(),
                &hash,
                &gcc_spec,
            )),
        );
    }

    // Per-(chost, cflags-hash) binpkg dir bind-mounted at /binpkgs.
    // PKGDIR=/binpkgs lives in the crossdev prefix make.conf (the config
    // {chost}-emerge actually reads), never in the target's -- the target
    // make.conf ships into images.
    let binpkgs_dir = board_binpkgs_dir(ws, board)?;

    let steps_to_run: Vec<&str> = match steps {
        Some(s) => s.iter().map(String::as_str).collect(),
        None => board.effective_build_steps(),
    };

    // Providers without a cross-emerge payload skip the toolchain store
    // entirely (it may not exist); their runners are plain sandbox runners.
    let needs_toolchain = provider.needs_cross_toolchain(&steps_to_run);

    let total = steps_to_run.len();
    let build_start = std::time::Instant::now();

    for (i, step) in steps_to_run.iter().enumerate() {
        let step_start = std::time::Instant::now();
        println!("==> [{}/{}] {}...", i + 1, total, step);

        // Steps that invoke the cross toolchain (deps cross-emerge,
        // bootloader, kernel) need the store overlay-mounted at
        // /usr/<chost>/.  Pure-userspace steps (checkout, assemble, pack)
        // don't, but the overlay costs nothing to mount, so always use
        // runner_for_board for consistency.
        let runner = if needs_toolchain {
            sandbox.runner_for_board(ws, &board.arch, board)?
        } else {
            sandbox.runner()
        }
        .with_target(&target.dir)
        .with_build(&bld.dir, &project_root(boards_root))
        .with_cache(ws.base())
        .with_binpkgs(&binpkgs_dir);

        let result = match *step {
            "deps" => run_step("deps", "deps", &bld, &runner, boards_root, board, |_r| {
                // A wrong atom is otherwise only found by emerge, which gets
                // there after the sandbox list has already been built -- ten
                // minutes of compiling thrown away over a package that was
                // renamed.  The repos are plain directories on the host, so
                // the same question costs a stat().  Provider-independent:
                // the sandbox is a Gentoo stage3 whatever fills the image.
                check_package_lists(
                    sandbox,
                    board,
                    &boards_root.join(&board.name),
                    defaults_root,
                )?;
                install_sandbox_extras(sandbox, board, boards_root)?;
                match provider.debootstrap() {
                    Some(deb) => debootstrap_deps(_r, target, board, boards_root, deb),
                    None if provider == RootfsProvider::Gentoo => {
                        default_deps(_r, ws, sandbox, target, board, boards_root, defaults_root)
                    }
                    None if provider == RootfsProvider::Alpine => {
                        alpine_deps(_r, target, board, boards_root)
                    }
                    None if provider == RootfsProvider::Fedora => {
                        fedora_deps(_r, target, board, boards_root)
                    }
                    None if provider == RootfsProvider::Buildroot => {
                        buildroot_deps(_r, target, board)
                    }
                    None if provider == RootfsProvider::OpenWrt => {
                        openwrt_deps(_r, target, board, boards_root)
                    }
                    // No default package installation; override-deps.sh
                    // (already handled by run_step) is the provider.
                    None => Ok(()),
                }
            }),
            "checkout" => run_step(
                "checkout",
                "sources",
                &bld,
                &runner,
                boards_root,
                board,
                |r| default_checkout(r, board, boards_root),
            ),
            "bootloader" => run_step(
                "bootloader",
                "bootloader",
                &bld,
                &runner,
                boards_root,
                board,
                |r| default_bootloader(r, board),
            ),
            "kernel" => run_step("kernel", "kernel", &bld, &runner, boards_root, board, |r| {
                default_kernel(r, board)
            }),
            "assemble" => run_step(
                "assemble",
                "assembled",
                &bld,
                &runner,
                boards_root,
                board,
                |r| {
                    let kernel_built =
                        steps_to_run.contains(&"kernel") || bld.dir.join("linux").is_dir();
                    default_assemble(r, board, &bld, ws, kernel_built)
                },
            ),
            "pack" => run_step("pack", "packed", &bld, &runner, boards_root, board, |r| {
                default_pack(r, board, &bld, boards_root)
            }),
            // Custom step: no Rust default. run_step still honours an
            // override-<step>.sh hook (with resume-marker support); if the
            // hook is missing, the default_fn below turns it into a hard error.
            other => run_step(other, other, &bld, &runner, boards_root, board, |_r| {
                let hook = format!("boards/{}/override-{}.sh", board.name, other);
                Err(crate::error::Error::BoardConfigParse {
                    file: hook.clone(),
                    msg: format!("no default for build step '{other}'; add {hook}"),
                })
            }),
        };

        let elapsed = step_start.elapsed();
        println!("    {} done ({})", step, format_duration(elapsed));
        result?;

        // The image tree is finished at the end of assemble, hooks included,
        // and portage never checked any of this: it matches CHOST, KEYWORDS,
        // USE and CPU_FLAGS_X86 when it decides a binary package fits, and
        // never CFLAGS.  A few seconds here against an illegal instruction
        // that otherwise surfaces on the hardware.
        if *step == "assemble" {
            // Every architecture: can the image load what it ships?  A binary
            // that asks for a symbol version the image does not carry does not
            // start at all, which is a worse failure than a wrong instruction
            // set and a cheaper one to catch.
            match crate::abi::verify(
                &runner,
                "/build/gen/root",
                &format!("{}readelf", board.cross_compile),
            ) {
                Ok(report) => {
                    if crate::abi::print(&report) {
                        return Err(crate::error::Error::CommandFailed {
                            code: 1,
                            reason: "the image cannot load its own binaries".into(),
                        });
                    }
                }
                Err(e) => tracing::warn!("ABI check could not run: {e}"),
            }
        }

        if *step == "assemble" && crate::isa::applies(board) {
            match crate::isa::verify(&runner, board, "/build/gen/root") {
                Ok(report) => {
                    let illegal = crate::isa::print(&report, Utf8Path::new("/build/gen/root"));
                    if illegal && board.isa_strict {
                        return Err(crate::error::Error::CommandFailed {
                            code: 1,
                            reason: "binaries use extensions this board does not have".into(),
                        });
                    }
                }
                // A board whose toolchain cannot answer is not a reason to
                // throw away a built image.
                Err(e) => tracing::warn!("ISA check could not run: {e}"),
            }
        }
    }

    // Collect and write manifest before returning. Build fails if manifest
    // collection fails -- but every probe is best-effort so this should only
    // fire on pathological runtime issues.  Use the overlay-mounted
    // runner so /usr/<chost>/etc/portage/make.conf resolves to the
    // store-resident prefix this build actually used, not whatever
    // legacy content the sandbox happens to carry.
    let runner = if needs_toolchain {
        sandbox.runner_for_board(ws, &board.arch, board)?
    } else {
        sandbox.runner()
    }
    .with_target(&target.dir)
    .with_build(&bld.dir, &project_root(boards_root))
    .with_cache(ws.base());
    record_sources(&runner, &mut manifest, board)?;
    if manifest.has_resolved_source() {
        let manifest_path = bld.dir.join("build.lock.toml");
        manifest.write(&runner, &manifest_path)?;
        tracing::info!("Manifest written: {manifest_path}");
    } else {
        // Partial builds (e.g. `--steps deps`) leave every source as
        // kind=missing because checkout hasn't run.  Skipping the lock
        // write here keeps `crossdev-stages update` from picking up a
        // useless lock as the newest one for the board.
        tracing::info!("Skipping manifest write: no resolved git sources yet");
    }

    let total_elapsed = build_start.elapsed();
    println!("\nBuild complete: {}", format_duration(total_elapsed));
    Ok(())
}

/// Flag sources that follow a default branch instead of a named tag, so
/// stale sysroots and silent upstream drifts stand out in build output.
/// Phase 4 turns this into a proper `status` command; for now it's a warn
/// at the top of every `image build`.
fn warn_unpinned_sources(board: &BoardConfig) {
    let check = |name: &str, repo: Option<&str>, tag: Option<&str>| {
        if let Some(repo) = repo {
            let t = tag.unwrap_or("master");
            if matches!(t, "master" | "main" | "trunk" | "HEAD") {
                tracing::warn!(
                    "source '{name}' ({repo}) tracks branch '{t}' -- no pin. \
                     Build is reproducible only against the resolved commit in \
                     build.lock.toml, not against the board.conf TAG field."
                );
            }
        }
    };
    check(
        "opensbi",
        board.opensbi_repo.as_deref(),
        board.opensbi_tag.as_deref(),
    );
    check(
        "uboot",
        board.u_boot_repo.as_deref(),
        board.u_boot_tag.as_deref(),
    );
    check(
        "syslinux",
        board.syslinux_repo.as_deref(),
        board.syslinux_tag.as_deref(),
    );
    let fw_tag = board.effective_firmware_tag();
    check(
        "firmware",
        board.firmware_repo.as_deref(),
        Some(fw_tag.as_str()),
    );
    if board.rootfs_provider == RootfsProvider::Buildroot {
        check(
            "buildroot",
            Some(board.buildroot_repo.as_deref().unwrap_or(BUILDROOT_REPO)),
            board.buildroot_tag.as_deref(),
        );
    }
    // A board whose kernel comes from its rootfs provider declares no
    // kernel repo; there is nothing to be unpinned about.
    if !board.kernel_repo.is_empty() {
        check("kernel", Some(&board.kernel_repo), Some(&board.kernel_tag));
    }
    // TODO: check fip once BOOT_PIPELINE lands fip_repo/fip_tag on BoardConfig.
}

fn record_sources(
    runner: &SandboxRunner,
    manifest: &mut crate::manifest::ManifestBuilder,
    board: &BoardConfig,
) -> Result<()> {
    if let (Some(repo), Some(tag)) = (&board.opensbi_repo, &board.opensbi_tag) {
        manifest.record_source(runner, "opensbi", repo, tag, "/build/opensbi")?;
    }
    if let (Some(repo), Some(tag)) = (&board.u_boot_repo, &board.u_boot_tag) {
        manifest.record_source(runner, "uboot", repo, tag, "/build/u-boot")?;
    }
    if let (Some(repo), Some(tag)) = (&board.syslinux_repo, &board.syslinux_tag) {
        manifest.record_source(runner, "syslinux", repo, tag, "/build/syslinux")?;
    }
    if let Some(repo) = &board.firmware_repo {
        let tag = board.effective_firmware_tag();
        manifest.record_source(runner, "firmware", repo, &tag, "/build/firmware")?;
    }
    // TODO: record fip once BOOT_PIPELINE lands fip_repo/fip_tag on BoardConfig.
    if board.rootfs_provider == RootfsProvider::Buildroot {
        manifest.record_source(
            runner,
            "buildroot",
            board.buildroot_repo.as_deref().unwrap_or(BUILDROOT_REPO),
            board.buildroot_tag.as_deref().unwrap_or("master"),
            BUILDROOT_SRC,
        )?;
    }
    if !board.kernel_repo.is_empty() {
        manifest.record_source(
            runner,
            "kernel",
            &board.kernel_repo,
            &board.kernel_tag,
            "/build/linux",
        )?;
    }
    Ok(())
}

fn format_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m {}s", secs / 3600, (secs % 3600) / 60, secs % 60)
    }
}

/// The ImageBuilder run, as one shell script.  Placeholders instead of
/// `format!` so the shell's own braces stay readable (same trick as
/// `abi::SCAN`).
const OPENWRT_IMAGEBUILDER: &str = r#"
set -e
mkdir -p /cache/openwrt /build/openwrt
cd /cache/openwrt

# The release directory never changes, but only the sums file says which
# compression this release used (.tar.zst since 25.12, .tar.xz before),
# so read the file name out of it rather than guessing one.
wget -nv -O sums.%KEY% %URL%/sha256sums
line=$(grep -E '^[0-9a-f]{64} [*]openwrt-imagebuilder-.*[.]Linux-x86_64[.]tar[.]' sums.%KEY% | head -n 1)
[ -n "$line" ] || { echo "no ImageBuilder listed in %URL%/sha256sums" >&2; exit 1; }
ib=$(echo "$line" | sed 's/^[0-9a-f]* [*]//')
%PIN%
if [ ! -f "$ib" ]; then
    wget -nv -O "$ib.part" "%URL%/$ib"
    mv -f "$ib.part" "$ib"
fi
printf '%s\n' "$line" > "$ib.sha256"
sha256sum -c "$ib.sha256" || {
    rm -f "$ib"
    echo "ImageBuilder checksum mismatch; the cached copy was dropped" >&2
    exit 1
}

rm -rf /build/openwrt/ib
mkdir -p /build/openwrt/ib
tar -xf "$ib" -C /build/openwrt/ib --strip-components=1
cd /build/openwrt/ib
mkdir -p tmp

# Feed URLs are baked in absolute.  apk (25.12 and later) and opkg (24.10
# and earlier) each keep them in a file of their own.
for f in repositories repositories.conf; do
    if [ -f "$f" ]; then sed -i 's|https://downloads.openwrt.org|%MIRROR%|g' "$f"; fi
done

# ImageBuilder builds the filesystems its target declares, and on most
# targets those are all device images.  The plain rootfs tarball is the
# one artifact this pipeline wants, so ask for it by name.
sed -i '/CONFIG_TARGET_ROOTFS_TARGZ/d' .config
echo 'CONFIG_TARGET_ROOTFS_TARGZ=y' >> .config

make image PROFILE='%PROFILE%' PACKAGES='%PACKAGES%'

rootfs=$(find bin/targets -name '*rootfs.tar.gz' | sort | head -n 1)
[ -n "$rootfs" ] || { echo 'ImageBuilder produced no rootfs tarball' >&2; exit 1; }
echo "OpenWrt rootfs: $rootfs"

find /target -mindepth 1 -maxdepth 1 \
    ! -name '.arch' ! -name '.stage3' ! -name '.provider' -exec rm -rf {} +
# Keep the mount point, drop whatever is inside it: a device node cannot
# be made in a rootless userns (the stage3 unpack excludes ./dev for the
# same reason), and devtmpfs mounts over it at boot.
tar -xf "$rootfs" -C /target --exclude='./dev/*'
"#;

/// Provision an OpenWrt rootfs in /target with the official ImageBuilder.
///
/// Fetches the pinned per-release, per-target ImageBuilder, checks it
/// against that release directory's published `sha256sums`, and runs
/// `make image` with the board's profile and package list.  Nothing is
/// compiled here: ImageBuilder only assembles prebuilt binary packages,
/// and the apk inside it verifies every package it pulls from the feeds
/// against the signing keys shipped in the tarball we just checksummed.
///
/// The rootfs lands in /target rather than straight in gen/root because
/// `assemble` deletes gen/ before it copies /target in: anything an
/// earlier step left there would not survive the next assemble.
///
/// OpenWrt's own kernel is not taken.  This project builds the kernel
/// (KERNEL_REPO, the `kernel` step) and `assemble` installs its modules
/// over whatever the provider put in /target, exactly as it does for
/// Gentoo and Debian.  Taking OpenWrt's kernel would mean taking its
/// image recipe, its DTB and its bootloader as well, which is the whole
/// of the pipeline this project is.
fn openwrt_deps(
    runner: &SandboxRunner,
    target: &Target,
    board: &BoardConfig,
    boards_root: &Utf8Path,
) -> Result<()> {
    let done = target.dir.join(".openwrt-done");
    if done.exists() {
        tracing::info!("OpenWrt rootfs already provisioned, skipping ImageBuilder.");
        return Ok(());
    }

    // All four name one immutable directory and one device inside it.
    // None has a defensible default: a missing release would have to mean
    // "whatever is newest today", and that is the opposite of a pin.
    let need = |key: &str, value: &Option<String>| -> Result<String> {
        value.clone().ok_or_else(|| Error::BoardConfigParse {
            file: board.name.clone(),
            msg: format!("{key} is required for ROOTFS_PROVIDER=\"openwrt\""),
        })
    };
    let release = need("OPENWRT_RELEASE", &board.openwrt_release)?;
    let owrt_target = need("OPENWRT_TARGET", &board.openwrt_target)?;
    let subtarget = need("OPENWRT_SUBTARGET", &board.openwrt_subtarget)?;
    let profile = need("OPENWRT_PROFILE", &board.openwrt_profile)?;
    let mirror = board
        .openwrt_mirror
        .as_deref()
        .unwrap_or("https://downloads.openwrt.org");

    // `-pkg` removes a package the profile would otherwise pull in;
    // ImageBuilder reads that syntax natively, so the lists here look
    // like the target-package lists elsewhere in the tree.
    let packages =
        read_distro_packages(&boards_root.join(&board.name).join("openwrt-packages.txt"))?
            .join(" ");

    // The stage3 already carries make, perl, python and tar; these three
    // are what OpenWrt's prereq check asks for on top of it.  Anything
    // else a board needs goes in its sandbox-packages.txt.
    let portage = Portage::new(runner);
    portage.emerge(&[
        "--noreplace",
        "net-misc/wget",
        "app-arch/unzip",
        "sys-apps/which",
    ])?;

    // A published sums file is fetched over the same connection as the
    // tarball, so on its own it proves only that the download arrived
    // intact.  OPENWRT_SHA256 is the value a mirror cannot talk its way
    // out of, and it is what makes the board.conf a real pin.
    let pin = match &board.openwrt_sha256 {
        Some(want) => format!(
            "sum=$(echo \"$line\" | sed 's/ .*//')\n\
             [ \"$sum\" = '{want}' ] || {{ \
                 echo \"OPENWRT_SHA256 is {want}, the release publishes $sum\" >&2; exit 1; }}"
        ),
        None => String::new(),
    };

    runner.run(
        &OPENWRT_IMAGEBUILDER
            .replace("%KEY%", &format!("{release}-{owrt_target}-{subtarget}"))
            .replace(
                "%URL%",
                &format!("{mirror}/releases/{release}/targets/{owrt_target}/{subtarget}"),
            )
            .replace("%MIRROR%", mirror)
            .replace("%PROFILE%", &profile)
            .replace("%PACKAGES%", &packages)
            .replace("%PIN%", &pin),
    )?;

    std::fs::write(done, chrono::Utc::now().to_rfc3339())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        atom_cpn, buildroot_build_script, extlinux_conf, extlinux_fdt, kernel_config_fragments,
        read_distro_packages,
    };
    use crate::board::BoardConfig;
    use camino::Utf8PathBuf;

    fn list_file(name: &str, body: &str) -> Utf8PathBuf {
        let path = Utf8PathBuf::from_path_buf(std::env::temp_dir())
            .expect("temp dir is utf-8")
            .join(format!("crossdev-stages-{name}.txt"));
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn a_missing_package_list_is_an_empty_one() {
        assert!(
            read_distro_packages("/nonexistent/openwrt-packages.txt".into())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_package_list_keeps_removals_and_drops_comments() {
        // `-pkg` is OpenWrt's own "leave this out" syntax and has to
        // survive; two names on one line is the case that used to be
        // half-read, so it stays an error.
        let path = list_file("packages", "# note\n\nluci-ssl\n  -dnsmasq  \n");
        assert_eq!(
            read_distro_packages(&path).unwrap(),
            vec!["luci-ssl".to_string(), "-dnsmasq".to_string()]
        );
        let bad = list_file("packages-bad", "luci-ssl htop\n");
        assert!(read_distro_packages(&bad).is_err());
    }

    fn board_with(fragments: &[&str]) -> BoardConfig {
        let mut board = crate::cli::util::default_board_config("riscv64");
        board.name = "demo".into();
        board.kernel_config_fragments = fragments.iter().map(|s| s.to_string()).collect();
        board
    }

    #[test]
    fn no_fragments_generates_nothing() {
        assert!(kernel_config_fragments(&board_with(&[])).is_empty());
    }

    #[test]
    fn a_fragment_is_looked_up_board_first_then_defaults() {
        let script = kernel_config_fragments(&board_with(&["riscv64-no-vector"]));
        assert!(script.contains("/scripts/boards/demo/kernel-config/riscv64-no-vector"));
        assert!(script.contains("/scripts/defaults/kernel-config/riscv64-no-vector"));
        // Both directions of the check have to be generated: a symbol that was
        // asked for and lost, and one asked to be off that came back.
        assert!(script.contains("kernel config lost"));
        assert!(script.contains("came back on"));
    }

    fn extlinux_board() -> BoardConfig {
        let mut board = board_with(&[]);
        board.extlinux = true;
        board.kernel_name = Some("Image".into());
        board.root_dev = Some("PARTUUID=${BOOT_PART_UUID_2}".into());
        board.console = Some("ttyS2,1500000".into());
        board.append = Some("earlycon".into());
        board.kernel_dtb_glob = Some("arch/arm64/boot/dts/rockchip/rk3568-odroid-m1.dtb".into());
        board
    }

    #[test]
    fn an_exact_dtb_path_names_the_device_tree_by_itself() {
        let board = extlinux_board();
        assert_eq!(
            extlinux_fdt(&board).unwrap().as_deref(),
            Some("rk3568-odroid-m1.dtb")
        );
    }

    #[test]
    fn a_dtb_glob_has_to_be_narrowed_by_hand() {
        let mut board = extlinux_board();
        board.kernel_dtb_glob = Some("arch/arm64/boot/dts/rockchip/*.dtb".into());
        assert!(extlinux_fdt(&board).is_err());
        board.dtb_name = Some("rk3588s-odroid-m2.dtb".into());
        assert_eq!(
            extlinux_fdt(&board).unwrap().as_deref(),
            Some("rk3588s-odroid-m2.dtb")
        );
    }

    #[test]
    fn the_written_config_is_an_extlinux_file_with_the_partuuid_expanded() {
        let script = extlinux_conf(&extlinux_board()).unwrap();
        // The heredoc has to stay unquoted, with the identifiers exported
        // ahead of it, or root= reaches the kernel as a literal ${...}.
        assert!(script.contains("export BOOT_PART_UUID_2="));
        let body = script.split("<<EOF\n").nth(1).unwrap();
        assert_eq!(
            body,
            "DEFAULT linux\n\
             TIMEOUT 30\n\
             LABEL linux\n    \
             MENU LABEL demo\n    \
             LINUX /Image\n    \
             FDT /rk3568-odroid-m1.dtb\n    \
             APPEND root=PARTUUID=${BOOT_PART_UUID_2} rw rootwait \
             rootfstype=ext4 console=ttyS2,1500000 earlycon\n\
             EOF\n"
        );
    }

    #[test]
    fn extlinux_without_a_kernel_name_is_rejected() {
        let mut board = extlinux_board();
        board.kernel_name = None;
        assert!(extlinux_conf(&board).is_err());
    }

    #[test]
    fn a_defconfig_is_looked_up_board_first_then_buildroots_own_configs() {
        let script = buildroot_build_script("demo", "qemu_riscv64_virt_defconfig");
        assert!(script.contains("/scripts/boards/demo/qemu_riscv64_virt_defconfig"));
        assert!(script.contains("configs/qemu_riscv64_virt_defconfig"));
        // A board file goes through BR2_DEFCONFIG; a stock name is a
        // make target.  Both forms have to be generated.
        assert!(script.contains("BR2_DEFCONFIG=\"$board_defconfig\" defconfig"));
        assert!(script.contains("make O=/build/buildroot-out qemu_riscv64_virt_defconfig"));
        // Neither one silently building the wrong thing: a name that is
        // in neither place stops the build.
        assert!(script.contains("no BUILDROOT_DEFCONFIG"));
    }

    #[test]
    fn the_rootfs_tar_is_forced_on_and_then_checked() {
        let script = buildroot_build_script("demo", "any_defconfig");
        // Appending the symbol is not enough: olddefconfig drops one
        // whose dependencies are unmet without a word.
        assert!(script.contains("echo 'BR2_TARGET_ROOTFS_TAR=y' >>"));
        assert!(script.contains("olddefconfig"));
        assert!(script.contains("grep -qx 'BR2_TARGET_ROOTFS_TAR=y'"));
    }

    /// Buildroot does not support a parallel top-level make, so the
    /// `-j$(nproc)` every other build step passes must not appear here.
    #[test]
    fn the_top_level_make_is_not_parallel() {
        let script = buildroot_build_script("demo", "any_defconfig");
        assert!(!script.contains("-j"));
        assert!(script.contains("BR2_DL_DIR=/cache/buildroot-dl"));
    }

    #[test]
    fn an_atom_reduces_to_the_directory_that_would_hold_it() {
        assert_eq!(atom_cpn("media-libs/mesa"), Some(("media-libs", "mesa")));
        // The forms boards actually write.
        assert_eq!(
            atom_cpn("=sys-apps/busybox-1.38.0"),
            Some(("sys-apps", "busybox"))
        );
        assert_eq!(
            atom_cpn(">=media-libs/x264-0.164"),
            Some(("media-libs", "x264"))
        );
        assert_eq!(atom_cpn("dev-lang/rust:stable"), Some(("dev-lang", "rust")));
        // A dash in the name is not a version.
        assert_eq!(
            atom_cpn("media-plugins/gst-plugins-v4l2"),
            Some(("media-plugins", "gst-plugins-v4l2"))
        );
        assert_eq!(atom_cpn("no-category-here"), None);
    }
}
