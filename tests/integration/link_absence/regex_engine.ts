// HWS hook: js_runtime_install_regex_engine
// HWS prefixes: perex::|perry_runtime::regex::perex_|regex::|regex_automata::|regex_syntax::
console.log("abc123".replace(/[0-9]+/g, "!"));
const re = new RegExp("[0-9]+", "g");
console.log(re.source, re.flags, String(re));
console.log(String.prototype.replace.call("ab12cd34", re, "!"));
console.log(String.prototype.match.call("ab12cd34", re).join(":"));
console.log(String.prototype.split.call("a12b34c", re).join(":"));
const C: any = RegExp;
console.log(C("ab").test("abc"));
console.log(new C("cd").test("abcd"));
