// A class whose int32 field is kept at rest (an I32 lane), written here
// with `|=`; the test module reads and writes it across the module edge.
export class Mask {
  constructor() {
    this.mask = 1 | 0;
  }
  set(channel) {
    this.mask = ((1 << channel) | 0) >>> 0;
  }
  enable(channel) {
    this.mask |= (1 << channel) | 0;
  }
  test(other) {
    return (this.mask & other.mask) !== 0;
  }
}
