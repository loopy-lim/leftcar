#include "RustraJSIBridge.hpp"
#include "RustraTurboInterop.hpp"
#include "rustra-generated-codecs.hpp"
#include <folly/dynamic.h>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <jsi/jsi.h>
#include <limits>
#include <memory>
#include <optional>
#include <vector>

// 핫코어 폴링/dlopen — POSIX 계열(iOS 시뮬레이터/Android/macOS) 전용.
// 그 외 호스트는 정적 모드 고정(파일 말미 stub 참고).
#if defined(__APPLE__) || defined(__ANDROID__)
#include <dirent.h>
#include <dlfcn.h>
#include <chrono>
#include <thread>
#endif

// CallInvoker 는 순수 C++ 헤더(ReactCommon/callinvoker)다 — iOS/Android 모두
// 동일 경로로 제공된다. 플랫폼 글루(.mm / jni.cpp) 가 invoker 를 얻어
// type-erase 해 전달하므로 이 파일은 플랫폼 헤더에 의존하지 않는다.
#if defined(__APPLE__)
#include <ReactCommon/CallInvoker.h>
#elif defined(__ANDROID__)
#include <ReactCommon/CallInvoker.h>
#endif

namespace rustra {

using namespace facebook::jsi;
namespace gen = rustra::generated;
namespace rc = rustra::codec;

// ── 코어 함수 포인터 테이블 (dylib 핫스왑 dev 코어, Phase 3) ─────────────
// 모든 FFI 호출부는 링크 심볼 직접 호출 대신 호출 시점 테이블을 로드한다.
// 정적 모드의 테이블은 링크된 심볼 주소라 기존 직접 호출과 동일 대상이며,
// 핫 스왑 시 atomic publish 로 테이블만 교체된다 — JS 재바인딩은 없다.
//
// 같은 논리 연산(동기 호출 1건, async dispatch 1건)은 진입 시 로드한 테이블
// 하나로만 수행한다 — 응답/버퍼의 free 짝이 반드시 생산 코어와 같은 모듈의
// allocator/debug free_guard 를 쓰게 한다(free_guard 의 live-allocation
// 집합은 모듈별 정적 상태라 교차 free 는 misuse abort 로 이어진다).
using CoreTable = core::CoreTable;

namespace core {
namespace {

// 정적 모드 테이블 — 링크된 심볼 주소(테이블 도입 전 직접 호출과 동일 대상).
const CoreTable kStaticCoreTable = {
    rustra_ffi_invoke,
    rustra_ffi_invoke_json,
    rustra_ffi_invoke_postcard,
    rustra_ffi_invoke_rkyv_v2,
    rustra_ffi_free,
    rustra_ffi_invoke_buffer,
    rustra_ffi_has_buffer,
    rustra_ffi_free_owned_bytes,
    rustra_ffi_event_sink_register,
    rustra_ffi_event_sink_unregister,
    rustra_ffi_channel_create,
    rustra_ffi_channel_send,
    rustra_ffi_channel_create_bytes,
    rustra_ffi_channel_send_bytes,
    rustra_ffi_channel_drop,
    rustra_mobile_init,
    rustra_ffi_invoke_cancel,
    rustra_ffi_invoke_rkyv_v2_async_into,
    rustra_ffi_invoke_rkyv_v2_into,
    rustra_ffi_invoke_raw,
    rustra_ffi_has_raw,
    rustra_ffi_get_schema,
    rustra_ffi_contract_hash,
};

// C++17 호환 홀더 — `std::atomic<std::shared_ptr<const CoreTable>>` 는
// C++20 라이브러리 기능이라 RN 툴체인(Xcode/NDK c++17)에서 쓸 수 없으므로
// atomic 포인터(release store / acquire load)로 동일 의미를 달성한다.
// 발행된 테이블은 불변·무폐기(구 dylib 비-unload 계약과 동일 수명)라
// 포인터 수명 걱정이 없다.
std::atomic<const CoreTable*> g_coreTable{&kStaticCoreTable};

} // namespace

const CoreTable* currentCoreTable() {
  return g_coreTable.load(std::memory_order_acquire);
}

} // namespace core

static double requireInteger(Runtime& rt, const Value& value, double max,
                             const char* label) {
  if (!value.isNumber()) {
    throw JSError(rt, std::string("RustraJSI: ") + label + " must be a number");
  }
  double number = value.asNumber();
  if (!std::isfinite(number) || number < 0.0 || number > max ||
      std::trunc(number) != number) {
    throw JSError(rt, std::string("RustraJSI: ") + label +
      " must be a finite non-negative integer in range");
  }
  return number;
}

static uint32_t requireU32(Runtime& rt, const Value& value, const char* label) {
  return static_cast<uint32_t>(requireInteger(
    rt, value, static_cast<double>(std::numeric_limits<uint32_t>::max()), label));
}

static uint16_t requireU16(Runtime& rt, const Value& value, const char* label) {
  return static_cast<uint16_t>(requireInteger(
    rt, value, static_cast<double>(std::numeric_limits<uint16_t>::max()), label));
}

static uint64_t requireSafeU64(Runtime& rt, const Value& value, const char* label) {
  constexpr double kMaxSafeInteger = 9007199254740991.0;
  return static_cast<uint64_t>(requireInteger(rt, value, kMaxSafeInteger, label));
}

// ── ArrayBuffer helpers ────────────────────────────────────

static Value createArrayBuffer(Runtime& rt, const uint8_t* data, size_t size) {
  // Function/PropNameID handles may never outlive their owning Runtime. A
  // process-global constructor cache corrupted Hermes during RN reload when a
  // later install destroyed the old Runtime's Function handle. This generic
  // copy path is not the direct byte-buffer hot path, so resolve the
  // constructor from the current Runtime instead of retaining a cross-runtime
  // JSI handle.
  Function ctor = rt.global().getPropertyAsFunction(rt, "ArrayBuffer");
  Object ab = ctor.callAsConstructor(rt, static_cast<double>(size))
    .getObject(rt);
  ArrayBuffer buf = ab.getArrayBuffer(rt);
  if (size > 0) {
    if (data == nullptr || buf.data(rt) == nullptr) {
      throw JSError(rt, "RustraJSI: detached or null ArrayBuffer storage");
    }
    std::memcpy(buf.data(rt), data, size);
  }
  return ab;
}

Value generated::make_array_buffer(Runtime& rt, const uint8_t* data, size_t size) {
  return createArrayBuffer(rt, data, size);
}

/// Owns the exact `(ptr, len)` transferred by `rustra_ffi_invoke_buffer`.
/// Hermes keeps the shared MutableBuffer alive for the lifetime of the JS
/// ArrayBuffer, so the Rust allocation is exposed without another bulk copy.
/// The finalizer stores no Runtime/JSI handles and is safe to run after reload.
/// free 짝은 버퍼를 만든 코어(소유 테이블)가 담당한다 — GC 로 소멸이 스왑
/// 뒤로 밀려도 다른 코어의 allocator/free_guard 에 건네지지 않는다.
class RustOwnedMutableBuffer final : public MutableBuffer {
public:
  RustOwnedMutableBuffer(uint8_t* data, size_t size, const CoreTable* owner)
    : data_(data), size_(size), owner_(owner) {}

  ~RustOwnedMutableBuffer() override {
    if (data_ != nullptr) owner_->free_owned_bytes(data_, size_);
  }

  size_t size() const override { return size_; }
  uint8_t* data() override { return data_; }

private:
  uint8_t* data_;
  size_t size_;
  /// 생성 시점 코어 테이블 — 불변·무폐기라 소멸 시점에도 유효하다.
  const CoreTable* owner_;
};

static Value createOwnedArrayBuffer(Runtime& rt, uint8_t* data, size_t size,
                                    const CoreTable* owner) {
  std::shared_ptr<RustOwnedMutableBuffer> buffer;
  try {
    buffer = std::make_shared<RustOwnedMutableBuffer>(data, size, owner);
  } catch (...) {
    owner->free_owned_bytes(data, size);
    throw;
  }
  ArrayBuffer bufferHandle(rt, std::move(buffer));
  return Value(rt, bufferHandle);
}

static std::pair<const uint8_t*, size_t> extractBytes(Runtime& rt, const Value& value) {
  auto obj = value.asObject(rt);

  if (obj.isArrayBuffer(rt)) {
    auto buf = obj.getArrayBuffer(rt);
    if (buf.size(rt) > 0 && buf.data(rt) == nullptr) {
      throw JSError(rt, "RustraJSI: detached ArrayBuffer");
    }
    return {buf.data(rt), buf.size(rt)};
  }

  auto bufferProp = obj.getProperty(rt, "buffer");
  if (bufferProp.isObject() && bufferProp.asObject(rt).isArrayBuffer(rt)) {
    auto buf = bufferProp.asObject(rt).getArrayBuffer(rt);
    // 클램프 — JS 가 건네는 byteOffset/byteLength 는 임의 값일 수 있다
    // (duck-typed 객체 통과). buf 범위 밖이면 네이티브 힙 OOB 읽기가 되므로
    // 명시적 에러로 거부한다. NaN/음수도 여기서 걸러진다.
    auto offsetProp = obj.getProperty(rt, "byteOffset");
    auto lengthProp = obj.getProperty(rt, "byteLength");
    if (!offsetProp.isNumber() || !lengthProp.isNumber()) {
      throw JSError(rt, "RustraJSI: byteOffset/byteLength must be numbers");
    }
    double offsetNum = offsetProp.asNumber();
    double lengthNum = lengthProp.asNumber();
    size_t bufSize = buf.size(rt);
    offsetNum = requireInteger(rt, offsetProp, static_cast<double>(bufSize), "byteOffset");
    lengthNum = requireInteger(rt, lengthProp, static_cast<double>(bufSize), "byteLength");
    if (lengthNum > static_cast<double>(bufSize) - offsetNum) {
      throw JSError(rt, "RustraJSI: byteOffset/byteLength out of buffer bounds");
    }
    auto byteOffset = static_cast<size_t>(offsetNum);
    auto byteLength = static_cast<size_t>(lengthNum);
    auto* data = buf.data(rt);
    if (bufSize > 0 && data == nullptr) {
      throw JSError(rt, "RustraJSI: detached TypedArray buffer");
    }
    return {byteLength == 0 ? data : data + byteOffset, byteLength};
  }

  throw JSError(rt, "RustraJSI: expected ArrayBuffer or TypedArray");
}

static std::pair<const uint8_t*, size_t> extractByteBuffer(
    Runtime& rt, const Value& value) {
  auto obj = value.asObject(rt);
  if (!obj.isArrayBuffer(rt)) {
    auto bytesPerElement = obj.getProperty(rt, "BYTES_PER_ELEMENT");
    if (!bytesPerElement.isNumber() || bytesPerElement.asNumber() != 1.0) {
      throw JSError(rt, "RustraJSI: expected Uint8Array or ArrayBuffer");
    }
  }
  return extractBytes(rt, value);
}

// ── rkyv V2 에러 와이어 파싱 ────────────────────────────────
// 에러 프레임: [ok:0][pad to @8][err_len u16 LE @8][postcard{code,message} @10]
// postcard 파싱 실패 시 원시 바이트로 폴백한다(계약: 실패해도 throw 아님).
// malformed(out_len < 10) 검사는 호출부에서 이미 완료했음을 전제로 한다.
static std::string parseRkyvV2ErrorBody(const uint8_t* resp, size_t out_len) {
  uint16_t errLen = (uint16_t)resp[8] | ((uint16_t)resp[9] << 8);
  size_t avail = out_len > 10 ? out_len - 10 : 0;
  size_t bodyLen = errLen <= avail ? errLen : avail;
  try {
    rc::Reader errReader(resp + 10, bodyLen);
    std::string code = errReader.read_string();
    std::string message = errReader.read_string();
    return code + ": " + message;
  } catch (...) {
    return std::string(reinterpret_cast<const char*>(resp + 10), bodyLen);
  }
}

// ── typed invoke 공통 tail ──────────────────────────────────
// invokeTyped / invokeTypedById / invokeTypedBatch(ById) 의 FFI 이후 꼬리:
// dispatch → 헤더 분기(null / empty / ok=0 에러 / malformed) → (성공 시)
// decoder → free. encode/decode 진입(by name / by id)만 호출부에서 다르다.
// decoder 는 성공 응답 바디(Reader)를 JS Value 로 변환한다.
// 에러면 JSError throw — 기존 세 경로의 메시지 텍스트를 그대로 보존한다:
//   - tailSuffix: malformed 계열(empty/error/success) 접미 — 단건 "", 배치 " (batch)".
//   - batchItemName: 이름 기반 배치 루프의 항목 이름. FFI null 접미
//     " (batch item <name>)" 조립에만 쓴다(에러 시 1회 조립 — hot path 비용 0).
//     nullptr 면 null 접미로 tailSuffix 를 쓴다(단건/byId 배치).
// free 짝 계약: (Tier 1) typedInvokeTail 은 caller-buffer 변형을 쓴다 —
// Rust 가 응답을 할당하지 않고 caller 소유 버퍼에 직접 기록하므로 free 짝이
// 필요 없다. 먼저 512B 스택 버퍼로 바로 dispatch+write하고, 부족한 경우에만
// 코어가 캐시한 같은 응답을 정확한 크기의 vector로 재시도한다. 작은 응답은
// FFI 1회, 큰 응답도 핸들러는 정확히 1회만 실행된다. 테이블은 진입 시 1회
// 로드해 probe/재시도를 같은 코어로 묶는다(스왑은 호출 경계에서만 반영).
template <typename Decode>
static Value typedInvokeTail(Runtime& rt, const uint8_t* reqData, size_t reqSize,
                             const char* tailSuffix, Decode decode,
                             const std::string* batchItemName = nullptr) {
  const CoreTable* core = core::currentCoreTable();
  // (Tier 1) 고정 스택 버퍼 — 대부분의 응답(숫자/작은 객체)이 여기에 들어온다.
  // 부족하면 아래 폴백 경로가 처리하므로 안전하다.
  constexpr size_t kStackCap = 512;
  uint8_t stackBuf[kStackCap];
  size_t out_len = 0;
  const uint8_t* resp = nullptr;
  std::vector<uint8_t> largeBuf;

  // 1단계: 스택 버퍼로 바로 dispatch+write. 대부분의 응답은 여기서 끝나
  // size-probe를 위한 두 번째 FFI 횡단과 thread_local 캐시 왕복이 없다.
  size_t n = core->invoke_rkyv_v2_into(
    reqData, reqSize, stackBuf, kStackCap, &out_len);
  if (n != SIZE_MAX && n > 0) {
    resp = stackBuf;
  }

  // 버퍼가 부족하면 코어가 out_len에 필요 크기를 쓰고 동일 응답을 캐시한다.
  // 정확한 크기로 한 번만 재시도하므로 비멱등 핸들러는 재실행되지 않는다.
  if (!resp && n == SIZE_MAX && out_len > kStackCap) {
    largeBuf.resize(out_len);
    n = core->invoke_rkyv_v2_into(
      reqData, reqSize, largeBuf.data(), largeBuf.size(), &out_len);
    if (n != SIZE_MAX && n > 0) {
      resp = largeBuf.data();
    }
  }
  if (!resp) {
    std::string nullSuffix(tailSuffix);
    if (batchItemName) nullSuffix = " (batch item " + *batchItemName + ")";
    throw JSError(rt, "RustraJSI: invokeRkyvV2 returned null" + nullSuffix);
  }
  if (out_len < 1) {
    throw JSError(rt, std::string("RustraJSI: empty rkyv v2 response") + tailSuffix);
  }
  if (resp[0] == 0) {
    // 에러 와이어: [ok:0][pad to @8][err_len u16 LE @8][err @10]
    if (out_len < 10) {
      throw JSError(rt, std::string("RustraJSI: malformed error response") + tailSuffix);
    }
    std::string errStr = parseRkyvV2ErrorBody(resp, out_len);
    throw JSError(rt, errStr);
  }

  // 성공: postcard(O) @8 부터 디코딩.
  if (out_len < 8) {
    throw JSError(rt, std::string("RustraJSI: malformed success response") + tailSuffix);
  }
  rc::Reader r(resp + 8, out_len - 8);
  Value result = decode(r);
  return result;
}

// ── C++ TurboModule 상호운용 — 공개 typed invoke 진입점 ──────────────────
// 헤더 선언 참고(TypedInvokeStatus 계약). HostFunction(invokeTyped/invokeTypedById)
// 도 같은 경로를 쓴다 — 이 구현이 단일 소스다. 예외 기반 tail(typedInvokeTail)을
// 감싸 status 로 변환한다: Rust 명령 에러 와이어는 "code: message" 문자열로
// 조립되므로(parseRkyvV2ErrorBody) 첫 ':' 기준으로 구조를 복원한다. code 에는
// ':' 가 올 수 없으므로(에러 코드 문자집합 계약) 첫 콜론 분리는 비모호하다.
namespace {

TypedInvokeResult toCommandErrorResult(
  facebook::jsi::Runtime& rt, const std::string& codeAndMessage) {
  size_t sep = codeAndMessage.find(": ");
  std::string code = sep == std::string::npos ? codeAndMessage : codeAndMessage.substr(0, sep);
  std::string detail =
    sep == std::string::npos ? std::string() : codeAndMessage.substr(sep + 2);
  facebook::jsi::Object errorObject(rt);
  errorObject.setProperty(rt, "code", facebook::jsi::String::createFromUtf8(rt, code));
  errorObject.setProperty(rt, "message", facebook::jsi::String::createFromUtf8(rt, detail));
  return TypedInvokeResult{
    TypedInvokeStatus::CommandError,
    facebook::jsi::Value(rt, errorObject),
    codeAndMessage,
  };
}

} // namespace

TypedInvokeResult invokeTypedByName(
  facebook::jsi::Runtime& rt, const std::string& commandName,
  const facebook::jsi::Value& args) {
  rc::Writer w;
  if (!gen::encode_by_name(rt, commandName, args, w)) {
    return TypedInvokeResult{
      TypedInvokeStatus::NoStaticCodec, facebook::jsi::Value(), std::string()};
  }
  try {
    facebook::jsi::Value out = typedInvokeTail(rt, w.data(), w.size(), "",
      [&rt, &commandName](rc::Reader& r) { return gen::decode_by_name(rt, commandName, r); });
    return TypedInvokeResult{
      TypedInvokeStatus::Ok, std::move(out), std::string()};
  } catch (const facebook::jsi::JSError& err) {
    std::string text = err.what();
    if (text.rfind("RustraJSI: ", 0) == 0) {
      return TypedInvokeResult{
        TypedInvokeStatus::MalformedResponse, facebook::jsi::Value(), text};
    }
    return toCommandErrorResult(rt, text);
  }
}

TypedInvokeResult invokeTypedById(
  facebook::jsi::Runtime& rt, uint16_t commandId,
  const facebook::jsi::Value& args) {
  rc::Writer w;
  if (!gen::encode_by_id(rt, commandId, args, w)) {
    return TypedInvokeResult{
      TypedInvokeStatus::NoStaticCodec, facebook::jsi::Value(), std::string()};
  }
  try {
    facebook::jsi::Value out = typedInvokeTail(rt, w.data(), w.size(), "",
      [&rt, commandId](rc::Reader& r) { return gen::decode_by_id(rt, commandId, r); });
    return TypedInvokeResult{
      TypedInvokeStatus::Ok, std::move(out), std::string()};
  } catch (const facebook::jsi::JSError& err) {
    std::string text = err.what();
    if (text.rfind("RustraJSI: ", 0) == 0) {
      return TypedInvokeResult{
        TypedInvokeStatus::MalformedResponse, facebook::jsi::Value(), text};
    }
    return toCommandErrorResult(rt, text);
  }
}

// ── EventDispatcher: Rust → JS push delivery ───────────────
//
// 스레드 마샬링 설계:
//   emitting 스레드(FFI 콜백)          JS 런타임 스레드
//   ────────────────────────────      ─────────────────────────────
//   onRustEvent()                       CallInvoker::invokeAsync
//     lock → queue.push_back             → drain(rt)
//     (drop-oldest if full)                lock → swap queue out
//     invokeAsync(drain) 예약              for each event:
//   ── never touches JS objects ─           listeners_[name].call(payload)
//
// CallInvoker 가 없으면(테스트/폴백) JS 가 drainEvents() 를 폴링 호출해
// 동일한 drain 을 수동으로 실행한다. 두 경로는 같은 drain_scheduled_ 플래그로
// 중복 실행을 막는다.

/// 전역 디스패처 — installRustraJSI 가 생성, 프로세스당 하나.
/// HostObject 와 별개로 살아있어야 FFI 콜백(HostObject 생명주기 밖) 이
/// 안전하게 참조할 수 있다.
static std::shared_ptr<EventDispatcher> g_eventDispatcher = nullptr;
static std::mutex g_dispatcherMutex;

static std::shared_ptr<EventDispatcher> getEventDispatcher() {
  std::lock_guard<std::mutex> lock(g_dispatcherMutex);
  if (!g_eventDispatcher) {
    g_eventDispatcher = std::make_shared<EventDispatcher>();
  }
  return g_eventDispatcher;
}

void EventDispatcher::setCallInvoker(std::shared_ptr<void> invoker) {
  // RN 리로드 대응: install 은 새 Runtime 의 JS 스레드에서 매번 실행되므로
  // 이전 Runtime 소유의 jsi::Function 리스너를 여기서 비운다(방치 시 UAF).
  // 큐의 잔여 이벤트도 이전 런타임 대상이므로 함께 폐기한다.
  bool hadListeners = false;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    callInvoker_ = std::move(invoker);
    hadListeners = !listeners_.empty();
    listeners_.clear();
    hasListeners_.store(false, std::memory_order_release);
    queue_.clear();
    drainScheduled_ = false;
  }
  // mutex_ 를 잡은 채 FFI unregister 를 호출하면 emit 중 onRustEvent 가 같은
  // 락을 기다리는 동안 교착할 수 있으므로 반드시 락 밖에서 해제한다.
  if (hadListeners) {
    // 리스너가 있던 상태로 리로드된 경우 싱크를 해제해 둔다 — 새 번들이
    // setListener 로 다시 등록하면 그때 재설치된다.
    core::currentCoreTable()->event_sink_unregister();
  }
}

