// ── rustra generated ────────────────────────────────────────
// File:   rustra-generated-codecs.cpp
// Source: schema.json (single source of truth for this file)
// Regen:  rustra codegen --config rustra.json
// Stage:  schema → cpp codec renderer
// DO NOT EDIT — changes will be overwritten and fail codegen --check.
// ────────────────────────────────────────────────────────────
// C++ postcard codec for the RN JSI fast path (B1).
#include "rustra-generated-codecs.hpp"
#include <cmath>
#include <cstring>
#include <jsi/jsi.h>
#include <limits>
#include <memory>
#include <stdexcept>
#include <string>
#include <unordered_map>
#include <utility>

using namespace facebook::jsi;
namespace jsi = facebook::jsi;
namespace rc = rustra::codec;

namespace rustra { namespace generated {
#ifdef RUSTRA_TEST_JSI_SHIM
  using RuntimePropNameCache = std::unordered_map<std::string, jsi::PropNameID>;
  std::shared_ptr<RuntimePropNameCache> runtimePropNameCache(jsi::Runtime&) {
    static auto cache = std::make_shared<RuntimePropNameCache>();
    return cache;
  }
#else
  class RuntimePropNameCache final : public jsi::NativeState {
  public:
    std::unordered_map<std::string, jsi::PropNameID> values;
  };
  std::shared_ptr<RuntimePropNameCache> runtimePropNameCache(jsi::Runtime& rt) {
    static std::unordered_map<jsi::Runtime*, std::weak_ptr<RuntimePropNameCache>> caches;
    auto found = caches.find(&rt);
    if (found != caches.end()) {
      if (auto cache = found->second.lock()) return cache;
    }
    auto cache = std::make_shared<RuntimePropNameCache>();
    jsi::Object holder(rt);
    holder.setNativeState(rt, cache);
    rt.global().setProperty(rt, "__rustraPropNameCache", std::move(holder));
    caches[&rt] = cache;
    return cache;
  }
#endif
  const jsi::PropNameID& cachedProp(jsi::Runtime& rt, const char* name) {
    auto cache = runtimePropNameCache(rt);
#ifdef RUSTRA_TEST_JSI_SHIM
    auto& values = *cache;
#else
    auto& values = cache->values;
#endif
    auto it = values.find(name);
    if (it == values.end()) {
      it = values.emplace(name, jsi::PropNameID::forAscii(rt, name)).first;
    }
    return it->second;
  }
}}

