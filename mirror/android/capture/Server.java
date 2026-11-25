package com.genymobile.scrcpy;

import java.io.*;
import java.lang.reflect.*;
import java.util.*;

/** Original session-scoped launcher. The installed scrcpy server is loaded unchanged. */
public final class Server {
    private static volatile ClassLoader upstreamLoader;
    private static DesktopWake desktopWake;
    private static final PrintStream LOG = System.out; // scrcpy later silences System.out.
    private static final Map<Object, Capture> CAPTURES = Collections.synchronizedMap(new IdentityHashMap<>());

    static boolean streamSource(int type, String name) {
        if (type != 5 || name == null) return false;
        // App streams may prefix the name with an opaque hexadecimal session tag.
        if (name.startsWith("[")) {
            int end = name.indexOf(']');
            if (end < 2 || !name.substring(1, end).matches("[0-9a-fA-F]{1,16}")) return false;
            name = name.substring(end + 1);
        }
        if (!name.startsWith("AppStream:")) return false;
        String pkg = name.substring("AppStream:".length()).split(":", 2)[0];
        return "desktop".equals(pkg) || "android".equals(pkg)
                || pkg.matches("[A-Za-z_][A-Za-z0-9_]*(\\.[A-Za-z_][A-Za-z0-9_]*)+");
    }
    static boolean matches(int source, String name, int mirrored) {
        return source > 0 && "scrcpy".equals(name) && mirrored == source;
    }
    private static Object invoke(Method method, Object receiver, Object[] args) throws Throwable {
        try { return method.invoke(receiver, args); }
        catch (InvocationTargetException e) { throw e.getCause(); }
    }
    private static int integer(Object info, String field) throws Exception {
        return info.getClass().getField(field).getInt(info);
    }
    private static int flag(String name) throws Exception {
        return Class.forName("android.hardware.display.DisplayManager").getField(name).getInt(null);
    }
    private static final class Capture {
        Object mirror, root;
        private static final Class<?> SC;
        private static final Class<?> TX;
        static {
            try {
                SC = Class.forName("android.view.SurfaceControl");
                TX = Class.forName("android.view.SurfaceControl$Transaction");
            } catch (Exception e) { throw new IllegalStateException(e); }
        }
        void attach(Object manager, int source, int target, Object config) throws Exception {
            Class<?> dm = Class.forName("android.hardware.display.DisplayManagerGlobal");
            Object from = dm.getMethod("getDisplayInfo", int.class).invoke(manager, source);
            Object to = dm.getMethod("getDisplayInfo", int.class).invoke(manager, target);
            if (from == null || to == null || integer(from, "logicalWidth") <= 0 || integer(from, "logicalHeight") <= 0)
                throw new IllegalStateException("Display disappeared");
            mirror = SC.getConstructor().newInstance();
            Object wm = Class.forName("android.view.WindowManagerGlobal").getMethod("getWindowManagerService").invoke(null);
            if (!Boolean.TRUE.equals(Class.forName("android.view.IWindowManager")
                    .getMethod("mirrorDisplay", int.class, SC).invoke(wm, source, mirror)))
                throw new IllegalStateException("Display mirror unavailable");
            Class<?> builder = Class.forName("android.view.SurfaceControl$Builder");
            Object b = builder.getConstructor().newInstance();
            builder.getMethod("setName", String.class).invoke(b, "Smart Connect capture root");
            builder.getMethod("setContainerLayer").invoke(b);
            root = builder.getMethod("build").invoke(b);
            Object txn = TX.getConstructor().newInstance();
            try {
                TX.getMethod("reparent", SC, SC).invoke(txn, mirror, root);
                TX.getMethod("setLayerStack", SC, int.class).invoke(txn, root, integer(to, "layerStack"));
                float sx = (float)(Integer)config.getClass().getMethod("getWidth").invoke(config) / integer(from, "logicalWidth");
                float sy = (float)(Integer)config.getClass().getMethod("getHeight").invoke(config) / integer(from, "logicalHeight");
                TX.getMethod("setScale", SC, float.class, float.class).invoke(txn, mirror, sx, sy);
                TX.getMethod("setLayer", SC, int.class).invoke(txn, root, Integer.MAX_VALUE);
                TX.getMethod("show", SC).invoke(txn, root);
                TX.getMethod("show", SC).invoke(txn, mirror);
                TX.getMethod("apply").invoke(txn);
            } finally { TX.getMethod("close").invoke(txn); }
        }
        void close() {
            try {
                Object txn = TX.getConstructor().newInstance();
                try {
                    if (root != null) TX.getMethod("remove", SC).invoke(txn, root);
                    if (mirror != null) TX.getMethod("remove", SC).invoke(txn, mirror);
                    TX.getMethod("apply").invoke(txn);
                } finally { TX.getMethod("close").invoke(txn); }
            } catch (Exception ignored) { }
            for (Object surface : new Object[]{root, mirror}) {
                if (surface != null) try { SC.getMethod("release").invoke(surface); } catch (Exception ignored) { }
            }
            root = null; mirror = null;
        }
    }
    @SuppressWarnings("unchecked")
    private static void isolate(String[] args) throws Exception {
        int selected = -1;
        for (String arg : args) if (arg.startsWith("display_id=")) selected = Integer.parseInt(arg.substring(11));
        final int source = selected;
        if (source <= 0) return;
        Class<?> dm = Class.forName("android.hardware.display.DisplayManagerGlobal");
        final Object manager = dm.getMethod("getInstance").invoke(null);
        Object info = dm.getMethod("getDisplayInfo", int.class).invoke(manager, source);
        if (info == null || !streamSource(integer(info, "type"), String.valueOf(info.getClass().getField("name").get(info)))) return;
        final int autoMirror = flag("VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR");
        final int independent = flag("VIRTUAL_DISPLAY_FLAG_OWN_CONTENT_ONLY") | flag("VIRTUAL_DISPLAY_FLAG_OWN_DISPLAY_GROUP")
                | flag("VIRTUAL_DISPLAY_FLAG_TRUSTED");
        Field service = dm.getDeclaredField("mDm"); service.setAccessible(true);
        final Object original = service.get(manager);
        final Class<?> api = Class.forName("android.hardware.display.IDisplayManager");
        Object proxy = Proxy.newProxyInstance(api.getClassLoader(), new Class<?>[]{api}, (object, method, values) -> {
            if (method.getName().equals("releaseVirtualDisplay") && values != null && values.length > 0) {
                Capture capture = CAPTURES.remove(values[0]);
                if (capture != null) capture.close();
            }
            if (method.getName().equals("createVirtualDisplay") && values != null && values.length > 1) {
                Object config = values[0]; Class<?> cls = config.getClass();
                String name = String.valueOf(cls.getMethod("getName").invoke(config));
                int mirrored = (Integer)cls.getMethod("getDisplayIdToMirror").invoke(config);
                if (matches(source, name, mirrored)) {
                    Field flags = cls.getDeclaredField("mFlags"); flags.setAccessible(true);
                    int previous = flags.getInt(config);
                    flags.setInt(config, (previous & ~autoMirror) | independent);
                    Capture capture = new Capture();
                    int target = -1;
                    try {
                        target = (Integer)invoke(method, original, values);
                        if (target < 0) throw new IllegalStateException("Capture display unavailable");
                        capture.attach(manager, source, target, config);
                        Capture old = CAPTURES.put(values[1], capture);
                        if (old != null) old.close();
                        synchronized (Server.class) {
                            if (desktopWake == null && upstreamLoader != null)
                                desktopWake = DesktopWake.start(upstreamLoader, manager, source, () -> !CAPTURES.isEmpty(), LOG);
                        }
                        LOG.println("Smart Connect capture: independent stream power ready");
                        return target;
                    } catch (Throwable error) {
                        capture.close();
                        if (target >= 0) {
                            try { api.getMethod("releaseVirtualDisplay", Class.forName("android.hardware.display.IVirtualDisplayCallback"))
                                    .invoke(original, values[1]); } catch (Exception ignored) { }
                        }
                        flags.setInt(config, previous);
                        LOG.println("Smart Connect capture: using standard capture (" + error.getClass().getSimpleName() + ")");
                    }
                }
            }
            return invoke(method, original, values);
        });
        // Both paths are used by Android: DMGlobal clients and the static createVirtualDisplay API.
        service.set(manager, proxy);
        Class<?> services = Class.forName("android.os.ServiceManager");
        final Object binder = services.getMethod("getService", String.class).invoke(null, "display");
        Class<?> binderApi = Class.forName("android.os.IBinder");
        Object bridge = Proxy.newProxyInstance(binderApi.getClassLoader(), new Class<?>[]{binderApi}, (object, method, values) -> {
            if (method.getName().equals("queryLocalInterface") && values != null && values.length == 1
                    && "android.hardware.display.IDisplayManager".equals(values[0])) return proxy;
            return invoke(method, binder, values);
        });
        Field cache = services.getDeclaredField("sCache"); cache.setAccessible(true);
        ((Map<String, Object>)cache.get(null)).put("display", bridge);
    }
    static void runUpstream(String name, String[] args, boolean capture) throws Throwable {
        if (capture) {
            try { isolate(args); }
            catch (Exception error) { LOG.println("Smart Connect capture: standard power path (" + error.getClass().getSimpleName() + ")"); }
        }
        File folder = new File("/data/local/tmp/smartconnect-server-" + System.nanoTime());
        if (!folder.mkdir()) throw new IOException("Cannot create private server directory");
        if (!folder.setReadable(false, false) || !folder.setWritable(false, false) || !folder.setExecutable(false, false)
                || !folder.setReadable(true, true) || !folder.setWritable(true, true) || !folder.setExecutable(true, true)) {
            folder.delete(); throw new IOException("Cannot protect server directory");
        }
        File upstream = new File(folder, "upstream.jar");
        // scrcpy calls System.exit(); a finally block alone does not clean extracted assets.
        Thread cleanup = new Thread(() -> {
            if (desktopWake != null) desktopWake.close();
            Capture[] remaining;
            synchronized (CAPTURES) { remaining = CAPTURES.values().toArray(new Capture[0]); CAPTURES.clear(); }
            for (Capture owned : remaining) owned.close();
            upstream.delete(); folder.delete();
        }, "smartconnect-capture-cleanup");
        Runtime.getRuntime().addShutdownHook(cleanup);
        try {
            try (InputStream input = Server.class.getClassLoader().getResourceAsStream("upstream.jar");
                    OutputStream output = new FileOutputStream(upstream)) {
                if (input == null) throw new IOException("Installed scrcpy server is missing");
                byte[] buffer = new byte[8192]; int n;
                while ((n = input.read(buffer)) != -1) output.write(buffer, 0, n);
            }
            if (!upstream.setReadOnly()) throw new IOException("Cannot protect server artifact");
            ClassLoader loader = (ClassLoader)Class.forName("dalvik.system.DexClassLoader")
                    .getConstructor(String.class, String.class, String.class, ClassLoader.class)
                    .newInstance(upstream.getPath(), null, null, Server.class.getClassLoader().getParent());
            upstreamLoader = loader;
            Thread.currentThread().setContextClassLoader(loader);
            Class<?> original = loader.loadClass("com.genymobile.scrcpy." + name);
            invoke(original.getMethod("main", String[].class), null, new Object[]{args});
        } finally { cleanup.run(); }
    }
    public static void main(String[] args) throws Throwable { runUpstream("Server", args, true); }
}
