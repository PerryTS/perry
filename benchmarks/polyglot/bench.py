import time


def bench_fibonacci():
    def fib(n):
        if n < 2:
            return n
        return fib(n - 1) + fib(n - 2)

    start = time.monotonic()
    result = fib(40)
    elapsed = int((time.monotonic() - start) * 1000)
    print(f"fibonacci:{elapsed}")
    print(f"  checksum: {result}")


def bench_loop_overhead():
    start = time.monotonic()
    checksum = 0x811C9DC5
    for i in range(100_000_000):
        checksum = ((checksum ^ i) * 0x01000193) & 0xFFFFFFFF
    elapsed = int((time.monotonic() - start) * 1000)
    print(f"loop_overhead:{elapsed}")
    print(f"  checksum: {checksum}")


def bench_array_write():
    arr = [0.0] * 10_000_000
    start = time.monotonic()
    for i in range(10_000_000):
        arr[i] = float(i)
    elapsed = int((time.monotonic() - start) * 1000)
    print(f"array_write:{elapsed}")
    print(f"  checksum: {arr[9_999_999]:.0f}")


def bench_array_read():
    arr = [float(i) for i in range(10_000_000)]
    start = time.monotonic()
    sum_val = 0.0
    for i in range(10_000_000):
        sum_val += arr[i]
    elapsed = int((time.monotonic() - start) * 1000)
    print(f"array_read:{elapsed}")
    print(f"  checksum: {sum_val:.0f}")


def bench_math_intensive():
    start = time.monotonic()
    result = 0.0
    for i in range(1, 50_000_001):
        result += 1.0 / i
    elapsed = int((time.monotonic() - start) * 1000)
    print(f"math_intensive:{elapsed}")
    print(f"  checksum: {result:.6f}")


class Point:
    __slots__ = ('x', 'y')

    def __init__(self, x, y):
        self.x = x
        self.y = y


def bench_object_create():
    start = time.monotonic()
    sum_val = 0.0
    for i in range(1_000_000):
        p = Point(float(i), float(i) * 2.0)
        sum_val += p.x + p.y
    elapsed = int((time.monotonic() - start) * 1000)
    print(f"object_create:{elapsed}")
    print(f"  checksum: {sum_val:.0f}")


def bench_nested_loops():
    n = 3000
    arr = [float(i) for i in range(n * n)]
    start = time.monotonic()
    sum_val = 0.0
    for i in range(n):
        for j in range(n):
            sum_val += arr[i * n + j]
    elapsed = int((time.monotonic() - start) * 1000)
    print(f"nested_loops:{elapsed}")
    print(f"  checksum: {sum_val:.0f}")


def bench_accumulate():
    start = time.monotonic()
    sum_val = 0.0
    for i in range(100_000_000):
        sum_val += i % 1000
    elapsed = int((time.monotonic() - start) * 1000)
    print(f"accumulate:{elapsed}")
    print(f"  checksum: {sum_val:.0f}")


def bench_loop_data_dependent():
    # Floating-point loop with a sequential multiply-carry. Unlike
    # bench_loop_overhead's integer carry, this adds two runtime array reads
    # and exposes FP-contraction behavior.
    N = 64
    ITERATIONS = 100_000_000
    seed = 42
    x = [0.0] * N
    for i in range(N):
        seed = (seed * 1103515245 + 12345) & 0x7FFFFFFF
        # [0.5, 1.0): contracts to a bounded fixed point. See bench.rs.
        x[i] = 0.5 + (seed / 2_147_483_647.0) * 0.5
    start = time.monotonic()
    sum_val = 1.0
    for i in range(ITERATIONS):
        sum_val = sum_val * x[i & (N - 1)] + x[(i * 7) & (N - 1)]
    elapsed = int((time.monotonic() - start) * 1000)
    print(f"loop_data_dependent:{elapsed}")
    print(f"  checksum: {sum_val:.6f}")


if __name__ == "__main__":
    bench_fibonacci()
    bench_loop_overhead()
    bench_array_write()
    bench_array_read()
    bench_math_intensive()
    bench_object_create()
    bench_nested_loops()
    bench_accumulate()
    bench_loop_data_dependent()
