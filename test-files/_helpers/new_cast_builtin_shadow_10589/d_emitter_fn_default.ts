import EventEmitter from "./emitter_fn_lib.ts";

function isUser(o: any): boolean {
  return !!(o && (o as any).__mark === "user");
}

export function run(): string {
  const results: string[] = [];
  try {
    results.push("plain=" + isUser(new EventEmitter()));
  } catch (e: any) {
    results.push("plain=THROW:" + (e && e.message));
  }
  try {
    results.push("cast=" + isUser(new (EventEmitter as any)()));
  } catch (e: any) {
    results.push("cast=THROW:" + (e && e.message));
  }
  const alias: any = EventEmitter;
  try {
    results.push("alias=" + isUser(new alias()));
  } catch (e: any) {
    results.push("alias=THROW:" + (e && e.message));
  }
  return results.join(" ");
}
