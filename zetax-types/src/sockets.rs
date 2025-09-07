use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use log::debug;
use mio::net::UdpSocket;
use mio::{Interest, Registry, Token};
use rustc_hash::FxHashMap;
use slab::Slab;
use socket2::{Domain, Protocol, Socket, Type};
use tquic::{PacketInfo, PacketSendHandler};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub struct QuicSocket {
    socks: Slab<UdpSocket>,
    addrs: FxHashMap<SocketAddr, usize>,
    local_addr: SocketAddr,
}

impl QuicSocket {
    pub fn new(local: &SocketAddr, registry: &Registry) -> Result<Self> {
        let mut socks = Slab::new();
        let mut addrs = FxHashMap::default();

        let socket = UdpSocket::bind(*local)?;
        let local_addr = socket.local_addr()?;
        let sid = socks.insert(socket);
        addrs.insert(local_addr, sid);

        let socket = socks.get_mut(sid).unwrap();
        registry.register(socket, Token(sid), Interest::READABLE)?;

        Ok(Self {
            socks,
            addrs,
            local_addr,
        })
    }

    pub fn new_client_socket(is_ipv4: bool, registry: &Registry) -> Result<Self> {
        let local = if is_ipv4 {
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        } else {
            IpAddr::V6(Ipv6Addr::UNSPECIFIED)
        };
        Self::new(&SocketAddr::new(local, 0), registry)
    }

    pub fn new_reuseport(local: &SocketAddr, registry: &Registry, n: usize) -> Result<Self> {
        if n == 0 {
            return Err("new_reuseport: n must be >= 1".into());
        }

        #[cfg(not(any(
            target_os = "linux",
            target_os = "android",
            target_os = "freebsd",
            target_os = "openbsd",
            target_os = "netbsd",
            target_os = "dragonfly",
            target_os = "macos",
            target_os = "ios"
        )))]
        {
            if n > 1 {
                return Err("SO_REUSEPORT not available on this platform; use n=1".into());
            }
        }

        let mut socks = Slab::new();
        let mut addrs = FxHashMap::default();

        let make_one = |local: &SocketAddr| -> std::io::Result<UdpSocket> {
            let domain = match local.ip() {
                std::net::IpAddr::V4(_) => Domain::IPV4,
                std::net::IpAddr::V6(_) => Domain::IPV6,
            };
            let sock = Socket::new(domain, Type::DGRAM.nonblocking(), Some(Protocol::UDP))?;
            sock.set_reuse_address(true)?;
            #[cfg(any(
                target_os = "linux",
                target_os = "android",
                target_os = "freebsd",
                target_os = "openbsd",
                target_os = "netbsd",
                target_os = "dragonfly",
                target_os = "macos",
                target_os = "ios"
            ))]
            sock.set_reuse_port(true)?;
            sock.bind(&(*local).into())?;

            let std_udp: std::net::UdpSocket = sock.into();
            std_udp.set_nonblocking(true)?;
            Ok(UdpSocket::from_std(std_udp))
        };

        for _ in 0..n {
            let socket = make_one(local)?;
            let local_addr = socket.local_addr()?;
            let sid = socks.insert(socket);
            addrs.insert(local_addr, sid);
        }

        for (sid, sock) in socks.iter_mut() {
            registry.register(sock, Token(sid), Interest::READABLE)?;
        }

        let local_addr = *addrs
            .keys()
            .next()
            .ok_or_else(|| "failed to create sockets".to_string())?;

        Ok(Self {
            socks,
            addrs,
            local_addr,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn recv_from(
        &self,
        buf: &mut [u8],
        token: mio::Token,
    ) -> std::io::Result<(usize, SocketAddr, SocketAddr)> {
        let socket = match self.socks.get(token.0) {
            Some(socket) => socket,
            None => return Err(std::io::Error::new(ErrorKind::Other, "invalid token")),
        };

        match socket.recv_from(buf) {
            Ok((len, remote)) => Ok((len, socket.local_addr()?, remote)),
            Err(e) => Err(e),
        }
    }

    pub fn send_to(&self, buf: &[u8], src: SocketAddr, dst: SocketAddr) -> std::io::Result<usize> {
        if let Some(&sid) = self.addrs.get(&src) {
            if let Some(socket) = self.socks.get(sid) {
                return socket.send_to(buf, dst);
            }
        }
        if let Some((_, socket)) = self.socks.iter().next() {
            return socket.send_to(buf, dst);
        }
        debug!("send_to drop packet with unknown address {:?}", src);
        Ok(buf.len())
    }
}

// A 'static sender wrapper that Endpoint can store as Rc<dyn PacketSendHandler + 'static>.
pub struct StaticSockSender {
    pub sock: std::sync::Arc<QuicSocket>,
}

impl PacketSendHandler for StaticSockSender {
    fn on_packets_send(&self, pkts: &[(Vec<u8>, PacketInfo)]) -> tquic::Result<usize> {
        let mut count = 0;
        for (pkt, info) in pkts {
            if let Err(e) = self.sock.send_to(pkt, info.src, info.dst) {
                if e.kind() == std::io::ErrorKind::WouldBlock {
                    debug!("socket send would block");
                    return Ok(count);
                }
                return Err(tquic::Error::InvalidOperation(format!(
                    "socket send_to(): {:?}",
                    e
                )));
            }
            debug!("written {} bytes", pkt.len());
            count += 1;
        }
        Ok(count)
    }
}
