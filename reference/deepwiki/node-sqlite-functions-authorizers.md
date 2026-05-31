# DeepWiki Response

Repository: `nodejs/node`
Question: How does node:sqlite implement DatabaseSync function, aggregate, enableDefensive, setAuthorizer, callback argument conversion, authorizer constants, and validation errors?
Search: https://deepwiki.com/search/how-does-nodesqlite-implement_05cc32e3-ba1d-48d1-b79e-cefae9c4c9bd

**Question**: How does node:sqlite implement DatabaseSync function, aggregate, enableDefensive, setAuthorizer, callback argument conversion, authorizer constants, and validation errors?

The `node:sqlite` module implements `DatabaseSync` and its associated functionalities, including `aggregate`, `enableDefensive`, `setAuthorizer`, callback argument conversion, authorizer constants, and validation errors, primarily through C++ bindings to the SQLite library.

## `DatabaseSync` Class Implementation
The `DatabaseSync` class in `node:sqlite` represents a synchronous connection to a SQLite database [1](#ref-q1-1) . Its constructor `new DatabaseSync(path[, options])` allows specifying the database path and various configuration options such as `readOnly`, `enableForeignKeyConstraints`, `timeout`, and `defensive` [2](#ref-q1-2) . The C++ implementation of `DatabaseSync` is defined in `src/node_sqlite.h` [3](#ref-q1-3)  and its methods are exposed to JavaScript through `Initialize` function in `src/node_sqlite.cc` [4](#ref-q1-4) .

## `aggregate` Function
The `database.aggregate(name, options)` method registers a new aggregate function with the SQLite database [5](#ref-q1-5) . This method is a wrapper around the SQLite C API function `sqlite3_create_window_function()` [6](#ref-q1-6) . The `options` object for `aggregate` can specify properties like `deterministic`, `directOnly`, `useBigIntArguments`, `varargs`, `start`, `step`, `result`, and `inverse` [7](#ref-q1-7) . The C++ implementation `DatabaseSync::AggregateFunction` in `src/node_sqlite.cc` handles the creation of the aggregate function, including argument validation for `options.start` and `options.step` [8](#ref-q1-8) . It uses `sqlite3_create_window_function` to register the function with the SQLite connection [9](#ref-q1-9) .

## `enableDefensive` Function
The `database.enableDefensive(active)` method controls the defensive flag for the database connection [10](#ref-q1-10) . When enabled, language features that could corrupt the database are disabled [10](#ref-q1-10) . The C++ implementation `DatabaseSync::EnableDefensive` in `src/node_sqlite.cc` takes a boolean argument and uses `sqlite3_db_config` with `SQLITE_DBCONFIG_DEFENSIVE` to set the defensive mode [11](#ref-q1-11) . By default, defensive mode is on [12](#ref-q1-12) .

## `setAuthorizer` Function
The `database.setAuthorizer(callback)` method sets an authorizer callback that SQLite invokes to authorize operations [13](#ref-q1-13) . This is a wrapper around `sqlite3_set_authorizer()` [14](#ref-q1-14) . The callback receives an `actionCode` and other context-dependent arguments [15](#ref-q1-15) . The C++ implementation `DatabaseSync::SetAuthorizer` in `src/node_sqlite.cc` stores the JavaScript callback function and registers `DatabaseSync::AuthorizerCallback` as the C-level authorizer function with SQLite [16](#ref-q1-16) . If `null` is passed, the authorizer is cleared [17](#ref-q1-17) .

### Callback Argument Conversion
Inside `DatabaseSync::AuthorizerCallback`, the SQLite parameters (`action_code`, `param1`, `param2`, `param3`, `param4`) are converted into JavaScript values and passed to the registered JavaScript callback function [18](#ref-q1-18) . The `action_code` is converted to an integer, and the string parameters are converted to JavaScript strings using `NullableSQLiteStringToValue` [19](#ref-q1-19) .

### Authorizer Constants
The authorizer callback must return one of `SQLITE_OK`, `SQLITE_DENY`, or `SQLITE_IGNORE` [20](#ref-q1-20) . These constants are exposed in the `node:sqlite` module as `constants.SQLITE_OK`, `constants.SQLITE_DENY`, and `constants.SQLITE_IGNORE` [21](#ref-q1-21) . The `DatabaseSync::AuthorizerCallback` checks the return value of the JavaScript callback to ensure it is one of these valid integer codes [22](#ref-q1-22) . Various action codes like `SQLITE_CREATE_TABLE`, `SQLITE_INSERT`, `SQLITE_UPDATE`, etc., are passed to the authorizer callback to indicate the type of operation being authorized [23](#ref-q1-23) .

## Validation Errors
`node:sqlite` performs extensive validation on input arguments for its methods. If an argument has an invalid type or value, an `ERR_INVALID_ARG_TYPE` error is thrown [24](#ref-q1-24) . For example, the `DatabaseSync` constructor validates the `path` argument [24](#ref-q1-24)  and various options like `open`, `readOnly`, and `timeout` [25](#ref-q1-25) . Similarly, the `aggregate` function validates `options.start`, `options.step`, `options.useBigIntArguments`, `options.varargs`, `options.directOnly`, and `options.inverse` [26](#ref-q1-26) . The `function` method also validates its arguments and options [27](#ref-q1-27) . In the C++ code, these checks often involve `THROW_ERR_INVALID_ARG_TYPE` macros [28](#ref-q1-28) . If the authorizer callback returns an invalid authorization code, a `RangeError` is thrown [29](#ref-q1-29) .

## Notes
The provided context focuses on the `DatabaseSync` class and its methods, which are synchronous. There might be an asynchronous counterpart (`Database`) not covered in the provided snippets. The `sqlite3.h` file provides the underlying SQLite C API definitions that `node:sqlite` wraps.

View this search on DeepWiki: https://deepwiki.com/search/how-does-nodesqlite-implement_05cc32e3-ba1d-48d1-b79e-cefae9c4c9bd

## References

<a id="ref-q1-1"></a>
### [1] `doc/api/sqlite.md:99-100`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/sqlite.md#L99-L100)

```markdown
This class represents a single [connection][] to a SQLite database. All APIs
exposed by this class execute synchronously.
```

<a id="ref-q1-2"></a>
### [2] `doc/api/sqlite.md:102-160`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/sqlite.md#L102-L160)

```markdown
### `new DatabaseSync(path[, options])`

<!-- YAML
added: v22.5.0
changes:
  - version: REPLACEME
    pr-url: https://github.com/nodejs/node/pull/61266
    description: Enable `defensive` by default.
  - version:
      - v25.1.0
      - v24.12.0
    pr-url: https://github.com/nodejs/node/pull/60217
    description: Add `defensive` option.
  - version:
      - v24.4.0
      - v22.18.0
    pr-url: https://github.com/nodejs/node/pull/58697
    description: Add new SQLite database options.
-->

* `path` {string | Buffer | URL} The path of the database. A SQLite database can be
  stored in a file or completely [in memory][]. To use a file-backed database,
  the path should be a file path. To use an in-memory database, the path
  should be the special name `':memory:'`.
* `options` {Object} Configuration options for the database connection. The
  following options are supported:
  * `open` {boolean} If `true`, the database is opened by the constructor. When
    this value is `false`, the database must be opened via the `open()` method.
    **Default:** `true`.
  * `readOnly` {boolean} If `true`, the database is opened in read-only mode.
    If the database does not exist, opening it will fail. **Default:** `false`.
  * `enableForeignKeyConstraints` {boolean} If `true`, foreign key constraints
    are enabled. This is recommended but can be disabled for compatibility with
    legacy database schemas. The enforcement of foreign key constraints can be
    enabled and disabled after opening the database using
    [`PRAGMA foreign_keys`][]. **Default:** `true`.
  * `enableDoubleQuotedStringLiterals` {boolean} If `true`, SQLite will accept
    [double-quoted string literals][]. This is not recommended but can be
    enabled for compatibility with legacy database schemas.
    **Default:** `false`.
  * `allowExtension` {boolean} If `true`, the `loadExtension` SQL function
    and the `loadExtension()` method are enabled.
    You can call `enableLoadExtension(false)` later to disable this feature.
    **Default:** `false`.
  * `timeout` {number} The [busy timeout][] in milliseconds. This is the maximum amount of
    time that SQLite will wait for a database lock to be released before
    returning an error. **Default:** `0`.
  * `readBigInts` {boolean} If `true`, integer fields are read as JavaScript `BigInt` values. If `false`,
    integer fields are read as JavaScript numbers. **Default:** `false`.
  * `returnArrays` {boolean} If `true`, query results are returned as arrays instead of objects.
    **Default:** `false`.
  * `allowBareNamedParameters` {boolean} If `true`, allows binding named parameters without the prefix
    character (e.g., `foo` instead of `:foo`). **Default:** `true`.
  * `allowUnknownNamedParameters` {boolean} If `true`, unknown named parameters are ignored when binding.
    If `false`, an exception is thrown for unknown named parameters. **Default:** `false`.
  * `defensive` {boolean} If `true`, enables the defensive flag. When the defensive flag is enabled,
    language features that allow ordinary SQL to deliberately corrupt the database file are disabled.
    The defensive flag can also be set using `enableDefensive()`.
    **Default:** `true`.
```

<a id="ref-q1-3"></a>
### [3] `src/node_sqlite.h:117`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.h#L117)

```c
class DatabaseSync : public BaseObject {
```

<a id="ref-q1-4"></a>
### [4] `src/node_sqlite.cc:3433-3480`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L3433-L3480)

```cpp
  Local<FunctionTemplate> db_tmpl =
      NewFunctionTemplate(isolate, DatabaseSync::New);
  db_tmpl->InstanceTemplate()->SetInternalFieldCount(
      DatabaseSync::kInternalFieldCount);
  Local<Object> constants = Object::New(isolate);

  DefineConstants(constants);

  SetProtoMethod(isolate, db_tmpl, "open", DatabaseSync::Open);
  SetProtoMethod(isolate, db_tmpl, "close", DatabaseSync::Close);
  SetProtoDispose(isolate, db_tmpl, DatabaseSync::Dispose);
  SetProtoMethod(isolate, db_tmpl, "prepare", DatabaseSync::Prepare);
  SetProtoMethod(isolate, db_tmpl, "exec", DatabaseSync::Exec);
  SetProtoMethod(isolate, db_tmpl, "function", DatabaseSync::CustomFunction);
  SetProtoMethod(
      isolate, db_tmpl, "createTagStore", DatabaseSync::CreateTagStore);
  SetProtoMethodNoSideEffect(
      isolate, db_tmpl, "location", DatabaseSync::Location);
  SetProtoMethod(
      isolate, db_tmpl, "aggregate", DatabaseSync::AggregateFunction);
  SetProtoMethod(
      isolate, db_tmpl, "createSession", DatabaseSync::CreateSession);
  SetProtoMethod(
      isolate, db_tmpl, "applyChangeset", DatabaseSync::ApplyChangeset);
  SetProtoMethod(isolate,
                 db_tmpl,
                 "enableLoadExtension",
                 DatabaseSync::EnableLoadExtension);
  SetProtoMethod(
      isolate, db_tmpl, "enableDefensive", DatabaseSync::EnableDefensive);
  SetProtoMethod(
      isolate, db_tmpl, "loadExtension", DatabaseSync::LoadExtension);
  SetProtoMethod(
      isolate, db_tmpl, "setAuthorizer", DatabaseSync::SetAuthorizer);
  SetSideEffectFreeGetter(isolate,
                          db_tmpl,
                          FIXED_ONE_BYTE_STRING(isolate, "isOpen"),
                          DatabaseSync::IsOpenGetter);
  SetSideEffectFreeGetter(isolate,
                          db_tmpl,
                          FIXED_ONE_BYTE_STRING(isolate, "isTransaction"),
                          DatabaseSync::IsTransactionGetter);
  Local<String> sqlite_type_key = FIXED_ONE_BYTE_STRING(isolate, "sqlite-type");
  Local<v8::Symbol> sqlite_type_symbol =
      v8::Symbol::For(isolate, sqlite_type_key);
  Local<String> database_sync_string =
      FIXED_ONE_BYTE_STRING(isolate, "node:sqlite");
  db_tmpl->InstanceTemplate()->Set(sqlite_type_symbol, database_sync_string);
```

<a id="ref-q1-5"></a>
### [5] `doc/api/sqlite.md:172-173`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/sqlite.md#L172-L173)

```markdown
Registers a new aggregate function with the SQLite database. This method is a wrapper around
[`sqlite3_create_window_function()`][].
```

<a id="ref-q1-6"></a>
### [6] `doc/api/sqlite.md:173`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/sqlite.md#L173)

```markdown
[`sqlite3_create_window_function()`][].
```

<a id="ref-q1-7"></a>
### [7] `doc/api/sqlite.md:176-199`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/sqlite.md#L176-L199)

```markdown
* `options` {Object} Function configuration settings.
  * `deterministic` {boolean} If `true`, the [`SQLITE_DETERMINISTIC`][] flag is
    set on the created function. **Default:** `false`.
  * `directOnly` {boolean} If `true`, the [`SQLITE_DIRECTONLY`][] flag is set on
    the created function. **Default:** `false`.
  * `useBigIntArguments` {boolean} If `true`, integer arguments to `options.step` and `options.inverse`
    are converted to `BigInt`s. If `false`, integer arguments are passed as
    JavaScript numbers. **Default:** `false`.
  * `varargs` {boolean} If `true`, `options.step` and `options.inverse` may be invoked with any number of
    arguments (between zero and [`SQLITE_MAX_FUNCTION_ARG`][]). If `false`,
    `inverse` and `step` must be invoked with exactly `length` arguments.
    **Default:** `false`.
  * `start` {number | string | null | Array | Object | Function} The identity
    value for the aggregation function. This value is used when the aggregation
    function is initialized. When a {Function} is passed the identity will be its return value.
  * `step` {Function} The function to call for each row in the aggregation. The
    function receives the current state and the row value. The return value of
    this function should be the new state.
  * `result` {Function} The function to call to get the result of the
    aggregation. The function receives the final state and should return the
    result of the aggregation.
  * `inverse` {Function} When this function is provided, the `aggregate` method will work as a window function.
    The function receives the current state and the dropped row value. The return value of this function should be the
    new state.
```

<a id="ref-q1-8"></a>
### [8] `src/node_sqlite.cc:1451-1479`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L1451-L1479)

```cpp
void DatabaseSync::AggregateFunction(const FunctionCallbackInfo<Value>& args) {
  DatabaseSync* db;
  ASSIGN_OR_RETURN_UNWRAP(&db, args.This());
  Environment* env = Environment::GetCurrent(args);
  THROW_AND_RETURN_ON_BAD_STATE(env, !db->IsOpen(), "database is not open");
  Utf8Value name(env->isolate(), args[0].As<String>());
  Local<Object> options = args[1].As<Object>();
  Local<Value> start_v;
  if (!options->Get(env->context(), env->start_string()).ToLocal(&start_v)) {
    return;
  }

  if (start_v->IsUndefined()) {
    THROW_ERR_INVALID_ARG_TYPE(env->isolate(),
                               "The \"options.start\" argument must be a "
                               "function or a primitive value.");
    return;
  }

  Local<Value> step_v;
  if (!options->Get(env->context(), env->step_string()).ToLocal(&step_v)) {
    return;
  }

  if (!step_v->IsFunction()) {
    THROW_ERR_INVALID_ARG_TYPE(
        env->isolate(), "The \"options.step\" argument must be a function.");
    return;
  }
```

<a id="ref-q1-9"></a>
### [9] `src/node_sqlite.cc:1589-1604`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L1589-L1604)

```cpp
  int r = sqlite3_create_window_function(db->connection_,
                                         *name,
                                         argc,
                                         text_rep,
                                         new CustomAggregate(env,
                                                             db,
                                                             use_bigint_args,
                                                             start_v,
                                                             stepFunction,
                                                             inverseFunc,
                                                             resultFunction),
                                         CustomAggregate::xStep,
                                         CustomAggregate::xFinal,
                                         xValue,
                                         xInverse,
                                         CustomAggregate::xDestroy);
```

<a id="ref-q1-10"></a>
### [10] `doc/api/sqlite.md:157-159`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/sqlite.md#L157-L159)

```markdown
  * `defensive` {boolean} If `true`, enables the defensive flag. When the defensive flag is enabled,
    language features that allow ordinary SQL to deliberately corrupt the database file are disabled.
    The defensive flag can also be set using `enableDefensive()`.
```

<a id="ref-q1-11"></a>
### [11] `src/node_sqlite.cc:1961-1979`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L1961-L1979)

```cpp
void DatabaseSync::EnableDefensive(const FunctionCallbackInfo<Value>& args) {
  DatabaseSync* db;
  ASSIGN_OR_RETURN_UNWRAP(&db, args.This());
  Environment* env = Environment::GetCurrent(args);
  THROW_AND_RETURN_ON_BAD_STATE(env, !db->IsOpen(), "database is not open");

  auto isolate = args.GetIsolate();
  if (!args[0]->IsBoolean()) {
    THROW_ERR_INVALID_ARG_TYPE(isolate,
                               "The \"active\" argument must be a boolean.");
    return;
  }

  const int enable = args[0].As<Boolean>()->Value();
  int defensive_enabled;
  const int defensive_ret = sqlite3_db_config(
      db->connection_, SQLITE_DBCONFIG_DEFENSIVE, enable, &defensive_enabled);
  CHECK_ERROR_OR_THROW(isolate, db, defensive_ret, SQLITE_OK, void());
}
```

<a id="ref-q1-12"></a>
### [12] `test/parallel/test-sqlite-config.js:23-26`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-sqlite-config.js#L23-L26)

```javascript
test('by default, defensive mode is on', (t) => {
  const db = new DatabaseSync(':memory:');
  t.assert.strictEqual(checkDefensiveMode(db), true);
});
```

<a id="ref-q1-13"></a>
### [13] `doc/api/sqlite.md:353-355`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/sqlite.md#L353-L355)

```markdown
### `database.setAuthorizer(callback)`

<!-- YAML
```

<a id="ref-q1-14"></a>
### [14] `doc/api/sqlite.md:365`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/sqlite.md#L365)

```markdown
This method is a wrapper around [`sqlite3_set_authorizer()`][].
```

<a id="ref-q1-15"></a>
### [15] `doc/api/sqlite.md:369-375`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/sqlite.md#L369-L375)

```markdown
* `actionCode` {number} The type of operation being performed (e.g.,
  `SQLITE_INSERT`, `SQLITE_UPDATE`, `SQLITE_SELECT`).
* `arg1` {string|null} The first argument (context-dependent, often a table name).
* `arg2` {string|null} The second argument (context-dependent, often a column name).
* `dbName` {string|null} The name of the database.
* `triggerOrView` {string|null} The name of the trigger or view causing the access.
```

<a id="ref-q1-16"></a>
### [16] `src/node_sqlite.cc:2017-2046`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L2017-L2046)

```cpp
void DatabaseSync::SetAuthorizer(const FunctionCallbackInfo<Value>& args) {
  DatabaseSync* db;
  ASSIGN_OR_RETURN_UNWRAP(&db, args.This());
  Environment* env = Environment::GetCurrent(args);
  Isolate* isolate = env->isolate();

  if (args[0]->IsNull()) {
    // Clear the authorizer
    sqlite3_set_authorizer(db->connection_, nullptr, nullptr);
    db->object()->SetInternalField(kAuthorizerCallback, Null(isolate));
    return;
  }

  if (!args[0]->IsFunction()) {
    THROW_ERR_INVALID_ARG_TYPE(
        isolate, "The \"callback\" argument must be a function or null.");
    return;
  }

  Local<Function> fn = args[0].As<Function>();

  db->object()->SetInternalField(kAuthorizerCallback, fn);

  int r = sqlite3_set_authorizer(
      db->connection_, DatabaseSync::AuthorizerCallback, db);

  if (r != SQLITE_OK) {
    CHECK_ERROR_OR_THROW(isolate, db, r, SQLITE_OK, void());
  }
}
```

<a id="ref-q1-17"></a>
### [17] `src/node_sqlite.cc:2023-2026`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L2023-L2026)

```cpp
  if (args[0]->IsNull()) {
    // Clear the authorizer
    sqlite3_set_authorizer(db->connection_, nullptr, nullptr);
    db->object()->SetInternalField(kAuthorizerCallback, Null(isolate));
```

<a id="ref-q1-18"></a>
### [18] `src/node_sqlite.cc:2068-2077`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L2068-L2077)

```cpp
  // Convert SQLite authorizer parameters to JavaScript values
  js_argv.emplace_back(Integer::New(isolate, action_code));
  js_argv.emplace_back(
      NullableSQLiteStringToValue(isolate, param1).ToLocalChecked());
  js_argv.emplace_back(
      NullableSQLiteStringToValue(isolate, param2).ToLocalChecked());
  js_argv.emplace_back(
      NullableSQLiteStringToValue(isolate, param3).ToLocalChecked());
  js_argv.emplace_back(
      NullableSQLiteStringToValue(isolate, param4).ToLocalChecked());
```

<a id="ref-q1-19"></a>
### [19] `src/node_sqlite.cc:2069-2077`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L2069-L2077)

```cpp
  js_argv.emplace_back(Integer::New(isolate, action_code));
  js_argv.emplace_back(
      NullableSQLiteStringToValue(isolate, param1).ToLocalChecked());
  js_argv.emplace_back(
      NullableSQLiteStringToValue(isolate, param2).ToLocalChecked());
  js_argv.emplace_back(
      NullableSQLiteStringToValue(isolate, param3).ToLocalChecked());
  js_argv.emplace_back(
      NullableSQLiteStringToValue(isolate, param4).ToLocalChecked());
```

<a id="ref-q1-20"></a>
### [20] `doc/api/sqlite.md:377-381`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/sqlite.md#L377-L381)

```markdown

* `SQLITE_OK` - Allow the operation.
* `SQLITE_DENY` - Deny the operation (causes an error).
* `SQLITE_IGNORE` - Ignore the operation (silently skip).
```

<a id="ref-q1-21"></a>
### [21] `doc/api/sqlite.md:383-392`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/sqlite.md#L383-L392)

```markdown
const { DatabaseSync, constants } = require('node:sqlite');
const db = new DatabaseSync(':memory:');

// Set up an authorizer that denies all table creation
db.setAuthorizer((actionCode) => {
  if (actionCode === constants.SQLITE_CREATE_TABLE) {
    return constants.SQLITE_DENY;
  }
  return constants.SQLITE_OK;
});
```

<a id="ref-q1-22"></a>
### [22] `src/node_sqlite.cc:2105-2119`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L2105-L2119)

```cpp
  int32_t int_result = result.As<Int32>()->Value();
  if (int_result != SQLITE_OK && int_result != SQLITE_DENY &&
      int_result != SQLITE_IGNORE) {
    if (!String::NewFromUtf8(
             isolate,
             "Authorizer callback returned a invalid authorization code")
             .ToLocal(&error_message)) {
      return SQLITE_DENY;
    }

    Local<Value> err = Exception::RangeError(error_message);
    isolate->ThrowException(err);
    db->SetIgnoreNextSQLiteError(true);
    return SQLITE_DENY;
  }
```

<a id="ref-q1-23"></a>
### [23] `doc/api/sqlite.md:1329-1380`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/sqlite.md#L1329-L1380)

```markdown
  <tr>
    <th>Constant</th>
    <th>Description</th>
  </tr>
  <tr>
    <td><code>SQLITE_CREATE_INDEX</code></td>
    <td>Create an index</td>
  </tr>
  <tr>
    <td><code>SQLITE_CREATE_TABLE</code></td>
    <td>Create a table</td>
  </tr>
  <tr>
    <td><code>SQLITE_CREATE_TEMP_INDEX</code></td>
    <td>Create a temporary index</td>
  </tr>
  <tr>
    <td><code>SQLITE_CREATE_TEMP_TABLE</code></td>
    <td>Create a temporary table</td>
  </tr>
  <tr>
    <td><code>SQLITE_CREATE_TEMP_TRIGGER</code></td>
    <td>Create a temporary trigger</td>
  </tr>
  <tr>
    <td><code>SQLITE_CREATE_TEMP_VIEW</code></td>
    <td>Create a temporary view</td>
  </tr>
  <tr>
    <td><code>SQLITE_CREATE_TRIGGER</code></td>
    <td>Create a trigger</td>
  </tr>
  <tr>
    <td><code>SQLITE_CREATE_VIEW</code></td>
    <td>Create a view</td>
  </tr>
  <tr>
    <td><code>SQLITE_DELETE</code></td>
    <td>Delete from a table</td>
  </tr>
  <tr>
    <td><code>SQLITE_DROP_INDEX</code></td>
    <td>Drop an index</td>
  </tr>
  <tr>
    <td><code>SQLITE_DROP_TABLE</code></td>
    <td>Drop a table</td>
  </tr>
  <tr>
    <td><code>SQLITE_DROP_TEMP_INDEX</code></td>
    <td>Drop a temporary index</td>
  </tr>
```

<a id="ref-q1-24"></a>
### [24] `test/parallel/test-sqlite-database-sync.js:27-33`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-sqlite-database-sync.js#L27-L33)

```javascript
  test('throws if database path is not a string, Uint8Array, or URL', (t) => {
    t.assert.throws(() => {
      new DatabaseSync();
    }, {
      code: 'ERR_INVALID_ARG_TYPE',
      message: /The "path" argument must be a string, Uint8Array, or URL without null bytes/,
    });
```

<a id="ref-q1-25"></a>
### [25] `test/parallel/test-sqlite-database-sync.js:63-88`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-sqlite-database-sync.js#L63-L88)

```javascript
  test('throws if options.open is provided but is not a boolean', (t) => {
    t.assert.throws(() => {
      new DatabaseSync('foo', { open: 5 });
    }, {
      code: 'ERR_INVALID_ARG_TYPE',
      message: /The "options\.open" argument must be a boolean/,
    });
  });

  test('throws if options.readOnly is provided but is not a boolean', (t) => {
    t.assert.throws(() => {
      new DatabaseSync('foo', { readOnly: 5 });
    }, {
      code: 'ERR_INVALID_ARG_TYPE',
      message: /The "options\.readOnly" argument must be a boolean/,
    });
  });

  test('throws if options.timeout is provided but is not an integer', (t) => {
    t.assert.throws(() => {
      new DatabaseSync('foo', { timeout: .99 });
    }, {
      code: 'ERR_INVALID_ARG_TYPE',
      message: /The "options\.timeout" argument must be an integer/,
    });
  });
```

<a id="ref-q1-26"></a>
### [26] `test/parallel/test-sqlite-aggregate-function.mjs:10-83`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-sqlite-aggregate-function.mjs#L10-L83)

```
    test('throws if options.start is not provided', (t) => {
      t.assert.throws(() => {
        db.aggregate('sum', {
          result: (total) => total
        });
      }, {
        code: 'ERR_INVALID_ARG_TYPE',
        message: 'The "options.start" argument must be a function or a primitive value.'
      });
    });

    test('throws if options.step is not a function', (t) => {
      t.assert.throws(() => {
        db.aggregate('sum', {
          start: 0,
          result: (total) => total
        });
      }, {
        code: 'ERR_INVALID_ARG_TYPE',
        message: 'The "options.step" argument must be a function.'
      });
    });

    test('throws if options.useBigIntArguments is not a boolean', (t) => {
      t.assert.throws(() => {
        db.aggregate('sum', {
          start: 0,
          step: () => null,
          useBigIntArguments: ''
        });
      }, {
        code: 'ERR_INVALID_ARG_TYPE',
        message: /The "options\.useBigIntArguments" argument must be a boolean/,
      });
    });

    test('throws if options.varargs is not a boolean', (t) => {
      t.assert.throws(() => {
        db.aggregate('sum', {
          start: 0,
          step: () => null,
          varargs: ''
        });
      }, {
        code: 'ERR_INVALID_ARG_TYPE',
        message: /The "options\.varargs" argument must be a boolean/,
      });
    });

    test('throws if options.directOnly is not a boolean', (t) => {
      t.assert.throws(() => {
        db.aggregate('sum', {
          start: 0,
          step: () => null,
          directOnly: ''
        });
      }, {
        code: 'ERR_INVALID_ARG_TYPE',
        message: /The "options\.directOnly" argument must be a boolean/,
      });
    });

    test('throws if options.inverse is not a function', (t) => {
      t.assert.throws(() => {
        db.aggregate('sum', {
          start: 0,
          step: (acc, value) => acc + value,
          inverse: 10
        });
      }, {
        code: 'ERR_INVALID_ARG_TYPE',
        message: /The "options\.inverse" argument must be a function/,
      });
    });
```

<a id="ref-q1-27"></a>
### [27] `test/parallel/test-sqlite-custom-functions.js:12-73`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-sqlite-custom-functions.js#L12-L73)

```javascript
    test('throws if name is not a string', () => {
      assert.throws(() => {
        db.function();
      }, {
        code: 'ERR_INVALID_ARG_TYPE',
        message: /The "name" argument must be a string/,
      });
    });

    test('throws if function is not a function', () => {
      assert.throws(() => {
        db.function('foo');
      }, {
        code: 'ERR_INVALID_ARG_TYPE',
        message: /The "function" argument must be a function/,
      });
    });

    test('throws if options is not an object', () => {
      assert.throws(() => {
        db.function('foo', null, () => {});
      }, {
        code: 'ERR_INVALID_ARG_TYPE',
        message: /The "options" argument must be an object/,
      });
    });

    test('throws if options.useBigIntArguments is not a boolean', () => {
      assert.throws(() => {
        db.function('foo', { useBigIntArguments: null }, () => {});
      }, {
        code: 'ERR_INVALID_ARG_TYPE',
        message: /The "options\.useBigIntArguments" argument must be a boolean/,
      });
    });

    test('throws if options.varargs is not a boolean', () => {
      assert.throws(() => {
        db.function('foo', { varargs: null }, () => {});
      }, {
        code: 'ERR_INVALID_ARG_TYPE',
        message: /The "options\.varargs" argument must be a boolean/,
      });
    });

    test('throws if options.deterministic is not a boolean', () => {
      assert.throws(() => {
        db.function('foo', { deterministic: null }, () => {});
      }, {
        code: 'ERR_INVALID_ARG_TYPE',
        message: /The "options\.deterministic" argument must be a boolean/,
      });
    });

    test('throws if options.directOnly is not a boolean', () => {
      assert.throws(() => {
        db.function('foo', { directOnly: null }, () => {});
      }, {
        code: 'ERR_INVALID_ARG_TYPE',
        message: /The "options\.directOnly" argument must be a boolean/,
      });
    });
```

<a id="ref-q1-28"></a>
### [28] `src/node_sqlite.cc:1464-1468`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L1464-L1468)

```cpp
    THROW_ERR_INVALID_ARG_TYPE(env->isolate(),
                               "The \"options.start\" argument must be a "
                               "function or a primitive value.");
    return;
  }
```

<a id="ref-q1-29"></a>
### [29] `src/node_sqlite.cc:2108-2119`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L2108-L2119)

```cpp
    if (!String::NewFromUtf8(
             isolate,
             "Authorizer callback returned a invalid authorization code")
             .ToLocal(&error_message)) {
      return SQLITE_DENY;
    }

    Local<Value> err = Exception::RangeError(error_message);
    isolate->ThrowException(err);
    db->SetIgnoreNextSQLiteError(true);
    return SQLITE_DENY;
  }
```
