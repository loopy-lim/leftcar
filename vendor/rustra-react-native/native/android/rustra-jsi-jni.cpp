#include <jni.h>
#include <jsi/jsi.h>
#include <android/log.h>
#include <ReactCommon/CallInvoker.h>
#include <ReactCommon/CallInvokerHolder.h>
#include <fbjni/fbjni.h>
#include <memory>
#include "RustraJSIBridge.hpp"

#define LOG_TAG "RustraBridge"
#define LOGI(...) __android_log_print(ANDROID_LOG_INFO, LOG_TAG, __VA_ARGS__)
#define LOGE(...) __android_log_print(ANDROID_LOG_ERROR, LOG_TAG, __VA_ARGS__)

// Java CallInvokerHolderImpl(CallInvokerHolder 하이브리드) → C++ CallInvoker.
// TurboModuleManager.initHybrid 와 동일한 fbjni 경로: adopt_local 로 로컬 ref 를
// 감싸고 cthis() 로 C++ 부분을 꺼낸다. 실패 시 nullptr 반환 — 호출부는 invoker
// 없이 설치해 JS 폴링(drainEvents) 폴백으로 동작시킨다.
static std::shared_ptr<facebook::react::CallInvoker> extractCallInvoker(
    JNIEnv* env, jobject holderObj) {
  if (env == nullptr || holderObj == nullptr) return nullptr;
  try {
    auto holder = facebook::jni::adopt_local<
        facebook::react::CallInvokerHolder::javaobject>(
        reinterpret_cast<
            facebook::react::CallInvokerHolder::javaobject>(holderObj));
    if (!holder) return nullptr;
    return holder->cthis()->getCallInvoker();
  } catch (const std::exception& ex) {
    LOGE("extractCallInvoker: %s", ex.what());
    return nullptr;
  } catch (...) {
    LOGE("extractCallInvoker: unknown exception");
    return nullptr;
  }
}

extern "C" JNIEXPORT jboolean JNICALL
Java_dev_rustra_bridge_RustraBridgeModule_nativeInstall(
    JNIEnv* env,
    jobject ths,
    jlong jsContextNativePointer,
    jobject jsCallInvokerHolder
) {
  if (jsContextNativePointer == 0) {
    LOGE("nativeInstall: jsContextNativePointer is null");
    return JNI_FALSE;
  }

  auto* runtime = reinterpret_cast<facebook::jsi::Runtime*>(jsContextNativePointer);
  if (!runtime) {
    LOGE("nativeInstall: failed to cast pointer to facebook::jsi::Runtime");
    return JNI_FALSE;
  }

  auto invoker = extractCallInvoker(env, jsCallInvokerHolder);
  if (!invoker) {
    LOGI("nativeInstall: jsCallInvoker unavailable — event push falls back to JS polling (drainEvents)");
  }

  try {
    // void shared_ptr 로 type-erase 해 전달 — RustraJSIBridge.cpp 가 iOS/Android
    // 단일 정의를 유지한다.
    rustra::installRustraJSIWithInvoker(
        *runtime, std::static_pointer_cast<void>(invoker));
    LOGI("RustraJSI bindings successfully installed on Android JSI Runtime");
    return JNI_TRUE;
  } catch (const std::exception& ex) {
    LOGE("nativeInstall exception: %s", ex.what());
    return JNI_FALSE;
  } catch (...) {
    LOGE("nativeInstall unknown exception");
    return JNI_FALSE;
  }
}

extern "C" JNIEXPORT void JNICALL
Java_dev_rustra_bridge_RustraBridgeModule_nativeInvalidate(
    JNIEnv*,
    jobject
) {
  rustra::invalidateRustraJSI();
}
