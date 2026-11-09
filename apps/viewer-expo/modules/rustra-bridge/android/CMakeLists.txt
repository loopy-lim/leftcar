# ── unikit generated ────────────────────────────────────────
# File:   android/CMakeLists.txt
# Source: schema.json (single source of truth for this file)
# Regen:  unikit codegen --config unikit.json
# Stage:  rust-probe schema → ts renderer
# DO NOT EDIT — changes will be overwritten and fail codegen --check.
# ────────────────────────────────────────────────────────────

cmake_minimum_required(VERSION 3.18.1)
project(unikit_bridge)
set(CMAKE_CXX_STANDARD 20)
find_package(fbjni REQUIRED CONFIG)
find_package(ReactAndroid REQUIRED CONFIG)
add_library(unikit_static STATIC IMPORTED)
set_target_properties(unikit_static PROPERTIES IMPORTED_LOCATION ${CMAKE_CURRENT_SOURCE_DIR}/src/main/cpp/libs/${ANDROID_ABI}/lib${UNIKIT_LIB_NAME}.a)
add_library(unikit_bridge SHARED ${UNIKIT_ADAPTER_ROOT}/android/unikit-jsi-jni.cpp ${UNIKIT_ADAPTER_ROOT}/cpp/UnikitJSIBridge.cpp ${UNIKIT_GENERATED_ROOT}/unikit-generated-codecs.cpp)
target_include_directories(unikit_bridge PRIVATE ${UNIKIT_ADAPTER_ROOT}/cpp ${UNIKIT_GENERATED_ROOT})
# folly 헤더 계약 — ReactAndroid prefab의 cmake-utils/folly-flags.cmake 세트와
# 동일하게 유지(RN 0.81 프리팹은 folly_runtime 타깃 미제공).
target_compile_definitions(unikit_bridge PRIVATE FOLLY_NO_CONFIG=1 FOLLY_HAVE_CLOCK_GETTIME=1 FOLLY_USE_LIBCPP=1 FOLLY_CFG_NO_COROUTINES=1 FOLLY_MOBILE=1 FOLLY_HAVE_RECVMMSG=1 FOLLY_HAVE_PTHREAD=1 FOLLY_HAVE_XSI_STRERROR_R=1)
target_link_libraries(unikit_bridge PRIVATE unikit_static fbjni::fbjni ReactAndroid::jsi ReactAndroid::reactnative android log)
# 상위 RN이 folly_runtime 타깃을 제공하면 공식 타깃이 플래그의 원천이 된다.
if(TARGET ReactAndroid::folly_runtime)
  target_link_libraries(unikit_bridge PRIVATE ReactAndroid::folly_runtime)
endif()
