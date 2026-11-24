package org.smartconnect;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.HashSet;
import java.util.Set;
import java.util.concurrent.TimeUnit;

/** Keeps only this session's scrcpy capture display powered on, never a physical screen. */
public final class DisplayPower {
    private static Object manager;
    private static Method getInfo;
    private static Method power;

    private static Object info(int id) throws Exception {
        return getInfo.invoke(manager, id);
    }
    private static int number(Object info, String field) throws Exception {
        return info.getClass().getField(field).getInt(info);
    }
    private static String text(Object info, String field) throws Exception {
        return String.valueOf(info.getClass().getField(field).get(info));
    }
    static boolean capture(Object info) throws Exception {
        return info != null && number(info, "type") == 5 && number(info, "ownerUid") == 2000
                && "scrcpy".equals(text(info, "name"))
                && text(info, "uniqueId").startsWith("virtual:com.android.shell,");
    }
    private static String dump() throws Exception {
        Process child = new ProcessBuilder("dumpsys", "display").start();
        Thread timeout = new Thread(() -> {
            try { if (!child.waitFor(2, TimeUnit.SECONDS)) child.destroy(); }
            catch (InterruptedException ignored) { }
        }, "capture-discovery-timeout");
        timeout.setDaemon(true);
        timeout.start();
        try {
            ByteArrayOutputStream bytes = new ByteArrayOutputStream();
            byte[] buffer = new byte[4096];
            try (InputStream input = child.getInputStream()) {
                int count;
                while ((count = input.read(buffer)) > 0) {
                    if (bytes.size() + count > 1024 * 1024) throw new IllegalStateException("display dump too large");
                    bytes.write(buffer, 0, count);
                }
            }
            if (child.waitFor() != 0) throw new IllegalStateException("display discovery failed");
            return new String(bytes.toByteArray(), StandardCharsets.UTF_8);
        } finally {
            timeout.interrupt();
            child.destroy();
        }
    }
    static String matchingCapture(String output, int source, Set<String> existing) {
        String found = null;
        for (String section : output.split("\n  DisplayDeviceInfo\\{")) {
            if (!section.startsWith("\"scrcpy\":")
                    || !section.matches("(?s).*\n    mDisplayIdToMirror=" + source + "\r?\n.*")) continue;
            for (String line : section.split("\n")) {
                String value = line.trim();
                if (value.startsWith("mUniqueId=virtual:com.android.shell,")) {
                    String candidate = value.substring("mUniqueId=".length());
                    if (existing.contains(candidate)) continue;
                    if (found != null) return null; // Ambiguous: never choose another viewer.
                    found = candidate;
                }
            }
        }
        return found;
    }
    public static void main(String[] args) {
        try {
            if (args.length != 1) throw new IllegalArgumentException("source display required");
            int source = Integer.parseInt(args[0]);
            if (source < 0) throw new IllegalArgumentException("virtual source required");
            Class<?> cls = Class.forName("android.hardware.display.DisplayManagerGlobal");
            manager = cls.getMethod("getInstance").invoke(null);
            getInfo = cls.getMethod("getDisplayInfo", int.class);
            try {
                power = cls.getMethod("requestDisplayPower", int.class, int.class);
            } catch (NoSuchMethodException unsupported) {
                System.out.println("capture power: per-display power override unavailable");
                return;
            }
            Object initial = info(source);
            if (initial == null || number(initial, "type") != 5 || number(initial, "ownerUid") == 2000) {
                throw new IllegalArgumentException("Smart Connect source display unavailable");
            }
            Set<String> existing = new HashSet<>();
            for (int id : (int[]) cls.getMethod("getDisplayIds").invoke(manager)) {
                Object current = info(id);
                if (capture(current)) existing.add(text(current, "uniqueId"));
            }
            System.out.println("capture power: ready");
            AtomicBoolean stop = new AtomicBoolean();
            Thread input = new Thread(() -> {
                try { while (System.in.read() != -1) { } } catch (Exception ignored) { }
                stop.set(true);
            }, "capture-owner");
            input.setDaemon(true);
            input.start();
            int target = -1;
            String unique = null;
            long missingSince = System.nanoTime();
            while (!stop.get() && info(source) != null) {
                Object current = target < 0 ? null : info(target);
                if (!capture(current) || !text(current, "uniqueId").equals(unique)) {
                    if (System.nanoTime() - missingSince > 12_000_000_000L) return;
                    unique = matchingCapture(dump(), source, existing);
                    target = -1;
                    if (unique != null) {
                        int[] ids = (int[]) cls.getMethod("getDisplayIds").invoke(manager);
                        for (int id : ids) {
                            Object candidate = info(id);
                            if (capture(candidate) && unique.equals(text(candidate, "uniqueId"))) {
                                target = id;
                                current = candidate;
                                break;
                            }
                        }
                    }
                }
                if (target >= 0 && capture(current)) {
                    missingSince = System.nanoTime();
                    if (number(current, "state") == 1 && Boolean.TRUE.equals(power.invoke(manager, target, 2))) {
                        System.out.println("capture power: restored virtual display=" + target + " source=" + source);
                    }
                }
                Thread.sleep(target < 0 ? 500 : 1000);
            }
        } catch (Exception error) {
            System.out.println("capture power: unavailable (" + error.getClass().getSimpleName() + ")");
        }
    }
}
