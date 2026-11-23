`Math.exp` returns Node's result bit for bit. It used the platform libm, which
differs from V8 in the last bit for about 5% of inputs; it now runs V8's
fdlibm-derived algorithm, fused the way V8's arm64 build fuses it (arm64 and
x86-64 Node themselves differ for about 1 input in 4000).
