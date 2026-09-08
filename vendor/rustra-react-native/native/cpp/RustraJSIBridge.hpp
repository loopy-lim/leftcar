#pragma once

#include <jsi/jsi.h>
#include <deque>
#include <memory>
#include <mutex>
#include <string>
#include <unordered_map>
#include <vector>
#include <utility>

namespace rustra {

extern "C" {
  // ── Generic FFI (from rustra::ffi) ──────────────────────
  uint8_t* rustra_ffi_invoke(
    const uint8_t* payload, size_t payload_len, size_t* out_len);
  uint8_t* rustra_ffi_invoke_json(
    const uint8_t* payload, size_t payload_len, size_t* out_len);
  uint8_t* rustra_ffi_invoke_postcard(
    const uint8_t* payload, size_t payload_len, size_t* out_len);
  void rustra_ffi_free(uint8_t* ptr, size_t len);
  uint32_t rustra_ffi_invoke_buffer(
    uint16_t command_id, const uint8_t* payload, size_t payload_len,
    uint8_t** out_ptr, size_t* out_len);
  uint32_t rustra_ffi_has_buffer(uint16_t command_id);
  void rustra_ffi_free_owned_bytes(uint8_t* ptr, size_t len);

  // ── Event sink push delivery (from rustra::ffi) ─────────
  // C 호스트가 Rust → JS 이벤트 푸시용 콜백을 등록/해제한다.
  // 콜백은 emit 호출 스레드에서 실행된다 — 호스트가 JS 런타임 스레드로
  // 마샬링해야 한다 (아래 EventDispatcher).
  // 콜백은 예외를 던지면 안 된다: Rust 프레임을 통과하는 외국 예외은
  // Rust 가 잡을 수 없어 프로세스 abort 다 ("C-unwind" ABI 계약).
  typedef void (*rustra_event_callback_t)(
    void* user_data, const char* name, const char* payload);
  void rustra_ffi_event_sink_register(
    rustra_event_callback_t callback, void* user_data);
  void rustra_ffi_event_sink_unregister(void);

  // ── Channel FFI (타입 패리티 2단계 — Tauri ipc::Channel 모델) ──
  // 채널은 호출 귀속 유니캐스트 회신이다(이벤트 싱크 = 브로드캐스트 와 대조).
  // createChannel(cb) → 핸들(≥1) 발급 → 커맨드 인자 channel 로 전달 →
  // Rust 가 send → JS 콜백 → 호출 완료 시 dropChannel. 핸들 번호는
  // 재사용되지 않는다(만료 후 send 는 조용히 false).
  typedef void (*rustra_channel_callback_t)(
    void* user_data, uint32_t handle, const char* payload);
  uint32_t rustra_ffi_channel_create(
    rustra_channel_callback_t callback, void* user_data);
  int32_t rustra_ffi_channel_send(uint32_t handle, const char* payload);
  int32_t rustra_ffi_channel_drop(uint32_t handle);

  // Stable package registration symbol emitted by `rustra::mobile_entry!`.
  void rustra_mobile_init(void);

#if defined(RUSTRA_ENABLE_LEGACY_BENCHMARKS)
  // ── Per-example FFI (benchmark legacy) ──────────────────
  uint8_t* rustra_calculator_invoke_bytes(
    const uint8_t* payload, size_t payload_len, size_t* out_len);
  uint8_t* rustra_calculator_invoke_raw(
    const uint8_t* payload, size_t payload_len, size_t* out_len);
  uint8_t* rustra_calculator_invoke_msgpack(
    const uint8_t* payload, size_t payload_len, size_t* out_len);
  uint8_t* rustra_calculator_invoke_bincode(
    const uint8_t* payload, size_t payload_len, size_t* out_len);
  uint8_t* rustra_calculator_invoke_postcard(
    const uint8_t* payload, size_t payload_len, size_t* out_len);
  uint8_t* rustra_calculator_invoke_rkyv(
    const uint8_t* payload, size_t payload_len, size_t* out_len);
  uint8_t* rustra_calculator_invoke_hybrid(
    const uint8_t* payload, size_t payload_len, size_t* out_len);
  uint8_t* rustra_calculator_invoke_rkyv_v2(
    const uint8_t* payload, size_t payload_len, size_t* out_len);

  void rustra_calculator_free_buffer(uint8_t* ptr, size_t len);
  void rustra_calculator_free_rkyv_v2_buffer(uint8_t* ptr, size_t len);
#endif

  // ── Cancellation (from rustra::ffi) ─────────────────────
  // invocation_id 로 진행 중 async 호출을 협력적 취소한다.
  bool rustra_ffi_invoke_cancel(uint64_t invocation_id);

  // ── rkyv V2 async caller-buffer (F3) ─────────────────────
  // Generic async entry. 성공 응답은 호스트 버퍼(buf/capacity)에 직접 기록되고
  // 콜백은 (user_data, resp, resp_len, owned) 로 1회 호출된다:
  //   owned=0 — resp 는 호스트가 넘긴 buf (호스트 해제 없음)
  //   owned=1 — resp 는 Rust heap 프레임 (rustra_ffi_free 로 정확히 1회 해제)
  // 수명 계약: buf 는 콜백이 실행되는 동안 살아 있어야 한다(호스트 소유).
  // 버퍼가 부족하면 재시도 프로토콜 없이 같은 dispatch 안에서 heap 프레임으로
  // 폴백한다 — 핸들러는 항상 정확히 1회 실행된다.
  typedef void (*rustra_async_into_callback_t)(
    void* user_data, uint8_t* resp, size_t resp_len, uint8_t owned);
  void rustra_ffi_invoke_rkyv_v2_async_into(
    const uint8_t* payload, size_t payload_len,
    uint8_t* buf, size_t capacity, void* user_data,
    rustra_async_into_callback_t on_complete, uint64_t* invocation_id);

  // ── (Tier 1) rkyv V2 caller-buffer — malloc→memcpy→free 사이클 제거 ──
  // buf=null → size-probe(0 반환, 필요 크기는 *out_len). buf≠null → 직접 기록,
  // 반환값은 기록한 바이트 수. capacity 부족 시 SIZE_MAX 반환(재probe 신호).
  // probe→write 2단계 사이 핸들러는 코어 probe 캐시로 1회만 실행된다.
  size_t rustra_ffi_invoke_rkyv_v2_into(
    const uint8_t* payload, size_t payload_len,
    uint8_t* buf, size_t capacity, size_t* out_len);

  // ── (Tier 0) 스칼라 직결 raw invoke — postcard 인코딩/디코딩 전부 제거 ──
  // 인자를 u64 슬롯(f64는 IEEE-754 비트, bool은 0/1)으로 직접 전달한다.
  // 반환: 0=성공(*out_slot 에 결과 슬롯), 1=핸들러 에러(에러 와이어를 err_buf
  // 에 복사, *err_len 에 필요 크기), UINT32_MAX=raw 불가 명령(호스트 폴백 신호).
  uint32_t rustra_ffi_invoke_raw(
    uint16_t command_id,
    const uint64_t* slots, size_t slot_count,
    uint64_t* out_slot,
    uint8_t* err_buf, size_t err_buf_cap, size_t* err_len);

  // 등록된 Rust 패키지가 해당 cmd_id 의 raw handler 를 실제로 보유하면 1.
  uint8_t rustra_ffi_has_raw(uint16_t command_id);
}

/// Cached function entry — stores PropNameID + pre-created JS Function.
struct CachedFunction {
  facebook::jsi::PropNameID propNameId;
  facebook::jsi::Function function;
};

/// Rust → JS 이벤트 푸시 디스패처.
///
/// FFI C 콜백(emitting 스레드)이 (name, payload_json) 을 큐에 적재하면
/// JS 런타임 스레드의 CallInvoker 가 큐를 drain 해 per-name JS 콜백으로
/// 전달한다. CallInvoker 가 없는 호스트(유닛 테스트 등)는 `__rustraNative`
/// 의 `drainEvents()` HostFunction 로 폴링 drain 할 수 있다.
///
/// 스레딩 계약:
/// - `onRustEvent` (FFI 콜백) — 어느 스레드에서든. 뮤텍스로 보호된 큐에
///   적재만 하고 JS 객체를 건드리지 않는다.
/// - `drain` — 반드시 JS 런타임 스레드에서. `Function::call` 은 JS 스레드에서만
///   안전하다 (CallInvoker 콜백 내부 또는 JS 가 drainEvents() 를 호출할 때).
/// - 큐는 고정 용량(1024) drop-oldest — JS 가 느려도 emit 스레드를 블록하지
///   않는다 (Rust EventBus 정책과 동일).
class EventDispatcher : public std::enable_shared_from_this<EventDispatcher> {
public:
  /// JS 스레드 마샬링용 CallInvoker 설정. installRustraJSI* 에서 호출된다.
  void setCallInvoker(std::shared_ptr<void> invoker);

