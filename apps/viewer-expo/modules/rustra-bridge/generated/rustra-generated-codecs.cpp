// ── rustra generated ────────────────────────────────────────
// File:   rustra-generated-codecs.cpp
// Source: schema.json (single source of truth for this file)
// Regen:  rustra codegen --config rustra.json
// Stage:  schema → cpp codec renderer
// DO NOT EDIT — changes will be overwritten and fail codegen --check.
// ────────────────────────────────────────────────────────────
// C++ postcard codec for the RN JSI fast path (B1).
#include "rustra-generated-codecs.hpp"
#include <algorithm>
#include <array>
#include <cmath>
#include <cstring>
#include <jsi/jsi.h>
#include <initializer_list>
#include <limits>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

using namespace facebook::jsi;
namespace jsi = facebook::jsi;
namespace rc = rustra::codec;
namespace rustra::generated {
const char* compiled_contract_hash() { return "be754a50f3177d068356707641c86b80ebcaf7d08930823fe7afc46cc354cf37"; }
}

// Each invocation owns its captured map values until canonical encoding finishes.
// Small maps avoid a vector allocation; larger maps retain bounded reservation.
class RustraMapEntries {
public:
  using Entry = std::pair<std::string, jsi::Value>;
  RustraMapEntries(jsi::Runtime& rt, size_t count) : use_inline_(count <= inline_entries_.size()) {
    if (count > heap_entries_.max_size()) throw jsi::JSError(rt, "rustra: map size exceeds native capacity");
    if (!use_inline_) heap_entries_.reserve(std::min<size_t>(count, 64));
  }
  void emplace_back(std::string&& key, jsi::Value&& value) {
    if (use_inline_) inline_entries_[size_] = Entry(std::move(key), std::move(value));
    else heap_entries_.emplace_back(std::move(key), std::move(value));
    ++size_;
  }
  Entry* begin() { return use_inline_ ? inline_entries_.data() : heap_entries_.data(); }
  Entry* end() { return begin() + size_; }
  size_t size() const { return size_; }
private:
  std::array<Entry, 4> inline_entries_;
  std::vector<Entry> heap_entries_;
  size_t size_ = 0;
  bool use_inline_;
};

// UTF8 may replace lone UTF16 surrogates. Preserve the prior normalized lookup
// for replacement-containing keys; ordinary keys retain their original JSI string.
[[maybe_unused]] static jsi::Value rustra_map_value(jsi::Runtime& rt, const jsi::Object& object, const jsi::String& key, const std::string& utf8) {
  if (utf8.find("\xEF\xBF\xBD") == std::string::npos) return object.getProperty(rt, key);
  return object.getProperty(rt, jsi::String::createFromUtf8(rt, reinterpret_cast<const uint8_t*>(utf8.data()), utf8.size()));
}

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

namespace rustra::generated {
struct BoundCodecContext {
  const uint16_t commandId;
  const std::vector<jsi::PropNameID> input;
  const std::vector<jsi::PropNameID> output;
private:
  static std::vector<jsi::PropNameID> ownNames(Runtime& rt, std::initializer_list<const char*> names) {
    std::vector<jsi::PropNameID> result; result.reserve(names.size());
    for (auto name : names) result.push_back(jsi::PropNameID::forAscii(rt, name));
    return result;
  }
  BoundCodecContext(Runtime& rt, uint16_t id, std::initializer_list<const char*> in, std::initializer_list<const char*> out)
    : commandId(id), input(ownNames(rt, in)), output(ownNames(rt, out)) {}
  friend std::shared_ptr<const BoundCodecContext> make_bound_codec_context(Runtime&, uint16_t);
};
std::shared_ptr<const BoundCodecContext> make_bound_codec_context(Runtime& rt, uint16_t commandId) {
  switch (commandId) {
    case 1: return std::shared_ptr<const BoundCodecContext>(new BoundCodecContext(rt, commandId, {"a", "b"}, {"value"}));
    default: throw JSError(rt, "rustra: no bound codec for cmd_id " + std::to_string(commandId));
  }
}
} // namespace rustra::generated


