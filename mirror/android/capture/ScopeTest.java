package com.genymobile.scrcpy;

/** Pure JVM regressions: no Android SDK, device or platform mutation. */
public final class ScopeTest {
    private static void check(boolean ok) { if (!ok) throw new AssertionError("capture scope"); }
    public static void main(String[] args) {
        check(Server.streamSource(5, "AppStream:desktop:rotate"));
        check(Server.streamSource(5, "AppStream:desktop"));
        check(!Server.streamSource(1, "AppStream:desktop"));
        check(Server.streamSource(5, "AppStream:com.example.notes"));
        check(Server.streamSource(5, "AppStream:com.example.notes:rotate"));
        check(Server.streamSource(5, "[aBc123]AppStream:com.example.notes:rotate"));
        check(!Server.streamSource(5, "[session-name]AppStream:com.example.notes"));
        check(!Server.streamSource(5, "[aBc123]AppStream:mirror"));
        check(!Server.streamSource(1, "[aBc123]AppStream:com.example.notes"));
        check(Server.streamSource(5, "AppStream:org.example_app.Player:rotate"));
        check(!Server.streamSource(1, "AppStream:com.example.notes"));
        check(!Server.streamSource(5, "AppStream:mirror:rotate"));
        check(!Server.streamSource(5, "AppStream:mobile"));
        check(!Server.streamSource(5, "AppStream:com..example"));
        check(!Server.streamSource(5, "AppStream:com.example/activity"));
        check(!Server.streamSource(5, "AppStream:"));
        check(!Server.streamSource(5, null));
        check(!Server.streamSource(5, "AppStream:desktop-other"));
        check(!Server.streamSource(5, "another viewer"));
        check(Server.matches(7, "scrcpy", 7));
        check(!Server.matches(0, "scrcpy", 0));
        check(!Server.matches(7, "another viewer", 7));
        check(!Server.matches(7, "scrcpy", 8));
        check(!Server.matches(-1, "scrcpy", -1));
        check(DesktopWake.doubleTap(new float[]{4f, 10f, 20f}));
        check(!DesktopWake.doubleTap(new float[]{1f}));
        check(!DesktopWake.doubleTap(new float[]{2f}));
        check(!DesktopWake.doubleTap(new float[0]));
        check(!DesktopWake.doubleTap(null));
        check(DesktopWake.physical("dt-gesture") == 0);
        check(DesktopWake.physical("s-dt-gesture") == 1);
        check(DesktopWake.physical("another sensor") == -1);
        check(DesktopWake.asleep(1, 1)); check(DesktopWake.asleep(1, 3));
        check(!DesktopWake.asleep(5, 1)); check(!DesktopWake.asleep(1, 2)); check(!DesktopWake.asleep(1, 0));
        check(DesktopWake.phoneAsleep(1, 1, 1, 1));
        check(DesktopWake.phoneAsleep(1, 3, 5, 2));
        check(DesktopWake.phoneAsleep(1, 1, 0, 0));
        check(!DesktopWake.phoneAsleep(1, 1, 1, 2));
        check(!DesktopWake.phoneAsleep(1, 1, 1, 0));
        check(!DesktopWake.phoneAsleep(5, 1, 1, 1));
        check(DesktopWake.allowed(true, true, true, false));
        check(!DesktopWake.allowed(false, true, true, false));
        check(!DesktopWake.allowed(true, false, true, false));
        check(!DesktopWake.allowed(true, true, false, false));
        check(!DesktopWake.allowed(true, true, true, true));
        System.out.println("Capture isolation and physical gesture scope tests passed");
    }
}
