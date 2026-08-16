export class ReadonlyFixtureSearchParams extends URLSearchParams {
  set(_name: string, _value: string): void {
    throw new TypeError("readonly search params");
  }
}

export class FixtureNextUrl {
  readonly pathname: string;
  readonly searchParams: ReadonlyFixtureSearchParams;

  constructor(url: string) {
    const parsed = new URL(url);
    this.pathname = parsed.pathname;
    this.searchParams = new ReadonlyFixtureSearchParams(parsed.search);
  }
}

export class FixtureNextRequest {
  readonly method: string;
  readonly url: string;
  readonly nextUrl: FixtureNextUrl;
  readonly headers: Headers;
  private readonly requestBody: string;

  constructor(method: string, url: string, id: string, requestBody = "") {
    this.method = method;
    this.url = url;
    this.nextUrl = new FixtureNextUrl(url);
    this.headers = new Headers({ "x-request-id": id });
    this.requestBody = requestBody;
  }

  async text(): Promise<string> {
    await Promise.resolve();
    return this.requestBody;
  }
}
