// #9200 discriminator. The original fixture reads rows only through the
// cross-module dispatch tower (`pickAll`), so a wrong answer cannot be
// attributed to the DATA vs the TOWER. This reads every row BOTH ways at every
// stage: directly (`r.a`, `r.c`, Object.keys) and through the tower.
//
// If the direct reads stay correct while the tower reads go NaN, the object is
// intact and the tower's inline keys-pointer guard is wrongly passing.
// If the direct reads ALSO break, the object itself lost its keys array.
import { Row, pickAll } from "./_helpers/repsel_pshape_tower_rows.ts";

function churn(n: number): number {
    const sink: { x: number }[] = [];
    for (let i = 0; i < n; i++) sink.push({ x: i });
    return sink.length;
}

const rows: Row[] = [];
for (let i = 0; i < 4; i++) rows.push(new Row(i + 1, (i + 1) * 10, (i + 1) * 3));

function report(tag: string): void {
    const direct: string[] = [];
    for (let i = 0; i < rows.length; i++) {
        const r: any = rows[i];
        direct.push(r.a + "/" + r.c + "[" + Object.keys(r).join("") + "]");
    }
    console.log(tag + " direct: " + direct.join(" "));
    console.log(tag + " tower : " + pickAll(rows).join(","));
}

report("S0");
delete (rows[1] as any).b;
report("S1-after-delete-row1");
churn(200_000);
report("S2-after-churn");
delete (rows[3] as any).a;
report("S3-after-delete-row3");
churn(200_000);
report("S4-after-churn2");
