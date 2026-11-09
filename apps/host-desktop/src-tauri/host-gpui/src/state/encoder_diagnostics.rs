//! 인코더 진단 뷰 파생(src/encoderDiagnostics.ts 포팅). 세션 통계를
//! 인스펙터 행 문자열(mode/가속, preset·profile, 적용/미지원/거부 속성,
//! 실험 프로필 + QP 상세, 압력, in-flight, 유효 출력 FPS, 세로 분할 7
//! 필드)로 바꾼다. "Measuring" 원칙: 측정 전 값은 조작하지 않고 측정 중
//! 문구로 보인다.
//!
//! 문안은 packages/ui-tokens i18n.ts의 `host.inspector` 키와 동일한
//! 문장을 이 모듈 표로 옮겨 담는다(lib 크레이트는 TS 패키지를 못 읽는다).
//! T-3의 i18n.rs가 이 표를 재사용·확장한다.
//!
//! 테스트는 encoderDiagnostics.test.ts의 케이스를 옮긴 것 + React
//! renderToStaticMarkup 두 케이스를 라벨 표 검증으로 분해한 것이다(React
//! 컴포넌트는 T-3 UI 범위라 뷰 데이터와 라벨 표로 증명을 나눴다).

use super::session_row::SessionRow;
use leftcar_host_desktop::settings::HostLanguage;

/// host.inspector 표 중 encoderDiagnosticsView가 쓰는 키 한 벌.
#[derive(Debug, Clone, Copy)]
pub struct InspectorStrings {
    pub measuring: &'static str,
    pub auto: &'static str,
    pub hardware: &'static str,
    pub software: &'static str,
    pub acceleration_checking: &'static str,
    pub encoder_mode_unknown: &'static str,
    pub encoder_id_checking: &'static str,
    pub preset_checking: &'static str,
    pub profile_checking: &'static str,
    pub none_value: &'static str,
    pub unsupported_value: &'static str,
    pub rejected_value: &'static str,
    pub qp_not_applied: &'static str,
    pub qp_measuring: &'static str,
    pub qp_adjustments: &'static str,
    pub qp_adjustments_measuring: &'static str,
    pub drops_count: &'static str,
    pub submit_p95_value: &'static str,
    pub callback_p95_value: &'static str,
    pub encoder_word: &'static str,
    pub packetization_word: &'static str,
    pub base_qp_basis: &'static str,
    pub joined_word: &'static str,
    pub mismatch_word: &'static str,
    pub experiment_rate_control: &'static str,
    pub experiment_adaptive_qp: &'static str,
    pub experiment_encoder_pool: &'static str,
    pub experiment_split_vertical: &'static str,
    pub requested_value: &'static str,
    pub applied_value: &'static str,
}

/// 인스펙터 행의 섹션 라벨(T-3 UI가 쓴다 — 원본 React 렌더 테스트가
/// 화면 문구로 고정하던 값들).
pub const LABELS: &[(&str, &str, &str)] = &[
    // (키, ko, en) — ui-tokens i18n.ts host.inspector와 같은 문장.
    ("experimentProfileLabel", "실험 프로필", "Experiment profile"),
    ("encoderPressureLabel", "인코더 압력", "Encoder pressure"),
    ("inFlightLabel", "인코더/패킷화 in-flight", "Encoder/packetization in-flight"),
    ("validOutputFpsLabel", "유효 출력 FPS", "Valid output FPS"),
    ("tileOutputLabel", "타일 출력/표시", "Tile output/display"),
    ("tileSyncLabel", "타일 동기화", "Tile sync"),
    ("splitPreparationLabel", "분할 준비 / pair callback p95", "Split preparation / pair callback p95"),
    ("splitFlowLabel", "분할 flow", "Split flow"),
    ("splitRecoveryLabel", "분할 복구", "Split recovery"),
    ("fixedQualityPercent", "{percent}% 고정", "Fixed at {percent}%"),
];

