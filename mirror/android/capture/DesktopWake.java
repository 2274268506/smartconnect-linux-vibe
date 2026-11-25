package com.genymobile.scrcpy;

import java.io.PrintStream;
import java.lang.reflect.*;
import java.util.List;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.function.BooleanSupplier;

/** Scoped physical double-tap wake while an independent app/desktop keeps the device interactive. */
final class DesktopWake {
    private final Object displays, sensors, listener, resolver, power;
    private final Class<?> sensorManager, listenerApi, secure;
    private final Method getInfo, setting, wakeDisplay, uptime;
    private final String operationPackage;
    private final int wakeReason;
    private final int source;
    private final BooleanSupplier capturing;
    private final PrintStream log;
    private final AtomicBoolean open = new AtomicBoolean(true), pending = new AtomicBoolean();
    private volatile boolean proximityKnown, near;
    private long lastGesture;
    private final AtomicBoolean errorReported = new AtomicBoolean();

    static int physical(String name) {
        return "dt-gesture".equals(name) ? 0 : "s-dt-gesture".equals(name) ? 1 : -1;
    }
    static boolean doubleTap(float[] values) { return values != null && values.length > 0 && values[0] == 4f; }
    static boolean asleep(int type, int state) { return type == 1 && (state == 1 || state == 3 || state == 4); }
    static boolean phoneAsleep(int targetType, int targetState, int otherType, int otherState) {
        return asleep(targetType, targetState) && (otherType != 1 || asleep(otherType, otherState));
    }
    static boolean allowed(boolean enabled, boolean capturing, boolean proximityKnown, boolean near) {
        return enabled && capturing && proximityKnown && !near;
    }
    static DesktopWake start(ClassLoader upstream, Object displays, int source, BooleanSupplier capturing, PrintStream log) {
        DesktopWake bridge = null;
        try {
            bridge = new DesktopWake(upstream, displays, source, capturing, log);
            if (!bridge.register()) { bridge.close(); return null; }
            log.println("Smart Connect stream wake: physical double-tap bridge ready");
            return bridge;
        } catch (Exception error) {
            if (bridge != null) bridge.close();
            log.println("Smart Connect stream wake: unavailable (" + error.getClass().getSimpleName() + ")");
            return null;
        }
    }
    private DesktopWake(ClassLoader upstream, Object displays, int source, BooleanSupplier capturing, PrintStream log) throws Exception {
        this.source = source; this.capturing = capturing; this.log = log;
        // Read current state directly: the regular display-info cache may retain pre-sleep state.
        Field displayService = Class.forName("android.hardware.display.DisplayManagerGlobal").getDeclaredField("mDm");
        displayService.setAccessible(true); this.displays = displayService.get(displays);
        getInfo = Class.forName("android.hardware.display.IDisplayManager").getMethod("getDisplayInfo", int.class);
        Object context = upstream.loadClass("com.genymobile.scrcpy.FakeContext").getMethod("get").invoke(null);
        Class<?> contextApi = Class.forName("android.content.Context");
        resolver = contextApi.getMethod("getContentResolver").invoke(context);
        operationPackage = (String)contextApi.getMethod("getOpPackageName").invoke(context);
        Object binder = Class.forName("android.os.ServiceManager").getMethod("getService", String.class).invoke(null, "power");
        power = Class.forName("android.os.IPowerManager$Stub").getMethod("asInterface", Class.forName("android.os.IBinder")).invoke(null, binder);
        wakeDisplay = Class.forName("android.os.IPowerManager").getMethod("wakeUpWithDisplayId", long.class, int.class, String.class, String.class, int.class);
        uptime = Class.forName("android.os.SystemClock").getMethod("uptimeMillis");
        wakeReason = Class.forName("android.os.PowerManager").getField("WAKE_REASON_GESTURE").getInt(null);
        secure = Class.forName("android.provider.Settings$Secure");
        setting = secure.getMethod("getInt", Class.forName("android.content.ContentResolver"), String.class, int.class);
        Class<?> looper = Class.forName("android.os.Looper");
        Object mainLooper = looper.getMethod("getMainLooper").invoke(null);
        if (mainLooper == null) throw new IllegalStateException("Stream event loop unavailable");
        sensors = Class.forName("android.hardware.SystemSensorManager").getConstructor(contextApi, looper).newInstance(context, mainLooper);
        sensorManager = Class.forName("android.hardware.SensorManager");
        listenerApi = Class.forName("android.hardware.SensorEventListener");
        listener = Proxy.newProxyInstance(listenerApi.getClassLoader(), new Class<?>[]{listenerApi}, (object, method, args) -> {
            switch (method.getName()) {
                case "hashCode": return System.identityHashCode(object);
                case "equals": return args != null && args.length == 1 && object == args[0];
                case "toString": return "Smart Connect desktop wake listener";
                case "onSensorChanged": if (args != null && args.length == 1) event(args[0]); break;
                default: break;
            }
            return null;
        });
    }
    private boolean enabled() throws Exception { return ((Integer)setting.invoke(null, resolver, "double_tap_to_wake", 0)) == 1; }
    @SuppressWarnings("unchecked")
    private boolean register() throws Exception {
        if (!enabled()) return false;
        Class<?> sensorApi = Class.forName("android.hardware.Sensor");
        Method list = sensorManager.getMethod("getSensorList", int.class);
        Method register = sensorManager.getMethod("registerListener", listenerApi, sensorApi, int.class);
        List<Object> proximity = (List<Object>)list.invoke(sensors, 8);
        if (proximity.isEmpty()) return false; // Preserve pocket/covered-screen protection.
        if (!Boolean.TRUE.equals(register.invoke(sensors, listener, proximity.get(0), 3))) return false;
        int count = 0;
        for (Object sensor : (List<Object>)list.invoke(sensors, 65566)) {
            String name = (String)sensorApi.getMethod("getName").invoke(sensor);
            if (physical(name) >= 0 && Boolean.TRUE.equals(register.invoke(sensors, listener, sensor, 3))) count++;
        }
        return count > 0;
    }
    private static int integer(Object object, String name) throws Exception { return object.getClass().getField(name).getInt(object); }
    private boolean sourceAlive() throws Exception {
        Object info = getInfo.invoke(displays, source);
        return info != null && Server.streamSource(integer(info, "type"), String.valueOf(info.getClass().getField("name").get(info))) && capturing.getAsBoolean();
    }
    private boolean phoneAsleep(int display) throws Exception {
        Object target = getInfo.invoke(displays, display);
        if (target == null) return false;
        Object other = getInfo.invoke(displays, display == 0 ? 1 : 0);
        return phoneAsleep(integer(target, "type"), integer(target, "state"),
                other == null ? 0 : integer(other, "type"), other == null ? 0 : integer(other, "state"));
    }
    private void event(Object event) {
        if (!open.get() || event == null) return;
        try {
            Object sensor = event.getClass().getField("sensor").get(event);
            float[] values = (float[])event.getClass().getField("values").get(event);
            Class<?> api = Class.forName("android.hardware.Sensor");
            int type = (Integer)api.getMethod("getType").invoke(sensor);
            if (type == 8 && values.length > 0) {
                float max = (Float)api.getMethod("getMaximumRange").invoke(sensor);
                near = !Float.isFinite(values[0]) || values[0] < max; proximityKnown = true; return;
            }
            if (type != 65566 || !doubleTap(values)) return;
            int physical = physical((String)api.getMethod("getName").invoke(sensor));
            if (physical < 0) return;
            long now = System.nanoTime();
            if (now - lastGesture < 1_000_000_000L) return;
            lastGesture = now;
            if (!enabled()) { log.println("Smart Connect stream wake: double-tap ignored, user setting disabled"); return; }
            if (!sourceAlive()) { log.println("Smart Connect stream wake: double-tap ignored, stream inactive"); return; }
            if (!proximityKnown || near) { log.println("Smart Connect stream wake: double-tap ignored, proximity not clear"); return; }
            if (!phoneAsleep(physical)) {
                log.println("Smart Connect stream wake: double-tap ignored, physical display not asleep"); return;
            }
            if (!pending.compareAndSet(false, true)) return;
            Thread thread = new Thread(() -> wake(physical), "smartconnect-physical-wake");
            thread.setDaemon(true); thread.start();
        } catch (Exception error) { report(error); }
    }
    private void report(Exception error) {
        if (!errorReported.compareAndSet(false, true)) return;
        Throwable cause = error;
        while (cause instanceof InvocationTargetException && cause.getCause() != null) cause = cause.getCause();
        log.println("Smart Connect stream wake: gesture check unavailable (" + cause.getClass().getSimpleName() + ")");
    }
    private void wake(int display) {
        try {
            if (!open.get() || !allowed(enabled(), sourceAlive(), proximityKnown, near)) return;
            if (!phoneAsleep(display)) return;
            // A wake key may be ignored when aggregate interactivity is true. Target the
            // sleeping physical display's power group directly; keyguard remains untouched.
            wakeDisplay.invoke(power, uptime.invoke(null), wakeReason, "smartconnect:physical-double-tap", operationPackage, display);
            log.println("Smart Connect stream wake: physical double-tap wake requested");
        } catch (Exception error) { report(error); }
        finally { pending.set(false); }
    }
    void close() {
        if (!open.getAndSet(false)) return;
        try { sensorManager.getMethod("unregisterListener", listenerApi).invoke(sensors, listener); } catch (Exception ignored) { }
    }
}
