//! 호스트 설정 영속화(파일 공유 게이트 등). `data_dir/leftcar-host/settings.json`
//! (0600)에 저장하며, 없으면 기본값으로 만든다. 읽지 못한 설정은
//! 승인 토글을 끈 채 시작하되 원본을 보존하고 변경 저장을 거부한다.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// 실험 스위치(2026-09-17 인터랙티브 페이싱 계획의 A/B 노브). `None`은
/// 미설정이고 shim의 기본 동작을 그대로 쓴다. 호스트가 시작 때 프로세스
/// 환경변수로 주입하고 shim은 스트림 시작마다 읽으므로([`experiment_env_vars`]),
/// 변경은 다음 스트림부터 적용된다. 범위 제한은 shim이 최종 클램프하지만
/// UI도 같은 범위만 입력받는다(EncoderPolicy.swift·UdpFramePacingBudget.swift).
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ExperimentSettings {
    /// 인코더 in-flight 상한(1–5). 미설정=해상도 유도 정책값.
    #[serde(rename = "maxEncodeInFlight", skip_serializing_if = "Option::is_none")]
    pub max_encode_in_flight: Option<u32>,
    /// UDP 큐 최대 나이 valve(ms). 0/미설정=끔.
    #[serde(rename = "queueMaxAgeMs", skip_serializing_if = "Option::is_none")]
    pub queue_max_age_ms: Option<u32>,
    /// 미디어 소켓 SO_SNDBUF(64KiB–2MiB). 미설정=512KiB.
    #[serde(rename = "sndbufBytes", skip_serializing_if = "Option::is_none")]
    pub sndbuf_bytes: Option<u32>,
    /// DataRateLimits 버스트 윈도(50–1000ms). 미설정=1000ms(1초). 초 단위
    /// 환경변수 형식(예: 0.25)으로 변환해 주입한다.
    #[serde(rename = "drlWindowMs", skip_serializing_if = "Option::is_none")]
    pub drl_window_ms: Option<u32>,
    /// 프레임 페이싱 예산 비율(30–100%). 미설정=80%.
    #[serde(rename = "pacingBudgetPct", skip_serializing_if = "Option::is_none")]
    pub pacing_budget_pct: Option<u32>,
    /// 프레임별 전송 trace(FRAME_TRACE). 진단용 로그라 기본 끔.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub frame_trace: bool,
}

/// 승인 토글 성격의 호스트 설정. 개인정보·캡처에 닿는 토글은 모두 기본
/// 꺼짐이며 `wan_access`(외부 접속 허용)도 예외가 아니다(2026-09-20) —
/// 공인 인터넷에 포트를 노출하는 행위라 기본은 꺼짐이고, 외부 접속의 정식
/// 경로는 Tailscale 같은 오버레이 네트워크다. 켜면 라우터 포트 매핑이 UPnP로
/// 등록되고 종료 때 제거되며 토글로 즉시 끌 수 있다.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HostSettings {
    pub clipboard_share: bool,
    pub file_share: bool,
    /// 커튼 모드: 스트리밍 중 호스트 물리 화면을 검은 오버레이로 가린다.
    pub privacy_curtain: bool,
    /// 스트리밍 중 데스크탑 우상단에 뜨는 "연결 중" 배지. 개인 기기 조합에서는
    /// 소음이므로 기본 꺼짐이며, 타인이 보는 환경에서만 켠다.
    pub streaming_badge: bool,
    /// 외부 접속 허용: UPnP로 컨트롤(TCP)·미디어(UDP) 포트를 라우터에 매핑하고
    /// 공개 엔드포인트를 뷰어에 광고한다. 꺼지면 매핑을 즉시 제거하고 LAN
    /// 직접 연결만 남는다.
    pub wan_access: bool,
    /// 네이티브 표면(트레이 메뉴 등)의 UI 언어. 웹뷰의 `leftcar_lang` 설정과
    /// 같은 값이 되며, 없으면 한국어가 기본이다.
    pub language: HostLanguage,
    /// 실험 스위치 — 기본은 전부 미설정이다.
    pub experiment: ExperimentSettings,
}

