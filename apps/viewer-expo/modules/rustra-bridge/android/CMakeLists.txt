# ── rustra generated ────────────────────────────────────────
# File:   android/CMakeLists.txt
# Source: schema.json (single source of truth for this file)
# Regen:  rustra codegen --config rustra.json
# Stage:  rust-probe schema → ts renderer
# DO NOT EDIT — changes will be overwritten and fail codegen --check.
# ────────────────────────────────────────────────────────────

cmake_minimum_required(VERSION 3.18.1)
project(rustra_bridge)
set(CMAKE_CXX_STANDARD 20)
find_package(fbjni REQUIRED CONFIG)
find_package(ReactAndroid REQUIRED CONFIG)
add_library(rustra_static STATIC IMPORTED)
set_target_properties(rustra_static PROPERTIES IMPORTED_LOCATION ${CMAKE_CURRENT_SOURCE_DIR}/src/main/cpp/libs/${ANDROID_ABI}/lib${RUSTRA_LIB_NAME}.a)
add_library(rustra_bridge SHARED ${RUSTRA_ADAPTER_ROOT}/android/rustra-jsi-jni.cpp ${RUSTRA_ADAPTER_ROOT}/cpp/RustraJSIBridge.cpp ${RUSTRA_GENERATED_ROOT}/rustra-generated-codecs.cpp)
target_include_directories(rustra_bridge PRIVATE ${RUSTRA_ADAPTER_ROOT}/cpp ${RUSTRA_GENERATED_ROOT})
target_compile_definitions(rustra_bridge PRIVATE FOLLY_NO_CONFIG=1 FOLLY_HAVE_CLOCK_GETTIME=1 FOLLY_USE_LIBCPP=1 FOLLY_CFG_NO_COROUTINES=1 FOLLY_MOBILE=1 FOLLY_HAVE_RECVMMSG=1 FOLLY_HAVE_PTHREAD=1 FOLLY_HAVE_XSI_STRERROR_R=1)
target_link_libraries(rustra_bridge PRIVATE rustra_static fbjni::fbjni ReactAndroid::jsi ReactAndroid::reactnative android log)
