function cached(getter) {
  return () => getter();
}
module.exports.cached = cached;
module.exports.flag = cached(() => true);
