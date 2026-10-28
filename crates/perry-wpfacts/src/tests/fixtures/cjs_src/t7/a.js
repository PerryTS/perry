function Reply(x) { this.raw = x; this.n = 1; }
Reply.prototype.f = function () { return this.raw; };
module.exports = Reply;
