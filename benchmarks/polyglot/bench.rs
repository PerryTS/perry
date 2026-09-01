use std::hint::black_box;
use std::time::Instant;

fn fib(n: i64) -> i64 {
    if n < 2 {
        return n;
    }
    fib(n - 1) + fib(n - 2)
}

fn bench_fibonacci() {
    let start = Instant::now();
    let result = fib(black_box(40));
    let elapsed = start.elapsed().as_millis();
    println!("fibonacci:{}", elapsed);
    println!("  checksum: {}", result);
}

fn bench_loop_overhead() {
    let start = Instant::now();
    let mut checksum: u32 = 0x811c9dc5;
    for i in 0..100_000_000_u32 {
        checksum = (checksum ^ i).wrapping_mul(0x01000193);
    }
    let elapsed = start.elapsed().as_millis();
    println!("loop_overhead:{}", elapsed);
    println!("  checksum: {}", checksum);
}

fn bench_array_write() {
    let mut arr = vec![0.0_f64; 10_000_000];
    // suite/03_array_write.ts fills every slot BEFORE it calls Date.now(), so
    // its timed loop overwrites resident pages. `vec![0.0; N]` is calloc, whose
    // pages are mapped lazily -- without this pre-touch the Rust timed loop
    // additionally pays ~10M first-touch faults the TS timed loop does not,
    // and the column measures page-fault cost rather than store throughput.
    for slot in arr.iter_mut() {
        *slot = 0.0;
    }
    black_box(&arr);
    let start = Instant::now();
    for i in 0..10_000_000 {
        arr[i] = i as f64;
    }
    let elapsed = start.elapsed().as_millis();
    println!("array_write:{}", elapsed);
    println!("  checksum: {:.0}", arr[9_999_999]);
}

fn bench_array_read() {
    let mut arr = vec![0.0_f64; 10_000_000];
    for i in 0..10_000_000 {
        arr[i] = i as f64;
    }
    let start = Instant::now();
    let mut sum: f64 = 0.0;
    for i in 0..10_000_000 {
        sum += arr[i];
    }
    let elapsed = start.elapsed().as_millis();
    println!("array_read:{}", elapsed);
    println!("  checksum: {:.0}", sum);
}

fn bench_math_intensive() {
    let start = Instant::now();
    let mut result: f64 = 0.0;
    for i in 1..=50_000_000 {
        result += 1.0 / i as f64;
    }
    let elapsed = start.elapsed().as_millis();
    println!("math_intensive:{}", elapsed);
    println!("  checksum: {:.6}", result);
}

struct Point {
    x: f64,
    y: f64,
}

fn bench_object_create() {
    let start = Instant::now();
    let mut sum: f64 = 0.0;
    for i in 0..1_000_000 {
        // Without the barrier LLVM proves the struct never escapes and removes
        // the allocation entirely, reporting 0 ms for a loop that never ran.
        let p = black_box(Point {
            x: i as f64,
            y: i as f64 * 2.0,
        });
        sum += p.x + p.y;
    }
    let elapsed = start.elapsed().as_millis();
    println!("object_create:{}", elapsed);
    println!("  checksum: {:.0}", sum);
}

fn bench_nested_loops() {
    let n = 3000;
    let mut arr = vec![0.0_f64; n * n];
    for i in 0..(n * n) {
        arr[i] = i as f64;
    }
    let start = Instant::now();
    let mut sum: f64 = 0.0;
    for i in 0..n {
        for j in 0..n {
            sum += arr[i * n + j];
        }
    }
    let elapsed = start.elapsed().as_millis();
    println!("nested_loops:{}", elapsed);
    println!("  checksum: {:.0}", sum);
}

fn bench_accumulate() {
    let start = Instant::now();
    let mut sum: f64 = 0.0;
    for i in 0..100_000_000_i64 {
        sum += (i % 1000) as f64;
    }
    let elapsed = start.elapsed().as_millis();
    println!("accumulate:{}", elapsed);
    println!("  checksum: {:.0}", sum);
}

