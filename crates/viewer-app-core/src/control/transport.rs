//! 제어 소켓 전송 주입 포트. TS 원본은 react-native-tcp-socket을 직접 쓰지만
//! Rust 코어는 트레잇 뒤로 숨겨 테스트·JSI 임베딩이 소켓을 갈아끼울 수 있게 한다.

use std::io::{self, Read};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

/// 제어 평면 연결 생성기. `timeout`은 소켓 연결(이름 풀이 포함)까지의 상한이다.
pub trait ControlTransport: Send + Sync {
    fn connect(&self, host: &str, port: u16, timeout: Duration)
        -> io::Result<Box<dyn ControlStream>>;
}

/// 연결된 제어 소켓. 읽기는 엔진의 읽기 스레드가, 쓰기는 요청 스레드가
/// [`ControlStream::writer`]로 분리해 간섭하지 않는다.
pub trait ControlStream: Send {
    /// 일부 바이트를 읽는다. 데이터가 올 때까지 막히며, 0은 상대 종료(EOF)다.
    /// 오류는 치명적이다(제한 시간 초과 등 일시 상태는 구현이 흡수한다).
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;
    /// 전체 바이트를 쓴다. 실패는 쓰기 시점에 동기로 보고된다 — FIFO 채널에는
    /// 응답 ID가 없으므로 엔진은 실패 즉시 보류 전체를 폐기한다.
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()>;
    /// 소켓의 숫자 피어 주소(MagicDNS 풀이 후) — 네이티브 미디어 허가에 쓴다.
    fn remote_address(&self) -> Option<String>;
    /// 소켓을 파괴한다(멱등). 이후 read는 0(EOF)을 돌려줘야 한다.
    fn destroy(&mut self);
    /// 쓰기 전용 핸들. 읽기 루프와 잠금을 공유하지 않게 분리한다.
    fn writer(&self) -> Box<dyn ControlWriter>;
}

/// [`ControlStream::writer`]가 돌려주는 쓰기 전용 핸들.
pub trait ControlWriter: Send {
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()>;
    /// 쓰기 쪽도 파괴한다(멱등).
    fn destroy(&mut self);
}

/// `std::net::TcpStream` 전송. 읽기는 200ms 단위로 시간 제한을 걸고 파괴 신호를
/// 확인해 즉시 끝난다(제한 시간 초과는 오류가 아니라 파괴 검사 기회로 흡수).
pub struct TcpTransport;

/// 읽기 루프가 파괴를 알아채는 주기.
const READ_POLL: Duration = Duration::from_millis(200);

struct TcpControlStream {
    socket: Option<TcpStream>,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
    remote: Option<String>,
}

impl TcpControlStream {
    fn is_shutdown(&self) -> bool {
        self.shutdown.load(std::sync::atomic::Ordering::Relaxed)
    }
}

struct TcpControlWriter {
    socket: Option<TcpStream>,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
}

impl ControlStream for TcpControlStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.is_shutdown() {
                return Ok(0);
            }
            let Some(socket) = self.socket.as_mut() else {
                return Ok(0);
            };
            match socket.read(buf) {
                Ok(n) => return Ok(n),
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                            | io::ErrorKind::Interrupted
                    ) =>
                {
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        let Some(socket) = self.socket.as_mut() else {
            return Err(io::Error::new(io::ErrorKind::NotConnected, "socket destroyed"));
        };
        io::Write::write_all(socket, buf)
    }

    fn remote_address(&self) -> Option<String> {
        self.remote.clone()
    }

    fn destroy(&mut self) {
        self.shutdown
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.socket = None;
    }

    fn writer(&self) -> Box<dyn ControlWriter> {
        Box::new(TcpControlWriter {
            socket: self.socket.as_ref().and_then(|s| s.try_clone().ok()),
            shutdown: self.shutdown.clone(),
        })
    }
}

impl ControlWriter for TcpControlWriter {
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        let Some(socket) = self.socket.as_mut() else {
            return Err(io::Error::new(io::ErrorKind::NotConnected, "socket destroyed"));
        };
        io::Write::write_all(socket, buf)
    }

    fn destroy(&mut self) {
        self.shutdown
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.socket = None;
    }
}

impl ControlTransport for TcpTransport {
    fn connect(&self, host: &str, port: u16, timeout: Duration) -> io::Result<Box<dyn ControlStream>> {
        let mut last_err: Option<io::Error> = None;
        for addr in (host, port).to_socket_addrs()? {
            match TcpStream::connect_timeout(&addr, timeout) {
                Ok(socket) => {
                    let _ = socket.set_nodelay(true);
                    let _ = socket.set_read_timeout(Some(READ_POLL));
                    let remote = socket.peer_addr().ok().map(|a| a.to_string());
                    return Ok(Box::new(TcpControlStream {
                        socket: Some(socket),
                        shutdown: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                        remote,
                    }));
                }
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no addresses")))
    }
}
