//! 패널 프로토콜(AC-H1) 클라이언트 — `leftcar-cli`가 실행 중 호스트를
//! 진단·제어하기 위해 쓴다. 프로토콜 자체는 [`crate::panel`] 문서를 따른다:
//! 첫 줄 토큰 인증, 이후 개행 JSON 요청·응답. 요청 `id`는 프로토콜 요소라
//! 매기지만 단일 연결 순차 프로토콜이라 응답 `id` 검증은 생략한다(서버는
//! 받은 `id`를 그대로 에코한다).
//!
//! 서버와 같은 "느긋한 연결" 의미론 — 한 연결에서 여러 명령을 순서대로
//! 부를 수 있고, `ok:false` 응답이 연결을 죽이지 않는다. CLI는 명령 하나를
//! 마치면 프로세스가 끝나며 연결도 닫힌다.

use std::io::{BufReader, Write};
use std::net::TcpStream;
use std::path::Path;

/// 응답 줄 상한. 서버가 내보내는 가장 큰 응답(기기 목록·파일 공유 대기열)도
/// 훨씬 밑돈다. 요청 상한(1MiB)과 별개 스코프다.
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

/// 연결이 예고 없이 끊겼을 때의 안내 — 토큰 불일치와 호스트 재시작이
/// 클라이언트에서는 구분되지 않는다.
fn connection_closed() -> String {
    "패널이 연결을 닫았습니다 — 토큰 불일치이거나 호스트가 재시작됐습니다 \
     (panel-token 파일이 오래됐으면 호스트를 다시 띄우세요)"
        .to_string()
}

#[derive(Debug)]
pub struct PanelClient {
    writer: TcpStream,
    reader: BufReader<TcpStream>,
    next_id: u64,
}

impl PanelClient {
    /// 루프백 패널에 연결하고 첫 줄 인증을 보낸다. 토큰 불일치는 서버가
    /// 즉시 연결을 끊으므로 이 시점에는 잡히지 않고, 첫 [`PanelClient::call`]
    ///에서 EOF로 드러난다.
    pub fn connect(port: u16, token: &str) -> Result<Self, String> {
        let stream = TcpStream::connect(("127.0.0.1", port)).map_err(|error| {
            format!(
                "패널 포트 {port} 연결 실패: {error} — leftcar 호스트가 실행 중인지 확인하세요"
            )
        })?;
        let _ = stream.set_nodelay(true);
        let writer = stream
            .try_clone()
            .map_err(|error| format!("패널 스트림 복제 실패: {error}"))?;
        let mut client = Self {
            writer,
            reader: BufReader::new(stream),
            next_id: 0,
        };
        client
            .writer
            .write_all(token.as_bytes())
            .and_then(|_| client.writer.write_all(b"\n"))
            .and_then(|_| client.writer.flush())
            .map_err(|error| format!("패널 인증 전송 실패: {error}"))?;
        Ok(client)
    }

    /// 명령 하나를 부르고 `ok:true`의 `result`를 돌려준다. `ok:false`는
    /// 서비스 오류 문자열을 담은 `Err`이 된다.
    pub fn call(
        &mut self,
        command: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let request = serde_json::json!({ "id": id, "command": command, "args": args });
        let line = request.to_string();
        self.writer
            .write_all(line.as_bytes())
            .and_then(|_| self.writer.write_all(b"\n"))
            .and_then(|_| self.writer.flush())
            .map_err(|error| match error.kind() {
                // 서버가 이미 연결을 닫은 뒤의 쓰기 — 토큰 불일치 즉시 종료와
                // 같은 상황이다(첫 응답 읽기 전에 쓰기가 먼저 실패한다).
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe => {
                    connection_closed()
                }
                _ => format!("패널 요청 전송 실패: {error}"),
            })?;

        let mut raw = Vec::new();
        let read =
            crate::panel::read_capped_line(&mut self.reader, &mut raw, MAX_RESPONSE_BYTES)
                .map_err(|error| match error {
                    // 서버가 토큰 불일치로 즉시 닫을 때 아직 못 읽은 요청이 버퍼에
                    // 남아 있으면 우아한 FIN 대신 RST가 온다 — EOF와 같은 의미다.
                    crate::panel::LineError::Io(error) => match error.kind() {
                        std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::BrokenPipe => connection_closed(),
                        _ => format!("패널 응답 읽기 실패: {error}"),
                    },
                    crate::panel::LineError::TooLong => format!(
                        "패널 응답이 {MAX_RESPONSE_BYTES} 바이트 상한을 넘었습니다"
                    ),
                })?;
        if read == 0 {
            return Err(connection_closed());
        }
        #[derive(serde::Deserialize)]
        struct PanelResponse {
            ok: bool,
            #[serde(default)]
            result: serde_json::Value,
            #[serde(default)]
            error: String,
        }
        let response: PanelResponse = serde_json::from_slice(&raw)
            .map_err(|error| format!("패널 응답 파싱 실패: {error}"))?;
        if response.ok {
            Ok(response.result)
        } else if response.error.is_empty() {
            Err("패널 명령이 실패했습니다(오류 문구 없음)".to_string())
        } else {
            Err(response.error)
        }
    }

    /// 인자 없는 편의 래퍼.
    pub fn call0(&mut self, command: &str) -> Result<serde_json::Value, String> {
        self.call(command, serde_json::Value::Null)
    }
}

