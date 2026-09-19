package dev.leftcar.viewer.stream;

import android.app.Activity;
import androidx.xr.runtime.Session;
import androidx.xr.scenecore.SpatialWindow;

/** Typed Java shim because beta02 exposes SpatialWindow.INSTANCE as a Java field. */
final class SpatialWindowBridge {
  private SpatialWindowBridge() {}

  static void setPreferredAspectRatio(Session session, Activity activity, float ratio) {
    SpatialWindow.INSTANCE.setPreferredAspectRatio(session, activity, ratio);
  }
}
