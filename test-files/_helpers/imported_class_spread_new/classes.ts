export class Plain {
  count: number;
  first: string;
  constructor(...items: string[]) {
    this.count = items.length;
    this.first = typeof items[0];
  }
}

export class Path extends Array<string> {
  constructor(...items: string[]) {
    super();
    this.push(...items);
  }
}