/// 호스트 UI 언어. 웹뷰의 `leftcar_lang`(localStorage)과 같은 "ko"/"en" 값을
/// 공유 settings.json에 남긴다.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HostLanguage {
    #[default]
    Ko,
    En,
}

impl HostLanguage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ko => "ko",
            Self::En => "en",
        }
    }
}

/// `dirs::data_dir()/leftcar-host/settings.json` (None when the platform has
/// no data dir — settings then stay in-memory per process).
pub fn default_settings_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("leftcar-host").join("settings.json"))
}

/// 읽기 실패 시 안전한 메모리 기본값으로 시작한다. 원본 파일은 바꾸지 않는다.
pub fn load_or_default(path: Option<&Path>) -> HostSettings {
    read_settings(path).unwrap_or_else(|error| {
        eprintln!("leftcar: {error}; using safe defaults without changing the settings file");
        HostSettings::default()
    })
}

fn read_settings(path: Option<&Path>) -> Result<HostSettings, String> {
    let Some(path) = path else {
        return Ok(HostSettings::default());
    };
    let body = match std::fs::read_to_string(path) {
        Ok(body) => body,
        Err(error) => {
            // A missing file is a fresh configuration; an existing unreadable
            // source (including a dangling symlink) must never be replaced.
            if error.kind() == std::io::ErrorKind::NotFound
                && std::fs::symlink_metadata(path).is_err_and(|metadata_error| {
                    metadata_error.kind() == std::io::ErrorKind::NotFound
                })
            {
                return Ok(HostSettings::default());
            }
            return Err(format!("cannot read settings file: {error}"));
        }
    };
    let parsed: serde_json::Value = serde_json::from_str(&body)
        .map_err(|error| format!("cannot parse settings file: {error}"))?;
    Ok(HostSettings {
        clipboard_share: parsed
            .get("clipboardShare")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        file_share: parsed
            .get("fileShare")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        privacy_curtain: parsed
            .get("privacyCurtain")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        streaming_badge: parsed
            .get("streamingBadge")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        wan_access: parsed
            .get("wanAccess")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        experiment: parsed
            .get("experiment")
            .and_then(|v| v.as_object())
            .map(|experiment| ExperimentSettings {
                max_encode_in_flight: experiment
                    .get("maxEncodeInFlight")
                    .and_then(|v| v.as_u64())
                    .and_then(|v| u32::try_from(v).ok()),
                queue_max_age_ms: experiment
                    .get("queueMaxAgeMs")
                    .and_then(|v| v.as_u64())
                    .and_then(|v| u32::try_from(v).ok()),
                sndbuf_bytes: experiment
                    .get("sndbufBytes")
                    .and_then(|v| v.as_u64())
                    .and_then(|v| u32::try_from(v).ok()),
                drl_window_ms: experiment
                    .get("drlWindowMs")
                    .and_then(|v| v.as_u64())
                    .and_then(|v| u32::try_from(v).ok()),
                pacing_budget_pct: experiment
                    .get("pacingBudgetPct")
                    .and_then(|v| v.as_u64())
                    .and_then(|v| u32::try_from(v).ok()),
                frame_trace: experiment
                    .get("frameTrace")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            })
            .unwrap_or_default(),
        language: match parsed
            .get("language")
            .or_else(|| parsed.get("leftcar_lang"))
            .and_then(|v| v.as_str())
        {
            Some("en") => HostLanguage::En,
            _ => HostLanguage::Ko,
        },
    })
}

