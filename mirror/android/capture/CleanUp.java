package com.genymobile.scrcpy;

/** Keep the installed server's original cleanup entrypoint and state restoration. */
public final class CleanUp {
    public static void main(String[] args) throws Throwable { Server.runUpstream("CleanUp", args, false); }
}
