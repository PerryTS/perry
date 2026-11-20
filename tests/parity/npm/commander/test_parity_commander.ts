// Behavioral parity test for commander (perry-stdlib).
//
// The full CLI uses process.argv parsing which is environment-sensitive;
// we parse a synthetic argv array so the output is deterministic.

import { Command } from "commander";

const program = new Command();
program
  .name("parity-cli")
  .description("commander parity probe")
  .version("1.2.3");

let captured: Record<string, unknown> | null = null;
program
  .command("serve")
  .description("start the server")
  .option("-p, --port <number>", "port number", "3000")
  .option("--verbose", "verbose output")
  .requiredOption("--host <host>", "hostname")
  .action((opts: Record<string, unknown>) => {
    captured = opts;
  });

// Simulate `parity-cli serve --port 8080 --verbose --host 127.0.0.1`.
const argv = ["node", "parity-cli", "serve", "--port", "8080", "--verbose", "--host", "127.0.0.1"];
program.parse(argv);

console.log("name:", program.name());
console.log("description:", program.description());
console.log("version literal stored:", program.version());

if (captured) {
  console.log("opt port:", captured.port);
  console.log("opt verbose:", captured.verbose);
  console.log("opt host:", captured.host);
} else {
  console.log("action did not fire");
}

// Issue #5137: a top-level program (no subcommand) that declares a positional
// via `.argument()`, reads `program.args`, and stringifies `program.opts()`.
// Parsing an explicit argv array (commander's `from: 'node'` default) must be
// honored, `opts()` must return a real object, and `args` a real array.
const top = new Command();
top.name("demo").option("-v, --verbose").argument("<file>");
top.parse(["node", "x", "in.txt", "-v"]);
console.log("positional:", top.args[0], "opts:", JSON.stringify(top.opts()));

/*
@covers
crates/perry-stdlib/src/commander.rs (mirrored in crates/perry-ext-commander/src/lib.rs):
  - js_commander_action
  - js_commander_args_array
  - js_commander_args_count
  - js_commander_argument
  - js_commander_command
  - js_commander_description
  - js_commander_get_arg
  - js_commander_get_option
  - js_commander_get_option_bool
  - js_commander_get_option_number
  - js_commander_name
  - js_commander_new
  - js_commander_option
  - js_commander_opts
  - js_commander_parse
  - js_commander_required_option
  - js_commander_version
*/
