//! Composable bootloader pipeline.
//!
//! Each component (opensbi, uboot, grub, syslinux, tfa, rkbin, ...) is a
//! self-contained module exposing three functions:
//!
//!   * `clone(runner, board)`         — fetch source, no-op if the board
//!                                       doesn't use this component.
//!   * `build(runner, board, env)`    — compile; receives env contributions
//!                                       from earlier stages as `&[String]`.
//!   * `exports(board) -> Vec<String>` — env vars this stage provides to
//!                                       LATER stages (e.g. tfa exports
//!                                       `BL31=...`).
//!
//! Boards declare an ordered `BOOT_PIPELINE` array selecting which stages
//! and in what order:
//!
//!   * RISC-V vendor SDKs (K1, K230, KY-X1):  `("opensbi" "uboot")`
//!   * Rockchip (Odroid M2):                  `("rkbin" "tfa" "uboot")`
//!   * Amlogic FIP (Odroid C2/C4):            `("tfa" "uboot" "amlogic-fip")`
//!   * x86 BIOS (Pentium MMX):                `("syslinux" "grub")`
//!   * Tenstorrent Blackhole:                 `("opensbi")`  — no U-Boot
//!   * Boards with all-prebuilt firmware:     `()`           — no stages
//!
//! When `BOOT_PIPELINE` is omitted, [`DEFAULT_PIPELINE`] kicks in
//! (`opensbi uboot syslinux grub` — every stage no-ops when its board.conf
//! keys are absent, so the default is safe for any board).  An explicitly
//! empty array `()` runs no stages.  Stage names are validated against
//! [`STAGES`] when board.conf is loaded.
//!
//! Components do not reach into each other.  Cross-stage data flows via
//! `exports()` collected by the pipeline runner and prepended (as `K=v`
//! env tokens) to the next `build()`'s shell command.

pub mod amlogic_fip;
pub mod grub;
pub mod opensbi;
pub mod rkbin;
pub mod syslinux;
pub mod tfa;
pub mod uboot;

use crate::board::BoardConfig;
use crate::container::SandboxRunner;
use crate::error::{Error, Result};

/// Pipeline used when `BOOT_PIPELINE` is not declared in `board.conf`.
/// Each stage no-ops unless its board.conf keys are set, so this covers
/// both the RISC-V vendor SDK pattern (opensbi + uboot) and the x86 BIOS
/// pattern (syslinux + grub) without configuration.
pub const DEFAULT_PIPELINE: &[&str] = &["opensbi", "uboot", "syslinux", "grub"];

/// All valid pipeline stage names (checked at board.conf load time).
pub const STAGES: &[&str] = &[
    "opensbi",
    "uboot",
    "grub",
    "syslinux",
    "tfa",
    "rkbin",
    "amlogic-fip",
];

/// Resolve the pipeline for `board`: explicit `BOOT_PIPELINE` if the key
/// is present (an empty array means no stages), otherwise
/// [`DEFAULT_PIPELINE`].
pub fn pipeline(board: &BoardConfig) -> Vec<&str> {
    match &board.boot_pipeline {
        None => DEFAULT_PIPELINE.to_vec(),
        Some(stages) => stages.iter().map(String::as_str).collect(),
    }
}

/// Run `clone()` for every stage in the pipeline (in declared order).
pub fn clone_pipeline(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    for stage in pipeline(board) {
        clone_stage(stage, runner, board)?;
    }
    Ok(())
}

/// Run `build()` for every stage in the pipeline, threading exported env
/// vars from earlier stages into later stages' shell commands.
pub fn build_pipeline(runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    let mut env: Vec<String> = Vec::new();
    for stage in pipeline(board) {
        build_stage(stage, runner, board, &env)?;
        env.extend(stage_exports(stage, board));
    }
    Ok(())
}

fn clone_stage(name: &str, runner: &SandboxRunner, board: &BoardConfig) -> Result<()> {
    match name {
        "opensbi" => opensbi::clone(runner, board),
        "uboot" => uboot::clone(runner, board),
        "grub" => grub::clone(runner, board),
        "syslinux" => syslinux::clone(runner, board),
        "tfa" => tfa::clone(runner, board),
        "rkbin" => rkbin::clone(runner, board),
        "amlogic-fip" => amlogic_fip::clone(runner, board),
        other => Err(unknown(other, board)),
    }
}

fn build_stage(
    name: &str,
    runner: &SandboxRunner,
    board: &BoardConfig,
    env: &[String],
) -> Result<()> {
    match name {
        "opensbi" => opensbi::build(runner, board, env),
        "uboot" => uboot::build(runner, board, env),
        "grub" => grub::build(runner, board, env),
        "syslinux" => syslinux::build(runner, board, env),
        "tfa" => tfa::build(runner, board, env),
        "rkbin" => rkbin::build(runner, board, env),
        "amlogic-fip" => amlogic_fip::build(runner, board, env),
        other => Err(unknown(other, board)),
    }
}

fn stage_exports(name: &str, board: &BoardConfig) -> Vec<String> {
    match name {
        "opensbi" => opensbi::exports(board),
        "uboot" => uboot::exports(board),
        "grub" => grub::exports(board),
        "syslinux" => syslinux::exports(board),
        "tfa" => tfa::exports(board),
        "rkbin" => rkbin::exports(board),
        "amlogic-fip" => amlogic_fip::exports(board),
        // Stage names are validated at board.conf load time.
        other => unreachable!("unvalidated BOOT_PIPELINE stage '{other}'"),
    }
}

fn unknown(name: &str, board: &BoardConfig) -> Error {
    Error::BoardConfigParse {
        file: board.name.clone(),
        msg: format!("unknown BOOT_PIPELINE stage '{name}'"),
    }
}
