# Gallery performance

## Evidence and references

The initial browser launched `app_process` separately for each of 24 thumbnails. It also decoded the same photos again when revisiting a page. This made process/ADB round trips dominate small thumbnail requests.

References inspected: [Fossify Gallery’s Glide integration](https://github.com/FossifyOrg/Gallery/blob/main/app/src/main/kotlin/org/fossify/gallery/extensions/Glide.kt), [Glide’s caching design](https://bumptech.github.io/glide/doc/caching.html), and [scrcpy’s client/server development notes](https://github.com/Genymobile/scrcpy/blob/master/doc/develop.md). Apply memory reuse and amortized server startup with original repository code; no external gallery code is copied or Glide dependency added.

## Implementation

A page now sends up to 24 paths to one Android process. The original helper emits bounded SCG1 frames: big-endian count then a big-endian length and JPEG payload for each path; a zero length denotes an unavailable image. Structural validation rejects count mismatches, truncation, oversized frames and trailing bytes. Errors in one image do not discard other images. Cancellation retains existing owned-process cleanup.

A device/path-keyed RAM LRU retains at most 96 thumbnails and 8 MiB for 30 seconds. Only missing thumbnails require phone decoding; cache hits avoid helper upload/startup. Explicit refresh and paired-peer changes clear it, and epoch checks reject old in-flight writers. Preview/original requests remain fresh. No persistent photo cache or original downloads are introduced.

## Validation

Pure regressions cover framing, mixed failures, TTL, identity separation, LRU/byte limits and invalidation. Use the existing owned synthetic EXIF fixture for batch correctness and cleanup. The opt-in SMARTCONNECT_GALLERY_BENCHMARK flag compares 24 legacy calls with one 24-request batch on the same generated image, isolating process overhead; it is not a promise about every real album or network.

Verified on the authorized phone with the owned generated EXIF fixture: 24 legacy single-thumbnail calls took 30,670 ms; one 24-request batch took 1,485 ms. This excludes initial device discovery, helper preparation and directory listing, and repeats the same generated image to isolate startup/transport overhead. Mixed-success batch handling, orientation and exact local/phone helper cleanup passed. Locked workspace release build passed without warnings; 337 tests passed, 0 failed, 40 optional tests skipped. Three isolated GTK regressions passed separately. No personal photos were read or modified.
