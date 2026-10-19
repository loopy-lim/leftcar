package dev.leftcar.viewer

import android.app.Application
import android.content.res.Configuration

import com.facebook.react.PackageList
import com.facebook.react.ReactApplication
import com.facebook.react.ReactNativeApplicationEntryPoint.loadReactNative
import com.facebook.react.ReactPackage
import com.facebook.react.ReactHost
import com.facebook.react.common.ReleaseLevel
import com.facebook.react.defaults.DefaultNewArchitectureEntryPoint

import dev.leftcar.viewer.diagnostics.CrashMarker
import dev.leftcar.viewer.diagnostics.DiagnosticsRuntime

import expo.modules.ApplicationLifecycleDispatcher
import expo.modules.ExpoReactHostFactory

class MainApplication : Application(), ReactApplication {

  override val reactHost: ReactHost by lazy {
    ExpoReactHostFactory.getDefaultReactHost(
      context = applicationContext,
      packageList =
        PackageList(this).packages.apply {
          // Multi-window stream launcher (one OS window per source)
          add(dev.leftcar.viewer.stream.StreamLauncherPackage())
          // Android Open Accessory lifecycle and the native fd bridge.
          add(dev.leftcar.viewer.usb.UsbAccessoryPackage())
          // NSD host discovery (_leftcar._tcp)
          add(dev.leftcar.viewer.nsd.NsdPackage())
          add(dev.leftcar.viewer.crypto.CryptoPackage())
          add(dev.leftcar.viewer.km.KmPackage())
          // Device shutdown diagnostics (docs/plans/2026-10-19-viewer-diagnostics.md)
          add(dev.leftcar.viewer.diagnostics.DiagnosticsPackage())
        }
    )
  }

  override fun onCreate() {
    super.onCreate()
    // JS 초기화와 무관하게 크래시 마커와 15초 샘플러가 프로세스 시작부터 동작한다.
    CrashMarker.install(DiagnosticsRuntime.store(this))
    DefaultNewArchitectureEntryPoint.releaseLevel = try {
      ReleaseLevel.valueOf(BuildConfig.REACT_NATIVE_RELEASE_LEVEL.uppercase())
    } catch (e: IllegalArgumentException) {
      ReleaseLevel.STABLE
    }
    loadReactNative(this)
    ApplicationLifecycleDispatcher.onApplicationCreate(this)
  }

  override fun onConfigurationChanged(newConfig: Configuration) {
    super.onConfigurationChanged(newConfig)
    ApplicationLifecycleDispatcher.onConfigurationChanged(this, newConfig)
  }
}