void EventDispatcher::setListener(facebook::jsi::Runtime& rt,
                                   const std::string& name,
                                   facebook::jsi::Function callback) {
  // JS 스레드에서만 호출됨(HostFunction 경유) — listeners_ 락 없이 접근.
  // jsi::Function 은 default-constructible 하지 않으므로 insert_or_assign 사용
  // (operator[] 는 기본 생성을 요구한다).
  bool wasEmpty = listeners_.empty();
  listeners_.insert_or_assign(name, std::move(callback));
  // 핫코어 스왑 재등록(rebindEventSink)이 읽는 원자 플래그 — 맵과 함께 유지.
  hasListeners_.store(true, std::memory_order_release);
  // 첫 리스너 등록 시 FFI 싱크를 설치한다(폴링 경로 → 푸시 전환).
  if (wasEmpty) {
    core::currentCoreTable()->event_sink_register(&EventDispatcher::onRustEvent, this);
  }
}

void EventDispatcher::removeListener(const std::string& name) {
  listeners_.erase(name);
  hasListeners_.store(!listeners_.empty(), std::memory_order_release);
  // 마지막 리스너 제거 시 FFI 싱크 해제(푸시 → 폴링 복귀).
  if (listeners_.empty()) {
    core::currentCoreTable()->event_sink_unregister();
  }
}

void EventDispatcher::rebindEventSink() {
  // 핫코어 스왑 직후(폴링 스레드) 호출 — 리스너 맵은 JS 스레드 전용이라
  // 읽지 않고 원자 플래그만 본다. 현재(신) 코어의 싱크 슬롯에 C++ 정적
  // 콜백을 재등록한다(콜백 재사용 안전 — 함수 포인터는 모듈 무관).
  if (!hasListeners_.load(std::memory_order_acquire)) return;
  core::currentCoreTable()->event_sink_register(&EventDispatcher::onRustEvent, this);
}

void EventDispatcher::onRustEvent(void* user_data, const char* name,
                                   const char* payload) {
  auto* self = static_cast<EventDispatcher*>(user_data);
  if (!self || !name || !payload) return;

  std::lock_guard<std::mutex> lock(self->mutex_);
  if (self->queue_.size() >= self->capacity_) {
    self->queue_.pop_front();
    ++self->dropped_;
  }
  self->queue_.emplace_back(name, payload);
  self->scheduleDrainLocked();
}

void EventDispatcher::scheduleDrainLocked() {
  // 락을 잡은 상태에서 호출됨. CallInvoker 가 있으면 drain 을 JS 스레드로
  // 예약한다 — invokeAsync 자체는 스레드 안전하다.
  if (drainScheduled_ || !callInvoker_) return;
  drainScheduled_ = true;

  auto self = shared_from_this();
  std::shared_ptr<void> invoker = callInvoker_;
  auto weak = std::weak_ptr<EventDispatcher>(self);
#if defined(__APPLE__) || defined(__ANDROID__)
  auto* nativeInvoker = static_cast<facebook::react::CallInvoker*>(invoker.get());
  nativeInvoker->invokeAsync([weak](facebook::jsi::Runtime& rt) {
    if (auto dispatcher = weak.lock()) {
      dispatcher->drain(rt);
    }
  });
#endif
}

void EventDispatcher::drain(facebook::jsi::Runtime& rt) {
  // JS 런타임 스레드에서만 호출된다(CallInvoker 콜백 또는 drainEvents()).
  std::deque<std::pair<std::string, std::string>> events;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    drainScheduled_ = false;
    events.swap(queue_);
  }

  for (auto& [name, payload] : events) {
    auto it = listeners_.find(name);
    if (it == listeners_.end()) continue;
    try {
      // 페이로드는 JSON 문자열 그대로 JS 로 — 파싱은 TS 래퍼에서 1회.
      // (JSI 경계를 넘기는 비용 < C++ 에서 JSON 파서를 두는 비용)
      it->second.call(rt, facebook::jsi::String::createFromUtf8(
        rt, reinterpret_cast<const uint8_t*>(payload.data()), payload.size()));
    } catch (const facebook::jsi::JSError& e) {
      // JS 콜백이 throw 해도 drain 은 계속한다 — 나머지 이벤트가 유실되지
      // 않게 한다(Rust 싱크의 패닉 격리 정책과 대칭).
      fprintf(stderr, "RustraJSI: event listener for '%s' threw: %s\n",
              name.c_str(), e.getMessage().c_str());
    }
  }
}

size_t EventDispatcher::pendingCount() {
  std::lock_guard<std::mutex> lock(mutex_);
  return queue_.size();
}

// enable_shared_from_this — scheduleDrainLocked 가 안전하게 self 를 캡처.
// (클래스 정의는 헤더에 있으므로 여기는 static_assert 로 계약 문서화)
static_assert(sizeof(EventDispatcher) > 0, "EventDispatcher must be complete");

// ── ChannelDispatcher: 채널 핸들별 유니캐스트 회신 (타입 패리티 2단계) ──
//
// EventDispatcher 와 동일한 마샬링 구조다 — FFI 콜백(send 스레드)은 큐에
// 적재만, JS 스레드 drain 이 callbacks_[handle] 호출. 차이점:
// - 콜백 레지스트리 키가 이벤트 이름(브로드캐스트)이 아니라 핸들(유니캐스트).
// - reset() 시 Rust 채널도 함께 drop — 채널은 호출 귀속이라 리로드된
//   런타임의 핸들은 무의미하다(이벤트 리스너와 달리 재등록되지 않는다).

static std::shared_ptr<ChannelDispatcher> g_channelDispatcher = nullptr;
static std::mutex g_channelDispatcherMutex;

static std::shared_ptr<ChannelDispatcher> getChannelDispatcher() {
  std::lock_guard<std::mutex> lock(g_channelDispatcherMutex);
  if (!g_channelDispatcher) {
    g_channelDispatcher = std::make_shared<ChannelDispatcher>();
  }
  return g_channelDispatcher;
}

void ChannelDispatcher::setCallInvoker(std::shared_ptr<void> invoker) {
  // mutex_ 없이 콜백 맵 정리(레지스트리는 JS 스레드 전용) 후 락 내부에서
  // invoker 교체·채널 drop. drop 이 FFI 를 호출하므로 reset() 은 락 밖 실행.
  std::vector<std::pair<uint32_t, const core::CoreTable*>> toDrop;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    callInvoker_ = std::move(invoker);
    for (auto& [handle, owner] : channelCores_) toDrop.emplace_back(handle, owner);
    callbacks_.clear();
    channelCores_.clear();
    queue_.clear();
    bytesQueue_.clear();
    bytesHandles_.clear();
    drainScheduled_ = false;
  }
  // 리로드 대응: 귀속 채널 전부를 발급 코어에서 drop(락 밖 — FFI 재진입 방지).
  // 소유 코어로 라우팅한다 — 핸들은 코어 귀속이라 스왑 뒤 현재 코어에 같은
  // 번호가 새로 발급돼 있어도 그것을 오해제하는 일이 없다.
  for (auto& [handle, owner] : toDrop) {
    owner->channel_drop(handle);
  }
}

uint32_t ChannelDispatcher::create(facebook::jsi::Runtime& rt,
                                    facebook::jsi::Function callback) {
  // JS 스레드에서만 호출됨(HostFunction 경유). FFI 가 핸들을 선발급하고
  // 콜백이 그 핸들을 캡처해 회신하므로, 여기선 JS 콜백만 핸들 키로 등록.
  // 발급 코어를 기록한다 — drop/폐기 라우팅의 소유권 원천.
  (void)rt;
  const core::CoreTable* owner = core::currentCoreTable();
  uint32_t handle =
    owner->channel_create(&ChannelDispatcher::onChannelPayload, this);
  if (handle == 0) return 0; // 발급 실패 sentinel — 사실상 도달하지 않는다.
  callbacks_.insert_or_assign(handle, std::move(callback));
  channelCores_.insert_or_assign(handle, owner);
  return handle;
}

