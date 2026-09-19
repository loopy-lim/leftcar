#pragma once

#include <jsi/jsi.h>
#include <atomic>
#include <deque>
#include <memory>
#include <mutex>
#include <string>
#include <unordered_map>
#include <unordered_set>
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
  uint8_t* rustra_ffi_invoke_frame(
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
  // 바이너리 채널 — 페이로드가 임의 바이트(Frame 프레임 등). JSON 채널과
  // 동일 핸들 공간/수명 계약, 한 핸들은 한 경로로만 동작한다.
  typedef void (*rustra_channel_bytes_callback_t)(
    void* user_data, uint32_t handle, const uint8_t* payload, size_t payload_len);
  uint32_t rustra_ffi_channel_create_bytes(
    rustra_channel_bytes_callback_t callback, void* user_data);
  int32_t rustra_ffi_channel_send_bytes(
    uint32_t handle, const uint8_t* payload, size_t payload_len);
  int32_t rustra_ffi_channel_drop(uint32_t handle);

  // Stable package registration symbol emitted by `rustra::mobile_entry!`.
  void rustra_mobile_init(void);

  // ── Cancellation (from rustra::ffi) ─────────────────────
  // invocation_id 로 진행 중 async 호출을 협력적 취소한다.
  bool rustra_ffi_invoke_cancel(uint64_t invocation_id);

  // ── Frame async caller-buffer (F3) ─────────────────────
  // Generic async entry. 성공 응답은 호스트 버퍼(buf/capacity)에 직접 기록되고
  // 콜백은 (user_data, resp, resp_len, owned) 로 1회 호출된다:
  //   owned=0 — resp 는 호스트가 넘긴 buf (호스트 해제 없음)
  //   owned=1 — resp 는 Rust heap 프레임 (rustra_ffi_free 로 정확히 1회 해제)
  // 수명 계약: buf 는 콜백이 실행되는 동안 살아 있어야 한다(호스트 소유).
  // 버퍼가 부족하면 재시도 프로토콜 없이 같은 dispatch 안에서 heap 프레임으로
  // 폴백한다 — 핸들러는 항상 정확히 1회 실행된다.
  typedef void (*rustra_async_into_callback_t)(
    void* user_data, uint8_t* resp, size_t resp_len, uint8_t owned);
  void rustra_ffi_invoke_frame_async_into(
    const uint8_t* payload, size_t payload_len,
    uint8_t* buf, size_t capacity, void* user_data,
    rustra_async_into_callback_t on_complete, uint64_t* invocation_id);

  // ── (Tier 1) Frame caller-buffer — malloc→memcpy→free 사이클 제거 ──
  // buf=null → size-probe(0 반환, 필요 크기는 *out_len). buf≠null → 직접 기록,
  // 반환값은 기록한 바이트 수. capacity 부족 시 SIZE_MAX 반환(재probe 신호).
  // probe→write 2단계 사이 핸들러는 코어 probe 캐시로 1회만 실행된다.
  size_t rustra_ffi_invoke_frame_into(
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

  // Stable live schema/hash query symbols (from rustra crate).
  uint8_t* rustra_ffi_get_schema(size_t* out_len);
  uint8_t* rustra_ffi_contract_hash(size_t* out_len);
}

// ── dylib 핫스왑 dev 코어 (Phase 3) ────────────────────────
// 설계: docs/plans/2026-09-09-native-hot-core-design.md. 정적 모드(기본)는
// 링크된 심볼 주소의 불변 테이블 1개 — 기존 직접 호출과 동일 대상이다. 핫
// 모드(`configureHotCore` 활성)에서는 폴링 스레드가 `<dir>/*-hot-live.*` 의
// sha256 변화마다 버전 카피 → dlopen → 23심볼 바인딩 → mobile_init →
// contract_hash → atomic publish 로 테이블을 교체한다. 모든 FFI 호출부가
// 호출 시점 테이블을 로드하므로 JS 재바인딩/재설치 없이 다음 호출부터 새
// 코어로 향한다(JSI HostFunction 은 C++ 셸 소유 유지).
namespace core {

/// 스왑 단위 C ABI 함수 포인터 테이블 — 어댑터가 참조하는 23 심볼.
/// 발행 후 절대 수정하지 않는 불변 객체이며, 구 코어를 절대 dlclose 하지
/// 않는 계약과 맞물려 테이블 객체 자체도 폐기하지 않는다(의도적 leak —
/// 스왑별 소량 누수는 dev 감수 정책, 설계 문서 "스왑 시퀀스" 5번과 동일).
struct CoreTable {
  // ── Generic FFI ──
  uint8_t* (*invoke)(const uint8_t* payload, size_t payload_len, size_t* out_len);
  uint8_t* (*invoke_json)(const uint8_t* payload, size_t payload_len, size_t* out_len);
  uint8_t* (*invoke_postcard)(const uint8_t* payload, size_t payload_len, size_t* out_len);
  uint8_t* (*invoke_frame)(const uint8_t* payload, size_t payload_len, size_t* out_len);
  void (*free)(uint8_t* ptr, size_t len);
  uint32_t (*invoke_buffer)(uint16_t command_id, const uint8_t* payload,
                            size_t payload_len, uint8_t** out_ptr, size_t* out_len);
  uint32_t (*has_buffer)(uint16_t command_id);
  void (*free_owned_bytes)(uint8_t* ptr, size_t len);
  // ── Event sink ──
  void (*event_sink_register)(rustra_event_callback_t callback, void* user_data);
  void (*event_sink_unregister)(void);
  // ── Channel ──
  uint32_t (*channel_create)(rustra_channel_callback_t callback, void* user_data);
  int32_t (*channel_send)(uint32_t handle, const char* payload);
  uint32_t (*channel_create_bytes)(rustra_channel_bytes_callback_t callback, void* user_data);
  int32_t (*channel_send_bytes)(uint32_t handle, const uint8_t* payload, size_t payload_len);
  int32_t (*channel_drop)(uint32_t handle);
  // ── Package registration (`rustra::mobile_entry!`) ──
  void (*mobile_init)(void);
  // ── Cancellation / async / raw / schema ──
  bool (*invoke_cancel)(uint64_t invocation_id);
  void (*invoke_frame_async_into)(const uint8_t* payload, size_t payload_len,
                                    uint8_t* buf, size_t capacity, void* user_data,
                                    rustra_async_into_callback_t on_complete,
                                    uint64_t* invocation_id);
  size_t (*invoke_frame_into)(const uint8_t* payload, size_t payload_len,
                                uint8_t* buf, size_t capacity, size_t* out_len);
  uint32_t (*invoke_raw)(uint16_t command_id, const uint64_t* slots, size_t slot_count,
                         uint64_t* out_slot, uint8_t* err_buf, size_t err_buf_cap,
                         size_t* err_len);
  uint8_t (*has_raw)(uint16_t command_id);
  uint8_t* (*get_schema)(size_t* out_len);
  uint8_t* (*contract_hash)(size_t* out_len);
};

/// 현재 테이블 — 절대 nullptr 이 아니다(정적 모드는 링크 심볼 테이블).
/// 모든 FFI 호출부가 호출 시점에 로드한다. 구현 홀더는 C++17 호환으로,
/// `std::atomic<std::shared_ptr<const CoreTable>>` 는 C++20 라이브러리
/// 기능이라 RN 툴체인(Xcode/NDK c++17)에서 쓸 수 없으므로 atomic 포인터
/// (release store / acquire load)로 동일 의미를 달성한다 — 테이블이 불변·
/// 무폐기라 포인터 수명 걱정이 없다.
const CoreTable* currentCoreTable();

/// 플랫폼 글루가 설치 시 1회 호출. 빈/null 문자열이면 정적 모드(기본,
/// 폴링 스레드 없음).
void configureHotCore(const char* hotDirPath);

/// 폴링 1회. 1=스왑 발생, 0=변화 없음, 음수=오류. 스레드 안전.
/// 오류 표면: 이 함수는 어떤 입력/실패에서도 예외를 밖으로 던지지 않고
/// 프로세스를 죽이지 않는다 — 모든 실패는 음수 반환 + stderr 로그다.
/// 같은 바이트(sha256)의 연속 실패가 상한(5회)에 도달하면 그 바이트를
/// 포이즌해 바이트가 바뀔 때까지 0을 반환한다(실패 재시도 폭주 방지).
int pollHotCoreOnce();

/// 300ms 폴링 스레드 기동(`configureHotCore` 로 활성화된 경우에만 실동작).
void startHotCorePolling();

/// dev 핫코어 상태 스냅샷 — JS `hotCoreStatus()` 표면의 원천.
struct HotCoreStatus {
  /// 핫 모드로 활성화됐는가(configureHotCore 에 비어 있지 않은 경로).
  bool enabled = false;
  /// 성공 스왑이 최소 1회 있었는가.
  bool swapped = false;
  /// 마지막 성공 스왑의 구/신 코어 계약 해시(스왑 전이면 빈 문자열).
  std::string oldHash;
  std::string newHash;
  /// 마지막 스왑 실패 사유(성공 스왑 시 비움).
  std::string error;
};

HotCoreStatus hotCoreStatusSnapshot();

} // namespace core

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

  /// 핫코어 스왑 직후 현재(신) 코어에 이벤트 싱크를 재등록한다 — 리스너를
  /// 보유 중일 때만. 폴링 스레드에서 호출되므로 리스너 맵은 건드리지 않고
  /// 원자 플래그만 읽는다(콜백은 C++ 정적 함수라 재사용 안전).
  void rebindEventSink();

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
  /// 리스너 보유 플래그 — 핫코어 스왑 재등록(rebindEventSink)이 폴링 스레드에서
  /// 읽는다. listeners_ 맵 자체는 JS 스레드 전용이라 여기서는 건드리지 않는다.
  std::atomic<bool> hasListeners_{false};
};

