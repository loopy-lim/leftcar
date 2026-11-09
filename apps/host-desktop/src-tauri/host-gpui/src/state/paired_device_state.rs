//! 페어링 기기 병합 엔진(src/paired-device-state.ts 포팅). React 앱은
//! 모듈 전역 저장소 + `useSyncExternalStore` 구독으로 이 로직을 썼다.
//! Rust에서는 [`PairedDeviceStateStore`] 하나가 상태와 리스너를 모두
//! 소유하는 `&mut self` 구조체다(설계 §2.1 원칙 4 — 상태 경계 이식).
//!
//! 불변성(원본 모듈 문서와 같다):
//! - 하나의 웹뷰는 하나의 Host 프로세스·프로필에 속한다. **완전 스냅샷만이
//!   revision을 전진시킨다.** 부분 응답은 그 credential에 관한 사실만 증명한다.
//! - 제거(revoke)는 완전 스냅샷이 구성원 자격을 확인할 때까지
//!   tombstone으로 남는다. 다른 기기의 부분 응답이 tombstone을 치울 수
//!   없다. 새 완전 스냅샷이 이 맵의 상계이며, 그 뒤 오래된 스냅샷은
//!   revision 게이트에 걸린다.
//! - 확인된 지연 응답이 역사적 스냅샷 이후에도 그 credential이 존재했음을
//!   증명하면 오래된 구성원 정보로 지우지 않는다(stateRevision 승리).
//! - 같은 credential의 그랜트는 revision이 높은 쪽이 이긴다.
//! - 명령 실패는 uncertainty epoch로 기록되고, 실패 이후에 발행된 저장만
//!   그 uncertainty를 해소할 수 있다(인과 결합).
//! - revoke 결과의 persistence 오류는 administrativeError로 올라가고,
//!   이후의 같은/더 높은 stateRevision 결과가 해소한다.
//!
//! 수치 도메인: 원본 TS는 초기 revision을 -1로 두므로 i64 하나로 통일한다
//! (스냅샷 revision·stateRevision 모두 패널 JSON에서는 ≥0이다). -1은
//! "아직 아무 스냅샷도 못 받았다"는 뜻이고 `devices_ready`의 근거다
//! (usePairingModel의 `revision >= 0` 규칙).

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// lib `source_grants::GrantView`의 미러(camelCase JSON 계약 동일).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceGrants {
    pub credential_id: String,
    /// Host 프로세스 뷰 순서. 원격 인증에는 절대 포함되지 않는다.
    pub state_revision: i64,
    pub source_ids: Vec<String>,
    pub revision: u64,
    pub review_required: bool,
    /// 이 기기의 상시 원격 입력 승인(유효값). 화면 승인과 별개 결정이다.
    pub input: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persistence_error: Option<String>,
}

/// lib `pairing::PairedDeviceView`의 미러. 최상위 필드는 lib 타입에
/// rename_all이 없어 snake_case다(source_grants 안쪽은 camelCase).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PairedDevice {
    pub source_grants: SourceGrants,
    pub device_id: String,
    pub name: String,
    pub paired_at: String,
    /// 인증된 제어 연결이 살아 있는지 — 뷰어 배지와 같은 사실이다.
    #[serde(default)]
    pub connected: bool,
}

/// lib `pairing::PairedDeviceState`의 미러(list_paired_device_state).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PairedDeviceStateSnapshot {
    pub revision: i64,
    pub devices: Vec<PairedDevice>,
}

/// lib `pairing::RevokeOutcome`의 미러(revoke_paired_device /
/// revoke_all_devices). camelCase다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RevokeOutcome {
    #[serde(default)]
    pub removed_devices: Vec<RemovedDevice>,
    #[serde(default)]
    pub state_revision: i64,
    #[serde(default)]
    pub persistence_errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovedDevice {
    pub device_id: String,
    pub credential_id: String,
}

/// 저장된 상태 한 벌. `administrative_error`는 revoke 결과의 저장 실패가
/// 올라오는 창이다(None = 해소됨).
#[derive(Debug, Clone, PartialEq)]
pub struct StoreState {
    pub revision: i64,
    pub devices: Vec<PairedDevice>,
    pub administrative_error: Option<String>,
}