pub fn inspector_strings(language: HostLanguage) -> InspectorStrings {
    match language {
        HostLanguage::Ko => InspectorStrings {
            measuring: "측정 중",
            auto: "자동",
            hardware: "하드웨어",
            software: "소프트웨어",
            acceleration_checking: "가속 확인 중",
            encoder_mode_unknown: "미확인",
            encoder_id_checking: "인코더 ID 확인 중",
            preset_checking: "preset 확인 중",
            profile_checking: "profile 확인 중",
            none_value: "없음",
            unsupported_value: "미지원 {value}",
            rejected_value: "거부 {value}",
            qp_not_applied: "QP 적용 안 됨",
            qp_measuring: "QP 측정 중",
            qp_adjustments: "QP {qp} ({count}회 조정)",
            qp_adjustments_measuring: "QP {qp} (조정 횟수 측정 중)",
            drops_count: "드롭 {count}",
            submit_p95_value: "제출 p95 {value}",
            callback_p95_value: "callback p95 {value}",
            encoder_word: "인코더",
            packetization_word: "패킷화",
            base_qp_basis: "Base QP 기반",
            joined_word: "결합",
            mismatch_word: "불일치",
            experiment_rate_control: "레이트 컨트롤",
            experiment_adaptive_qp: "적응형 QP",
            experiment_encoder_pool: "인코더 풀",
            experiment_split_vertical: "4K 수직 분할",
            requested_value: "요청 {value}",
            applied_value: "적용 {value}",
        },
        HostLanguage::En => InspectorStrings {
            measuring: "Measuring",
            auto: "Auto",
            hardware: "Hardware",
            software: "Software",
            acceleration_checking: "checking acceleration",
            encoder_mode_unknown: "unknown",
            encoder_id_checking: "identifying encoder",
            preset_checking: "identifying preset",
            profile_checking: "identifying profile",
            none_value: "None",
            unsupported_value: "unsupported {value}",
            rejected_value: "rejected {value}",
            qp_not_applied: "QP not applied",
            qp_measuring: "QP measuring",
            qp_adjustments: "QP {qp} ({count} adjustments)",
            qp_adjustments_measuring: "QP {qp} (adjustments measuring)",
            drops_count: "{count} drops",
            submit_p95_value: "submit p95 {value}",
            callback_p95_value: "callback p95 {value}",
            encoder_word: "encoder",
            packetization_word: "packetization",
            base_qp_basis: "Base QP-based",
            joined_word: "joined",
            mismatch_word: "mismatch",
            experiment_rate_control: "Rate control",
            experiment_adaptive_qp: "Adaptive QP",
            experiment_encoder_pool: "Encoder pool",
            experiment_split_vertical: "4K vertical split",
            requested_value: "requested {value}",
            applied_value: "applied {value}",
        },
    }
}

/// ui-tokens의 interpolate({key}) 치환과 같은 규칙.
pub fn interpolate(template: &str, values: &[(&str, String)]) -> String {
    let mut out = template.to_string();
    for (key, value) in values {
        out = out.replace(&format!("{{{key}}}"), value);
    }
    out
}

/// 원본 EncoderDiagnosticsView와 같은 행 모음(읽기 전용 진단).
#[derive(Debug, Clone, PartialEq)]
pub struct EncoderDiagnosticsView {
    pub path: String,
    pub identity: String,
    pub configuration: String,
    pub applied: String,
    pub unavailable: String,
    pub fallback: Option<String>,
    pub experiment: String,
    pub experiment_detail: String,
    pub experiment_fallback: String,
    pub pressure: String,
    pub in_flight: String,
    pub valid_output_fps: String,
    pub quality_basis: Option<String>,
    pub has_encoder_pressure: bool,
    pub has_experiment_fallback: bool,
    pub split_encode: Option<String>,
    pub split_render: Option<String>,
    pub split_sync: Option<String>,
    pub split_loss: Option<String>,
    pub split_latency: Option<String>,
    pub split_flow: Option<String>,
    pub split_recovery: Option<String>,
}

/// TS number의 Number.isFinite 게이트.
fn finite_number(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite())
}

fn measured_integer(value: Option<f64>, measuring: &str) -> String {
    match finite_number(value) {
        None => measuring.to_string(),
        Some(measured) => format!("{}", measured.trunc() as i64),
    }
}

fn measured_milliseconds(value: Option<f64>, measuring: &str) -> String {
    match finite_number(value) {
        None => measuring.to_string(),
        Some(measured) => format!("{:.1}ms", measured / 1000.0),
    }
}

fn experiment_id(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|value| !value.is_empty())
}

fn experiment_label(value: Option<&str>, labels: &ExperimentLabels, measuring: &str) -> String {
    match value {
        None => measuring.to_string(),
        Some(value) => labels.for_id(value).unwrap_or(value).to_string(),
    }
}

struct ExperimentLabels {
    auto: &'static str,
    rate_control: &'static str,
    adaptive_qp: &'static str,
    encoder_pool: &'static str,
    split_vertical: &'static str,
}

impl ExperimentLabels {
    fn for_id(&self, value: &str) -> Option<&'static str> {
        match value {
            "auto" => Some(self.auto),
            "rateControl" => Some(self.rate_control),
            "adaptiveQp" => Some(self.adaptive_qp),
            "encoderPool" => Some(self.encoder_pool),
            "splitVertical" => Some(self.split_vertical),
            _ => None,
        }
    }
}

fn experiment_labels(t: &InspectorStrings) -> ExperimentLabels {
    ExperimentLabels {
        auto: t.auto,
        rate_control: t.experiment_rate_control,
        adaptive_qp: t.experiment_adaptive_qp,
        encoder_pool: t.experiment_encoder_pool,
        split_vertical: t.experiment_split_vertical,
    }
}

