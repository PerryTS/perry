// Pattern: template literal with multiple interpolations in a hot
// loop. Models error message construction, log formatting, prompt
// templating.

const N = 200_000;

const out: string[] = [];
for (let i = 0; i < N; i++) {
    const id = i;
    const status = i % 4 === 0 ? "ok" : "fail";
    const took = (i % 1000) * 0.1;
    const s = `[${id}] status=${status} took=${took.toFixed(1)}ms user=${"u" + (i % 100)}`;
    out.push(s);
}

let total = 0;
for (let i = 0; i < out.length; i++) total += out[i].length;
console.log("checksum:", total);
