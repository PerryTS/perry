const std = @import("std");

// Zig 0.15 + macOS 26 linker workaround: use extern write(2) for stdout.
// Compile: zig build-obj bench.zig -OReleaseFast && cc bench.o -o bench_zig -lSystem
extern "c" fn write(fd: c_int, buf: [*]const u8, count: usize) isize;

fn print(comptime fmt: []const u8, args: anytype) void {
    var buf: [512]u8 = undefined;
    const result = std.fmt.bufPrint(&buf, fmt, args) catch return;
    _ = write(1, result.ptr, result.len);
}

fn fib(n: i32) i32 {
    if (n < 2) return n;
    return fib(n - 1) + fib(n - 2);
}

fn benchFibonacci() void {
    const start = std.time.milliTimestamp();
    const result = fib(40);
    const elapsed = std.time.milliTimestamp() - start;
    print("fibonacci:{d}\n", .{elapsed});
    print("  checksum: {d}\n", .{result});
}

fn benchLoopOverhead() void {
    const start = std.time.milliTimestamp();
    var checksum: u32 = 0x811c9dc5;
    var i: u32 = 0;
    while (i < 100_000_000) : (i +%= 1) {
        checksum = (checksum ^ i) *% 0x01000193;
    }
    const elapsed = std.time.milliTimestamp() - start;
    print("loop_overhead:{d}\n", .{elapsed});
    print("  checksum: {d}\n", .{checksum});
}

fn benchArrayWrite() void {
    const allocator = std.heap.page_allocator;
    const arr = allocator.alloc(f64, 10_000_000) catch return;
    defer allocator.free(arr);

    const start = std.time.milliTimestamp();
    for (0..10_000_000) |i| {
        arr[i] = @as(f64, @floatFromInt(i));
    }
    const elapsed = std.time.milliTimestamp() - start;
    print("array_write:{d}\n", .{elapsed});
    print("  checksum: {d:.0}\n", .{arr[9_999_999]});
}

fn benchArrayRead() void {
    const allocator = std.heap.page_allocator;
    const arr = allocator.alloc(f64, 10_000_000) catch return;
    defer allocator.free(arr);

    for (0..10_000_000) |i| {
        arr[i] = @as(f64, @floatFromInt(i));
    }

    const start = std.time.milliTimestamp();
    var sum: f64 = 0.0;
    for (0..10_000_000) |i| {
        sum += arr[i];
    }
    const elapsed = std.time.milliTimestamp() - start;
    print("array_read:{d}\n", .{elapsed});
    print("  checksum: {d:.0}\n", .{sum});
}

fn benchMathIntensive() void {
    const start = std.time.milliTimestamp();
    var result: f64 = 0.0;
    var i: i32 = 1;
    while (i <= 50_000_000) : (i += 1) {
        result += 1.0 / @as(f64, @floatFromInt(i));
    }
    const elapsed = std.time.milliTimestamp() - start;
    print("math_intensive:{d}\n", .{elapsed});
    print("  checksum: {d:.6}\n", .{result});
}

const Point = struct {
    x: f64,
    y: f64,
};

fn benchObjectCreate() void {
    const start = std.time.milliTimestamp();
    var sum: f64 = 0.0;
    var i: i32 = 0;
    while (i < 1_000_000) : (i += 1) {
        const fi = @as(f64, @floatFromInt(i));
        const p = Point{ .x = fi, .y = fi * 2.0 };
        sum += p.x + p.y;
    }
    const elapsed = std.time.milliTimestamp() - start;
    print("object_create:{d}\n", .{elapsed});
    print("  checksum: {d:.0}\n", .{sum});
}

fn benchNestedLoops() void {
    const n: usize = 3000;
    const allocator = std.heap.page_allocator;
    const arr = allocator.alloc(f64, n * n) catch return;
    defer allocator.free(arr);

    for (0..n * n) |i| {
        arr[i] = @as(f64, @floatFromInt(i));
    }

    const start = std.time.milliTimestamp();
    var sum: f64 = 0.0;
    for (0..n) |i| {
        for (0..n) |j| {
            sum += arr[i * n + j];
        }
    }
    const elapsed = std.time.milliTimestamp() - start;
    print("nested_loops:{d}\n", .{elapsed});
    print("  checksum: {d:.0}\n", .{sum});
}

fn benchAccumulate() void {
    const start = std.time.milliTimestamp();
    var sum: f64 = 0.0;
    var i: i64 = 0;
    while (i < 100_000_000) : (i += 1) {
        sum += @as(f64, @floatFromInt(@mod(i, 1000)));
    }
    const elapsed = std.time.milliTimestamp() - start;
    print("accumulate:{d}\n", .{elapsed});
    print("  checksum: {d:.0}\n", .{sum});
}

pub fn main() void {
    benchFibonacci();
    benchLoopOverhead();
    benchArrayWrite();
    benchArrayRead();
    benchMathIntensive();
    benchObjectCreate();
    benchNestedLoops();
    benchAccumulate();
}