uint32_t ChannelDispatcher::createBytes(facebook::jsi::Runtime& rt,
                                         facebook::jsi::Function callback) {
  // JSON 경로와 동일한 등록 + 바이너리 경로 FFI 발급. bytesHandles_ 표시로
  // drain 이 ArrayBuffer 로 전달한다.
  (void)rt;
  const core::CoreTable* owner = core::currentCoreTable();
  uint32_t handle =
    owner->channel_create_bytes(&ChannelDispatcher::onChannelPayloadBytes, this);
  if (handle == 0) return 0;
  callbacks_.insert_or_assign(handle, std::move(callback));
  channelCores_.insert_or_assign(handle, owner);
  {
    std::lock_guard<std::mutex> lock(mutex_);
    bytesHandles_.insert(handle);
  }
  return handle;
}

bool ChannelDispatcher::drop(uint32_t handle) {
  // JS 스레드 호출. 발급 코어에서 Rust 채널 해제 후 콜백 제거. 해제 후 drain 에
  // 이미 적재된 해당 핸들 페이로드는 콜백 부재로 무시된다(유니캐스트 만료).
  // 발급 코어로 라우팅한다 — 스왑 뒤 새 코어에 같은 번호가 재발급돼 있어도
  // 발급 주체를 해제한다(코어 귀속 계약).
  auto ownerIt = channelCores_.find(handle);
  const core::CoreTable* owner =
    ownerIt != channelCores_.end() ? ownerIt->second : core::currentCoreTable();
  int dropped = owner->channel_drop(handle);
  callbacks_.erase(handle);
  channelCores_.erase(handle);
  {
    std::lock_guard<std::mutex> lock(mutex_);
    bytesHandles_.erase(handle);
  }
  return dropped == 1;
}

void ChannelDispatcher::onChannelPayload(void* user_data, uint32_t handle,
                                          const char* payload) {
  // send 스레드에서 호출 — JS 객체 미접근, 큐 적재 + drain 예약만.
  auto* self = static_cast<ChannelDispatcher*>(user_data);
  if (!self || !payload) return;

  std::lock_guard<std::mutex> lock(self->mutex_);
  if (self->queue_.size() >= self->capacity_) {
    self->queue_.pop_front(); // drop-oldest — JS 가 느려도 send 스레드 비블록
  }
  // payload 는 NUL 종결 C 문자열 — FfiChannelSink 가 CString 으로 만들어
  // 전달했으므로 여기서 복사해 소유한다(콜백 반환 후 무효).
  self->queue_.emplace_back(handle, std::string(payload));
  self->scheduleDrainLocked();
}

void ChannelDispatcher::onChannelPayloadBytes(
  void* user_data, uint32_t handle, const uint8_t* payload, size_t payload_len) {
  // send 스레드 — JSON 경로와 동일하게 큐 적재 + drain 예약만(복사 소유).
  auto* self = static_cast<ChannelDispatcher*>(user_data);
  if (!self) return;

  std::lock_guard<std::mutex> lock(self->mutex_);
  if (self->bytesQueue_.size() >= self->capacity_) {
    self->bytesQueue_.pop_front(); // drop-oldest — JSON 경로와 동일 정책
  }
  const uint8_t* src = payload ? payload : reinterpret_cast<const uint8_t*>("");
  self->bytesQueue_.emplace_back(
    handle, std::vector<uint8_t>(src, src + payload_len));
  self->scheduleDrainLocked();
}

void ChannelDispatcher::drain(facebook::jsi::Runtime& rt) {
  // JS 런타임 스레드에서만 호출(CallInvoker 콜백 또는 폴링). 스왑 리셋
  // 요청이 쌓여 있으면 먼저 소비한다 — 레지스트리 폐기는 이 스레드에서만.
  if (resetQueued_.exchange(false, std::memory_order_acq_rel)) {
    dropStaleChannelsAfterSwap();
  }
  std::deque<std::pair<uint32_t, std::string>> items;
  std::deque<std::pair<uint32_t, std::vector<uint8_t>>> byteItems;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    drainScheduled_ = false;
    items.swap(queue_);
    byteItems.swap(bytesQueue_);
  }
  for (auto& [handle, payload] : byteItems) {
    auto it = callbacks_.find(handle);
    if (it == callbacks_.end()) continue; // 만료 채널 — 조용히 무시
    try {
      // 바이너리 페이로드는 복사본 ArrayBuffer 로 — 소유권 이전 없이 안전.
      it->second.call(rt, createArrayBuffer(rt, payload.data(), payload.size()));
    } catch (const std::exception&) {
      // JSON 경로와 동일 정책 — 콜백 예외 무시, 나머지 프레임 계속 전달.
    }
  }
  for (auto& [handle, payload] : items) {
    auto it = callbacks_.find(handle);
    if (it == callbacks_.end()) continue; // 만료 채널 — 조용히 무시
    try {
      // 페이로드는 JSON 문자열 그대로 JS 로 — 파싱은 TS 래퍼에서 1회.
      it->second.call(rt, facebook::jsi::String::createFromUtf8(rt, payload));
    } catch (const std::exception&) {
      // JS 콜백 예외는 무시 — 이벤트 drain 과 동일 정책(호출자 보호).
    }
  }
}

size_t ChannelDispatcher::pendingCount() {
  std::lock_guard<std::mutex> lock(mutex_);
  return queue_.size() + bytesQueue_.size();
}

void ChannelDispatcher::scheduleDrainLocked() {
  if (drainScheduled_ || !callInvoker_) return;
  drainScheduled_ = true;

  auto self = shared_from_this();
  std::shared_ptr<void> invoker = callInvoker_;
  auto weak = std::weak_ptr<ChannelDispatcher>(self);
#if defined(__APPLE__) || defined(__ANDROID__)
  auto* nativeInvoker = static_cast<facebook::react::CallInvoker*>(invoker.get());
  nativeInvoker->invokeAsync([weak](facebook::jsi::Runtime& rt) {
    if (auto dispatcher = weak.lock()) {
      dispatcher->drain(rt);
    }
  });
#endif
}

void ChannelDispatcher::reset() {
  // 리로드 대응 전체 폐기 — JS 콜백 맵·큐 클리어 후 발급 코어에서 채널
  // drop(락 밖). 전체 폐기이므로 바이너리 큐/핸들 표시도 함께 비운다.
  std::vector<std::pair<uint32_t, const core::CoreTable*>> toDrop;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    for (auto& [handle, owner] : channelCores_) toDrop.emplace_back(handle, owner);
    callbacks_.clear();
    channelCores_.clear();
    queue_.clear();
    bytesQueue_.clear();
    bytesHandles_.clear();
    drainScheduled_ = false;
  }
  for (auto& [handle, owner] : toDrop) {
    owner->channel_drop(handle);
  }
}

void ChannelDispatcher::requestResetAfterSwap() {
  // 폴링 스레드 — 레지스트리(callbacks_/channelCores_)는 JS 스레드 전용이므로
  // 플래그 + drain 예약만 한다. 실제 폐기는 drain 안의
  // dropStaleChannelsAfterSwap(JS 스레드)이 수행한다.
  std::lock_guard<std::mutex> lock(mutex_);
  resetQueued_.store(true, std::memory_order_release);
  scheduleDrainLocked();
}

void ChannelDispatcher::dropStaleChannelsAfterSwap() {
  // drain(JS 스레드) 안에서만 호출 — 레지스트리 수정은 이 스레드로 국한.
  // 발급 코어가 현재 코어가 아닌(스왑으로 은퇴한) 채널만 폐기한다: 스왑
  // 발행 뒤 새 코어로 새로 만든 채널은 유지한다(발행→drain 사이 창의
  // 신규 생성 보호). 채널은 "스왑 시 코어 내 상태 소실" 설계 정책에 따라
  // 구 코어 발급분은 만료다 — 새 코어의 같은 번호 채널 오해제 창을 닫는다.
  const core::CoreTable* current = core::currentCoreTable();
  std::vector<std::pair<uint32_t, const core::CoreTable*>> toDrop;
  for (auto it = channelCores_.begin(); it != channelCores_.end();) {
    if (it->second != current) {
      toDrop.emplace_back(it->first, it->second);
      callbacks_.erase(it->first);
      it = channelCores_.erase(it);
    } else {
      ++it;
    }
  }
  if (toDrop.empty()) return;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    for (auto& [handle, _owner] : toDrop) bytesHandles_.erase(handle);
  }
  // FFI 는 락 밖 — owner 테이블은 불변·무폐기라 맵에서 지운 뒤에도 안전하다.
  for (auto& [handle, owner] : toDrop) {
    owner->channel_drop(handle);
  }
}

// ── Async invocation lifetime / Runtime reload safety ─────────────────
// Async callback은 Rust worker에서 도착하지만 JSI Function은 생성한 Runtime의
// JS 스레드에서만 호출/폐기해야 한다. shared context registry를 두어 플랫폼
// module invalidate 또는 다음 install 시 pending id를 취소하고 Function을
// 먼저 JS 스레드에서 reset한다. 늦은 native callback은 shared context만
// 정리하고 구 Runtime을 절대 건드리지 않는다.
struct AsyncCallContext {
  std::string commandName;
  /// (G2) byId 진입 플래그 + cmd id — byId 경로에서는 이름 문자열 복사를
  /// 피하기 위해 commandName 을 비워 두고 id 만 실는다(decode_by_id 진단용).
  bool hasCommandId = false;
  uint16_t commandId = 0;
  std::optional<facebook::jsi::Function> onSuccess;
  std::optional<facebook::jsi::Function> onError;
  std::shared_ptr<void> callInvoker;
  std::mutex mutex;
  bool valid = true;
  uint64_t generation = 0;
  uint64_t invocationId = 0;
  /// (F3) caller-buffer async 응답 버퍼 — Rust 워커가 응답을 여기에 직접
  /// 기록한다(owned=0). context(shared_ptr)가 완료 콜백과 JS 스레드 전달
  /// 람다까지 수명을 보장하므로 복사 없이 제자리 읽는다. 버퍼에 안 들어가는
  /// 응답만 Rust heap 프레임으로 돌아온다(owned=1 → free 짝).
  std::vector<uint8_t> frameBuffer = std::vector<uint8_t>(512);
  /// dispatch 시점 코어 테이블 — owned=1 프레임의 free 짝은 생산 코어가
  /// 담당한다(스왑이 콜백보다 먼저 일어나도 교차 코어 해제가 없다).
  /// registerAsyncContext 전에 설정되므로 콜백에서 절대 null 이 아니다.
  const CoreTable* dispatchCore = nullptr;
};

static std::atomic<uint64_t> g_runtimeGeneration{0};
static std::mutex g_asyncContextsMutex;
static std::unordered_map<AsyncCallContext*, std::shared_ptr<AsyncCallContext>>
  g_asyncContexts;

static void registerAsyncContext(const std::shared_ptr<AsyncCallContext>& ctx) {
  std::lock_guard<std::mutex> lock(g_asyncContextsMutex);
  g_asyncContexts.insert_or_assign(ctx.get(), ctx);
}

static void unregisterAsyncContext(const std::shared_ptr<AsyncCallContext>& ctx) {
  std::lock_guard<std::mutex> lock(g_asyncContextsMutex);
  g_asyncContexts.erase(ctx.get());
}

void invalidateRustraJSI() {
  g_runtimeGeneration.fetch_add(1, std::memory_order_acq_rel);
  std::vector<uint64_t> pendingIds;
  {
    // registry lock이 context 수명을 고정한다. Function reset은 플랫폼이
    // 보장한 JS thread의 invalidate/install 경로에서만 실행된다.
    std::lock_guard<std::mutex> registryLock(g_asyncContextsMutex);
    pendingIds.reserve(g_asyncContexts.size());
    for (auto& [_, ctx] : g_asyncContexts) {
      std::lock_guard<std::mutex> contextLock(ctx->mutex);
      ctx->valid = false;
      if (ctx->invocationId != 0) pendingIds.push_back(ctx->invocationId);
      ctx->onSuccess.reset();
      ctx->onError.reset();
      ctx->callInvoker.reset();
    }
    // native callback의 heap-held shared_ptr가 완료까지 context를 살린다.
    g_asyncContexts.clear();
  }
  for (uint64_t id : pendingIds) {
    // 스왑을 지난 id 는 새 코어에 없어 false(무해 no-op)다 — 협력적 취소 계약.
    core::currentCoreTable()->invoke_cancel(id);
  }
}

// ── HostObject with cached functions ───────────────────────

// generic FFI invoke 시그니처 — makeInvoke 가 CoreTable 멤버 선택에 쓴다.
using InvokeFn = uint8_t*(*)(const uint8_t*, size_t, size_t*);

// live schema FFI (from rustra crate) — 선언은 RustraJSIBridge.hpp extern "C"
// 블록으로 옮겨졌다(정적 테이블 초기화와 23심볼 바인딩의 단일 선언 지점).

