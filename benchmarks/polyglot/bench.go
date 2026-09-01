package main

import (
	"fmt"
	"time"
)

func benchFibonacci() {
	var fib func(n int) int
	fib = func(n int) int {
		if n < 2 {
			return n
		}
		return fib(n-1) + fib(n-2)
	}

	start := time.Now()
	result := fib(40)
	elapsed := time.Since(start).Milliseconds()
	fmt.Printf("fibonacci:%d\n", elapsed)
	fmt.Printf("  checksum: %d\n", result)
}

func benchLoopOverhead() {
	start := time.Now()
	checksum := uint32(0x811c9dc5)
	for i := uint32(0); i < 100_000_000; i++ {
		checksum = (checksum ^ i) * 0x01000193
	}
	elapsed := time.Since(start).Milliseconds()
	fmt.Printf("loop_overhead:%d\n", elapsed)
	fmt.Printf("  checksum: %d\n", checksum)
}

func benchArrayWrite() {
	arr := make([]float64, 10_000_000)
	start := time.Now()
	for i := 0; i < 10_000_000; i++ {
		arr[i] = float64(i)
	}
	elapsed := time.Since(start).Milliseconds()
	fmt.Printf("array_write:%d\n", elapsed)
	fmt.Printf("  checksum: %.0f\n", arr[9_999_999])
}

func benchArrayRead() {
	arr := make([]float64, 10_000_000)
	for i := 0; i < 10_000_000; i++ {
		arr[i] = float64(i)
	}
	start := time.Now()
	sum := 0.0
	for i := 0; i < 10_000_000; i++ {
		sum += arr[i]
	}
	elapsed := time.Since(start).Milliseconds()
	fmt.Printf("array_read:%d\n", elapsed)
	fmt.Printf("  checksum: %.0f\n", sum)
}

func benchMathIntensive() {
	start := time.Now()
	result := 0.0
	for i := 1; i <= 50_000_000; i++ {
		result += 1.0 / float64(i)
	}
	elapsed := time.Since(start).Milliseconds()
	fmt.Printf("math_intensive:%d\n", elapsed)
	fmt.Printf("  checksum: %.6f\n", result)
}

type Point struct {
	x float64
	y float64
}

func benchObjectCreate() {
	start := time.Now()
	sum := 0.0
	for i := 0; i < 1_000_000; i++ {
		p := Point{x: float64(i), y: float64(i) * 2.0}
		sum += p.x + p.y
	}
	elapsed := time.Since(start).Milliseconds()
	fmt.Printf("object_create:%d\n", elapsed)
	fmt.Printf("  checksum: %.0f\n", sum)
}

func benchNestedLoops() {
	n := 3000
	arr := make([]float64, n*n)
	for i := 0; i < n*n; i++ {
		arr[i] = float64(i)
	}
	start := time.Now()
	sum := 0.0
	for i := 0; i < n; i++ {
		for j := 0; j < n; j++ {
			sum += arr[i*n+j]
		}
	}
	elapsed := time.Since(start).Milliseconds()
	fmt.Printf("nested_loops:%d\n", elapsed)
	fmt.Printf("  checksum: %.0f\n", sum)
}

func benchAccumulate() {
	start := time.Now()
	sum := 0.0
	for i := 0; i < 100_000_000; i++ {
		sum += float64(i % 1000)
	}
	elapsed := time.Since(start).Milliseconds()
	fmt.Printf("accumulate:%d\n", elapsed)
	fmt.Printf("  checksum: %.0f\n", sum)
}

// Floating-point loop with a sequential multiply-carry. Unlike
// benchLoopOverhead's integer carry, this adds two runtime array reads and
// exposes FP-contraction behavior. Go has no fast-math flag.
func benchLoopDataDependent() {
	const N = 64
	const ITERATIONS = 100_000_000
	seed := uint64(42)
	x := make([]float64, N)
	for i := 0; i < N; i++ {
		seed = (seed*1103515245 + 12345) & 0x7FFFFFFF
		// [0.5, 1.0): contracts to a bounded fixed point. See bench.rs.
		x[i] = 0.5 + (float64(seed)/2_147_483_647.0)*0.5
	}
	start := time.Now()
	sum := 1.0
	for i := 0; i < ITERATIONS; i++ {
		sum = sum*x[i&(N-1)] + x[(i*7)&(N-1)]
	}
	elapsed := time.Since(start).Milliseconds()
	fmt.Printf("loop_data_dependent:%d\n", elapsed)
	fmt.Printf("  checksum: %.6f\n", sum)
}

func main() {
	benchFibonacci()
	benchLoopOverhead()
	benchArrayWrite()
	benchArrayRead()
	benchMathIntensive()
	benchObjectCreate()
	benchNestedLoops()
	benchAccumulate()
	benchLoopDataDependent()
}