fn persist(path: &Path, settings: &HostSettings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    }
    let body = serde_json::json!({
        "v": 1,
        "clipboardShare": settings.clipboard_share,
        "fileShare": settings.file_share,
        "privacyCurtain": settings.privacy_curtain,
        "streamingBadge": settings.streaming_badge,
        "wanAccess": settings.wan_access,
        "language": settings.language.as_str(),
        "experiment": {
            "maxEncodeInFlight": settings.experiment.max_encode_in_flight,
            "queueMaxAgeMs": settings.experiment.queue_max_age_ms,
            "sndbufBytes": settings.experiment.sndbuf_bytes,
            "drlWindowMs": settings.experiment.drl_window_ms,
            "pacingBudgetPct": settings.experiment.pacing_budget_pct,
            "frameTrace": settings.experiment.frame_trace,
        },
    })
    .to_string();
    // 임시 파일에 쓰고 같은 디렉터리의 rename으로 갈아끼운다 — 대상 파일을
    // 곧바로 truncate하지 않으므로 동시 쓰기나 중간 크래시로도 settings.json이
    // 반쯤 잘린 상태로 남지 않는다. rename은 유닉스에서 같은 파일시스템 안에서
    // 원자적이고, Windows의 std::fs::rename도 기존 파일을 대상으로 덮어쓴다.
    let tmp = path.with_extension("json.tmp");
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .and_then(|mut f| f.write_all(body.as_bytes()))
            .map_err(|e| format!("write: {e}"))?;
    }
    #[cfg(not(unix))]
    std::fs::write(&tmp, body).map_err(|e| format!("write: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"))
}

/// 실험 스위치의 허용 범위(shim 클램프와 같은 값). 범위 밖 저장은 UI 오류다.
fn validate_experiment(experiment: &ExperimentSettings) -> Result<(), String> {
    let check = |name: &str,
                 value: Option<u32>,
                 range: std::ops::RangeInclusive<u32>|
     -> Result<(), String> {
        if let Some(value) = value {
            if !range.contains(&value) {
                return Err(format!(
                    "{name} out of range {value} (allowed {}..={})",
                    range.start(),
                    range.end()
                ));
            }
        }
        Ok(())
    };
    check("maxEncodeInFlight", experiment.max_encode_in_flight, 1..=5)?;
    check("queueMaxAgeMs", experiment.queue_max_age_ms, 0..=120_000)?;
    check(
        "sndbufBytes",
        experiment.sndbuf_bytes,
        64 * 1024..=2 * 1024 * 1024,
    )?;
    check("drlWindowMs", experiment.drl_window_ms, 50..=1000)?;
    check("pacingBudgetPct", experiment.pacing_budget_pct, 30..=100)?;
    Ok(())
}

/// 실험 설정을 shim 환경변수 쌍으로 변환한다. 값이 있는 것만 내보내고, 없으면
/// shim 기본값이 그대로 쓰인다. 문자열 형식은 shim 파서(Int/Double init,
/// FRAME_TRACE는 "1" 비교)와 정확히 맞춘다. shim은 스트림 시작마다 읽으므로
/// 주입은 앱 시작 때와 설정 변경 때 하면 충분하다.
pub fn experiment_env_vars(experiment: &ExperimentSettings) -> Vec<(&'static str, String)> {
    let mut vars = Vec::new();
    if let Some(limit) = experiment.max_encode_in_flight {
        vars.push(("LEFTCAR_MAX_ENCODE_IN_FLIGHT", limit.to_string()));
    }
    if let Some(ms) = experiment.queue_max_age_ms {
        if ms > 0 {
            vars.push(("LEFTCAR_QUEUE_MAX_AGE_MS", ms.to_string()));
        }
    }
    if let Some(bytes) = experiment.sndbuf_bytes {
        vars.push(("LEFTCAR_SO_SNDBUF_BYTES", bytes.to_string()));
    }
    if let Some(ms) = experiment.drl_window_ms {
        vars.push((
            "LEFTCAR_DRL_WINDOW_SECONDS",
            format!("{}", f64::from(ms) / 1000.0),
        ));
    }
    if let Some(pct) = experiment.pacing_budget_pct {
        vars.push(("LEFTCAR_PACING_BUDGET_PCT", pct.to_string()));
    }
    if experiment.frame_trace {
        vars.push(("LEFTCAR_FRAME_TRACE", "1".to_string()));
    }
    vars
}

/// 프로세스 전역에서 공유되는 설정값(메모리 상태 + 영속 경로). ControlServer의
/// 제어 명령 게이트와 Tauri UI 명령이 같은 인스턴스를 나눠 쓴다.
pub struct SharedSettings {
    path: Option<PathBuf>,
    load_error: Option<String>,
    settings: Mutex<HostSettings>,
}