[[maybe_unused]] static double rustra_f64(jsi::Runtime& rt, const jsi::Value& value, const char* field) {
  if (!value.isNumber()) throw jsi::JSError(rt, std::string("rustra: '") + field + "' must be a number");
  double number = value.asNumber();
  if (!std::isfinite(number)) throw jsi::JSError(rt, std::string("rustra: '") + field + "' must be finite");
  return number;
}
[[maybe_unused]] static int64_t rustra_i64(jsi::Runtime& rt, const jsi::Value& value, const char* field) {
  if (value.isBigInt()) return value.asBigInt(rt).asInt64(rt);
  double number = rustra_f64(rt, value, field);
  constexpr double maxSafe = 9007199254740991.0;
  if (std::trunc(number) != number || number < -maxSafe || number > maxSafe)
    throw jsi::JSError(rt, std::string("rustra: '") + field + "' must be a safe integer or bigint");
  return static_cast<int64_t>(number);
}
[[maybe_unused]] static uint64_t rustra_u64(jsi::Runtime& rt, const jsi::Value& value, const char* field) {
  if (value.isBigInt()) return value.asBigInt(rt).asUint64(rt);
  double number = rustra_f64(rt, value, field);
  constexpr double maxSafe = 9007199254740991.0;
  if (std::trunc(number) != number || number < 0.0 || number > maxSafe)
    throw jsi::JSError(rt, std::string("rustra: '") + field + "' must be a non-negative safe integer or bigint");
  return static_cast<uint64_t>(number);
}
[[maybe_unused]] static uint8_t rustra_u8(jsi::Runtime& rt, const jsi::Value& value, const char* field) {
  if (!value.isNumber()) throw jsi::JSError(rt, std::string("rustra: '") + field + "' must be a number");
  double number = value.asNumber();
  if (!(number >= 0.0 && number <= 255.0))
    throw jsi::JSError(rt, std::string("rustra: '") + field + "' must be an integer in 0..255");
  uint8_t byte = static_cast<uint8_t>(number);
  if (static_cast<double>(byte) != number)
    throw jsi::JSError(rt, std::string("rustra: '") + field + "' must be an integer in 0..255");
  return byte;
}
struct RustraByteSpan { const uint8_t* data; size_t size; };
[[maybe_unused]] static RustraByteSpan rustra_bytes(jsi::Runtime& rt, const jsi::Value& value, const char* field) {
  if (!value.isObject())
    throw jsi::JSError(rt, std::string("rustra: '") + field + "' must be a one-byte TypedArray, ArrayBuffer, or number[]");
  auto object = value.asObject(rt);
  if (object.isArrayBuffer(rt)) {
    auto buffer = object.getArrayBuffer(rt);
    auto size = buffer.length(rt);
    auto* data = buffer.data(rt);
    if (size > 0 && data == nullptr)
      throw jsi::JSError(rt, std::string("rustra: '") + field + "' has detached ArrayBuffer storage");
    return {data, size};
  }
  auto bytesPerElement = object.getProperty(rt, "BYTES_PER_ELEMENT");
  auto bufferValue = object.getProperty(rt, "buffer");
  auto offsetValue = object.getProperty(rt, "byteOffset");
  auto lengthValue = object.getProperty(rt, "byteLength");
  if (!bytesPerElement.isNumber() || bytesPerElement.asNumber() != 1.0 || !bufferValue.isObject() || !bufferValue.asObject(rt).isArrayBuffer(rt) || !offsetValue.isNumber() || !lengthValue.isNumber())
    throw jsi::JSError(rt, std::string("rustra: '") + field + "' must be a one-byte TypedArray or ArrayBuffer");
  auto buffer = bufferValue.asObject(rt).getArrayBuffer(rt);
  auto bufferSize = buffer.length(rt);
  double offsetNumber = offsetValue.asNumber();
  double lengthNumber = lengthValue.asNumber();
  if (!std::isfinite(offsetNumber) || !std::isfinite(lengthNumber) || std::trunc(offsetNumber) != offsetNumber || std::trunc(lengthNumber) != lengthNumber || offsetNumber < 0.0 || lengthNumber < 0.0 || offsetNumber > static_cast<double>(bufferSize) || lengthNumber > static_cast<double>(bufferSize) - offsetNumber)
    throw jsi::JSError(rt, std::string("rustra: '") + field + "' view is outside its ArrayBuffer");
  auto offset = static_cast<size_t>(offsetNumber);
  auto size = static_cast<size_t>(lengthNumber);
  auto* data = buffer.data(rt);
  if (bufferSize > 0 && data == nullptr)
    throw jsi::JSError(rt, std::string("rustra: '") + field + "' has detached TypedArray storage");
  return {size == 0 ? data : data + offset, size};
}
[[maybe_unused]] static float rustra_f32(jsi::Runtime& rt, const jsi::Value& value, const char* field) {
  double number = rustra_f64(rt, value, field);
  if (number < -std::numeric_limits<float>::max() || number > std::numeric_limits<float>::max())
    throw jsi::JSError(rt, std::string("rustra: '") + field + "' is outside the f32 range");
  return static_cast<float>(number);
}


static void encode_addNumbers(jsi::Runtime& rt, const jsi::Value& args, rc::Writer& w) {
  w.push_u8(1); w.push_u8(0); // cmd_id = 1 LE
  auto argsObj = args.asObject(rt);
  w.push_i64(rustra_i64(rt, argsObj.getProperty(rt, "a"), "a"));
  w.push_i64(rustra_i64(rt, argsObj.getProperty(rt, "b"), "b"));
}

// (Tier 1 positional) 개별 인자 → 직접 인코딩. argsObj 경유 대비 JSI 프로퍼티 조회 2회 제거.
static void encode_pos_addNumbers(jsi::Runtime& rt, const jsi::Value* argv, size_t argc, rc::Writer& w) {
  if (argc != 2) throw jsi::JSError(rt, "rustra: addNumbers expects 2 positional argument(s), got " + std::to_string(argc));
  w.push_u8(1); w.push_u8(0); // cmd_id = 1 LE
  w.push_i64(rustra_i64(rt, argv[0], "a"));
  w.push_i64(rustra_i64(rt, argv[1], "b"));
}

static jsi::Value decode_addNumbers(jsi::Runtime& rt, rc::Reader& r) {
  auto resultObj = jsi::Object(rt);
  resultObj.setProperty(rt, rustra::generated::cachedProp(rt, "value"), [&]() -> jsi::Value { auto _v = r.read_i64(); if (_v >= -9007199254740991ll && _v <= 9007199254740991ll) return jsi::Value(static_cast<double>(_v)); return jsi::Value(rt, jsi::BigInt::fromInt64(rt, _v)); }());
  return std::move(resultObj);
}

