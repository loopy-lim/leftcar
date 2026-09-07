//! Durable ownership and serialized placement for Leftcar-managed displays.
use crate::provider::{DisplaySpec, VirtualDisplay, VirtualDisplayProvider};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DisplayPosition {
    Left,
    Right,
    Above,
    Below,
}
impl DisplayPosition {
    pub fn parse(v: &str) -> Result<Self, String> {
        match v {
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            "above" => Ok(Self::Above),
            "below" => Ok(Self::Below),
            _ => Err("위치는 left, right, above, below 중 하나여야 합니다.".into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}
fn i32_coord(v: i64) -> Result<i32, String> {
    i32::try_from(v).map_err(|_| "디스플레이 배치 좌표 범위를 벗어났습니다.".into())
}
pub fn adjacent_rect_offset(
    a: DisplayRect,
    size: (u32, u32),
    p: DisplayPosition,
    offset: u32,
) -> Result<DisplayRect, String> {
    let (w, h) = size;
    let (x, y) = match p {
        DisplayPosition::Left => (
            i64::from(a.x) - i64::from(w) - i64::from(offset),
            i64::from(a.y),
        ),
        DisplayPosition::Right => (
            i64::from(a.x) + i64::from(a.width) + i64::from(offset),
            i64::from(a.y),
        ),
        DisplayPosition::Above => (
            i64::from(a.x),
            i64::from(a.y) - i64::from(h) - i64::from(offset),
        ),
        DisplayPosition::Below => (
            i64::from(a.x),
            i64::from(a.y) + i64::from(a.height) + i64::from(offset),
        ),
    };
    Ok(DisplayRect {
        x: i32_coord(x)?,
        y: i32_coord(y)?,
        width: w,
        height: h,
    })
}
pub fn adjacent_rect(a: DisplayRect, s: (u32, u32), p: DisplayPosition) -> DisplayRect {
    adjacent_rect_offset(a, s, p, 0).expect("adjacent coordinates fit i32")
}
fn occupied_offset(
    anchor: DisplayRect,
    records: impl Iterator<Item = DisplayRect>,
    position: DisplayPosition,
) -> Result<u32, String> {
    let edge = match position {
        DisplayPosition::Right => i64::from(anchor.x) + i64::from(anchor.width),
        DisplayPosition::Left => i64::from(anchor.x),
        DisplayPosition::Below => i64::from(anchor.y) + i64::from(anchor.height),
        DisplayPosition::Above => i64::from(anchor.y),
    };
    let extent = records
        .map(|r| match position {
            DisplayPosition::Right => i64::from(r.x) + i64::from(r.width) - edge,
            DisplayPosition::Left => edge - i64::from(r.x),
            DisplayPosition::Below => i64::from(r.y) + i64::from(r.height) - edge,
            DisplayPosition::Above => edge - i64::from(r.y),
        })
        .max()
        .unwrap_or(0)
        .max(0);
    u32::try_from(extent).map_err(|_| "디스플레이 배치 크기가 너무 큽니다.".into())
}

#[cfg(target_os = "macos")]
pub fn active_anchor_rect() -> Option<DisplayRect> {
    use std::os::raw::c_uint;
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGMainDisplayID() -> c_uint;
        fn CGDisplayBounds(id: c_uint) -> CGRect;
    }
    #[repr(C)]
    struct CGPoint {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    struct CGSize {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    struct CGRect {
        origin: CGPoint,
        size: CGSize,
    }
    let r = unsafe { CGDisplayBounds(CGMainDisplayID()) };
    (r.size.width > 0.0 && r.size.height > 0.0).then_some(DisplayRect {
        x: r.origin.x.round() as i32,
        y: r.origin.y.round() as i32,
        width: r.size.width.round() as u32,
        height: r.size.height.round() as u32,
    })
}
#[cfg(not(target_os = "macos"))]
pub fn active_anchor_rect() -> Option<DisplayRect> {
    None
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedDisplayView {
    pub id: String,
    pub name: String,
    pub logical_width: u32,
    pub logical_height: u32,
    pub scale: u8,
    pub backing_width: u32,
    pub backing_height: u32,
    pub position: DisplayPosition,
    pub provider_kind: String,
    #[serde(default)]
    pub verified: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OwnershipRecord {
    view: ManagedDisplayView,
    rect: DisplayRect,
    betterdisplay_tag_id: Option<String>,
    betterdisplay_uuid: Option<String>,
    display_id: Option<u32>,
}
struct ManagedDisplay {
    record: OwnershipRecord,
    display: VirtualDisplay,
    provider: Arc<dyn VirtualDisplayProvider>,
}
struct Registry {
    displays: HashMap<String, ManagedDisplay>,
}
fn refresh_observed_rects(registry: &mut Registry) {
    #[cfg(target_os = "macos")]
    for entry in registry.displays.values_mut() {
        if let Some(id) = entry.record.display_id {
            if let Some((x, y, width, height)) =
                crate::virtual_display::coregraphics_display_rect(id)
            {
                entry.record.rect = DisplayRect {
                    x,
                    y,
                    width,
                    height,
                };
            }
        }
    }
}
#[cfg(target_os = "macos")]
fn current_primary_display() -> Option<u32> {
    Some(crate::virtual_display::main_display_id())
}
#[cfg(not(target_os = "macos"))]
fn current_primary_display() -> Option<u32> {
    None
}
#[derive(Clone)]
pub struct DisplayManager {
    registry: Arc<Mutex<Registry>>,
    state_path: Option<PathBuf>,
}
impl Default for DisplayManager {
    fn default() -> Self {
        Self::new(None)
    }
}

impl DisplayManager {
    pub fn new(state_path: Option<PathBuf>) -> Self {
        let mut displays = HashMap::new();
        if let Some(path) = state_path.as_deref() {
            for mut record in load_records(path) {
                if record.view.provider_kind != "betterdisplay" {
                    continue;
                }
                #[cfg(target_os = "macos")]
                {
                    if let Ok(o) = crate::virtual_display::observe_display(&record.view.name) {
                        if record
                            .betterdisplay_uuid
                            .as_deref()
                            .is_some_and(|uuid| uuid != o.uuid)
                        {
                            continue;
                        }
                        record.betterdisplay_tag_id = Some(o.tag_id);
                        record.betterdisplay_uuid = Some(o.uuid);
                        record.view.logical_width = o.logical_width;
                        record.view.logical_height = o.logical_height;
                        record.view.backing_width = o.pixel_width;
                        record.view.backing_height = o.pixel_height;
                        record.view.verified = true;
                        record.display_id = Some(o.display_id);
                    }
                    let provider: Arc<dyn VirtualDisplayProvider> =
                        Arc::new(crate::provider::BetterDisplayProvider::new());
                    let id = record.view.id.clone();
                    displays.insert(
                        id,
                        ManagedDisplay {
                            display: VirtualDisplay {
                                name: record.view.name.clone(),
                                cgvd_display_id: None,
                            },
                            provider,
                            record,
                        },
                    );
                }
            }
        }
        Self {
            registry: Arc::new(Mutex::new(Registry { displays })),
            state_path,
        }
    }
    pub fn default_state_path() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("dev.leftcar.host")
            .join("managed-displays.json")
    }
    pub fn list(&self) -> Vec<ManagedDisplayView> {
        let g = self.registry.lock().expect("display registry poisoned");
        let mut v: Vec<_> = g.displays.values().map(|e| e.record.view.clone()).collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }
    fn persist(&self, g: &Registry) -> Result<(), String> {
        let Some(p) = self.state_path.as_deref() else {
            return Ok(());
        };
        let r: Vec<_> = g.displays.values().map(|e| e.record.clone()).collect();
        persist_records(p, &r)
    }
    pub fn add(
        &self,
        provider: Arc<dyn VirtualDisplayProvider>,
        name: String,
        width: u32,
        height: u32,
        scale: u8,
        position: DisplayPosition,
    ) -> Result<ManagedDisplayView, String> {
        let mut g = self.registry.lock().map_err(|e| e.to_string())?;
        let spec = DisplaySpec {
            name: name.trim().into(),
            width,
            height,
            scale,
        };
        spec.backing_dimensions().map_err(|e| e.message())?;
        crate::virtual_display::validate_name(&spec.name)?;
        let anchor = active_anchor_rect()
            .ok_or_else(|| "활성 화면이 없어 배치할 수 없습니다.".to_string())?;
        refresh_observed_rects(&mut g);
        let offset = occupied_offset(anchor, g.displays.values().map(|e| e.record.rect), position)?;
        let rect = adjacent_rect_offset(anchor, (width, height), position, offset)?;
        let id = Uuid::new_v4().to_string();
        let managed_name = format!("{} [leftcar:{}]", spec.name, id);
        let primary_before = current_primary_display();
        let display = match provider.create(&DisplaySpec {
            name: managed_name.clone(),
            width,
            height,
            scale,
        }) {
            Ok(display) => display,
            Err(error) => {
                #[cfg(target_os = "macos")]
                if provider.name() == "betterdisplay" {
                    if let Ok(o) = crate::virtual_display::observe_display(&managed_name) {
                        let view = ManagedDisplayView {
                            id: id.clone(),
                            name: managed_name.clone(),
                            logical_width: o.logical_width,
                            logical_height: o.logical_height,
                            scale,
                            backing_width: o.pixel_width,
                            backing_height: o.pixel_height,
                            position,
                            provider_kind: provider.name().into(),
                            verified: true,
                        };
                        let record = OwnershipRecord {
                            view,
                            rect,
                            betterdisplay_tag_id: Some(o.tag_id),
                            betterdisplay_uuid: Some(o.uuid),
                            display_id: Some(o.display_id),
                        };
                        g.displays.insert(
                            id.clone(),
                            ManagedDisplay {
                                record,
                                display: VirtualDisplay {
                                    name: managed_name,
                                    cgvd_display_id: None,
                                },
                                provider,
                            },
                        );
                        let persist_error = self
                            .persist(&g)
                            .err()
                            .map(|e| format!("; 영속 소유권 저장 실패: {e}"))
                            .unwrap_or_default();
                        return Err(format!(
                            "{}; 정리되지 않은 화면의 소유권을 ID {id}로 유지했습니다{persist_error}.",
                            error.message(),
                        ));
                    }
                    if let Ok(identity) =
                        crate::virtual_display::resolve_display_identity(&managed_name)
                    {
                        let (backing_width, backing_height) =
                            spec.backing_dimensions().map_err(|v| v.message())?;
                        let view = ManagedDisplayView {
                            id: id.clone(),
                            name: managed_name.clone(),
                            logical_width: 0,
                            logical_height: 0,
                            scale,
                            backing_width,
                            backing_height,
                            position,
                            provider_kind: provider.name().into(),
                            verified: false,
                        };
                        let record = OwnershipRecord {
                            view,
                            rect,
                            betterdisplay_tag_id: Some(identity.tag_id),
                            betterdisplay_uuid: Some(identity.uuid),
                            display_id: Some(identity.display_id),
                        };
                        g.displays.insert(
                            id.clone(),
                            ManagedDisplay {
                                record,
                                display: VirtualDisplay {
                                    name: managed_name,
                                    cgvd_display_id: None,
                                },
                                provider,
                            },
                        );
                        let persist = self
                            .persist(&g)
                            .err()
                            .map(|v| format!("; 영속 소유권 저장 실패: {v}"))
                            .unwrap_or_default();
                        return Err(format!(
                            "{}; 모드 검증 전 확보한 UUID로 복구 ID {id}를 유지합니다{persist}",
                            error.message()
                        ));
                    }
                }
                return Err(error.message());
            }
        };
        let o = match observe_created(&display, provider.name(), width, height, scale) {
            Ok(v) => v,
            Err(e) => match provider.remove(&display) {
                Ok(()) => return Err(e),
                Err(cleanup) => {
                    let (backing_width, backing_height) =
                        spec.backing_dimensions().map_err(|v| v.message())?;
                    let view = ManagedDisplayView {
                        id: id.clone(),
                        name: managed_name.clone(),
                        logical_width: 0,
                        logical_height: 0,
                        scale,
                        backing_width,
                        backing_height,
                        position,
                        provider_kind: provider.name().into(),
                        verified: false,
                    };
                    let record = OwnershipRecord {
                        view,
                        rect,
                        betterdisplay_tag_id: None,
                        betterdisplay_uuid: None,
                        display_id: display.cgvd_display_id,
                    };
                    g.displays.insert(
                        id.clone(),
                        ManagedDisplay {
                            record,
                            display,
                            provider,
                        },
                    );
                    let persist = self
                        .persist(&g)
                        .err()
                        .map(|v| format!("; 영속 소유권 저장 실패: {v}"))
                        .unwrap_or_default();
                    return Err(format!(
                        "{e}; 정리 실패: {}. 복구 ID {id}{persist}",
                        cleanup.message()
                    ));
                }
            },
        };
        let placement = place_display(provider.as_ref(), &display, o.uuid.as_deref(), rect);
        let mut primary_after = current_primary_display();
        #[cfg(target_os = "macos")]
        if placement.is_ok() && primary_after != primary_before {
            if let Some(original) = primary_before {
                let _ = crate::virtual_display::set_main_display(original);
                primary_after = current_primary_display();
            }
        }
        if let Some(cause) = placement.err().map(|e| format!("가상 디스플레이 배치 실패: {e}")).or_else(|| (primary_after != primary_before).then(|| format!("가상 디스플레이 생성 중 주 화면이 변경되었습니다: {primary_before:?} -> {primary_after:?}"))) {
            match provider.remove(&display) {
                Ok(()) => return Err(cause),
                Err(cleanup) => {
                    let view = ManagedDisplayView {
                        id: id.clone(),
                        name: managed_name.clone(),
                        logical_width: o.logical_width,
                        logical_height: o.logical_height,
                        scale,
                        backing_width: o.pixel_width,
                        backing_height: o.pixel_height,
                        position,
                        provider_kind: provider.name().into(),
                        verified: false,
                    };
                    let record = OwnershipRecord {
                        view,
                        rect,
                        betterdisplay_tag_id: o.tag_id,
                        betterdisplay_uuid: o.uuid,
                        display_id: Some(o.display_id),
                    };
                    g.displays.insert(
                        id.clone(),
                        ManagedDisplay {
                            record,
                            display,
                            provider,
                        },
                    );
                    let persist = self
                        .persist(&g)
                        .err()
                        .map(|v| format!("; 영속 소유권 저장 실패: {v}"))
                        .unwrap_or_default();
                    return Err(format!(
                        "{cause}; 정리 실패: {}. 복구 ID {id}{persist}",
                        cleanup.message()
                    ));
                }
            }
        }
        let view = ManagedDisplayView {
            id: id.clone(),
            name: managed_name,
            logical_width: o.logical_width,
            logical_height: o.logical_height,
            scale,
            backing_width: o.pixel_width,
            backing_height: o.pixel_height,
            position,
            provider_kind: provider.name().into(),
            verified: true,
        };
        let record = OwnershipRecord {
            view: view.clone(),
            rect,
            betterdisplay_tag_id: o.tag_id,
            betterdisplay_uuid: o.uuid,
            display_id: Some(o.display_id),
        };
        g.displays.insert(
            id.clone(),
            ManagedDisplay {
                record,
                display,
                provider,
            },
        );
        if let Err(e) = self.persist(&g) {
            let cleanup = remove_owned(g.displays.get(&id).expect("inserted"));
            if cleanup.is_ok() {
                g.displays.remove(&id);
            }
            return Err(match cleanup { Ok(()) => format!("소유권 저장 실패: {e}; 생성 화면은 정리했습니다."), Err(cleanup) => format!("소유권 저장 실패: {e}; 정리 실패로 복구 ID {id}를 메모리에 유지합니다: {cleanup}") });
        }
        Ok(view)
    }
    pub fn remove(&self, id: &str) -> Result<(), String> {
        let mut g = self.registry.lock().map_err(|e| e.to_string())?;
        let e = g
            .displays
            .get(id)
            .ok_or_else(|| "관리 중인 디스플레이를 찾을 수 없습니다.".to_string())?;
        remove_owned(e)?;
        g.displays.remove(id);
        self.persist(&g)
    }
    pub fn set_position(
        &self,
        id: &str,
        p: DisplayPosition,
        anchor: DisplayRect,
    ) -> Result<ManagedDisplayView, String> {
        let mut g = self.registry.lock().map_err(|e| e.to_string())?;
        refresh_observed_rects(&mut g);
        let cur = g
            .displays
            .get(id)
            .ok_or_else(|| "관리 중인 디스플레이를 찾을 수 없습니다.".to_string())?;
        let offset = occupied_offset(
            anchor,
            g.displays
                .values()
                .filter(|e| e.record.view.id != id)
                .map(|e| e.record.rect),
            p,
        )?;
        let rect = adjacent_rect_offset(
            anchor,
            (
                cur.record.view.logical_width,
                cur.record.view.logical_height,
            ),
            p,
            offset,
        )?;
        place_display(
            cur.provider.as_ref(),
            &cur.display,
            cur.record.betterdisplay_uuid.as_deref(),
            rect,
        )?;
        let e = g.displays.get_mut(id).expect("checked");
        e.record.view.position = p;
        e.record.rect = rect;
        let view = e.record.view.clone();
        if let Err(error) = self.persist(&g) {
            return Err(format!(
                "화면 이동은 적용됐지만 새 위치의 영속 저장에 실패했습니다: {error}"
            ));
        }
        Ok(view)
    }
    /// Switches a managed display to a new mode without releasing ownership.
    /// The rect origin is preserved (only the size follows the new logical
    /// mode) and every mutation of the record happens strictly after the
    /// engine confirms the new mode — a failed resize leaves the old record
    /// untouched.
    pub fn resize(
        &self,
        id: &str,
        width: u32,
        height: u32,
        scale: u8,
    ) -> Result<ManagedDisplayView, String> {
        crate::virtual_display::validate_dimensions(width, height)?;
        crate::virtual_display::validate_scale(scale)?;
        let mut g = self.registry.lock().map_err(|e| e.to_string())?;
        let entry = g
            .displays
            .get(id)
            .ok_or_else(|| "관리 중인 디스플레이를 찾을 수 없습니다.".to_string())?;
        let rect = DisplayRect {
            x: entry.record.rect.x,
            y: entry.record.rect.y,
            width,
            height,
        };
        let mode = entry
            .provider
            .resize(&entry.display, width, height, scale)
            .map_err(|e| e.message())?;
        place_display(
            entry.provider.as_ref(),
            &entry.display,
            entry.record.betterdisplay_uuid.as_deref(),
            rect,
        )
        .map_err(|e| format!("리사이즈 후 위치 유지에 실패했습니다: {e}"))?;
        let view = ManagedDisplayView {
            id: entry.record.view.id.clone(),
            name: entry.record.view.name.clone(),
            logical_width: mode.logical_width,
            logical_height: mode.logical_height,
            scale,
            backing_width: mode.pixel_width,
            backing_height: mode.pixel_height,
            position: entry.record.view.position,
            provider_kind: entry.record.view.provider_kind.clone(),
            verified: true,
        };
        let entry = g.displays.get_mut(id).expect("checked above");
        entry.record.view = view.clone();
        entry.record.rect = rect;
        if let Err(error) = self.persist(&g) {
            return Err(format!(
                "리사이즈는 적용됐지만 영속 소유권 저장에 실패했습니다: {error}"
            ));
        }
        Ok(view)
    }
    /// 테스트 전용: 레지스트리에 관리 화면 레코드를 직접 심는다. 실기 엔진
    /// 없이 resize 재사용 경로를 검증할 때 쓴다(`resize_fixture`와 동일한
    /// 레코드 모양).
    #[cfg(test)]
    pub(crate) fn seed_test_display(
        manager: &DisplayManager,
        id: &str,
        name: &str,
        logical_width: u32,
        logical_height: u32,
        scale: u8,
        provider: Arc<dyn VirtualDisplayProvider>,
    ) {
        let record = OwnershipRecord {
            view: ManagedDisplayView {
                id: id.to_owned(),
                name: name.to_owned(),
                logical_width,
                logical_height,
                scale,
                backing_width: logical_width * u32::from(scale),
                backing_height: logical_height * u32::from(scale),
                position: DisplayPosition::Right,
                provider_kind: provider.name().into(),
                verified: true,
            },
            rect: DisplayRect {
                x: 1920,
                y: 0,
                width: logical_width,
                height: logical_height,
            },
            betterdisplay_tag_id: None,
            betterdisplay_uuid: None,
            display_id: None,
        };
        manager
            .registry
            .lock()
            .expect("display registry poisoned")
            .displays
            .insert(
                id.to_owned(),
                ManagedDisplay {
                    display: VirtualDisplay {
                        name: name.to_owned(),
                        cgvd_display_id: None,
                    },
                    record,
                    provider,
                },
            );
    }

    pub fn cleanup_all(&self) -> Vec<String> {
        let mut g = self.registry.lock().expect("display registry poisoned");
        let ids: Vec<_> = g.displays.keys().cloned().collect();
        let mut errors = vec![];
        for id in ids {
            let result = {
                let e = &g.displays[&id];
                remove_owned(e)
            };
            match result {
                Ok(()) => {
                    g.displays.remove(&id);
                }
                Err(e) => errors.push(format!("{id}: {e}")),
            }
        }
        if let Err(e) = self.persist(&g) {
            errors.push(e)
        }
        errors
    }
}

struct Observation {
    tag_id: Option<String>,
    uuid: Option<String>,
    display_id: u32,
    logical_width: u32,
    logical_height: u32,
    pixel_width: u32,
    pixel_height: u32,
}
#[cfg(target_os = "macos")]
fn observe_created(
    d: &VirtualDisplay,
    kind: &str,
    w: u32,
    h: u32,
    s: u8,
) -> Result<Observation, String> {
    if kind == "betterdisplay" {
        let o = crate::virtual_display::verify_display_mode(&d.name, w, h, s)?;
        return Ok(Observation {
            tag_id: Some(o.tag_id),
            uuid: Some(o.uuid),
            display_id: o.display_id,
            logical_width: o.logical_width,
            logical_height: o.logical_height,
            pixel_width: o.pixel_width,
            pixel_height: o.pixel_height,
        });
    }
    let id = d
        .cgvd_display_id
        .ok_or_else(|| "CGVD displayID가 없습니다.".to_string())?;
    let pw = w
        .checked_mul(s.into())
        .ok_or_else(|| "요청 픽셀이 너무 큽니다.".to_string())?;
    let ph = h
        .checked_mul(s.into())
        .ok_or_else(|| "요청 픽셀이 너무 큽니다.".to_string())?;
    // CgvdProvider returns only after the retained shim reports and verifies
    // these observed logical and pixel dimensions in its READY contract.
    Ok(Observation {
        tag_id: None,
        uuid: None,
        display_id: id,
        logical_width: w,
        logical_height: h,
        pixel_width: pw,
        pixel_height: ph,
    })
}
#[cfg(not(target_os = "macos"))]
fn observe_created(
    _: &VirtualDisplay,
    _: &str,
    _: u32,
    _: u32,
    _: u8,
) -> Result<Observation, String> {
    Err("가상 디스플레이는 macOS에서만 지원됩니다.".into())
}
fn remove_owned(entry: &ManagedDisplay) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    if entry.provider.name() == "betterdisplay" {
        let resolved;
        let uuid = if let Some(uuid) = entry.record.betterdisplay_uuid.as_deref() {
            uuid
        } else {
            resolved = crate::virtual_display::resolve_display_identity(&entry.display.name)?;
            resolved.uuid.as_str()
        };
        return crate::virtual_display::remove_virtual_display_by_uuid(uuid).map(|_| ());
    }
    entry
        .provider
        .remove(&entry.display)
        .map_err(|e| e.message())
}
#[cfg(target_os = "macos")]
fn place_display(
    provider: &dyn VirtualDisplayProvider,
    display: &VirtualDisplay,
    uuid: Option<&str>,
    r: DisplayRect,
) -> Result<(), String> {
    if provider.name() == "betterdisplay" {
        crate::virtual_display::set_placement_by_uuid(
            uuid.ok_or_else(|| "BetterDisplay UUID가 없습니다.".to_string())?,
            r.x,
            r.y,
        )
        .map(|_| ())
    } else {
        provider
            .place(display, r.x, r.y)
            .map_err(|error| error.message())
    }
}
#[cfg(not(target_os = "macos"))]
fn place_display(
    _: &dyn VirtualDisplayProvider,
    _: &VirtualDisplay,
    _: Option<&str>,
    _: DisplayRect,
) -> Result<(), String> {
    Err("가상 디스플레이는 macOS에서만 지원됩니다.".into())
}
fn load_records(p: &Path) -> Vec<OwnershipRecord> {
    std::fs::read(p)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
fn persist_records(p: &Path, r: &[OwnershipRecord]) -> Result<(), String> {
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("상태 폴더 생성 실패: {e}"))?
    }
    let temp = p.with_extension("json.tmp");
    std::fs::write(
        &temp,
        serde_json::to_vec_pretty(r).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("상태 저장 실패: {e}"))?;
    std::fs::rename(temp, p).map_err(|e| format!("상태 교체 실패: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct RemoveFails;
    impl VirtualDisplayProvider for RemoveFails {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn available(&self) -> bool {
            true
        }
        fn create(
            &self,
            _: &DisplaySpec,
        ) -> Result<VirtualDisplay, crate::provider::ProviderError> {
            unreachable!()
        }
        fn remove(&self, _: &VirtualDisplay) -> Result<(), crate::provider::ProviderError> {
            Err(crate::provider::ProviderError::EngineFailed(
                "injected cleanup failure".into(),
            ))
        }
    }
    fn test_record(id: &str) -> OwnershipRecord {
        OwnershipRecord {
            view: ManagedDisplayView {
                id: id.into(),
                name: "Leftcar recovery [leftcar:00000000-0000-0000-0000-000000000000]".into(),
                logical_width: 0,
                logical_height: 0,
                scale: 2,
                backing_width: 3200,
                backing_height: 2000,
                position: DisplayPosition::Right,
                provider_kind: "betterdisplay".into(),
                verified: false,
            },
            rect: DisplayRect {
                x: 1920,
                y: 0,
                width: 1600,
                height: 1000,
            },
            betterdisplay_tag_id: None,
            betterdisplay_uuid: None,
            display_id: None,
        }
    }
    #[test]
    fn placement_is_checked_and_non_overlapping() {
        let a = DisplayRect {
            x: 10,
            y: 20,
            width: 1920,
            height: 1080,
        };
        assert_eq!(
            adjacent_rect_offset(a, (1600, 1000), DisplayPosition::Right, 1200)
                .unwrap()
                .x,
            3130
        );
        assert!(adjacent_rect_offset(
            DisplayRect {
                x: i32::MAX,
                y: 0,
                width: u32::MAX,
                height: 1
            },
            (1, 1),
            DisplayPosition::Right,
            0
        )
        .is_err())
    }
    #[test]
    fn ownership_round_trip() {
        let p = std::env::temp_dir().join(format!("leftcar-display-{}.json", Uuid::new_v4()));
        let r = OwnershipRecord {
            view: ManagedDisplayView {
                id: "a".into(),
                name: "unique".into(),
                logical_width: 1600,
                logical_height: 1000,
                scale: 2,
                backing_width: 3200,
                backing_height: 2000,
                position: DisplayPosition::Right,
                provider_kind: "betterdisplay".into(),
                verified: true,
            },
            rect: DisplayRect {
                x: 1920,
                y: 0,
                width: 1600,
                height: 1000,
            },
            betterdisplay_tag_id: Some("42".into()),
            betterdisplay_uuid: Some("uuid".into()),
            display_id: Some(7),
        };
        persist_records(&p, &[r]).unwrap();
        assert_eq!(load_records(&p).len(), 1);
        let _ = std::fs::remove_file(p);
    }
    #[test]
    fn observed_rect_extent_places_after_externally_moved_peer() {
        let anchor = DisplayRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let peer = DisplayRect {
            x: 2500,
            y: 0,
            width: 1600,
            height: 1000,
        };
        let offset = occupied_offset(anchor, [peer].into_iter(), DisplayPosition::Right).unwrap();
        assert_eq!(
            adjacent_rect_offset(anchor, (1200, 800), DisplayPosition::Right, offset)
                .unwrap()
                .x,
            4100
        );
    }
    #[test]
    fn cleanup_failure_retains_in_memory_ownership() {
        let id = "retained".to_string();
        let record = test_record(&id);
        let manager = DisplayManager {
            registry: Arc::new(Mutex::new(Registry {
                displays: HashMap::from([(
                    id.clone(),
                    ManagedDisplay {
                        display: VirtualDisplay {
                            name: record.view.name.clone(),
                            cgvd_display_id: None,
                        },
                        record,
                        provider: Arc::new(RemoveFails),
                    },
                )]),
            })),
            state_path: None,
        };
        assert_eq!(manager.cleanup_all().len(), 1);
        assert_eq!(manager.list()[0].id, id);
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn transient_restart_lookup_failure_keeps_unresolved_record() {
        let path = std::env::temp_dir().join(format!("leftcar-recovery-{}.json", Uuid::new_v4()));
        persist_records(&path, &[test_record("restart")]).unwrap();
        let manager = DisplayManager::new(Some(path.clone()));
        assert_eq!(manager.list()[0].id, "restart");
        assert!(!manager.list()[0].verified);
        let _ = std::fs::remove_file(path);
    }
    struct ResizeOk;
    impl VirtualDisplayProvider for ResizeOk {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn available(&self) -> bool {
            true
        }
        fn create(
            &self,
            _: &DisplaySpec,
        ) -> Result<VirtualDisplay, crate::provider::ProviderError> {
            unreachable!()
        }
        fn place(
            &self,
            _: &VirtualDisplay,
            _: i32,
            _: i32,
        ) -> Result<(), crate::provider::ProviderError> {
            Ok(())
        }
        fn remove(&self, _: &VirtualDisplay) -> Result<(), crate::provider::ProviderError> {
            unreachable!()
        }
        fn resize(
            &self,
            _: &VirtualDisplay,
            width: u32,
            height: u32,
            scale: u8,
        ) -> Result<crate::provider::ResizedDisplayMode, crate::provider::ProviderError> {
            Ok(crate::provider::ResizedDisplayMode {
                logical_width: width,
                logical_height: height,
                pixel_width: width * u32::from(scale),
                pixel_height: height * u32::from(scale),
            })
        }
    }
    struct ResizeFails;
    impl VirtualDisplayProvider for ResizeFails {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn available(&self) -> bool {
            true
        }
        fn create(
            &self,
            _: &DisplaySpec,
        ) -> Result<VirtualDisplay, crate::provider::ProviderError> {
            unreachable!()
        }
        fn remove(&self, _: &VirtualDisplay) -> Result<(), crate::provider::ProviderError> {
            unreachable!()
        }
        fn resize(
            &self,
            _: &VirtualDisplay,
            _: u32,
            _: u32,
            _: u8,
        ) -> Result<crate::provider::ResizedDisplayMode, crate::provider::ProviderError> {
            Err(crate::provider::ProviderError::EngineFailed(
                "injected resize failure".into(),
            ))
        }
    }
    struct ResizePanics;
    impl VirtualDisplayProvider for ResizePanics {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn available(&self) -> bool {
            true
        }
        fn create(
            &self,
            _: &DisplaySpec,
        ) -> Result<VirtualDisplay, crate::provider::ProviderError> {
            unreachable!()
        }
        fn remove(&self, _: &VirtualDisplay) -> Result<(), crate::provider::ProviderError> {
            unreachable!()
        }
        fn resize(
            &self,
            _: &VirtualDisplay,
            _: u32,
            _: u32,
            _: u8,
        ) -> Result<crate::provider::ResizedDisplayMode, crate::provider::ProviderError> {
            // Validation must reject before the provider is ever consulted.
            panic!("resize must not reach the provider for invalid input")
        }
    }
    /// A managed 1600x1000@2x record (rect 1920,0) backed by the given provider.
    fn resize_fixture(id: &str, provider: Arc<dyn VirtualDisplayProvider>) -> DisplayManager {
        let mut record = test_record(id);
        record.view.logical_width = 1600;
        record.view.logical_height = 1000;
        record.view.provider_kind = provider.name().into();
        record.view.verified = true;
        DisplayManager {
            registry: Arc::new(Mutex::new(Registry {
                displays: HashMap::from([(
                    id.to_string(),
                    ManagedDisplay {
                        display: VirtualDisplay {
                            name: record.view.name.clone(),
                            cgvd_display_id: None,
                        },
                        record,
                        provider,
                    },
                )]),
            })),
            state_path: None,
        }
    }
    #[test]
    fn resize_updates_record_and_preserves_position() {
        let manager = resize_fixture("r1", Arc::new(ResizeOk));
        let view = manager.resize("r1", 1280, 800, 1).unwrap();
        assert_eq!((view.logical_width, view.logical_height), (1280, 800));
        assert_eq!(view.scale, 1);
        assert_eq!((view.backing_width, view.backing_height), (1280, 800));
        assert!(view.verified);
        assert_eq!(view.id, "r1");
        assert_eq!(view.position, DisplayPosition::Right);
        let g = manager.registry.lock().unwrap();
        let entry = g.displays.get("r1").expect("record kept");
        // 위치(원점)는 그대로, rect 크기만 새 논리 크기로 따라간다.
        assert_eq!((entry.record.rect.x, entry.record.rect.y), (1920, 0));
        assert_eq!(
            (entry.record.rect.width, entry.record.rect.height),
            (1280, 800)
        );
        assert_eq!(entry.record.view.name, test_record("r1").view.name);
        assert_eq!(entry.record.view.provider_kind, "fake");
    }
    #[test]
    fn resize_unknown_id_is_rejected() {
        let manager = resize_fixture("r1", Arc::new(ResizeOk));
        assert!(manager.resize("missing", 1280, 800, 1).is_err());
    }
    #[test]
    fn resize_validates_before_touching_the_provider() {
        let manager = resize_fixture("r1", Arc::new(ResizePanics));
        assert!(manager.resize("r1", 1280, 800, 3).is_err());
        assert!(manager.resize("r1", 640, 480, 1).is_err());
        assert!(manager.resize("r1", 0, 800, 1).is_err());
        let view = &manager.list()[0];
        assert_eq!(
            (
                view.logical_width,
                view.logical_height,
                view.scale,
                view.backing_width,
                view.backing_height
            ),
            (1600, 1000, 2, 3200, 2000)
        );
    }
    #[test]
    fn resize_failure_keeps_previous_record() {
        let manager = resize_fixture("r1", Arc::new(ResizeFails));
        let error = manager.resize("r1", 1280, 800, 1).unwrap_err();
        assert!(error.contains("injected resize failure"));
        let view = &manager.list()[0];
        assert_eq!(
            (
                view.logical_width,
                view.logical_height,
                view.scale,
                view.backing_width,
                view.backing_height,
                view.verified
            ),
            (1600, 1000, 2, 3200, 2000, true)
        );
        let g = manager.registry.lock().unwrap();
        let entry = g.displays.get("r1").expect("record kept");
        assert_eq!(
            (entry.record.rect.width, entry.record.rect.height),
            (1600, 1000)
        );
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn placement_failure_keeps_previous_record() {
        struct PlaceFails;
        impl VirtualDisplayProvider for PlaceFails {
            fn name(&self) -> &'static str {
                "fake"
            }
            fn available(&self) -> bool {
                true
            }
            fn create(
                &self,
                _: &DisplaySpec,
            ) -> Result<VirtualDisplay, crate::provider::ProviderError> {
                unreachable!()
            }
            fn place(
                &self,
                _: &VirtualDisplay,
                _: i32,
                _: i32,
            ) -> Result<(), crate::provider::ProviderError> {
                Err(crate::provider::ProviderError::EngineFailed(
                    "injected place failure".into(),
                ))
            }
            fn remove(&self, _: &VirtualDisplay) -> Result<(), crate::provider::ProviderError> {
                unreachable!()
            }
            fn resize(
                &self,
                _: &VirtualDisplay,
                width: u32,
                height: u32,
                scale: u8,
            ) -> Result<crate::provider::ResizedDisplayMode, crate::provider::ProviderError>
            {
                Ok(crate::provider::ResizedDisplayMode {
                    logical_width: width,
                    logical_height: height,
                    pixel_width: width * u32::from(scale),
                    pixel_height: height * u32::from(scale),
                })
            }
        }
        let manager = resize_fixture("r1", Arc::new(PlaceFails));
        let error = manager.resize("r1", 1280, 800, 1).unwrap_err();
        assert!(error.contains("위치 유지에 실패"));
        let view = &manager.list()[0];
        assert_eq!(
            (view.logical_width, view.logical_height, view.scale),
            (1600, 1000, 2)
        );
        let g = manager.registry.lock().unwrap();
        let entry = g.displays.get("r1").expect("record kept");
        assert_eq!(
            (entry.record.rect.width, entry.record.rect.height),
            (1600, 1000)
        );
    }

    #[cfg(target_os = "macos")]
    fn run_live_manager_smoke(
        provider: impl Fn() -> Arc<dyn VirtualDisplayProvider>,
        first_scale: u8,
    ) {
        let manager = DisplayManager::default();
        let primary = crate::virtual_display::main_display_id();
        let primary_stable = crate::virtual_display::main_display_stable_id()
            .expect("main display must have a stable UUID");
        let result = (|| -> Result<(), String> {
            let first = manager.add(
                provider(),
                "Leftcar manager smoke A".into(),
                1600,
                1000,
                first_scale,
                DisplayPosition::Right,
            )?;
            let second = manager.add(
                provider(),
                "Leftcar manager smoke B".into(),
                1600,
                1000,
                2,
                DisplayPosition::Right,
            )?;
            manager.set_position(
                &first.id,
                DisplayPosition::Below,
                active_anchor_rect().ok_or("missing anchor")?,
            )?;
            let mut registry = manager.registry.lock().map_err(|e| e.to_string())?;
            refresh_observed_rects(&mut registry);
            let a = registry
                .displays
                .get(&first.id)
                .ok_or("first missing")?
                .record
                .rect;
            let b = registry
                .displays
                .get(&second.id)
                .ok_or("second missing")?
                .record
                .rect;
            let overlaps = a.x < b.x + b.width as i32
                && a.x + a.width as i32 > b.x
                && a.y < b.y + b.height as i32
                && a.y + a.height as i32 > b.y;
            if overlaps {
                return Err(format!("managed displays overlap: {a:?} {b:?}"));
            }
            if crate::virtual_display::main_display_id() != primary {
                return Err("primary display changed".into());
            }
            Ok(())
        })();
        let cleanup = manager.cleanup_all();
        assert!(cleanup.is_empty(), "cleanup errors: {cleanup:?}");
        result.unwrap();
        assert_eq!(
            crate::virtual_display::main_display_stable_id().as_deref(),
            Some(primary_stable.as_str())
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "mutates the live macOS display layout; run explicitly on a GUI host"]
    fn live_manager_add_move_remove_preserves_primary_and_non_overlap() {
        run_live_manager_smoke(|| Arc::new(crate::provider::CgvdProvider::new()), 1);
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "requires BetterDisplay and mutates the live macOS display layout"]
    fn live_betterdisplay_manager_add_move_remove() {
        run_live_manager_smoke(
            || Arc::new(crate::provider::BetterDisplayProvider::new()),
            2,
        );
    }
}