/// ChannelDispatcher: 채널 핸들별 Rust→JS 유니캐스트 회신 (2단계).
///
/// EventDispatcher(브로드캐스트) 와 동일한 큐+CallInvoker 마샬링을
/// 쓰지만, JS 콜백이 핸들별로 분리돼 있고 리로드 시 채널 테이블째
/// 폐기된다(채널은 호출 귀속 — 이전 런타임 대상 핸들은 무의미).
///
/// 핫코어 스왑 대응: 채널 핸들은 발급 코어에 귀속된다(새 dylib 의 핸들
/// 발급기는 새로 시작해 번호가 겹친다). drop 은 발급 코어의 channel_drop
/// 로 라우팅하고(`channelCores_` 소유권 기록), 스왑 직후에는 구 코어 발급
/// 채널의 레지스트리를 폐기한다 — 교차 코어 drop 오발(같은 번호의 신규
/// 채널을 해제하는 사고)을 원천 차단한다. 폐기는 "스왑 시 코어 내 상태
/// 소실" 설계 정책과 동일 선에서, 이벤트 싱크 재등록(rebindEventSink)의
/// 채널 대응이다.
///
/// createChannel(cb) 이 u32 핸들을 발급하면 JS 는 그 값을 커맨드 인자
/// `channel` 로 그대로 전달한다(TS 타입 ChannelHandle = number).
/// CallInvoker 가 없는 호스트는 `drainEvents()` 폴링으로 이 큐도 소비한다
/// (해당 HostFunction 이 EventDispatcher 와 본 디스패처를 함께 drain).
class ChannelDispatcher : public std::enable_shared_from_this<ChannelDispatcher> {
public:
  /// JS 스레드 마샬링용 CallInvoker 설정(EventDispatcher 와 동일 소스 공유).
  void setCallInvoker(std::shared_ptr<void> invoker);

