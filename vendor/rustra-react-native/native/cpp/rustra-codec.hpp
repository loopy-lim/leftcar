// rustra-codec.hpp — 순수 C++ postcard wire codec (JSI 무의존).
//
// Rust(rkyv V2 typed postcard handler) 와 바이트-동일한 postcard 인코딩/디코딩을
// 제공한다. codegen 이 생성하는 per-command codec(rustra-generated-codecs.cpp)이
// 이 Reader/Writer 를 사용해 JSI Value <-> postcard 바이트 변환을 수행한다.
//
// postcard 포맷 (Rust `postcard` crate 호환):
//   - unsigned varint(LEB128): 길이/카운트 및 부호화 정수의 베이스.
//   - 부호 정수(i64): zigzag → varint. (정확한 64-bit; 기존 TS codec 의 32-bit 절단 한계 제거)
//   - f64/f32: little-endian 고정폭.
//   - bool: 1바이트(0/1).
//   - String/bytes: varint 길이 + UTF-8/raw.
//
// 헤더 전용(inline) — 별도 .cpp 없이 단일 컴파일로 단위 테스트 가능.

#pragma once

#include <algorithm>
#include <array>
#include <cstdint>
#include <cstring>
#include <limits>
#include <stdexcept>
#include <string>
#include <vector>

namespace rustra::codec {

// ── Writer ────────────────────────────────────────────────

/// postcard 바이트를 순차적으로 누적하는 라이터.
class Writer {
public:
  Writer() = default;

  const uint8_t* data() const {
    return using_heap_ ? heap_buf_.data() : inline_buf_.data();
  }

  size_t size() const { return size_; }

  /// 누적된 바이트를 반환(이동).
  /// 비동기 호출처럼 Writer 수명 밖에서 바이트를 보관할 때만 사용한다.
  std::vector<uint8_t> take() {
    std::vector<uint8_t> result;
    if (using_heap_) {
      result = std::move(heap_buf_);
      heap_buf_.clear();
      using_heap_ = false;
    } else {
      result.assign(inline_buf_.begin(), inline_buf_.begin() + size_);
    }
    size_ = 0;
    return result;
  }

  void push_u8(uint8_t v) {
    ensure_capacity(1);
    if (using_heap_) {
      heap_buf_.push_back(v);
    } else {
      inline_buf_[size_] = v;
    }
    ++size_;
  }

  /// 부호 없는 varint(LEB128). 길이/카운트용.
  void push_uvar(uint64_t n) {
    do {
      uint8_t b = static_cast<uint8_t>(n & 0x7f);
      n >>= 7;
      if (n != 0) b |= 0x80;
      push_u8(b);
    } while (n != 0);
  }

  /// 부호 있는 i64 → zigzag → varint. (정수 필드 공통)
  void push_i64(int64_t v) {
    uint64_t z = zigzag_encode(v);
    push_uvar(z);
  }

  /// f64 little-endian 고정폭.
  void push_f64(double v) {
    uint64_t bits;
    std::memcpy(&bits, &v, sizeof(bits));
    uint8_t bytes[8];
    for (int i = 0; i < 8; ++i) bytes[i] = static_cast<uint8_t>(bits >> (8 * i));
    push_bytes(bytes, sizeof(bytes));
  }

  /// f32 little-endian 고정폭.
  void push_f32(float v) {
    uint32_t bits;
    std::memcpy(&bits, &v, sizeof(bits));
    uint8_t bytes[4];
    for (int i = 0; i < 4; ++i) bytes[i] = static_cast<uint8_t>(bits >> (8 * i));
    push_bytes(bytes, sizeof(bytes));
  }

  /// bool → 1바이트(0/1).
  void push_bool(bool v) { push_u8(v ? 1 : 0); }

  /// UTF-8 문자열: varint 길이 + 바이트.
  void push_string(const std::string& s) {
    push_uvar(s.size());
    push_bytes(reinterpret_cast<const uint8_t*>(s.data()), s.size());
  }

  /// raw 바이트(길이 접두사 없음). 필요 시 호출측에서 길이 처리.
  void push_bytes(const uint8_t* data, size_t len) {
    if (len == 0) return;
    ensure_capacity(len);
    if (using_heap_) {
      heap_buf_.insert(heap_buf_.end(), data, data + len);
    } else {
      std::memcpy(inline_buf_.data() + size_, data, len);
    }
    size_ += len;
  }

  /// 연속 바이트를 호출자가 직접 채울 공간을 한 번에 확보한다. Vec<u8>를
  /// JSI Array에서 옮길 때 원소마다 ensure_capacity/분기하지 않도록 한다.
  uint8_t* append_uninitialized(size_t len) {
    if (len == 0) return nullptr;
    ensure_capacity(len);
    uint8_t* out;
    if (using_heap_) {
      const size_t start = heap_buf_.size();
      heap_buf_.resize(start + len);
      out = heap_buf_.data() + start;
    } else {
      out = inline_buf_.data() + size_;
    }
    size_ += len;
    return out;
  }

private:
  static constexpr size_t kInlineCapacity = 128;
  // inline_buf_ 는 의도적으로 미초기화다 — 모든 읽기 경로(data()/take())가
  // size_ 미만만 다루고 push_* 는 항상 쓸 위치부터 기록한다. 128B zero-init
  // 매 호출 memset 은 positional 요청(대부분 ≤32B)의 핫패스에서 순수 낭비다.
  std::array<uint8_t, kInlineCapacity> inline_buf_;
  std::vector<uint8_t> heap_buf_;
  size_t size_ = 0;
  bool using_heap_ = false;

