# K230 Heterogeneous Dual-Core Architecture

## Hardware

The Canaan K230 has two RISC-V cores with different capabilities:

- **C908 (big core)**: RV64GCV, vector extension (VLEN=128), higher performance
- **C906 (little core)**: RV64GC, no vector extension

## Problem

Compiling with `-march=rv64gcv_zvl128b` enables vector instructions. If the
scheduler runs vector code on the C906 (little core), it triggers an illegal
instruction trap.

## Options

1. **Vector-enabled, big core only** (current choice)
   - Compile with `rv64gcv_zvl128b`
   - Only use the C908 core for Linux
   - Wastes the C906, but gets full vector performance
   - Best for: crypto benchmarks, compute-heavy workloads

2. **No vector, both cores**
   - Compile with `rv64gc`
   - Full SMP with both cores
   - No vector performance
   - Best for: general-purpose server, I/O-heavy workloads

3. **Manual CPU affinity**
   - Compile with `rv64gc` for general code
   - Use `taskset` to pin vector-specific binaries to the big core
   - Requires per-application configuration
   - Best for: mixed workloads

4. **Kernel-level heterogeneous scheduling** (theoretical)
   - Kernel traps illegal instruction on C906, migrates to C908
   - Extremely high overhead per trap
   - RISC-V lacks standardized big.LITTLE scheduling (unlike ARM EAS)
   - Per-hart capability detection exists (`/proc/cpuinfo` shows ISA per core)
   - Not practical today

## Current Decision

Using option 1: `BOARD_CFLAGS="-O3 -march=rv64gcv_zvl128b -pipe"`

This enables the RISC-V vector crypto implementations (Keccak, Kyber,
Dilithium from the PQRV project) to run at full speed on the C908 core.

## References

- K230 hardware: dual C908+C906, Canaan
- PQRV crypto implementations: https://github.com/Ji-Peng/PQRV
- RISC-V vector crypto paper: https://eprint.iacr.org/2024/1515.pdf
- linux-riscv vector crypto patches: https://lists.infradead.org/pipermail/linux-riscv/2026-February/085543.html
