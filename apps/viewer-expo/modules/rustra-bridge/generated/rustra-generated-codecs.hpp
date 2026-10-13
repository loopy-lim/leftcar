// ── rustra generated ────────────────────────────────────────
// File:   rustra-generated-codecs.hpp
// Source: schema.json (single source of truth for this file)
// Regen:  rustra codegen --config rustra.json
// Stage:  schema → cpp codec renderer
// DO NOT EDIT — changes will be overwritten and fail codegen --check.
// ────────────────────────────────────────────────────────────
// C++ postcard codec for the RN JSI fast path (B1).
// C++는 postcard subset과 Set을 제외한 complex subset을 직접 인코딩/디코딩한다.
// Set을 포함한 complex 명령은 JS codec이 invokeFrame로 전달하고, 동적 명령은
// JS Tier 3 fallback을 사용한다.
#pragma once

#include <cstddef>
#include <cstdint>
#include <memory>
#include <jsi/jsi.h>
#include <string>
#include "rustra-codec.hpp"

// Optional adapter capability: old generated consumers omit this marker.
#define RUSTRA_GENERATED_CODEC_CONTRACT_IDENTITY 1
#define RUSTRA_GENERATED_BOUND_CODEC_CONTEXT 1

namespace rustra::generated {

const char* compiled_contract_hash();

// Internal bound codec handles belong to this runtime. Retain only in managed
// HostFunction captures or synchronous locals; never global/async owners or another runtime.
struct BoundCodecContext;
std::shared_ptr<const BoundCodecContext> make_bound_codec_context(facebook::jsi::Runtime& rt, uint16_t commandId);
bool encode_bound(facebook::jsi::Runtime& rt, const BoundCodecContext& context, const facebook::jsi::Value& args, rustra::codec::Writer& w);
facebook::jsi::Value decode_bound(facebook::jsi::Runtime& rt, const BoundCodecContext& context, rustra::codec::Reader& r);
facebook::jsi::Value decode_raw_bound(facebook::jsi::Runtime& rt, const BoundCodecContext& context, uint64_t slot);
facebook::jsi::Value decode_buffer_bound(facebook::jsi::Runtime& rt, const BoundCodecContext& context, facebook::jsi::Value buffer);

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
