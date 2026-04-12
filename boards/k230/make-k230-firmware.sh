#!/bin/bash
# Wrap a binary into the Canaan K230 firmware image format.
# Header: magic(4) + len(4) + enc(4) + sha256(32) + pad(484) + version(4) + payload
set -e

INPUT="" OUTPUT=""
while [[ $# -gt 0 ]]; do
    case "$1" in -i) INPUT="$2"; shift 2;; -o) OUTPUT="$2"; shift 2;; *) echo "Usage: $0 -i input -o output" >&2; exit 1;; esac
done
[ -z "$INPUT" ] || [ -z "$OUTPUT" ] && { echo "Usage: $0 -i input -o output" >&2; exit 1; }

# hex string to raw bytes via printf
hex2bin() { local hex="$1"; local i; for ((i=0; i<${#hex}; i+=2)); do printf "\\x${hex:$i:2}"; done; }

TMP=$(mktemp -d)
trap "rm -rf $TMP" EXIT

# body = 4-byte version (0) + payload
dd if=/dev/zero bs=1 count=4 of="$TMP/body" 2>/dev/null
cat "$INPUT" >> "$TMP/body"

BODY_LEN=$(stat -c%s "$TMP/body")
HASH_HEX=$(sha256sum "$TMP/body" | cut -d' ' -f1)

# length as little-endian hex
LE_HEX=$(printf '%08x' "$BODY_LEN" | sed 's/\(..\)\(..\)\(..\)\(..\)/\4\3\2\1/')

{
    printf 'K230'
    hex2bin "$LE_HEX"
    printf '\x00\x00\x00\x00'
    hex2bin "$HASH_HEX"
    dd if=/dev/zero bs=1 count=484 2>/dev/null
} > "$TMP/header"

cat "$TMP/header" "$TMP/body" > "$OUTPUT"
