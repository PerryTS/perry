Whole-program facts: a global the realm declares (`Error`, `JSON`, `Promise`)
is read as what the program stored under its name on the global object plus
the realm's value. It also reads as unknown once something whose value the
facts do not carry may have written it: unknown code holding the global
object, a store into an unknown value once the global object reached unknown
code, an accessor, or an unseen module, which may write any global when it
loads. The bare name is read off the unresolved-global sentinel, which is the
same object as `globalThis`, so a write through either spelling is now seen by
a read through either. Before, both spellings read the realm's value whatever
had been stored.
