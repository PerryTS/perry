class Body {
  json() { return Promise.resolve({ name: "left-pad" }); }
}
class Response extends Body {}

export function fetch() {
  return Promise.resolve(new Response());
}
