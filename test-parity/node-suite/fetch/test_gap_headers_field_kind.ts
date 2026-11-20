class Request {
  headers: Headers;
  constructor(id: string) { this.headers = new Headers({ "x-request-id": id }); }
}
console.log(new Request("hi").headers.get("x-request-id"));
