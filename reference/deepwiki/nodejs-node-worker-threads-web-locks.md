# DeepWiki Response

Repository: `nodejs/node`
Question: How is worker_threads.locks request/query implemented and what observable LockManager/Lock shapes does Node expose?
Search: https://deepwiki.com/search/how-is-workerthreadslocks-requ_913814f1-c719-4368-9e44-a2b51b10b766

**Question**: How is worker_threads.locks request/query implemented and what observable LockManager/Lock shapes does Node expose?

The `worker_threads.locks` API in Node.js provides a `LockManager` instance for coordinating access to shared resources across multiple threads, mirroring the browser's Web Locks API [1](#ref-q1-1) . The implementation is primarily in C++ within `src/node_locks.cc` [2](#ref-q1-2) .

## `locks.request()` Implementation

The `locks.request()` method is implemented by the `LockManager::Request` C++ function [3](#ref-q1-3) .

1.  **Argument Parsing**: It takes `name`, `clientId`, `mode`, `steal`, `ifAvailable`, and a `callback` as arguments [4](#ref-q1-4) .
2.  **Promise Creation**: Two `v8::Promise::Resolver` instances, `waiting_promise` and `released_promise`, are created to manage the asynchronous nature of lock acquisition and release [5](#ref-q1-5) . These promises are marked as handled to prevent unhandled rejection warnings [6](#ref-q1-6) .
3.  **Lock Request Creation**: A `LockRequest` object is created, encapsulating the lock's details and the associated promises and callback [7](#ref-q1-7) .
4.  **Queueing**: The `LockRequest` is added to the `pending_queue_` of the `LockManager`. If the `steal` option is `true`, the request is placed at the front of the queue, giving it priority [8](#ref-q1-8) .
5.  **Processing Queue**: The `LockManager::ProcessQueue` method is then called to attempt to grant pending lock requests [9](#ref-q1-9) .
6.  **Return Value**: The `released_promise`'s promise object is returned to JavaScript, which resolves once the lock is released [10](#ref-q1-10) .

The `LockManager::ProcessQueue` function handles the core logic for granting locks [11](#ref-q1-11) . It iterates through the `pending_queue_`, determines if a request is grantable using `LockManager::IsGrantable` [12](#ref-q1-12) , and if so, executes the JavaScript callback associated with the lock [13](#ref-q1-13) . If `ifAvailable` is true and the lock cannot be granted, the callback is invoked with `null` [14](#ref-q1-14) . If a lock is stolen, existing locks with the same name are marked as stolen, their `released_promise` is rejected, and their owners are notified [15](#ref-q1-15) .

## `locks.query()` Implementation

The `locks.query()` method is implemented by the `LockManager::Query` C++ function [16](#ref-q1-16) .

1.  **Promise Creation**: A `v8::Promise::Resolver` is created, and its promise is immediately returned to JavaScript [17](#ref-q1-17) .
2.  **Collecting Lock Information**: It iterates through `held_locks_` and `pending_queue_` to gather information about currently held and pending locks for the current environment [18](#ref-q1-18) .
3.  **Creating LockInfo Objects**: For each lock or pending request, a `LockInfo` object is created using `CreateLockInfoObject` [19](#ref-q1-19) . This object contains the `name` and `mode` of the lock [20](#ref-q1-20) .
4.  **Resolving Promise**: The collected `held` and `pending` lock lists are then used to create a `LockManagerSnapshot` object, which is used to resolve the promise returned in step 1 [21](#ref-q1-21) .

## Observable `LockManager` and `Lock` Shapes

Node.js exposes `LockManager` and `Lock` objects that mirror the browser's Web Locks API [22](#ref-q1-22) .

### `LockManager`

The `LockManager` instance is available via `worker_threads.locks` [23](#ref-q1-23)  or `navigator.locks` [24](#ref-q1-24) . It exposes two primary methods:

*   `locks.request(name[, options], callback)`: Requests a lock with a given `name`, `options` (including `mode`, `ifAvailable`, `steal`, `signal`), and a `callback` function [25](#ref-q1-25) . It returns a `Promise` that resolves when the lock is released [26](#ref-q1-26) .
*   `locks.query()`: Returns a `Promise` that resolves with a `LockManagerSnapshot` object [27](#ref-q1-27) .

### `Lock`

The `Lock` object is passed to the callback function of `locks.request()` when a lock is granted [28](#ref-q1-28) . It has the following properties:

*   `lock.name`: A `string` representing the name of the lock [29](#ref-q1-29) .
*   `lock.mode`: A `string` indicating the mode of the lock, either `'shared'` or `'exclusive'` [30](#ref-q1-30) .

### `LockManagerSnapshot`

The `LockManagerSnapshot` object returned by `locks.query()` has two properties:

*   `snapshot.held`: An array of `Lock` objects representing locks currently held by the current process [31](#ref-q1-31) .
*   `snapshot.pending`: An array of objects, each describing a pending lock request, with `name` and `mode` properties [32](#ref-q1-32) .

## Notes

The `worker_threads.locks` API is marked as experimental [33](#ref-q1-33) . The `LockManager` and `Lock` objects are implemented in C++ in `src/node_locks.cc` [34](#ref-q1-34)  [35](#ref-q1-35) . The JavaScript-facing API is exposed through `lib/worker_threads.js` which imports `locks` from `internal/locks` [36](#ref-q1-36) .

Wiki pages you might want to explore:
- [Core Runtime Architecture (nodejs/node)](/wiki/nodejs/node#2)

View this search on DeepWiki: https://deepwiki.com/search/how-is-workerthreadslocks-requ_913814f1-c719-4368-9e44-a2b51b10b766

## References

<a id="ref-q1-1"></a>
### [1] `doc/api/worker_threads.md:771-785`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L771-L785)

```markdown
## `worker_threads.locks`

<!-- YAML
added: v24.5.0
-->

> Stability: 1 - Experimental

* {LockManager}

An instance of a [`LockManager`][LockManager] that can be used to coordinate
access to resources that may be shared across multiple threads within the same
process. The API mirrors the semantics of the
[browser `LockManager`][]
```

<a id="ref-q1-2"></a>
### [2] `src/node_locks.cc:1-125`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_locks.cc#L1-L125)

```cpp
#include "node_locks.h"

#include "base_object-inl.h"
#include "env-inl.h"
#include "node_errors.h"
#include "node_external_reference.h"
#include "node_internals.h"
#include "util-inl.h"
#include "v8.h"

namespace node::worker::locks {

using node::errors::TryCatchScope;
using v8::Array;
using v8::Context;
using v8::DictionaryTemplate;
using v8::Exception;
using v8::Function;
using v8::FunctionCallbackInfo;
using v8::FunctionTemplate;
using v8::HandleScope;
using v8::Isolate;
using v8::Local;
using v8::LocalVector;
using v8::MaybeLocal;
using v8::Object;
using v8::ObjectTemplate;
using v8::Promise;
using v8::PropertyAttribute;
using v8::Value;

// Reject two promises and return `false` on failure.
static bool RejectBoth(Local<Context> ctx,
                       Local<Promise::Resolver> first,
                       Local<Promise::Resolver> second,
                       Local<Value> reason) {
  return first->Reject(ctx, reason).IsJust() &&
         second->Reject(ctx, reason).IsJust();
}

Lock::Lock(Environment* env,
           const std::u16string& name,
           Mode mode,
           const std::string& client_id,
           Local<Promise::Resolver> waiting,
           Local<Promise::Resolver> released)
    : env_(env), name_(name), mode_(mode), client_id_(client_id) {
  waiting_promise_.Reset(env_->isolate(), waiting);
  released_promise_.Reset(env_->isolate(), released);
}

void Lock::MemoryInfo(node::MemoryTracker* tracker) const {
  tracker->TrackFieldWithSize("name", name_.size());
  tracker->TrackField("client_id", client_id_);
  tracker->TrackField("waiting_promise", waiting_promise_);
  tracker->TrackField("released_promise", released_promise_);
}

LockRequest::LockRequest(Environment* env,
                         Local<Promise::Resolver> waiting,
                         Local<Promise::Resolver> released,
                         Local<Function> callback,
                         const std::u16string& name,
                         Lock::Mode mode,
                         std::string client_id,
                         bool steal,
                         bool if_available)
    : env_(env),
      name_(name),
      mode_(mode),
      client_id_(std::move(client_id)),
      steal_(steal),
      if_available_(if_available) {
  waiting_promise_.Reset(env_->isolate(), waiting);
  released_promise_.Reset(env_->isolate(), released);
  callback_.Reset(env_->isolate(), callback);
}

Local<DictionaryTemplate> GetLockInfoTemplate(Environment* env) {
  auto tmpl = env->lock_info_template();
  if (tmpl.IsEmpty()) {
    static constexpr std::string_view names[] = {
        "name",
        "mode",
        "clientId",
    };
    tmpl = DictionaryTemplate::New(env->isolate(), names);
    env->set_lock_info_template(tmpl);
  }
  return tmpl;
}

// The request here can be either a Lock or a LockRequest.
static MaybeLocal<Object> CreateLockInfoObject(Environment* env,
                                               const auto& request) {
  auto tmpl = GetLockInfoTemplate(env);
  MaybeLocal<Value> values[] = {
      ToV8Value(env->context(), request.name()),
      request.mode() == Lock::Mode::Exclusive ? env->exclusive_string()
                                              : env->shared_string(),
      ToV8Value(env->context(), request.client_id()),
  };

  return NewDictionaryInstance(env->context(), tmpl, values);
}

bool LockManager::IsGrantable(const LockRequest* request) const {
  // Steal requests bypass all normal granting rules
  if (request->steal()) return true;

  auto held_locks_iter = held_locks_.find(request->name());
  // No existing locks for this resource name
  if (held_locks_iter == held_locks_.end()) return true;

  // Exclusive requests cannot coexist with any existing locks
  if (request->mode() == Lock::Mode::Exclusive) return false;

  // For shared requests, check if any existing lock is exclusive
  for (const auto& existing_lock : held_locks_iter->second) {
    if (existing_lock->mode() == Lock::Mode::Exclusive) return false;
  }
  // All existing locks are shared, so this shared request can be granted
  return true;
}
```

<a id="ref-q1-3"></a>
### [3] `src/node_locks.cc:568-633`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_locks.cc#L568-L633)

```cpp
void LockManager::Request(const FunctionCallbackInfo<Value>& args) {
  Environment* env = Environment::GetCurrent(args);
  Isolate* isolate = env->isolate();
  HandleScope scope(isolate);
  Local<Context> context = env->context();

  CHECK_EQ(args.Length(), 6);
  CHECK(args[0]->IsString());    // name
  CHECK(args[1]->IsString());    // clientId
  CHECK(args[2]->IsString());    // mode
  CHECK(args[3]->IsBoolean());   // steal
  CHECK(args[4]->IsBoolean());   // ifAvailable
  CHECK(args[5]->IsFunction());  // callback

  TwoByteValue resource_name(isolate, args[0]);
  Utf8Value client_id(isolate, args[1]);
  Utf8Value mode(isolate, args[2]);
  bool steal = args[3]->BooleanValue(isolate);
  bool if_available = args[4]->BooleanValue(isolate);
  Local<Function> callback = args[5].As<Function>();

  Local<Promise::Resolver> waiting_promise;
  Local<Promise::Resolver> released_promise;

  if (!Promise::Resolver::New(context).ToLocal(&waiting_promise) ||
      !Promise::Resolver::New(context).ToLocal(&released_promise)) {
    return;
  }

  // Mark both internal promises as handled to prevent unhandled rejection
  // warnings
  waiting_promise->GetPromise()->MarkAsHandled();
  released_promise->GetPromise()->MarkAsHandled();

  LockManager* manager = GetCurrent();
  {
    Mutex::ScopedLock scoped_lock(manager->mutex_);

    // Register cleanup hook for the environment only once
    if (manager->registered_envs_.insert(env).second) {
      env->AddCleanupHook(LockManager::OnEnvironmentCleanup, env);
    }

    auto lock_request = std::make_unique<LockRequest>(
        env,
        waiting_promise,
        released_promise,
        callback,
        resource_name.ToU16String(),
        mode.ToStringView() == "shared" ? Lock::Mode::Shared
                                        : Lock::Mode::Exclusive,
        client_id.ToString(),
        steal,
        if_available);
    // Steal requests get priority by going to front of queue
    if (steal) {
      manager->pending_queue_.emplace_front(std::move(lock_request));
    } else {
      manager->pending_queue_.push_back(std::move(lock_request));
    }
  }

  manager->ProcessQueue(env);

  args.GetReturnValue().Set(released_promise->GetPromise());
}
```

<a id="ref-q1-4"></a>
### [4] `src/node_locks.cc:574-587`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_locks.cc#L574-L587)

```cpp
  CHECK_EQ(args.Length(), 6);
  CHECK(args[0]->IsString());    // name
  CHECK(args[1]->IsString());    // clientId
  CHECK(args[2]->IsString());    // mode
  CHECK(args[3]->IsBoolean());   // steal
  CHECK(args[4]->IsBoolean());   // ifAvailable
  CHECK(args[5]->IsFunction());  // callback

  TwoByteValue resource_name(isolate, args[0]);
  Utf8Value client_id(isolate, args[1]);
  Utf8Value mode(isolate, args[2]);
  bool steal = args[3]->BooleanValue(isolate);
  bool if_available = args[4]->BooleanValue(isolate);
  Local<Function> callback = args[5].As<Function>();
```

<a id="ref-q1-5"></a>
### [5] `src/node_locks.cc:589-595`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_locks.cc#L589-L595)

```cpp
  Local<Promise::Resolver> waiting_promise;
  Local<Promise::Resolver> released_promise;

  if (!Promise::Resolver::New(context).ToLocal(&waiting_promise) ||
      !Promise::Resolver::New(context).ToLocal(&released_promise)) {
    return;
  }
```

<a id="ref-q1-6"></a>
### [6] `src/node_locks.cc:599-600`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_locks.cc#L599-L600)

```cpp
  waiting_promise->GetPromise()->MarkAsHandled();
  released_promise->GetPromise()->MarkAsHandled();
```

<a id="ref-q1-7"></a>
### [7] `src/node_locks.cc:611-621`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_locks.cc#L611-L621)

```cpp
    auto lock_request = std::make_unique<LockRequest>(
        env,
        waiting_promise,
        released_promise,
        callback,
        resource_name.ToU16String(),
        mode.ToStringView() == "shared" ? Lock::Mode::Shared
                                        : Lock::Mode::Exclusive,
        client_id.ToString(),
        steal,
        if_available);
```

<a id="ref-q1-8"></a>
### [8] `src/node_locks.cc:622-627`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_locks.cc#L622-L627)

```cpp
    // Steal requests get priority by going to front of queue
    if (steal) {
      manager->pending_queue_.emplace_front(std::move(lock_request));
    } else {
      manager->pending_queue_.push_back(std::move(lock_request));
    }
```

<a id="ref-q1-9"></a>
### [9] `src/node_locks.cc:630`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_locks.cc#L630)

```cpp
  manager->ProcessQueue(env);
```

<a id="ref-q1-10"></a>
### [10] `src/node_locks.cc:632`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_locks.cc#L632)

```cpp
  args.GetReturnValue().Set(released_promise->GetPromise());
```

<a id="ref-q1-11"></a>
### [11] `src/node_locks.cc:222`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_locks.cc#L222)

```cpp
void LockManager::ProcessQueue(Environment* env) {
```

<a id="ref-q1-12"></a>
### [12] `src/node_locks.cc:291`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_locks.cc#L291)

```cpp
        if (should_wait_for_earlier_requests || !IsGrantable(request)) {
```

<a id="ref-q1-13"></a>
### [13] `src/node_locks.cc:473-484`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_locks.cc#L473-L484)

```cpp
      if (!grantable_request->callback()
               ->Call(context, Undefined(isolate), 1, &callback_arg)
               .ToLocal(&callback_result)) {
        // We don't really need to check the return value here since
        // we're returning early in either case.
        USE(RejectBoth(context,
                       grantable_request->waiting_promise(),
                       grantable_request->released_promise(),
                       try_catch_scope.Exception()));
        return;
      }
    }
```

<a id="ref-q1-14"></a>
### [14] `src/node_locks.cc:321-322`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_locks.cc#L321-L322)

```cpp
    if (if_available_request) {
      Local<Value> null_arg = Null(isolate);
```

<a id="ref-q1-15"></a>
### [15] `src/node_locks.cc:404-422`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_locks.cc#L404-L422)

```cpp
    if (grantable_request->steal()) {
      std::unordered_set<Environment*> envs_to_notify;

      {
        Mutex::ScopedLock scoped_lock(mutex_);
        auto held_locks_iter = held_locks_.find(grantable_request->name());
        if (held_locks_iter != held_locks_.end()) {
          // Mark existing locks as stolen and collect environments to notify
          for (auto& existing_lock : held_locks_iter->second) {
            existing_lock->mark_stolen();
            envs_to_notify.insert(existing_lock->env());

            Local<Value> error =
                Exception::Error(FIXED_ONE_BYTE_STRING(isolate, "LOCK_STOLEN"));

            if (existing_lock->released_promise()
                    ->Reject(context, error)
                    .IsNothing())
              return;
```

<a id="ref-q1-16"></a>
### [16] `src/node_locks.cc:635-701`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_locks.cc#L635-L701)

```cpp
void LockManager::Query(const FunctionCallbackInfo<Value>& args) {
  Environment* env = Environment::GetCurrent(args);
  Isolate* isolate = env->isolate();
  HandleScope scope(isolate);
  Local<Context> context = env->context();

  Local<Promise::Resolver> resolver;
  if (!Promise::Resolver::New(context).ToLocal(&resolver)) {
    return;
  }

  // Always set the return value first so Javascript gets a promise
  args.GetReturnValue().Set(resolver->GetPromise());

  LocalVector<Value> held_list(isolate);
  LocalVector<Value> pending_list(isolate);
  LockManager* manager = GetCurrent();

  {
    Mutex::ScopedLock scoped_lock(manager->mutex_);

    Local<Object> lock_info;
    for (const auto& resource_entry : manager->held_locks_) {
      for (const auto& held_lock : resource_entry.second) {
        if (held_lock->env() == env) {
          if (!CreateLockInfoObject(env, *held_lock).ToLocal(&lock_info)) {
            // There should already be a pending exception scheduled.
            return;
          }
          held_list.push_back(lock_info);
        }
      }
    }

    for (const auto& pending_request : manager->pending_queue_) {
      if (pending_request->env() == env) {
        if (!CreateLockInfoObject(env, *pending_request).ToLocal(&lock_info)) {
          // There should already be a pending exception scheduled.
          return;
        }
        pending_list.push_back(lock_info);
      }
    }
  }

  auto tmpl = env->lock_query_template();
  if (tmpl.IsEmpty()) {
    static constexpr std::string_view names[] = {
        "held",
        "pending",
    };
    tmpl = DictionaryTemplate::New(isolate, names);
    env->set_lock_query_template(tmpl);
  }

  MaybeLocal<Value> values[] = {
      Array::New(isolate, held_list.data(), held_list.size()),
      Array::New(isolate, pending_list.data(), pending_list.size()),
  };

  Local<Object> result;
  if (NewDictionaryInstance(env->context(), tmpl, values).ToLocal(&result)) {
    // There's no reason to check IsNothing here since we're just returning.
    USE(resolver->Resolve(context, result));
  }
}
```

<a id="ref-q1-17"></a>
### [17] `src/node_locks.cc:641-647`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_locks.cc#L641-L647)

```cpp
  Local<Promise::Resolver> resolver;
  if (!Promise::Resolver::New(context).ToLocal(&resolver)) {
    return;
  }

  // Always set the return value first so Javascript gets a promise
  args.GetReturnValue().Set(resolver->GetPromise());
```

<a id="ref-q1-18"></a>
### [18] `src/node_locks.cc:657-677`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_locks.cc#L657-L677)

```cpp
    for (const auto& resource_entry : manager->held_locks_) {
      for (const auto& held_lock : resource_entry.second) {
        if (held_lock->env() == env) {
          if (!CreateLockInfoObject(env, *held_lock).ToLocal(&lock_info)) {
            // There should already be a pending exception scheduled.
            return;
          }
          held_list.push_back(lock_info);
        }
      }
    }

    for (const auto& pending_request : manager->pending_queue_) {
      if (pending_request->env() == env) {
        if (!CreateLockInfoObject(env, *pending_request).ToLocal(&lock_info)) {
          // There should already be a pending exception scheduled.
          return;
        }
        pending_list.push_back(lock_info);
      }
    }
```

<a id="ref-q1-19"></a>
### [19] `src/node_locks.cc:94-105`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_locks.cc#L94-L105)

```cpp
static MaybeLocal<Object> CreateLockInfoObject(Environment* env,
                                               const auto& request) {
  auto tmpl = GetLockInfoTemplate(env);
  MaybeLocal<Value> values[] = {
      ToV8Value(env->context(), request.name()),
      request.mode() == Lock::Mode::Exclusive ? env->exclusive_string()
                                              : env->shared_string(),
      ToV8Value(env->context(), request.client_id()),
  };

  return NewDictionaryInstance(env->context(), tmpl, values);
}
```

<a id="ref-q1-20"></a>
### [20] `src/node_locks.cc:98-101`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_locks.cc#L98-L101)

```cpp
      ToV8Value(env->context(), request.name()),
      request.mode() == Lock::Mode::Exclusive ? env->exclusive_string()
                                              : env->shared_string(),
      ToV8Value(env->context(), request.client_id()),
```

<a id="ref-q1-21"></a>
### [21] `src/node_locks.cc:690-699`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_locks.cc#L690-L699)

```cpp
  MaybeLocal<Value> values[] = {
      Array::New(isolate, held_list.data(), held_list.size()),
      Array::New(isolate, pending_list.data(), pending_list.size()),
  };

  Local<Object> result;
  if (NewDictionaryInstance(env->context(), tmpl, values).ToLocal(&result)) {
    // There's no reason to check IsNothing here since we're just returning.
    USE(resolver->Resolve(context, result));
  }
```

<a id="ref-q1-22"></a>
### [22] `doc/api/worker_threads.md:781-785`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L781-L785)

```markdown
An instance of a [`LockManager`][LockManager] that can be used to coordinate
access to resources that may be shared across multiple threads within the same
process. The API mirrors the semantics of the
[browser `LockManager`][]
```

<a id="ref-q1-23"></a>
### [23] `doc/api/worker_threads.md:779-781`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L779-L781)

```markdown
* {LockManager}

An instance of a [`LockManager`][LockManager] that can be used to coordinate
```

<a id="ref-q1-24"></a>
### [24] `doc/api/globals.md:814-825`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/globals.md#L814-L825)

```markdown
### `navigator.locks`

<!-- YAML
added: v24.5.0
-->

> Stability: 1 - Experimental

The `navigator.locks` read-only property returns a [`LockManager`][] instance that
can be used to coordinate access to resources that may be shared across multiple
threads within the same process. This global implementation matches the semantics
of the [browser `LockManager`][] API.
```

<a id="ref-q1-25"></a>
### [25] `doc/api/worker_threads.md:836-857`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L836-L857)

```markdown
#### `locks.request(name[, options], callback)`

<!-- YAML
added: v24.5.0
-->

* `name` {string}
* `options` {Object}
  * `mode` {string} Either `'exclusive'` or `'shared'`. **Default:** `'exclusive'`.
  * `ifAvailable` {boolean} If `true`, the request will only be granted if the
    lock is not already held. If it cannot be granted, `callback` will be
    invoked with `null` instead of a `Lock` instance. **Default:** `false`.
  * `steal` {boolean} If `true`, any existing locks with the same name are
    released and the request is granted immediately, pre-empting any queued
    requests. **Default:** `false`.
  * `signal` {AbortSignal} that can be used to abort a
    pending (but not yet granted) lock request.
* `callback` {Function} Invoked once the lock is granted (or immediately with
  `null` if `ifAvailable` is `true` and the lock is unavailable). The lock is
  released automatically when the function returns, or—if the function returns
  a promise—when that promise settles.
* Returns: {Promise} Resolves once the lock has been released.
```

<a id="ref-q1-26"></a>
### [26] `doc/api/worker_threads.md:857-858`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L857-L858)

```markdown
* Returns: {Promise} Resolves once the lock has been released.
```

<a id="ref-q1-27"></a>
### [27] `doc/api/worker_threads.md:880-888`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L880-L888)

```markdown
#### `locks.query()`

<!-- YAML
added: v24.5.0
-->

* Returns: {Promise}

Resolves with a `LockManagerSnapshot` describing the currently held and pending
```

<a id="ref-q1-28"></a>
### [28] `doc/api/worker_threads.md:791-794`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L791-L794)

```markdown

The `Lock` interface provides information about a lock that has been granted via
[`locks.request()`][locks.request()]
```

<a id="ref-q1-29"></a>
### [29] `doc/api/worker_threads.md:801-804`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L801-L804)

```markdown
* {string}

The name of the lock.
```

<a id="ref-q1-30"></a>
### [30] `doc/api/worker_threads.md:811-813`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L811-L813)

```markdown
* {string}

The mode of the lock. Either `shared` or `exclusive`.
```

<a id="ref-q1-31"></a>
### [31] `doc/api/worker_threads.md:894-896`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L894-L896)

```markdown
const snapshot = await locks.query();
for (const lock of snapshot.held) {
  console.log(`held lock: name ${lock.name}, mode ${lock.mode}`);
```

<a id="ref-q1-32"></a>
### [32] `doc/api/worker_threads.md:898-900`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L898-L900)

```markdown
for (const pending of snapshot.pending) {
  console.log(`pending lock: name ${pending.name}, mode ${pending.mode}`);
}
```

<a id="ref-q1-33"></a>
### [33] `doc/api/worker_threads.md:777`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/worker_threads.md#L777)

```markdown
> Stability: 1 - Experimental
```

<a id="ref-q1-34"></a>
### [34] `src/node_locks.cc:41-50`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_locks.cc#L41-L50)

```cpp
Lock::Lock(Environment* env,
           const std::u16string& name,
           Mode mode,
           const std::string& client_id,
           Local<Promise::Resolver> waiting,
           Local<Promise::Resolver> released)
    : env_(env), name_(name), mode_(mode), client_id_(client_id) {
  waiting_promise_.Reset(env_->isolate(), waiting);
  released_promise_.Reset(env_->isolate(), released);
}
```

<a id="ref-q1-35"></a>
### [35] `src/node_locks.cc:59-77`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/src/node_locks.cc#L59-L77)

```cpp
LockRequest::LockRequest(Environment* env,
                         Local<Promise::Resolver> waiting,
                         Local<Promise::Resolver> released,
                         Local<Function> callback,
                         const std::u16string& name,
                         Lock::Mode mode,
                         std::string client_id,
                         bool steal,
                         bool if_available)
    : env_(env),
      name_(name),
      mode_(mode),
      client_id_(std::move(client_id)),
      steal_(steal),
      if_available_(if_available) {
  waiting_promise_.Reset(env_->isolate(), waiting);
  released_promise_.Reset(env_->isolate(), released);
  callback_.Reset(env_->isolate(), callback);
}
```

<a id="ref-q1-36"></a>
### [36] `lib/worker_threads.js:33`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/worker_threads.js#L33)

```javascript
const { locks } = require('internal/locks');
```
