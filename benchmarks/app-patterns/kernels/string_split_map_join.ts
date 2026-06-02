// Pattern: parse CSV-shaped lines with split + map + filter + join.
// Models any line-oriented log/data parser.

// Build 50k synthetic CSV lines.
const N = 50_000;
const lines: string[] = [];
for (let i = 0; i < N; i++) {
    lines.push(i + ",user_" + i + ",user_" + i + "@example.com," + (i * 1.5).toFixed(2));
}

// Process: split each line, parse fields, filter, re-join.
const out: string[] = [];
for (let k = 0; k < lines.length; k++) {
    const parts = lines[k].split(",");
    const id = +parts[0];
    if (id % 3 === 0) continue; // filter: keep 2/3 of rows
    const upper = parts[1].toUpperCase();
    out.push([upper, parts[2], parts[3]].join("|"));
}

let total = 0;
for (let i = 0; i < out.length; i++) total += out[i].length;
console.log("checksum:", total, "kept:", out.length);
