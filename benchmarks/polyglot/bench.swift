import Foundation

func benchFibonacci() {
    func fib(_ n: Int) -> Int {
        if n < 2 { return n }
        return fib(n - 1) + fib(n - 2)
    }

    let start = CFAbsoluteTimeGetCurrent()
    let result = fib(40)
    let elapsed = Int((CFAbsoluteTimeGetCurrent() - start) * 1000)
    print("fibonacci:\(elapsed)")
    print("  checksum: \(result)")
}

func benchLoopOverhead() {
    let start = CFAbsoluteTimeGetCurrent()
    var checksum: UInt32 = 0x811c9dc5
    var i: UInt32 = 0
    while i < 100_000_000 {
        checksum = (checksum ^ i) &* 0x01000193
        i &+= 1
    }
    let elapsed = Int((CFAbsoluteTimeGetCurrent() - start) * 1000)
    print("loop_overhead:\(elapsed)")
    print("  checksum: \(checksum)")
}

func benchArrayWrite() {
    var arr = [Double](repeating: 0.0, count: 10_000_000)
    let start = CFAbsoluteTimeGetCurrent()
    for i in 0..<10_000_000 {
        arr[i] = Double(i)
    }
    let elapsed = Int((CFAbsoluteTimeGetCurrent() - start) * 1000)
    print("array_write:\(elapsed)")
    print("  checksum: \(Int(arr[9_999_999]))")
}

func benchArrayRead() {
    var arr = [Double](repeating: 0.0, count: 10_000_000)
    for i in 0..<10_000_000 {
        arr[i] = Double(i)
    }
    let start = CFAbsoluteTimeGetCurrent()
    var sum: Double = 0.0
    for i in 0..<10_000_000 {
        sum += arr[i]
    }
    let elapsed = Int((CFAbsoluteTimeGetCurrent() - start) * 1000)
    print("array_read:\(elapsed)")
    print("  checksum: \(Int(sum))")
}

func benchMathIntensive() {
    let start = CFAbsoluteTimeGetCurrent()
    var result: Double = 0.0
    for i in 1...50_000_000 {
        result += 1.0 / Double(i)
    }
    let elapsed = Int((CFAbsoluteTimeGetCurrent() - start) * 1000)
    print("math_intensive:\(elapsed)")
    print("  checksum: \(String(format: "%.6f", result))")
}

struct Point {
    var x: Double
    var y: Double
}

func benchObjectCreate() {
    let start = CFAbsoluteTimeGetCurrent()
    var sum: Double = 0.0
    for i in 0..<1_000_000 {
        let p = Point(x: Double(i), y: Double(i) * 2.0)
        sum += p.x + p.y
    }
    let elapsed = Int((CFAbsoluteTimeGetCurrent() - start) * 1000)
    print("object_create:\(elapsed)")
    print("  checksum: \(Int(sum))")
}

func benchNestedLoops() {
    let n = 3000
    var arr = [Double](repeating: 0.0, count: n * n)
    for i in 0..<(n * n) {
        arr[i] = Double(i)
    }
    let start = CFAbsoluteTimeGetCurrent()
    var sum: Double = 0.0
    for i in 0..<n {
        for j in 0..<n {
            sum += arr[i * n + j]
        }
    }
    let elapsed = Int((CFAbsoluteTimeGetCurrent() - start) * 1000)
    print("nested_loops:\(elapsed)")
    print("  checksum: \(Int(sum))")
}

func benchAccumulate() {
    let start = CFAbsoluteTimeGetCurrent()
    var sum: Double = 0.0
    for i in 0..<100_000_000 {
        sum += Double(i % 1000)
    }
    let elapsed = Int((CFAbsoluteTimeGetCurrent() - start) * 1000)
    print("accumulate:\(elapsed)")
    print("  checksum: \(Int(sum))")
}

// Floating-point loop with a sequential multiply-carry. Unlike
// benchLoopOverhead's integer carry, this adds two runtime array reads and
// exposes FP-contraction behavior.
func benchLoopDataDependent() {
    let N = 64
    let ITERATIONS = 100_000_000
    var seed: UInt64 = 42
    var x = [Double](repeating: 0.0, count: N)
    for i in 0..<N {
        seed = (seed &* 1103515245 &+ 12345) & 0x7FFFFFFF
        // [0.5, 1.0): contracts to a bounded fixed point. See bench.rs.
        x[i] = 0.5 + (Double(seed) / 2_147_483_647.0) * 0.5
    }
    let start = CFAbsoluteTimeGetCurrent()
    var sum: Double = 1.0
    for i in 0..<ITERATIONS {
        sum = sum * x[i & (N - 1)] + x[(i &* 7) & (N - 1)]
    }
    let elapsed = Int((CFAbsoluteTimeGetCurrent() - start) * 1000)
    print("loop_data_dependent:\(elapsed)")
    print("  checksum: \(sum)")
}

benchFibonacci()
benchLoopOverhead()
benchArrayWrite()
benchArrayRead()
benchMathIntensive()
benchObjectCreate()
benchNestedLoops()
benchAccumulate()
benchLoopDataDependent()