namespace rustra::generated {

bool encode_by_name(Runtime& rt, const std::string& name, const Value& args, rc::Writer& w) {
  if (name == "addNumbers") { encode_addNumbers(rt, args, w); return true; }
  return false; // 동적 명령 — JS 가 Tier 3 fallback 처리
}

Value decode_by_name(Runtime& rt, const std::string& name, rc::Reader& r) {
  if (name == "addNumbers") return decode_addNumbers(rt, r);
  throw JSError(rt, "rustra: no C++ codec for '" + name + "'");
}

bool encode_by_id(Runtime& rt, uint16_t cmd_id, const Value& args, rc::Writer& w) {
  switch (cmd_id) {
    case 1: encode_addNumbers(rt, args, w); return true;
    default: return false; // 동적/알 수 없는 cmd_id — JS 가 Tier 3 fallback 처리
  }
}

Value decode_by_id(Runtime& rt, uint16_t cmd_id, rc::Reader& r) {
  switch (cmd_id) {
    case 1: return decode_addNumbers(rt, r);
    default: throw JSError(rt, "rustra: no C++ codec for cmd_id " + std::to_string(cmd_id));
  }
}

bool has_static_codec(const std::string& name) {
  if (name == "addNumbers") return true;
  return false;
}

bool has_static_codec_id(uint16_t cmd_id) {
  switch (cmd_id) {
    case 1: return true;
    default: return false;
  }
}

/// (Tier 1) positional 인자를 직접 인코딩 가능한 cmd_id 집합 — JS 폴백 판별용.
bool has_pos_codec(uint16_t cmd_id) {
  if (cmd_id == 1) return true;
  return false;
}

/// (Tier 1) 개별 Value 인자 → postcard 바이트. 명령별 코덱이 argc를 정확히 검증한다.
void encode_pos_by_id(jsi::Runtime& rt, uint16_t cmd_id, const jsi::Value* argv, size_t argc, rc::Writer& w) {
  switch (cmd_id) {
    case 1: encode_pos_addNumbers(rt, argv, argc, w); return;
    default: throw JSError(rt, "rustra: no positional codec for cmd_id " + std::to_string(cmd_id));
  }
}

bool has_buffer_codec(uint16_t cmd_id) {
  switch (cmd_id) {

    default: return false;
  }
}

void encode_buffer_by_id(uint16_t cmd_id, const uint8_t* data, size_t size, rc::Writer& w) {
  if (size > 0 && data == nullptr) throw std::invalid_argument("rustra: null byte buffer");
  switch (cmd_id) {
    default: throw std::invalid_argument("rustra: no buffer codec for cmd_id " + std::to_string(cmd_id));
  }
}

Value decode_buffer_result_by_id(Runtime& rt, uint16_t cmd_id, Value buffer) {
  switch (cmd_id) {
    default: throw JSError(rt, "rustra: no buffer result codec for cmd_id " + std::to_string(cmd_id));
  }
}

bool has_raw_codec(uint16_t cmd_id) {
  switch (cmd_id) {
    case 1: return true;
    default: return false;
  }
}

void encode_raw_slots(Runtime& rt, uint16_t cmd_id, const Value* argv, size_t argc, uint64_t* slots) {
  switch (cmd_id) {
    case 1: {
      if (argc != 2) throw JSError(rt, "rustra: addNumbers expects 2 raw argument(s), got " + std::to_string(argc));
      { int64_t value = rustra_i64(rt, argv[0], "a"); std::memcpy(&slots[0], &value, sizeof(value)); }
      { int64_t value = rustra_i64(rt, argv[1], "b"); std::memcpy(&slots[1], &value, sizeof(value)); }
      return;
    }
    default: throw JSError(rt, "rustra: no raw input codec for cmd_id " + std::to_string(cmd_id));
  }
}

Value decode_raw_result(Runtime& rt, uint16_t cmd_id, uint64_t slot) {
  switch (cmd_id) {
    case 1: {
      Object result(rt);
      int64_t value; std::memcpy(&value, &slot, sizeof(value));
      result.setProperty(rt, cachedProp(rt, "value"), value >= -9007199254740991ll && value <= 9007199254740991ll ? jsi::Value(static_cast<double>(value)) : jsi::Value(rt, jsi::BigInt::fromInt64(rt, value)));
      return std::move(result);
    }
    default: throw JSError(rt, "rustra: no raw result codec for cmd_id " + std::to_string(cmd_id));
  }
}

} // namespace rustra::generated
