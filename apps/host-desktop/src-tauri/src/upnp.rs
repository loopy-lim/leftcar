//! UPnP IGD (Internet Gateway Device) client for automatic NAT port mapping.
//!
//! Enables direct P2P WAN streaming by mapping external router ports
//! to host internal ports and discovering the gateway's public IP address.

use std::net::Ipv4Addr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};

const SSDP_MULTICAST_ADDR: &str = "239.255.255.250:1900";
const SSDP_SEARCH_TARGETS: &[&str] = &[
    "urn:schemas-upnp-org:device:InternetGatewayDevice:1",
    "urn:schemas-upnp-org:service:WANIPConnection:1",
    "urn:schemas-upnp-org:service:WANPPPConnection:1",
];

/// 포트 매핑 lease(초). 갱신 태스크가 이의 절반 간격으로 재등록한다 — 갱신이
/// 반복 실패하면 매핑 상태를 내려놓고 공개 엔드포인트 광고를 끊는다(lease가
/// 라우터 쪽 매핑을 만료시킨다).
pub const MAPPING_LEASE_SECS: u32 = 3600;
pub const RENEWAL_INTERVAL_SECS: u64 = 1200;
/// WAN에서 미디어가 도달해야 하는 호스트 수신 UDP 포트. 미디어는 뷰어→호스트로
/// 들어오고(데이터그램 + LCH1 sealed echo가 5002/5003으로 답한다), 컨트롤 TCP
/// 포트는 실행 때 선택된다.
pub const MEDIA_PORTS: [u16; 5] = [5001, 5002, 5003, 5004, 5005];
const CONTROL_DESCRIPTION: &str = "Leftcar Control";
const MEDIA_DESCRIPTION: &str = "Leftcar Media";
/// 라우터 설명 문서·SOAP 응답의 크기 상한. SSDP LOCATION을 사칭한 응답자가
/// 무한 응답으로 메모리를 채우지 못하게 잘라낸다.
const MAX_DESCRIPTION_BYTES: usize = 256 * 1024;
const MAX_SOAP_RESPONSE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct UpnpGateway {
    pub location: String,
    pub host: String,
    pub port: u16,
    pub control_url: String,
    pub service_type: String,
}

impl UpnpGateway {
    /// Discovers an active UPnP IGD gateway on the local network.
    pub async fn discover(timeout_duration: Duration) -> Option<Self> {
        let socket = UdpSocket::bind("0.0.0.0:0").await.ok()?;
        socket.set_broadcast(true).ok()?;

        for target in SSDP_SEARCH_TARGETS {
            let msg = format!(
                "M-SEARCH * HTTP/1.1\r\n\
                 HOST: 239.255.255.250:1900\r\n\
                 ST: {target}\r\n\
                 MAN: \"ssdp:discover\"\r\n\
                 MX: 2\r\n\r\n"
            );
            let _ = socket.send_to(msg.as_bytes(), SSDP_MULTICAST_ADDR).await;
        }

        let mut buf = [0u8; 4096];
        let deadline = tokio::time::Instant::now() + timeout_duration;

        while tokio::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            match tokio::time::timeout(remaining, socket.recv_from(&mut buf)).await {
                Ok(Ok((len, addr))) => {
                    let text = String::from_utf8_lossy(&buf[..len]);
                    if let Some(location) = parse_header_value(&text, "LOCATION") {
                        // SSDP 응답은 로컬 서브넷의 응답자 자신만 받아들인다.
                        // LOCATION이 응답자가 아닌 호스트를 가리키면 사칭이다 —
                        // 그 설명 문서를 따라가면 임의 호스트로 SOAP을 쏘는
                        // SSRF가 되고, 가짜 공개 IP가 뷰어에 광고될 수 있다.
                        if let Some((host, _, _)) = parse_http_url(&location) {
                            if host == addr.ip().to_string() && is_local_gateway_ip(addr.ip()) {
                                if let Some(gw) = Self::resolve_description(&location).await {
                                    return Some(gw);
                                }
                            }
                        }
                    }
                }
                _ => break,
            }
        }

