export class Observable {
  private value: string;

  constructor(value: string) {
    this.value = value;
  }

  subscribe(): string {
    return "subscribe:" + this.value;
  }
}
