# Prefer LAN addresses over PAN

## Cause

All private interfaces were sorted by IP, allowing Bluetooth tethering to be advertised first. The latest authenticated mDNS/session address overwrote the remembered phone endpoint. ADB reused the actual session peer, and scrcpy could select a lone TCP device even when it did not match that peer. Linux had renamed the PAN interface to an Ethernet-looking name, so prefix filtering would be unreliable.

## Change

Read kernel DEVTYPE and interface/subnet metadata. Prefer Wi-Fi then Ethernet then USB; omit PAN while ordinary LAN exists. Keep PAN-only fallback and explicit --ip selection. Use the same policy to filter loaded/discovered endpoints and session-source cache updates. Reject an application control connection arriving on an unadvertised PAN interface and request normal reconnect using the selected LAN; network-policy changes can migrate an existing PAN control socket. Bluetooth/PAN itself is never disconnected or reconfigured by these operations. Existing USB cable negotiation remains supported.

Keep phoneAddress truthful to the current control peer. Once that session is on LAN, file/gallery discovery and scrcpy inherit the right endpoint. Scrcpy now avoids auto-selecting a different TCP device before connecting the paired endpoint, retaining explicit and USB choices. No fixed address or subnet heuristics, LAN scans, pairing resets or saved Wi-Fi credentials are involved.

## Validation

Pure checks cover transport ordering, PAN fallback/overrides, renamed Bluetooth metadata, live-address access and ADB TCP mismatch. Run locked release workspace build/tests. Hardware checks use only connection metadata and preserve the current PAN link while reloading an idle daemon. No private endpoints, identifiers or logs belong in this note.

Locked release workspace build completed without warnings; release tests passed (345 passed, 0 failed, 40 ignored). Reloaded the idle local daemon and confirmed its actual control peer routes over Wi-Fi, ADB is authorized at that same peer, and the existing PAN remains connected.