fn encoder_mode_label(mode: Option<&str>, t: &InspectorStrings) -> String {
    match mode {
        Some("ave") => "AVE".into(),
        Some("rtvc") => "RTVC".into(),
        Some("nvenc") => "NVENC".into(),
        _ => t.encoder_mode_unknown.to_string(),
    }
}

fn acceleration_label(accelerated: Option<bool>, t: &InspectorStrings) -> String {
    match accelerated {
        Some(true) => t.hardware.to_string(),
        Some(false) => t.software.to_string(),
        None => t.acceleration_checking.to_string(),
    }
}

/// baseFrameQp: null(적용 안 됨)과 부재(측정 전)가 다른 문장이 된다.
fn qp_detail(session: &SessionRow, t: &InspectorStrings) -> String {
    match session.base_frame_qp {
        Some(None) => t.qp_not_applied.to_string(),
        Some(Some(qp)) => {
            let qp = format!("{}", qp.trunc() as i64);
            match finite_number(session.base_frame_qp_changes) {
                None => interpolate(t.qp_adjustments_measuring, &[("qp", qp)]),
                Some(changes) => interpolate(
                    t.qp_adjustments,
                    &[("qp", qp), ("count", format!("{}", changes.trunc() as i64))],
                ),
            }
        }
        None => t.qp_measuring.to_string(),
    }
}

