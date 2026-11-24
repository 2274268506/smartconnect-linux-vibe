# Main-window close responsiveness

Closing the main window hides it and immediately refreshes the tray. The old Tray::set called
ksni's blocking Handle::update on the GTK thread. That method waits both for the service-state
lock and for the D-Bus service to acknowledge its property/menu update. A delayed tray therefore
blocked close, reopen and other GTK callbacks.

Tray updates now use one background worker and a latest-value slot, with a bounded wake channel.
Posting a change never waits for D-Bus. The worker takes the value and releases the slot mutex
before calling the blocking library. Repeated identical state/visibility pairs are skipped;
changes during a stalled update overwrite the pending slot so the final visibility wins.
Dropping the tray only enqueues service shutdown and never joins the worker on the GTK thread.

A deterministic test blocks the worker callback, posts multiple changes and verifies they return
before releasing the worker, then checks that the final visibility is applied. A desktop-session
GTK regression test repeatedly presents, closes and reopens the real main window while checking
that main-loop timers still fire. Closing continues to hide to the tray; it does not stop the
phone desktop or change the existing Quit behavior.

Validation on the current KDE Wayland session: eight close/reopen cycles completed and GTK
timers kept firing, both with the default renderer and with GSK_RENDERER=cairo. Both paths
still emitted GTK frame-timing warnings; those warnings alone did not reproduce a permanent
freeze, so this change does not force a renderer override or claim to fix GTK's warnings.

A separate phone-video close issue was reproduced using a single appsrc frame followed by
no further media: WM_DELETE_WINDOW invoked the GL close path, but no GStreamer ERROR/EOS
message arrived until another frame. This explains why the host could keep a closed/closing
view active on a damage-driven phone stream.

`mirror/src/window_close.rs` installs an explicit GL close callback after the sink's first
completed frame (after sink setup installs its own handlers). The callback only sets an owned
atomic flag. The host detects it on its next tick, notifies the phone and tears the pipeline
down on the existing cleanup worker. Callback data is an owned Arc with a GL destroy notifier,
with no pipeline/session reference cycle. No frame is needed to acknowledge the close.

An isolated GL regression pushes exactly one frame, sends WM_DELETE_WINDOW and verifies the
close flag without sending any more buffers. Raw experiments and logs remain under /tmp.
