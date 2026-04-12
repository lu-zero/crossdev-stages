#!/bin/bash
# Wrap a binary into the Canaan K230 firmware image format.
# Header: magic(4) + len(4) + enc(4) + sha256(32) + pad(484) + version(4) + payload
set -e

INPUT="" OUTPUT=""
while [[ $# -gt 0 ]]; do
    case "$1" in -i) INPUT="$2"; shift 2;; -o) OUTPUT="$2"; shift 2;; *) echo "Usage: $0 -i input -o output" >&2; exit 1;; esac
done
[ -z "$INPUT" ] || [ -z "$OUTPUT" ] && { echo "Usage: $0 -i input -o output" >&2; exit 1; }

TMP=$(mktemp -d)
trap "rm -rf $TMP" EXIT

# body = 4-byte version (0) + payload
dd if=/dev/zero bs=1 count=4 of="$TMP/body" 2>/dev/null
cat "$INPUT" >> "$TMP/body"

BODY_LEN=$(stat -c%s "$TMP/body")
HASH_HEX=$(sha256sum "$TMP/body" | cut -d' ' -f1)

# Write header as raw bytes
# magic
printf 'K230' > "$TMP/header"
# length (little-endian u32)
printf '%08x' "$BODY_LEN" | sed 's/\(..\)\(..\)\(..\)\(..\)/\4\3\2\1/' | xxd -r -p >> "$TMP/header"
# encryption type = 0
printf '\x00\x00\x00\x00' >> "$TMP/header"
# SHA-256 (32 bytes from hex)
echo -n "$HASH_HEX" | xxd -r -p >> "$TMP/header"
# 484 bytes zero padding
dd if=/dev/zero bs=1 count=484 >> "$TMP/header" 2>/dev/null

cat "$TMP/header" "$TMP/body" > "$OUTPUT"
