#import <React/RCTBridgeModule.h>
#import <React/RCTBridge+Private.h>
#import <React/RCTLog.h>
#import <ReactCommon/CallInvoker.h>
#import <ReactCommon/RCTTurboModule.h>
#import <jsi/jsi.h>

#include <cstdlib>
#include <exception>

#import "RustraJSIBridge.hpp"

@interface RustraBridge : NSObject <RCTBridgeModule>
@end

@implementation RustraBridge

RCT_EXPORT_MODULE(RustraBridge)

- (void)invalidate {
  // RCTBridge가 Runtime을 폐기하기 전에 JSI Function과 pending async invoke를
  // 정리한다. 늦게 도착한 Rust callback은 generation guard가 폐기한다.
  rustra::invalidateRustraJSI();
}

RCT_REMAP_METHOD(install,
                  installWithResolver:(RCTPromiseResolveBlock)resolve
                  rejecter:(RCTPromiseRejectBlock)reject) {
  @try {
    // In new arch, self.bridge may not be set.
    // Get the bridge through the RN shared infrastructure.
    RCTBridge *bridge = [RCTBridge currentBridge];
    if (!bridge) {
      reject(@"ERR_NO_BRIDGE", @"[RCTBridge currentBridge] returned nil", nil);
      return;
    }

    RCTCxxBridge *cxxBridge = (RCTCxxBridge *)bridge;
    // JS 스레드 CallInvoker — 이벤트 푸시 drain 을 JS 런타임 스레드로 마샬링.
    // RCTTurboModule 카테고리(RCTBridge (RCTTurboModule))의 jsCallInvoker 접근자는
    // RCTCxxBridge 구현이 제공한다 — shared_ptr<CallInvoker> 를 값으로 반환한다.
    std::shared_ptr<facebook::react::CallInvoker> jsCallInvoker =
        [cxxBridge jsCallInvoker];
    if (!jsCallInvoker) {
      reject(@"ERR_NO_CALL_INVOKER", @"RustraJSI requires a JS CallInvoker", nil);
      return;
    }

    // TurboModule promise methods execute on a native module queue, not the JS
    // Runtime thread. Mutating Hermes through cxxBridge.runtime from this queue
    // races normal JS execution and eventually corrupts the heap during reload.
    // Schedule the complete install through the CallInvoker; its Runtime& is
    // also guaranteed to be the live Runtime associated with this invocation.
    auto typeErasedCallInvoker =
        std::static_pointer_cast<void>(jsCallInvoker);
    RCTPromiseResolveBlock resolveCopy = [resolve copy];
    RCTPromiseRejectBlock rejectCopy = [reject copy];
    jsCallInvoker->invokeAsync(
        [typeErasedCallInvoker = std::move(typeErasedCallInvoker),
         resolveCopy,
         rejectCopy](facebook::jsi::Runtime &runtime) {
          try {
            // dev 핫코어(iOS 시뮬레이터 스코프) — RUSTRA_HOT_CORE_DIR 이 가리키는
            // 디렉터의 `*-hot-live.*` 를 폴링해 dylib 를 스왑한다. 이름 구분 계약:
            // Tauri 계열의 RUSTRA_HOT_CORE 는 파일 경로, RN 은 디렉터(DIR 접미)다.
            // live 아티팩트가 이미 게이트를 통과해 놓여 있으면 설치 전 1회 폴링으로
            // 즉시 스왑해(≤300ms 대신) 첫 호출부터 신 코어로 향하게 한다.
            const char *hotCoreDir = std::getenv("RUSTRA_HOT_CORE_DIR");
            if (hotCoreDir != nullptr && hotCoreDir[0] != '\0') {
              rustra::core::configureHotCore(hotCoreDir);
              rustra::core::pollHotCoreOnce();
              rustra::core::startHotCorePolling();
            }
            rustra::installRustraJSIWithInvoker(runtime, typeErasedCallInvoker);
            resolveCopy(@(YES));
          } catch (const std::exception &error) {
            NSString *message = [NSString stringWithUTF8String:error.what()];
            rejectCopy(@"ERR_INSTALL", message ?: @"Unknown C++ error", nil);
          } catch (...) {
            rejectCopy(@"ERR_INSTALL", @"Unknown native error", nil);
          }
        });
  } @catch (NSException *exception) {
    reject(@"ERR_INSTALL", exception.reason ?: @"Unknown error", nil);
  }
}

@end
