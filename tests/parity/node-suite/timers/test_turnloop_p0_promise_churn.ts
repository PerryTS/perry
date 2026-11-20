// turnloop P0 loop-statistics probe: 10,000 awaited promises (the notify fast
// path, which must make no OS wait), then one 10 ms timeout.
async function main() {
  let count = 0;
  for (let i = 0; i < 10000; i++) {
    await Promise.resolve(i);
    count++;
  }
  console.log("promises", count);
  setTimeout(() => console.log("churn deadline hit"), 10);
}
main();
