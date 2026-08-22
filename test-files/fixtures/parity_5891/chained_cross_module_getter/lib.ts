export class Scroll {
  private _top: number = 42;
  get scrollTop(): number { return this._top; }
}
export class Viewport {
  readonly scroll: Scroll;
  constructor() { this.scroll = new Scroll(); }
}
export class VM {
  readonly viewport: Viewport;
  constructor() { this.viewport = new Viewport(); }
}
