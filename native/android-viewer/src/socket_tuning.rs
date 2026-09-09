//! Media socket tuning shared by renderer transports.

/// IP_TOS for media datagrams (DSCP AF41); every sender must mark alike or
/// switch/queue treatment differs per socket.
#[cfg(any(target_os = "android", test))]
pub(crate) const MEDIA_TOS: libc::c_int = 0x88;
/// IP_TOS for the control socket (DSCP EF).
#[cfg(any(target_os = "android", test))]
pub(crate) const CONTROL_TOS: libc::c_int = 0xb8;

#[cfg(any(target_os = "android", test))]
use std::io;
#[cfg(any(target_os = "android", test))]
use std::net::UdpSocket;
#[cfg(any(target_os = "android", test))]
use std::os::fd::AsRawFd;

#[cfg(any(target_os = "android", test))]
const SPLIT_MEDIA_RECEIVE_BUFFER_BYTES: libc::c_int = 4 * 1024 * 1024;

#[cfg(any(target_os = "android", test))]
pub(crate) fn split_media_receive_buffer_bytes() -> usize {
    SPLIT_MEDIA_RECEIVE_BUFFER_BYTES as usize
}

/// Split recovery sends two independently decodable IDRs. The more complex
/// tile can exceed Android's small default UDP queue before userspace gets a
/// chance to run FEC/reassembly, so reserve enough kernel space for one large
/// recovery boundary without adding any GPU work or userspace copies.
#[cfg(any(target_os = "android", test))]
pub(crate) fn configure_split_media_socket(socket: &UdpSocket) -> io::Result<usize> {
    let requested = SPLIT_MEDIA_RECEIVE_BUFFER_BYTES;
    let result = unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVBUF,
            &requested as *const _ as *const libc::c_void,
            std::mem::size_of_val(&requested) as libc::socklen_t,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }

    let media_tos: libc::c_int = MEDIA_TOS;
    unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::IPPROTO_IP,
            libc::IP_TOS,
            &media_tos as *const _ as *const libc::c_void,
            std::mem::size_of_val(&media_tos) as libc::socklen_t,
        );
    }

    let mut actual: libc::c_int = 0;
    let mut length = std::mem::size_of_val(&actual) as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVBUF,
            &mut actual as *mut _ as *mut libc::c_void,
            &mut length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(actual.max(0) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configures_and_reports_the_effective_receive_buffer() {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let actual = configure_split_media_socket(&socket).unwrap();
        assert!(actual > 0);
    }
}