        None
    }

    /// Fetches root device description XML and locates the WAN IP/PPP connection service.
    pub async fn resolve_description(location: &str) -> Option<Self> {
        let (host, port, path) = parse_http_url(location)?;
        let xml = http_get(&host, port, &path).await.ok()?;

        let (service_type, rel_control_url) = extract_upnp_service(&xml)?;
        let control_url = resolve_relative_url(location, &rel_control_url);
        let (gw_host, gw_port, _) = parse_http_url(&control_url)?;
        // controlURL은 설명 문서와 같은 호스트여야 한다. 다른 호스트를 가리키는
        // 응답은 조작된 것 — 그쪽으로 SOAP을 보내지 않는다.
        if gw_host != host {
            return None;
        }

        Some(Self {
            location: location.to_owned(),
            host: gw_host,
            port: gw_port,
            control_url,
            service_type,
        })
    }

    /// Queries the external public IP address from the router.
    pub async fn get_external_ip(&self) -> Result<Ipv4Addr, String> {
        let body = format!(
            "<?xml version=\"1.0\"?>\r\n\
             <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
             s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\r\n\
               <s:Body>\r\n\
                 <u:GetExternalIPAddress xmlns:u=\"{}\"/>\r\n\
               </s:Body>\r\n\
             </s:Envelope>",
            self.service_type
        );
        let action = format!("\"{}#GetExternalIPAddress\"", self.service_type);
        let response = self.soap_request(&action, &body).await?;

        extract_xml_tag(&response, "NewExternalIPAddress")
            .and_then(|ip_str| ip_str.parse::<Ipv4Addr>().ok())
            .filter(|ip| is_public_ipv4(*ip))
            .ok_or_else(|| "Failed to parse NewExternalIPAddress from SOAP response".into())
    }

    /// Requests a port mapping from the gateway router.
    pub async fn add_port_mapping(
        &self,
        protocol: &str,
        external_port: u16,
        internal_port: u16,
        internal_client: &str,
        description: &str,
        lease_duration: u32,
    ) -> Result<(), String> {
        let body = format!(
            "<?xml version=\"1.0\"?>\r\n\
             <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
             s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\r\n\
               <s:Body>\r\n\
                 <u:AddPortMapping xmlns:u=\"{}\">\r\n\
                   <NewRemoteHost></NewRemoteHost>\r\n\
                   <NewExternalPort>{}</NewExternalPort>\r\n\
                   <NewProtocol>{}</NewProtocol>\r\n\
                   <NewInternalPort>{}</NewInternalPort>\r\n\
                   <NewInternalClient>{}</NewInternalClient>\r\n\
                   <NewEnabled>1</NewEnabled>\r\n\
                   <NewPortMappingDescription>{}</NewPortMappingDescription>\r\n\
                   <NewLeaseDuration>{}</NewLeaseDuration>\r\n\
                 </u:AddPortMapping>\r\n\
               </s:Body>\r\n\
             </s:Envelope>",
            self.service_type,
            external_port,
            protocol.to_uppercase(),
            internal_port,
            internal_client,
            description,
            lease_duration
        );
        let action = format!("\"{}#AddPortMapping\"", self.service_type);
        let resp = self.soap_request(&action, &body).await?;
        if resp.contains("AddPortMappingResponse") {
            Ok(())
        } else {
            Err(format!("AddPortMapping rejected: {resp}"))
        }
    }

    /// Deletes an existing port mapping from the gateway router.
    pub async fn delete_port_mapping(
        &self,
        protocol: &str,
        external_port: u16,
    ) -> Result<(), String> {
        let body = format!(
            "<?xml version=\"1.0\"?>\r\n\
             <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
             s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\r\n\
               <s:Body>\r\n\
                 <u:DeletePortMapping xmlns:u=\"{}\">\r\n\
                   <NewRemoteHost></NewRemoteHost>\r\n\
                   <NewExternalPort>{}</NewExternalPort>\r\n\
                   <NewProtocol>{}</NewProtocol>\r\n\
                 </u:DeletePortMapping>\r\n\
               </s:Body>\r\n\
             </s:Envelope>",
            self.service_type,
            external_port,
            protocol.to_uppercase()
        );
        let action = format!("\"{}#DeletePortMapping\"", self.service_type);
        let resp = self.soap_request(&action, &body).await?;
        if resp.contains("DeletePortMappingResponse") {
            Ok(())
        } else {
            Err(format!("DeletePortMapping rejected: {resp}"))
        }
    }

    async fn soap_request(&self, action: &str, body: &str) -> Result<String, String> {
        let (_, _, path) =
            parse_http_url(&self.control_url).ok_or_else(|| "Invalid control URL".to_string())?;
        http_post(&self.host, self.port, &path, action, body).await
    }

    async fn mapping_owner(
        &self,
        protocol: &str,
        port: u16,
    ) -> Result<Option<MappingOwner>, String> {
        let body = format!(
            "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body><u:GetSpecificPortMappingEntry xmlns:u=\"{}\"><NewRemoteHost></NewRemoteHost><NewExternalPort>{port}</NewExternalPort><NewProtocol>{protocol}</NewProtocol></u:GetSpecificPortMappingEntry></s:Body></s:Envelope>",
            self.service_type,
        );
        let action = format!("\"{}#GetSpecificPortMappingEntry\"", self.service_type);
        let response = self.soap_request(&action, &body).await?;
        if extract_xml_tag(&response, "errorCode").as_deref() == Some("714") {
            return Ok(None);
        }
        if !response.contains("GetSpecificPortMappingEntryResponse") {
            return Err("Cannot verify existing gateway mapping ownership".into());
        }
        Ok(Some(MappingOwner {
            client: extract_xml_tag(&response, "NewInternalClient")
                .ok_or("Gateway mapping has no internal client")?,
            port: extract_xml_tag(&response, "NewInternalPort")
                .and_then(|value| value.parse().ok())
                .ok_or("Gateway mapping has no internal port")?,
            description: extract_xml_tag(&response, "NewPortMappingDescription")
                .ok_or("Gateway mapping has no description")?,
        }))
    }

    async fn verify_mapping_owner(
        &self,
        protocol: &str,
        port: u16,
        local_ip: &str,
        description: &str,
    ) -> Result<(), String> {
        if self
            .mapping_owner(protocol, port)
            .await?
            .is_some_and(|owner| !owner.matches(local_ip, port, description))
        {
            return Err(format!(
                "{protocol} {port} is already owned by another gateway mapping"
            ));
        }
        Ok(())
    }

    async fn acquire_mapping(
        &self,
        protocol: &str,
        port: u16,
        local_ip: &str,
        description: &str,
    ) -> Result<(), String> {
        self.verify_mapping_owner(protocol, port, local_ip, description)
            .await?;
        self.add_port_mapping(
            protocol,
            port,
            port,
            local_ip,
            description,
            MAPPING_LEASE_SECS,
        )
        .await
    }
}

#[derive(Clone)]
struct MappingOwner {
    client: String,
    port: u16,
    description: String,
}

impl MappingOwner {
    fn matches(&self, client: &str, port: u16, description: &str) -> bool {
        self.client == client && self.port == port && self.description == description
    }
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, _, _] = ip.octets();
    !ip.is_private()
        && !ip.is_loopback()
        && !ip.is_link_local()
        && !ip.is_broadcast()
        && !ip.is_documentation()
        && !ip.is_multicast()
        && a != 0
        && a < 240
        && !(a == 100 && (64..=127).contains(&b))
        && !(a == 198 && (b == 18 || b == 19))
}

