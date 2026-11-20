function f() {
  var n = 1;
  class K { nested() { return new (class { r() { return n; } })().r(); } }
  n = 2;
  return new K().nested();
}
console.log(f());
