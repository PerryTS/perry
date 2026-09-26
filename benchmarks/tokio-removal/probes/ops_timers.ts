const N = Number(process.argv[2] || 1000);
async function main() {
  for (let i = 0; i < N; i++) await new Promise((ok) => setTimeout(ok, 0));
  console.log("timers", N);
}
main();
