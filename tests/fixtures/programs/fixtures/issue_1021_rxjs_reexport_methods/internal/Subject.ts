import { Observable } from "../index.js";

export class Subject extends Observable {
  private closed = false;

  error(message: string): string {
    this.closed = true;
    return "error:" + message;
  }

  next(value: string): string {
    if (this.closed) {
      return "closed";
    }
    return this.subscribe() + ":next:" + value;
  }
}
