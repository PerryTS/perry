export class ZS {
  v = 5;
  static create = (): ZS => new ZS();
}
const stringType = ZS.create;
export { stringType as string };
