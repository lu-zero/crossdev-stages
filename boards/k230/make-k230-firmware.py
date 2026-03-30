#!/usr/bin/env python3
"""Wrap a binary into the Canaan K230 firmware image format.

The K230 boot ROM loads firmware from a fixed SD card offset and
validates a 532-byte header before jumping to the payload.

Header layout (little-endian):
  0x000  4B  magic "K230"
  0x004  4B  length of (version + payload)
  0x008  4B  encryption type (0 = none)
  0x00C 32B  SHA-256 of (version + payload)
  0x02C 484B zero padding
  0x210  4B  version (0x00000000)
  0x214  ..  raw payload

References:
  - K230 boot ROM & firmware format analysis:
    https://dev.to/andelf/bare-metal-embedded-programming-on-k230-using-rust-4h4g
  - Bare-metal K230 reference implementation:
    https://github.com/andelf/k230-bare-metal
  - Rémi Denis-Courmont's K230 boot tools:
    https://code.videolan.org/Courmisch/k230-boot
"""

import argparse
import hashlib
import struct
import sys

MAGIC = b"K230"
HEADER_PAD = 484
VERSION = b"\x00\x00\x00\x00"


def make_firmware(payload):
    body = VERSION + payload
    header = struct.pack("<4sII", MAGIC, len(body), 0)  # magic, length, encryption
    header += hashlib.sha256(body).digest()
    header += bytes(HEADER_PAD)
    return header + body


def main():
    p = argparse.ArgumentParser(description="Wrap binary into K230 firmware format")
    p.add_argument("-i", required=True, help="input binary")
    p.add_argument("-o", required=True, help="output firmware image")
    args = p.parse_args()

    with open(args.i, "rb") as f:
        payload = f.read()

    with open(args.o, "wb") as f:
        f.write(make_firmware(payload))


if __name__ == "__main__":
    main()
