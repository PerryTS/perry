// A string literal that spells the TLS specifier keyword (highlight.js's C++
// keyword list, bundled by Claude Code). The module-wide local-exec TLS rewrite
// searched each global's whole line, so it rewrote the string's own bytes to
// `thread_local(localexec)` and the image no longer matched its declared
// length: "constant expression type mismatch" and no binary.
const keywords = "alignas alignof thread_local restrict thread_local(initialexec) final override";
const words = keywords.split(" ");
console.log(keywords.length, words.length, words[2], words[4]);
console.log({ keyword: "int float thread_local char" }.keyword.indexOf("thread_local"));
