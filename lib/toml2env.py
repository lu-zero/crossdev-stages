#!/usr/bin/env python3
"""Read a board TOML config and output shell-sourceable environment variables."""

import shlex
import sys
import tomllib


def quote(s):
    return shlex.quote(str(s))


def shell_array(items):
    return "(" + " ".join(quote(i) for i in items) + ")"


def main():
    if len(sys.argv) != 2:
        print(f"Usage: {sys.argv[0]} <board.toml>", file=sys.stderr)
        sys.exit(1)

    with open(sys.argv[1], "rb") as f:
        data = tomllib.load(f)

    board = data.get("board", {})
    repos = data.get("repos", {})
    build = data.get("build", {})
    packages = data.get("packages", {})
    workarounds = data.get("workarounds", {})
    image = data.get("image", {})

    # Board basics
    print(f"BOARD_NAME={quote(board.get('name', ''))}")
    print(f"BOARD_ARCH={quote(board.get('arch', ''))}")
    print(f"BOARD_CFLAGS={quote(board.get('cflags', ''))}")

    # Repos as parallel arrays
    names = list(repos.keys())
    print(f"BOARD_REPO_NAMES={shell_array(names)}")
    print(f"BOARD_REPO_URLS={shell_array(repos[n]['url'] for n in names)}")
    print(f"BOARD_REPO_TAGS={shell_array(repos[n]['tag'] for n in names)}")

    # Build config
    print(f"BOARD_BUILD_STEPS={shell_array(build.get('steps', []))}")
    print(f"BOARD_LINUX_DEFCONFIG={quote(build.get('linux_defconfig', ''))}")
    print(f"BOARD_LINUX_EXTRA_TARGETS={shell_array(build.get('linux_extra_targets', []))}")
    print(f"BOARD_UBOOT_DEFCONFIG={quote(build.get('uboot_defconfig', ''))}")
    print(f"BOARD_OPENSBI_PLATFORM={quote(build.get('opensbi_platform', 'generic'))}")
    print(f"BOARD_OPENSBI_EXTRA={quote(build.get('opensbi_extra', ''))}")
    print(f"BOARD_OPENSBI_BINARY={quote(build.get('opensbi_binary', 'fw_dynamic.bin'))}")

    # Package lists
    print(f"BOARD_PACKAGES_BOOT={shell_array(packages.get('boot', []))}")
    print(f"BOARD_PACKAGES_EXTRA={shell_array(packages.get('extra', []))}")

    # Per-package CFLAGS workarounds
    pkg_cflags = workarounds.get("package_cflags", {})
    print(f"BOARD_WORKAROUND_PKGS={shell_array(pkg_cflags.keys())}")
    print(f"BOARD_WORKAROUND_CFLAGS={shell_array(pkg_cflags.values())}")

    # Image
    print(f"BOARD_IMAGE_NAME={quote(image.get('name', ''))}")


if __name__ == "__main__":
    main()
