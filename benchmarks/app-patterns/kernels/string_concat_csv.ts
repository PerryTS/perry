// Pattern: build CSV rows by concatenation. Models log line emit,
// CSV / TSV export, prompt-template construction, etc.

const N = 100_000;

const rows: string[] = [];
for (let i = 0; i < N; i++) {
    const id = i;
    const name = "user_" + i;
    const email = "user_" + i + "@example.com";
    const score = (i * 1.5).toFixed(2);
    // Six-field comma-joined row, the canonical "build a string from
    // mixed-typed parts in a hot loop" shape.
    const row = id + "," + name + "," + email + "," + score + "," + (i % 3 === 0 ? "true" : "false") + ",2026-05-09";
    rows.push(row);
}

let total = 0;
for (let i = 0; i < rows.length; i++) total += rows[i].length;
console.log("checksum:", total, "rows:", rows.length);