RustraHostObject::RustraHostObject(Runtime& rt) {
  // makeInvoke — fn/free 를 람다 캡처하지 않고 호출 시점 테이블에서 읽는다.
  // 핫코어 스왑 뒤 다음 호출부터 새 코어로 향한다. fn/free 를 같은 테이블
  // 로드에서 꺼내므로 free 짝이 같은 코어의 allocator 를 쓴다.
  auto makeInvoke = [&](const char* name, InvokeFn CoreTable::* fnMember, const char* err) {
    auto propNameId = PropNameID::forAscii(rt, name);
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 1,
      [fnMember, err](Runtime& rt, const Value&, const Value* args, size_t count) -> Value {
        if (count < 1) {
          throw JSError(rt, std::string("RustraJSI: requires 1 argument — ") + err);
        }
        auto [data, size] = extractBytes(rt, args[0]);
        const CoreTable* core = core::currentCoreTable();
        size_t out_len = 0;
        uint8_t* result = (core->*fnMember)(data, size, &out_len);
        if (!result) {
          throw JSError(rt, std::string("RustraJSI: ") + err);
        }
        auto returnValue = createArrayBuffer(rt, result, out_len);
        core->free(result, out_len);
        return returnValue;
      });
    cache_[name] = std::make_unique<CachedFunction>(
      CachedFunction{std::move(propNameId), std::move(hostFn)});
  };

  // ── Generic FFI paths (default, json, postcard, rkyv V2) — magic 헤더
  //    레이아웃이므로 free 로 해제 짝. ─────────────────────
  makeInvoke("invoke",            &CoreTable::invoke,          "Rust returned null");
  makeInvoke("invokeJson",        &CoreTable::invoke_json,     "Rust json returned null");
  makeInvoke("invokePostcardFFI", &CoreTable::invoke_postcard, "Rust postcard FFI returned null");
  // rkyv V2 는 코어 제네릭 심볼 직결이며 legacy ifdef 밖에 둔다 — 엔진 tier2/3
  // 폴백이 모든 빌드(legacy-OFF 포함)에서 이 함수를 요구한다(RustraNative
  // non-optional). 응답은 코어 FFI 레이아웃(8B magic 헤더)이므로 free 짝은
  // rustra_ffi_free (과거 double-free 크래시의 free-짝 계약 유지).
  makeInvoke("invokeRkyvV2",      &CoreTable::invoke_rkyv_v2,  "Rust rkyv v2 returned null");

  // noop: returns input bytes unchanged
  {
    auto propNameId = PropNameID::forAscii(rt, "noop");
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 1,
      [](Runtime& rt, const Value&, const Value* args, size_t count) -> Value {
        if (count != 1) {
          throw JSError(rt, "RustraJSI: noop requires exactly one ArrayBuffer");
        }
        auto [data, size] = extractBytes(rt, args[0]);
        return createArrayBuffer(rt, data, size);
      });
    cache_["noop"] = std::make_unique<CachedFunction>(
      CachedFunction{std::move(propNameId), std::move(hostFn)});
  }

  // getSchema: live schema query → get_schema (정적 + 동적 명령)
  {
    auto propNameId = PropNameID::forAscii(rt, "getSchema");
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 0,
      [](Runtime& rt, const Value&, const Value*, size_t) -> Value {
        // 같은 테이블 로드에서 호출+free 짝 — 교차 코어 해제를 원천 차단.
        const CoreTable* core = core::currentCoreTable();
        size_t out_len = 0;
        uint8_t* data = core->get_schema(&out_len);
        if (!data) {
          throw JSError(rt, "RustraJSI: getSchema returned null");
        }
        auto returnValue = createArrayBuffer(rt, data, out_len);
        core->free(data, out_len);
        return returnValue;
      });
    cache_["getSchema"] = std::make_unique<CachedFunction>(
      CachedFunction{std::move(propNameId), std::move(hostFn)});
  }

  // getContractHash: (F5) native 빌드 계약 해시 → contract_hash.
  // 엔진 옵션 contractHash 설정 시 JS 가 생성된 GENERATED_CONTRACT_HASH 와
  // 비교해 스키마 드리프트(contract.mismatch)를 검증한다.
  {
    auto propNameId = PropNameID::forAscii(rt, "getContractHash");
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 0,
      [](Runtime& rt, const Value&, const Value*, size_t) -> Value {
        const CoreTable* core = core::currentCoreTable();
        size_t out_len = 0;
        uint8_t* data = core->contract_hash(&out_len);
        if (!data) {
          throw JSError(rt, "RustraJSI: getContractHash returned null");
        }
        auto returnValue = createArrayBuffer(rt, data, out_len);
        core->free(data, out_len);
        return returnValue;
      });
    cache_["getContractHash"] = std::make_unique<CachedFunction>(
      CachedFunction{std::move(propNameId), std::move(hostFn)});
  }

  // ── Event push: onEvent(name, cb) / offEvent(name) / drainEvents() ──
  // JS 콜백 등록은 HostFunction 에서 즉시 EventDispatcher 에 반영된다.
  // 등록 시점에 FFI 싱크가 설치되고, 이후 emit 은 큐 → CallInvoker → drain 경로로
  // 이 콜백에 도달한다. 페이로드는 JSON 문자열 — TS 래퍼가 JSON.parse 1회.
  {
    auto dispatcher = getEventDispatcher();
    auto propNameId = PropNameID::forAscii(rt, "onEvent");
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 2,
      [dispatcher](Runtime& rt, const Value&, const Value* args, size_t count) -> Value {
        if (count < 2) {
          throw JSError(rt, "RustraJSI: onEvent requires (name, callback)");
        }
        std::string name = args[0].asString(rt).utf8(rt);
        if (!args[1].isObject() || !args[1].asObject(rt).isFunction(rt)) {
          throw JSError(rt, "RustraJSI: onEvent callback must be a function");
        }
        Function cb = args[1].asObject(rt).getFunction(rt);
        dispatcher->setListener(rt, name, std::move(cb));
        return Value::undefined();
      });
    cache_["onEvent"] = std::make_unique<CachedFunction>(
      CachedFunction{std::move(propNameId), std::move(hostFn)});
  }
  {
    auto dispatcher = getEventDispatcher();
    auto propNameId = PropNameID::forAscii(rt, "offEvent");
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 1,
      [dispatcher](Runtime& rt, const Value&, const Value* args, size_t count) -> Value {
        if (count < 1) {
          throw JSError(rt, "RustraJSI: offEvent requires (name)");
        }
        std::string name = args[0].asString(rt).utf8(rt);
        dispatcher->removeListener(name);
        return Value::undefined();
      });
    cache_["offEvent"] = std::make_unique<CachedFunction>(
      CachedFunction{std::move(propNameId), std::move(hostFn)});
  }
  // drainEvents(): CallInvoker 없는 호스트의 JS 폴링 drain. 이벤트 큐와 채널
  // 큐(JSON+바이너리)를 모두 소비한다 — 반환값 = 처리한 프레임 수(이벤트 +
  // 채널 합산). CallInvoker 경로가 켜져 있으면 보통 비어 있다(자동 drain 됨).
  {
    auto dispatcher = getEventDispatcher();
    auto channels = getChannelDispatcher();
    auto propNameId = PropNameID::forAscii(rt, "drainEvents");
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 0,
      [dispatcher, channels](Runtime& rt, const Value&, const Value*, size_t) -> Value {
        size_t before = dispatcher->pendingCount() + channels->pendingCount();
        dispatcher->drain(rt);
        channels->drain(rt);
        return Value(static_cast<double>(before));
      });
    cache_["drainEvents"] = std::make_unique<CachedFunction>(
      CachedFunction{std::move(propNameId), std::move(hostFn)});
  }

  // ── Channel: createChannel(cb) / dropChannel(handle) (타입 패리티 2단계) ──
  // createChannel 은 JS 콜백에 u32 핸들을 발급해 되돌려준다 — JS 는 이 값을
  // 커맨드 인자 channel(ChannelHandle = number) 로 그대로 전달한다.
  // Rust 가 channel.send 하면 onChannelPayload → (드레인) → 등록한 cb(payload).
  // 호출 완료/취소 시 dropChannel(handle) — 이후 send 는 조용히 만료(false).
  {
    auto dispatcher = getChannelDispatcher();
    auto propNameId = PropNameID::forAscii(rt, "createChannel");
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 1,
      [dispatcher](Runtime& rt, const Value&, const Value* args, size_t count) -> Value {
        if (count < 1 || !args[0].isObject() || !args[0].asObject(rt).isFunction(rt)) {
          throw JSError(rt, "RustraJSI: createChannel requires (callback)");
        }
        Function cb = args[0].asObject(rt).getFunction(rt);
        uint32_t handle = dispatcher->create(rt, std::move(cb));
        return Value(static_cast<double>(handle));
      });
    cache_["createChannel"] = std::make_unique<CachedFunction>(
      CachedFunction{std::move(propNameId), std::move(hostFn)});
  }
  {
    // 바이너리 채널 — 콜백이 ArrayBuffer(복사본)를 받는다. rkyv V2 프레임 등
    // 임의 바이트를 JSON 직렬화 없이 흘리는 TurboModule 상호운용 경로.
    auto dispatcher = getChannelDispatcher();
    auto propNameId = PropNameID::forAscii(rt, "createChannelBytes");
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 1,
      [dispatcher](Runtime& rt, const Value&, const Value* args, size_t count) -> Value {
        if (count < 1 || !args[0].isObject() || !args[0].asObject(rt).isFunction(rt)) {
          throw JSError(rt, "RustraJSI: createChannelBytes requires (callback)");
        }
        Function cb = args[0].asObject(rt).getFunction(rt);
        uint32_t handle = dispatcher->createBytes(rt, std::move(cb));
        return Value(static_cast<double>(handle));
      });
    cache_["createChannelBytes"] = std::make_unique<CachedFunction>(
      CachedFunction{std::move(propNameId), std::move(hostFn)});
  }
  {
    auto dispatcher = getChannelDispatcher();
    auto propNameId = PropNameID::forAscii(rt, "dropChannel");
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 1,
      [dispatcher](Runtime& rt, const Value&, const Value* args, size_t count) -> Value {
        if (count < 1) {
          throw JSError(rt, "RustraJSI: dropChannel requires (handle)");
        }
        uint32_t handle = requireU32(rt, args[0], "channel handle");
        return Value(dispatcher->drop(handle) ? true : false);
      });
    cache_["dropChannel"] = std::make_unique<CachedFunction>(
      CachedFunction{std::move(propNameId), std::move(hostFn)});
  }

  // ── B1 fast path: 정적 명령을 C++ postcard 코덱으로 인코딩/디코딩 ──
  // JS 측 codec.encode/decode(~3.4µs)를 C++로 옮겨 JS↔바이트 왕복을 제거.
  // 동적 명령은 hasStaticCodec() == false → JS가 Tier 3 JSON fallback.

  // hasStaticCodec(name): codegen 시점에 알려진 정적 명령인지 반환.
  {
    auto propNameId = PropNameID::forAscii(rt, "hasStaticCodec");
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 1,
      [](Runtime& rt, const Value&, const Value* args, size_t count) -> Value {
        if (count < 1) {
          throw JSError(rt, "RustraJSI: hasStaticCodec requires 1 argument");
        }
        std::string name = args[0].asString(rt).utf8(rt);
        return Value(rt, gen::has_static_codec(name));
      });
    cache_["hasStaticCodec"] = std::make_unique<CachedFunction>(
      CachedFunction{std::move(propNameId), std::move(hostFn)});
  }

  // getCodecCapabilities(cmdId): 엔진 구성 시 한 번 캐시할 정적 라우팅 마스크.
  // bit0=typed, bit1=positional, bit2=raw, bit3=single byte buffer. raw 는 생성 메타데이터와 실제
  // Rust registry handler를 함께 확인해 오래된/동적 registry 드리프트에서
  // 잘못 광고하지 않는다.
  {
    auto propNameId = PropNameID::forAscii(rt, "getCodecCapabilities");
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 1,
      [](Runtime& rt, const Value&, const Value* args, size_t count) -> Value {
        if (count < 1) {
          throw JSError(rt, "RustraJSI: getCodecCapabilities requires (cmdId)");
        }
        uint16_t cmdId = requireU16(rt, args[0], "command id");
        // 라우팅 마스크 조회도 현재 코어 기준 — 스왑 뒤 신 코어의 실제
        // handler 보유를 따라간다(오래된 코어를 광고하지 않는다).
        const CoreTable* core = core::currentCoreTable();
        uint32_t capabilities = 0;
        if (gen::has_static_codec_id(cmdId)) capabilities |= 1u;
        if (gen::has_pos_codec(cmdId)) capabilities |= 2u;
        if (gen::has_raw_codec(cmdId) && core->has_raw(cmdId) != 0) capabilities |= 4u;
        if (gen::has_buffer_codec(cmdId) && core->has_buffer(cmdId) != 0) capabilities |= 8u;
        return Value(static_cast<double>(capabilities));
      });
    cache_["getCodecCapabilities"] = std::make_unique<CachedFunction>(
      CachedFunction{std::move(propNameId), std::move(hostFn)});
  }

  // ── (Tier 0.5) invokeTypedBuffer(cmdId, Uint8Array|ArrayBuffer) ─────
  // 입력 메모리는 이 동기 HostFunction 안에서만 빌린다. Rust는 user handler
  // 실행 전에 소유 Vec로 1회 복사하므로 JS GC/reload 뒤 입력 포인터를 보관하지
  // 않는다. handler의 출력 Vec는 postcard frame/추가 copy 없이 JSI
  // MutableBuffer에 넘기며, JS ArrayBuffer GC 시 정확한 ptr/len으로 해제한다.
  {
    auto propNameId = PropNameID::forAscii(rt, "invokeTypedBuffer");
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 2,
      [](Runtime& rt, const Value&, const Value* args, size_t count) -> Value {
        if (count != 2) {
          throw JSError(rt, "RustraJSI: invokeTypedBuffer requires (cmdId, bytes)");
        }
        uint16_t cmdId = requireU16(rt, args[0], "command id");
        auto [data, size] = extractByteBuffer(rt, args[1]);
        // 진입 시 테이블 1회 로드 — invoke/free_owned_bytes 짝과 결과
        // ArrayBuffer 의 소유 테이블이 모두 같은 코어에 묶인다.
        const CoreTable* core = core::currentCoreTable();
        uint8_t* output = nullptr;
        size_t outputSize = 0;
        uint32_t status = core->invoke_buffer(
          cmdId, data, size, &output, &outputSize);
        if (status == UINT32_MAX || output == nullptr) {
          throw JSError(rt, "RustraJSI: direct buffer ABI failed");
        }
        if (status != 0) {
          std::string error(reinterpret_cast<const char*>(output), outputSize);
          core->free_owned_bytes(output, outputSize);
          throw JSError(rt, error.empty() ? "buffer invoke failed" : error);
        }
        Value buffer = createOwnedArrayBuffer(rt, output, outputSize, core);
        return gen::decode_buffer_result_by_id(rt, cmdId, std::move(buffer));
      });
    cache_["invokeTypedBuffer"] = std::make_unique<CachedFunction>(
      CachedFunction{std::move(propNameId), std::move(hostFn)});
  }

  // invokeTyped(name, args): 정적 명령 전용 postcard fast path.
  // 흐름: encode_by_name → invoke_rkyv_v2 FFI → decode_by_name.
  // Rust 에러면 JSError throw, 성공이면 디코딩된 JS 객체 반환.
  {
    auto propNameId = PropNameID::forAscii(rt, "invokeTyped");
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 2,
      [](Runtime& rt, const Value&, const Value* args, size_t count) -> Value {
        if (count < 2) {
          throw JSError(rt, "RustraJSI: invokeTyped requires (name, args)");
        }
        std::string name = args[0].asString(rt).utf8(rt);

        // 공개 C++ 진입점(invokeTypedByName)과 동일 경로 — HostFunction 은
        // status 를 기존 예외 메시지로 그대로 옮긴다(호환 보존).
        TypedInvokeResult result = invokeTypedByName(rt, name, args[1]);
        switch (result.status) {
          case TypedInvokeStatus::Ok:
            return std::move(result.value);
          case TypedInvokeStatus::NoStaticCodec:
            throw JSError(rt, "RustraJSI: no C++ codec for '" + name + "'");
          default:
            throw JSError(rt, result.message);
        }
      });
    cache_["invokeTyped"] = std::make_unique<CachedFunction>(
      CachedFunction{std::move(propNameId), std::move(hostFn)});
  }

  // ── P0-3: invokeTypedById(cmdId, args) — id 인덱싱 typed 진입 ──
  // invokeTyped 와 동일한 흐름(encode→FFI→decode)이지만 문자열 마샬링과
  // C++ 이름 비교체인 대신 u16 cmd_id switch 디스패치를 쓴다. JS 엔진은
  // 정적 명령 집합을 엔진 생애 1회 스윕(hasStaticCodec)으로 캐시해 이 진입으로
  // 호출한다 — JSI 횡단 2→1, 문자열 2→0. 미발견 cmd_id 는 encode_by_id 가
  // false 를 반환해 JSError 로 명시 실패한다(호출侧 캐시 불변식 위반 노출).
  {
    auto propNameId = PropNameID::forAscii(rt, "invokeTypedById");
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 2,
      [](Runtime& rt, const Value&, const Value* args, size_t count) -> Value {
        if (count < 2) {
          throw JSError(rt, "RustraJSI: invokeTypedById requires (cmdId, args)");
        }
        uint16_t cmdId = requireU16(rt, args[0], "command id");

        // 공개 C++ 진입점(invokeTypedById)과 동일 경로 — status 를 기존 예외
        // 메시지로 옮긴다(호환 보존).
        TypedInvokeResult result = invokeTypedById(rt, cmdId, args[1]);
        switch (result.status) {
          case TypedInvokeStatus::Ok:
            return std::move(result.value);
          case TypedInvokeStatus::NoStaticCodec:
            throw JSError(rt, "RustraJSI: no C++ codec for cmd_id " + std::to_string(cmdId));
          default:
            throw JSError(rt, result.message);
        }
      });
    cache_["invokeTypedById"] = std::make_unique<CachedFunction>(
      CachedFunction{std::move(propNameId), std::move(hostFn)});
  }

  // ── (Tier 0) invokeTypedRaw(cmdId, ...scalarArgs) — 스칼라 직결 ──
  // postcard 왕복(인코딩~디코딩)이 전혀 없다: JS 인자를 u64 슬롯으로 조립해
  // rustra_ffi_invoke_raw 를 부르고 결과 슬롯을 그대로 JS number 로 감싼다.
  // 코어가 UINT32_MAX 를 돌려주면(비대상 명령) 특수 값 RUSTRA_RAW_FALLBACK
  // (NaN 페이로드)을 반환해 JS 엔진이 invokeTypedById 로 폴백하게 한다.
  // 에러(1)는 기존 rkyv V2 에러 와이어를 파싱해 JSError 로 정규화한다.
  {
    auto propNameId = PropNameID::forAscii(rt, "invokeTypedRaw");
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 4,
      [](Runtime& rt, const Value&, const Value* args, size_t count) -> Value {
        if (count < 1) {
          throw JSError(rt, "RustraJSI: invokeTypedRaw requires (cmdId, ...args)");
        }
        uint16_t cmdId = requireU16(rt, args[0], "command id");
        size_t slotCount = count - 1;
        // 스택 슬롯 — 아리티 최대 3 + 여유. 힙 할당 없음.
        uint64_t slots[8] = {0, 0, 0, 0, 0, 0, 0, 0};
        if (slotCount > 8) {
          throw JSError(rt, "RustraJSI: invokeTypedRaw supports up to 8 scalar args");
        }
        // 명령 스키마가 각 슬롯 종류를 결정한다. 값이 정수처럼 보이더라도 f64
        // 필드면 IEEE-754 비트로 보내야 하므로 런타임 값 휴리스틱을 쓰지 않는다.
        gen::encode_raw_slots(rt, cmdId, args + 1, slotCount, slots);
        uint64_t outSlot = 0;
        uint8_t errBuf[256];
        size_t errLen = 0;
        uint32_t code = core::currentCoreTable()->invoke_raw(
          cmdId, slots, slotCount, &outSlot, errBuf, sizeof(errBuf), &errLen);
        if (code == UINT32_MAX) {
          // 폴백 신호 — 특수 NaN 페이로드. JS 엔진은 Number.isNaN 으로 감별해
          // invokeTypedById 로 되돌린다(엔진 코드의 판별 주석 참조).
          uint64_t fallbackBits = 0x7ff8000000000001ULL;
          double fallback;
          std::memcpy(&fallback, &fallbackBits, sizeof(double));
          return Value(fallback);
        }
        if (code != 0) {
          // 에러 와이어([ok:0][pad][err_len u16 @8][postcard @10]) 파싱 —
          // typedInvokeTail 과 동일한 parseRkyvV2ErrorBody 재사용.
          if (errLen >= 10) {
            throw JSError(rt, parseRkyvV2ErrorBody(errBuf, errLen));
          }
          throw JSError(rt, "RustraJSI: invokeTypedRaw failed");
        }
        // 결과 슬롯 → 생성된 공개 output shape. 필드 종류별 비트 해석(f64,
        // bool, signed/unsigned)과 단일 프로퍼티 이름은 코드젠 메타데이터가
        // 복원한다. raw lower-bound primitive가 공개 API로 새지 않는다.
        return gen::decode_raw_result(rt, cmdId, outSlot);
      });
    cache_["invokeTypedRaw"] = std::make_unique<CachedFunction>(
      CachedFunction{std::move(propNameId), std::move(hostFn)});
  }

  // ── (Tier 1) invokeTypedPos(cmdId, a, b, …) — positional 인자 직접 진입 ──
  // JS 측 인자 객체 리터럴 {a, b} 생성과 C++ asObject/getProperty 순회를 모두
  // 건너뛴다 — HostFunction 스택의 Value 배열에서 postcard 바이트로 직렬화.
  // encode_pos_by_id 는 스칼라(≤3필드) 명령만 커버한다: 미지원 cmd_id 는
  // JSError 로 명시 실패하고 JS 엔진은 invokeTypedById 로 폴백한다.
  // argc 는 JS 코드젠(positional facade)이 시그니처로 보장하지만 런타임
  // 가드도 둔다(수동 호출 방어).
  {
    auto propNameId = PropNameID::forAscii(rt, "invokeTypedPos");
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 4,
      [](Runtime& rt, const Value&, const Value* args, size_t count) -> Value {
        if (count < 1) {
          throw JSError(rt, "RustraJSI: invokeTypedPos requires (cmdId, ...fields)");
        }
        uint16_t cmdId = requireU16(rt, args[0], "command id");
        const Value* argv = count > 1 ? args + 1 : nullptr;
        size_t argc = count > 1 ? count - 1 : 0;

        rc::Writer w;
        gen::encode_pos_by_id(rt, cmdId, argv, argc, w); // 미지원 시 throw
        return typedInvokeTail(rt, w.data(), w.size(), "", [&rt, cmdId](rc::Reader& r) {
          return gen::decode_by_id(rt, cmdId, r);
        });
      });
    cache_["invokeTypedPos"] = std::make_unique<CachedFunction>(
      CachedFunction{std::move(propNameId), std::move(hostFn)});
  }

  // ── P0-2 invokeTypedBatch: N 개 정적 명령을 단 한 번의 JSI 횡단으로 처리 ──
  // 잦은 단건 호출의 JSI 경계 비용(N-1 회 횡단 + JS 재진입)을 상쇄 → jank 완화.
  // 흐름: names/args 배열을 받아 C++ 루프에서 encode→FFI→decode → 결과 JS Array 1회 반환.
  // 모든 항목이 정적 코덱이어야 함(JS 가 hasStaticCodec 으로 사전 검증). 첫 에러에서 throw.
  {
    auto propNameId = PropNameID::forAscii(rt, "invokeTypedBatch");
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 2,
      [](Runtime& rt, const Value&, const Value* args, size_t count) -> Value {
        if (count < 2) {
          throw JSError(rt, "RustraJSI: invokeTypedBatch requires (names, args)");
        }
        Array names = args[0].asObject(rt).getArray(rt);
        Array inputs = args[1].asObject(rt).getArray(rt);
        size_t n = names.length(rt);
        if (inputs.length(rt) != n) {
          throw JSError(rt, "RustraJSI: invokeTypedBatch names/args length mismatch");
        }

        Array results(rt, n);
        for (size_t i = 0; i < n; i++) {
          std::string name = names.getValueAtIndex(rt, i).asString(rt).utf8(rt);
          const Value& oneArgs = inputs.getValueAtIndex(rt, i);

          // encode (정적 명령 필수)
          rc::Writer w;
          if (!gen::encode_by_name(rt, name, oneArgs, w)) {
            throw JSError(rt, "RustraJSI: batch item has no C++ codec for '" + name + "'");
          }
          // FFI + 응답 tail — 공통 헬퍼 (fail-fast: 첫 에러에서 throw).
          // 접미 계약 유지: null → " (batch item <name>)", malformed → " (batch)".
          Value decoded = typedInvokeTail(rt, w.data(), w.size(), " (batch)",
                                          [&rt, &name](rc::Reader& r) {
                                            return gen::decode_by_name(rt, name, r);
                                          },
                                          &name);
          results.setValueAtIndex(rt, i, decoded);
        }
        return results;
      });
    cache_["invokeTypedBatch"] = std::make_unique<CachedFunction>(
      CachedFunction{std::move(propNameId), std::move(hostFn)});
  }

  // ── P0-2 byId: invokeTypedBatchById(cmdIds, args) — id 인덱싱 배치 진입 ──
  // invokeTypedBatch 와 동일 계약(단일 횡단, fail-fast, 결과 Array 순서 보존)이지만
  // encode/decode 를 이름 대신 u16 cmd_id 로 디스패치한다 — 항목당 문자열
  // 마샬링 2회(name 인자 + 응답 decode_by_name)를 제거한다. JS 엔진은 정적
  // 명령 id 캐시(P0-3 ensureStaticIds)에서 id 배열을 조립해 이 진입으로 호출한다.
  // 미발견 cmd_id 는 encode_by_id 가 false 를 반환해 JSError 로 명시 실패한다.
  {
    auto propNameId = PropNameID::forAscii(rt, "invokeTypedBatchById");
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 2,
      [](Runtime& rt, const Value&, const Value* args, size_t count) -> Value {
        if (count < 2) {
          throw JSError(rt, "RustraJSI: invokeTypedBatchById requires (cmdIds, args)");
        }
        Array ids = args[0].asObject(rt).getArray(rt);
        Array inputs = args[1].asObject(rt).getArray(rt);
        size_t n = ids.length(rt);
        if (inputs.length(rt) != n) {
          throw JSError(rt, "RustraJSI: invokeTypedBatchById cmdIds/args length mismatch");
        }

        Array results(rt, n);
        for (size_t i = 0; i < n; i++) {
          uint16_t cmdId = requireU16(rt, ids.getValueAtIndex(rt, i), "batch command id");
          const Value& oneArgs = inputs.getValueAtIndex(rt, i);

          // encode by id (정적 cmd_id 필수)
          rc::Writer w;
          if (!gen::encode_by_id(rt, cmdId, oneArgs, w)) {
            throw JSError(rt,
              "RustraJSI: batch item has no C++ codec for cmd_id " + std::to_string(cmdId));
          }
          // FFI + 응답 tail — 공통 헬퍼 (fail-fast: 첫 에러에서 throw).
          // 접미는 이름 기반 배치와 동일하게 유지한다: null → " (batch)",
          // malformed → " (batch)". 항목 이름을 알 수 없는 byId 경로의
          // null 접미는 이름 조립 없이 배치 접미를 그대로 쓴다.
          Value decoded = typedInvokeTail(rt, w.data(), w.size(), " (batch)",
                                          [&rt, cmdId](rc::Reader& r) {
                                            return gen::decode_by_id(rt, cmdId, r);
                                          });
          results.setValueAtIndex(rt, i, decoded);
        }
        return results;
      });
    cache_["invokeTypedBatchById"] = std::make_unique<CachedFunction>(
      CachedFunction{std::move(propNameId), std::move(hostFn)});
  }

  // ── follow-up 3: invokeTypedAsync + invokeCancel ────────────
  // invokeTypedAsync(name, args, onSuccess, onError) → invocation id (number).
  // 결과는 CallInvoker 로 JS 스레드에 마샬링된다. id 로 invokeCancel(id) 호출 시
  // Rust 취소 체크포인트(워커 dispatch 전)까지 전파된다. 구형 계약(void 반환)
  // 호환은 JS 어댑터가 처리한다.

  {
    auto propNameId = PropNameID::forAscii(rt, "invokeTypedAsync");
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 4,
      [](Runtime& rt, const Value&, const Value* args, size_t count) -> Value {
        if (count != 4) {
          throw JSError(rt, "RustraJSI: invokeTypedAsync requires (name, args, onSuccess, onError)");
        }
        std::string name = args[0].asString(rt).utf8(rt);
        if (!args[2].isObject() || !args[2].asObject(rt).isFunction(rt) ||
            !args[3].isObject() || !args[3].asObject(rt).isFunction(rt)) {
          throw JSError(rt, "RustraJSI: invokeTypedAsync callbacks must be functions");
        }

        // CallInvoker — EventDispatcher 의 전역 디스패처에서 빌려온다.
        std::shared_ptr<void> invoker = getEventDispatcher()->currentCallInvoker();

        if (!invoker) {
          throw JSError(rt,
            "RustraJSI: invokeTypedAsync requires a CallInvoker — "
            "install via installRustraJSIWithInvoker");
        }

        // 1) JS 객체 → postcard 요청 바이트 (invokeTyped 와 동일한 인코딩).
        rc::Writer w;
        if (!gen::encode_by_name(rt, name, args[1], w)) {
          throw JSError(rt, "RustraJSI: no C++ codec for '" + name + "'");
        }
        auto req = w.take();

        auto ctx = std::make_shared<AsyncCallContext>();
        ctx->commandName = name;
        ctx->onSuccess.emplace(args[2].asObject(rt).getFunction(rt));
        ctx->onError.emplace(args[3].asObject(rt).getFunction(rt));
        ctx->callInvoker = std::move(invoker);
        ctx->generation = g_runtimeGeneration.load(std::memory_order_acquire);
        registerAsyncContext(ctx);
        // C ABI user_data는 shared_ptr holder를 소유한다. 동기 오류 콜백과
        // install/invalidate 경합에서도 context 수명이 보장된다. 응답 버퍼는
        // context 안에 살아 있다 — 콜백(ctx 해제 경합 포함)이 끝날 때까지
        // holder/shared_ptr 체인이 수명을 보장하므로 Rust 워커가 안전히 쓴다.
        auto* holder = new std::shared_ptr<AsyncCallContext>(ctx);

        // 2) 비동기 FFI — id 를 동기 반환한다 (취소 핸들). 테이블은 이
        // dispatch 의 생산 코어로 고정된다(콜백 free 짝이 같은 코어).
        const CoreTable* dispatchCore = core::currentCoreTable();
        ctx->dispatchCore = dispatchCore;
        uint64_t invocationId = 0;
        dispatchCore->invoke_rkyv_v2_async_into(
          req.data(), req.size(),
          ctx->frameBuffer.data(), ctx->frameBuffer.size(),
          holder,
          [](void* user_data, uint8_t* resp, size_t resp_len, uint8_t owned) {
            // 워커 스레드 또는(즉시 실패 시) 호출 스레드에서 실행 — JS 객체를
            // 건드리지 않고, 결과를 소유한 뒤 CallInvoker 로 JS 스레드에
            // 마샬링한다.
            std::unique_ptr<std::shared_ptr<AsyncCallContext>> holder(
              static_cast<std::shared_ptr<AsyncCallContext>*>(user_data));
            std::shared_ptr<AsyncCallContext> ctx = *holder;
            // 응답 소유 규칙(FFI 계약):
            //   owned=0 — resp 는 ctx->frameBuffer 자체. ctx(shared_ptr)가
            //             JS 스레드 람다까지 살아 있어 그대로 제자리 읽는다.
            //             복사 0회 — 기존 std::vector assign 제거 지점.
            //   owned=1 — resp 는 Rust heap 프레임. shared_ptr 로 소유권을
            //             감싸 전달한다 — CallInvoker 가 reload/teardown 시
            //             큐잉된 람다를 실행 없이 파괴해도 deleter 가 free 를
            //             보장한다(구형 std::vector 경로의 누수 없음 특성 유지).
            //             deleter 는 정확한 (ptr, resp_len) 짝으로 free 한다 —
            //             debug free_guard 가 len 불일치 free 에 abort 하므로
            //             길이를 버리는 deleter 는 쓸 수 없다. free 대상 코어는
            //             dispatch 시점 테이블(dispatchCore)로 고정 — 스왑이
            //             콜백을 추월해도 다른 코어의 free_guard/allocator 에
            //             건네지지 않는다.
            const CoreTable* dispatchCore = ctx->dispatchCore;
            std::shared_ptr<uint8_t> ownedFrame;
            if (owned == 1) {
              ownedFrame = std::shared_ptr<uint8_t>(
                resp, [resp_len, dispatchCore](uint8_t* p) { dispatchCore->free(p, resp_len); });
            }
            std::shared_ptr<void> invoker;
            bool valid = false;
            {
              std::lock_guard<std::mutex> lock(ctx->mutex);
              invoker = ctx->callInvoker;
              valid = ctx->valid &&
                ctx->generation == g_runtimeGeneration.load(std::memory_order_acquire);
            }
            if (!valid || !invoker) {
              // invalidate가 JSI Function을 이미 JS thread에서 reset함 —
              // 전달은 폐기. owned 프레임은 ownedFrame 의 소멸이 free 한다.
              unregisterAsyncContext(ctx);
              return;
            }
            auto* nativeInvoker =
              static_cast<facebook::react::CallInvoker*>(invoker.get());
            nativeInvoker->invokeAsync(
              [ctx, resp, resp_len, owned, ownedFrame](facebook::jsi::Runtime& rt) {
              std::optional<facebook::jsi::Function> onSuccess;
              std::optional<facebook::jsi::Function> onError;
              std::string name;
              bool deliver = false;
              {
                std::lock_guard<std::mutex> lock(ctx->mutex);
                if (ctx->valid &&
                    ctx->generation == g_runtimeGeneration.load(std::memory_order_acquire)) {
                  ctx->valid = false;
                  name = ctx->commandName;
                  onSuccess = std::move(ctx->onSuccess);
                  onError = std::move(ctx->onError);
                  ctx->callInvoker.reset();
                  deliver = true;
                }
              }
                unregisterAsyncContext(ctx);
                // owned=1 프레임의 해제는 캡처한 ownedFrame 의 소멸이 담당
                // 한다 — 아래 모든 exit 경로(deliver 경합/empty/malformed/
                // 에러/디코드 실패)와 람다가 실행되지 않고 파괴되는 teardown
                // 경로에서도 정확히 1회 free. owned=0 이면 빈 shared_ptr —
                // resp 는 ctx->frameBuffer(ctx 가 수명 보장).
                if (!deliver || !onSuccess || !onError) return;
                const size_t out_len = resp_len;
                if (out_len < 1) {
                  onError->call(rt, "RustraJSI: empty rkyv v2 async response");
                  return;
                }
                if (resp[0] == 0) {
                  // 에러 와이어: [ok:0][pad][err_len u16 @8][postcard{code,message} @10]
                  if (out_len < 10) {
                    onError->call(rt, "RustraJSI: malformed async error response");
                    return;
                  }
                  // postcard {code, message} → "code: message" 문자열 (RustraError
                  // Display 형태) — JS parseRustraErrorString 가 코드를 복구한다.
                  // 파싱 실패 시 원시 바이트 폴백(onError 누락 없음).
                  onError->call(rt, parseRkyvV2ErrorBody(resp, out_len));
                  return;
                }
                if (out_len < 8) {
                  onError->call(rt, "RustraJSI: malformed async success response");
                  return;
                }
                try {
                  rc::Reader r(resp + 8, out_len - 8);
                  Value result = gen::decode_by_name(rt, name, r);
                  onSuccess->call(rt, std::move(result));
                } catch (const facebook::jsi::JSError& e) {
                  // 디코딩 실패는 에러 콜백으로 정규화 — 콜백 누락 방지.
                  onError->call(rt, e.getMessage());
                } catch (const std::exception& e) {
                  // 생성 디코더와 read_uvar 가드는 std::runtime_error 를 던진다 —
                  // JSError 만 받으면 async 콜백 스레드에서 미포착 → terminate.
                  onError->call(rt, e.what());
                }
              });
          },
          &invocationId);

        {
          std::lock_guard<std::mutex> lock(ctx->mutex);
          ctx->invocationId = invocationId;
        }

        // JS 는 동기적으로 id 를 받는다 — abort 전파에 쓸 취소 핸들.
        return Value(static_cast<double>(invocationId));
      });
    cache_["invokeTypedAsync"] = std::make_unique<CachedFunction>(
      CachedFunction{std::move(propNameId), std::move(hostFn)});
  }

  // ── G2: invokeTypedAsyncById(cmdId, args, onSuccess, onError) ──
  // invokeTypedAsync 의 id 인덱싱 변형 — 이름 문자열 마샬링(std::string name
  // utf8 변환 + encode_by_name 의 name→codec 해시)을 cmdId 직접 인덱싱으로
  // 대체한다. 완료 콜백의 std::string name 복사도 제거 — 컨텍스트에
  // uint16_t commandId 만 실려 간다. 에러/디코드 진단은 id 기반 조립.
  {
    auto propNameId = PropNameID::forAscii(rt, "invokeTypedAsyncById");
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 4,
      [](Runtime& rt, const Value&, const Value* args, size_t count) -> Value {
        if (count != 4) {
          throw JSError(rt, "RustraJSI: invokeTypedAsyncById requires (cmdId, args, onSuccess, onError)");
        }
        if (!args[0].isNumber()) {
          throw JSError(rt, "RustraJSI: invokeTypedAsyncById cmdId must be a number");
        }
        if (!args[2].isObject() || !args[2].asObject(rt).isFunction(rt) ||
            !args[3].isObject() || !args[3].asObject(rt).isFunction(rt)) {
          throw JSError(rt, "RustraJSI: invokeTypedAsyncById callbacks must be functions");
        }
        const auto cmdId = static_cast<uint16_t>(args[0].asNumber());

        std::shared_ptr<void> invoker = getEventDispatcher()->currentCallInvoker();
        if (!invoker) {
          throw JSError(rt,
            "RustraJSI: invokeTypedAsyncById requires a CallInvoker — "
            "install via installRustraJSIWithInvoker");
        }

        // 1) JS 객체 → postcard 요청 바이트 (encode_by_id — 해시 없이 직접 인덱싱).
        rc::Writer w;
        if (!gen::encode_by_id(rt, cmdId, args[1], w)) {
          throw JSError(rt, "RustraJSI: no C++ codec for cmd id " + std::to_string(cmdId));
        }
        auto req = w.take();

        auto ctx = std::make_shared<AsyncCallContext>();
        ctx->hasCommandId = true;
        ctx->commandId = cmdId;
        ctx->onSuccess.emplace(args[2].asObject(rt).getFunction(rt));
        ctx->onError.emplace(args[3].asObject(rt).getFunction(rt));
        ctx->callInvoker = std::move(invoker);
        ctx->generation = g_runtimeGeneration.load(std::memory_order_acquire);
        registerAsyncContext(ctx);
        auto* holder = new std::shared_ptr<AsyncCallContext>(ctx);

        // 2) 비동기 FFI — invokeTypedAsync 와 동일한 엔트리, 동일한 응답 규약.
        // dispatch 시점 테이블로 생산 코어를 고정한다(owned 프레임 free 짝).
        const CoreTable* dispatchCore = core::currentCoreTable();
        ctx->dispatchCore = dispatchCore;
        uint64_t invocationId = 0;
        dispatchCore->invoke_rkyv_v2_async_into(
          req.data(), req.size(),
          ctx->frameBuffer.data(), ctx->frameBuffer.size(),
          holder,
          [](void* user_data, uint8_t* resp, size_t resp_len, uint8_t owned) {
            std::unique_ptr<std::shared_ptr<AsyncCallContext>> holder(
              static_cast<std::shared_ptr<AsyncCallContext>*>(user_data));
            std::shared_ptr<AsyncCallContext> ctx = *holder;
            const CoreTable* dispatchCore = ctx->dispatchCore;
            std::shared_ptr<uint8_t> ownedFrame;
            if (owned == 1) {
              ownedFrame = std::shared_ptr<uint8_t>(
                resp, [resp_len, dispatchCore](uint8_t* p) { dispatchCore->free(p, resp_len); });
            }
            std::shared_ptr<void> invoker;
            bool valid = false;
            {
              std::lock_guard<std::mutex> lock(ctx->mutex);
              invoker = ctx->callInvoker;
              valid = ctx->valid &&
                ctx->generation == g_runtimeGeneration.load(std::memory_order_acquire);
            }
            if (!valid || !invoker) {
              unregisterAsyncContext(ctx);
              return;
            }
            auto* nativeInvoker =
              static_cast<facebook::react::CallInvoker*>(invoker.get());
            nativeInvoker->invokeAsync(
              [ctx, resp, resp_len, owned, ownedFrame](facebook::jsi::Runtime& rt) {
              std::optional<facebook::jsi::Function> onSuccess;
              std::optional<facebook::jsi::Function> onError;
              uint16_t commandId = 0;
              bool deliver = false;
              {
                std::lock_guard<std::mutex> lock(ctx->mutex);
                if (ctx->valid &&
                    ctx->generation == g_runtimeGeneration.load(std::memory_order_acquire)) {
                  ctx->valid = false;
                  commandId = ctx->commandId;
                  onSuccess = std::move(ctx->onSuccess);
                  onError = std::move(ctx->onError);
                  ctx->callInvoker.reset();
                  deliver = true;
                }
              }
                unregisterAsyncContext(ctx);
                if (!deliver || !onSuccess || !onError) return;
                const size_t out_len = resp_len;
                if (out_len < 1) {
                  onError->call(rt, "RustraJSI: empty rkyv v2 async response");
                  return;
                }
                if (resp[0] == 0) {
                  if (out_len < 10) {
                    onError->call(rt, "RustraJSI: malformed async error response");
                    return;
                  }
                  onError->call(rt, parseRkyvV2ErrorBody(resp, out_len));
                  return;
                }
                if (out_len < 8) {
                  onError->call(rt, "RustraJSI: malformed async success response");
                  return;
                }
                try {
                  rc::Reader r(resp + 8, out_len - 8);
                  Value result = gen::decode_by_id(rt, commandId, r);
                  onSuccess->call(rt, std::move(result));
                } catch (const facebook::jsi::JSError& e) {
                  onError->call(rt, e.getMessage());
                } catch (const std::exception& e) {
                  onError->call(rt, e.what());
                }
              });
          },
          &invocationId);

        {
          std::lock_guard<std::mutex> lock(ctx->mutex);
          ctx->invocationId = invocationId;
        }

        return Value(static_cast<double>(invocationId));
      });
    cache_["invokeTypedAsyncById"] = std::make_unique<CachedFunction>(
      CachedFunction{std::move(propNameId), std::move(hostFn)});
  }

  // invokeCancel(id) → boolean — 진행 중 async 호출의 협력적 취소.
  {
    auto propNameId = PropNameID::forAscii(rt, "invokeCancel");
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 1,
      [](Runtime& rt, const Value&, const Value* args, size_t count) -> Value {
        if (count < 1) {
          throw JSError(rt, "RustraJSI: invokeCancel requires (invocationId)");
        }
        uint64_t id = requireSafeU64(rt, args[0], "invocation id");
        return Value(core::currentCoreTable()->invoke_cancel(id));
      });
    cache_["invokeCancel"] = std::make_unique<CachedFunction>(
      CachedFunction{std::move(propNameId), std::move(hostFn)});
  }

  // ── hotCoreStatus(): dev 핫코어 상태 관측 ───────────────────
  // 정적 모드(비활성)는 null. 핫 모드는 마지막 스왑의 구/신 계약 해시와
  // 오류를 실는다 — stderr 스왑 로그의 JS 콘솔 관측 표면.
  {
    auto propNameId = PropNameID::forAscii(rt, "hotCoreStatus");
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 0,
      [](Runtime& rt, const Value&, const Value*, size_t) -> Value {
        core::HotCoreStatus status = core::hotCoreStatusSnapshot();
        if (!status.enabled) {
          return Value::null();
        }
        Object result(rt);
        result.setProperty(rt, "enabled", Value(true));
        result.setProperty(rt, "swapped", Value(status.swapped));
        auto setString = [&rt, &result](const char* key, const std::string& value) {
          result.setProperty(rt, key, String::createFromUtf8(
            rt, reinterpret_cast<const uint8_t*>(value.data()), value.size()));
        };
        setString("oldHash", status.oldHash);
        setString("newHash", status.newHash);
        setString("error", status.error);
        return result;
      });
    cache_["hotCoreStatus"] = std::make_unique<CachedFunction>(
      CachedFunction{std::move(propNameId), std::move(hostFn)});
  }
}

