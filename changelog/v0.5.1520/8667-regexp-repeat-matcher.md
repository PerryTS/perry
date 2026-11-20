Fixed RegExp backtracking behavior for quantified capture groups, nullable
iterations, and captures referenced across lookaround assertions. Perry now
uses an ECMAScript matcher for the affected patterns while retaining the
existing linear-time engines for the common path.

Regular-expression replacement also now matches JavaScript strings containing
lone surrogates as UTF-16 code units without losing their WTF-8 representation.
Together these changes make all 89 test262 cases tracked by #5897 pass.
