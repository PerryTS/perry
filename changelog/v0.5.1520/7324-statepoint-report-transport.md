#7322 deleted `PERRY_STATEPOINT_REPORT` as a user knob under the GC kill-policy,
leaving it as the driver's internal transport to the rayon module workers. Two
gaps remained, both raised in review:

- `statepoint_report::enabled()` still reads the variable, so a value inherited
  from the user's environment switched reporting on without `--statepoint-report`.
  The env spelling was therefore still a knob in fact.
- The variable was set only when the flag was present and never cleared. `perry
  dev` reuses its process, so one reporting build made the flag sticky for every
  later compile in that process.

The driver now writes the variable unconditionally — set when the flag is given,
removed when it is not — so the flag is the single entry point in fact rather
than by convention.
