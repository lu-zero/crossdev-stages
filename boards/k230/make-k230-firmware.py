#! /usr/bin/python3
# -*- coding: utf-8 -*-
#
# This script wraps a file into the Canaan K230 firmware image format.
# This is necessary at least for SPL which is loaded by the boot ROM.
#
# The format consists of a proprietary 532-byte firmware header followed by the
# raw image payload (possibly encrypted). The header is laid out like this:
# * 4-byte "K230" magic number,
# * 4-byte length in byte of the payload plus four (native endian),
# * 4-byte encryption type enumeration, always 0 for unencrypted format,
# * Then in the case of the unencrypted format:
#   32-byte SHA-256 hash of the four zero bytes then the payload,
# * 484 bytes of zero padding (reserved for encrypted formats).
# The length field and hash calculation starts count this point:
# * 4-byte version number: currently always 0,
# * the raw image payload.

import os
import getopt
import sys
import hashlib

if __name__ == "__main__":
    inputfile = ''
    outputfile = ''
    try:
        opts, args = getopt.getopt(sys.argv[1:], 'hi:o:asn', ['ifile=', 'ofile='])
    except getopt.GetoptError:
        print('make-k230-firmware -i <inputfile> -o <outputfile>')
        sys.exit(2)
    for opt, arg in opts:
        if opt == '-h':
            print('make-k230-firmware -i <inputfile> -o <outputfile>')
            sys.exit()
        elif opt in ('-i', '--ifile'):
            inputfile = arg
        elif opt in ('-o', '--ofile'):
            outputfile = arg

    input = open(inputfile, 'rb')
    patch_otp = open(outputfile, 'wb')
    input_data = b'\x00\x00\x00\x00' + input.read()
    magic = b'\x4b\x32\x33\x30'
    patch_otp.write(magic)
    message = input_data
    data_len = len(input_data)
    data_len_byte = data_len.to_bytes(4, byteorder=sys.byteorder, signed=True)
    patch_otp.write(data_len_byte)
    encrypto_type = 0
    encrypto_type_b = encrypto_type.to_bytes(4, byteorder=sys.byteorder, signed=True)
    patch_otp.write(encrypto_type_b)
    hash_data = hashlib.sha256(message).digest()
    patch_otp.write(hash_data)
    patch_otp.write(bytes(484))
    patch_otp.write(message)
    patch_otp.close()
    input.close()