  void ensure_capacity(size_t additional) {
    if (additional > std::numeric_limits<size_t>::max() - size_) {
      throw std::length_error("postcard writer size overflows");
    }
    const size_t needed = size_ + additional;
    if (!using_heap_ && needed <= kInlineCapacity) return;

    if (!using_heap_) {
      heap_buf_.reserve(std::max(kInlineCapacity * 2, needed));
      heap_buf_.insert(heap_buf_.end(), inline_buf_.begin(), inline_buf_.begin() + size_);
      using_heap_ = true;
      return;
    }

    if (needed > heap_buf_.capacity()) {
      heap_buf_.reserve(std::max(heap_buf_.capacity() * 2, needed));
    }
  }

  /// i64 → uint64 zigzag. (n << 1) ^ (n >> 63), 산술 시프트.
  static uint64_t zigzag_encode(int64_t v) {
    return (static_cast<uint64_t>(v) << 1) ^ static_cast<uint64_t>(v >> 63);
  }
};

// ── Reader ────────────────────────────────────────────────

/// postcard 바이트에서 순차적으로 읽는 리더. 초과 읽기 시 runtime_error.
class Reader {
public:
  struct StringView {
    const uint8_t* data;
    size_t size;
  };

  const uint8_t* data;
  size_t len;
  size_t pos;

  Reader(const uint8_t* d, size_t l) : data(d), len(l), pos(0) {}

  bool eof() const { return pos >= len; }

  uint8_t read_u8() {
    require(1);
    return data[pos++];
  }

  /// 부호 없는 varint(LEB128). 최대 10바이트(i64 범위).
  /// 10바이트째 마지막 바이트는 0x00/0x01 만 허용한다(Rust postcard
  /// max_of_last_byte = 2^(64%7)−1 = 1, TS _pcDecodeVarint64 와 동일 계약) —
  /// 64비트 초과 인코딩은 무음 왜곡 대신 throw.
  uint64_t read_uvar() {
    uint64_t value = 0;
    int shift = 0;
    for (int i = 0; i < 10; ++i) {
      require(1);
      uint8_t b = data[pos++];
      value |= static_cast<uint64_t>(b & 0x7f) << shift;
      if ((b & 0x80) == 0) {
        if (i == 9 && (b & 0x7f) > 0x01) {
          throw std::runtime_error("postcard varint exceeds 64 bits");
        }
        return value;
      }
      shift += 7;
    }
    throw std::runtime_error("postcard varint too long");
  }

  /// i64 (zigzag varint). JS Number(double) 호환 반환.
  int64_t read_i64() {
    uint64_t z = read_uvar();
    return zigzag_decode(z);
  }

  /// f64 little-endian.
  double read_f64() {
    require(8);
    uint64_t bits = 0;
    for (int i = 0; i < 8; ++i) bits |= static_cast<uint64_t>(data[pos + i]) << (8 * i);
    pos += 8;
    double v;
    std::memcpy(&v, &bits, sizeof(v));
    return v;
  }

  /// f32 little-endian.
  float read_f32() {
    require(4);
    uint32_t bits = 0;
    for (int i = 0; i < 4; ++i) bits |= static_cast<uint32_t>(data[pos + i]) << (8 * i);
    pos += 4;
    float v;
    std::memcpy(&v, &bits, sizeof(v));
    return v;
  }

  /// bool(1바이트, 0/1).
  bool read_bool() {
    uint8_t b = read_u8();
    return b != 0;
  }

  /// UTF-8 문자열: varint 길이 + 바이트.
  StringView read_string_view() {
    uint64_t n = read_uvar();
    if (n > static_cast<uint64_t>(len - pos)) {
      throw std::runtime_error("postcard string length overflows");
    }
    StringView view{data + pos, static_cast<size_t>(n)};
    pos += view.size;
    return view;
  }

  /// 소유 문자열이 필요한 에러 처리 등 비-hot 경로용.
  std::string read_string() {
    const auto view = read_string_view();
    return std::string(reinterpret_cast<const char*>(view.data), view.size);
  }

  /// 현재 커서(읽지 않은 첫 바이트). Vec<u8> 등 원시 복사用.
  const uint8_t* cur() const { return data + pos; }

  /// n 바이트 건너뛰기 — cur() 로 복사한 뒤 호출.
  void skip(size_t n) {
    require(n);
    pos += n;
  }

  /// n바이트를 한 번만 bounds-check하고 뷰로 반환한다.
  StringView read_bytes_view(size_t n) {
    require(n);
    StringView view{data + pos, n};
    pos += n;
    return view;
  }

private:
  void require(size_t n) const {
    if (pos + n > len) throw std::runtime_error("postcard read past end");
  }

  /// uint64 zigzag → i64. (z >> 1) ^ -(z & 1).
  static int64_t zigzag_decode(uint64_t z) {
    return static_cast<int64_t>((z >> 1) ^ (0u - (z & 1u)));
  }
};

} // namespace rustra::codec
