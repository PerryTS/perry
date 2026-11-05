Runtime throws now record where their values came from; the affected Buffer and tty errors are real Error objects whose `message` and `name` are no longer own enumerable keys, as in Node.