pub fn parse_header_value(headers: &str, name: &str) -> Option<String> {
    for line in headers.lines() {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case(name) {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

pub fn parse_http_url(url: &str) -> Option<(String, u16, String)> {
    if url
        .bytes()
        .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
    {
        return None;
    }
    let remainder = url.strip_prefix("http://")?;
    let (host_port, path) = match remainder.split_once('/') {
        Some((hp, p)) => (hp, format!("/{p}")),
        None => (remainder, "/".to_string()),
    };
    if host_port.is_empty() || host_port.contains('@') || remainder.contains('#') {
        return None;
    }
    let (host, port) = match host_port.split_once(':') {
        Some((h, p)) => (h.to_string(), p.parse::<u16>().ok()?),
        None => (host_port.to_string(), 80),
    };
    Some((host, port, path))
}

pub fn resolve_relative_url(base: &str, relative: &str) -> String {
    if relative.starts_with("http://") || relative.starts_with("https://") {
        return relative.to_string();
    }
    if let Some((host, port, _)) = parse_http_url(base) {
        let normalized = if relative.starts_with('/') {
            relative.to_string()
        } else {
            format!("/{relative}")
        };
        if port == 80 {
            format!("http://{host}{normalized}")
        } else {
            format!("http://{host}:{port}{normalized}")
        }
    } else {
        relative.to_string()
    }
}

pub fn extract_xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml[start..end].trim().to_string())
}

pub fn extract_upnp_service(xml: &str) -> Option<(String, String)> {
    let targets = [
        "urn:schemas-upnp-org:service:WANIPConnection:1",
        "urn:schemas-upnp-org:service:WANIPConnection:2",
        "urn:schemas-upnp-org:service:WANPPPConnection:1",
    ];

    for target in targets {
        if let Some(pos) = xml.find(target) {
            let snippet = &xml[pos..];
            if let Some(control_url) = extract_xml_tag(snippet, "controlURL") {
                return Some((target.to_string(), control_url));
            }
        }
    }
    None
}

async fn http_get(host: &str, port: u16, path: &str) -> Result<String, String> {
    // 게이트웨이 요청은 전부 시간·크기 제한이 있다 — 응답 없는(또는 사칭하는)
    // 응답자 앞에서 기동이 붙잡히지 않게 한다.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut stream = TcpStream::connect((host, port))
            .await
            .map_err(|e| format!("Connect to {host}:{port} failed: {e}"))?;

        let req = format!(
            "GET {path} HTTP/1.1\r\n\
             Host: {host}:{port}\r\n\
             Connection: close\r\n\
             User-Agent: Leftcar/1.0\r\n\r\n"
        );
        stream
            .write_all(req.as_bytes())
            .await
            .map_err(|e| format!("Write failed: {e}"))?;

        let mut response = Vec::new();
        stream
            .take(MAX_DESCRIPTION_BYTES as u64)
            .read_to_end(&mut response)
            .await
            .map_err(|e| format!("Read failed: {e}"))?;

        let text = String::from_utf8_lossy(&response);
        if let Some((_headers, body)) = text.split_once("\r\n\r\n") {
            Ok(body.to_string())
        } else {
            Ok(text.to_string())
        }
    })
    .await
    .map_err(|_| format!("Gateway description from {host}:{port} timed out"))?
}

async fn http_post(
    host: &str,
    port: u16,
    path: &str,
    action: &str,
    body: &str,
) -> Result<String, String> {
    tokio::time::timeout(std::time::Duration::from_secs(6), async {
        let mut stream = TcpStream::connect((host, port))
            .await
            .map_err(|e| format!("Connect to {host}:{port} failed: {e}"))?;

        let req = format!(
            "POST {path} HTTP/1.1\r\n\
             Host: {host}:{port}\r\n\
             Content-Type: text/xml; charset=\"utf-8\"\r\n\
             SOAPAction: {action}\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\r\n\
             {body}",
            body.len()
        );

        stream
            .write_all(req.as_bytes())
            .await
            .map_err(|e| format!("Write failed: {e}"))?;

        let mut response = Vec::new();
        stream
            .take(MAX_SOAP_RESPONSE_BYTES as u64)
            .read_to_end(&mut response)
            .await
            .map_err(|e| format!("Read failed: {e}"))?;

        let text = String::from_utf8_lossy(&response);
        if let Some((_headers, body)) = text.split_once("\r\n\r\n") {
            Ok(body.to_string())
        } else {
            Ok(text.to_string())
        }
    })
    .await
    .map_err(|_| format!("Gateway SOAP call to {host}:{port} timed out"))?
}

/// SSDP 응답자는 사설·링크 로컬 IPv4만 받아들인다 — 외부 주소가 응답하면
/// 그것은 우리 서브넷의 게이트웨이가 아니다.
fn is_local_gateway_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => v4.is_private() || v4.is_link_local(),
        std::net::IpAddr::V6(_) => false,
    }
}

/// 활성화 시 등록하는 (프로토콜, 외부·내부 포트, 설명) 목록. 컨트롤 TCP 한 개와
/// 미디어 UDP 전체 — UDP는 뷰어→호스트 미디어 진입로라 WAN 접속에 필수다.
fn mapping_plan(control_port: u16) -> Vec<(&'static str, u16, &'static str)> {
    let mut plan = vec![("TCP", control_port, CONTROL_DESCRIPTION)];
    plan.extend(
        MEDIA_PORTS
            .iter()
            .map(|&port| ("UDP", port, MEDIA_DESCRIPTION)),
    );
    plan
}

/// WAN 포트 매핑의 런타임 상태. 설정 토글(외부 접속 허용)이 즉시 효력을 갖도록
/// 활성화·해제·갱신을 이 객체 하나에서 조율한다.
pub struct UpnpMappingManager {
    active: tokio::sync::Mutex<Option<ActiveMapping>>,
    endpoint_notify: std::sync::Mutex<Option<EndpointNotify>>,
    generation: tokio::sync::watch::Sender<u64>,
    operation: std::sync::Mutex<()>,
}

type EndpointNotify = std::sync::Arc<dyn Fn(Option<String>) + Send + Sync>;
type Mapping = (&'static str, u16, &'static str);

struct ActiveMapping {
    gateway: UpnpGateway,
    generation: u64,
    control_port: u16,
    local_ip: String,
    acquired: Vec<Mapping>,
    renewal: Option<tokio::task::JoinHandle<()>>,
}