static void encode_body_addNumbers(jsi::Runtime& rt, const jsi::Object& argsObj, rc::Writer& w, const jsi::PropNameID* _properties) {
  const auto& _prop_0 = _properties[0];
  const auto& _prop_1 = _properties[1];
  w.push_i64(rustra_i64(rt, argsObj.getProperty(rt, _prop_0), "a"));
  w.push_i64(rustra_i64(rt, argsObj.getProperty(rt, _prop_1), "b"));
}
static void encode_addNumbers(jsi::Runtime& rt, const jsi::Value& args, rc::Writer& w) {
  w.push_u8(1); w.push_u8(0); // cmd_id = 1 LE
  auto argsObj = args.asObject(rt);
  const std::array<jsi::PropNameID, 2> _properties{jsi::PropNameID::forAscii(rt, "a"), jsi::PropNameID::forAscii(rt, "b")};
  encode_body_addNumbers(rt, argsObj, w, _properties.data());
}

// (Tier 1 positional) 개별 인자 → 직접 인코딩. argsObj 경유 대비 JSI 프로퍼티 조회 2회 제거.
static void encode_pos_addNumbers(jsi::Runtime& rt, const jsi::Value* argv, size_t argc, rc::Writer& w) {
  if (argc != 2) throw jsi::JSError(rt, "rustra: addNumbers expects 2 positional argument(s), got " + std::to_string(argc));
  w.push_u8(1); w.push_u8(0); // cmd_id = 1 LE
  w.push_i64(rustra_i64(rt, argv[0], "a"));
  w.push_i64(rustra_i64(rt, argv[1], "b"));
}

static jsi::Value decode_body_addNumbers(jsi::Runtime& rt, jsi::Object& resultObj, rc::Reader& r, const jsi::PropNameID* _properties) {
  const auto& _prop_0 = _properties[0];
  resultObj.setProperty(rt, _prop_0, [&]() -> jsi::Value { auto _v = r.read_i64(); if (_v >= -9007199254740991ll && _v <= 9007199254740991ll) return jsi::Value(static_cast<double>(_v)); return jsi::Value(rt, jsi::BigInt::fromInt64(rt, _v)); }());
  return std::move(resultObj);
}
static jsi::Value decode_addNumbers(jsi::Runtime& rt, rc::Reader& r) {
  auto resultObj = jsi::Object(rt);
  const std::array<jsi::PropNameID, 1> _properties{jsi::PropNameID::forAscii(rt, "value")};
  return decode_body_addNumbers(rt, resultObj, r, _properties.data());
}

namespace rustra::generated {

bool encode_bound(Runtime& rt, const BoundCodecContext& context, const Value& args, rc::Writer& w) {
  switch (context.commandId) {
    case 1: {
      w.push_u8(1); w.push_u8(0);
      auto argsObj = args.asObject(rt);
      encode_body_addNumbers(rt, argsObj, w, context.input.data()); return true;
    }
    default: return false;
  }
}

Value decode_bound(Runtime& rt, const BoundCodecContext& context, rc::Reader& r) {
  switch (context.commandId) {
    case 1: {
      auto resultObj = jsi::Object(rt);
      return decode_body_addNumbers(rt, resultObj, r, context.output.data());
    }
    default: throw JSError(rt, "rustra: no bound codec");
  }
}

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

Value decode_buffer_bound(Runtime& rt, const BoundCodecContext& context, Value buffer) {
  switch (context.commandId) {
    default: throw JSError(rt, "rustra: no bound buffer result codec");
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
      result.setProperty(rt, jsi::PropNameID::forAscii(rt, "value"), value >= -9007199254740991ll && value <= 9007199254740991ll ? jsi::Value(static_cast<double>(value)) : jsi::Value(rt, jsi::BigInt::fromInt64(rt, value)));
      return std::move(result);
    }
    default: throw JSError(rt, "rustra: no raw result codec for cmd_id " + std::to_string(cmd_id));
  }
}

Value decode_raw_bound(Runtime& rt, const BoundCodecContext& context, uint64_t slot) {
  switch (context.commandId) {
    case 1: {
      Object result(rt);
      int64_t value; std::memcpy(&value, &slot, sizeof(value));
      result.setProperty(rt, context.output[0], value >= -9007199254740991ll && value <= 9007199254740991ll ? jsi::Value(static_cast<double>(value)) : jsi::Value(rt, jsi::BigInt::fromInt64(rt, value)));
      return std::move(result);
    }
    default: throw JSError(rt, "rustra: no bound raw result codec");
  }
}

} // namespace rustra::generated
