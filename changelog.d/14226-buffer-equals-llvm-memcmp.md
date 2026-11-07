Use LLVM's AArch64 SIMD equality-only `memcmp` lowering for proven fixed-size `Buffer.equals` calls up to 64 bytes. Calls without safe fixed-size view proofs keep the runtime path.
