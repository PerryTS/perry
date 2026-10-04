### ConstFn method lanes on by default

Executables now build step 5C ConstFn lanes unless `PERRY_CONSTFN_SHAPE=0`:
a shape slot that has only ever held one function body records it, and a
method site whose shape compare passes calls that body directly. Classless
modules now take part in frontend shape discovery as well. Dylibs still never
advertise a permanent body. `PERRY_CONSTFN_SHAPE=0` restores the previous
behaviour.
