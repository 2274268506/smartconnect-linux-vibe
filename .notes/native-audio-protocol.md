# Native audio control recovery

**OBSERVED (static analysis, current phone app):** service 21 carries R4 audio envelopes with version "3". Dispatch maps command 1 to peer trigger, 2 to parameters, 3 to start acknowledgement, 4 to state change, 5 to stop, 6 to resume, 7 to pause and 8 to WebRTC signalling/candidate data. Peer start carries requestId, instanceId, port, frequency, channels, encoding and protocol. WebRTC messages carry sessionId, type and string data. Parameters include codec/transport fields and heartbeat. These are original descriptions of observed behavior, not copied implementation.

This resolves the older index's tentative drag/drop attribution; service 24 remains drag/drop. It does not establish a functioning Linux audio session. The phone manager checks its own per-device routing/session state before starting, and custom WebRTC/transport fields still need a verified live exchange. No guessed trigger is sent or audio service advertised by this research.

The normal service-15 video offer is distinct. Preserving audio SDP and receiving offered Opus does not make a video-only offer produce audio. Full native audio remains unfinished, while scrcpy audio stays available. Research APKs/decompiled material are outside the repository.