  /// 현재 설치된 type-erased CallInvoker (없으면 nullptr).
  /// `invokeTypedAsync` 가 결과를 JS 스레드로 마샬링할 때 빌려간다.
  std::shared_ptr<void> currentCallInvoker() {
    std::lock_guard<std::mutex> lock(mutex_);
    return callInvoker_;
  }

  /// (name, callback) JS 리스너 등록/해제. JS 스레드에서 호출됨
  /// (HostFunction 경유). 같은 이름에 두 번 등록하면 마지막이 이긴다.
  void setListener(facebook::jsi::Runtime& rt, const std::string& name,
                   facebook::jsi::Function callback);
  void removeListener(const std::string& name);

  /// FFI C 콜백 — emitting 스레드에서 호출된다. 큐 적재 + CallInvoker 로
  /// drain 예약만 한다.
  static void onRustEvent(void* user_data, const char* name, const char* payload);

  /// 큐의 모든 이벤트를 JS 리스너로 전달한다. JS 런타임 스레드에서만 호출.
  void drain(facebook::jsi::Runtime& rt);

  /// 미처리 이벤트 수 (JS 폴링/디버그용).
  size_t pendingCount();

private:
  void scheduleDrainLocked();

  std::mutex mutex_;
  std::deque<std::pair<std::string, std::string>> queue_;
  size_t capacity_ = 1024;
  size_t dropped_ = 0;
  bool drainScheduled_ = false;
  std::shared_ptr<void> callInvoker_;
  /// per-name JS 콜백 레지스트리 — drain 에서만 접근(JS 스레드).
  std::unordered_map<std::string, facebook::jsi::Function> listeners_;
};

/// ChannelDispatcher: 채널 핸들별 Rust→JS 유니캐스트 회신 (2단계).
///
/// EventDispatcher(브로드캐스트) 와 동일한 큐+CallInvoker 마샬링을
/// 쓰지만, JS 콜백이 핸들별로 분리돼 있고 리로드 시 채널 테이블째
/// 폐기된다(채널은 호출 귀속 — 이전 런타임 대상 핸들은 무의미).
///
/// createChannel(cb) 이 u32 핸들을 발급하면 JS 는 그 값을 커맨드 인자
/// `channel` 로 그대로 전달한다(TS 타입 ChannelHandle = number).
class ChannelDispatcher : public std::enable_shared_from_this<ChannelDispatcher> {
public:
  /// JS 스레드 마샬링용 CallInvoker 설정(EventDispatcher 와 동일 소스 공유).
  void setCallInvoker(std::shared_ptr<void> invoker);