impl SharedSettings {
    pub fn load_or_default(path: Option<PathBuf>) -> Self {
        let (settings, load_error) = match read_settings(path.as_deref()) {
            Ok(settings) => (settings, None),
            Err(error) => {
                eprintln!("leftcar: {error}; using safe defaults and preserving the settings file");
                (HostSettings::default(), Some(error))
            }
        };
        Self {
            path,
            load_error,
            settings: Mutex::new(settings),
        }
    }

    /// 영속 경로 없이(기본 꺼짐) 시작하는 공유 인스턴스 — 테스트·폴백용.
    pub fn in_memory() -> Arc<Self> {
        Arc::new(Self::load_or_default(None))
    }

    pub fn get(&self) -> HostSettings {
        *self.settings.lock().unwrap()
    }

    pub fn file_share(&self) -> bool {
        self.get().file_share
    }

    pub fn clipboard_share(&self) -> bool {
        self.get().clipboard_share
    }

    pub fn privacy_curtain(&self) -> bool {
        self.get().privacy_curtain
    }

    pub fn streaming_badge(&self) -> bool {
        self.get().streaming_badge
    }

    pub fn wan_access(&self) -> bool {
        self.get().wan_access
    }

    /// 외부 접속 토글 — 다른 게이트 토글과 같은 0600 파일에 영속된다. 매핑의
    /// 등록·제거는 호출자(upnp 명령)가 즉시 실행한다.
    pub fn set_wan_access(&self, enabled: bool) -> Result<(), String> {
        self.update_field(|s| s.wan_access = enabled)
    }

    /// 스트리밍 배지 토글 — 다른 게이트 토글과 같은 0600 파일에 영속된다.
    pub fn set_streaming_badge(&self, enabled: bool) -> Result<(), String> {
        self.update_field(|s| s.streaming_badge = enabled)
    }

    pub fn experiment(&self) -> ExperimentSettings {
        self.get().experiment
    }

    /// 실험 스위치 저장. 범위 밖 값은 저장하기 전에 거부한다 — shim도 클램프
    /// 하지만, UI 입력 오류를 저장 시점에 드러내는 쪽이 실험 기록을 신뢰할 수
    /// 있다. 적용 시점은 다음 스트림 시작이다(세션 시작마다 환경변수를 읽음).
    pub fn set_experiment(&self, experiment: ExperimentSettings) -> Result<(), String> {
        validate_experiment(&experiment)?;
        self.update_field(|s| s.experiment = experiment)
    }

    pub fn language(&self) -> HostLanguage {
        self.get().language
    }

    /// 메모리 값을 바꾸고 디스크에 영속한다. 영속에 실패하면 오류를 반환하고
    /// 메모리 값은 바꾸지 않는다 — 호출자(UI)가 실패를 사용자에게 보여 준다.
    fn update_field(&self, mutate: impl FnOnce(&mut HostSettings)) -> Result<(), String> {
        // 읽기→수정→영속→반영 전체를 잠금 하나에서 끝낸다. 잠금 사이에
        // 놓이면 두 세터가 같은 사본을 고쳐 나중 토글이 먼저 토글을 지워
        // 버린다(갱신 유실). 설정 변경은 드문 UI 동작이므로 파일 IO 동안
        // 잠금을 쥐고 있어도 충분하다. 실패한 토글이 메모리 값을 바꿔 놓으면
        // UI가 실패 값을 보여 주므로, 디스크 쓰기가 성공한 뒤에만 반영한다.
        let mut settings = self.settings.lock().unwrap();
        if let Some(error) = &self.load_error {
            return Err(format!(
                "settings were not loaded; preserving the original file: {error}. Restore access or repair the file, then restart Leftcar Host"
            ));
        }
        if let Some(path) = &self.path {
            // Also preserve a source that became unreadable/corrupt after
            // startup. Validate before creating or truncating a temporary file.
            read_settings(Some(path)).map_err(|error| {
                format!("cannot save settings; preserving the original file: {error}")
            })?;
        }
        let mut next = *settings;
        mutate(&mut next);
        if let Some(path) = &self.path {
            persist(path, &next)?;
        }
        *settings = next;
        Ok(())
    }

