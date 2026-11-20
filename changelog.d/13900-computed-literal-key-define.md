Fix computed object-literal keys: `{[k]: v}` defines the property instead of assigning it, so `{["__proto__"]: v}` no longer changes the prototype and inherited setters no longer run.
