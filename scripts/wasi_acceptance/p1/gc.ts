import { collect } from "perry/gc";
// Forced collections while exceptions unwind through live frames.
const retained: any[] = [];
for (let i = 0; i < 200; i++) {
  retained.push({ text: `value-${i}`, values: [i, i + 1] });
  if (i % 10 === 0) collect();
  try {
    if (i % 50 === 0) throw new Error(`gc-${i}`);
  } catch (error) {
    retained.push((error as Error).message);
    collect();
  }
}
collect();
console.log(retained.length, retained[0].text, retained.filter((x) => typeof x === "string").join("|"));