  /// JS 콜백 등록 + Rust 채널 발급. 반환값 = 채널 핸들(≥1). JS 스레드 호출.
  uint32_t create(facebook::jsi::Runtime& rt,
                  facebook::jsi::Function callback);
  /// 채널 해제(호출 완료/취소). 성공 true. JS 스레드 호출.
  bool drop(uint32_t handle);

  /// FFI C 콜백 — send 스레드에서 호출. 큐 적재 + drain 예약만.
  /// handle 은 발급 시 캡처된 채널 번호(핸들→JS 콜백 룩업 키).
  static void onChannelPayload(void* user_data, uint32_t handle, const char* payload);

  /// 큐의 모든 페이로드를 대응 핸들의 JS 콜백으로 전달. JS 스레드만.
  void drain(facebook::jsi::Runtime& rt);

  /// 리로드 대응: 보유 콜백·큐 폐기 및 Rust 채널 전부 drop. JS 스레드 호출.
  void reset();

private:
  void scheduleDrainLocked();

  std::mutex mutex_;
  /// (handle, payload) 큐 — onChannelPayload 가 적재, drain 이 소비.
  std::deque<std::pair<uint32_t, std::string>> queue_;
  size_t capacity_ = 1024;
  bool drainScheduled_ = false;
  std::shared_ptr<void> callInvoker_;
  /// 핸들별 JS 콜백 — drain 에서만 접근(JS 스레드).
  std::unordered_map<uint32_t, facebook::jsi::Function> callbacks_;
};

/// Optimized HostObject that caches all JSI functions on first access.
/// Avoids per-call string comparison and Function::createFromHostFunction allocation.
class RustraHostObject : public facebook::jsi::HostObject {
public:
  explicit RustraHostObject(facebook::jsi::Runtime& rt);