Value RustraHostObject::get(Runtime& rt, const PropNameID& name) {
  // Fast path: compare PropNameID against cached entries.
  // This avoids string allocation from name.utf8(rt).
  for (auto& [key, cached] : cache_) {
    if (PropNameID::compare(rt, name, cached->propNameId)) {
      return Value(rt, cached->function);
    }
  }
  return Value::undefined();
}

std::vector<PropNameID> RustraHostObject::getPropertyNames(Runtime& rt) {
  std::vector<PropNameID> names;
  names.reserve(cache_.size());
  for (auto& [key, cached] : cache_) {
    names.push_back(PropNameID::forUtf8(rt, key));
  }
  return names;
}

std::vector<PropNameID> RustraHostObject::propertyNames(Runtime& rt) {
  // getPropertyNames 와 동일 로직의 non-virtual 버전 — 설치 평탄화 전용.
  return getPropertyNames(rt);
}

Function RustraHostObject::getFunction(Runtime& rt, const PropNameID& name) {
  // get 과 동일 스캔이지만 Value 가 아닌 Function 반환. 이 jsi 버전의
  // Function 은 move-only(Pointer 계열, 복사 생성자 삭제)라 Object::getFunction
  // 의 const& 오버로드가 runtime.cloneObject 로 새 핸들을 만들어 준다 —
  // 같은 Runtime 힙의 동일 JS 함수 객체를 참조하므로 사실상의 복사다.
  for (auto& [key, cached] : cache_) {
    if (PropNameID::compare(rt, name, cached->propNameId)) {
      return cached->function.getFunction(rt);
    }
  }
  // 설치 경로는 propertyNames 로 얻은 이름만 전달하므로 도달하지 않는다.
  // 그래도 침묵 대신 명시적 실패로 — 평탄화 누락을 조기에 노출한다.
  throw JSError(rt, "RustraJSI: getFunction — unknown property");
}