/// 토큰·포트 파일에서 (포트, 토큰)을 읽어 실행 중 호스트를 발견한다.
/// 파일이 비거나 포트가 숫자가 아니면 명확한 오류로 안내한다.
pub fn discover(token_file: &Path, port_file: &Path) -> Result<(u16, String), String> {
    let token = std::fs::read_to_string(token_file)
        .map_err(|error| {
            format!(
                "토큰 파일 {} 읽기 실패: {error} — 호스트가 떠 있는지 확인하세요",
                token_file.display()
            )
        })?
        .trim()
        .to_string();
    if token.is_empty() {
        return Err(format!(
            "토큰 파일 {}이(가) 비어 있습니다 — 호스트를 다시 띄우세요",
            token_file.display()
        ));
    }
    let port_raw = std::fs::read_to_string(port_file)
        .map_err(|error| {
            format!(
                "포트 파일 {} 읽기 실패: {error} — 호스트가 떠 있는지 확인하세요",
                port_file.display()
            )
        })?
        .trim()
        .to_string();
    let port: u16 = port_raw.parse().map_err(|_| {
        format!(
            "포트 파일 {}에 유효한 포트가 없습니다: {port_raw:?}",
            port_file.display()
        )
    })?;
    Ok((port, token))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead as _;

    /// 프로토콜을 지키는 최소 가짜 패널: 첫 줄 토큰 확인 후 요청의 `command`를
    /// 그대로 돌려준다. 토큰이 다르면 즉시 닫는다(실제 서버와 같은 방어).
    fn spawn_fake_panel(token: &str) -> u16 {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let expected = token.to_string();
        std::thread::Builder::new()
            .name("fake-panel".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else { continue };
                    let expected = expected.clone();
                    std::thread::Builder::new()
                        .name("fake-panel-conn".into())
                        .spawn(move || {
                            let mut writer = stream.try_clone().unwrap();
                            let mut reader = BufReader::new(stream);
                            let mut line = String::new();
                            if reader.read_line(&mut line).unwrap() == 0 {
                                return;
                            }
                            if line.trim() != expected {
                                return; // 실제 서버처럼 즉시 종료
                            }
                            let mut line = String::new();
                            while reader.read_line(&mut line).unwrap() > 0 {
                                let request: serde_json::Value =
                                    serde_json::from_str(line.trim()).unwrap();
                                let command = request["command"].as_str().unwrap().to_string();
                                let id = request["id"].clone();
                                let response = serde_json::json!({
                                    "id": id,
                                    "ok": command != "boom",
                                    "result": command,
                                    "error": if command == "boom" { "서비스 오류" } else { "" },
                                });
                                writeln!(writer, "{response}").unwrap();
                                line.clear();
                            }
                        })
                        .unwrap();
                }
            })
            .unwrap();
        port
    }

    #[test]
    fn call_returns_result_and_surfaces_service_errors() {
        let port = spawn_fake_panel("tok");
        let mut client = PanelClient::connect(port, "tok").unwrap();
        assert_eq!(
            client.call0("get_status").unwrap(),
            serde_json::json!("get_status")
        );
        // ok:false는 연결을 죽이지 않는다 — 다음 명령이 계속된다.
        assert_eq!(client.call0("boom").unwrap_err(), "서비스 오류");
        assert_eq!(
            client.call("echo", serde_json::json!({"x": 1})).unwrap(),
            serde_json::json!("echo")
        );
    }

    #[test]
    fn token_mismatch_reports_closed_connection() {
        let port = spawn_fake_panel("tok");
        let mut client = PanelClient::connect(port, "wrong").unwrap();
        let error = client.call0("get_status").unwrap_err();
        assert!(error.contains("연결을 닫았습니다"), "{error}");
    }

    #[test]
    fn connect_failure_mentions_host_check() {
        let error = PanelClient::connect(1, "tok").unwrap_err();
        assert!(error.contains("호스트"), "{error}");
    }

    /// 고유 임시 디렉터리(크레이트에 tempfile이 없어 std로 만든다).
    struct TempDir(std::path::PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "leftcar-panel-client-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn discover_reads_port_and_token() {
        let dir = TempDir::new("ok");
        std::fs::write(dir.0.join("panel-port"), "41234\n").unwrap();
        std::fs::write(dir.0.join("panel-token"), "  secret42 \n").unwrap();
        let (port, token) = discover(
            &dir.0.join("panel-token"),
            &dir.0.join("panel-port"),
        )
        .unwrap();
        assert_eq!(port, 41234);
        assert_eq!(token, "secret42");
    }

    #[test]
    fn discover_rejects_missing_and_malformed_files() {
        let dir = TempDir::new("bad");
        let error = discover(
            &dir.0.join("panel-token"),
            &dir.0.join("panel-port"),
        )
        .unwrap_err();
        assert!(error.contains("토큰 파일"), "{error}");

        std::fs::write(dir.0.join("panel-token"), "secret").unwrap();
        std::fs::write(dir.0.join("panel-port"), "not-a-port").unwrap();
        let error = discover(
            &dir.0.join("panel-token"),
            &dir.0.join("panel-port"),
        )
        .unwrap_err();
        assert!(error.contains("유효한 포트"), "{error}");
    }
}
