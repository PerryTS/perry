// #10440 — local bindings whose names match Node builtin modules must keep
// ordinary JavaScript receiver semantics. A name alone is not proof that the
// receiver is the builtin namespace.

const crypto = {
  sha256(value: string) {
    return `user.sha256(${value})`;
  },
  md5(value: string) {
    return `user.md5(${value})`;
  },
};
const fs = {
  readFileSync(value: string) {
    return `user.readFileSync(${value})`;
  },
};
const path = {
  join(...parts: string[]) {
    return `user.join(${parts.join(",")})`;
  },
  sep: "user-sep",
};
const os = {
  platform() {
    return "user-platform";
  },
  EOL: "user-eol",
};
const net = {
  createServer() {
    return "user-server";
  },
};

function throughParameter(crypto: { md5(value: string): string }) {
  return crypto.md5("param");
}

console.log(crypto.sha256("abc"));
console.log(fs.readFileSync("missing"));
console.log(path.join("a", "b"));
console.log(path.sep);
console.log(os.platform());
console.log(os.EOL);
console.log(net.createServer());
console.log(throughParameter(crypto));
