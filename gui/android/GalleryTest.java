package smartconnect.gallery;
/** JVM-only regression checks; no Android device or media is accessed. */
public final class GalleryTest {
    public static void main(String[] args) {
        if (Gallery.sample(4000,3000,256) != 16) throw new AssertionError();
        if (Gallery.sample(3000,4000,2048) != 2) throw new AssertionError();
        if (Gallery.sample(120,60,256) != 1) throw new AssertionError();
        for (int[] invalid : new int[][]{{0,1,256},{1,-1,256},{40000,40000,256},{10,10,0},{10,10,4096}}) {
            boolean rejected=false;
            try {Gallery.sample(invalid[0],invalid[1],invalid[2]);} catch (IllegalArgumentException expected) {rejected=true;}
            if (!rejected) throw new AssertionError();
        }
        System.out.println("Gallery image bounds tests passed");
    }
}