struct Uncertainty {
    epoch: u64,
    error: String,
}

type Listener = Arc<dyn Fn(&StoreState) + Send + Sync>;

/// 상태 + 리스너를 소유하는 병합 엔진. React 앱의 모듈 전역 변수 5개
/// (state, uncertain, failureEpoch, administrativeRevision,
/// removedCredentials)를 필드로 옮긴 것이다.
pub struct PairedDeviceStateStore {
    state: StoreState,
    uncertain: HashMap<String, Uncertainty>,
    failure_epoch: u64,
    administrative_revision: i64,
    /// 제거는 완전 스냅샷이 구성원 자격을 확인할 때까지 남는다(credential
    /// → 그 제거를 증명한 stateRevision).
    removed_credentials: HashMap<String, i64>,
    listeners: Vec<Option<Listener>>,
}

impl Default for PairedDeviceStateStore {
    fn default() -> PairedDeviceStateStore {
        PairedDeviceStateStore::new()
    }
}

impl PairedDeviceStateStore {
    pub fn new() -> PairedDeviceStateStore {
        PairedDeviceStateStore {
            state: StoreState {
                revision: -1,
                devices: Vec::new(),
                administrative_error: None,
            },
            uncertain: HashMap::new(),
            failure_epoch: 0,
            administrative_revision: -1,
            removed_credentials: HashMap::new(),
            listeners: Vec::new(),
        }
    }

    /// usePairingModel의 `devicesReady` 규칙: revision ≥ 0이면 한 번이라도
    /// 스냅샷을 받은 것이다.
    pub fn devices_ready(&self) -> bool {
        self.state.revision >= 0
    }

    pub fn get_state(&self) -> &StoreState {
        &self.state
    }

    /// 상태 교체 후 리스너를 부른다. 리스너는 새 상태를 참조로 받는다 —
    /// React의 useSyncExternalStore가 스냅샷을 다시 읽던 규칙의 직역이다.
    fn publish(&mut self, next: StoreState) {
        self.state = next;
        for listener in self.listeners.iter().flatten() {
            listener(&self.state);
        }
    }

    /// 리스너를 등록하고 구독 해제에 쓸 식별자를 돌려준다.
    pub fn subscribe(&mut self, listener: impl Fn(&StoreState) + Send + Sync + 'static) -> usize {
        self.listeners.push(Some(Arc::new(listener)));
        self.listeners.len() - 1
    }

    pub fn unsubscribe(&mut self, id: usize) {
        if let Some(slot) = self.listeners.get_mut(id) {
            *slot = None;
        }
    }

    pub fn source_grant_uncertainty_epoch(&self, credential_id: &str) -> u64 {
        self.uncertain
            .get(credential_id)
            .map_or(0, |failure| failure.epoch)
    }

    /// uncertainty가 살아 있는 그랜트는 검토 필요 + 저장 실패 문구를
    /// 뒤집어 쓴다(원본 retainUncertainty).
    fn retain_uncertainty(&self, mut grants: SourceGrants) -> SourceGrants {
        if let Some(failure) = self.uncertain.get(&grants.credential_id) {
            grants.review_required = true;
            grants.persistence_error = Some(failure.error.clone());
        }
        grants
    }

