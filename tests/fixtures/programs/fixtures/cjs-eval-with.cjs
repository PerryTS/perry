let outerValue = 12;
function throughWith(source) {
  const box = { outerValue: 70 };
  with (box) { eval(source); }
  return box.outerValue;
}
module.exports = throughWith;
