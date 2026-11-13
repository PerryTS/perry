// family_reach: a write to stdout with a 'hex' or 'base64' encoding decodes
// the text to bytes inside the runtime. Those bytes are no JavaScript value:
// the program creates no Buffer and names none, the fact says the typed-array
// family is never created or observed, and the birth witness must not report
// (the decoders used to allocate a temporary Buffer: rc 86, verdict family).
process.stdout.write("68657820", "hex");
process.stdout.write("YmFzZTY0IA==", "base64");
process.stdout.write("bGF0aW4x", "base64url");
process.stdout.write(" plain\n");
process.stderr.write("", "hex");
console.log("6f6b".length);
