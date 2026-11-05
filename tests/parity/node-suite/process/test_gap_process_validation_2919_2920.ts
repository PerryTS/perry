// #2919 — process.setuid/setgid/seteuid/setegid argument validation.
// #2920 — process.umask(mask) argument validation + octal-string parsing.
//
// Node throws synchronously on bad ARG types/ranges *before* attempting the
// privileged syscall, so the validation path is testable without root. For
// valid-but-unprivileged ids Node would throw EPERM at the syscall, so this
// fixture only probes invalid arguments for the credential setters and
// restores the umask after every valid mutation.

function probeId(label: string, fn: () => any) {
  try {
    fn();
    console.log(label, "no-throw");
  } catch (err: any) {
    console.log(label, err.name, err.code, err.message);
  }
}

probeId("setuid({})", () => process.setuid({} as any));
probeId("setuid(-1)", () => process.setuid(-1));
probeId("setuid(1.5)", () => process.setuid(1.5));
probeId("setuid(undefined)", () => process.setuid(undefined as any));
probeId("setuid(null)", () => process.setuid(null as any));
probeId("setuid(true)", () => process.setuid(true as any));
probeId("setuid(2**32)", () => process.setuid(2 ** 32));
probeId("setgid({})", () => process.setgid({} as any));
probeId("setgid(1.5)", () => process.setgid(1.5));
probeId("setgid(-1)", () => process.setgid(-1));
probeId("seteuid({})", () => process.seteuid({} as any));
probeId("seteuid(1.5)", () => process.seteuid(1.5));
probeId("setegid({})", () => process.setegid({} as any));
probeId("setegid(1.5)", () => process.setegid(1.5));

const original = process.umask();

function probeMask(label: string, value: any) {
  try {
    const before = process.umask();
    const out = process.umask(value);
    const after = process.umask();
    console.log(label, "ok", out.toString(8), before.toString(8), after.toString(8));
  } catch (err: any) {
    console.log(label, err.name, err.code, err.message);
  } finally {
    process.umask(original);
  }
}

probeMask("undefined", undefined);
probeMask("null", null);
probeMask("object", {});
probeMask("boolean", true);
probeMask("invalid string", "abc");
probeMask("octal string 077", "077");
probeMask("octal string 64", "64");
probeMask("bad octal string 8", "8");
probeMask("empty string", "");
probeMask("negative", -1);
probeMask("fractional", 1.5);
probeMask("nan", NaN);
probeMask("infinity", Infinity);
probeMask("0o1000", 0o1000);
probeMask("zero", 0);
