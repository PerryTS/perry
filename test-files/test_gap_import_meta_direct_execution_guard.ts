// The conventional ESM direct-execution guard must survive native
// compilation: argv[1] names the source entry while argv[0] names the binary.
function main() {
  console.log("direct execution guard fired");
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
