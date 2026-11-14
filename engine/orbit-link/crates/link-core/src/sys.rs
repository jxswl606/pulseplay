//! The few operating-system calls the engine needs beyond std: waiting on
//! several sockets at once, larger socket buffers, and (on Windows) keeping
//! ICMP errors from interrupting UDP sockets. Unix uses poll(2); Windows uses
//! WSAPoll, which takes the same kind of table.

#[cfg(unix)]
mod imp {
    pub use std::os::fd::AsRawFd as AsRaw;

    pub type PollFd = libc::pollfd;
    pub const READABLE: i16 = libc::POLLIN;
    pub const WRITABLE: i16 = libc::POLLOUT;

    pub fn entry(socket: &impl AsRaw, events: i16) -> PollFd {
        libc::pollfd { fd: socket.as_raw_fd(), events, revents: 0 }
    }

    pub fn poll(fds: &mut [PollFd], timeout_ms: i32) {
        unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout_ms) };
    }

    pub fn set_buffers(socket: &impl AsRaw, bytes: i32) {
        for option in [libc::SO_RCVBUF, libc::SO_SNDBUF] {
            unsafe {
                libc::setsockopt(
                    socket.as_raw_fd(),
                    libc::SOL_SOCKET,
                    option,
                    &bytes as *const i32 as *const libc::c_void,
                    std::mem::size_of::<i32>() as libc::socklen_t,
                );
            }
        }
    }

    pub fn quiet_udp(_socket: &std::net::UdpSocket) {}
}

#[cfg(windows)]
mod imp {
    pub use std::os::windows::io::AsRawSocket as AsRaw;
    use windows_sys::Win32::Networking::WinSock::{
        setsockopt, WSAIoctl, WSAPoll, POLLRDNORM, POLLWRNORM, SIO_UDP_CONNRESET, SOCKET, SOL_SOCKET, SO_RCVBUF,
        SO_SNDBUF, WSAPOLLFD,
    };

    pub type PollFd = WSAPOLLFD;
    // WSAPoll rejects some Unix flags; the "normal data" ones mean the same here.
    pub const READABLE: i16 = POLLRDNORM;
    pub const WRITABLE: i16 = POLLWRNORM;

    pub fn entry(socket: &impl AsRaw, events: i16) -> PollFd {
        WSAPOLLFD { fd: socket.as_raw_socket() as SOCKET, events, revents: 0 }
    }

    pub fn poll(fds: &mut [PollFd], timeout_ms: i32) {
        if fds.is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(timeout_ms.max(0) as u64));
            return;
        }
        unsafe { WSAPoll(fds.as_mut_ptr(), fds.len() as u32, timeout_ms) };
    }

    pub fn set_buffers(socket: &impl AsRaw, bytes: i32) {
        for option in [SO_RCVBUF, SO_SNDBUF] {
            unsafe {
                setsockopt(
                    socket.as_raw_socket() as SOCKET,
                    SOL_SOCKET,
                    option,
                    &bytes as *const i32 as *const u8,
                    std::mem::size_of::<i32>() as i32,
                );
            }
        }
    }

    /// Windows reports an ICMP "port unreachable" for an earlier send as an
    /// error on the next receive, which would cut a burst of reads short.
    pub fn quiet_udp(socket: &std::net::UdpSocket) {
        let off: u32 = 0;
        let mut returned: u32 = 0;
        unsafe {
            WSAIoctl(
                socket.as_raw_socket() as SOCKET,
                SIO_UDP_CONNRESET,
                &off as *const u32 as *const core::ffi::c_void,
                std::mem::size_of::<u32>() as u32,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
                None,
            );
        }
    }
}

pub use imp::*;