  facebook::jsi::Value get(
    facebook::jsi::Runtime& rt,
    const facebook::jsi::PropNameID& name) override;

  void set(
    facebook::jsi::Runtime& rt,
    const facebook::jsi::PropNameID& name,
    const facebook::jsi::Value& value) override {}

  std::vector<facebook::jsi::PropNameID> getPropertyNames(
    facebook::jsi::Runtime& rt) override;

  /// 설치 평탄화용: 캐시된 함수 이름 목록 (getPropertyNames 와 동일 로직,
  /// non-virtual). installRustraJSIWithInvoker 가 일반 Object 에 함수들을
  /// 프로퍼티로 박을 때 열거에 사용한다.
  std::vector<facebook::jsi::PropNameID> propertyNames(
    facebook::jsi::Runtime& rt);

  /// 설치 평탄화용: 이름으로 캐시된 Function 반환 (get 과 동일 스캔 로직,
  /// Value 래핑 대신 Function 자체 — cloneObject 로 같은 JS 함수를 참조하는
  /// 새 핸들). 못 찾으면 throw — 설치 경로는 propertyNames 로 얻은 이름만
  /// 쓰므로 정상적으로는 도달 안 함.
  facebook::jsi::Function getFunction(
    facebook::jsi::Runtime& rt,
    const facebook::jsi::PropNameID& name);

private:
  /// Cache of function name → {PropNameID, Function}.
  /// Populated lazily on first property access for each name.
  std::unordered_map<std::string, std::unique_ptr<CachedFunction>> cache_;
};

void installRustraJSI(facebook::jsi::Runtime& rt);

/// Runtime teardown/reload 직전에 pending async 콜백을 취소하고 구 Runtime
/// 소유 JSI Function 핸들을 JS 스레드에서 폐기한다.
void invalidateRustraJSI();

/// installRustraJSI + JS 스레드 CallInvoker 주입. iOS(RCTCxxBridge) 와
/// Android(CallInvokerHolder) 플랫폼 글루가 각자의 방식으로 CallInvoker 를
/// 얻어 이 진입점으로 넘긴다.
///
/// CallInvoker 타입은 `facebook::react::CallInvoker` 이지만 이 헤더는
/// ReactAndroid/React-callinvoker 헤더에 의존하지 않는다 — 플랫폼 글루가
/// `void` shared_ptr 로 type-erase 해서 전달하고, .cpp 가 내부에서
/// static_cast 로 복원한다(단일 정의 지점 유지).
void installRustraJSIWithInvoker(
  facebook::jsi::Runtime& rt,
  std::shared_ptr<void> typeErasedCallInvoker);

} // namespace rustra
