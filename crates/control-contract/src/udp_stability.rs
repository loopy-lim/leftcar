use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const UDP_STABILITY_VERSION: u16 = 1;
pub const SPLIT_FEEDBACK_V2_BYTES: u16 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum UdpStabilityProfile {
    Auto,
    Responsive,
    Balanced,
    Stable,
    Custom,
}

impl UdpStabilityProfile {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Responsive => "responsive",
            Self::Balanced => "balanced",
            Self::Stable => "stable",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ViewerUdpCapabilities {
    pub version: u16,
    pub max_fec_parity_shards: u8,
    pub split_feedback_bytes: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UdpStabilityCapabilities {
    pub version: u16,
    pub profiles: Vec<UdpStabilityProfile>,
    pub burst_datagram_options: Vec<u8>,
    pub fec_parity_options: Vec<u8>,
    pub adaptive_pacing: bool,
    pub requires_reconnect: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UdpStabilityRequest {
    pub profile: UdpStabilityProfile,
    #[serde(default)]
    pub burst_datagrams: Option<u8>,
    #[serde(default)]
    pub fec_parity_shards: Option<u8>,
    #[serde(default)]
    pub adaptive_pacing: Option<bool>,
    #[serde(default)]
    pub viewer: Option<ViewerUdpCapabilities>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AppliedUdpStability {
    pub requested: UdpStabilityProfile,
    pub applied: UdpStabilityProfile,
    pub burst_datagrams: u8,
    pub fec_parity_shards: u8,
    pub adaptive_pacing: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
}

pub fn host_udp_stability_capabilities() -> UdpStabilityCapabilities {
    UdpStabilityCapabilities {
        version: UDP_STABILITY_VERSION,
        profiles: vec![
            UdpStabilityProfile::Auto,
            UdpStabilityProfile::Responsive,
            UdpStabilityProfile::Balanced,
            UdpStabilityProfile::Stable,
        ],
        burst_datagram_options: vec![2, 4, 8, 16],
        fec_parity_options: vec![2, 4],
        adaptive_pacing: true,
        requires_reconnect: true,
    }
}

pub fn resolve_udp_stability(
    request: Option<&UdpStabilityRequest>,
    host: &UdpStabilityCapabilities,
) -> Result<AppliedUdpStability, String> {
    let Some(request) = request else {
        return Ok(AppliedUdpStability {
            requested: UdpStabilityProfile::Responsive,
            applied: UdpStabilityProfile::Responsive,
            burst_datagrams: 8,
            fec_parity_shards: 2,
            adaptive_pacing: false,
            fallback_reason: None,
        });
    };

    if request.profile == UdpStabilityProfile::Custom {
        return resolve_custom(request, host);
    }

    if !host.profiles.contains(&request.profile) {
        if request.profile != UdpStabilityProfile::Auto {
            return Err(format!(
                "Host가 {:?} UDP 안정성 모드를 지원하지 않습니다",
                request.profile
            ));
        }
        return resolve_auto_fallback(request, host, "Host가 자동 모드를 지원하지 않습니다");
    }

    let (burst_datagrams, fec_parity_shards, adaptive_pacing) = match request.profile {
        UdpStabilityProfile::Auto => (4, 2, true),
        UdpStabilityProfile::Responsive => (8, 2, false),
        UdpStabilityProfile::Balanced => (4, 2, false),
        UdpStabilityProfile::Stable => (2, 4, false),
        UdpStabilityProfile::Custom => unreachable!("handled above"),
    };

    if request.profile == UdpStabilityProfile::Stable {
        ensure_viewer_parity(request.viewer.as_ref(), 4)?;
    }

    if request.profile == UdpStabilityProfile::Auto
        && (!host.adaptive_pacing
            || request.viewer.as_ref().is_none_or(|viewer| {
                viewer.version < UDP_STABILITY_VERSION
                    || viewer.split_feedback_bytes < SPLIT_FEEDBACK_V2_BYTES
            }))
    {
        return resolve_auto_fallback(
            request,
            host,
            "양쪽이 feedback v2 자동 조절을 지원하지 않습니다",
        );
    }

    ensure_host_option(burst_datagrams, &host.burst_datagram_options, "UDP burst")?;
    ensure_host_option(fec_parity_shards, &host.fec_parity_options, "FEC parity")?;

    Ok(AppliedUdpStability {
        requested: request.profile,
        applied: request.profile,
        burst_datagrams,
        fec_parity_shards,
        adaptive_pacing,
        fallback_reason: None,
    })
}

fn resolve_custom(
    request: &UdpStabilityRequest,
    host: &UdpStabilityCapabilities,
) -> Result<AppliedUdpStability, String> {
    let burst_datagrams = request
        .burst_datagrams
        .ok_or_else(|| "사용자 지정 UDP burst 값이 필요합니다".to_string())?;
    let fec_parity_shards = request
        .fec_parity_shards
        .ok_or_else(|| "사용자 지정 FEC parity 값이 필요합니다".to_string())?;
    let adaptive_pacing = request
        .adaptive_pacing
        .ok_or_else(|| "사용자 지정 자동 조절 값이 필요합니다".to_string())?;
    ensure_host_option(burst_datagrams, &host.burst_datagram_options, "UDP burst")?;
    ensure_host_option(fec_parity_shards, &host.fec_parity_options, "FEC parity")?;
    ensure_viewer_parity(request.viewer.as_ref(), fec_parity_shards)?;
    if adaptive_pacing && !host.adaptive_pacing {
        return Err("Host가 UDP 자동 조절을 지원하지 않습니다".into());
    }
    if adaptive_pacing
        && request
            .viewer
            .as_ref()
            .is_none_or(|viewer| viewer.split_feedback_bytes < SPLIT_FEEDBACK_V2_BYTES)
    {
        return Err("UDP 자동 조절에는 feedback v2 Viewer가 필요합니다".into());
    }
    Ok(AppliedUdpStability {
        requested: UdpStabilityProfile::Custom,
        applied: UdpStabilityProfile::Custom,
        burst_datagrams,
        fec_parity_shards,
        adaptive_pacing,
        fallback_reason: None,
    })
}

fn resolve_auto_fallback(
    request: &UdpStabilityRequest,
    host: &UdpStabilityCapabilities,
    reason: &str,
) -> Result<AppliedUdpStability, String> {
    let (applied, burst_datagrams) = if host.profiles.contains(&UdpStabilityProfile::Balanced)
        && host.burst_datagram_options.contains(&4)
    {
        (UdpStabilityProfile::Balanced, 4)
    } else if host.profiles.contains(&UdpStabilityProfile::Responsive)
        && host.burst_datagram_options.contains(&8)
    {
        (UdpStabilityProfile::Responsive, 8)
    } else {
        return Err("Host와 Viewer가 함께 지원하는 UDP 안정성 모드가 없습니다".into());
    };
    ensure_host_option(2, &host.fec_parity_options, "FEC parity")?;
    Ok(AppliedUdpStability {
        requested: request.profile,
        applied,
        burst_datagrams,
        fec_parity_shards: 2,
        adaptive_pacing: false,
        fallback_reason: Some(reason.into()),
    })
}

fn ensure_viewer_parity(
    viewer: Option<&ViewerUdpCapabilities>,
    requested: u8,
) -> Result<(), String> {
    let supported = viewer.is_some_and(|viewer| {
        viewer.version >= UDP_STABILITY_VERSION && viewer.max_fec_parity_shards >= requested
    });
    if supported {
        return Ok(());
    }
    if requested == 4 {
        return Err("안정성 우선 모드는 4개 FEC parity를 지원하는 Viewer가 필요합니다".into());
    }
    Err(format!(
        "Viewer가 FEC parity {requested} 설정을 지원하지 않습니다"
    ))
}

fn ensure_host_option(value: u8, supported: &[u8], label: &str) -> Result<(), String> {
    if supported.contains(&value) {
        Ok(())
    } else {
        Err(format!("Host가 {label} {value} 설정을 지원하지 않습니다"))
    }
}
