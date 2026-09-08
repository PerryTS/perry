Add native registry-instance identities and monotonically issued registration
serials while preserving the numeric provider ABI. Coordinate wrapper and
operation leases with pending insertion, retirement, both quarantine tiers,
and bounded id reuse across FFI, Common, and External Events registries. Net
reservations retain their private registry domain while sharing the FFI id pool.
Explicit Common ids reject occupied or retained slots. JavaScript publication
and receiver representation remain unchanged in this preparatory phase.
