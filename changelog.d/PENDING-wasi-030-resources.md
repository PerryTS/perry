Commit ownership of generated async export results before canonical return.
Retain incoming canonical borrows through suspension and retire them at return
or cancellation, independently of later task exit. Keep the public async and
resource-profile guards while the full WASI 0.3.0 profile is qualified.
