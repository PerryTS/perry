async function main() {
  let count = 0;
  for (let i = 0; i < 1000; i++) {
    await Promise.resolve();
    count++;
  }
  console.log("promises", count);
  setTimeout(() => console.log("churn deadline hit"), 10);
}
main();