pub fn encoder_diagnostics_view(
    session: &SessionRow,
    language: HostLanguage,
) -> EncoderDiagnosticsView {
    let t = inspector_strings(language);
    let experiment_diagnostics_available = session.encoder_experiment_diagnostics_available == Some(true);
    let mode = encoder_mode_label(session.encoder_mode.as_deref(), &t);
    let acceleration = acceleration_label(session.encoder_hardware_accelerated, &t);

    let mut unavailable_parts: Vec<String> = Vec::new();
    if let Some(properties) = &session.encoder_unsupported_properties {
        if !properties.is_empty() {
            unavailable_parts.push(interpolate(
                t.unsupported_value,
                &[("value", properties.join(", "))],
            ));
        }
    }
    if let Some(properties) = &session.encoder_rejected_properties {
        if !properties.is_empty() {
            unavailable_parts.push(interpolate(
                t.rejected_value,
                &[("value", properties.join(", "))],
            ));
        }
    }
    let unavailable = unavailable_parts.join(" · ");

    let requested_experiment = experiment_diagnostics_available
        .then(|| experiment_id(&session.encoder_experiment_requested))
        .flatten();
    let applied_experiment = experiment_diagnostics_available
        .then(|| experiment_id(&session.encoder_experiment_applied))
        .flatten();
    let experiment_fallback = if experiment_diagnostics_available {
        experiment_id(&session.encoder_experiment_fallback_reason)
            .unwrap_or(t.none_value)
            .to_string()
    } else {
        t.measuring.to_string()
    };
    let encoder_drops = experiment_diagnostics_available
        .then(|| finite_number(session.encoder_frame_drops))
        .flatten();
    let valid_output_fps = experiment_diagnostics_available
        .then(|| finite_number(session.valid_encode_output_fps))
        .flatten();
    let is_split =
        applied_experiment == Some("splitVertical") || session.split_direction.as_deref() == Some("vertical");

    let labels = experiment_labels(&t);
    let has_experiment_fallback =
        experiment_diagnostics_available && experiment_fallback != t.none_value;
    let experiment_detail = format!(
        "{} · {} · {}",
        interpolate(
            t.requested_value,
            &[(
                "value",
                requested_experiment
                    .map(Into::into)
                    .unwrap_or_else(|| t.measuring.to_string())
            )],
        ),
        interpolate(
            t.applied_value,
            &[(
                "value",
                applied_experiment
                    .map(Into::into)
                    .unwrap_or_else(|| t.measuring.to_string())
            )],
        ),
        if experiment_diagnostics_available {
            qp_detail(session, &t)
        } else {
            t.qp_measuring.to_string()
        },
    );
    let pressure = format!(
        "{} · {} · {}",
        interpolate(t.drops_count, &[("count", measured_integer(encoder_drops, t.measuring))]),
        interpolate(
            t.submit_p95_value,
            &[(
                "value",
                measured_milliseconds(
                    experiment_diagnostics_available
                        .then_some(session.encode_submit_call_p95_us)
                        .flatten(),
                    t.measuring,
                )
            )],
        ),
        interpolate(
            t.callback_p95_value,
            &[(
                "value",
                measured_milliseconds(
                    experiment_diagnostics_available
                        .then_some(session.encoder_callback_p95_us)
                        .flatten(),
                    t.measuring,
                )
            )],
        ),
    );

    EncoderDiagnosticsView {
        path: format!("{mode} · {acceleration}"),
        identity: match session.encoder_id.as_deref() {
            Some(id) if !id.is_empty() => id.to_string(),
            _ => t.encoder_id_checking.to_string(),
        },
        configuration: format!(
            "{} · {}",
            session
                .encoder_preset
                .as_deref()
                .filter(|preset| !preset.is_empty())
                .unwrap_or(t.preset_checking),
            session
                .encoder_profile
                .as_deref()
                .filter(|profile| !profile.is_empty())
                .unwrap_or(t.profile_checking),
        ),
        applied: match session.encoder_applied_properties.as_ref() {
            Some(properties) if !properties.is_empty() => properties.join(", "),
            _ => t.none_value.to_string(),
        },
        unavailable: if unavailable.is_empty() {
            t.none_value.to_string()
        } else {
            unavailable
        },
        fallback: session.encoder_fallback_reason.clone(),
        experiment: experiment_label(
            applied_experiment.or(requested_experiment),
            &labels,
            t.measuring,
        ),
        experiment_detail,
        experiment_fallback,
        pressure,
        in_flight: format!(
            "{} {} · {} {}",
            t.encoder_word,
            measured_integer(
                experiment_diagnostics_available.then_some(session.encode_in_flight).flatten(),
                t.measuring,
            ),
            t.packetization_word,
            measured_integer(
                experiment_diagnostics_available
                    .then_some(session.packetization_in_flight)
                    .flatten(),
                t.measuring,
            ),
        ),
        valid_output_fps: match valid_output_fps {
            None => t.measuring.to_string(),
            Some(fps) => format!("{} FPS", fps.trunc() as i64),
        },
        quality_basis: (experiment_diagnostics_available && applied_experiment == Some("adaptiveQp"))
            .then(|| t.base_qp_basis.to_string()),
        has_encoder_pressure: encoder_drops.is_some_and(|drops| drops > 0.0),
        has_experiment_fallback,
        split_encode: is_split.then(|| {
            format!(
                "L {} / R {} FPS",
                measured_integer(session.left_valid_encode_output_fps, t.measuring),
                measured_integer(session.right_valid_encode_output_fps, t.measuring),
            )
        }),
        split_render: is_split.then(|| {
            format!(
                "L {} / R {} / {} {} FPS",
                measured_integer(session.left_rendered_fps, t.measuring),
                measured_integer(session.right_rendered_fps, t.measuring),
                t.joined_word,
                measured_integer(session.joined_rendered_fps, t.measuring),
            )
        }),
        split_sync: is_split.then(|| {
            format!(
                "p95 {} · max {} · timeout {} · {} {}",
                measured_milliseconds(session.pair_ready_delta_p95_us, t.measuring),
                measured_milliseconds(session.pair_ready_delta_max_us, t.measuring),
                measured_integer(session.pair_sync_timeouts, t.measuring),
                t.mismatch_word,
                measured_integer(session.unmatched_output_drops, t.measuring),
            )
        }),
        split_loss: is_split.then(|| {
            format!(
                "L {} / R {}",
                measured_integer(session.left_receiver_loss, t.measuring),
                measured_integer(session.right_receiver_loss, t.measuring),
            )
        }),
        split_latency: is_split.then(|| {
            format!(
                "{} / {}",
                measured_milliseconds(session.split_preparation_p95_us, t.measuring),
                measured_milliseconds(session.encoded_pair_callback_p95_us, t.measuring),
            )
        }),
        split_flow: is_split.then(|| {
            format!(
                "lease {}/{} · queue {} · oldest {}",
                measured_integer(session.split_flow_active_leases, t.measuring),
                measured_integer(session.split_flow_capacity, t.measuring),
                measured_integer(session.split_encoded_queue_depth, t.measuring),
                measured_milliseconds(session.split_encoded_queue_oldest_us, t.measuring),
            )
        }),
        split_recovery: is_split.then(|| {
            format!(
                "capture {} · boundary {} · post-encode {} · wire {}/{} · gap IDR {} / delta {}",
                measured_integer(session.split_pre_encode_admission_drops, t.measuring),
                measured_integer(session.split_recovery_boundary_discards, t.measuring),
                measured_integer(session.split_post_encode_delta_drops, t.measuring),
                measured_integer(session.split_wire_pairs_attempted, t.measuring),
                measured_integer(session.split_wire_pair_send_failures, t.measuring),
                measured_integer(session.split_keyframe_gap_recoveries, t.measuring),
                measured_integer(session.split_delta_gap_recoveries, t.measuring),
            )
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn session(value: serde_json::Value) -> SessionRow {
        serde_json::from_value(value).unwrap()
    }

    fn base() -> serde_json::Value {
        // encoderDiagnostics.test.ts의 base session.
        json!({
            "session": 7, "sourceIndex": 0, "sourceName": "Built-in Display",
            "viewerAddr": "192.168.0.20:5001", "state": "running",
            "fps": 60, "kbps": 4200, "inputEnabled": true, "inputRateHz": 60,
            "encoderMode": "ave",
            "encoderID": "com.apple.videotoolbox.videoencoder.ave.avc",
            "encoderHardwareAccelerated": true,
            "encoderPreset": "high-speed",
            "encoderProfile": "main",
            "encoderAppliedProperties": ["HighSpeed", "Quality"],
            "encoderUnsupportedProperties": ["SuggestedLookAheadFrameCount"],
            "encoderRejectedProperties": ["Quality=-12900"],
            "encoderFallbackReason": null
        })
    }

    fn adaptive() -> serde_json::Value {
        // encoderDiagnostics.test.ts의 adaptiveSession.
        let mut value = base();
        let object = value.as_object_mut().unwrap();
        object.insert("encoderExperimentDiagnosticsAvailable".into(), json!(true));
        object.insert("encoderExperimentRequested".into(), json!("adaptiveQp"));
        object.insert("encoderExperimentApplied".into(), json!("adaptiveQp"));
        object.insert("encoderExperimentFallbackReason".into(), json!(null));
        object.insert("encoderFrameDrops".into(), json!(3));
        object.insert("encoderFrameDropFps".into(), json!(1));
        object.insert("validEncodeOutputFps".into(), json!(58));
        object.insert("encodeSubmitCallP50Us".into(), json!(8_600));
        object.insert("encodeSubmitCallP95Us".into(), json!(17_200));
        object.insert("encoderCallbackP50Us".into(), json!(9_100));
        object.insert("encoderCallbackP95Us".into(), json!(18_100));
        object.insert("encodeInFlight".into(), json!(0));
        object.insert("packetizationInFlight".into(), json!(1));
        object.insert("baseFrameQp".into(), json!(34));
        object.insert("baseFrameQpChanges".into(), json!(2));
        value
    }

    // 원본: keeps the actual AVE identity and property outcomes visible
    #[test]
    fn keeps_the_actual_ave_identity_and_property_outcomes_visible() {
        let view = encoder_diagnostics_view(&session(base()), HostLanguage::Ko);
        assert_eq!(view.path, "AVE · 하드웨어");
        assert_eq!(view.identity, "com.apple.videotoolbox.videoencoder.ave.avc");
        assert_eq!(view.configuration, "high-speed · main");
        assert_eq!(view.applied, "HighSpeed, Quality");
        assert_eq!(
            view.unavailable,
            "미지원 SuggestedLookAheadFrameCount · 거부 Quality=-12900"
        );
        assert_eq!(view.fallback, None);
    }

    // 원본: labels the Linux NVENC hardware encoder
    #[test]
    fn labels_the_linux_nvenc_hardware_encoder() {
        let mut value = base();
        let object = value.as_object_mut().unwrap();
        object.insert("encoderMode".into(), json!("nvenc"));
        object.insert("encoderID".into(), json!("nvenc"));
        object.insert("encoderPreset".into(), json!("p4-ultraLowLatency"));
        object.insert("encoderProfile".into(), json!("baseline"));
        object.insert("encoderAppliedProperties".into(), json!([]));
        object.insert("encoderUnsupportedProperties".into(), json!([]));
        object.insert("encoderRejectedProperties".into(), json!([]));
        let view = encoder_diagnostics_view(&session(value), HostLanguage::Ko);
        assert_eq!(view.path, "NVENC · 하드웨어");
        assert_eq!(view.identity, "nvenc");
        assert_eq!(view.configuration, "p4-ultraLowLatency · baseline");
        assert_eq!(view.applied, "없음");
        assert_eq!(view.unavailable, "없음");
    }

    // 원본: shows the applied adaptive QP profile and true encoder pressure
    #[test]
    fn shows_the_applied_adaptive_qp_profile_and_true_encoder_pressure() {
        let view = encoder_diagnostics_view(&session(adaptive()), HostLanguage::Ko);
        assert_eq!(view.experiment, "적응형 QP");
        assert_eq!(
            view.experiment_detail,
            "요청 adaptiveQp · 적용 adaptiveQp · QP 34 (2회 조정)"
        );
        assert_eq!(view.pressure, "드롭 3 · 제출 p95 17.2ms · callback p95 18.1ms");
        assert_eq!(view.in_flight, "인코더 0 · 패킷화 1");
        assert_eq!(view.valid_output_fps, "58 FPS");
        assert_eq!(view.quality_basis.as_deref(), Some("Base QP 기반"));
    }

    // 원본 it.each: maps the known experiment label
    #[test]
    fn maps_the_known_experiment_labels() {
        let cases = [
            ("auto", "자동"),
            ("rateControl", "레이트 컨트롤"),
            ("adaptiveQp", "적응형 QP"),
            ("encoderPool", "인코더 풀"),
            ("splitVertical", "4K 수직 분할"),
        ];
        for (experiment, expected) in cases {
            let mut value = adaptive();
            value.as_object_mut().unwrap()
                .insert("encoderExperimentApplied".into(), json!(experiment));
            let view = encoder_diagnostics_view(&session(value), HostLanguage::Ko);
            assert_eq!(view.experiment, expected, "experiment: {experiment}");
        }
    }

    // 원본: keeps the existing quality wording outside adaptive QP
    #[test]
    fn keeps_the_existing_quality_wording_outside_adaptive_qp() {
        let mut value = adaptive();
        value.as_object_mut().unwrap()
            .insert("encoderExperimentApplied".into(), json!("rateControl"));
        let view = encoder_diagnostics_view(&session(value), HostLanguage::Ko);
        assert_eq!(view.quality_basis, None);
    }

    // 원본: preserves unknown requested and applied experiment IDs
    #[test]
    fn preserves_unknown_requested_and_applied_experiment_ids() {
        let mut value = adaptive();
        let object = value.as_object_mut().unwrap();
        object.insert("encoderExperimentRequested".into(), json!("futureRequested"));
        object.insert("encoderExperimentApplied".into(), json!("futureApplied"));
        let view = encoder_diagnostics_view(&session(value), HostLanguage::Ko);
        assert_eq!(view.experiment, "futureApplied");
        assert_eq!(
            view.experiment_detail,
            "요청 futureRequested · 적용 futureApplied · QP 34 (2회 조정)"
        );
    }

    // 원본: shows measuring states for old-shim defaults when diagnostics
    // are unavailable
    #[test]
    fn shows_measuring_states_when_diagnostics_are_unavailable() {
        let value = json!({
            "session": 7, "sourceIndex": 0, "sourceName": "s",
            "viewerAddr": "a", "state": "running", "fps": 60, "kbps": 1,
            "inputEnabled": true, "inputRateHz": 60,
            "encoderMode": "ave", "encoderID": "com.apple.videotoolbox.videoencoder.ave.avc",
            "encoderHardwareAccelerated": true,
            "encoderPreset": "high-speed", "encoderProfile": "main",
            "encoderExperimentDiagnosticsAvailable": false,
            "encoderExperimentRequested": "auto",
            "encoderExperimentApplied": "rateControl",
            "encoderExperimentFallbackReason": null,
            "encoderFrameDrops": 0,
            "validEncodeOutputFps": 0,
            "encodeSubmitCallP95Us": 0,
            "encoderCallbackP95Us": 0,
            "encodeInFlight": 0,
            "packetizationInFlight": 0,
            "baseFrameQp": null,
            "baseFrameQpChanges": 0
        });
        let view = encoder_diagnostics_view(&session(value), HostLanguage::Ko);
        assert_eq!(view.experiment, "측정 중");
        assert_eq!(view.experiment_detail, "요청 측정 중 · 적용 측정 중 · QP 측정 중");
        assert_eq!(view.experiment_fallback, "측정 중");
        assert_eq!(view.pressure, "드롭 측정 중 · 제출 p95 측정 중 · callback p95 측정 중");
        assert_eq!(view.in_flight, "인코더 측정 중 · 패킷화 측정 중");
        assert_eq!(view.valid_output_fps, "측정 중");
        assert_eq!(view.quality_basis, None);
        assert!(!view.has_encoder_pressure);
        assert!(!view.has_experiment_fallback);
    }

    // 원본: renders legitimate zero measurements when diagnostics are
    // available
    #[test]
    fn renders_legitimate_zero_measurements_when_available() {
        let mut value = adaptive();
        let object = value.as_object_mut().unwrap();
        object.insert("encoderExperimentRequested".into(), json!("rateControl"));
        object.insert("encoderExperimentApplied".into(), json!("rateControl"));
        object.insert("encoderFrameDrops".into(), json!(0));
        object.insert("validEncodeOutputFps".into(), json!(0));
        object.insert("encodeSubmitCallP95Us".into(), json!(0));
        object.insert("encoderCallbackP95Us".into(), json!(0));
        object.insert("encodeInFlight".into(), json!(0));
        object.insert("packetizationInFlight".into(), json!(0));
        object.insert("baseFrameQp".into(), json!(null));
        object.insert("baseFrameQpChanges".into(), json!(0));
        let view = encoder_diagnostics_view(&session(value), HostLanguage::Ko);
        assert_eq!(view.experiment, "레이트 컨트롤");
        assert_eq!(
            view.experiment_detail,
            "요청 rateControl · 적용 rateControl · QP 적용 안 됨"
        );
        assert_eq!(view.experiment_fallback, "없음");
        assert_eq!(view.pressure, "드롭 0 · 제출 p95 0.0ms · callback p95 0.0ms");
        assert_eq!(view.in_flight, "인코더 0 · 패킷화 0");
        assert_eq!(view.valid_output_fps, "0 FPS");
        assert_eq!(view.quality_basis, None);
        assert!(!view.has_encoder_pressure);
        assert!(!view.has_experiment_fallback);
    }

    // 원본: shows no experiment fallback when the Host reports none
    #[test]
    fn shows_no_experiment_fallback_when_the_host_reports_none() {
        let view = encoder_diagnostics_view(&session(adaptive()), HostLanguage::Ko);
        assert_eq!(view.experiment_fallback, "없음");
    }

    // 원본: shows an explicit experiment fallback independently from
    // encoder identity fallback
    #[test]
    fn shows_an_explicit_experiment_fallback_independently() {
        let mut value = adaptive();
        let object = value.as_object_mut().unwrap();
        object.insert(
            "encoderExperimentFallbackReason".into(),
            json!("RTVC adaptive QP unavailable"),
        );
        object.insert("encoderFallbackReason".into(), json!("AVE identity unavailable"));
        let view = encoder_diagnostics_view(&session(value), HostLanguage::Ko);
        assert_eq!(view.experiment_fallback, "RTVC adaptive QP unavailable");
        assert_eq!(view.fallback.as_deref(), Some("AVE identity unavailable"));
        assert!(view.has_experiment_fallback);
    }

    // 원본: never folds network or recovery loss into encoder drops
    #[test]
    fn never_folds_network_or_recovery_loss_into_encoder_drops() {
        let mut value = adaptive();
        let object = value.as_object_mut().unwrap();
        object.insert("encoderFrameDrops".into(), json!(0));
        object.insert("networkDropped".into(), json!(99));
        object.insert("networkQueueDropped".into(), json!(88));
        object.insert("recoveryFramesDropped".into(), json!(77));
        let view = encoder_diagnostics_view(&session(value), HostLanguage::Ko);
        assert_eq!(view.pressure, "드롭 0 · 제출 p95 17.2ms · callback p95 18.1ms");
        assert!(!view.has_encoder_pressure);
    }

    // 원본 렌더 케이스의 뷰 절반: renders the experiment and
    // encoder-pressure rows as read-only diagnostics — 컴포넌트 문구는
    // 아래 labels_match_the_korean_inspector_rows가 증명하고, 수동 화질
    // 상한("40% 고정")이 진단 뷰에 없다는 사실은 필드 구성으로 증명한다.
    #[test]
    fn diagnostics_rows_carry_only_read_only_experiment_data() {
        let view = encoder_diagnostics_view(&session(adaptive()), HostLanguage::Ko);
        assert_eq!(view.experiment, "적응형 QP");
        assert_eq!(view.experiment_fallback, "없음");
        assert!(!view.has_experiment_fallback);
        assert_eq!(view.pressure, "드롭 3 · 제출 p95 17.2ms · callback p95 18.1ms");
        assert_eq!(view.in_flight, "인코더 0 · 패킷화 1");
        assert_eq!(view.valid_output_fps, "58 FPS");
    }

    // 원본 렌더 케이스의 문구 절반: 세션 카드 본체의 수동 화질 상한 문구와
    // 인스펙터 행 라벨이 i18n 표에 그대로 있다.
    #[test]
    fn labels_match_the_korean_inspector_rows() {
        for (key, ko, _en) in LABELS {
            let expected = match *key {
                "experimentProfileLabel" => "실험 프로필",
                "encoderPressureLabel" => "인코더 압력",
                "inFlightLabel" => "인코더/패킷화 in-flight",
                "validOutputFpsLabel" => "유효 출력 FPS",
                "tileOutputLabel" => "타일 출력/표시",
                "tileSyncLabel" => "타일 동기화",
                "splitPreparationLabel" => "분할 준비 / pair callback p95",
                "splitFlowLabel" => "분할 flow",
                "splitRecoveryLabel" => "분할 복구",
                "fixedQualityPercent" => "{percent}% 고정",
                other => unreachable!("unknown label key: {other}"),
            };
            assert_eq!(*ko, expected, "label: {key}");
        }
    }

    // 원본: shows both tile pipelines and pair synchronization for a
    // split stream
    #[test]
    fn shows_both_tile_pipelines_and_pair_synchronization_for_a_split_stream() {
        let mut value = adaptive();
        let object = value.as_object_mut().unwrap();
        object.insert("encoderExperimentRequested".into(), json!("splitVertical"));
        object.insert("encoderExperimentApplied".into(), json!("splitVertical"));
        object.insert("splitDirection".into(), json!("vertical"));
        object.insert("leftValidEncodeOutputFps".into(), json!(60));
        object.insert("rightValidEncodeOutputFps".into(), json!(59));
        object.insert("leftRenderedFps".into(), json!(60));
        object.insert("rightRenderedFps".into(), json!(59));
        object.insert("joinedRenderedFps".into(), json!(59));
        object.insert("leftReceiverLoss".into(), json!(2));
        object.insert("rightReceiverLoss".into(), json!(3));
        object.insert("pairReadyDeltaP95Us".into(), json!(2_300));
        object.insert("pairReadyDeltaMaxUs".into(), json!(9_800));
        object.insert("pairSyncTimeouts".into(), json!(1));
        object.insert("unmatchedOutputDrops".into(), json!(4));
        object.insert("splitPreparationP95Us".into(), json!(1_800));
        object.insert("encodedPairCallbackP95Us".into(), json!(41_200));
        object.insert("splitFlowActiveLeases".into(), json!(3));
        object.insert("splitFlowCapacity".into(), json!(5));
        object.insert("splitPreEncodeAdmissionDrops".into(), json!(8));
        object.insert("splitEncodedQueueDepth".into(), json!(1));
        object.insert("splitEncodedQueueOldestUs".into(), json!(12_300));
        object.insert("splitRecoveryBoundaryDiscards".into(), json!(4));
        object.insert("splitPostEncodeDeltaDrops".into(), json!(0));
        object.insert("splitWirePairsAttempted".into(), json!(600));
        object.insert("splitWirePairSendFailures".into(), json!(0));
        object.insert("splitKeyframeGapRecoveries".into(), json!(1));
        object.insert("splitDeltaGapRecoveries".into(), json!(1));
        let view = encoder_diagnostics_view(&session(value), HostLanguage::Ko);
        assert_eq!(view.experiment, "4K 수직 분할");
        assert_eq!(view.split_encode.as_deref(), Some("L 60 / R 59 FPS"));
        assert_eq!(view.split_render.as_deref(), Some("L 60 / R 59 / 결합 59 FPS"));
        assert_eq!(
            view.split_sync.as_deref(),
            Some("p95 2.3ms · max 9.8ms · timeout 1 · 불일치 4")
        );
        assert_eq!(view.split_loss.as_deref(), Some("L 2 / R 3"));
        assert_eq!(view.split_latency.as_deref(), Some("1.8ms / 41.2ms"));
        assert_eq!(
            view.split_flow.as_deref(),
            Some("lease 3/5 · queue 1 · oldest 12.3ms")
        );
        assert_eq!(
            view.split_recovery.as_deref(),
            Some("capture 8 · boundary 4 · post-encode 0 · wire 600/0 · gap IDR 1 / delta 1")
        );
    }

    // 파생: 세로 분할은 applied 실험 ID만으로도 켜진다(splitDirection이
    // 없어도) — 원본 isSplit의 OR 조건.
    #[test]
    fn split_rows_turn_on_from_the_applied_experiment_alone() {
        let mut value = adaptive();
        value.as_object_mut().unwrap()
            .insert("encoderExperimentApplied".into(), json!("splitVertical"));
        let view = encoder_diagnostics_view(&session(value), HostLanguage::Ko);
        assert!(view.split_encode.is_some());
        assert!(view.split_recovery.is_some());
    }

    // 파생: en 문안 — 같은 데이터가 같은 구조의 영어 문장이 된다.
    #[test]
    fn english_view_uses_the_same_structure() {
        let view = encoder_diagnostics_view(&session(adaptive()), HostLanguage::En);
        assert_eq!(view.path, "AVE · Hardware");
        assert_eq!(view.experiment, "Adaptive QP");
        assert_eq!(
            view.experiment_detail,
            "requested adaptiveQp · applied adaptiveQp · QP 34 (2 adjustments)"
        );
        assert_eq!(view.pressure, "3 drops · submit p95 17.2ms · callback p95 18.1ms");
        assert_eq!(view.in_flight, "encoder 0 · packetization 1");
        assert_eq!(view.valid_output_fps, "58 FPS");
        assert_eq!(view.quality_basis.as_deref(), Some("Base QP-based"));
        assert_eq!(view.unavailable, "unsupported SuggestedLookAheadFrameCount · rejected Quality=-12900");
        assert_eq!(view.applied, "HighSpeed, Quality");
    }

    // interpolate: ui-tokens 규칙과 같은 {key} 치환.
    #[test]
    fn interpolate_replaces_named_placeholders() {
        assert_eq!(
            interpolate("QP {qp} ({count}회 조정)", &[("qp", "34".into()), ("count", "2".into())]),
            "QP 34 (2회 조정)"
        );
        assert_eq!(interpolate("미지원 {value}", &[("value", "X".into())]), "미지원 X");
    }

    // measured 헬퍼: 비유한수는 측정 중으로 떨어진다(Number.isFinite 선례).
    #[test]
    fn non_finite_values_fall_back_to_measuring() {
        assert_eq!(measured_integer(Some(f64::NAN), "측정 중"), "측정 중");
        assert_eq!(measured_milliseconds(Some(f64::INFINITY), "측정 중"), "측정 중");
        assert_eq!(measured_integer(None, "측정 중"), "측정 중");
        assert_eq!(measured_integer(Some(17.9), "측정 중"), "17");
        assert_eq!(measured_milliseconds(Some(17_200.0), "측정 중"), "17.2ms");
    }
}
