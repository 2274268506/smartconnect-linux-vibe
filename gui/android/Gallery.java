package smartconnect.gallery;

import java.io.File;

/** Original, read-only Android thumbnail helper. No Android SDK is required to build it. */
public final class Gallery {
    public static int sample(int width, int height, int edge) {
        if (width <= 0 || height <= 0 || edge < 64 || edge > 2048
                || (long) width * height > 160000000L) throw new IllegalArgumentException();
        int sample = 1;
        while (width / sample > edge || height / sample > edge) sample *= 2;
        return sample;
    }
    private static void image(String path, int edge, java.io.OutputStream output) throws Exception {
        File root = new File("/sdcard").getCanonicalFile();
        File image = new File(path).getCanonicalFile();
        if (!image.getPath().startsWith(root.getPath() + File.separator)
                || !image.isFile() || image.length() > 64L * 1024 * 1024) throw new IllegalArgumentException();
        Class<?> decoderApi = Class.forName("android.graphics.ImageDecoder");
        Class<?> sourceApi = Class.forName("android.graphics.ImageDecoder$Source");
        Class<?> listenerApi = Class.forName("android.graphics.ImageDecoder$OnHeaderDecodedListener");
        Class<?> infoApi = Class.forName("android.graphics.ImageDecoder$ImageInfo");
        Class<?> sizeApi = Class.forName("android.util.Size");
        Object source = decoderApi.getMethod("createSource", File.class).invoke(null, image);
        Object listener = java.lang.reflect.Proxy.newProxyInstance(listenerApi.getClassLoader(),
                new Class<?>[]{listenerApi}, (object, method, values) -> {
                    if (!method.getName().equals("onHeaderDecoded")) return null;
                    Object size = infoApi.getMethod("getSize").invoke(values[1]);
                    int width = (Integer) sizeApi.getMethod("getWidth").invoke(size);
                    int height = (Integer) sizeApi.getMethod("getHeight").invoke(size);
                    int downsample = sample(width, height, edge);
                    decoderApi.getMethod("setAllocator", int.class).invoke(values[0], 1);
                    decoderApi.getMethod("setTargetSize", int.class, int.class)
                            .invoke(values[0], Math.max(1, width / downsample), Math.max(1, height / downsample));
                    return null;
                });
        Object bitmap = decoderApi.getMethod("decodeBitmap", sourceApi, listenerApi).invoke(null, source, listener);
        if (bitmap == null) throw new IllegalArgumentException();
        Class<?> bitmapApi = Class.forName("android.graphics.Bitmap");
        Class<?> format = Class.forName("android.graphics.Bitmap$CompressFormat");
        boolean ok = (Boolean) bitmapApi.getMethod("compress", format, int.class, java.io.OutputStream.class)
                .invoke(bitmap, format.getField("JPEG").get(null), edge <= 256 ? 78 : 92, output);
        bitmapApi.getMethod("recycle").invoke(bitmap);
        output.flush();
        if (!ok) throw new IllegalStateException();
    }
    public static void main(String[] args) {
        try {
            if (args.length >= 2 && args[0].equals("--batch")) {
                if (args.length > 25) throw new IllegalArgumentException();
                java.io.DataOutputStream frames = new java.io.DataOutputStream(System.out);
                frames.writeInt(0x53434731); // SCG1
                frames.writeInt(args.length - 1);
                for (int i = 1; i < args.length; i++) {
                    byte[] data = null;
                    try {
                        java.io.ByteArrayOutputStream bytes = new java.io.ByteArrayOutputStream();
                        image(args[i], 256, bytes);
                        if (bytes.size() <= 1024 * 1024) data = bytes.toByteArray();
                    } catch (Exception unavailable) { }
                    frames.writeInt(data == null ? 0 : data.length);
                    if (data != null) frames.write(data);
                    frames.flush();
                }
            } else {
                if (args.length != 2) throw new IllegalArgumentException();
                image(args[0], Integer.parseInt(args[1]), System.out);
            }
        } catch (Throwable error) {
            System.err.println("Gallery image unavailable");
            System.exit(1);
        }
    }
}
