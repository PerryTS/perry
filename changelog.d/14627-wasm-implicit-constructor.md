`--target web`: a class without a constructor ran nothing on `new`, so its field
initializers and its parent's constructor were skipped and their fields read
`undefined`. Such a class now runs JavaScript's implicit constructor.
