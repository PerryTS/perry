// String.prototype.replace / replaceAll with a *string* pattern must process
// the special replacement patterns from ECMAScript GetSubstitution:
//   $$ -> $    $& -> matched    $` -> text before match    $' -> text after
// A string pattern has no capture groups, so $1 / $<name> stay literal.

function show(label: string, value: any) {
  console.log(label + " = " + value);
}

// replace (first occurrence).
show("$$", "abc".replace("b", "$$"));
show("$&", "abc".replace("b", "[$&]"));
show("$`", "abc".replace("b", "$`"));
show("$'", "abc".replace("b", "$'"));
show("$1-literal", "abc".replace("b", "$1"));
show("mixed", "xby".replace("b", "$$$&$`$'"));
show("price", "cost 100".replace("100", "$$100"));
show("trailing-$", "ab".replace("a", "x$"));

// replaceAll.
show("all-$$", "a.b.c".replaceAll(".", "$$"));
show("all-$&", "a.b.c".replaceAll(".", "[$&]"));
show("all-mixed", "1x2x3".replaceAll("x", "-$&-"));

// Regression: no `$`, no match, empty pattern.
show("plain", "a.b.c".replaceAll(".", "-"));
show("plain-first", "abc".replace("b", "X"));
show("no-match", "abc".replace("z", "$$"));
show("empty-pat", "abc".replace("", "$"));

// UTF-8 safety around and inside the match.
show("utf8-repl", "héllo".replace("l", "$&$&"));
show("utf8-around", "aXé".replace("X", "[$`|$']"));

// The regex path is unchanged (still supports $1 captures).
show("regex-$$", "abc".replace(/b/, "$$"));
show("regex-$1", "2024-03".replace(/(\d+)-(\d+)/, "$2/$1"));
