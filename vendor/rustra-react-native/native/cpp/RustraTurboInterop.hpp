#pragma once

// C++ TurboModule 상호운용 — folly::dynamic 진입점(선택 헤더).
//
// RustraJSIBridge.hpp 의 jsi::Value 기반 진입점(invokeTypedByName/ById) 은
// 코어 헤더에 folly 의존을 두지 않기 위해 dynamic 오버로드을 여기에 둔다.
// 구현은 RustraJSIBridge.cpp 에 있다(번들 링크). folly 는 ReactCommon 가
// 이미 의존하므로 RN 네이티브 빌드 환경에서 추가 비용이 없다.

#include <folly/dynamic.h>
#include <jsi/jsi.h>
#include <string>

#include "RustraJSIBridge.hpp"

namespace rustra {

/// folly::dynamic → typed invoke (이름 기반). 계약은 jsi::Value 오버로드와
/// 동일 — dynamic 을 jsi::Value 로 변환한 뒤 동일 경로로 인코딩한다.
/// int64 중 2^53 을 넘는 값은 double 로 손실 전달된다(jsi 표면 한계).
TypedInvokeResult invokeTypedByNameDynamic(
  facebook::jsi::Runtime& rt,
  const std::string& commandName,
  const folly::dynamic& args);

/// folly::dynamic → typed invoke (command_id 기반).
TypedInvokeResult invokeTypedByIdDynamic(
  facebook::jsi::Runtime& rt,
  uint16_t commandId,
  const folly::dynamic& args);

} // namespace rustra