// ── Install ────────────────────────────────────────────────

void installRustraJSIWithInvoker(Runtime& rt,
                                  std::shared_ptr<void> typeErasedCallInvoker) {
  // 같은 프로세스의 Fast Refresh/bridge reload에서 구 Runtime 소유 async
  // callback을 먼저 무효화한다. 플랫폼 invalidate가 호출되지 않은 호스트도
  // 다음 install을 안전망으로 사용한다.
  invalidateRustraJSI();
  // 현재(정적 또는 핫스왑된) 코어의 패키지 등록 — 핫 모드에서 신 코어는
  // 자체 mobile_init 을 스왑 경로에서 이미 호출했다(모듈별 컨텍스트).
  core::currentCoreTable()->mobile_init();
  auto dispatcher = getEventDispatcher();
  dispatcher->setCallInvoker(typeErasedCallInvoker);
  // 채널 디스패처도 동일 CallInvoker 공유(2단계) — reset 내부에서 이전
  // 런타임 귀속 채널을 Rust 쪽까지 폐기한다.
  getChannelDispatcher()->setCallInvoker(std::move(typeErasedCallInvoker));

  // 평탄화(Nitro 방식): 모든 호스트 함수를 일반 JS 객체의 프로퍼티로 설치
  // 시점에 박는다. 이후 native.invokeRkyvV2(...) 조회가 HostObject get 콜백
  // (엔트리당 compare 가상 호출, 최대 22회) 대신 엔진의 인라인 프로퍼티
  // 로드가 된다. 동작 불변: 프로퍼티 목록과 각 함수는 기존과 동일하며,
  // unknown 프로퍼티 조회는 HostObject 의 undefined 반환과 마찬가지로
  // JS undefined 로 귀결된다.
  // RustraHostObject 는 함수 팩토리로만 사용 — get/getPropertyNames
  // 오버라이드는 다른 설치 경로 호환용 안전 폴백으로 유지된다.
  auto hostObject = std::make_shared<RustraHostObject>(rt);
  Object obj(rt);
  for (auto& propName : hostObject->propertyNames(rt)) {
    // setProperty(PropNameID, T&&) 오버로드 — Function 은 Object 파생이라
    // detail::toValue 가 Value(rt, function) 로 변환한다.
    obj.setProperty(rt, propName, hostObject->getFunction(rt, propName));
  }
  rt.global().setProperty(rt, "__rustraNative", std::move(obj));
}

