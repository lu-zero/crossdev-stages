use camino::{Utf8Path, Utf8PathBuf};

use chrono::Utc;

use crate::board::BoardConfig;
use crate::container::SandboxRunner;
use crate::error::{Error, Result};
use crate::portage::Portage;
use crate::sandbox::Sandbox;
use crate::target::Target;
use crate::workspace::Workspace;

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
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13],
            b[14], b[15],
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

fn default_deps(
    _runner: &SandboxRunner,
    ws: &Workspace,
    sandbox: &Sandbox,
    target: &Target,
    board: &BoardConfig,
    boards_root: &Utf8Path,
    defaults_root: &Utf8Path,
) -> Result<()> {
    // A wrong atom is otherwise only found by emerge, which gets there after
    // the sandbox list has already been built -- ten minutes of compiling
    // thrown away over a package that was renamed.  The repos are plain
    // directories on the host, so the same question costs a stat().
    let board_dir = boards_root.join(&board.name);
    check_package_lists(sandbox, board, &board_dir, defaults_root)?;

    // Sandbox extras: defaults are already installed during prepare; only the
    // board's own extras (e.g. grub for pentium-mmx) need emerging here.
    // merge() with an empty base means a `-atom` line here can only cancel
    // the board's own extras, never uninstall a prepare-time default.
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

    // Target packages: defaults UNION board extras MINUS board `-atom` lines.
    let target_pkgs = crate::package_list::merge(
        crate::package_list::read_required(&defaults_root.join("target-packages.txt"))?,
        crate::package_list::read_optional(&board_dir.join("target-packages.txt"))?,
    );
    if !target_pkgs.is_empty() {
        // The keyword a line asks for has to be written where the cross emerge
        // will look for it: `{chost}-emerge` reads PORTAGE_CONFIGROOT=
        // /usr/{chost}, not the sandbox's own /etc/portage.  Without this a
        // line like `sys-boot/syslinux **` gets its atom emerged and its
        // keyword silently dropped, and the emerge fails on a masked package.
        let cross_portage = sandbox
            .dir
            .join(format!("usr/{}/etc/portage", board.chost()));
        crate::package_list::write_accept_keywords(&target_pkgs, &cross_portage)?;

        let target_runner = sandbox
            .runner_for_board(ws, &board.arch, board)?
            .with_target(&target.dir)
            .with_binpkgs(&board_binpkgs_dir(ws, board)?);
        let portage = Portage::new(&target_runner);
        portage.cross_emerge(&board.chost(), &crate::package_list::atoms(&target_pkgs))?;
    }

    Ok(())
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
) -> Result<()> {
    let karch =
        board
            .kernel_arch
            .as_deref()
            .ok_or_else(|| crate::error::Error::BoardConfigParse {
                file: board.name.clone(),
                msg: "KERNEL_ARCH required for assemble".into(),
            })?;

    // Start from nothing.  `cp -a /target/.` merges, so anything an earlier
    // build of this same tree put here outlives being taken back out -- a
    // service dropped from the board's list, a package.mask deleted from the
    // target, a firmware directory renamed.  The whole tree is rebuilt from
    // /target and this step's own work, so there is nothing here worth
    // keeping, and the copy that follows costs the same either way.
    crate::container::destroy_dir(&build.dir.join("gen"), ws.base())?;

    runner.run("mkdir -p /build/gen/root /build/gen/boot")?;
    runner.run("cp -a /target/. /build/gen/root/")?;
    // unpack_tarball excludes ./dev to avoid permission errors in rootless containers.
    // Recreate the empty mount-point directories so the kernel can mount devtmpfs,
    // procfs, sysfs and tmpfs at boot.
    runner.run(
        "mkdir -p /build/gen/root/{dev,proc,sys,run,tmp,mnt,media} && \
         chmod 1777 /build/gen/root/tmp",
    )?;

    runner.run(&format!(
        "make -C /build/linux ARCH={karch} CROSS_COMPILE={cc} \
         INSTALL_MOD_PATH=/build/gen/root modules_install",
        cc = board.cross_compile,
    ))?;

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

    if let Some(dtb_glob) = &board.kernel_dtb_glob {
        runner.run(&format!("cp /build/linux/{dtb_glob} /build/gen/boot/"))?;
    }

    if let Some(kname) = &board.kernel_name {
        runner.run(&format!(
            "cp /build/linux/arch/{karch}/boot/{kname} /build/gen/boot/"
        ))?;
    }

    if board.extlinux {
        runner.run(&extlinux_conf(board)?)?;
    }

    runner
        .run("mkdir -p /build/gen/root/etc/runlevels/{boot,default,nonetwork,shutdown,sysinit}")?;

    // Board-agnostic: grow-rootfs oneshot, runs once on first boot, fills
    // the rootfs partition out to the disk end + resize2fs.  Needs
    // sys-block/parted + sys-fs/e2fsprogs in the target.
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
    )?;

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

    // Refresh the target's make.conf with the board's CFLAGS before any
    // step runs so cross-emerges in `deps`, `kernel`, etc. all see the
    // same flags the board declares.  Cheap and idempotent; recovers from
    // targets unpacked elsewhere or built against a different board.
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
        &ws.store_dir()
            .join(crate::workspace::store_key(&board.chost(), &hash, &gcc_spec)),
    );

    // Per-(chost, cflags-hash) binpkg dir bind-mounted at /binpkgs.
    // PKGDIR=/binpkgs lives in the crossdev prefix make.conf (the config
    // {chost}-emerge actually reads), never in the target's -- the target
    // make.conf ships into images.
    let binpkgs_dir = board_binpkgs_dir(ws, board)?;

    let steps_to_run: Vec<&str> = match steps {
        Some(s) => s.iter().map(String::as_str).collect(),
        None => board.effective_build_steps(),
    };

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
        let runner = sandbox
            .runner_for_board(ws, &board.arch, board)?
            .with_target(&target.dir)
            .with_build(&bld.dir, &project_root(boards_root))
            .with_cache(ws.base())
            .with_binpkgs(&binpkgs_dir);

        let result = match *step {
            "deps" => run_step("deps", "deps", &bld, &runner, boards_root, board, |_r| {
                default_deps(_r, ws, sandbox, target, board, boards_root, defaults_root)
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
                |r| default_assemble(r, board, &bld, ws),
            ),
            "pack" => run_step("pack", "packed", &bld, &runner, boards_root, board, |r| {
                default_pack(r, board, &bld, boards_root)
            }),
            other => {
                tracing::warn!("Unknown step '{}', skipping.", other);
                Ok(())
            }
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
    let runner = sandbox
        .runner_for_board(ws, &board.arch, board)?
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
    check("kernel", Some(&board.kernel_repo), Some(&board.kernel_tag));
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
    manifest.record_source(
        runner,
        "kernel",
        &board.kernel_repo,
        &board.kernel_tag,
        "/build/linux",
    )?;
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

#[cfg(test)]
mod tests {
    use super::{atom_cpn, extlinux_conf, extlinux_fdt, kernel_config_fragments};
    use crate::board::BoardConfig;

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
