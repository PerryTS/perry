AbortController, AbortSignal, EventTarget and Event objects are born in their final shape and read their internal state from slots (`new AbortController().signal` 133k → 3.8k instructions).
