### Fixed

- Remove a member from its previous native domain when `Domain.add()` transfers it to another domain. Reuse the existing removal helper, root the previous owner, and refetch destination members after potentially allocating operations. The original node-parity transfer regression now matches Node; domain parity remains partial with 11 of 48 cases passing and 37 known failures unchanged.
