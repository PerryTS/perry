Fixed computed instance-field keys in nested and CommonJS-wrapped classes being
rewritten to unbound constructor capture parameters. Symbol-keyed fields now
retain the PropertyKey resolved at class definition time, allowing undici's
pool state to initialize under the symbols used by its request path.
