package dev.leftcar.viewer.km;

oneway interface IKmMouseCallback {
    void onMouse(double dx, double dy, double wheelX, double wheelY, int buttons);
    void onCaptureLost();
}
