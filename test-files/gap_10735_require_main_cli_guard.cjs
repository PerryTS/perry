// #10735 helper: the real-world shape (dotenv's bundled CLI, and countless
// other packages) — `if (require.main === module) { ...CLI... }`. Importing
// this as a dependency must NOT take the CLI branch: no "CLI" line, no
// process.exit(1).
if (require.main === module) {
  console.log('CLI');
  process.exit(1);
}
exports.ok = true;
