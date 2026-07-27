# `node:readline` parity coverage

This suite uses Node.js **v26.5.0** as the exact oracle. It has 95 small
fixtures. Each fixture prints deterministic evidence for a focused contract.
Fixtures that exercise I/O use in-memory streams. The suite needs no output
normalization.

## Fixed sources

- Node.js [v26.5.0](https://github.com/nodejs/node/tree/v26.5.0):
  `lib/readline.js`, `lib/readline/promises.js`, `lib/internal/readline/*`, and
  `test/parallel/test-readline*.js`.
- Deno
  [2.9.3](https://github.com/denoland/deno/tree/f39575ecd50602a5b42b1ba8e93849460de9fcf4)
  for runtime output, plus
  [main at `34c46613`](https://github.com/denoland/deno/tree/34c46613cbe20450b74c0e8d4f0fd8f6f781d807)
  for its current Node-compat selection and `ext/node` implementation.
- Bun
  [1.2.18](https://github.com/oven-sh/bun/tree/0d4089ea7c48d339e87cc48f1871aeee745d8112)
  for runtime output, plus
  [main at `43d60f69`](https://github.com/oven-sh/bun/tree/43d60f69c95a9f31591165816ced29b83e94673e)
  for its current readline port and tests.

Deno selects 20 upstream readline tests in its Node-compat suite. Bun keeps a
larger dedicated readline suite and copies of the Node tests. Node remains the
oracle when either runtime differs.

## Contract map

The fixtures cover:

- classic and promises exports, aliases, classes, prototype descriptors, public
  legacy method aliases, and `Symbol.asyncIterator`/`Symbol.dispose`;
- options and positional `createInterface()` overloads, defaults, terminal
  inference, `crlfDelay`, and option validation;
- LF, CR, CRLF, Unicode separators, empty lines, UTF-8 and CRLF chunk edges, the
  final unterminated line, stream reuse across interfaces, input errors, and
  close ordering;
- pause/resume, prompt changes, recursive writes, cursor width, close
  idempotence, and use after close;
- callback and promises questions, receiver identity, concurrent questions,
  abort, abort causes, prompt recovery, and closed interfaces;
- async iterator values, completion, nested iterators, early break, and input
  errors;
- callback, sync, async, rejected, and undefined-column completers with
  memory-only I/O;
- history events, mutation, size limits, and duplicate removal;
- fixed printable, modifier, CSI, and split-CSI keypress bytes;
- callback helpers and promises `Readline` commit, rollback, validation, and
  write errors.

Each category traces to primary implementation or tests:

| Fixtures                                               | Node 26.5.0 anchors                                                                         | Deno/Bun comparison anchors                                                                                                                          |
| ------------------------------------------------------ | ------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `exports/**`                                           | `lib/readline.js`, `lib/readline/promises.js`, `test-readline.js`                           | Deno `ext/node/polyfills/{_readline.mjs,readline.ts,readline/promises.ts}`; Bun `src/js/node/{readline.js,readline.promises.js}`                     |
| `create-interface/**`, `interface/**`, `validation/**` | `test-readline-interface.js`, `test-readline-promises-interface.js`                         | Deno `tests/unit_node/readline_test.ts`; Bun `test/js/node/readline/{readline.node.test.ts,readline_promises.node.test.ts}` and its Node test copies |
| `streams/**`                                           | `test-readline-{interface,input-onerror,line-separators,carriage-return-between-chunks}.js` | Both runtimes' copied Node tests and readline interface implementations                                                                              |
| `lifecycle/**`, `question/**`, `promises/question-*`   | classic and promises interface tests and implementations                                    | Both runtimes' interface implementations and dedicated readline tests                                                                                |
| `async-iterator/**`                                    | `test-readline-async-iterators.js`, `test-readline-async-iterators-destroy.js`              | Both runtimes' copied Node async-iterator tests                                                                                                      |
| `completer/**`                                         | classic and promises tab-complete tests                                                     | Both runtimes' copied Node tab-complete tests and dedicated suites                                                                                   |
| `history/**`                                           | `test-readline-interface.js`, `lib/internal/readline/interface.js`                          | Both runtimes' interface implementations and dedicated suites                                                                                        |
| `keypress/**`                                          | `test-readline-{emit-keypress-events,keys}.js`                                              | Both runtimes' copied Node keypress tests and `emitKeypressEvents` ports                                                                             |
| `helpers/**`, `promises/readline-*`                    | `test-readline-csi.js`, `test-readline-promises-csi.mjs`                                    | Both runtimes' copied Node CSI tests and callback implementations                                                                                    |

## Measured results

Three Node 26.5.0 runs completed **95/95** with byte-identical output. Their
fixture/output digest was
`24fa2032d200f2a44726f2e0203f073ae7a2cc4661cd10b4d1215e19fa6d2c85`.

Three focused Perry runs were identical: **20 pass, 75 output differences, zero
compile failures, zero crashes, and zero timeouts**. The baseline records that
clean 20/95 floor.

Three cross-runtime runs were also identical:

| Runtime    | Match | Difference | Error | Timeout |
| ---------- | ----: | ---------: | ----: | ------: |
| Deno 2.9.3 |    91 |          2 |     2 |       0 |
| Bun 1.2.18 |    79 |         14 |     2 |       0 |

Deno differs on `Symbol.dispose` and the promises `Interface.length`; its two
errors are the sync and async promises completer fixtures. Bun's stable gaps
include Unicode line separators, class names/descriptors, `Symbol.dispose`,
post-close methods, abort error shape, completer output, and validation text.
Its two errors are pre-aborted and post-close promises questions. Bun main has
already ported some newer Node behavior, including Unicode separators and
`Symbol.dispose`; the runtime table stays pinned to the installed 1.2.18 binary
so it remains reproducible.

Run the focused Perry comparison with:

```sh
NODE_BIN="$HOME/.nvm/versions/node/v26.5.0/bin/node" \
python3 scripts/node_suite_run.py \
  "$PWD/target/perry-dev/perry" "$PWD" readline
```

Reproduce the per-fixture cross-runtime classification and digest from the
repository root with:

```sh
python3 - <<'PY'
from collections import Counter
from pathlib import Path
import hashlib, subprocess

files = sorted(Path("test-parity/node-suite/readline").rglob("*.ts"))
commands = {
    "node": [str(Path.home() / ".nvm/versions/node/v26.5.0/bin/node")],
    "deno": ["deno", "run", "--allow-all", "--quiet"],
    "bun": ["bun"],
}

def run(command, fixture):
    try:
        result = subprocess.run([*command, fixture], capture_output=True, timeout=30)
        return result.returncode, result.stdout, result.stderr
    except subprocess.TimeoutExpired:
        return None, b"", b""

rows = {name: [(str(f), *run(command, f)) for f in files]
        for name, command in commands.items()}
oracle = {path: result for path, *result in rows["node"]}
for name, results in rows.items():
    counts = Counter()
    digest = hashlib.sha256()
    for path, code, stdout, stderr in results:
        digest.update(path.encode() + b"\0" + str(code).encode() + b"\0" +
                      stdout + b"\0" + stderr + b"\0")
        if code is None:
            status = "timeout"
        elif code != 0:
            status = "error"
        elif [code, stdout] == oracle[path][:2]:
            status = "match"
        else:
            status = "difference"
        counts[status] += 1
        if name != "node" and status != "match":
            print(name, status, path)
    print(name, dict(counts), digest.hexdigest())
PY
```

The table compares stdout byte for byte and also checks exit status. An `Error`
means the runtime exited nonzero; a `Difference` means it exited zero with
different output.

## Boundaries and stopping rule

The removed child-process fixture used `sh` and tested pipe ownership. That
belongs to `child_process` and `stream`, not readline.

This suite excludes:

- real TTY, raw mode, resize, columns, and PTY behavior (`tty`);
- stdin, signals, ref/unref, and host lifecycle (`process`);
- generic listener tables and EventEmitter rules (`events`);
- generic pipe, destroy, and high-water-mark behavior (`stream`);
- editor commands and file-backed or multiline history (`repl`);
- sleeps, races, leak/stress cases, GC/finalization, large input, files,
  network, ports, and CLI processes.

Node's backpressure test uses more than 6,000 lines, so it is outside this
granular lane. The fixtures do not assert receiver errors because Node does not
define or test a stable branded-receiver contract for these methods.

The audit stopped after all remaining Node, Deno, and Bun readline tests fell
into one of those owners or needed timing, host terminal state, large input, or
duplicate coverage. Perry PR [#6858](https://github.com/PerryTS/perry/pull/6858)
was still open and absent from `main` during this audit. These fixtures neither
depend on nor copy its implementation change.