void installRustraJSI(Runtime& rt) {
  // CallInvoker 없는 설치(레거시 경로) — 이벤트 푸시는 JS 가 drainEvents() 로
  // 폴링해야 한다. 프로덕션 플랫폼 글루는 installRustraJSIWithInvoker 사용.
  installRustraJSIWithInvoker(rt, nullptr);
}

// ── dylib 핫스왑 dev 코어 (Phase 3) — 폴링/스왑 구현 ─────────────────
// 설계: docs/plans/2026-09-09-native-hot-core-design.md. CLI 발행 계약은
// `<stem>-hot-live<ext>`(게이트 통과 빌드만 temp+rename 원자 발행)이고, 폴링은
// 그 파일의 sha256 변화만 본다(notify 같은 감시 의존 없음 — Rust watch 와
// 동일 정책). 안전 규약:
// - 구 핸들은 절대 dlclose 하지 않고 leak 한다 — macOS/iOS 는 std TLS 때문에
//   언로드 자체가 불가능하고(libloading #59, dyld man 3), 구 심볼의
//   use-after-unload 를 원천 차단한다. 발행된 테이블 객체도 같은 수명으로
//   leak 한다(스왑별 소량 누수는 dev 감수).
// - 실패한 스왑은 lastHash 를 갱신하지 않는다(Rust watch 와 동일 — 빌드 중
//   반쯤 쓰인 아티팩트가 다음 폴링에서 재시도되고, 게이트 reject 로 라이브
//   파일이 구 바이트로 남으면 구 코어가 유지된다: fail-closed). 단 같은
//   바이트의 연속 실패가 상한(kMaxSwapFailuresPerBytes)에 도달하면 그
//   바이트를 포이즌해 바이트가 바뀔 때까지 재시도하지 않는다 — 매 폴링마다
//   카피+dlopen 이 반복되는 실패 폭주 방지(Rust watch 와 동일 정책).
// - 오류 표면: 아래 함수들은 어떤 입력/실패에서도 예외를 밖으로 던지지 않고
//   프로세스를 죽이지 않는다 — 폴링 스레드의 모든 실패는 음수 반환/stderr
//   로그/상태 스냅샷의 error 필드로 흡수된다.
#if defined(__APPLE__) || defined(__ANDROID__)

namespace core {
namespace {

/// 상태 — pollMutex 는 스왑 시도(dlopen 포함 수백 ms)를 직렬화하고,
/// statusMutex 는 JS 가 읽는 스냅샷 필드만 짧게 보호한다(스왑 중 상태 조회가
/// 다른 스레드를 오래 막지 않게 둘로 분리).
struct HotCoreState {
  std::mutex pollMutex;
  std::mutex statusMutex;
  std::string dir;       // 빈 문자열 = 정적 모드(기본)
  std::string lastHash;  // 성공 기준선(라이브 파일 바이트 sha256 hex)
  uint64_t counter = 0;  // 버전 카피 카운터(단조) — pollMutex 배타 구간 전용
  bool swapped = false;
  std::string oldHash;
  std::string newHash;
  std::string error;
  // 실패 재시도 상한(폭주 방지) — streak 는 바이트(해시) 단위로 집계된다.
  std::string streakHash;    // 실패 streak 집계 중인 바이트 상태
  uint32_t failureStreak = 0; // streakHash 바이트의 연속 실패 횟수
  std::string poisonHash;    // 상한 실패로 재시도 중단된 바이트 — 변화까지 대기
};

HotCoreState& hotState() {
  static HotCoreState state;
  return state;
}

std::atomic<bool> g_hotCorePollingStarted{false};

void recordHotCoreError(const std::string& message) {
  std::lock_guard<std::mutex> lock(hotState().statusMutex);
  hotState().error = message;
  fprintf(stderr, "[rustra] hot-core: %s\n", message.c_str());
}

/// 같은 바이트 연속 스왑 실패 허용치 — Rust watch(FailureTracker)와 동일 값·정책.
constexpr uint32_t kMaxSwapFailuresPerBytes = 5;

std::string hashToShort8(const std::string& hash) {
  return hash.size() > 8 ? hash.substr(0, 8) : hash;
}

/// 스왑 실패 기록 — 같은 바이트의 연속 실패를 세고 상한 도달 시 그 바이트를
/// 포이즌해 바이트가 바뀔 때까지 재시도를 멈춘다. 열리지 않는 아티팩트가
/// 폴링 주기(300ms)마다 카피+dlopen 재시도와 오류 로그를 무한 반복하는
/// 폭주를 끊는다. 바이트가 바뀌면 streak 은 새 바이트 기준으로 다시 센다.
void noteSwapFailure(const std::string& hash) {
  if (hash.empty()) return;
  bool poisoned = false;
  {
    auto& state = hotState();
    std::lock_guard<std::mutex> lock(state.statusMutex);
    if (state.streakHash != hash) {
      state.streakHash = hash;
      state.failureStreak = 0;
    }
    ++state.failureStreak;
    if (state.failureStreak >= kMaxSwapFailuresPerBytes) {
      state.failureStreak = 0;
      state.poisonHash = hash;
      poisoned = true;
    }
  }
  if (poisoned) {
    recordHotCoreError("giving up on artifact bytes " + hashToShort8(hash) +
                       " after " + std::to_string(kMaxSwapFailuresPerBytes) +
                       " failed swaps — retrying when new bytes are published");
  }
}

std::string joinHotPath(const std::string& dir, const std::string& name) {
  if (!dir.empty() && dir.back() == '/') return dir + name;
  return dir + "/" + name;
}

/// CLI 발행 계약(`<stem>-hot-live<ext>`) 단일 라이브 파일 스캔. 없으면 빈
/// 문자열(게이트 reject 또는 미빌드 — 구 코어 유지 신호).
std::string findLiveArtifact(const std::string& dir) {
  DIR* dirStream = opendir(dir.c_str());
  if (dirStream == nullptr) return std::string();
  std::string found;
  while (dirent* entry = readdir(dirStream)) {
    const std::string name = entry->d_name;
    if (name.find("-hot-live.") == std::string::npos) continue;
    found = name;
    break; // 발행 계약상 단일 파일 — 첫 매치면 충분하다.
  }
  closedir(dirStream);
  return found;
}

/// sha256 (FIPS 180-4) — 의존 추가 금지 계약에 따른 자체 구현(파일 바이트
/// 해시 하나가 목적이라 최소만 만든다).
class Sha256 {
public:
  void update(const uint8_t* data, size_t len) {
    bitLen_ += static_cast<uint64_t>(len) * 8;
    absorb(data, len);
  }

  std::string finishHex() {
    // 패딩은 비트 길이 추적 없이(absorb) 처리 — update 의 bitLen_ 오염 방지.
    const uint64_t messageBits = bitLen_;
    const uint8_t padByte = 0x80;
    const uint8_t zero = 0;
    absorb(&padByte, 1);
    while (bufferLen_ != 56) absorb(&zero, 1);
    uint8_t lengthBytes[8];
    for (int i = 0; i < 8; ++i) {
      lengthBytes[i] = static_cast<uint8_t>(messageBits >> (56 - 8 * i));
    }
    absorb(lengthBytes, sizeof(lengthBytes));
    static const char kHex[] = "0123456789abcdef";
    char out[2 * 32];
    // digest_ 는 8개 u32 워드 — 워드당 빅엔디안 8 hex 문자, 기점 8*i.
    for (int i = 0; i < 8; ++i) {
      out[8 * i] = kHex[(digest_[i] >> 28) & 0xf];
      out[8 * i + 1] = kHex[(digest_[i] >> 24) & 0xf];
      out[8 * i + 2] = kHex[(digest_[i] >> 20) & 0xf];
      out[8 * i + 3] = kHex[(digest_[i] >> 16) & 0xf];
      out[8 * i + 4] = kHex[(digest_[i] >> 12) & 0xf];
      out[8 * i + 5] = kHex[(digest_[i] >> 8) & 0xf];
      out[8 * i + 6] = kHex[(digest_[i] >> 4) & 0xf];
      out[8 * i + 7] = kHex[digest_[i] & 0xf];
    }
    return std::string(out, sizeof(out));
  }

private:
  void absorb(const uint8_t* data, size_t len) {
    while (len > 0) {
      size_t take = 64 - bufferLen_;
      if (take > len) take = len;
      std::memcpy(buffer_ + bufferLen_, data, take);
      bufferLen_ += take;
      data += take;
      len -= take;
      if (bufferLen_ == 64) {
        processBlock(buffer_);
        bufferLen_ = 0;
      }
    }
  }

  static uint32_t rotr(uint32_t x, int n) { return (x >> n) | (x << (32 - n)); }

  void processBlock(const uint8_t* block) {
    static const uint32_t k[64] = {
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1,
        0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
        0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
        0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
        0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2};
    uint32_t w[64];
    for (int i = 0; i < 16; ++i) {
      w[i] = (static_cast<uint32_t>(block[4 * i]) << 24) |
             (static_cast<uint32_t>(block[4 * i + 1]) << 16) |
             (static_cast<uint32_t>(block[4 * i + 2]) << 8) |
             static_cast<uint32_t>(block[4 * i + 3]);
    }
    for (int i = 16; i < 64; ++i) {
      uint32_t s0 = rotr(w[i - 15], 7) ^ rotr(w[i - 15], 18) ^ (w[i - 15] >> 3);
      uint32_t s1 = rotr(w[i - 2], 17) ^ rotr(w[i - 2], 19) ^ (w[i - 2] >> 10);
      w[i] = w[i - 16] + s0 + w[i - 7] + s1;
    }
    uint32_t a = digest_[0], b = digest_[1], c = digest_[2], d = digest_[3];
    uint32_t e = digest_[4], f = digest_[5], g = digest_[6], h = digest_[7];
    for (int i = 0; i < 64; ++i) {
      uint32_t s1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
      uint32_t ch = (e & f) ^ (~e & g);
      uint32_t t1 = h + s1 + ch + k[i] + w[i];
      uint32_t s0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
      uint32_t maj = (a & b) ^ (a & c) ^ (b & c);
      uint32_t t2 = s0 + maj;
      h = g; g = f; f = e; e = d + t1;
      d = c; c = b; b = a; a = t1 + t2;
    }
    digest_[0] += a; digest_[1] += b; digest_[2] += c; digest_[3] += d;
    digest_[4] += e; digest_[5] += f; digest_[6] += g; digest_[7] += h;
  }

