package org.smartconnect;

import java.util.Collections;

/** Pure JVM scope/discovery regressions; no Android SDK, ADB, or device required. */
public final class DisplayPowerTest {
    public static final class Info {
        public int type = 5;
        public int ownerUid = 2000;
        public String name = "scrcpy";
        public String uniqueId = "virtual:com.android.shell,2000,scrcpy,example";
    }
    private static void check(boolean result) {
        if (!result) throw new AssertionError("unsafe capture selection");
    }
    private static String device(String name, String unique, int source) {
        return "\n  DisplayDeviceInfo{\"" + name + "\": ...\n    mUniqueId=" + unique
                + "\n    mDisplayIdToMirror=" + source + "\n    mWindowManagerMirroring=false\n";
    }
    public static void main(String[] args) throws Exception {
        Info info = new Info();
        check(DisplayPower.capture(info));
        info.type = 1; check(!DisplayPower.capture(info));
        info.type = 5; info.ownerUid = 10001; check(!DisplayPower.capture(info));
        info.ownerUid = 2000; info.name = "another viewer"; check(!DisplayPower.capture(info));
        info.name = "scrcpy"; info.uniqueId = "local:example"; check(!DisplayPower.capture(info));
        String ours = "virtual:com.android.shell,2000,scrcpy,example";
        String second = "virtual:com.android.shell,2000,scrcpy,second";
        String dump = device("physical", "local:example", 7) + device("scrcpy", ours, 7);
        check(ours.equals(DisplayPower.matchingCapture(dump, 7, Collections.emptySet())));
        check(DisplayPower.matchingCapture(dump, 70, Collections.emptySet()) == null);
        check(DisplayPower.matchingCapture(dump, 7, Collections.singleton(ours)) == null);
        check(DisplayPower.matchingCapture(dump + device("scrcpy", second, 7), 7, Collections.emptySet()) == null);
        check(DisplayPower.matchingCapture(device("scrcpy", "virtual:other", 7), 7, Collections.emptySet()) == null);
        System.out.println("DisplayPower scope and discovery tests passed");
    }
}
