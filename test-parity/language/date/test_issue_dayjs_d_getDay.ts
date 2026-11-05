// Regression for the dayjs `format()` crash where `this.$d.getDay()` threw
// `TypeError: (number).getDay is not a function`. dayjs stores a Date instance
// in `this.$d` from a function-decl-shaped class, then reads `getDay()` /
// `getDate()` etc on that field during `format`. Before the fix, the `getDay`
// arm was missing from the HIR Date method dispatch list — so the lowering
// fell through to runtime dynamic dispatch, which saw a numeric (Date is an
// f64 timestamp internally) and threw the "is not a function" TypeError.
//
// This standalone shape mirrors dayjs's pattern: store a Date in a field,
// read it back in a method, call `getDay()` on it.

class MyDate {
  $d: Date;
  $D: number;
  $W: number;
  constructor(s: string) {
    this.$d = new Date(s);
    this.$D = this.$d.getDate();
    this.$W = this.$d.getDay();
  }

  format() {
    const $d = this.$d;
    const day = $d.getDay();
    const date = $d.getDate();
    return "day=" + day + " date=" + date;
  }
}

const m = new MyDate("2024-01-02");
console.log(m.format());
// 2024-01-02 is a Tuesday → getDay() === 2
console.log("W=" + m.$W);
console.log("D=" + m.$D);
