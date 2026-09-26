// fetch client throughput: argv[2]=url, argv[3]=N requests, argv[4]=concurrency.
const url = process.argv[2] || "http://127.0.0.1:18090/";
const N = Number(process.argv[3] || 2000);
const C = Number(process.argv[4] || 1);
async function worker(count: number): Promise<number> {
  let bytes = 0;
  for (let i = 0; i < count; i++) {
    const r = await fetch(url);
    bytes += (await r.text()).length;
  }
  return bytes;
}
async function main() {
  const t0 = performance.now();
  const per = Math.floor(N / C);
  const parts: Promise<number>[] = [];
  for (let c = 0; c < C; c++) parts.push(worker(per));
  const b = await Promise.all(parts);
  const ms = performance.now() - t0;
  const total = per * C;
  console.log(JSON.stringify({ requests: total, bytes: b.reduce((a, x) => a + x, 0), ms, rps: (total * 1000) / ms }));
}
main();
