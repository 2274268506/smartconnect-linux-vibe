# Cancel next to app Opening status

The previous transient Opening toast had no cancellation action. It now includes Cancel beside its title and remains visible through pending startup. The action calls the existing cancel_app/StartControl path. A PendingApp owns its toast and dismisses it on removal, covering success, failure, disconnect and cancellation; callbacks retain only a weak page reference. Existing running-list cancellation remains available.

The existing isolated GTK preparation test exercises the toast button before a session exists. No phone message is required.

Native bidirectional audio is separate and not implemented by this change. The phone application contains R4Audio helper/WebRTC classes, but this repository has no complete verified command mapping or native audio session handler. The TODO service-21 attribution conflicts with the public protocol index; no speculative service is advertised. Research APKs and extracted symbols remain outside the repository.

Validation: workspace cargo check, 54 GUI release tests and the isolated GTK toast-action cancellation test passed. The updated GUI was restarted without touching the stream host. Native audio remains research only: bean shapes were inspected outside the repository, but numeric dispatch and full duplex negotiation have not been verified.