  /// JS 콜백 등록 + Rust 채널 발급(JSON 경로 — 페이로드는 JSON 문자열).
  /// 반환값 = 채널 핸들(≥1). JS 스레드 호출.
  uint32_t create(facebook::jsi::Runtime& rt,
                  facebook::jsi::Function callback);
  /// 바이너리 채널 변형 — 콜백은 ArrayBuffer 를 받는다(복사본).
  uint32_t createBytes(facebook::jsi::Runtime& rt,
                       facebook::jsi::Function callback);
  /// 채널 해제(호출 완료/취소). 성공 true. JS 스레드 호출.
  bool drop(uint32_t handle);

  /// FFI C 콜백 — send 스레드에서 호출. 큐 적재 + drain 예약만.
  /// handle 은 발급 시 캡처된 채널 번호(핸들→JS 콜백 룩업 키).
  static void onChannelPayload(void* user_data, uint32_t handle, const char* payload);
  /// 바이너리 경로 C 콜백 — 페이로드는 (ptr, len), 콜백 반환 전까지만 유효.
  static void onChannelPayloadBytes(
    void* user_data, uint32_t handle, const uint8_t* payload, size_t payload_len);

  /// 큐의 모든 페이로드를 대응 핸들의 JS 콜백으로 전달. JS 스레드만.
  void drain(facebook::jsi::Runtime& rt);

  /// 미처리 프레임 수(JSON + 바이너리 합산 — JS 폴링/디버그용).
  size_t pendingCount();

  /// 리로드 대응: 보유 콜백·큐 폐기 및 Rust 채널 전부 drop. JS 스레드 호출.
  void reset();

