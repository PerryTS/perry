A for-of inside a generator body over a proven sync generator now resumes that generator's body directly instead of going through the runtime iterator step.
