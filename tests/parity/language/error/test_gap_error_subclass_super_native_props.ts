// An Error subclass keeps the own properties its constructor assigns after super(),
// the prototype methods it installs with Object.setPrototypeOf, and survives the
// spread and `in` checks an error handler makes. When the class value escapes
// (it is called through a variable below), super() used to leave the derived
// instance uninitialised, so a backend error lost `statusCode`/`code`-style own
// properties and every error response became a generic 500.
const ErrorCode = { InvalidKey: 'InvalidKey', KeyAlreadyExists: 'KeyAlreadyExists', InternalError: 'InternalError' };

class BackendError extends Error {
  httpStatusCode;
  originalError;
  userStatusCode;
  resource;
  code;
  metadata = {};
  error;

  constructor(options) {
    super(options.message);
    this.code = options.code;
    this.httpStatusCode = options.httpStatusCode;
    this.userStatusCode = options.httpStatusCode === 500 ? 500 : 400;
    this.message = options.message;
    this.originalError = options.originalError;
    this.resource = options.resource;
    this.error = options.error;
    Object.setPrototypeOf(this, BackendError.prototype);
  }
  static withStatusCode(statusCode, options) {
    const error = new BackendError(options);
    error.userStatusCode = statusCode;
    return error;
  }
  withStatusCode(statusCode) {
    this.userStatusCode = statusCode;
    return this;
  }
  render() {
    return {
      statusCode: this.httpStatusCode.toString(),
      code: this.code,
      error: this.code,
      message: this.message,
    };
  }
  getOriginalError() {
    return this.originalError;
  }
}

const ERRORS = {
  InvalidKey: (key, e) =>
    new BackendError({
      code: ErrorCode.InvalidKey,
      resource: key,
      httpStatusCode: 400,
      message: `Invalid key: ${key}`,
      originalError: e,
    }),
  KeyAlreadyExists: (key, e) =>
    new BackendError({
      code: ErrorCode.KeyAlreadyExists,
      resource: key,
      error: 'Duplicate',
      httpStatusCode: 409,
      message: 'The resource already exists',
      originalError: e,
    }),
};

function isRenderableError(error) {
  return !!error && typeof error === 'object' && 'render' in error && typeof error.render === 'function';
}

function handle(error) {
  if (isRenderableError(error)) {
    const renderableError = error.render();
    const statusCode = error.userStatusCode ? error.userStatusCode : renderableError.statusCode === '500' ? 500 : 400;
    return { statusCode, body: { ...renderableError, error: error.error || renderableError.code } };
  }
  return { statusCode: 500, body: { statusCode: '500', error: 'Internal', code: ErrorCode.InternalError } };
}

function check(label, make) {
  let err;
  try {
    throw make();
  } catch (e) {
    err = e;
  }
  console.log(label, 'instanceof', err instanceof BackendError, err instanceof Error);
  console.log(label, 'own keys', Object.keys(err).join(','));
  console.log(label, 'props', err.code, err.httpStatusCode, err.userStatusCode, err.resource, err.error, err.message);
  console.log(label, 'in', 'render' in err, 'code' in err, 'httpStatusCode' in err, typeof err.render);
  console.log(label, 'render', JSON.stringify(err.render()));
  console.log(label, 'json', JSON.stringify(err));
  console.log(label, 'assign', JSON.stringify(Object.assign({}, err)));
  console.log(label, 'spread', JSON.stringify({ ...err }));
  console.log(label, 'handled', JSON.stringify(handle(err)));
}

check('invalid-key', () => ERRORS.InvalidKey('a//b'));
check('key-exists', () => ERRORS.KeyAlreadyExists('hello.bin', new Error('dup')));
check('with-status', () => ERRORS.InvalidKey('x').withStatusCode(404));
check('static-with-status', () => BackendError.withStatusCode(409, { code: ErrorCode.KeyAlreadyExists, httpStatusCode: 409, message: 'm' }));

// Dynamic dispatch of class methods on Error-subclass instances. Several classes
// define each name, so the call goes through the runtime's method dispatch and
// not a statically bound body. `super()` must initialize the instance the derived
// construct allocated: replacing it with a fresh native Error left the class's
// methods and own fields unreachable (`{}`).
class Coded extends Error {
  constructor(m) { super(m); this.code = 'C'; }
  who() { return 'Coded'; }
  tag() { return this.code; }
  info() { return { a: 1 }; }
}
class Other extends Error {
  constructor(m) { super(m); this.code = 'O'; }
  who() { return 'Other'; }
  tag() { return this.code; }
  info() { return { a: 2 }; }
}
class Bare extends TypeError {
  who() { return 'Bare'; }
  tag() { return 'bare'; }
  info() { return { a: 3 }; }
}
const call = (o, name) => o[name]();
for (const make of [() => new Coded('c'), () => new Other('o'), () => new Bare('b', { cause: 'x' })]) {
  const e = make();
  console.log('dyn', call(e, 'who'), call(e, 'tag'), JSON.stringify(call(e, 'info')), e.message, e instanceof Error, Object.keys(e).join(','));
}
console.log('dyn cause', new Bare('b', { cause: 'why' }).cause, Object.getOwnPropertyNames(new Coded('c')).join(','));
