# App density in desktop mode

The reported symptom was launcher/display text preferences changing desktop chrome while app
content stayed small. The host previously always sent displayDensity=160, displayScale=1.0 and
physical dimensions corresponding to about 142 DPI, even at a high native video resolution.
Phone desktop launcher preferences are separate from virtual-display density; the latter must
be set at session creation for Android app layouts/text that use dp/sp to follow it.

The host's desktop request accepts scale=100..300 percent, validated before any phone command.
Desktop peer start data uses 160*scale/100 DPI, matching displayScale, and physical dimensions
calculated from pixel size and DPI. Resolution is unchanged. Other stream types retain their
original handshake. The phone-reported density and requested scale are exposed separately.
A running desktop rejects a changed scale until closed, preventing a false success message.

GTK adds an App and text scaling selector: follow the monitor (default) or 100–300% presets.
The saved scalePercent is null for automatic mode; it resolves at start to the active GDK
monitor's scale, clamped to supported values. Selection changes save only desktop preferences
and offer Apply and restart desktop. A capability check prevents an old host silently ignoring
scaling. PC-to-phone Display settings and phone global read/font state are untouched.

Tests cover scale validation, proportional physical dimensions, outgoing desktop handshake
versus unchanged standalone apps, legacy preference defaults, and an isolated GTK control test.
Live density validation is recorded below. No claim is made that fixed-pixel Android apps
honor display density or that phone launcher font preferences control standalone app windows.

Validation: 38 GUI unit tests and 42 mirror unit/media tests passed, plus the isolated GTK
app-scaling test. The rebuilt host and GUI are running. On the actual phone, a new desktop
session requested at 200% reported density=320, with video dimensions still 2880x1920 and
borderless=true. Media sockets retained their enlarged receive buffers. This verifies that
the phone accepts the density and keeps resolution independent of scaling; custom app rendering
or phone launcher font preferences can still behave differently. The existing standalone
app handshake remains unchanged and is covered by the signaling test.