impl Default for UpnpMappingManager {
    fn default() -> Self {
        let (generation, _) = tokio::sync::watch::channel(0);
        Self {
            active: tokio::sync::Mutex::new(None),
            endpoint_notify: std::sync::Mutex::new(None),
            generation,
            operation: std::sync::Mutex::new(()),
        }
    }
}

impl UpnpMappingManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_endpoint_notify(&self, notify: EndpointNotify) {
        *self.endpoint_notify.lock().unwrap() = Some(notify);
    }

    fn notify_endpoint(&self, endpoint: Option<String>) {
        if let Some(notify) = self.endpoint_notify.lock().unwrap().as_ref() {
            notify(endpoint);
        }
    }

    fn begin_operation(&self) -> u64 {
        let _operation = self.operation.lock().unwrap();
        let mut next = 0;
        self.generation.send_modify(|value| {
            *value = value.wrapping_add(1);
            next = *value;
        });
        // Withdraw before any network await, including a stalled discovery or
        // router cleanup. A superseded enable cannot publish after this point.
        self.notify_endpoint(None);
        next
    }

    pub fn enable(
        self: std::sync::Arc<Self>,
        control_port: u16,
        local_ip: String,
    ) -> impl std::future::Future<Output = ()> {
        self.enable_gateway(control_port, local_ip, None)
    }

    fn enable_gateway(
        self: std::sync::Arc<Self>,
        control_port: u16,
        local_ip: String,
        provided_gateway: Option<UpnpGateway>,
    ) -> impl std::future::Future<Output = ()> {
        let unchanged = self.active.try_lock().ok().is_some_and(|active| {
            active.as_ref().is_some_and(|active| {
                active.control_port == control_port
                    && active.local_ip == local_ip
                    && active.generation == *self.generation.borrow()
                    && active
                        .renewal
                        .as_ref()
                        .is_some_and(|renewal| !renewal.is_finished())
            })
        });
        // Reserve intent when the future is CREATED, not when a background
        // executor first polls it. An older queued enable must never win over
        // a newer disable just because it was scheduled later.
        let generation = (!unchanged).then(|| self.begin_operation());
        async move {
            if let Some(generation) = generation {
                self.apply_gateway(generation, control_port, local_ip, provided_gateway)
                    .await;
            }
        }
    }

    async fn apply_gateway(
        self: std::sync::Arc<Self>,
        generation: u64,
        control_port: u16,
        local_ip: String,
        provided_gateway: Option<UpnpGateway>,
    ) {
        let mut changes = self.generation.subscribe();
        let mut active = self.active.lock().await;
        if *self.generation.borrow() != generation {
            return;
        }
        // Cleanup is bounded and outside the cancelable setup. Superseding an
        // enable must not drop the only owner of ports still being deleted.
        if let Some(previous) = active.take() {
            release_mapping(previous).await;
        }
        let work = async {
            let gateway = match provided_gateway {
                Some(gateway) => gateway,
                None => match UpnpGateway::discover(Duration::from_secs(2)).await {
                    Some(gateway) => gateway,
                    None => return,
                },
            };
            let external_ip = match gateway.get_external_ip().await {
                Ok(ip) => ip,
                Err(error) => {
                    eprintln!("UPnP: {error}");
                    return;
                }
            };
            *active = Some(ActiveMapping {
                gateway: gateway.clone(),
                generation,
                control_port,
                local_ip: local_ip.clone(),
                acquired: Vec::new(),
                renewal: None,
            });
            for mapping @ (protocol, port, description) in mapping_plan(control_port) {
                let verified = tokio::time::timeout(
                    Duration::from_secs(2),
                    gateway.verify_mapping_owner(protocol, port, &local_ip, description),
                )
                .await;
                if !matches!(verified, Ok(Ok(()))) {
                    return;
                }
                // AddPortMapping may be applied before its response is lost or
                // cancelled. Retain the candidate before sending; cleanup will
                // re-query ownership and delete only matching entries.
                active.as_mut().unwrap().acquired.push(mapping);
                let acquired = tokio::time::timeout(
                    Duration::from_secs(2),
                    gateway.add_port_mapping(
                        protocol,
                        port,
                        port,
                        &local_ip,
                        description,
                        MAPPING_LEASE_SECS,
                    ),
                )
                .await;
                match acquired {
                    Ok(Ok(())) => {}
                    _ => {
                        eprintln!("UPnP: {protocol} {port} could not be acquired; external endpoint unavailable");
                        return;
                    }
                }
            }
            // No await between the final generation check and publication.
            let current = self.generation.borrow();
            if *current != generation {
                return;
            }
            self.notify_endpoint(Some(format!("{external_ip}:{}", MEDIA_PORTS[0])));
            active.as_mut().unwrap().renewal = Some(tokio::spawn(renewal_loop(
                self.clone(),
                generation,
                control_port,
                local_ip,
                gateway,
            )));
        };
        // Selecting the entire setup also interrupts an outstanding HTTP call
        // and releases the active lock so disable can collect acquired ports.
        if *changes.borrow_and_update() != generation {
            return;
        }
        tokio::select! {
            biased;
            _ = changes.changed() => {}
            _ = work => {}
        }
        if active
            .as_ref()
            .is_some_and(|mapping| mapping.renewal.is_none())
        {
            if let Some(partial) = active.take() {
                release_mapping(partial).await;
            }
        }
    }

    pub fn disable(self: std::sync::Arc<Self>) -> impl std::future::Future<Output = ()> {
        let generation = self.begin_operation();
        async move { self.apply_disable(generation).await }
    }

    async fn apply_disable(&self, generation: u64) {
        let mut active = self.active.lock().await;
        if *self.generation.borrow() != generation {
            return;
        }
        // Retain serialization during cleanup: a newer enable must not acquire
        // these same ports while the older disable is still deleting them.
        if let Some(mapping) = active.take() {
            release_mapping(mapping).await;
        }
    }

    async fn drop_active(&self, generation: u64) {
        let mut active = self.active.lock().await;
        let current = self.generation.borrow();
        if *current == generation {
            if let Some(mut active) = active.take() {
                if let Some(renewal) = active.renewal.take() {
                    renewal.abort();
                }
            }
            self.notify_endpoint(None);
        }
    }
}