  uint32_t digest_[8] = {0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
                         0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19};
  uint8_t buffer_[64];
  size_t bufferLen_ = 0;
  uint64_t bitLen_ = 0;
};

/// 파일 바이트 sha256 hex — 읽기 실패는 빈 문자열(다음 폴링 재시도 신호).
std::string sha256FileHex(const std::string& path) {
  FILE* file = fopen(path.c_str(), "rb");
  if (file == nullptr) return std::string();
  Sha256 sha;
  uint8_t buf[65536];
  size_t n;
  while ((n = fread(buf, 1, sizeof(buf), file)) > 0) {
    sha.update(buf, n);
  }
  const bool ok = ferror(file) == 0;
  fclose(file);
  if (!ok) return std::string();
  return sha.finishHex();
}

/// 버전 카피 생성(고유 경로가 목적이라 직접 기록 — 부분 파일은 dlopen 실패로
/// 걸러지고 다음 시도는 새 카운터의 새 경로를 쓴다). 실패 시 부분 파일 제거.
bool copyFileContents(const std::string& from, const std::string& to) {
  FILE* in = fopen(from.c_str(), "rb");
  if (in == nullptr) return false;
  FILE* out = fopen(to.c_str(), "wb");
  if (out == nullptr) {
    fclose(in);
    return false;
  }
  char buf[65536];
  size_t n;
  bool ok = true;
  while ((n = fread(buf, 1, sizeof(buf), in)) > 0) {
    if (fwrite(buf, 1, n, out) != n) {
      ok = false;
      break;
    }
  }
  if (ferror(in) != 0) ok = false;
  fclose(in);
  if (fclose(out) != 0) ok = false;
  if (!ok) remove(to.c_str());
  return ok;
}

/// 계약 해시 응답(ASCII 텍스트)을 같은 테이블의 free 로 짝지어 읽는다.
std::string readContractHash(const CoreTable* table) {
  size_t len = 0;
  uint8_t* data = table->contract_hash(&len);
  if (data == nullptr) return std::string();
  std::string hash(reinterpret_cast<const char*>(data), len);
  table->free(data, len);
  return hash;
}

} // namespace

void configureHotCore(const char* hotDirPath) {
  auto& state = hotState();
  std::lock_guard<std::mutex> lock(state.statusMutex);
  // 설치 경로에서 1회 호출이 계약 — 마지막 호출이 이긴다. 빈/null 문자열은
  // 정적 모드(기본, 폴링 스레드 없음).
  state.dir = (hotDirPath != nullptr) ? std::string(hotDirPath) : std::string();
  state.lastHash.clear();
  state.swapped = false;
  state.oldHash.clear();
  state.newHash.clear();
  state.error.clear();
  state.streakHash.clear();
  state.failureStreak = 0;
  state.poisonHash.clear();
}

int pollHotCoreOnce() {
  // 스왑 시도 대상 바이트 상태 — catch 경로의 실패 집계가 접근한다(try 안
  // 변수는 catch 에서 보이지 않으므로 try 밖에서 선언).
  std::string attemptedHash;
  try {
    std::string dir;
    {
      auto& state = hotState();
      std::lock_guard<std::mutex> lock(state.statusMutex);
      dir = state.dir;
    }
    if (dir.empty()) return 0; // 정적 모드 — 스왑 대상 없음(변화 없음).

    // 스왑 시도 직렬화 — 수동 poll(설치 경로)과 폴링 스레드가 같은 카피를
    // 두 번 열지 않게 한다. JS 스레드와 무관한 락이다(status 와 분리).
    std::lock_guard<std::mutex> pollLock(hotState().pollMutex);

    const std::string liveName = findLiveArtifact(dir);
    if (liveName.empty()) return 0; // 게이트 reject/미빌드 — 구 코어 유지.
    const std::string livePath = joinHotPath(dir, liveName);
    const std::string hash = sha256FileHex(livePath);
    if (hash.empty()) return 0; // 빌드 중/일시 부재 — 다음 폴링 재시도.
    {
      auto& state = hotState();
      std::lock_guard<std::mutex> lock(state.statusMutex);
      if (hash == state.lastHash) return 0; // 변화 없음.
      // 상한 실패로 포이즌된 바이트 — 새 바이트가 발행될 때까지 재시도 없음.
      if (hash == state.poisonHash) return 0;
    }
    attemptedHash = hash;

    // ── 스왑 시퀀스(설계 문서) ──
    // 1) 버전 카피 `<stem>-hot-<counter><ext>` — 같은 경로 재 dlopen 은 캐시
    //    히트로 구 매핑을 돌려주므로 반드시 신규 경로다. 카운터는 pollMutex
    //    배타 구간에서 단조 증가한다.
    auto& state = hotState();
    const uint64_t counter = ++state.counter;
    const size_t marker = liveName.find("-hot-live");
    const std::string stem = liveName.substr(0, marker);
    const std::string ext = liveName.substr(marker + std::strlen("-hot-live"));
    const std::string copyPath =
        joinHotPath(dir, stem + "-hot-" + std::to_string(counter) + ext);
    if (!copyFileContents(livePath, copyPath)) {
      recordHotCoreError("version copy failed: " + copyPath);
      noteSwapFailure(hash);
      return -1;
    }

    // 2) dlopen(RTLD_LOCAL) + 23심볼 바인딩 — 하나라도 없으면 카피 삭제+포기.
    //    실패 바인딩의 핸들도 leak 한다(dlclose 금지 계약 — dlopen 이 실행한
    //    초기화를 되돌릴 수 없다). 파일 삭제는 디렉터 엔트리만 지운다 —
    //    로드된 매핑은 inode 로 살아 있어 안전하다.
    void* handle = dlopen(copyPath.c_str(), RTLD_LOCAL);
    if (handle == nullptr) {
      const char* dlError = dlerror();
      recordHotCoreError(std::string("dlopen failed: ") +
                         (dlError != nullptr ? dlError : "unknown"));
      remove(copyPath.c_str());
      noteSwapFailure(hash);
      return -1;
    }
    auto* table = new CoreTable(); // value-init — 모든 fn 포인터 0
    bool bound = true;
    const char* missing = nullptr;
#define RUSTRA_BIND(field, symbolName)                                        \
  do {                                                                        \
    table->field = reinterpret_cast<decltype(table->field)>(                  \
      dlsym(handle, symbolName));                                             \
    if (table->field == nullptr) {                                            \
      bound = false;                                                          \
      missing = symbolName;                                                   \
    }                                                                         \
  } while (0)
    RUSTRA_BIND(invoke, "rustra_ffi_invoke");
    RUSTRA_BIND(invoke_json, "rustra_ffi_invoke_json");
    RUSTRA_BIND(invoke_postcard, "rustra_ffi_invoke_postcard");
    RUSTRA_BIND(invoke_rkyv_v2, "rustra_ffi_invoke_rkyv_v2");
    RUSTRA_BIND(free, "rustra_ffi_free");
    RUSTRA_BIND(invoke_buffer, "rustra_ffi_invoke_buffer");
    RUSTRA_BIND(has_buffer, "rustra_ffi_has_buffer");
    RUSTRA_BIND(free_owned_bytes, "rustra_ffi_free_owned_bytes");
    RUSTRA_BIND(event_sink_register, "rustra_ffi_event_sink_register");
    RUSTRA_BIND(event_sink_unregister, "rustra_ffi_event_sink_unregister");
    RUSTRA_BIND(channel_create, "rustra_ffi_channel_create");
    RUSTRA_BIND(channel_send, "rustra_ffi_channel_send");
    RUSTRA_BIND(channel_create_bytes, "rustra_ffi_channel_create_bytes");
    RUSTRA_BIND(channel_send_bytes, "rustra_ffi_channel_send_bytes");
    RUSTRA_BIND(channel_drop, "rustra_ffi_channel_drop");
    RUSTRA_BIND(mobile_init, "rustra_mobile_init");
    RUSTRA_BIND(invoke_cancel, "rustra_ffi_invoke_cancel");
    RUSTRA_BIND(invoke_rkyv_v2_async_into, "rustra_ffi_invoke_rkyv_v2_async_into");
    RUSTRA_BIND(invoke_rkyv_v2_into, "rustra_ffi_invoke_rkyv_v2_into");
    RUSTRA_BIND(invoke_raw, "rustra_ffi_invoke_raw");
    RUSTRA_BIND(has_raw, "rustra_ffi_has_raw");
    RUSTRA_BIND(get_schema, "rustra_ffi_get_schema");
    RUSTRA_BIND(contract_hash, "rustra_ffi_contract_hash");
#undef RUSTRA_BIND
    if (!bound) {
      recordHotCoreError(std::string("symbol missing in hot dylib: ") +
                         (missing != nullptr ? missing : "?"));
      remove(copyPath.c_str());
      delete table;
      noteSwapFailure(hash);
      return -1;
    }

    // 3) 새 코어 초기화 + 계약 해시 조회 — 미발행 테이블 대상이라 JS 스레드
    //    호출과 경합하지 않는다(발행은 5번). 모듈별 FFI_CONTEXT 이므로 신
    //    dylib 의 mobile_init 은 자기 컨텍스트를 새로 등록한다(Rust 무수정).
    //    contract_hash 부재 코어는 발행하지 않는다(Rust watch 가 open 을
    //    실패 처리하는 것과 동일한 fail-closed — 핸들/테이블은 leak 계약대로
    //    폐기하지 않고 카피 엔트리만 지운다).
    table->mobile_init();
    const std::string newHash = readContractHash(table);
    if (newHash.empty()) {
      recordHotCoreError("hot dylib did not report a contract hash — not swapped");
      remove(copyPath.c_str());
      delete table;
      noteSwapFailure(hash);
      return -1;
    }

    // 4) 구 코어 계약 해시(로그/상태용) — 읽기 전용 조회라 호출 중 JS 스레드와
    //    무해하게 병행된다(Tauri watch 가 live handle 에서 읽는 것과 동일).
    const std::string oldHash = readContractHash(currentCoreTable());

    // 5) atomic publish — release store 이후 모든 호출부의 acquire load 가
    //    초기화가 끝난 신 코어의 메모리 가시성을 보장한다. 구 핸들/테이블은
    //    의도적으로 leak — 절대 dlclose 하지 않는다.
    g_coreTable.store(table, std::memory_order_release);

    // 6) 리스너 보유 시 신 코어에 이벤트 싱크 재등록(콜백은 C++ 정적 함수라
    //    재사용 안전). 발행→재등록 사이 미세 창의 신 코어 이벤트 유실은
    //    dev 감수(설계 "상태 소실" 정책).
    getEventDispatcher()->rebindEventSink();

    // 6b) 구 코어 발급 채널 폐기 요청 — 채널 핸들은 발급 코어에 귀속되고
    //     새 dylib 의 핸들 발급기는 번호를 처음부터 재사용하므로, 스왑 뒤
    //     레지스트리를 비우지 않으면 같은 번호의 신규 채널을 오해제하는
    //     창이 있다. 폐기는 drain(JS 스레드)에서 수행 — 이벤트 재등록과
    //     같은 마샬링 규약("상태 소실" 정책의 채널 대응).
    getChannelDispatcher()->requestResetAfterSwap();

    {
      std::lock_guard<std::mutex> lock(state.statusMutex);
      state.lastHash = hash;
      state.swapped = true;
      state.oldHash = oldHash;
      state.newHash = newHash;
      state.error.clear();
      state.streakHash.clear();
      state.failureStreak = 0;
      state.poisonHash.clear();
    }
    fprintf(stderr, "[rustra] hot-core: swapped %s -> %s\n",
            hashToShort8(oldHash).c_str(), hashToShort8(newHash).c_str());
    return 1;
  } catch (const std::exception& error) {
    // 어떤 실패도 폴링 스레드(나아가 앱 프로세스)를 죽이지 않는다 — lastHash
    // 는 갱신되지 않아 같은 바이트 상태를 다음 폴링이 재시도한다(단 같은
    // 바이트의 연속 실패는 noteSwapFailure 의 상한이 포이즌한다).
    recordHotCoreError(std::string("swap failed: ") + error.what());
    noteSwapFailure(attemptedHash);
    return -1;
  } catch (...) {
    recordHotCoreError("swap failed: unknown error");
    noteSwapFailure(attemptedHash);
    return -1;
  }
}

void startHotCorePolling() {
  // 이중 기동 방지 — 스레드는 프로세스 종료까지 산다(dev 전용 표면).
  if (g_hotCorePollingStarted.exchange(true, std::memory_order_acq_rel)) return;
  bool enabled = false;
  {
    std::lock_guard<std::mutex> lock(hotState().statusMutex);
    enabled = !hotState().dir.empty();
  }
  if (!enabled) {
    // 정적 모드 — 계약대로 폴링 스레드를 만들지 않고 재호출을 허용한다.
    g_hotCorePollingStarted.store(false, std::memory_order_release);
    return;
  }
  std::thread([] {
    for (;;) {
      std::this_thread::sleep_for(std::chrono::milliseconds(300));
      pollHotCoreOnce(); // 절대 던지지 않고 프로세스를 죽이지 않는다(위 계약).
    }
  }).detach();
}

HotCoreStatus hotCoreStatusSnapshot() {
  auto& state = hotState();
  std::lock_guard<std::mutex> lock(state.statusMutex);
  HotCoreStatus status;
  status.enabled = !state.dir.empty();
  status.swapped = state.swapped;
  status.oldHash = state.oldHash;
  status.newHash = state.newHash;
  status.error = state.error;
  return status;
}

} // namespace core

#else // !__APPLE__ && !__ANDROID__ — 핫스왑 미지원 호스트: 정적 모드 고정.

namespace rustra::core {

void configureHotCore(const char* /*hotDirPath*/) {
  // 미지원 플랫폼 — 호출을 받아도 정적 모드로 무시한다.
}

int pollHotCoreOnce() {
  return -1; // 핫스왑 미지원 — 폴링 자체를 오류로 보고한다.
}

void startHotCorePolling() {
  // 스레드 없음.
}

HotCoreStatus hotCoreStatusSnapshot() {
  return HotCoreStatus{}; // enabled=false — JS 표면은 null.
}

} // namespace rustra::core

#endif

// ── folly::dynamic 진입점 — jsi::Value 오버로드의 dynamic 변환 wrapper ──
// 변환 규칙: null/bool/int/double/string/array/object. int64 는 jsi 표면에
// BigInt 생성이 없어 double 로 간다(2^53 초과 손실 — 헤더 계약에 명시).
namespace {
facebook::jsi::Value dynamicToValue(
  facebook::jsi::Runtime& rt, const folly::dynamic& value) {
  using facebook::jsi::Value;
  switch (value.type()) {
    case folly::dynamic::NULLT:
      return Value::null();
    case folly::dynamic::BOOL:
      return Value(static_cast<bool>(value.asBool()));
    case folly::dynamic::INT64:
      return Value(static_cast<double>(value.asInt()));
    case folly::dynamic::DOUBLE:
      return Value(value.asDouble());
    case folly::dynamic::STRING: {
      const std::string& str = value.asString();
      return facebook::jsi::String::createFromUtf8(
        rt, reinterpret_cast<const uint8_t*>(str.data()), str.size());
    }
    case folly::dynamic::ARRAY: {
      facebook::jsi::Array array(rt, value.size());
      size_t index = 0;
      for (const auto& item : value) {
        array.setValueAtIndex(rt, index++, dynamicToValue(rt, item));
      }
      return Value(rt, array);
    }
    case folly::dynamic::OBJECT: {
      facebook::jsi::Object object(rt);
      for (const auto& [key, item] : value.items()) {
        object.setProperty(
          rt, facebook::jsi::String::createFromUtf8(rt, key.getString()), dynamicToValue(rt, item));
      }
      return Value(rt, object);
    }
  }
  return facebook::jsi::Value::undefined();
}
} // namespace

TypedInvokeResult invokeTypedByNameDynamic(
  facebook::jsi::Runtime& rt, const std::string& commandName,
  const folly::dynamic& args) {
  facebook::jsi::Value value = dynamicToValue(rt, args);
  return invokeTypedByName(rt, commandName, value);
}

TypedInvokeResult invokeTypedByIdDynamic(
  facebook::jsi::Runtime& rt, uint16_t commandId,
  const folly::dynamic& args) {
  facebook::jsi::Value value = dynamicToValue(rt, args);
  return invokeTypedById(rt, commandId, value);
}

} // namespace rustra
