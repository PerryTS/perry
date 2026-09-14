async function run(): Promise<void> {
  let checksum = 0;
  for (let i = 0; i < 100000; i++) {
    const value = await Promise.resolve(i).then((n: number) => n + 1);
    checksum += value;
  }
  console.log(checksum);
}
run();