fn bench_loop_data_dependent() {
    // Verified non-foldable on rustc 1.94.1 stable as of 2026-04-25.
    // `rustc -O -C codegen-units=1 --emit=asm bench.rs` produces this
    // exact loop body (label LBB5_41 in the dump):
    //
    //     LBB5_41:
    //       and  x9,  x8, #0x3f            // i & 63
    //       lsl  w10, w8, #3               // i*8
    //       sub  w10, w10, w8              // i*8 - i = i*7
    //       add  x8,  x8, #1               // i++
    //       and  x10, x10, #0x3f           // (i*7) & 63
    //       ldr  d1,  [x19, x9,  lsl #3]   // load x[i & 63]
    //       fmul d0,  d0, d1               // sum *= x[i & 63]
    //       ldr  d1,  [x19, x10, lsl #3]   // load x[(i*7) & 63]
    //       fadd d0,  d0, d1               // sum += x[(i*7) & 63]
    //       cmp  x8,  x22                  // i < ITERATIONS?
    //       b.ne LBB5_41
    //
    // Scalar loop with two array loads, one fmul, one fadd, sequential
    // carry through d0 (sum). Not vectorized, not unrolled, not folded.
    // The sequential dependency on `sum` defeats both reassoc and the
    // vectorizer; the array reads defeat constant propagation past the
    // loop boundary.
    //
    // FP-contract caveat (matters for cross-language comparison): the
    // expression `sum * a + b` can be lowered as either two instructions
    // (fmul + fadd, 2 IEEE-754 roundings) or a single fused FMADDD
    // (one rounding). The fused form has shorter per-iteration latency
    // (~4 cycles vs ~6-8 on Apple silicon) and changes the result by at
    // most 0.5 ULP per iteration. LLVM's default does NOT contract —
    // observable: rustc 1.94.1 default `-O` emits the fmul+fadd shape
    // shown above, and `-C target-cpu=native` does not change this.
    // Apple Clang `-O2+` (and therefore Apple `g++ -O3`) DOES contract
    // to FMADDD by default — verified by running `g++ -O3 bench.cpp`
    // and inspecting the loop body. Go always contracts. Swift `-O`,
    // V8 (Node), JSC (Bun), Java HotSpot without `-XX:+UseFMA`, and
    // Perry's default codegen all stay in the no-contract pack. So
    // the kernel divides the field into two clusters: FMA-contract
    // (~128 ms here: Go, Apple Clang -O3) and no-contract (~225 ms:
    // Rust, Swift, Perry, Node, Bun, Java). Both clusters preserve
    // the dependency chain on `sum`; the win is one ISA-level fusion,
    // not a fold or a vectorize.
    //
    // This is the floating-point companion to bench_loop_overhead. Both
    // kernels have a sequential carry; this one additionally performs two
    // runtime array loads and surfaces FP contraction as a secondary axis.
    const N: usize = 64;
    const ITERATIONS: u64 = 100_000_000;
    let mut seed: u64 = 42;
    let mut x = vec![0.0_f64; N];
    for i in 0..N {
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345) & 0x7FFF_FFFF;
        // Range [0.5, 1.0): every multiplicand x < 1 so the
        // multiplicative chain strictly contracts to a bounded
        // fixed point. Values centered on 1.0 (e.g. [0.99, 1.01])
        // overflow to Infinity because tiny mean-drift compounds
        // geometrically over 100M iterations.
        x[i] = 0.5 + (seed as f64 / 2_147_483_647.0) * 0.5;
    }
    let start = Instant::now();
    let mut sum: f64 = 1.0;
    for i in 0..ITERATIONS {
        let i_us = i as usize;
        sum = sum * x[i_us & (N - 1)] + x[(i_us * 7) & (N - 1)];
    }
    let elapsed = start.elapsed().as_millis();
    println!("loop_data_dependent:{}", elapsed);
    println!("  checksum: {:.6}", sum);
}

fn main() {
    bench_fibonacci();
    bench_loop_overhead();
    bench_array_write();
    bench_array_read();
    bench_math_intensive();
    bench_object_create();
    bench_nested_loops();
    bench_accumulate();
    bench_loop_data_dependent();
}
