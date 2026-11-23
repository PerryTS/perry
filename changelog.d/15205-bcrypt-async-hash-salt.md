`bcrypt.hash` takes a salt string or a cost as its second argument, like
`hashSync`: a cost of `0` is 10 and any other is clamped to 4..31. Both forms
were rejected before.
