// A TTY `process.stdout` carries `columns` and `rows` as own data properties
// (Node's tty.WriteStream), so a read through any value — an alias, a
// parameter, `config.stdout ?? process.stdout` — sees the terminal size, and a
// SIGWINCH listener sees the new size. Only the literal `process.stdout.columns`
// spelling used to answer; every other read was undefined, so a TUI such as
// opentui laid itself out for 80x24 in any terminal.
//
// Undriven (stdout not a TTY, as in the parity sweep) every read is undefined
// in Node and Perry alike. `crates/perry/tests/tty_write_stream_size.rs`
// drives it under a PTY.
function viaParameter(stream: any): unknown {
  return stream.columns;
}
function viaDefault(config: any = {}): string {
  const stdout = config.stdout ?? process.stdout;
  return `${stdout.columns}x${stdout.rows}`;
}
const alias: any = process.stdout;
const own = Object.prototype.hasOwnProperty.call(alias, 'columns');
console.log(`SIZE:${viaDefault()} param=${viaParameter(process.stdout)} own=${own}`);
if (process.env.PERRY_TTY_SIZE_DRIVE === '1') {
  process.on('SIGWINCH', () => {
    console.log(`RESIZED:${alias.columns}x${alias.rows}`);
    process.exit(0);
  });
  console.log('READY');
  setTimeout(() => {
    console.log('RESIZED:timeout');
    process.exit(1);
  }, 10000);
}
