// Helper for test_gap_new_globalthis_shadowed_10359.ts — user classes that
// deliberately share their names with global constructors, so importing them
// shadows the bare names in the test module.
export class Event {
  readonly tag = "user-Event";
}
export class Request {
  readonly tag = "user-Request";
}
export class MessageChannel {
  readonly tag = "user-MessageChannel";
}
export class Map {
  readonly tag = "user-Map";
}
export class Int32Array {
  readonly tag = "user-Int32Array";
}
export class ReadableStream {
  readonly tag = "user-ReadableStream";
}
