### Fix ConstFn finalizer metadata allocating heap strings

Emit packed property names for ConstFn finalizers as read-only byte constants.
Previously these metadata blobs entered the JavaScript string pool, allocating
and permanently rooting strings whose handles were never used. The Zod
enabled/disabled comparison exposed three extra strings totaling 168 heap bytes.
The finalizer still receives the same packed bytes and length; shape identity,
receiver validation, and closure rooting are unchanged.
