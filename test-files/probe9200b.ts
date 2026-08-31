// #9200 probe 2. Probe 1 (direct read BEFORE the tower at every stage) did NOT
// reproduce — so the direct reads MASK the bug. This one preserves the original
// fixture's exact call sequence and only ADDS observation at the very end,
// after the failing tower call, to ask whether the DATA is damaged or only the
// tower's answer is.
import { Row, pickAll } from "./_helpers/repsel_pshape_tower_rows.ts";

function churn(n: number): number {
    const sink: { x: number }[] = [];
    for (let i = 0; i < n; i++) sink.push({ x: i });
    return sink.length;
}

const rows: Row[] = [];
for (let i = 0; i < 4; i++) rows.push(new Row(i + 1, (i + 1) * 10, (i + 1) * 3));

console.log("before:", pickAll(rows).join(","));
console.log("churn:", churn(200_000));
delete (rows[1] as any).b;
console.log("after:", pickAll(rows).join(","));
console.log("b:", (rows[1] as any).b);
console.log("c:", rows[1].c);
console.log("keys:", Object.keys(rows[1] as any).join("|"));
delete (rows[3] as any).a;
console.log("churn2:", churn(200_000));
console.log("after2:", pickAll(rows).join(","));
console.log("keys3:", Object.keys(rows[3] as any).join("|"));

// --- everything below is NEW observation, after the failure has happened.
console.log("POST direct row1:", (rows[1] as any).a, (rows[1] as any).c);
console.log("POST keys row1:", Object.keys(rows[1] as any).join("|"));
console.log("POST direct row3:", (rows[3] as any).a, (rows[3] as any).c);
console.log("POST keys row3:", Object.keys(rows[3] as any).join("|"));
console.log("POST tower again:", pickAll(rows).join(","));
console.log("POST keys all:", rows.map((r: any) => Object.keys(r).join("")).join(" "));
