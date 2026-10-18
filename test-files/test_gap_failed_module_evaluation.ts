async function check(name: string) {
  for (let attempt = 0; attempt < 2; attempt++) {
    try {
      await import('./_helpers/sf1_require_failures/' + name + '.cjs');
      console.log(name, attempt, 'returned');
    } catch (error: any) {
      console.log(name, attempt, error.name);
    }
  }
}
async function main() {
  await check('bad');
  await check('throws');
}
main();
