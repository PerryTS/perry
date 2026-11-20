export function summarize(values) {
    if (values.length === 0)
        throw new RangeError("no values");
    const sorted = [...values].sort((a, b) => a - b);
    const n = sorted.length;
    const mean = sorted.reduce((s, v) => s + v, 0) / n;
    const variance = sorted.reduce((s, v) => s + (v - mean) ** 2, 0) / n;
    const median = n % 2 ? sorted[(n - 1) / 2] : (sorted[n / 2 - 1] + sorted[n / 2]) / 2;
    return { count: n, min: sorted[0], max: sorted[n - 1], mean, median, stddev: Math.sqrt(variance) };
}
export function groupBy(items, key) {
    const out = new Map();
    for (const item of items) {
        const k = key(item);
        const bucket = out.get(k);
        if (bucket)
            bucket.push(item);
        else
            out.set(k, [item]);
    }
    return out;
}
