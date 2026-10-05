// Remaining RegExp one-shape observability gap: flags_reads_getters (S3).
// The flags proof still bypasses an overridden prototype global getter.
// Keep the parity-failure entries under #11881 until S3 fixes this row.
function t(name: string, f: () => any): void {
  let line: string;
  try {
    line = name + ": " + JSON.stringify(f());
  } catch (e: any) {
    line = name + ": throw " + (e && e.constructor && e.constructor.name);
  }
  console.log(line);
}

t("flags_reads_getters", () => { const d = Object.getOwnPropertyDescriptor(RegExp.prototype, "global"); Object.defineProperty(RegExp.prototype, "global", { get() { return false; }, configurable: true }); try { return /a/g.flags; } finally { Object.defineProperty(RegExp.prototype, "global", d); } });
