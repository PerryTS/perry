import {
  handle as perchHttpHandler,
  handleRetained as perchRetainedHandler,
} from "./handlers/main";

export function perchHttpEntry(frame: Buffer): any {
  return perchHttpHandler(frame);
}

export function perchRetainedEntry(frame: Buffer): any {
  return perchRetainedHandler(frame);
}