    pub fn set_privacy_curtain(&self, enabled: bool) -> Result<(), String> {
        self.update_field(|s| s.privacy_curtain = enabled)
    }

    /// 클립보드 토글 — file_share와 같은 0600 파일에 함께 영속된다.
    pub fn set_clipboard_share(&self, enabled: bool) -> Result<(), String> {
        self.update_field(|s| s.clipboard_share = enabled)
    }

    /// 메모리 값을 바꾸고 디스크에 영속한다. 영속에 실패하면 오류를 반환하고
    /// 메모리 값은 바꾸지 않는다 — 호출자(UI)가 실패를 사용자에게 보여 준다.
    pub fn set_file_share(&self, enabled: bool) -> Result<(), String> {
        self.update_field(|s| s.file_share = enabled)
    }

    /// UI 언어 토글 — 웹뷰의 leftcar_lang 값을 네이티브 표면(트레이 등)과
    /// 공유하기 위해 같은 0600 파일에 남긴다.
    pub fn set_language(&self, language: HostLanguage) -> Result<(), String> {
        self.update_field(|s| s.language = language)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "leftcar-settings-{tag}-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn missing_file_defaults_with_wan_access_off() {
        let path = temp_path("missing");
        let settings = load_or_default(Some(&path));
        assert!(!settings.file_share);
        assert!(!settings.clipboard_share);
        assert!(!settings.privacy_curtain);
        // 외부 접속은 전부 기본 꺼짐이다 — 공인 인터넷 노출(wanAccess)도
        // 예외가 아니다(2026-09-20). 정식 외부 경로는 오버레이 네트워크다.
        assert!(!settings.wan_access);
    }

