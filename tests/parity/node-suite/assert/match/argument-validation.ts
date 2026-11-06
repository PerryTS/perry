import assert from "node:assert";

function marker(message: string, needle: string, present: string, missing: string): string {
  return message.indexOf(needle) >= 0 ? present : missing;
}

function report(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label, "ok");
  } catch (err) {
    const e = err as any;
    const operator = e && "operator" in e ? e.operator : "no-operator";
    const generated = e && "generatedMessage" in e ? e.generatedMessage : "no-generated";
    const message = String(e && e.message);
    console.log(
      label,
      "throw",
      e && e.name,
      e && e.code,
      operator,
      generated,
      marker(
        message,
        'The "string" argument must be of type string',
        "string-msg",
        "no-string-msg",
      ),
      marker(
        message,
        'The "regexp" argument must be an instance of RegExp',
        "regexp-msg",
        "no-regexp-msg",
      ),
      marker(message, "Received type number (123)", "number-received", "no-number-received"),
      marker(message, "Received type string ('a')", "string-received", "no-string-received"),
      marker(message, "Received an instance of Object", "object-received", "no-object-received"),
    );
  }
}

report("match non-string actual", () => assert.match(123 as any, /23/));
report("doesNotMatch non-string actual", () => assert.doesNotMatch(123 as any, /45/));
report("match string matcher", () => assert.match("abc", "a" as any));
report("doesNotMatch object matcher", () => assert.doesNotMatch("abc", {} as any));
report("match validates matcher first", () => assert.match(123 as any, "a" as any));
report("match valid", () => assert.match("abc", /b/));
report("doesNotMatch valid", () => assert.doesNotMatch("abc", /z/));
