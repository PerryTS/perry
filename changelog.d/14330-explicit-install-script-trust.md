### Security

- Require explicit approval for dependency lifecycle scripts in `perry install`. Match approvals to installed dependency names and aliases, not self-declared package names, and remove implicit trust for bundled names and prefixes.