    #[test]
    fn wan_access_toggle_persists_and_survives_reload() {
        let path = temp_path("wan");
        let shared = SharedSettings::load_or_default(Some(path.clone()));
        assert!(!shared.wan_access());
        shared.set_wan_access(true).unwrap();
        assert!(shared.wan_access());
        let reloaded = load_or_default(Some(&path));
        assert!(reloaded.wan_access);
        // 다른 필드는 따라 바뀌지 않는다.
        assert!(!reloaded.file_share);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn experiment_settings_roundtrip_and_env_format() {
        let path = temp_path("experiment");
        let shared = SharedSettings::load_or_default(Some(path.clone()));
        // 기본은 전부 미설정 — 환경변수도 하나도 내보내지 않는다.
        assert_eq!(experiment_env_vars(&shared.experiment()), vec![]);
        shared
            .set_experiment(ExperimentSettings {
                max_encode_in_flight: Some(2),
                queue_max_age_ms: Some(33),
                sndbuf_bytes: Some(1024 * 1024),
                drl_window_ms: Some(250),
                pacing_budget_pct: Some(90),
                frame_trace: true,
            })
            .unwrap();
        let vars = experiment_env_vars(&shared.experiment());
        assert_eq!(
            vars,
            vec![
                ("LEFTCAR_MAX_ENCODE_IN_FLIGHT", "2".to_string()),
                ("LEFTCAR_QUEUE_MAX_AGE_MS", "33".to_string()),
                ("LEFTCAR_SO_SNDBUF_BYTES", (1024 * 1024).to_string()),
                ("LEFTCAR_DRL_WINDOW_SECONDS", "0.25".to_string()),
                ("LEFTCAR_PACING_BUDGET_PCT", "90".to_string()),
                ("LEFTCAR_FRAME_TRACE", "1".to_string()),
            ]
        );
        let reloaded = load_or_default(Some(&path));
        assert_eq!(reloaded.experiment, shared.experiment());
        // 0 밸브는 끔과 같다 — 환경변수를 만들지 않는다.
        shared
            .set_experiment(ExperimentSettings {
                queue_max_age_ms: Some(0),
                ..shared.experiment()
            })
            .unwrap();
        assert!(!experiment_env_vars(&shared.experiment())
            .iter()
            .any(|(key, _)| *key == "LEFTCAR_QUEUE_MAX_AGE_MS"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn experiment_settings_reject_out_of_range_values() {
        let shared = SharedSettings::in_memory();
        let base = ExperimentSettings::default();
        let mut bad = base;
        bad.max_encode_in_flight = Some(6);
        assert!(shared.set_experiment(bad).is_err());
        let mut bad = base;
        bad.drl_window_ms = Some(2000);
        assert!(shared.set_experiment(bad).is_err());
        let mut bad = base;
        bad.pacing_budget_pct = Some(10);
        assert!(shared.set_experiment(bad).is_err());
        let mut bad = base;
        bad.sndbuf_bytes = Some(1024);
        assert!(shared.set_experiment(bad).is_err());
        // 거부된 값은 메모리에 반영되지 않는다.
        assert_eq!(shared.experiment(), base);
    }

    #[test]
    fn legacy_settings_file_without_experiment_block_stays_default() {
        let path = temp_path("legacy-experiment");
        std::fs::write(
            &path,
            r#"{"v":1,"clipboardShare":true,"wanAccess":false,"language":"en"}"#,
        )
        .unwrap();
        let settings = load_or_default(Some(&path));
        assert!(settings.clipboard_share);
        assert_eq!(settings.experiment, ExperimentSettings::default());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn legacy_lock_setting_is_ignored_and_not_persisted() {
        let path = temp_path("legacy-lock");
        std::fs::write(&path, r#"{"lockOnDisconnect":true,"privacyCurtain":true}"#).unwrap();

        let shared = SharedSettings::load_or_default(Some(path.clone()));
        assert!(shared.privacy_curtain());
        shared.set_privacy_curtain(true).unwrap();

        let persisted = std::fs::read_to_string(&path).unwrap();
        assert!(!persisted.contains("lockOnDisconnect"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn privacy_curtain_persists_and_survives_reload() {
        let path = temp_path("privacy");
        let shared = SharedSettings::load_or_default(Some(path.clone()));
        shared.set_privacy_curtain(true).unwrap();
        let reloaded = load_or_default(Some(&path));
        assert!(reloaded.privacy_curtain);
        assert!(!reloaded.file_share, "independent fields must not leak");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn language_persists_and_survives_reload() {
        let path = temp_path("language");
        let shared = SharedSettings::load_or_default(Some(path.clone()));
        assert_eq!(
            shared.language(),
            HostLanguage::Ko,
            "missing value defaults to Korean"
        );
        shared.set_language(HostLanguage::En).unwrap();
        assert_eq!(load_or_default(Some(&path)).language, HostLanguage::En);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn set_file_share_persists_and_survives_reload() {
        let path = temp_path("persist");
        let shared = SharedSettings::load_or_default(Some(path.clone()));
        assert!(!shared.file_share());
        shared.set_file_share(true).unwrap();
        assert!(shared.file_share());

        let reloaded = load_or_default(Some(&path));
        assert!(reloaded.file_share);
        assert!(!reloaded.clipboard_share);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "settings must be 0600");
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn corrupt_settings_use_safe_defaults_without_overwriting_the_source() {
        let path = temp_path("corrupt");
        let original = b"{not json";
        std::fs::write(&path, original).unwrap();
        let shared = SharedSettings::load_or_default(Some(path.clone()));
        let snapshot = shared.get();
        assert_eq!(snapshot, HostSettings::default());
        let error = shared.set_file_share(true).unwrap_err();
        assert!(error.contains("settings were not loaded"), "{error}");
        assert_eq!(shared.get(), snapshot);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(!path.with_extension("json.tmp").exists());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn unreadable_settings_bytes_are_preserved_on_mutation() {
        let path = temp_path("invalid-utf8");
        let original = [0xff, 0xfe, 0x80];
        std::fs::write(&path, original).unwrap();
        let shared = SharedSettings::load_or_default(Some(path.clone()));
        let snapshot = shared.get();
        assert_eq!(snapshot, HostSettings::default());
        let error = shared.set_clipboard_share(true).unwrap_err();
        assert!(error.contains("settings were not loaded"), "{error}");
        assert_eq!(shared.get(), snapshot);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(!path.with_extension("json.tmp").exists());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_settings_directory_is_rejected_before_creating_a_temporary_file() {
        let path = temp_path("read-error-directory");
        let _ = std::fs::remove_file(path.with_extension("json.tmp"));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir(&path).unwrap();
        let marker = path.join("keep.txt");
        std::fs::write(&marker, b"original").unwrap();
        let shared = SharedSettings::load_or_default(Some(path.clone()));
        let snapshot = shared.get();
        let error = shared.set_privacy_curtain(true).unwrap_err();
        assert!(error.contains("settings were not loaded"), "{error}");
        assert_eq!(shared.get(), snapshot);
        assert_eq!(std::fs::read(marker).unwrap(), b"original");
        assert!(path.is_dir());
        assert!(!path.with_extension("json.tmp").exists());
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn corruption_after_startup_is_not_replaced_by_a_later_mutation() {
        let path = temp_path("late-corruption");
        std::fs::write(&path, br#"{"clipboardShare":true}"#).unwrap();
        let shared = SharedSettings::load_or_default(Some(path.clone()));
        let snapshot = shared.get();
        assert!(snapshot.clipboard_share);
        let original = b"saved settings became unreadable JSON";
        std::fs::write(&path, original).unwrap();
        assert!(shared.set_streaming_badge(true).is_err());
        assert_eq!(shared.get(), snapshot);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(!path.with_extension("json.tmp").exists());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn persist_failure_leaves_memory_unchanged() {
        // Keep the source readable so the failure occurs in persistence, not
        // the read guard. A directory at the temporary-file path blocks writes.
        let path = temp_path("persist-failure");
        std::fs::write(&path, br#"{"privacyCurtain":true}"#).unwrap();
        let tmp = path.with_extension("json.tmp");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir(&tmp).unwrap();
        let shared = SharedSettings::load_or_default(Some(path.clone()));
        let snapshot = shared.get();
        assert!(shared.set_file_share(true).is_err());
        assert!(shared.set_clipboard_share(true).is_err());
        assert_eq!(shared.get(), snapshot);
        assert_eq!(load_or_default(Some(&path)), snapshot);
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir_all(tmp);
    }

    #[test]
    fn concurrent_setters_do_not_lose_updates_or_tear_the_file() {
        let path = temp_path("race");
        let shared = std::sync::Arc::new(SharedSettings::load_or_default(Some(path.clone())));
        let tmp = path.with_extension("json.tmp");
        let mut handles = Vec::new();
        for i in 0..16 {
            let shared = shared.clone();
            handles.push(std::thread::spawn(move || {
                let on = i % 2 == 0;
                // 세 세터를 모두 두드린다 — 어느 필드 하나 유실되면 안 된다.
                let _ = shared.set_privacy_curtain(!on);
                let _ = shared.set_clipboard_share(on);
                let _ = shared.set_file_share(!on);
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
        // 갱신 유실이 없으려면 마지막으로 끄닥린 세터의 상태가 메모리와
        // 파일 양쪽에 그대로 있어야 한다(찢어진 JSON은 기본값으로 읽혀
        // 불일치로 잡힌다).
        let memory = shared.get();
        let file = load_or_default(Some(&path));
        assert_eq!(memory, file, "memory and file must agree after the race");
        assert!(
            !tmp.exists(),
            "a successful persist must leave no .tmp leftover"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn persist_goes_through_a_tmp_rename_and_survives_a_stale_tmp() {
        let path = temp_path("tmp-rename");
        let shared = SharedSettings::load_or_default(Some(path.clone()));
        // 이전 크래시가 남긴 어금니 있는 .tmp가 있어도 동작에 영향이 없다.
        std::fs::write(path.with_extension("json.tmp"), b"stale").unwrap();
        shared.set_file_share(true).unwrap();
        assert!(load_or_default(Some(&path)).file_share);
        assert!(
            !path.with_extension("json.tmp").exists(),
            "the stale .tmp must be consumed by the next persist"
        );
        // 성공 경로에는 .tmp가 남지 않는다.
        assert!(!path.with_extension("json.tmp").exists());
        let _ = std::fs::remove_file(&path);
    }
}
