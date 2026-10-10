package dev.leftcar.viewer.km;

import android.accessibilityservice.AccessibilityServiceInfo;
import dev.leftcar.viewer.km.IKmMouseCallback;

/** Local-only Shizuku capability. No command strings, network, or arbitrary file paths. */
interface IKmInputAuthority {
    boolean prepare(IBinder owner) = 0;
    void heartbeat() = 1;
    void inject(int kind, int code, boolean down, double x, double y) = 2;
    void releaseAll() = 3;
    void close() = 4;
    boolean configureOwnObservation(IBinder connection, int mode) = 5;
    boolean probeMouse(String name) = 7;
    boolean captureMouse(String name, IKmMouseCallback callback) = 8;
    void releaseMouse() = 9;
    int observedSources(in AccessibilityServiceInfo info) = 10;
    boolean mouseCaptureSupported() = 11;
    void destroy() = 16777114;
}