    /// 완전 스냅샷 접수(receivePairedDeviceState). 낮은 revision은 버린다.
    pub fn receive_paired_device_state(&mut self, snapshot: PairedDeviceStateSnapshot) {
        if snapshot.revision < self.state.revision {
            return;
        }
        // 이 완전 스냅샷이 실어 온 구성원 집합(tombstone 소거 판정용) —
        // 소비 전에 미리 모은다.
        let membership: HashSet<String> = snapshot
            .devices
            .iter()
            .map(|device| device.source_grants.credential_id.clone())
            .collect();
        let mut previous_by_id: HashMap<&str, &PairedDevice> = self
            .state
            .devices
            .iter()
            .map(|device| (device.device_id.as_str(), device))
            .collect();

        let mut devices: Vec<PairedDevice> = Vec::with_capacity(snapshot.devices.len());
        for device in snapshot.devices {
            // tombstone이 살아 있는 credential은 어떤 스냅샷도 부활시키지
            // 못한다(제거가 완전 스냅샷에 확인될 때까지).
            if self.removed_credentials.contains_key(&device.source_grants.credential_id) {
                continue;
            }
            let previous = previous_by_id.remove(device.device_id.as_str());
            let grants = match previous {
                // 알려진 더 새 부분 확인이 이 역사적 스냅샷 이후에도 그
                // credential이 존재했음을 증명한다 — 오래된 구성원 정보로
                // 지우지 않는다.
                Some(previous) if previous.source_grants.state_revision > snapshot.revision => {
                    devices.push(previous.clone());
                    continue;
                }
                Some(previous) => {
                    let same_credential = previous.source_grants.credential_id
                        == device.source_grants.credential_id;
                    if same_credential && previous.source_grants.revision > device.source_grants.revision
                    {
                        previous.source_grants.clone()
                    } else {
                        device.source_grants
                    }
                }
                None => device.source_grants,
            };
            devices.push(PairedDevice {
                source_grants: self.retain_uncertainty(grants),
                ..device
            });
        }
        // 스냅샷에서 빠졌지만 더 새 부분 확인을 지닌 기기는 남는다.
        for previous in previous_by_id.into_values() {
            if previous.source_grants.state_revision > snapshot.revision {
                devices.push(previous.clone());
            }
        }

        // 현재 구성원이 아닌 credential의 uncertainty는 치운다.
        let current: HashSet<&str> = devices
            .iter()
            .map(|device| device.source_grants.credential_id.as_str())
            .collect();
        self.uncertain
            .retain(|credential, _| current.contains(credential.as_str()));

        // 이 완전 스냅샷이 구성원 부재로 제거를 확인하면 tombstone을 치운다.
        self.removed_credentials.retain(|credential, removed_at| {
            !(snapshot.revision >= *removed_at && !membership.contains(credential))
        });

        let administrative_error = self.state.administrative_error.clone();
        self.publish(StoreState {
            revision: snapshot.revision,
            devices,
            administrative_error,
        });
    }

    /// 저장 명령의 성공 응답 접수(confirmSourceGrants). 검증 3종(기기
    /// 부재·credential 불일치·오래된 revision)을 통과해야 반영된다.
    pub fn confirm_source_grants(
        &mut self,
        device_id: &str,
        grants: SourceGrants,
        started_at_epoch: Option<u64>,
    ) {
        let Some(index) = self
            .state
            .devices
            .iter()
            .position(|device| device.device_id == device_id)
        else {
            return;
        };
        let max_state_revision = {
            let current = &self.state.devices[index].source_grants;
            if current.credential_id != grants.credential_id || current.revision > grants.revision {
                return;
            }
            grants.state_revision.max(current.state_revision)
        };
        // 거절된 명령에는 Host revision과 인과로 묶인 값이 없다. 마지막으로
        // 관찰한 실패 이후에 발행된 저장만 그 정확한 uncertainty를 해소한다.
        if let Some(epoch) = started_at_epoch {
            if self
                .uncertain
                .get(&grants.credential_id)
                .is_some_and(|failure| failure.epoch == epoch)
            {
                self.uncertain.remove(&grants.credential_id);
            }
        }
        let confirmed =
            self.retain_uncertainty(SourceGrants {
                state_revision: max_state_revision,
                ..grants
            });
        let mut devices = self.state.devices.clone();
        devices[index].source_grants = confirmed;
        let revision = self.state.revision;
        let administrative_error = self.state.administrative_error.clone();
        self.publish(StoreState {
            revision,
            devices,
            administrative_error,
        });
    }

