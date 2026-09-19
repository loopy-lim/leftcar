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
    let remainder = url.strip_prefix("http://")?;
    let (host_port, path) = match remainder.split_once('/') {
        Some((hp, p)) => (hp, format!("/{p}")),
        None => (remainder, "/".to_string()),
    };
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
    plan.extend(MEDIA_PORTS.iter().map(|&port| ("UDP", port, MEDIA_DESCRIPTION)));
    plan
}

/// WAN 포트 매핑의 런타임 상태. 설정 토글(외부 접속 허용)이 즉시 효력을 갖도록
/// 활성화·해제·갱신을 이 객체 하나에서 조율한다.
pub struct UpnpMappingManager {
    active: tokio::sync::Mutex<Option<ActiveMapping>>,
    endpoint_notify: std::sync::Mutex<Option<std::sync::Arc<dyn Fn(Option<String>) + Send + Sync>>>,
}

struct ActiveMapping {
    gateway: UpnpGateway,
    control_port: u16,
    renewal: tokio::task::JoinHandle<()>,
}

impl UpnpMappingManager {
    pub fn new() -> Self {
        Self {
            active: tokio::sync::Mutex::new(None),
            endpoint_notify: std::sync::Mutex::new(None),
        }
    }

    /// 공개 미디어 엔드포인트가 바뀔 때 호스트에 알리는 콜백. 상태 스냅숏이 이
    /// 값을 뷰어에 광고하며, 매핑이 없으면 None으로 지운다.
    pub fn set_endpoint_notify(
        &self,
        notify: std::sync::Arc<dyn Fn(Option<String>) + Send + Sync>,
    ) {
        *self.endpoint_notify.lock().unwrap() = Some(notify);
    }

    fn notify_endpoint(&self, endpoint: Option<String>) {
        if let Some(notify) = self.endpoint_notify.lock().unwrap().as_ref() {
            notify(endpoint);
        }
    }

    /// 게이트웨이를 찾아 컨트롤(TCP)·미디어(UDP) 매핑을 (재)등록하고 갱신
    /// 태스크를 심는다. 같은 컨트롤 포트로 이미 활성화돼 있으면 아무 것도 하지
    /// 않는다. 재등록 전에 같은 포트의 남은 매핑을 지운다 — 이전 실행이 포트가
    /// 달라졌을 때 남긴 매핑은 lease가 만료시킨다.
    pub async fn enable(self: std::sync::Arc<Self>, control_port: u16, local_ip: String) {
        let mut active = self.active.lock().await;
        if let Some(current) = active.as_ref() {
            if current.control_port == control_port {
                return;
            }
        }
        println!("UPnP: probing for IGD gateway...");
        let Some(gateway) = UpnpGateway::discover(std::time::Duration::from_secs(2)).await else {
            println!("UPnP: no IGD gateway found (continuing LAN/Tailscale direct)");
            return;
        };
        println!("UPnP: found gateway at {}", gateway.location);
        let external_ip = match gateway.get_external_ip().await {
            Ok(ip) => ip,
            Err(error) => {
                eprintln!("UPnP: failed to get external IP: {error}");
                return;
            }
        };
        println!("UPnP: external public IP is {external_ip}");

        for (protocol, port, description) in mapping_plan(control_port) {
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                gateway.delete_port_mapping(protocol, port),
            )
            .await;
            let added = tokio::time::timeout(
                std::time::Duration::from_secs(2),
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
            match added {
                Ok(Ok(())) => {
                    println!("UPnP: mapped {protocol} {port} -> {local_ip}:{port}");
                }
                Ok(Err(error)) => {
                    eprintln!("UPnP: failed to map {protocol} {port}: {error}");
                }
                Err(_) => {
                    eprintln!("UPnP: mapping {protocol} {port} timed out");
                }
            }
        }

        self.notify_endpoint(Some(format!("{external_ip}:{}", MEDIA_PORTS[0])));
        let renewal = tokio::spawn(renewal_loop(
            self.clone(),
            control_port,
            local_ip,
            gateway.clone(),
        ));
        *active = Some(ActiveMapping {
            gateway,
            control_port,
            renewal,
        });
    }

    /// 매핑을 모두 해제한다(토글 off·호스트 종료). 개별 삭제는 2초로 제한한다 —
    /// 응답 없는 게이트웨이 앞에서 종료가 붙잡히지 않게 하고, 못 치운 매핑은
    /// lease가 만료시킨다.
    pub async fn disable(&self) {
        let Some(active) = self.active.lock().await.take() else {
            return;
        };
        active.renewal.abort();
        for (protocol, port, _) in mapping_plan(active.control_port) {
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                active.gateway.delete_port_mapping(protocol, port),
            )
            .await;
        }
        self.notify_endpoint(None);
    }

    /// 갱신이 반복 실패해 매핑 상태만 내려놓는다. 삭제는 시도하지 않는다 —
    /// 게이트웨이가 응답하지 않는 상황이고, lease가 라우터 쪽을 만료시킨다.
    async fn drop_active(&self) {
        if let Some(active) = self.active.lock().await.take() {
            active.renewal.abort();
            self.notify_endpoint(None);
        }
    }
}

/// lease 절반 간격으로 매핑을 재등록한다. 3회 연속 실패하면 매핑 상태를
/// 내려놓는다(공개 엔드포인트 광고도 끊긴다) — 다음 토글·기동이 다시 시도한다.
async fn renewal_loop(
    manager: std::sync::Arc<UpnpMappingManager>,
    control_port: u16,
    local_ip: String,
    gateway: UpnpGateway,
) {
    let mut consecutive_failures = 0u32;
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(RENEWAL_INTERVAL_SECS)).await;
        let mut failed = false;
        for (protocol, port, description) in mapping_plan(control_port) {
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(2),
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
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    failed = true;
                    eprintln!("UPnP: renewal {protocol} {port} failed: {error}");
                }
                Err(_) => {
                    failed = true;
                    eprintln!("UPnP: renewal {protocol} {port} timed out");
                }
            }
        }
        if failed {
            consecutive_failures += 1;
            if consecutive_failures >= 3 {
                eprintln!("UPnP: renewal failed 3 times in a row, dropping mapping state");
                manager.drop_active().await;
                return;
            }
        } else {
            consecutive_failures = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_header_case_insensitively() {
        let headers = "HTTP/1.1 200 OK\r\nLocation: http://192.168.0.1:1900/desc.xml\r\nServer: MiniUPnP\r\n";
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
            .add_port_mapping("TCP", 7777, 7777, "192.168.0.5", CONTROL_DESCRIPTION, MAPPING_LEASE_SECS)
            .await
            .unwrap();
        let request = handle.await.unwrap();
        assert!(request.contains("<NewProtocol>TCP</NewProtocol>"), "{request}");
        assert!(request.contains("<NewExternalPort>7777</NewExternalPort>"), "{request}");
        assert!(
            request.contains(&format!("<NewLeaseDuration>{MAPPING_LEASE_SECS}</NewLeaseDuration>")),
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
