# BLE wake and persistent PAN

## Findings

BLE wake and NetworkManager PAN are separate paths. Existing code called Device1.Disconnect after invite completion and failed connect attempts; BlueZ defines that operation as disconnecting all profiles and ACL, so a shared phone object could lose PAN. This establishes a possible interference path, not historical proof for a reported NAP boot failure. The live PAN was already connected when investigated.

## Change

Snapshot connection state before BLE connect. Cleanup preserves initially connected or unverified devices. For an owned attempt, reread service UUIDs with a bounded wait: Classic service classes (including PANU, NAP, GN and audio), missing metadata or empty metadata all prevent global disconnect. Only a known owned LE-only link uses the old cancellation/cleanup path. Adapter power, pairing, trust and NM activation are not changed by this policy.

On this host, set the existing user-selected PAN profile to autoconnect with unlimited retries and IPv4/IPv6 never-default, without deactivating it. These machine-specific identifiers/configuration are not committed. Phone tethering must remain enabled; NM scheduling/backoff still applies.

## Sources and verification

[BlueZ Device API](https://bluez.readthedocs.io/en/latest/device-api/) defines generic connect/disconnect scope. [Network API](https://bluez.readthedocs.io/en/latest/network-api/) exposes PAN connection state. [NetworkManager settings](https://www.networkmanager.dev/docs/api/latest/nm-settings-nmcli.html) define retry zero and never-default behavior. Pure tests cover ownership/profile combinations; workspace release checks and non-disruptive live state/route verification complete validation. No raw Bluetooth addresses, phone identifiers or personal logs enter the repository.

Verified: locked workspace release build passed without compiler warnings; 341 tests passed, 0 failed, 40 environment-dependent tests skipped. With no active/pending projection, the local user daemon was reloaded. The existing PAN remained connected at all ten samples across twenty seconds, Smart Connect reconnected, and both IPv4 and IPv6 default routes remained on Wi-Fi. The selected NM profile persistently reports autoconnect enabled, retry count zero and never-default for both address families. No pairing/trust changes or Bluetooth adapter/profile resets were performed. This is observation of the current setup, not a guarantee while phone tethering is disabled or out of range.