    /// 저장 명령의 실패 접수(markSourceGrantsUncertain). 실패마다 새 epoch를
    /// 발급한다 — 이전 실패 이후의 성공 응답만 옛 실패를 해소할 수 있다.
    pub fn mark_source_grants_uncertain(
        &mut self,
        device_id: &str,
        credential_id: &str,
        error: &str,
    ) {
        let Some(index) = self.state.devices.iter().position(|device| {
            device.device_id == device_id && device.source_grants.credential_id == credential_id
        }) else {
            return;
        };
        self.failure_epoch += 1;
        self.uncertain.insert(
            credential_id.to_string(),
            Uncertainty {
                epoch: self.failure_epoch,
                error: error.to_string(),
            },
        );
        let mut devices = self.state.devices.clone();
        devices[index].source_grants = self.retain_uncertainty(devices[index].source_grants.clone());
        let revision = self.state.revision;
        let administrative_error = self.state.administrative_error.clone();
        self.publish(StoreState {
            revision,
            devices,
            administrative_error,
        });
    }

    /// revoke 명령 결과 접수(receiveRevokeOutcome). 완전 스냅샷은 구성원을
    /// 대체할 수 있지만 무관한 부분 응답은 못 한다.
    pub fn receive_revoke_outcome(&mut self, outcome: RevokeOutcome) {
        if outcome.state_revision > self.state.revision {
            for removed in outcome.removed_devices {
                let known = self
                    .removed_credentials
                    .get(&removed.credential_id)
                    .copied()
                    .unwrap_or(-1);
                self.removed_credentials.insert(
                    removed.credential_id.clone(),
                    outcome.state_revision.max(known),
                );
                self.uncertain.remove(&removed.credential_id);
            }
        }
        let devices: Vec<PairedDevice> = self
            .state
            .devices
            .iter()
            .filter(|device| {
                !self
                    .removed_credentials
                    .contains_key(&device.source_grants.credential_id)
            })
            .cloned()
            .collect();
        let error = outcome.persistence_errors.join("\n");
        let administrative_error = if !error.is_empty() {
            Some(error)
        } else if outcome.state_revision >= self.administrative_revision {
            None
        } else {
            self.state.administrative_error.clone()
        };
        self.administrative_revision = self.administrative_revision.max(outcome.state_revision);
        let revision = self.state.revision;
        self.publish(StoreState {
            revision,
            devices,
            administrative_error,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 원본 TS 테스트 자리가 없어 모듈 불변성 + usePairingModel.ts 사용례에서
    /// 파생한 케이스들이다. 각 테스트 주석에 근거를 적었다.
    fn grants(credential_id: &str, state_revision: i64, revision: u64) -> SourceGrants {
        SourceGrants {
            credential_id: credential_id.into(),
            state_revision,
            source_ids: vec!["source:0".into()],
            revision,
            review_required: false,
            input: false,
            persistence_error: None,
        }
    }

    fn device(device_id: &str, credential_id: &str, state_revision: i64, revision: u64) -> PairedDevice {
        PairedDevice {
            source_grants: grants(credential_id, state_revision, revision),
            device_id: device_id.into(),
            name: "Galaxy XR".into(),
            paired_at: "2026-11-06T00:00:00Z".into(),
            connected: false,
        }
    }

    fn snapshot(revision: i64, devices: Vec<PairedDevice>) -> PairedDeviceStateSnapshot {
        PairedDeviceStateSnapshot { revision, devices }
    }

    // 불변성: 초기 상태는 -1이고 스냅샷을 받기 전까지 준비되지 않았다.
    #[test]
    fn initial_state_is_empty_and_not_ready() {
        let store = PairedDeviceStateStore::new();
        assert!(!store.devices_ready());
        assert_eq!(store.get_state().revision, -1);
        assert!(store.get_state().devices.is_empty());
        assert_eq!(store.get_state().administrative_error, None);
    }

    // 불변성: 완전 스냅샷만이 revision을 전진시킨다.
    #[test]
    fn complete_snapshot_advances_revision_and_devices() {
        let mut store = PairedDeviceStateStore::new();
        let notified = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let sink = notified.clone();
        let subscription = store.subscribe(move |_| {
            sink.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        assert_ne!(subscription, usize::MAX);

        store.receive_paired_device_state(snapshot(3, vec![device("d1", "c1", 3, 1)]));
        assert!(store.devices_ready());
        assert_eq!(store.get_state().revision, 3);
        assert_eq!(store.get_state().devices.len(), 1);
        assert_eq!(
            notified.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "publish마다 리스너가 부른다"
        );
    }

    // 불변성: 낮은 revision의 스냅샷은 버려진다(단조 게이트).
    #[test]
    fn older_snapshot_is_ignored() {
        let mut store = PairedDeviceStateStore::new();
        store.receive_paired_device_state(snapshot(5, vec![device("d1", "c1", 5, 2)]));
        store.receive_paired_device_state(snapshot(3, vec![device("d2", "c2", 3, 1)]));
        assert_eq!(store.get_state().revision, 5);
        let ids: Vec<&str> = store
            .get_state()
            .devices
            .iter()
            .map(|device| device.device_id.as_str())
            .collect();
        assert_eq!(ids, vec!["d1"]);
    }

    // usePairingModel: refreshDevices가 접수하는 스냅샷의 connected는
    // 병합이 훼손하지 않는다.
    #[test]
    fn connected_flag_follows_the_snapshot_device() {
        let mut store = PairedDeviceStateStore::new();
        let mut live = device("d1", "c1", 3, 1);
        live.connected = true;
        store.receive_paired_device_state(snapshot(3, vec![live.clone()]));
        assert!(store.get_state().devices[0].connected);
        store.receive_paired_device_state(snapshot(4, vec![live]));
        assert!(store.get_state().devices[0].connected);
    }

    // 불변성: revoke tombstone은 어떤 부분 스냅샷도 부활시키지 못한다.
    #[test]
    fn removed_credential_stays_hidden_until_acknowledged() {
        let mut store = PairedDeviceStateStore::new();
        store.receive_paired_device_state(snapshot(3, vec![device("d1", "c1", 3, 1)]));
        store.receive_revoke_outcome(RevokeOutcome {
            removed_devices: vec![RemovedDevice {
                device_id: "d1".into(),
                credential_id: "c1".into(),
            }],
            state_revision: 6,
            persistence_errors: vec![],
        });
        assert!(store.get_state().devices.is_empty());

        // 같은/더 낮은 revision의 스냅샷이 c1을 다시 실어 와도 숨는다.
        store.receive_paired_device_state(snapshot(6, vec![device("d1", "c1", 6, 2)]));
        assert!(store.get_state().devices.is_empty());
    }

    // 불변성: 완전 스냅샷이 구성원 부재로 제거를 확인하면 tombstone이
    // 치우고, 그 뒤의 더 새 스냅샷은 다시 보여줄 수 있다.
    #[test]
    fn tombstone_prunes_once_a_complete_snapshot_acknowledges_removal() {
        let mut store = PairedDeviceStateStore::new();
        store.receive_paired_device_state(snapshot(3, vec![device("d1", "c1", 3, 1)]));
        store.receive_revoke_outcome(RevokeOutcome {
            removed_devices: vec![RemovedDevice {
                device_id: "d1".into(),
                credential_id: "c1".into(),
            }],
            state_revision: 6,
            persistence_errors: vec![],
        });
        // c1 없는 완전 스냅샷(rev 6) — 제거를 확인한다.
        store.receive_paired_device_state(snapshot(6, vec![]));
        // 이후 더 새 스냅샷(rev 7)이 c1을 다시 실어 오면 보인다.
        store.receive_paired_device_state(snapshot(7, vec![device("d1", "c1", 7, 3)]));
        assert_eq!(store.get_state().devices.len(), 1);
        assert_eq!(store.get_state().devices[0].source_grants.revision, 3);
    }

    // 불변성: 알려진 더 새 부분 확인(stateRevision이 스냅샷 revision보다
    // 큰 기기)은 오래된 구성원 정보로 지워지지 않는다.
    #[test]
    fn later_partial_confirmation_is_not_erased_by_older_membership() {
        let mut store = PairedDeviceStateStore::new();
        store.receive_paired_device_state(snapshot(5, vec![device("d1", "c1", 5, 2)]));
        // confirm이 stateRevision을 스냅샷 revision 위로 끌어올린 상황.
        let confirmed = grants("c1", 9, 3);
        store.confirm_source_grants("d1", confirmed, None);
        // rev 6 스냅샷은 d1의 stateRevision 6을 실어 온다 — 9가 이긴다.
        store.receive_paired_device_state(snapshot(6, vec![device("d1", "c1", 6, 2)]));
        assert_eq!(store.get_state().devices[0].source_grants.state_revision, 9);
        assert_eq!(store.get_state().devices[0].source_grants.revision, 3);
    }

    // 불변성: 같은 credential의 그랜트는 revision이 높은 쪽이 이긴다 —
    // 그랜트 객체 전체가 이기는 방식이다(TS가 필드별로 합치지 않는다).
    #[test]
    fn per_grant_revision_win_keeps_newer_grants() {
        let mut store = PairedDeviceStateStore::new();
        store.receive_paired_device_state(snapshot(5, vec![device("d1", "c1", 5, 5)]));
        store.receive_paired_device_state(snapshot(6, vec![device("d1", "c1", 6, 3)]));
        let device = &store.get_state().devices[0];
        assert_eq!(device.source_grants.revision, 5);
        assert_eq!(device.source_grants.state_revision, 5);
    }

    // 불변성: credential이 다르면 그랜트 revision 비교 없이 스냅샷 그랜트가
    // 이긴다(재페어링으로 credential이 바뀐 경우).
    #[test]
    fn different_credential_lets_the_snapshot_win() {
        let mut store = PairedDeviceStateStore::new();
        store.receive_paired_device_state(snapshot(5, vec![device("d1", "c1", 5, 9)]));
        store.receive_paired_device_state(snapshot(6, vec![device("d1", "c2", 6, 1)]));
        assert_eq!(store.get_state().devices[0].source_grants.credential_id, "c2");
        assert_eq!(store.get_state().devices[0].source_grants.revision, 1);
    }

    // 불변성: uncertainty는 reviewRequired + persistenceError로 보인다.
    #[test]
    fn uncertainty_flags_review_required_and_persistence_error() {
        let mut store = PairedDeviceStateStore::new();
        store.receive_paired_device_state(snapshot(3, vec![device("d1", "c1", 3, 1)]));
        store.mark_source_grants_uncertain("d1", "c1", "persistence denied");
        let device = &store.get_state().devices[0];
        assert!(device.source_grants.review_required);
        assert_eq!(
            device.source_grants.persistence_error.as_deref(),
            Some("persistence denied")
        );
        assert_eq!(store.source_grant_uncertainty_epoch("c1"), 1);
    }

    // usePairingModel approveScreens: confirm은 시작 때 본 epoch와 같을 때만
    // uncertainty를 해소한다.
    #[test]
    fn confirm_resolves_uncertainty_only_for_the_matching_epoch() {
        let mut store = PairedDeviceStateStore::new();
        store.receive_paired_device_state(snapshot(3, vec![device("d1", "c1", 3, 1)]));

        store.mark_source_grants_uncertain("d1", "c1", "boom");
        let epoch = store.source_grant_uncertainty_epoch("c1");
        store.confirm_source_grants("d1", grants("c1", 4, 2), Some(epoch));
        assert_eq!(store.source_grant_uncertainty_epoch("c1"), 0);
        assert_eq!(
            store.get_state().devices[0].source_grants.persistence_error,
            None
        );

        // 두 번째 실패 이후, 옛 epoch로 확인한 저장은 해소하지 못한다.
        store.mark_source_grants_uncertain("d1", "c1", "boom again");
        let fresh = store.source_grant_uncertainty_epoch("c1");
        store.confirm_source_grants("d1", grants("c1", 5, 3), Some(epoch));
        assert_eq!(store.source_grant_uncertainty_epoch("c1"), fresh);
        assert_eq!(
            store.get_state().devices[0].source_grants.persistence_error.as_deref(),
            Some("boom again")
        );
        // 새 epoch로 확인하면 해소된다.
        store.confirm_source_grants("d1", grants("c1", 6, 4), Some(fresh));
        assert_eq!(store.source_grant_uncertainty_epoch("c1"), 0);
    }

    // confirm 검증 3종: 기기 부재 / credential 불일치 / 오래된 revision.
    #[test]
    fn confirm_ignores_stale_or_mismatched_grants() {
        let mut store = PairedDeviceStateStore::new();
        store.receive_paired_device_state(snapshot(3, vec![device("d1", "c1", 3, 5)]));

        // 모르는 기기.
        store.confirm_source_grants("dX", grants("c1", 9, 6), None);
        assert_eq!(store.get_state().devices.len(), 1);

        // credential 불일치.
        store.confirm_source_grants("d1", grants("c2", 9, 6), None);
        assert_eq!(store.get_state().devices[0].source_grants.revision, 5);

        // 오래된 그랜트 revision.
        store.confirm_source_grants("d1", grants("c1", 9, 4), None);
        assert_eq!(store.get_state().devices[0].source_grants.revision, 5);
        assert_eq!(store.get_state().devices[0].source_grants.state_revision, 3);

        // 통과한 확인은 stateRevision을 max로 올린다.
        store.confirm_source_grants("d1", grants("c1", 9, 6), None);
        let device = &store.get_state().devices[0];
        assert_eq!(device.source_grants.revision, 6);
        assert_eq!(device.source_grants.state_revision, 9);
    }

    // 불변성: credential이 현재 구성원에서 사라지면 uncertainty도 치운다.
    #[test]
    fn uncertainty_is_pruned_when_the_credential_leaves_the_state() {
        let mut store = PairedDeviceStateStore::new();
        store.receive_paired_device_state(snapshot(3, vec![device("d1", "c1", 3, 1)]));
        store.mark_source_grants_uncertain("d1", "c1", "boom");
        assert_eq!(store.source_grant_uncertainty_epoch("c1"), 1);

        store.receive_paired_device_state(snapshot(4, vec![device("d2", "c2", 4, 1)]));
        assert_eq!(store.source_grant_uncertainty_epoch("c1"), 0);
    }

    // 불변성: revoke 결과의 persistence 오류는 administrativeError로 올라가고
    // 같은/더 높은 stateRevision의 깨끗한 결과가 해소한다. 무관한 부분
    // 응답(stateRevision이 더 낮음)은 해소하지 못한다.
    #[test]
    fn administrative_error_resolves_on_a_fresh_outcome() {
        let mut store = PairedDeviceStateStore::new();
        store.receive_paired_device_state(snapshot(3, vec![device("d1", "c1", 3, 1)]));
        store.receive_revoke_outcome(RevokeOutcome {
            removed_devices: vec![RemovedDevice {
                device_id: "d1".into(),
                credential_id: "c1".into(),
            }],
            state_revision: 4,
            persistence_errors: vec!["disk full".into()],
        });
        assert_eq!(
            store.get_state().administrative_error.as_deref(),
            Some("disk full")
        );
        assert!(store.get_state().devices.is_empty());

        // stateRevision 3의 깨끗한 부분 응답 — 해소 못 한다.
        store.receive_revoke_outcome(RevokeOutcome::default());
        assert_eq!(
            store.get_state().administrative_error.as_deref(),
            Some("disk full")
        );

        // administrativeRevision(4) 이상의 깨끗한 결과 — 해소한다.
        store.receive_revoke_outcome(RevokeOutcome {
            removed_devices: vec![],
            state_revision: 4,
            persistence_errors: vec![],
        });
        assert_eq!(store.get_state().administrative_error, None);
    }

    // 불변성: revoke로 제거된 credential의 uncertainty는 같이 치운다.
    #[test]
    fn revoke_clears_uncertainty_for_removed_credentials() {
        let mut store = PairedDeviceStateStore::new();
        store.receive_paired_device_state(snapshot(3, vec![device("d1", "c1", 3, 1)]));
        store.mark_source_grants_uncertain("d1", "c1", "boom");
        store.receive_revoke_outcome(RevokeOutcome {
            removed_devices: vec![RemovedDevice {
                device_id: "d1".into(),
                credential_id: "c1".into(),
            }],
            state_revision: 5,
            persistence_errors: vec![],
        });
        assert_eq!(store.source_grant_uncertainty_epoch("c1"), 0);
    }

    // 불변성: 여러 persistence 오류는 줄바꿈으로 합쳐진다(원본 join("\n")).
    #[test]
    fn persistence_errors_join_with_newlines() {
        let mut store = PairedDeviceStateStore::new();
        store.receive_paired_device_state(snapshot(3, vec![]));
        store.receive_revoke_outcome(RevokeOutcome {
            removed_devices: vec![],
            state_revision: 4,
            persistence_errors: vec!["disk full".into(), "read only".into()],
        });
        assert_eq!(
            store.get_state().administrative_error.as_deref(),
            Some("disk full\nread only")
        );
    }

    // JSON 계약: lib pairing::PairedDeviceState의 직렬화가 이 미러로 읽힌다
    // (commands.rs 테스트가 실물 라운드트립을 함께 고정한다).
    #[test]
    fn mirrors_deserialize_the_panel_json_shape() {
        let raw = serde_json::json!({
            "revision": 4,
            "devices": [{
                "source_grants": {
                    "credentialId": "c1",
                    "stateRevision": 4,
                    "sourceIds": ["source:0"],
                    "revision": 2,
                    "reviewRequired": true,
                    "input": false,
                    "persistenceError": null
                },
                "device_id": "d1",
                "name": "Galaxy XR",
                "paired_at": "2026-11-06T00:00:00Z",
                "connected": true
            }]
        });
        let snapshot: PairedDeviceStateSnapshot = serde_json::from_value(raw).unwrap();
        assert_eq!(snapshot.revision, 4);
        assert_eq!(snapshot.devices[0].device_id, "d1");
        assert!(snapshot.devices[0].source_grants.review_required);
        assert!(snapshot.devices[0].connected);

        let outcome: RevokeOutcome = serde_json::from_value(serde_json::json!({
            "removedDevices": [{ "deviceId": "d1", "credentialId": "c1" }],
            "stateRevision": 6,
            "persistenceErrors": []
        }))
        .unwrap();
        assert_eq!(outcome.removed_devices.len(), 1);
        assert_eq!(outcome.removed_devices[0].credential_id, "c1");
    }

    // 구독: unsubscribe 이후 리스너가 부르지 않는다.
    #[test]
    fn unsubscribe_stops_notifications() {
        let mut store = PairedDeviceStateStore::new();
        let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let sink = count.clone();
        let subscription = store.subscribe(move |_| {
            sink.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        store.unsubscribe(subscription);
        store.receive_paired_device_state(snapshot(1, vec![]));
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    // 파생(revoke_all_devices 결과): removed_devices에 같은 credential이
    // 두 번 와도 tombstone은 최댓값 하나를 유지한다.
    #[test]
    fn duplicate_removals_keep_the_max_revision() {
        let mut store = PairedDeviceStateStore::new();
        store.receive_paired_device_state(snapshot(3, vec![device("d1", "c1", 3, 1)]));
        let removal = || RemovedDevice {
            device_id: "d1".into(),
            credential_id: "c1".into(),
        };
        store.receive_revoke_outcome(RevokeOutcome {
            removed_devices: vec![removal()],
            state_revision: 6,
            persistence_errors: vec![],
        });
        store.receive_revoke_outcome(RevokeOutcome {
            removed_devices: vec![removal()],
            state_revision: 5,
            persistence_errors: vec![],
        });
        // rev 5의 재도착이 tombstone을 6에서 5로 깎지 못한다(max 유지).
        store.receive_paired_device_state(snapshot(6, vec![]));
        store.receive_paired_device_state(snapshot(7, vec![device("d1", "c1", 7, 4)]));
        assert_eq!(store.get_state().devices.len(), 1);
    }
}