  /// 핫코어 스왑 직후(폴링 스레드) 호출 — 구 코어 발급 채널의 폐기를
  /// 요청한다. 레지스트리(callbacks_/channelCores_)는 JS 스레드 전용이므로
  /// 여기서는 플래그만 세우고 drain 을 예약한다(실제 폐기는 drain 안에서).
  void requestResetAfterSwap();

private:
  void scheduleDrainLocked();

  /// 스왑 리셋의 실제 폐기 — drain(JS 스레드) 안에서 소비된다. 발급 코어가
  /// 현재 코어가 아닌(스왑으로 은퇴한) 채널만 drop 하고, 스왑 뒤 새 코어로
  /// 새로 만든 채널은 유지한다. FFI channel_drop 은 락 밖에서 호출된다.
  void dropStaleChannelsAfterSwap();

  /// 스왑 리셋 지연 플래그 — 폴링 스레드가 세우고 drain(JS 스레드)이 소비.
  std::atomic<bool> resetQueued_{false};
  std::mutex mutex_;
  /// (handle, payload) 큐 — onChannelPayload 가 적재, drain 이 소비.
  std::deque<std::pair<uint32_t, std::string>> queue_;
  /// 바이너리 큐 — onChannelPayloadBytes 가 적재. drop-oldest 정책 동일.
  std::deque<std::pair<uint32_t, std::vector<uint8_t>>> bytesQueue_;
  /// 바이너리 경로로 발급된 핸들 — drain 이 전달 형태를 고른다.
  std::unordered_set<uint32_t> bytesHandles_;
  size_t capacity_ = 1024;
  bool drainScheduled_ = false;
  std::shared_ptr<void> callInvoker_;
  /// 핸들별 JS 콜백 — drain 에서만 접근(JS 스레드).
  std::unordered_map<uint32_t, facebook::jsi::Function> callbacks_;
  /// 핸들 → 발급 코어 테이블 — drop/폐기를 올바른 코어의 channel_drop 로
  /// 라우팅하기 위한 소유권 기록. CoreTable 은 불변·무폐기라 스왑 뒤에도
  /// 포인터가 유효하다. callbacks_ 와 동일 수명주기로 함께 수정된다(JS 스레드).
  std::unordered_map<uint32_t, const core::CoreTable*> channelCores_;
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

// ── C++ TurboModule 상호운용 — jsi::Value 직접 typed invoke ─────────────
// 다른 C++ TurboModule/네이티브 코드가 JS 왕복 없이 rustra 정적 명령을
// 호출하는 진입점. JS 측 __rustraNative.invokeTyped* 와 동일 경로
// (encode → FFI → decode)를 공유한다 — HostFunction 이 이 함수들을 감싼다.
//
// 계약:
// - JS 런타임 스레드에서만 호출(jsi 스레드 친화성 — installRustraJSI 와 동일).
// - installRustraJSI 없이도 호출 가능 — FFI 전역 패키지(native_entry 등록)만
//   필요하다. JS 전역(__rustraNative) 설치 상태와 무관하다.
// - 예외를 던지지 않는다 — 결과는 status 로 구분한다(아래 enum 참고).

enum class TypedInvokeStatus {
  /// 성공 — value 에 디코딩된 출력이 담긴다.
  Ok,
  /// 정적 코덱 미보유 명령 — value 는 undefined. 호출자는 JS 엔진과 동일하게
  /// invokeFrame(Tier 2/3) 폴백을 선택할 수 있다.
  NoStaticCodec,
  /// Rust 명령 에러 — value 는 { code: string, message: string } 객체,
  /// message 필드는 "code: message" 결합 텍스트.
  CommandError,
  /// 응답 와이어 파손/디코딩 실패 — value 는 undefined, message 에 상세.
  MalformedResponse,
};

struct TypedInvokeResult {
  TypedInvokeStatus status;
  facebook::jsi::Value value;
  /// CommandError 는 "code: message" 결합 텍스트, MalformedResponse 는 상세.
  /// Ok/NoStaticCodec 은 빈 문자열.
  std::string message;
};

/// 이름 기반 typed invoke — encode_by_name → FFI → decode_by_name.
TypedInvokeResult invokeTypedByName(
  facebook::jsi::Runtime& rt,
  const std::string& commandName,
  const facebook::jsi::Value& args);

/// command_id(u16) 기반 typed invoke — encode_by_id → FFI → decode_by_id.
TypedInvokeResult invokeTypedById(
  facebook::jsi::Runtime& rt,
  uint16_t commandId,
  const facebook::jsi::Value& args);

} // namespace rustra