/// Cleanup checks ownership again: another application may have replaced an
/// entry since our successful AddPortMapping. Unknown ownership is never deleted.
async fn release_mapping(mut active: ActiveMapping) {
    if let Some(renewal) = active.renewal.take() {
        renewal.abort();
    }
    let mut cleanup = tokio::task::JoinSet::new();
    for (protocol, port, description) in active.acquired {
        let gateway = active.gateway.clone();
        let local_ip = active.local_ip.clone();
        cleanup.spawn(async move {
            let _ = tokio::time::timeout(Duration::from_secs(2), async {
                if gateway
                    .mapping_owner(protocol, port)
                    .await
                    .ok()
                    .flatten()
                    .is_some_and(|owner| owner.matches(&local_ip, port, description))
                {
                    let _ = gateway.delete_port_mapping(protocol, port).await;
                }
            })
            .await;
        });
    }
    while cleanup.join_next().await.is_some() {}
}

async fn renewal_loop(
    manager: std::sync::Arc<UpnpMappingManager>,
    generation: u64,
    control_port: u16,
    local_ip: String,
    gateway: UpnpGateway,
) {
    let mut changes = manager.generation.subscribe();
    let renew = async {
        let mut consecutive_failures = 0u32;
        loop {
            tokio::time::sleep(Duration::from_secs(RENEWAL_INTERVAL_SECS)).await;
            let mut failed = false;
            for (protocol, port, description) in mapping_plan(control_port) {
                if !matches!(
                    tokio::time::timeout(
                        Duration::from_secs(4),
                        gateway.acquire_mapping(protocol, port, &local_ip, description),
                    )
                    .await,
                    Ok(Ok(()))
                ) {
                    failed = true;
                }
            }
            if failed {
                consecutive_failures += 1;
                {
                    let current = manager.generation.borrow();
                    if *current == generation {
                        manager.notify_endpoint(None);
                    }
                }
                if consecutive_failures >= 3 {
                    manager.drop_active(generation).await;
                    return;
                }
            } else {
                consecutive_failures = 0;
                let endpoint = gateway
                    .get_external_ip()
                    .await
                    .ok()
                    .map(|ip| format!("{ip}:{}", MEDIA_PORTS[0]));
                let current = manager.generation.borrow();
                if *current == generation {
                    manager.notify_endpoint(endpoint);
                }
            }
        }
    };
    if *changes.borrow_and_update() != generation {
        return;
    }
    tokio::select! {
        biased;
        _ = changes.changed() => {}
        _ = renew => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct RouterState {
        mappings: HashMap<(String, u16), MappingOwner>,
        writes: Vec<(String, String, u16)>,
        fail_add: Option<u16>,
        drop_add_response: Option<u16>,
        delay_add_response: Option<u16>,
        applied: Arc<tokio::sync::Notify>,
        release_response: Arc<tokio::sync::Notify>,
    }

    struct TestRouter {
        gateway: UpnpGateway,
        state: Arc<Mutex<RouterState>>,
        task: tokio::task::JoinHandle<()>,
    }

    impl Drop for TestRouter {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn test_router() -> TestRouter {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let state = Arc::new(Mutex::new(RouterState::default()));
        let shared = state.clone();
        let task = tokio::spawn(async move {
            let mut requests = tokio::task::JoinSet::new();
            while let Ok((mut socket, _)) = listener.accept().await {
                let shared = shared.clone();
                requests.spawn(async move {
                let mut request = Vec::new();
                loop {
                    let mut bytes = [0u8; 4096];
                    let size = socket.read(&mut bytes).await.unwrap();
                    if size == 0 {
                        break;
                    }
                    request.extend_from_slice(&bytes[..size]);
                    let text = String::from_utf8_lossy(&request);
                    if let Some((headers, body)) = text.split_once("\r\n\r\n") {
                        let len = parse_header_value(headers, "Content-Length")
                            .unwrap()
                            .parse::<usize>()
                            .unwrap();
                        if body.len() >= len {
                            break;
                        }
                    }
                }
                let request = String::from_utf8(request).unwrap();
                let response = {
                    let mut state = shared.lock().unwrap();
                    if request.contains("#GetExternalIPAddress") {
                        "<NewExternalIPAddress>8.8.8.8</NewExternalIPAddress>".to_owned()
                    } else {
                        let protocol = extract_xml_tag(&request, "NewProtocol").unwrap();
                        let port = extract_xml_tag(&request, "NewExternalPort")
                            .unwrap()
                            .parse()
                            .unwrap();
                        let key = (protocol.clone(), port);
                        if request.contains("#GetSpecificPortMappingEntry") {
                            match state.mappings.get(&key) {
                                Some(owner) => format!("<GetSpecificPortMappingEntryResponse><NewInternalClient>{}</NewInternalClient><NewInternalPort>{}</NewInternalPort><NewPortMappingDescription>{}</NewPortMappingDescription></GetSpecificPortMappingEntryResponse>", owner.client, owner.port, owner.description),
                                None => "<errorCode>714</errorCode>".to_owned(),
                            }
                        } else if request.contains("#AddPortMapping") {
                            state.writes.push(("add".into(), protocol, port));
                            if state.fail_add == Some(port) {
                                "<errorCode>718</errorCode>".to_owned()
                            } else {
                                state.mappings.insert(
                                    key,
                                    MappingOwner {
                                        client: extract_xml_tag(&request, "NewInternalClient")
                                            .unwrap(),
                                        port: extract_xml_tag(&request, "NewInternalPort")
                                            .unwrap()
                                            .parse()
                                            .unwrap(),
                                        description: extract_xml_tag(
                                            &request,
                                            "NewPortMappingDescription",
                                        )
                                        .unwrap(),
                                    },
                                );
                                "<AddPortMappingResponse/>".to_owned()
                            }
                        } else {
                            state.writes.push(("delete".into(), protocol, port));
                            state.mappings.remove(&key);
                            "<DeletePortMappingResponse/>".to_owned()
                        }
                    }
                };
                let (drop_response, delay_response, applied, release) = {
                    let state = shared.lock().unwrap();
                    let added_port = request.contains("#AddPortMapping")
                        .then(|| extract_xml_tag(&request, "NewExternalPort").unwrap().parse::<u16>().unwrap());
                    (
                        added_port.is_some() && added_port == state.drop_add_response,
                        added_port.is_some() && added_port == state.delay_add_response,
                        state.applied.clone(), state.release_response.clone(),
                    )
                };
                if drop_response { return; }
                if delay_response {
                    applied.notify_one();
                    release.notified().await;
                }
                let _ = socket
                    .write_all(
                        format!("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{response}")
                            .as_bytes(),
                    )
                    .await;
                });
            }
        });
        TestRouter {
            gateway: UpnpGateway {
                location: format!("http://{addr}/desc"),
                host: addr.ip().to_string(),
                port: addr.port(),
                control_url: format!("http://{addr}/ctl"),
                service_type: SSDP_SEARCH_TARGETS[1].into(),
            },
            state,
            task,
        }
    }

    type ObservedEndpoints = Arc<Mutex<Vec<Option<String>>>>;

    fn observed_manager() -> (Arc<UpnpMappingManager>, ObservedEndpoints) {
        let manager = Arc::new(UpnpMappingManager::new());
        let endpoints = Arc::new(Mutex::new(Vec::new()));
        let observed = endpoints.clone();
        manager.set_endpoint_notify(Arc::new(move |value| observed.lock().unwrap().push(value)));
        (manager, endpoints)
    }

    fn foreign_owner(port: u16) -> MappingOwner {
        MappingOwner {
            client: "192.168.0.99".into(),
            port,
            description: "Other application".into(),
        }
    }

    #[tokio::test]
    async fn foreign_mapping_is_never_replaced_or_deleted() {
        let router = test_router().await;
        router
            .state
            .lock()
            .unwrap()
            .mappings
            .insert(("TCP".into(), 7777), foreign_owner(7777));
        let (manager, endpoints) = observed_manager();
        manager
            .clone()
            .enable_gateway(7777, "192.168.0.5".into(), Some(router.gateway.clone()))
            .await;
        manager.clone().disable().await;
        let state = router.state.lock().unwrap();
        assert!(state.writes.is_empty());
        assert_eq!(state.mappings.len(), 1);
        assert!(endpoints.lock().unwrap().iter().all(Option::is_none));
    }

    #[tokio::test]
    async fn partial_mapping_failure_rolls_back_without_advertising() {
        let router = test_router().await;
        router.state.lock().unwrap().fail_add = Some(5002);
        let (manager, endpoints) = observed_manager();
        manager
            .clone()
            .enable_gateway(7777, "192.168.0.5".into(), Some(router.gateway.clone()))
            .await;
        {
            let state = router.state.lock().unwrap();
            assert!(state.mappings.is_empty());
            assert_eq!(
                state
                    .writes
                    .iter()
                    .filter(|(action, _, _)| action == "delete")
                    .count(),
                2
            );
        }
        assert!(endpoints.lock().unwrap().iter().all(Option::is_none));
        assert!(manager.active.lock().await.is_none());
    }

    #[tokio::test]
    async fn applied_mapping_with_lost_response_is_reconciled_and_removed() {
        let router = test_router().await;
        router.state.lock().unwrap().drop_add_response = Some(5001);
        let (manager, endpoints) = observed_manager();
        manager
            .clone()
            .enable_gateway(7777, "192.168.0.5".into(), Some(router.gateway.clone()))
            .await;
        assert!(router.state.lock().unwrap().mappings.is_empty());
        assert!(endpoints.lock().unwrap().iter().all(Option::is_none));
    }

    #[tokio::test]
    async fn cancel_after_router_applies_add_cleans_the_unacknowledged_mapping() {
        let router = test_router().await;
        let applied = {
            let mut state = router.state.lock().unwrap();
            state.delay_add_response = Some(5001);
            state.applied.clone()
        };
        let (manager, endpoints) = observed_manager();
        let enable = tokio::spawn(manager.clone().enable_gateway(
            7777,
            "192.168.0.5".into(),
            Some(router.gateway.clone()),
        ));
        applied.notified().await;
        assert_eq!(router.state.lock().unwrap().mappings.len(), 2);
        tokio::time::timeout(Duration::from_secs(1), manager.clone().disable())
            .await
            .unwrap();
        enable.await.unwrap();
        assert!(router.state.lock().unwrap().mappings.is_empty());
        assert!(endpoints.lock().unwrap().iter().all(Option::is_none));
    }

    #[tokio::test]
    async fn successful_mapping_publishes_and_cleanup_preserves_replaced_ports() {
        let router = test_router().await;
        let (manager, endpoints) = observed_manager();
        manager
            .clone()
            .enable_gateway(7777, "192.168.0.5".into(), Some(router.gateway.clone()))
            .await;
        assert_eq!(
            endpoints.lock().unwrap().last(),
            Some(&Some("8.8.8.8:5001".into()))
        );
        assert_eq!(router.state.lock().unwrap().mappings.len(), 6);
        router
            .state
            .lock()
            .unwrap()
            .mappings
            .insert(("UDP".into(), 5001), foreign_owner(5001));
        manager.clone().disable().await;
        assert_eq!(endpoints.lock().unwrap().last(), Some(&None));
        let state = router.state.lock().unwrap();
        assert_eq!(state.mappings.len(), 1);
        assert_eq!(state.mappings[&("UDP".into(), 5001)].client, "192.168.0.99");
        assert!(!state
            .writes
            .iter()
            .any(|(action, _, port)| action == "delete" && *port == 5001));
    }

    #[tokio::test]
    async fn repeated_enable_is_idempotent_and_port_change_retires_the_old_mapping() {
        let router = test_router().await;
        let (manager, endpoints) = observed_manager();
        manager
            .clone()
            .enable_gateway(7777, "192.168.0.5".into(), Some(router.gateway.clone()))
            .await;
        let old_renewal = manager
            .active
            .lock()
            .await
            .as_ref()
            .unwrap()
            .renewal
            .as_ref()
            .unwrap()
            .id();
        manager
            .clone()
            .enable_gateway(7777, "192.168.0.5".into(), Some(router.gateway.clone()))
            .await;
        assert_eq!(router.state.lock().unwrap().writes.len(), 6);
        assert_eq!(endpoints.lock().unwrap().len(), 2);
        assert_eq!(
            manager
                .active
                .lock()
                .await
                .as_ref()
                .unwrap()
                .renewal
                .as_ref()
                .unwrap()
                .id(),
            old_renewal
        );
        manager
            .clone()
            .enable_gateway(7778, "192.168.0.5".into(), Some(router.gateway.clone()))
            .await;
        {
            let state = router.state.lock().unwrap();
            assert_eq!(state.mappings.len(), 6);
            assert!(!state.mappings.contains_key(&("TCP".into(), 7777)));
            assert!(state.mappings.contains_key(&("TCP".into(), 7778)));
        }
        assert_ne!(
            manager
                .active
                .lock()
                .await
                .as_ref()
                .unwrap()
                .renewal
                .as_ref()
                .unwrap()
                .id(),
            old_renewal
        );
        manager.clone().disable().await;
        assert!(router.state.lock().unwrap().mappings.is_empty());
    }

    #[tokio::test]
    async fn disable_cancels_stalled_enable_before_a_late_endpoint_can_publish() {
        let router = test_router().await;
        router.task.abort();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut gateway = router.gateway.clone();
        gateway.host = addr.ip().to_string();
        gateway.port = addr.port();
        gateway.control_url = format!("http://{addr}/ctl");
        let (manager, endpoints) = observed_manager();
        let enable = tokio::spawn(manager.clone().enable_gateway(
            7777,
            "192.168.0.5".into(),
            Some(gateway),
        ));
        let (_stalled, _) = listener.accept().await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), manager.clone().disable())
            .await
            .expect("disable must interrupt setup, not await the gateway timeout");
        enable.await.unwrap();
        assert!(manager.active.lock().await.is_none());
        assert!(endpoints.lock().unwrap().iter().all(Option::is_none));
    }

    #[tokio::test]
    async fn superseded_disable_cannot_retire_a_newer_enable() {
        let router = test_router().await;
        let (manager, _) = observed_manager();
        let active = manager.active.lock().await;
        let mut changes = manager.generation.subscribe();
        let stopping = manager.clone();
        let disable = tokio::spawn(async move { stopping.disable().await });
        changes.changed().await.unwrap();
        let enable = tokio::spawn(manager.clone().enable_gateway(
            7777,
            "192.168.0.5".into(),
            Some(router.gateway.clone()),
        ));
        changes.changed().await.unwrap();
        drop(active);
        disable.await.unwrap();
        enable.await.unwrap();
        assert_eq!(router.state.lock().unwrap().mappings.len(), 6);
        assert!(manager.active.lock().await.is_some());
        manager.clone().disable().await;
    }

    #[tokio::test]
    async fn queued_enable_cannot_reopen_ports_after_a_later_disable() {
        let router = test_router().await;
        let (manager, endpoints) = observed_manager();
        let queued = manager.clone().enable_gateway(
            7777,
            "192.168.0.5".into(),
            Some(router.gateway.clone()),
        );
        manager.clone().disable().await;
        queued.await;
        assert!(router.state.lock().unwrap().writes.is_empty());
        assert!(endpoints.lock().unwrap().iter().all(Option::is_none));
    }

    #[tokio::test]
    async fn queued_disable_cannot_close_ports_from_a_later_enable() {
        let router = test_router().await;
        let (manager, endpoints) = observed_manager();
        let queued = manager.clone().disable();
        manager
            .clone()
            .enable_gateway(7777, "192.168.0.5".into(), Some(router.gateway.clone()))
            .await;
        queued.await;
        assert_eq!(router.state.lock().unwrap().mappings.len(), 6);
        assert_eq!(
            endpoints.lock().unwrap().last(),
            Some(&Some("8.8.8.8:5001".into()))
        );
        manager.clone().disable().await;
    }

    #[test]
    fn non_public_gateway_addresses_are_not_advertised() {
        for address in [
            "0.0.0.0",
            "10.0.0.1",
            "127.0.0.1",
            "100.64.0.1",
            "100.127.255.254",
            "169.254.1.1",
            "198.18.0.1",
            "192.0.2.1",
            "224.0.0.1",
            "255.255.255.255",
        ] {
            assert!(!is_public_ipv4(address.parse().unwrap()), "{address}");
        }
        assert!(is_public_ipv4("8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn parses_header_case_insensitively() {
        let headers =
            "HTTP/1.1 200 OK\r\nLocation: http://192.168.0.1:1900/desc.xml\r\nServer: MiniUPnP\r\n";
        assert_eq!(
            parse_header_value(headers, "LOCATION"),
            Some("http://192.168.0.1:1900/desc.xml".to_string())
        );
        assert_eq!(
            parse_header_value(headers, "location"),
            Some("http://192.168.0.1:1900/desc.xml".to_string())
        );
        assert_eq!(parse_header_value(headers, "FOO"), None);
    }

    #[test]
    fn parses_http_urls_correctly() {
        let (host, port, path) =
            parse_http_url("http://192.168.0.1:1900/udtpg/rootDesc.xml").unwrap();
        assert_eq!(host, "192.168.0.1");
        assert_eq!(port, 1900);
        assert_eq!(path, "/udtpg/rootDesc.xml");

        let (host, port, path) = parse_http_url("http://router.local/").unwrap();
        assert_eq!(host, "router.local");
        assert_eq!(port, 80);
        assert_eq!(path, "/");
        for url in [
            "http://192.168.0.1/path\r\nInjected: yes",
            "http://192.168.0.1/bad path",
            "http://user@192.168.0.1/ctl",
            "http:///ctl",
            "http://192.168.0.1/ctl#fragment",
        ] {
            assert!(parse_http_url(url).is_none(), "{url}");
        }
    }

    #[test]
    fn resolves_relative_urls_against_base() {
        let base = "http://192.168.0.1:1900/udtpg/rootDesc.xml";
        assert_eq!(
            resolve_relative_url(base, "/udtpg/ctl/IPConn"),
            "http://192.168.0.1:1900/udtpg/ctl/IPConn"
        );
        assert_eq!(
            resolve_relative_url(base, "ctl/IPConn"),
            "http://192.168.0.1:1900/ctl/IPConn"
        );
        assert_eq!(
            resolve_relative_url(base, "http://another.router:5000/ctl"),
            "http://another.router:5000/ctl"
        );
    }

    #[test]
    fn extracts_xml_tag_values() {
        let xml = "<root><NewExternalIPAddress> 1.217.35.59 </NewExternalIPAddress></root>";
        assert_eq!(
            extract_xml_tag(xml, "NewExternalIPAddress"),
            Some("1.217.35.59".to_string())
        );
        assert_eq!(extract_xml_tag(xml, "NonExistent"), None);
    }

    #[test]
    fn extracts_service_and_control_url() {
        let xml = r#"
            <serviceList>
              <service>
                <serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>
                <controlURL>/udtpg/ctl/IPConn</controlURL>
              </service>
            </serviceList>
        "#;
        let (service, url) = extract_upnp_service(xml).unwrap();
        assert_eq!(service, "urn:schemas-upnp-org:service:WANIPConnection:1");
        assert_eq!(url, "/udtpg/ctl/IPConn");
    }

    #[test]
    fn accepts_only_local_subnet_gateway_responders() {
        assert!(is_local_gateway_ip("192.168.0.1".parse().unwrap()));
        assert!(is_local_gateway_ip("10.0.0.138".parse().unwrap()));
        assert!(is_local_gateway_ip("172.16.1.1".parse().unwrap()));
        assert!(is_local_gateway_ip("169.254.12.34".parse().unwrap()));
        // 외부·루프백 주소의 응답자는 게이트웨이가 아니다.
        assert!(!is_local_gateway_ip("8.8.8.8".parse().unwrap()));
        assert!(!is_local_gateway_ip("203.0.113.7".parse().unwrap()));
        assert!(!is_local_gateway_ip("::1".parse().unwrap()));
    }

    #[test]
    fn rejects_control_url_on_a_different_host() {
        let location = "http://192.168.0.1:1900/rootDesc.xml";
        assert_eq!(
            parse_http_url(location).map(|(host, _, _)| host),
            Some("192.168.0.1".to_string())
        );
        // resolve_description은 controlURL 호스트가 LOCATION 호스트와 다르면
        // None이다. 이 테스트는 같은 판정식의 순수한 절반을 고정한다.
        let control_url = resolve_relative_url(location, "http://evil.example/ctl");
        let (gw_host, _, _) = parse_http_url(&control_url).unwrap();
        assert_ne!(gw_host, "192.168.0.1");
    }

    #[tokio::test]
    async fn add_port_mapping_sends_tcp_mapping_with_lease_and_description() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let n = sock.read(&mut buf).await.unwrap();
            sock.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\n\r\n\
                  <s:Envelope><s:Body><u:AddPortMappingResponse \
                  xmlns:u=\"urn:schemas-upnp-org:service:WANIPConnection:1\"/>\
                  </s:Body></s:Envelope>",
            )
            .await
            .unwrap();
            String::from_utf8_lossy(&buf[..n]).to_string()
        });
        let gateway = UpnpGateway {
            location: format!("http://{addr}/desc"),
            host: addr.ip().to_string(),
            port: addr.port(),
            control_url: format!("http://{addr}/ctl"),
            service_type: "urn:schemas-upnp-org:service:WANIPConnection:1".to_string(),
        };
        gateway
            .add_port_mapping(
                "TCP",
                7777,
                7777,
                "192.168.0.5",
                CONTROL_DESCRIPTION,
                MAPPING_LEASE_SECS,
            )
            .await
            .unwrap();
        let request = handle.await.unwrap();
        assert!(
            request.contains("<NewProtocol>TCP</NewProtocol>"),
            "{request}"
        );
        assert!(
            request.contains("<NewExternalPort>7777</NewExternalPort>"),
            "{request}"
        );
        assert!(
            request.contains(&format!(
                "<NewLeaseDuration>{MAPPING_LEASE_SECS}</NewLeaseDuration>"
            )),
            "{request}"
        );
        assert!(request.contains(CONTROL_DESCRIPTION), "{request}");
    }

    #[tokio::test]
    async fn delete_port_mapping_reports_rejection() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let _ = sock.read(&mut buf).await.unwrap();
            sock.write_all(
                b"HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/xml\r\n\r\n\
                  <s:Envelope><s:Body><s:Fault>UPnPIgnore</s:Fault></s:Body></s:Envelope>",
            )
            .await
            .unwrap();
        });
        let gateway = UpnpGateway {
            location: format!("http://{addr}/desc"),
            host: addr.ip().to_string(),
            port: addr.port(),
            control_url: format!("http://{addr}/ctl"),
            service_type: "urn:schemas-upnp-org:service:WANIPConnection:1".to_string(),
        };
        let error = gateway.delete_port_mapping("UDP", 5001).await.unwrap_err();
        assert!(error.contains("rejected"), "{error}");
        handle.await.unwrap();
    }

    #[test]
    fn mapping_plan_covers_control_and_every_media_port() {
        let plan = mapping_plan(7777);
        assert_eq!(plan[0], ("TCP", 7777, CONTROL_DESCRIPTION));
        let udp_ports: Vec<u16> = plan[1..].iter().map(|(_, port, _)| *port).collect();
        assert_eq!(udp_ports, MEDIA_PORTS.to_vec());
        assert!(plan[1..].iter().all(|(protocol, _, description)| {
            *protocol == "UDP" && *description == MEDIA_DESCRIPTION
        }));
    }
}
