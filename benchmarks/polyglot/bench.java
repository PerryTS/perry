public class bench {

    static int fib(int n) {
        if (n < 2) return n;
        return fib(n - 1) + fib(n - 2);
    }

    static void benchFibonacci() {
        long start = System.currentTimeMillis();
        int result = fib(40);
        long elapsed = System.currentTimeMillis() - start;
        System.out.println("fibonacci:" + elapsed);
        System.out.println("  checksum: " + result);
    }

    static void benchLoopOverhead() {
        long start = System.currentTimeMillis();
        int checksum = 0x811c9dc5;
        for (int i = 0; i < 100_000_000; i++) {
            checksum = (checksum ^ i) * 0x01000193;
        }
        long elapsed = System.currentTimeMillis() - start;
        System.out.println("loop_overhead:" + elapsed);
        System.out.println("  checksum: " + Integer.toUnsignedLong(checksum));
    }

    static void benchArrayWrite() {
        double[] arr = new double[10_000_000];
        long start = System.currentTimeMillis();
        for (int i = 0; i < 10_000_000; i++) {
            arr[i] = (double) i;
        }
        long elapsed = System.currentTimeMillis() - start;
        System.out.println("array_write:" + elapsed);
        System.out.printf("  checksum: %.0f%n", arr[9_999_999]);
    }

    static void benchArrayRead() {
        double[] arr = new double[10_000_000];
        for (int i = 0; i < 10_000_000; i++) {
            arr[i] = (double) i;
        }
        long start = System.currentTimeMillis();
        double sum = 0.0;
        for (int i = 0; i < 10_000_000; i++) {
            sum += arr[i];
        }
        long elapsed = System.currentTimeMillis() - start;
        System.out.println("array_read:" + elapsed);
        System.out.printf("  checksum: %.0f%n", sum);
    }

    static void benchMathIntensive() {
        long start = System.currentTimeMillis();
        double result = 0.0;
        for (int i = 1; i <= 50_000_000; i++) {
            result += 1.0 / (double) i;
        }
        long elapsed = System.currentTimeMillis() - start;
        System.out.println("math_intensive:" + elapsed);
        System.out.printf("  checksum: %.6f%n", result);
    }

    static class Point {
        double x;
        double y;

        Point(double x, double y) {
            this.x = x;
            this.y = y;
        }
    }

    static void benchObjectCreate() {
        long start = System.currentTimeMillis();
        double sum = 0.0;
        for (int i = 0; i < 1_000_000; i++) {
            Point p = new Point((double) i, (double) i * 2.0);
            sum += p.x + p.y;
        }
        long elapsed = System.currentTimeMillis() - start;
        System.out.println("object_create:" + elapsed);
        System.out.printf("  checksum: %.0f%n", sum);
    }

    static void benchNestedLoops() {
        int n = 3000;
        double[] arr = new double[n * n];
        for (int i = 0; i < n * n; i++) {
            arr[i] = (double) i;
        }
        long start = System.currentTimeMillis();
        double sum = 0.0;
        for (int i = 0; i < n; i++) {
            for (int j = 0; j < n; j++) {
                sum += arr[i * n + j];
            }
        }
        long elapsed = System.currentTimeMillis() - start;
        System.out.println("nested_loops:" + elapsed);
        System.out.printf("  checksum: %.0f%n", sum);
    }

    static void benchAccumulate() {
        long start = System.currentTimeMillis();
        double sum = 0.0;
        for (int i = 0; i < 100_000_000; i++) {
            sum += (double) (i % 1000);
        }
        long elapsed = System.currentTimeMillis() - start;
        System.out.println("accumulate:" + elapsed);
        System.out.printf("  checksum: %.0f%n", sum);
    }

    // Floating-point loop with a sequential multiply-carry. Unlike
    // benchLoopOverhead's integer carry, this adds two runtime array reads and
    // exposes FP-contraction behavior.
    static void benchLoopDataDependent() {
        final int N = 64;
        final long ITERATIONS = 100_000_000L;
        long seed = 42;
        double[] x = new double[N];
        for (int i = 0; i < N; i++) {
            seed = (seed * 1103515245L + 12345L) & 0x7FFFFFFFL;
            // [0.5, 1.0): contracts to a bounded fixed point. See bench.rs.
            x[i] = 0.5 + ((double) seed / 2_147_483_647.0) * 0.5;
        }
        long start = System.currentTimeMillis();
        double sum = 1.0;
        for (long i = 0; i < ITERATIONS; i++) {
            sum = sum * x[(int)(i & (N - 1))] + x[(int)((i * 7) & (N - 1))];
        }
        long elapsed = System.currentTimeMillis() - start;
        System.out.println("loop_data_dependent:" + elapsed);
        System.out.printf("  checksum: %.6f%n", sum);
    }

    public static void main(String[] args) {
        benchFibonacci();
        benchLoopOverhead();
        benchArrayWrite();
        benchArrayRead();
        benchMathIntensive();
        benchObjectCreate();
        benchNestedLoops();
        benchAccumulate();
        benchLoopDataDependent();
    }
}
