//! 페어링·세션이 만나는 외부 세계 포트. TS에서 expo-secure-store·expo-constants·
//! react-native-tcp-socket·모듈 싱글턴이 담당하던 것을 트레잇으로 분리한다 —
//! Rust 코어는 IO를 소유하지 않고 임베딩(Rustra JSI, 테스트)이 채워 넣는다.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 저장소 실패. TS는 원시 Error를 던지지만 Rust 코어는 문자열 원인을 감싼다.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct StoreError(pub String);

impl StoreError {
    pub fn new(message: impl Into<String>) -> Self {
        StoreError(message.into())
    }
}

/// SecureStore 대응 — 키-값 문자열 저장소(토큰·기기 ID·recent hosts 블롭).
pub trait CredentialStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>, StoreError>;
    fn set(&self, key: &str, value: String) -> Result<(), StoreError>;
    fn delete(&self, key: &str) -> Result<(), StoreError>;
}

/// 메모리 구현 — 테스트·JSI 임베딩이 자체 저장소를 붙이기 전까지의 기본값.
#[derive(Default)]
pub struct MemoryCredentialStore {
    entries: Mutex<HashMap<String, Option<String>>>,
}

impl MemoryCredentialStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 테스트 단정용 스냅숏.
    pub fn snapshot(&self) -> HashMap<String, String> {
        self.entries
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .filter_map(|(k, v)| v.clone().map(|value| (k.clone(), value)))
            .collect()
    }

    pub fn put(&self, key: &str, value: &str) {
        self.entries
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key.to_string(), Some(value.to_string()));
    }
}

impl CredentialStore for MemoryCredentialStore {
    fn get(&self, key: &str) -> Result<Option<String>, StoreError> {
        Ok(self
            .entries
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(key)
            .cloned()
            .flatten())
    }

    fn set(&self, key: &str, value: String) -> Result<(), StoreError> {
        self.entries
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key.to_string(), Some(value));
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), StoreError> {
        self.entries
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key.to_string(), None);
        Ok(())
    }
}

/// TS recent-hosts.ts가 소유하던 저장 블롭의 읽기·쓰기 포트. 엔트리 JSON
/// 스키마는 T-7 prefs 모듈이 완성하지만, 페어링이 필요한 접점(핀 해석·핀
/// 저장·별칭 저장)은 여기서 계약으로 고정한다.
pub trait RecentHostsPort: Send + Sync {
    /// Strict 스냅숏 — 망가진 정체 데이터는 오류로 실패 닫힘한다.
    fn entries(&self) -> Result<Vec<RecentHostEntry>, StoreError>;
    /// host:port 엔트리에 hostKey를 반영한다(표시 이름은 보존).
    fn persist_host_key(&self, host: &str, port: u16, host_key: &str) -> Result<(), StoreError>;
    /// 대표 주소 엔트리에 별칭 경로를 붙인다(QR 테일넷 폴백).
    fn persist_alias(&self, host: &str, port: u16, alias: &str) -> Result<(), StoreError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentHostEntry {
    pub host: String,
    pub port: u16,
    pub name: Option<String>,
    pub host_key: Option<String>,
    pub aliases: Vec<String>,
}

/// 프로세스 시계 — 승인 대기 데드라인 계산. 테스트에서는 수동 시계를 넣는다.
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

/// 호스트 공개키 핀 저장소(프로세스 메모리) — pinned-host-keys.ts의 메모리 맵
/// 대응. QR 스캔·recent hosts 로드 시 채워지고 connect()의 핀 대조에 쓰인다.
/// TS와 달리 모듈 싱글턴이 아니라 인스턴스다 — 임베딩이 하나를 들고 다닌다.
#[derive(Default)]
pub struct HostKeyPins {
    pins: Mutex<HashMap<String, String>>,
}

impl HostKeyPins {
    pub fn new() -> Self {
        Self::default()
    }

    fn key_of(host: &str, port: u16) -> String {
        format!("{}:{port}", host.trim().to_lowercase())
    }

    /// 메모리에만 등록한다(앱 시작 시 recent hosts에서 복원할 때).
    pub fn register(&self, host: &str, port: u16, host_key: &str) {
        self.pins
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(Self::key_of(host, port), host_key.to_string());
    }

    pub fn get(&self, host: &str, port: u16) -> Option<String> {
        self.pins
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&Self::key_of(host, port))
            .cloned()
    }

