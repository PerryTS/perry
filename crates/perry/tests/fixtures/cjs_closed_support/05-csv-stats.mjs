// CSV stats: group rows by a column and summarize a numeric column.
//   csv-stats sales.csv region amount
import { readFileSync } from "fs";
import { summarize, groupBy } from "./stats";
function parseCsv(text) {
    const [header, ...lines] = text.trim().split(/\r?\n/);
    const cols = header.split(",").map((c) => c.trim());
    return lines.map((line) => {
        const cells = line.split(",");
        return Object.fromEntries(cols.map((c, i) => [c, (cells[i] ?? "").trim()]));
    });
}
const fmt = (n) => n.toFixed(2).padStart(10);
const [file, groupCol, valueCol] = process.argv.slice(2);
if (!file || !groupCol || !valueCol) {
    console.error("usage: csv-stats <file.csv> <group-column> <value-column>");
    process.exit(1);
}
const rows = parseCsv(readFileSync(file, "utf8"));
const groups = groupBy(rows, (r) => r[groupCol]);
const results = [];
for (const [name, members] of groups) {
    const nums = members.map((r) => parseFloat(r[valueCol])).filter((v) => !Number.isNaN(v));
    results.push([name, summarize(nums)]);
}
results.sort(([, a], [, b]) => b.mean - a.mean);
console.log(`${"group".padEnd(10)}${"count".padStart(6)}${"min".padStart(10)}${"max".padStart(10)}${"mean".padStart(10)}${"median".padStart(10)}${"stddev".padStart(10)}`);
for (const [name, s] of results) {
    console.log(`${name.padEnd(10)}${String(s.count).padStart(6)}${fmt(s.min)}${fmt(s.max)}${fmt(s.mean)}${fmt(s.median)}${fmt(s.stddev)}`);
}
