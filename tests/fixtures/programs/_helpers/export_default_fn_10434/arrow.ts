// Control: an arrow function held in a const.
const arrow: any = (a?: unknown, b?: unknown) => typeof a + "," + typeof b;
arrow.kind = "arrow-static";
export const holder = { arrow };
export default arrow;
