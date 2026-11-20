// #10711: a function read from an object property must not drop its own call
// to a second function handed to it as a parameter (also read from an object
// property).
//
// This is commander's `_displayError` shape. `lib/command.js` builds a default
// output configuration holding two function properties —
//
//     writeErr:    (str) => process.stderr.write(str),
//     outputError: (str, write) => write(str),
//
// — and every error path calls
// `this._outputConfiguration.outputError(msg, this._outputConfiguration.writeErr)`:
// an object-property function invoking a SECOND object-property function that
// was passed to it as a parameter. If the inner `write(str)` evaporates, the
// program still runs and still throws the right CommanderError — it just
// prints nothing. A silently dropped call is the worst failure mode there is,
// so this fixture asserts the inner call RAN, not merely that something got
// printed.
//
// Every writer sinks to stdout on purpose. The parity harness merges stdout
// and stderr into one compared stream, so a fixture that used both would race
// on the interleaving; the stream is incidental to the indirection under test.

function out(s: string): void {
  process.stdout.write(s);
}

// ── 1. The reported shape, verbatim: object-literal arrow properties, both
//       reached through one level of plain-function call. ──────────────────
const config: any = {
  writeErr: (str: string) => out(str),
  outputError: (str: string, write: (s: string) => void) => write(str),
};

function fireError(cfg: any, message: string) {
  cfg.outputError(message, cfg.writeErr);
}

fireError(config, "1 verbatim: error: something went wrong\n");

// ── 2. The inner call is observably entered and left. Printing "before" and
//       "after" around it is what separates "the call ran" from "the outer
//       body ran and the inner call vanished" — the two look identical when
//       only the payload is checked. ──────────────────────────────────────
const traced: any = {
  writeErr: (str: string) => out("   inner: " + str),
  outputError: (str: string, write: (s: string) => void) => {
    out("2 before, typeof write=" + typeof write + "\n");
    write(str);
    out("2 after\n");
  },
};
fireError(traced, "payload\n");

// ── 3. Method-shorthand spelling of the same object. ────────────────────────
const shorthand: any = {
  writeErr(str: string) {
    out(str);
  },
  outputError(str: string, write: (s: string) => void) {
    write(str);
  },
};
fireError(shorthand, "3 shorthand: error: something went wrong\n");

// ── 4. commander's real home for it: a class field holding the config, the
//       receiver reached as `this.<field>` inside a method. ────────────────
class Reporter {
  _outputConfiguration: any = {
    writeOut: (str: string) => out(str),
    writeErr: (str: string) => out(str),
    outputError: (str: string, write: (s: string) => void) => write(str),
    getOutHelpWidth: () => 80,
  };
  configureOutput(cfg: any): Reporter {
    Object.assign(this._outputConfiguration, cfg);
    return this;
  }
  error(message: string): void {
    this._outputConfiguration.outputError(
      `${message}\n`,
      this._outputConfiguration.writeErr,
    );
  }
}

const reporter = new Reporter();
reporter.error("4 class field: error: something went wrong");

// ── 5. `configureOutput` replaces the writer after construction — the call
//       must reach the REPLACEMENT, not a value baked in at literal-creation
//       time. ────────────────────────────────────────────────────────────
reporter.configureOutput({ writeErr: (str: string) => out("[override]" + str) });
reporter.error("5 after configureOutput");

// ── 6. Spread-built config, nested receiver, and a writer taken from a
//       DIFFERENT object than the one holding `outputError`. ──────────────
const defaults: any = {
  writeErr: (str: string) => out(str),
  outputError: (str: string, write: (s: string) => void) => write(str),
};
const nested: any = { io: { ...defaults } };
nested.io.outputError("6 nested spread: ok\n", nested.io.writeErr);

const sink: any = { writeErr: (str: string) => out("[other]" + str) };
nested.io.outputError("6 cross-object writer\n", sink.writeErr);

// ── 7. Repeated dispatch: the shape must survive a loop, where the call site
//       is re-entered and any per-site caching gets a second look. ─────────
for (let i = 0; i < 3; i++) {
  fireError(config, "7 loop " + i + "\n");
}

// ── 8. And when the writer really is missing, the call must be LOUD. A
//       TypeError here is the property that keeps every future instance of
//       this bug class from presenting as a plausible wrong answer. (Only the
//       error's name is printed: Node names the callee — "write is not a
//       function" — where Perry says "value is not a function".) ───────────
const noWriter: any = {
  outputError: (str: string, write: (s: string) => void) => write(str),
};
try {
  fireError(noWriter, "never printed\n");
  out("8 MISSING WRITER SILENTLY DROPPED THE CALL\n");
} catch (e: any) {
  out("8 threw " + e.name + "\n");
}
