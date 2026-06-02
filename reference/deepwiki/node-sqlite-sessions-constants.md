# DeepWiki Response

Repository: `nodejs/node`
Question: How does node:sqlite implement createSession, Session changeset/patchset/close/disposal, applyChangeset, and sqlite.constants for changeset and authorizer behavior?
Search: https://deepwiki.com/search/how-does-nodesqlite-implement_20dd8c00-bb3c-49bb-b1d2-17536ff3ac20

**Question**: How does node:sqlite implement createSession, Session changeset/patchset/close/disposal, applyChangeset, and sqlite.constants for changeset and authorizer behavior?

The `node:sqlite` module implements session management, including `createSession`, `changeset`, `patchset`, `close`, and disposal of `Session` objects, as well as `applyChangeset` and `sqlite.constants` for changeset and authorizer behavior, through a C++ addon that wraps the SQLite Session Extension API.

### `database.createSession()`
The `database.createSession()` method in `node:sqlite` is implemented by the C++ function `DatabaseSync::CreateSession` [1](#ref-q1-1) . This function takes an optional `options` object that can specify a `table` to track changes for and a `db` name [2](#ref-q1-2) . It creates a new SQLite session using `sqlite3session_create()` and attaches it to the specified table using `sqlite3session_attach()` [3](#ref-q1-3) . A `Session` object is then created and returned to JavaScript [4](#ref-q1-4) .

### `Session` Changeset, Patchset, Close, and Disposal
The `Session` class in `node:sqlite` is a wrapper around the `sqlite3_session` C structure [5](#ref-q1-5) .

*   **`session.changeset()` and `session.patchset()`**: These methods are implemented by the templated C++ function `Session::Changeset` [6](#ref-q1-6) . They call either `sqlite3session_changeset()` or `sqlite3session_patchset()` respectively, to retrieve the binary changeset or patchset data [7](#ref-q1-7)  [8](#ref-q1-8) . The resulting binary data is then copied into a `Uint8Array` and returned [9](#ref-q1-9) .
*   **`session.close()`**: This method is implemented by the C++ function `Session::Close` [10](#ref-q1-10) . It calls the private `Session::Delete()` method [11](#ref-q1-11) , which in turn calls `sqlite3session_delete()` to close the underlying SQLite session [12](#ref-q1-12) .
*   **`session[Symbol.dispose]()`**: This method is implemented by `Session::Dispose` [13](#ref-q1-13) , which simply calls `Session::Close` [14](#ref-q1-14) . This allows for explicit resource management using `using` declarations in JavaScript [15](#ref-q1-15) .

### `database.applyChangeset()`
The `database.applyChangeset()` method is implemented by the C++ function `DatabaseSync::ApplyChangeset` [16](#ref-q1-16) . This function takes a `Uint8Array` representing the changeset or patchset and an optional `options` object [17](#ref-q1-17) . It can include `filter` and `onConflict` callbacks [18](#ref-q1-18) . The C++ implementation sets up these callbacks to be invoked during the `sqlite3changeset_apply()` operation [19](#ref-q1-19) . The core logic involves calling `sqlite3changeset_apply()` with the provided changeset and the configured callbacks [20](#ref-q1-20) .

### `sqlite.constants` for Changeset and Authorizer Behavior
The `sqlite.constants` object provides various constants for conflict resolution during changeset application and for authorizer callbacks [21](#ref-q1-21) . These constants are defined in the C++ code within the `DefineConstants` function [22](#ref-q1-22) .

*   **Changeset Conflict Resolution**: Constants like `SQLITE_CHANGESET_DATA`, `SQLITE_CHANGESET_NOTFOUND`, `SQLITE_CHANGESET_CONFLICT`, `SQLITE_CHANGESET_CONSTRAINT`, and `SQLITE_CHANGESET_FOREIGN_KEY` are passed to the `onConflict` handler in `applyChangeset` [23](#ref-q1-23) . The handler can return `SQLITE_CHANGESET_OMIT`, `SQLITE_CHANGESET_REPLACE`, or `SQLITE_CHANGESET_ABORT` to control conflict resolution [24](#ref-q1-24) . These are exposed via `NODE_DEFINE_CONSTANT` in `DefineConstants` [25](#ref-q1-25) .
*   **Authorizer Behavior**: Constants such as `SQLITE_OK`, `SQLITE_DENY`, and `SQLITE_IGNORE` are used with the `database.setAuthorizer()` method to control permissions for various SQLite operations [26](#ref-q1-26) . These are also defined using `NODE_DEFINE_CONSTANT` in `DefineConstants` [27](#ref-q1-27) .

## Notes
The `node:sqlite` module and its session extension support were added in Node.js v22.12.0 and v23.3.0 [28](#ref-q1-28)  [29](#ref-q1-29)  [30](#ref-q1-30) . When dynamically linking SQLite, compiling with `SQLITE_ENABLE_SESSION` and `SQLITE_ENABLE_PREUPDATE_HOOK` defines is required [31](#ref-q1-31) . The `Session` class also supports Explicit Resource Management (ERM) via `Symbol.dispose` [15](#ref-q1-15)  [32](#ref-q1-32) .

Wiki pages you might want to explore:
- [Node.js Overview (nodejs/node)](/wiki/nodejs/node#1)

View this search on DeepWiki: https://deepwiki.com/search/how-does-nodesqlite-implement_20dd8c00-bb3c-49bb-b1d2-17536ff3ac20

## References

<a id="ref-q1-1"></a>
### [1] `src/node_sqlite.cc:1608-1678`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L1608-L1678)

```cpp
void DatabaseSync::CreateSession(const FunctionCallbackInfo<Value>& args) {
  std::string table;
  std::string db_name = "main";

  Environment* env = Environment::GetCurrent(args);
  if (args.Length() > 0) {
    if (!args[0]->IsObject()) {
      THROW_ERR_INVALID_ARG_TYPE(env->isolate(),
                                 "The \"options\" argument must be an object.");
      return;
    }

    Local<Object> options = args[0].As<Object>();

    Local<String> table_key = env->table_string();
    bool hasIt;
    if (!options->HasOwnProperty(env->context(), table_key).To(&hasIt)) {
      return;
    }
    if (hasIt) {
      Local<Value> table_value;
      if (!options->Get(env->context(), table_key).ToLocal(&table_value)) {
        return;
      }

      if (table_value->IsString()) {
        table = Utf8Value(env->isolate(), table_value).ToString();
      } else {
        THROW_ERR_INVALID_ARG_TYPE(
            env->isolate(), "The \"options.table\" argument must be a string.");
        return;
      }
    }

    Local<String> db_key = FIXED_ONE_BYTE_STRING(env->isolate(), "db");

    if (!options->HasOwnProperty(env->context(), db_key).To(&hasIt)) {
      return;
    }
    if (hasIt) {
      Local<Value> db_value;
      if (!options->Get(env->context(), db_key).ToLocal(&db_value)) {
        // An error will have been scheduled.
        return;
      }
      if (db_value->IsString()) {
        db_name = Utf8Value(env->isolate(), db_value).ToString();
      } else {
        THROW_ERR_INVALID_ARG_TYPE(
            env->isolate(), "The \"options.db\" argument must be a string.");
        return;
      }
    }
  }

  DatabaseSync* db;
  ASSIGN_OR_RETURN_UNWRAP(&db, args.This());
  THROW_AND_RETURN_ON_BAD_STATE(env, !db->IsOpen(), "database is not open");

  sqlite3_session* pSession;
  int r = sqlite3session_create(db->connection_, db_name.c_str(), &pSession);
  CHECK_ERROR_OR_THROW(env->isolate(), db, r, SQLITE_OK, void());
  db->sessions_.insert(pSession);

  r = sqlite3session_attach(pSession, table == "" ? nullptr : table.c_str());
  CHECK_ERROR_OR_THROW(env->isolate(), db, r, SQLITE_OK, void());

  BaseObjectPtr<Session> session =
      Session::Create(env, BaseObjectWeakPtr<DatabaseSync>(db), pSession);
  args.GetReturnValue().Set(session->object());
}
```

<a id="ref-q1-2"></a>
### [2] `doc/api/sqlite.md:616-618`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/sqlite.md#L616-L618)

```markdown
* `options` {Object} The configuration options for the session.
  * `table` {string} A specific table to track changes for. By default, changes to all tables are tracked.
  * `db` {string} Name of the database to track. This is useful when multiple databases have been added using [`ATTACH DATABASE`][]. **Default**: `'main'`.
```

<a id="ref-q1-3"></a>
### [3] `src/node_sqlite.cc:1667-1673`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L1667-L1673)

```cpp
  sqlite3_session* pSession;
  int r = sqlite3session_create(db->connection_, db_name.c_str(), &pSession);
  CHECK_ERROR_OR_THROW(env->isolate(), db, r, SQLITE_OK, void());
  db->sessions_.insert(pSession);

  r = sqlite3session_attach(pSession, table == "" ? nullptr : table.c_str());
  CHECK_ERROR_OR_THROW(env->isolate(), db, r, SQLITE_OK, void());
```

<a id="ref-q1-4"></a>
### [4] `src/node_sqlite.cc:1675-1677`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L1675-L1677)

```cpp
  BaseObjectPtr<Session> session =
      Session::Create(env, BaseObjectWeakPtr<DatabaseSync>(db), pSession);
  args.GetReturnValue().Set(session->object());
```

<a id="ref-q1-5"></a>
### [5] `src/node_sqlite.h:279-284`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.h#L279-L284)

```c
class Session : public BaseObject {
 public:
  Session(Environment* env,
          v8::Local<v8::Object> object,
          BaseObjectWeakPtr<DatabaseSync> database,
          sqlite3_session* session);
```

<a id="ref-q1-6"></a>
### [6] `src/node_sqlite.cc:3322-3345`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L3322-L3345)

```cpp
template <Sqlite3ChangesetGenFunc sqliteChangesetFunc>
void Session::Changeset(const FunctionCallbackInfo<Value>& args) {
  Session* session;
  ASSIGN_OR_RETURN_UNWRAP(&session, args.This());
  Environment* env = Environment::GetCurrent(args);
  THROW_AND_RETURN_ON_BAD_STATE(
      env, !session->database_->IsOpen(), "database is not open");
  THROW_AND_RETURN_ON_BAD_STATE(
      env, session->session_ == nullptr, "session is not open");

  int nChangeset;
  void* pChangeset;
  int r = sqliteChangesetFunc(session->session_, &nChangeset, &pChangeset);
  CHECK_ERROR_OR_THROW(
      env->isolate(), session->database_.get(), r, SQLITE_OK, void());

  auto freeChangeset = OnScopeLeave([&] { sqlite3_free(pChangeset); });

  Local<ArrayBuffer> buffer = ArrayBuffer::New(env->isolate(), nChangeset);
  std::memcpy(buffer->GetBackingStore()->Data(), pChangeset, nChangeset);
  Local<Uint8Array> uint8Array = Uint8Array::New(buffer, 0, nChangeset);

  args.GetReturnValue().Set(uint8Array);
}
```

<a id="ref-q1-7"></a>
### [7] `src/node_sqlite.cc:3334`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L3334)

```cpp
  int r = sqliteChangesetFunc(session->session_, &nChangeset, &pChangeset);
```

<a id="ref-q1-8"></a>
### [8] `doc/api/sqlite.md:734-749`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/sqlite.md#L734-L749)

```markdown
Retrieves a changeset containing all changes since the changeset was created. Can be called multiple times.
An exception is thrown if the database or the session is not open. This method is a wrapper around [`sqlite3session_changeset()`][].

### `session.patchset()`

<!-- YAML
added:
  - v23.3.0
  - v22.12.0
-->

* Returns: {Uint8Array} Binary patchset that can be applied to other databases.

Similar to the method above, but generates a more compact patchset. See [Changesets and Patchsets][]
in the documentation of SQLite. An exception is thrown if the database or the session is not open. This method is a
wrapper around [`sqlite3session_patchset()`][].
```

<a id="ref-q1-9"></a>
### [9] `src/node_sqlite.cc:3340-3343`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L3340-L3343)

```cpp
  Local<ArrayBuffer> buffer = ArrayBuffer::New(env->isolate(), nChangeset);
  std::memcpy(buffer->GetBackingStore()->Data(), pChangeset, nChangeset);
  Local<Uint8Array> uint8Array = Uint8Array::New(buffer, 0, nChangeset);
```

<a id="ref-q1-10"></a>
### [10] `src/node_sqlite.cc:3347-3357`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L3347-L3357)

```cpp
void Session::Close(const FunctionCallbackInfo<Value>& args) {
  Session* session;
  ASSIGN_OR_RETURN_UNWRAP(&session, args.This());
  Environment* env = Environment::GetCurrent(args);
  THROW_AND_RETURN_ON_BAD_STATE(
      env, !session->database_->IsOpen(), "database is not open");
  THROW_AND_RETURN_ON_BAD_STATE(
      env, session->session_ == nullptr, "session is not open");

  session->Delete();
}
```

<a id="ref-q1-11"></a>
### [11] `src/node_sqlite.cc:3356`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L3356)

```cpp
  session->Delete();
```

<a id="ref-q1-12"></a>
### [12] `src/node_sqlite.cc:3368`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L3368)

```cpp
  if (!database_ || !database_->connection_ || session_ == nullptr) return;
```

<a id="ref-q1-13"></a>
### [13] `src/node_sqlite.cc:3359-3365`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L3359-L3365)

```cpp
void Session::Dispose(const v8::FunctionCallbackInfo<v8::Value>& args) {
  v8::TryCatch try_catch(args.GetIsolate());
  Close(args);
  if (try_catch.HasCaught()) {
    CHECK(try_catch.CanContinue());
  }
}
```

<a id="ref-q1-14"></a>
### [14] `src/node_sqlite.cc:3361`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L3361)

```cpp
  Close(args);
```

<a id="ref-q1-15"></a>
### [15] `doc/api/sqlite.md:757-762`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/sqlite.md#L757-L762)

```markdown

<!-- YAML
added: v24.9.0
-->

Closes the session. If the session is already closed, does nothing.
```

<a id="ref-q1-16"></a>
### [16] `src/node_sqlite.h:145`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.h#L145)

```c
  static void ApplyChangeset(const v8::FunctionCallbackInfo<v8::Value>& args);
```

<a id="ref-q1-17"></a>
### [17] `doc/api/sqlite.md:631-633`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/sqlite.md#L631-L633)

```markdown
* `changeset` {Uint8Array} A binary changeset or patchset.
* `options` {Object} The configuration options for how the changes will be applied.
  * `filter` {Function} Skip changes that, when targeted table name is supplied to this function, return a truthy value.
```

<a id="ref-q1-18"></a>
### [18] `doc/api/sqlite.md:634-655`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/sqlite.md#L634-L655)

```markdown
    By default, all changes are attempted.
  * `onConflict` {Function} A function that determines how to handle conflicts. The function receives one argument,
    which can be one of the following values:

    * `SQLITE_CHANGESET_DATA`: A `DELETE` or `UPDATE` change does not contain the expected "before" values.
    * `SQLITE_CHANGESET_NOTFOUND`: A row matching the primary key of the `DELETE` or `UPDATE` change does not exist.
    * `SQLITE_CHANGESET_CONFLICT`: An `INSERT` change results in a duplicate primary key.
    * `SQLITE_CHANGESET_FOREIGN_KEY`: Applying a change would result in a foreign key violation.
    * `SQLITE_CHANGESET_CONSTRAINT`: Applying a change results in a `UNIQUE`, `CHECK`, or `NOT NULL` constraint
      violation.

    The function should return one of the following values:

    * `SQLITE_CHANGESET_OMIT`: Omit conflicting changes.
    * `SQLITE_CHANGESET_REPLACE`: Replace existing values with conflicting changes (only valid with
      `SQLITE_CHANGESET_DATA` or `SQLITE_CHANGESET_CONFLICT` conflicts).
    * `SQLITE_CHANGESET_ABORT`: Abort on conflict and roll back the database.

    When an error is thrown in the conflict handler or when any other value is returned from the handler,
    applying the changeset is aborted and the database is rolled back.

    **Default**: A function that returns `SQLITE_CHANGESET_ABORT`.
```

<a id="ref-q1-19"></a>
### [19] `src/node_sqlite.cc:1850-1911`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L1850-L1911)

```cpp
      Local<Function> conflictFunc = conflictValue.As<Function>();
      context.conflictCallback = [env, conflictFunc](int conflictType) -> int {
        Local<Value> argv[] = {Integer::New(env->isolate(), conflictType)};
        TryCatch try_catch(env->isolate());
        Local<Value> result =
            conflictFunc->Call(env->context(), Null(env->isolate()), 1, argv)
                .FromMaybe(Local<Value>());
        if (try_catch.HasCaught()) {
          try_catch.ReThrow();
          return SQLITE_CHANGESET_ABORT;
        }
        constexpr auto invalid_value = -1;
        if (!result->IsInt32()) return invalid_value;
        return result->Int32Value(env->context()).FromJust();
      };
    }

    bool hasIt;
    if (!options->HasOwnProperty(env->context(), env->filter_string())
             .To(&hasIt)) {
      return;
    }
    if (hasIt) {
      Local<Value> filterValue;
      if (!options->Get(env->context(), env->filter_string())
               .ToLocal(&filterValue)) {
        // An error will have been scheduled.
        return;
      }

      if (!filterValue->IsFunction()) {
        THROW_ERR_INVALID_ARG_TYPE(
            env->isolate(),
            "The \"options.filter\" argument must be a function.");
        return;
      }

      Local<Function> filterFunc = filterValue.As<Function>();

      context.filterCallback = [&](std::string_view item) -> bool {
        // If there was an error in the previous call to the filter's
        // callback, we skip calling it again.
        if (db->ignore_next_sqlite_error_) {
          return false;
        }

        Local<Value> argv[1];
        if (!ToV8Value(env->context(), item, env->isolate())
                 .ToLocal(&argv[0])) {
          db->SetIgnoreNextSQLiteError(true);
          return false;
        }

        Local<Value> result;
        if (!filterFunc->Call(env->context(), Null(env->isolate()), 1, argv)
                 .ToLocal(&result)) {
          db->SetIgnoreNextSQLiteError(true);
          return false;
        }

        return result->BooleanValue(env->isolate());
      };
```

<a id="ref-q1-20"></a>
### [20] `src/node_sqlite.cc:1915-1916`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L1915-L1916)

```cpp
  ArrayBufferViewContents<uint8_t> buf(args[0]);
  int r = sqlite3changeset_apply(
```

<a id="ref-q1-21"></a>
### [21] `doc/api/sqlite.md:1233`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/sqlite.md#L1233)

```markdown
An object containing commonly used constants for SQLite operations.
```

<a id="ref-q1-22"></a>
### [22] `src/node_sqlite.cc:3374-3425`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L3374-L3425)

```cpp
void DefineConstants(Local<Object> target) {
  NODE_DEFINE_CONSTANT(target, SQLITE_CHANGESET_OMIT);
  NODE_DEFINE_CONSTANT(target, SQLITE_CHANGESET_REPLACE);
  NODE_DEFINE_CONSTANT(target, SQLITE_CHANGESET_ABORT);

  NODE_DEFINE_CONSTANT(target, SQLITE_CHANGESET_DATA);
  NODE_DEFINE_CONSTANT(target, SQLITE_CHANGESET_NOTFOUND);
  NODE_DEFINE_CONSTANT(target, SQLITE_CHANGESET_CONFLICT);
  NODE_DEFINE_CONSTANT(target, SQLITE_CHANGESET_CONSTRAINT);
  NODE_DEFINE_CONSTANT(target, SQLITE_CHANGESET_FOREIGN_KEY);

  // Authorization result codes
  NODE_DEFINE_CONSTANT(target, SQLITE_OK);
  NODE_DEFINE_CONSTANT(target, SQLITE_DENY);
  NODE_DEFINE_CONSTANT(target, SQLITE_IGNORE);

  // Authorization action codes
  NODE_DEFINE_CONSTANT(target, SQLITE_CREATE_INDEX);
  NODE_DEFINE_CONSTANT(target, SQLITE_CREATE_TABLE);
  NODE_DEFINE_CONSTANT(target, SQLITE_CREATE_TEMP_INDEX);
  NODE_DEFINE_CONSTANT(target, SQLITE_CREATE_TEMP_TABLE);
  NODE_DEFINE_CONSTANT(target, SQLITE_CREATE_TEMP_TRIGGER);
  NODE_DEFINE_CONSTANT(target, SQLITE_CREATE_TEMP_VIEW);
  NODE_DEFINE_CONSTANT(target, SQLITE_CREATE_TRIGGER);
  NODE_DEFINE_CONSTANT(target, SQLITE_CREATE_VIEW);
  NODE_DEFINE_CONSTANT(target, SQLITE_DELETE);
  NODE_DEFINE_CONSTANT(target, SQLITE_DROP_INDEX);
  NODE_DEFINE_CONSTANT(target, SQLITE_DROP_TABLE);
  NODE_DEFINE_CONSTANT(target, SQLITE_DROP_TEMP_INDEX);
  NODE_DEFINE_CONSTANT(target, SQLITE_DROP_TEMP_TABLE);
  NODE_DEFINE_CONSTANT(target, SQLITE_DROP_TEMP_TRIGGER);
  NODE_DEFINE_CONSTANT(target, SQLITE_DROP_TEMP_VIEW);
  NODE_DEFINE_CONSTANT(target, SQLITE_DROP_TRIGGER);
  NODE_DEFINE_CONSTANT(target, SQLITE_DROP_VIEW);
  NODE_DEFINE_CONSTANT(target, SQLITE_INSERT);
  NODE_DEFINE_CONSTANT(target, SQLITE_PRAGMA);
  NODE_DEFINE_CONSTANT(target, SQLITE_READ);
  NODE_DEFINE_CONSTANT(target, SQLITE_SELECT);
  NODE_DEFINE_CONSTANT(target, SQLITE_TRANSACTION);
  NODE_DEFINE_CONSTANT(target, SQLITE_UPDATE);
  NODE_DEFINE_CONSTANT(target, SQLITE_ATTACH);
  NODE_DEFINE_CONSTANT(target, SQLITE_DETACH);
  NODE_DEFINE_CONSTANT(target, SQLITE_ALTER_TABLE);
  NODE_DEFINE_CONSTANT(target, SQLITE_REINDEX);
  NODE_DEFINE_CONSTANT(target, SQLITE_ANALYZE);
  NODE_DEFINE_CONSTANT(target, SQLITE_CREATE_VTABLE);
  NODE_DEFINE_CONSTANT(target, SQLITE_DROP_VTABLE);
  NODE_DEFINE_CONSTANT(target, SQLITE_FUNCTION);
  NODE_DEFINE_CONSTANT(target, SQLITE_SAVEPOINT);
  NODE_DEFINE_CONSTANT(target, SQLITE_COPY);
  NODE_DEFINE_CONSTANT(target, SQLITE_RECURSIVE);
}
```

<a id="ref-q1-23"></a>
### [23] `doc/api/sqlite.md:1241-1269`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/sqlite.md#L1241-L1269)

```markdown
One of the following constants is available as an argument to the `onConflict`
conflict resolution handler passed to [`database.applyChangeset()`][]. See also
[Constants Passed To The Conflict Handler][] in the SQLite documentation.

<table>
  <tr>
    <th>Constant</th>
    <th>Description</th>
  </tr>
  <tr>
    <td><code>SQLITE_CHANGESET_DATA</code></td>
    <td>The conflict handler is invoked with this constant when processing a DELETE or UPDATE change if a row with the required PRIMARY KEY fields is present in the database, but one or more other (non primary-key) fields modified by the update do not contain the expected "before" values.</td>
  </tr>
  <tr>
    <td><code>SQLITE_CHANGESET_NOTFOUND</code></td>
    <td>The conflict handler is invoked with this constant when processing a DELETE or UPDATE change if a row with the required PRIMARY KEY fields is not present in the database.</td>
  </tr>
  <tr>
    <td><code>SQLITE_CHANGESET_CONFLICT</code></td>
    <td>This constant is passed to the conflict handler while processing an INSERT change if the operation would result in duplicate primary key values.</td>
  </tr>
  <tr>
    <td><code>SQLITE_CHANGESET_CONSTRAINT</code></td>
    <td>If foreign key handling is enabled, and applying a changeset leaves the database in a state containing foreign key violations, the conflict handler is invoked with this constant exactly once before the changeset is committed. If the conflict handler returns <code>SQLITE_CHANGESET_OMIT</code>, the changes, including those that caused the foreign key constraint violation, are committed. Or, if it returns <code>SQLITE_CHANGESET_ABORT</code>, the changeset is rolled back.</td>
  </tr>
  <tr>
    <td><code>SQLITE_CHANGESET_FOREIGN_KEY</code></td>
    <td>If any other constraint violation occurs while applying a change (i.e. a UNIQUE, CHECK or NOT NULL constraint), the conflict handler is invoked with this constant.</td>
  </tr>
```

<a id="ref-q1-24"></a>
### [24] `doc/api/sqlite.md:1271-1292`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/sqlite.md#L1271-L1292)

```markdown

One of the following constants must be returned from the `onConflict` conflict
resolution handler passed to [`database.applyChangeset()`][]. See also
[Constants Returned From The Conflict Handler][] in the SQLite documentation.

<table>
  <tr>
    <th>Constant</th>
    <th>Description</th>
  </tr>
  <tr>
    <td><code>SQLITE_CHANGESET_OMIT</code></td>
    <td>Conflicting changes are omitted.</td>
  </tr>
  <tr>
    <td><code>SQLITE_CHANGESET_REPLACE</code></td>
    <td>Conflicting changes replace existing values. Note that this value can only be returned when the type of conflict is either <code>SQLITE_CHANGESET_DATA</code> or <code>SQLITE_CHANGESET_CONFLICT</code>.</td>
  </tr>
  <tr>
    <td><code>SQLITE_CHANGESET_ABORT</code></td>
    <td>Abort when a change encounters a conflict and roll back database.</td>
  </tr>
```

<a id="ref-q1-25"></a>
### [25] `src/node_sqlite.cc:3375-3383`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L3375-L3383)

```cpp
  NODE_DEFINE_CONSTANT(target, SQLITE_CHANGESET_OMIT);
  NODE_DEFINE_CONSTANT(target, SQLITE_CHANGESET_REPLACE);
  NODE_DEFINE_CONSTANT(target, SQLITE_CHANGESET_ABORT);

  NODE_DEFINE_CONSTANT(target, SQLITE_CHANGESET_DATA);
  NODE_DEFINE_CONSTANT(target, SQLITE_CHANGESET_NOTFOUND);
  NODE_DEFINE_CONSTANT(target, SQLITE_CHANGESET_CONFLICT);
  NODE_DEFINE_CONSTANT(target, SQLITE_CHANGESET_CONSTRAINT);
  NODE_DEFINE_CONSTANT(target, SQLITE_CHANGESET_FOREIGN_KEY);
```

<a id="ref-q1-26"></a>
### [26] `doc/api/sqlite.md:1297-1319`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/sqlite.md#L1297-L1319)

```markdown
The following constants are used with the [`database.setAuthorizer()`][] method.

##### Authorization result codes

One of the following constants must be returned from the authorizer callback
function passed to [`database.setAuthorizer()`][].

<table>
  <tr>
    <th>Constant</th>
    <th>Description</th>
  </tr>
  <tr>
    <td><code>SQLITE_OK</code></td>
    <td>Allow the operation to proceed normally.</td>
  </tr>
  <tr>
    <td><code>SQLITE_DENY</code></td>
    <td>Deny the operation and cause an error to be returned.</td>
  </tr>
  <tr>
    <td><code>SQLITE_IGNORE</code></td>
    <td>Ignore the operation and continue as if it had never been requested.</td>
```

<a id="ref-q1-27"></a>
### [27] `src/node_sqlite.cc:3386-3425`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_sqlite.cc#L3386-L3425)

```cpp
  NODE_DEFINE_CONSTANT(target, SQLITE_OK);
  NODE_DEFINE_CONSTANT(target, SQLITE_DENY);
  NODE_DEFINE_CONSTANT(target, SQLITE_IGNORE);

  // Authorization action codes
  NODE_DEFINE_CONSTANT(target, SQLITE_CREATE_INDEX);
  NODE_DEFINE_CONSTANT(target, SQLITE_CREATE_TABLE);
  NODE_DEFINE_CONSTANT(target, SQLITE_CREATE_TEMP_INDEX);
  NODE_DEFINE_CONSTANT(target, SQLITE_CREATE_TEMP_TABLE);
  NODE_DEFINE_CONSTANT(target, SQLITE_CREATE_TEMP_TRIGGER);
  NODE_DEFINE_CONSTANT(target, SQLITE_CREATE_TEMP_VIEW);
  NODE_DEFINE_CONSTANT(target, SQLITE_CREATE_TRIGGER);
  NODE_DEFINE_CONSTANT(target, SQLITE_CREATE_VIEW);
  NODE_DEFINE_CONSTANT(target, SQLITE_DELETE);
  NODE_DEFINE_CONSTANT(target, SQLITE_DROP_INDEX);
  NODE_DEFINE_CONSTANT(target, SQLITE_DROP_TABLE);
  NODE_DEFINE_CONSTANT(target, SQLITE_DROP_TEMP_INDEX);
  NODE_DEFINE_CONSTANT(target, SQLITE_DROP_TEMP_TABLE);
  NODE_DEFINE_CONSTANT(target, SQLITE_DROP_TEMP_TRIGGER);
  NODE_DEFINE_CONSTANT(target, SQLITE_DROP_TEMP_VIEW);
  NODE_DEFINE_CONSTANT(target, SQLITE_DROP_TRIGGER);
  NODE_DEFINE_CONSTANT(target, SQLITE_DROP_VIEW);
  NODE_DEFINE_CONSTANT(target, SQLITE_INSERT);
  NODE_DEFINE_CONSTANT(target, SQLITE_PRAGMA);
  NODE_DEFINE_CONSTANT(target, SQLITE_READ);
  NODE_DEFINE_CONSTANT(target, SQLITE_SELECT);
  NODE_DEFINE_CONSTANT(target, SQLITE_TRANSACTION);
  NODE_DEFINE_CONSTANT(target, SQLITE_UPDATE);
  NODE_DEFINE_CONSTANT(target, SQLITE_ATTACH);
  NODE_DEFINE_CONSTANT(target, SQLITE_DETACH);
  NODE_DEFINE_CONSTANT(target, SQLITE_ALTER_TABLE);
  NODE_DEFINE_CONSTANT(target, SQLITE_REINDEX);
  NODE_DEFINE_CONSTANT(target, SQLITE_ANALYZE);
  NODE_DEFINE_CONSTANT(target, SQLITE_CREATE_VTABLE);
  NODE_DEFINE_CONSTANT(target, SQLITE_DROP_VTABLE);
  NODE_DEFINE_CONSTANT(target, SQLITE_FUNCTION);
  NODE_DEFINE_CONSTANT(target, SQLITE_SAVEPOINT);
  NODE_DEFINE_CONSTANT(target, SQLITE_COPY);
  NODE_DEFINE_CONSTANT(target, SQLITE_RECURSIVE);
}
```

<a id="ref-q1-28"></a>
### [28] `doc/api/sqlite.md:611-613`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/sqlite.md#L611-L613)

```markdown
added:
  - v23.3.0
  - v22.12.0
```

<a id="ref-q1-29"></a>
### [29] `doc/changelogs/CHANGELOG_V22.md:2249-2252`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/changelogs/CHANGELOG_V22.md#L2249-L2252)

```markdown
#### SQLite Session Extension

Basic support for the [SQLite Session Extension](https://www.sqlite.org/sessionintro.html)
got added to the experimental `node:sqlite` module.
```

<a id="ref-q1-30"></a>
### [30] `doc/changelogs/CHANGELOG_V23.md:1258-1261`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/changelogs/CHANGELOG_V23.md#L1258-L1261)

```markdown
#### SQLite Session Extension

Basic support for the [SQLite Session Extension](https://www.sqlite.org/sessionintro.html)
got added to the experimental `node:sqlite` module.
```

<a id="ref-q1-31"></a>
### [31] `doc/changelogs/CHANGELOG_V22.md:2271-2275`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/changelogs/CHANGELOG_V22.md#L2271-L2275)

```markdown

Of note to distributors when dynamically linking with SQLite (using the `--shared-sqlite`
flag): compiling SQLite with `SQLITE_ENABLE_SESSION` and `SQLITE_ENABLE_PREUPDATE_HOOK`
defines is now required.
```

<a id="ref-q1-32"></a>
### [32] `test/parallel/test-sqlite-session.js:571-584`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-sqlite-session.js#L571-L584)

```javascript
test('session supports ERM', (t) => {
  const database = new DatabaseSync(':memory:');
  let afterDisposeSession;
  {
    using session = database.createSession();
    afterDisposeSession = session;
    const changeset = session.changeset();
    t.assert.ok(changeset instanceof Uint8Array);
    t.assert.strictEqual(changeset.length, 0);
  }
  t.assert.throws(() => afterDisposeSession.changeset(), {
    message: /session is not open/,
  });
});
```