    /// 실패한 자격증명 거래에서 메모리 엔트리를 복원한다.
    pub fn restore(&self, host: &str, port: u16, host_key: Option<&str>) {
        let mut pins = self.pins.lock().unwrap_or_else(|p| p.into_inner());
        match host_key {
            None => pins.remove(&Self::key_of(host, port)),
            Some(key) => pins.insert(Self::key_of(host, port), key.to_string()),
        };
    }

    /// 테스트 초기화.
    pub fn clear(&self) {
        self.pins.lock().unwrap_or_else(|p| p.into_inner()).clear();
    }
}

/// USB 액세서리 상태 포트(TS usb.getUsbState). 붙어 있으면 루프백 제어 포트를
/// 돌려준다 — 액세서리는 대상 호스트의 프록시다. 세션은 이 값으로 루프백 우선
/// 경로를 시도한다.
pub trait UsbPort: Send + Sync {
    fn control_port(&self) -> Option<u16>;
}

pub struct NoUsb;

impl UsbPort for NoUsb {
    fn control_port(&self) -> Option<u16> {
        None
    }
}

/// 취소 토큰 — TS AbortSignal의 동기 대응. 대기 중 취소도 즉시 깨어난다.
#[derive(Debug, Default)]
pub struct CancelToken {
    state: Mutex<bool>,
    signal: Condvar,
}

impl CancelToken {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn is_cancelled(&self) -> bool {
        *self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn cancel(&self) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        *state = true;
        self.signal.notify_all();
    }

    /// 취소되면 true, `ms`가 지나면 false를 돌려준다.
    pub fn wait(&self, ms: u64) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            if *state {
                return true;
            }
            let (guard, timeout) = self
                .signal
                .wait_timeout(state, Duration::from_millis(ms.max(1)))
                .unwrap_or_else(|p| p.into_inner());
            state = guard;
            if timeout.timed_out() {
                return false;
            }
        }
    }
}

/// 페어링·세션이 공유하는 포트 묶음. `credential_lock`은 TS의 자격증명 변위
/// 직렬화 큐(serializeCredentialMutation) 대응 — 읽기-마이그레이션 경계가
/// 새 페어링의 쓰기와 섞이지 않게 한다.
#[derive(Clone)]
pub struct CorePorts {
    pub transport: Arc<dyn crate::control::ControlTransport>,
    pub store: Arc<dyn CredentialStore>,
    pub pins: Arc<HostKeyPins>,
    pub recent: Arc<dyn RecentHostsPort>,
    pub random: Arc<dyn crate::secure::RandomSource>,
    pub clock: Arc<dyn Clock>,
    pub usb: Arc<dyn UsbPort>,
    pub messages: crate::control::ErrorMessages,
    /// 호스트 페어링 목록에 표시될 기기 라벨(TS deviceName 주입).
    pub device_name: String,
    /// TS serializeCredentialMutation 큐의 뮤텍스. 읽기-마이그레이션 경계와
    /// 페어링 쓰기가 섞이지 않게 한다.
    pub credential_lock: Arc<Mutex<()>>,
}

impl CorePorts {
    pub fn new(
        transport: Arc<dyn crate::control::ControlTransport>,
        store: Arc<dyn CredentialStore>,
    ) -> Self {
        Self {
            transport,
            store,
            pins: Arc::new(HostKeyPins::new()),
            recent: Arc::new(NoRecentHosts),
            random: Arc::new(crate::secure::OsRandom),
            clock: Arc::new(SystemClock),
            usb: Arc::new(NoUsb),
            messages: crate::control::ErrorMessages::KOREAN,
            device_name: "viewer".to_string(),
            credential_lock: Arc::new(Mutex::new(())),
        }
    }
}

/// 기본 recent hosts 포트 — 데이터가 없는 저장소(빈 스냅숏)로 동작한다.
pub struct NoRecentHosts;

impl RecentHostsPort for NoRecentHosts {
    fn entries(&self) -> Result<Vec<RecentHostEntry>, StoreError> {
        Ok(Vec::new())
    }

    fn persist_host_key(&self, _host: &str, _port: u16, _host_key: &str) -> Result<(), StoreError> {
        Ok(())
    }

    fn persist_alias(&self, _host: &str, _port: u16, _alias: &str) -> Result<(), StoreError> {
        Ok(())
    }
}
