// ── rustra generated ────────────────────────────────────────
// File:   rustra-generated-codecs.hpp
// Source: schema.json (single source of truth for this file)
// Regen:  rustra codegen --config rustra.json
// Stage:  schema → cpp codec renderer
// DO NOT EDIT — changes will be overwritten and fail codegen --check.
// ────────────────────────────────────────────────────────────
// C++ postcard codec for the RN JSI fast path (B1).
// C++는 postcard subset과 Set을 제외한 complex subset을 직접 인코딩/디코딩한다.
// Set을 포함한 complex 명령은 JS codec이 invokeRkyvV2로 전달하고, 동적 명령은
// JS Tier 3 fallback을 사용한다.
#pragma once

#include <cstddef>
#include <cstdint>
#include <jsi/jsi.h>
#include <string>
#include "rustra-codec.hpp"

namespace rustra::generated {

const facebook::jsi::PropNameID& cachedProp(facebook::jsi::Runtime& rt, const char* name);

facebook::jsi::Value make_array_buffer(facebook::jsi::Runtime& rt, const uint8_t* data, size_t size);

bool encode_by_name(facebook::jsi::Runtime& rt, const std::string& name, const facebook::jsi::Value& args, rustra::codec::Writer& w);
facebook::jsi::Value decode_by_name(facebook::jsi::Runtime& rt, const std::string& name, rustra::codec::Reader& r);

bool encode_by_id(facebook::jsi::Runtime& rt, uint16_t cmd_id, const facebook::jsi::Value& args, rustra::codec::Writer& w);
facebook::jsi::Value decode_by_id(facebook::jsi::Runtime& rt, uint16_t cmd_id, rustra::codec::Reader& r);

bool has_static_codec(const std::string& name);
bool has_static_codec_id(uint16_t cmd_id);
bool has_pos_codec(uint16_t cmd_id);
bool has_buffer_codec(uint16_t cmd_id);

facebook::jsi::Value decode_buffer_result_by_id(facebook::jsi::Runtime& rt, uint16_t cmd_id, facebook::jsi::Value buffer);
void encode_buffer_by_id(uint16_t cmd_id, const uint8_t* data, size_t size, rustra::codec::Writer& w);

void encode_pos_by_id(facebook::jsi::Runtime& rt, uint16_t cmd_id, const facebook::jsi::Value* argv, size_t argc, rustra::codec::Writer& w);

bool has_raw_codec(uint16_t cmd_id);
void encode_raw_slots(facebook::jsi::Runtime& rt, uint16_t cmd_id, const facebook::jsi::Value* argv, size_t argc, uint64_t* slots);
facebook::jsi::Value decode_raw_result(facebook::jsi::Runtime& rt, uint16_t cmd_id, uint64_t slot);

} // namespace rustra::generated
