// Console stream-write validation parity (#3080).
// Node's `new Console(...)` throws ERR_CONSOLE_WRITABLE_STREAM (a TypeError)
// when a resolved stdout/stderr does not expose a callable `write` method.
const Console = (console as any).Console;

function check(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label + ": ok");
  } catch (e: any) {
    console.log(label + ": " + e.name + " " + e.code + " | " + e.message);
  }
}

// stdout object without a write method -> stdout error.
check("no-write", () => {
  new Console({});
});

// single positional stream with a callable write -> accepted.
check("with-write", () => {
  const c = new Console({ write() {} });
  void c;
});

// write present but not callable -> stdout error.
check("write-not-fn", () => {
  new Console({ write: 5 });
});

// valid stdout, second positional stderr without write -> stderr error.
check("stderr-bad", () => {
  new Console({ write() {} }, {});
});

// valid stdout, stderr omitted -> defaults to stdout, accepted.
check("stderr-omitted", () => {
  const c = new Console({ write() {} });
  void c;
});

// options-bag form with valid stdout -> accepted.
check("opts-ok", () => {
  const c = new Console({ stdout: { write() {} } });
  void c;
});

// options-bag form with invalid stdout -> stdout error.
check("opts-bad-stdout", () => {
  new Console({ stdout: {} });
});

// options-bag form, valid stdout, invalid stderr -> stderr error.
check("opts-bad-stderr", () => {
  new Console({ stdout: { write() {} }, stderr: {} });
});

// options-bag form, valid stdout and stderr -> accepted.
check("opts-both-ok", () => {
  const c = new Console({ stdout: { write() {} }, stderr: { write() {} } });
  void c;
});
