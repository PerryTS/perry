function CjsCtor(v) {
  this.v = v;
}
CjsCtor.prototype.get = function () {
  return this.v;
};
module.exports = CjsCtor;
