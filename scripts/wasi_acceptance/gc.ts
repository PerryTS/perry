import { collect } from "perry/gc";
const retained: any[] = [];
for (let i = 0; i < 200; i++) {
  retained.push({ text: `value-${i}`, values: [i, i + 1] });
  if (i % 10 === 0) collect();
}
collect();
console.log(retained.length, retained[0].text, retained[199].values.join(","));
