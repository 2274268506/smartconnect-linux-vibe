# Built-in audio reception

The video codec filter previously removed rtpmap/fmtp attributes from all media sections. Restrict it to video and retain audio codec negotiation. Incoming audio pads now have an Opus decode/playback branch; scrcpy owner and headless/unsupported tracks drain instead. Audio packets do not feed video recovery counters.

The mirror suite passed 88 tests including mixed H.264/Opus SDP preservation. This fixes receiver capability only: it does not supply the missing native audio control handshake. Current phone offers may contain video alone, so end-to-end built-in sound is not claimed. Private phone analysis remains outside the repository.
