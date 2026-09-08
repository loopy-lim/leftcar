#include "RustraJSIBridge.hpp"
#include "rustra-generated-codecs.hpp"
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <jsi/jsi.h>
#include <limits>
#include <memory>
#include <optional>
#include <vector>

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
class RustOwnedMutableBuffer final : public MutableBuffer {
public:
  RustOwnedMutableBuffer(uint8_t* data, size_t size) : data_(data), size_(size) {}

  ~RustOwnedMutableBuffer() override {
    if (data_ != nullptr) rustra_ffi_free_owned_bytes(data_, size_);
  }

  size_t size() const override { return size_; }
  uint8_t* data() override { return data_; }

private:
  uint8_t* data_;
  size_t size_;
};

static Value createOwnedArrayBuffer(Runtime& rt, uint8_t* data, size_t size) {
  std::shared_ptr<RustOwnedMutableBuffer> owner;
  try {
    owner = std::make_shared<RustOwnedMutableBuffer>(data, size);
  } catch (...) {
    rustra_ffi_free_owned_bytes(data, size);
    throw;
  }
  ArrayBuffer buffer(rt, std::move(owner));
  return Value(rt, buffer);
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
// free 짝 계약: (Tier 1) typedInvokeTail 은 caller-buffer 변형
// (rustra_ffi_invoke_rkyv_v2_into) 을 쓴다 — Rust 가 응답을 할당하지 않고
// caller 소유 버퍼에 직접 기록하므로 free 짝이 필요 없다. 먼저 512B 스택
// 버퍼로 바로 dispatch+write하고, 부족한 경우에만 코어가 캐시한 같은 응답을
// 정확한 크기의 vector로 재시도한다. 작은 응답은 FFI 1회, 큰 응답도 핸들러는
// 정확히 1회만 실행된다.
template <typename Decode>
static Value typedInvokeTail(Runtime& rt, const uint8_t* reqData, size_t reqSize,
                             const char* tailSuffix, Decode decode,
                             const std::string* batchItemName = nullptr) {
  // (Tier 1) 고정 스택 버퍼 — 대부분의 응답(숫자/작은 객체)이 여기에 들어온다.
  // 부족하면 아래 폴백 경로가 처리하므로 안전하다.
  constexpr size_t kStackCap = 512;
  uint8_t stackBuf[kStackCap];
  size_t out_len = 0;
  const uint8_t* resp = nullptr;
  std::vector<uint8_t> largeBuf;

  // 1단계: 스택 버퍼로 바로 dispatch+write. 대부분의 응답은 여기서 끝나
  // size-probe를 위한 두 번째 FFI 횡단과 thread_local 캐시 왕복이 없다.
  size_t n = rustra_ffi_invoke_rkyv_v2_into(
    reqData, reqSize, stackBuf, kStackCap, &out_len);
  if (n != SIZE_MAX && n > 0) {
    resp = stackBuf;
  }

  // 버퍼가 부족하면 코어가 out_len에 필요 크기를 쓰고 동일 응답을 캐시한다.
  // 정확한 크기로 한 번만 재시도하므로 비멱등 핸들러는 재실행되지 않는다.
  if (!resp && n == SIZE_MAX && out_len > kStackCap) {
    largeBuf.resize(out_len);
    n = rustra_ffi_invoke_rkyv_v2_into(
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
    queue_.clear();
    drainScheduled_ = false;
  }
  // mutex_ 를 잡은 채 FFI unregister 를 호출하면 emit 중 onRustEvent 가 같은
  // 락을 기다리는 동안 교착할 수 있으므로 반드시 락 밖에서 해제한다.
  if (hadListeners) {
    // 리스너가 있던 상태로 리로드된 경우 싱크를 해제해 둔다 — 새 번들이
    // setListener 로 다시 등록하면 그때 재설치된다.
    rustra_ffi_event_sink_unregister();
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
  // 첫 리스너 등록 시 FFI 싱크를 설치한다(폴링 경로 → 푸시 전환).
  if (wasEmpty) {
    rustra_ffi_event_sink_register(&EventDispatcher::onRustEvent, this);
  }
}

void EventDispatcher::removeListener(const std::string& name) {
  listeners_.erase(name);
  // 마지막 리스너 제거 시 FFI 싱크 해제(푸시 → 폴링 복귀).
  if (listeners_.empty()) {
    rustra_ffi_event_sink_unregister();
  }
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
  std::vector<uint32_t> toDrop;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    callInvoker_ = std::move(invoker);
    for (auto& [h, _cb] : callbacks_) toDrop.push_back(h);
    callbacks_.clear();
    queue_.clear();
    drainScheduled_ = false;
  }
  // 리로드 대응: 귀속 채널 전부를 Rust 쪽에서도 drop(락 밖 — FFI 재진입 방지).
  for (uint32_t h : toDrop) {
    rustra_ffi_channel_drop(h);
  }
}

uint32_t ChannelDispatcher::create(facebook::jsi::Runtime& rt,
                                    facebook::jsi::Function callback) {
  // JS 스레드에서만 호출됨(HostFunction 경유). FFI 가 핸들을 선발급하고
  // 콜백이 그 핸들을 캡처해 회신하므로, 여기선 JS 콜백만 핸들 키로 등록.
  (void)rt;
  uint32_t handle = rustra_ffi_channel_create(&ChannelDispatcher::onChannelPayload, this);
  if (handle == 0) return 0; // 발급 실패 sentinel — 사실상 도달하지 않는다.
  callbacks_.insert_or_assign(handle, std::move(callback));
  return handle;
}

bool ChannelDispatcher::drop(uint32_t handle) {
  // JS 스레드 호출. Rust 채널 해제 후 콜백 제거. 해제 후 drain 에 이미
  // 적재된 해당 핸들 페이로드는 콜백 부재로 무시된다(유니캐스트 만료).
  int dropped = rustra_ffi_channel_drop(handle);
  callbacks_.erase(handle);
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

void ChannelDispatcher::drain(facebook::jsi::Runtime& rt) {
  // JS 런타임 스레드에서만 호출(CallInvoker 콜백 또는 폴링).
  std::deque<std::pair<uint32_t, std::string>> items;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    drainScheduled_ = false;
    items.swap(queue_);
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
  // 리로드 대응 전체 폐기 — JS 콜백 맵·큐 클리어 후 Rust 채널 drop(락 밖).
  std::vector<uint32_t> toDrop;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    for (auto& [h, _cb] : callbacks_) toDrop.push_back(h);
    callbacks_.clear();
    queue_.clear();
    drainScheduled_ = false;
  }
  for (uint32_t h : toDrop) {
    rustra_ffi_channel_drop(h);
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
  /// 응답만 Rust heap 프레임으로 돌아온다(owned=1 → rustra_ffi_free 짝).
  std::vector<uint8_t> frameBuffer = std::vector<uint8_t>(512);
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
    rustra_ffi_invoke_cancel(id);
  }
}

// ── HostObject with cached functions ───────────────────────

using InvokeFn = uint8_t*(*)(const uint8_t*, size_t, size_t*);

// free 짝 계약: generic FFI response buffers use rustra_ffi_free. The optional
// calculator benchmark surface is compiled only in the repository fixture and
// retains its legacy allocator-specific pairs.
using FreeFn = void(*)(uint8_t*, size_t);

// live schema FFI (from rustra crate)
extern "C" uint8_t* rustra_ffi_get_schema(size_t* out_len);
extern "C" uint8_t* rustra_ffi_contract_hash(size_t* out_len);

RustraHostObject::RustraHostObject(Runtime& rt) {
  auto makeInvoke = [&](const char* name, InvokeFn fn, FreeFn freeFn, const char* err) {
    auto propNameId = PropNameID::forAscii(rt, name);
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 1,
      [fn, freeFn, err](Runtime& rt, const Value&, const Value* args, size_t count) -> Value {
        if (count < 1) {
          throw JSError(rt, std::string("RustraJSI: requires 1 argument — ") + err);
        }
        auto [data, size] = extractBytes(rt, args[0]);
        size_t out_len = 0;
        uint8_t* result = fn(data, size, &out_len);
        if (!result) {
          throw JSError(rt, std::string("RustraJSI: ") + err);
        }
        auto returnValue = createArrayBuffer(rt, result, out_len);
        freeFn(result, out_len);
        return returnValue;
      });
    cache_[name] = std::make_unique<CachedFunction>(
      CachedFunction{std::move(propNameId), std::move(hostFn)});
  };

  // ── Generic FFI paths (default, json, postcard) — magic 헤더 레이아웃이므로
  //    rustra_ffi_free 로 해제 짝. ─────────────────────────────
  makeInvoke("invoke",        rustra_ffi_invoke,              rustra_ffi_free, "Rust returned null");
  makeInvoke("invokeJson",    rustra_ffi_invoke_json,         rustra_ffi_free, "Rust json returned null");
  makeInvoke("invokePostcardFFI", rustra_ffi_invoke_postcard, rustra_ffi_free, "Rust postcard FFI returned null");

#if defined(RUSTRA_ENABLE_LEGACY_BENCHMARKS)
  // ── Per-example benchmark paths (legacy) — calculator 응답(magic 헤더 없는
  //    Box<[u8]>)이므로 rustra_calculator_free_buffer 로 해제 짝. ──
  makeInvoke("invokeBytes",   rustra_calculator_invoke_bytes,  rustra_calculator_free_buffer, "Rust bytes returned null");
  makeInvoke("invokeMsgpack",  rustra_calculator_invoke_msgpack, rustra_calculator_free_buffer, "Rust msgpack returned null");
  makeInvoke("invokeBincode",  rustra_calculator_invoke_bincode, rustra_calculator_free_buffer, "Rust bincode returned null");
  // Keep the public JS adapter name aligned with RustraNative. This is the
  // calculator's legacy postcard envelope (command + a + b), while
  // invokePostcardFFI above is the generic framework envelope.
  makeInvoke("invokePostcard", rustra_calculator_invoke_postcard, rustra_calculator_free_buffer, "Rust postcard returned null");
  makeInvoke("invokeLegacyPostcard", rustra_calculator_invoke_postcard, rustra_calculator_free_buffer, "Rust postcard returned null");
  makeInvoke("invokeRkyv",     rustra_calculator_invoke_rkyv,    rustra_calculator_free_buffer, "Rust rkyv returned null");
  makeInvoke("invokeHybrid",   rustra_calculator_invoke_hybrid,  rustra_calculator_free_buffer, "Rust hybrid returned null");
  // rkyv V2 는 코어 rustra_ffi_invoke_rkyv_v2 로 위임된 뒤라 응답이 코어 FFI
  // 레이아웃(8B magic 헤더)이다 — 전용 free 짝 필수(Phase 2 위임 시 누락돼
  // ArrayBuffer 경로에서 double-free/unallocated-free 크래시를 일으켰다).
  makeInvoke("invokeRkyvV2",   rustra_calculator_invoke_rkyv_v2, rustra_calculator_free_rkyv_v2_buffer, "Rust rkyv v2 returned null");
  makeInvoke("invokeRaw",      rustra_calculator_invoke_raw,     rustra_calculator_free_buffer, "Rust invoke_raw returned null");
#endif

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

  // getSchema: live schema query → rustra_ffi_get_schema (정적 + 동적 명령)
  {
    auto propNameId = PropNameID::forAscii(rt, "getSchema");
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 0,
      [](Runtime& rt, const Value&, const Value*, size_t) -> Value {
        size_t out_len = 0;
        uint8_t* data = rustra_ffi_get_schema(&out_len);
        if (!data) {
          throw JSError(rt, "RustraJSI: getSchema returned null");
        }
        auto returnValue = createArrayBuffer(rt, data, out_len);
        rustra_ffi_free(data, out_len);
        return returnValue;
      });
    cache_["getSchema"] = std::make_unique<CachedFunction>(
      CachedFunction{std::move(propNameId), std::move(hostFn)});
  }

  // getContractHash: (F5) native 빌드 계약 해시 → rustra_ffi_contract_hash.
  // 엔진 옵션 contractHash 설정 시 JS 가 생성된 GENERATED_CONTRACT_HASH 와
  // 비교해 스키마 드리프트(contract.mismatch)를 검증한다.
  {
    auto propNameId = PropNameID::forAscii(rt, "getContractHash");
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 0,
      [](Runtime& rt, const Value&, const Value*, size_t) -> Value {
        size_t out_len = 0;
        uint8_t* data = rustra_ffi_contract_hash(&out_len);
        if (!data) {
          throw JSError(rt, "RustraJSI: getContractHash returned null");
        }
        auto returnValue = createArrayBuffer(rt, data, out_len);
        rustra_ffi_free(data, out_len);
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
  // drainEvents(): CallInvoker 없는 호스트의 JS 폴링 drain. 반환값 = 처리된
  // 이벤트 수. CallInvoker 경로가 켜져 있으면 보통 비어 있다(자동 drain 됨).
  {
    auto dispatcher = getEventDispatcher();
    auto propNameId = PropNameID::forAscii(rt, "drainEvents");
    auto hostFn = Function::createFromHostFunction(
      rt, propNameId, 0,
      [dispatcher](Runtime& rt, const Value&, const Value*, size_t) -> Value {
        size_t before = dispatcher->pendingCount();
        dispatcher->drain(rt);
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
        uint32_t capabilities = 0;
        if (gen::has_static_codec_id(cmdId)) capabilities |= 1u;
        if (gen::has_pos_codec(cmdId)) capabilities |= 2u;
        if (gen::has_raw_codec(cmdId) && rustra_ffi_has_raw(cmdId) != 0) capabilities |= 4u;
        if (gen::has_buffer_codec(cmdId) && rustra_ffi_has_buffer(cmdId) != 0) capabilities |= 8u;
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
        uint8_t* output = nullptr;
        size_t outputSize = 0;
        uint32_t status = rustra_ffi_invoke_buffer(
          cmdId, data, size, &output, &outputSize);
        if (status == UINT32_MAX || output == nullptr) {
          throw JSError(rt, "RustraJSI: direct buffer ABI failed");
        }
        if (status != 0) {
          std::string error(reinterpret_cast<const char*>(output), outputSize);
          rustra_ffi_free_owned_bytes(output, outputSize);
          throw JSError(rt, error.empty() ? "buffer invoke failed" : error);
        }
        Value buffer = createOwnedArrayBuffer(rt, output, outputSize);
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

        // 1) JS 객체 → postcard 요청 바이트 ([cmd_id u16 LE][postcard(I)])
        rc::Writer w;
        if (!gen::encode_by_name(rt, name, args[1], w)) {
          throw JSError(rt, "RustraJSI: no C++ codec for '" + name + "'");
        }
        // 2) Rust FFI (rkyv V2 단일 엔진) + 응답 tail — 공통 헬퍼로
        //    (typedInvokeTail 주석의 free 짝 계약: rustra_calculator_free_buffer).
        //    decoder 만 이름 기반 decode_by_name.
        return typedInvokeTail(rt, w.data(), w.size(), "", [&rt, &name](rc::Reader& r) {
          return gen::decode_by_name(rt, name, r);
        });
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

        // 1) JS 객체 → postcard 요청 바이트 ([cmd_id u16 LE][postcard(I)])
        rc::Writer w;
        if (!gen::encode_by_id(rt, cmdId, args[1], w)) {
          throw JSError(rt, "RustraJSI: no C++ codec for cmd_id " + std::to_string(cmdId));
        }
        // 2) Rust FFI + 응답 tail — invokeTyped 와 동일하지만 decoder 만
        //    u16 디스패치 decode_by_id (free 짝: rustra_calculator_free_buffer).
        return typedInvokeTail(rt, w.data(), w.size(), "", [&rt, cmdId](rc::Reader& r) {
          return gen::decode_by_id(rt, cmdId, r);
        });
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
        uint32_t code = rustra_ffi_invoke_raw(
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

        // 2) 비동기 FFI — id 를 동기 반환한다 (취소 핸들).
        uint64_t invocationId = 0;
        rustra_ffi_invoke_rkyv_v2_async_into(
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
            //             길이를 버리는 deleter 는 쓸 수 없다.
            std::shared_ptr<uint8_t> ownedFrame;
            if (owned == 1) {
              ownedFrame = std::shared_ptr<uint8_t>(
                resp, [resp_len](uint8_t* p) { rustra_ffi_free(p, resp_len); });
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
        uint64_t invocationId = 0;
        rustra_ffi_invoke_rkyv_v2_async_into(
          req.data(), req.size(),
          ctx->frameBuffer.data(), ctx->frameBuffer.size(),
          holder,
          [](void* user_data, uint8_t* resp, size_t resp_len, uint8_t owned) {
            std::unique_ptr<std::shared_ptr<AsyncCallContext>> holder(
              static_cast<std::shared_ptr<AsyncCallContext>*>(user_data));
            std::shared_ptr<AsyncCallContext> ctx = *holder;
            std::shared_ptr<uint8_t> ownedFrame;
            if (owned == 1) {
              ownedFrame = std::shared_ptr<uint8_t>(
                resp, [resp_len](uint8_t* p) { rustra_ffi_free(p, resp_len); });
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
        return Value(rustra_ffi_invoke_cancel(id));
      });
    cache_["invokeCancel"] = std::make_unique<CachedFunction>(
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
  rustra_mobile_init();
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

} // namespace rustra
