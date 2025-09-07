use std::net::SocketAddr;
use std::rc::Rc;
use std::sync::{
    Arc,
    mpsc::{Receiver, Sender, channel},
};
use std::thread;
use std::time::{Duration, Instant};

use futures::Stream; // trait for return type
use log::debug;
use tokio_stream::wrappers::ReceiverStream;

use tquic::{Config, Connection, Endpoint, PacketInfo, TlsConfig, TransportHandler};

use crate::sockets::{QuicSocket, Result, StaticSockSender};

const MAX_BUF_SIZE: usize = 65_536;

pub struct QuicRpcClient {
    connect_to: SocketAddr,
    idle_timeout: u64, // milliseconds
    poll: mio::Poll,
    sock: Arc<QuicSocket>,
    sender: Rc<dyn tquic::PacketSendHandler>,
    recv_buf: Vec<u8>,
    // NEW: TLS + ALPN knobs
    tls_insecure: bool,
    alpn: Vec<Vec<u8>>,
    // NEW: per-RPC stream allocator (client-initiated bidi IDs: 0,4,8,...)
    next_stream_id: u64,
}

impl QuicRpcClient {
    pub fn builder(connect_to: SocketAddr) -> QuicRpcClientBuilder {
        QuicRpcClientBuilder::new(connect_to)
    }

    fn from_builder(b: QuicRpcClientBuilder) -> Result<Self> {
        let poll = mio::Poll::new()?;
        let registry = poll.registry();

        let sock_owned = Arc::new(QuicSocket::new_client_socket(
            b.connect_to.is_ipv4(),
            registry,
        )?);
        let sender: Rc<dyn tquic::PacketSendHandler> = Rc::new(StaticSockSender {
            sock: sock_owned.clone(),
        });

        Ok(Self {
            connect_to: b.connect_to,
            idle_timeout: b.idle_timeout,
            poll,
            sock: sock_owned,
            sender,
            recv_buf: vec![0u8; MAX_BUF_SIZE],
            tls_insecure: b.tls_insecure,
            alpn: b.alpn,
            next_stream_id: 0, // first client bidi stream
        })
    }

    /// Allocate a fresh client-initiated **bidirectional** stream ID.
    /// QUIC v1 layout lets the client pick 0,4,8,... for bidi streams.
    fn alloc_bidi_stream_id(&mut self) -> u64 {
        let sid = self.next_stream_id;
        self.next_stream_id = self.next_stream_id.saturating_add(4);
        sid
    }

    /// Unary RPC without payload
    pub fn call(&mut self, name: &str) -> Result<Vec<u8>> {
        self.call_with(name, &[])
    }

    /// Unary RPC with payload (accumulate until FIN) + per-RPC deadline + per-RPC **stream id**
    pub fn call_with(&mut self, name: &str, payload: &[u8]) -> Result<Vec<u8>> {
        let mut request = Vec::with_capacity(name.len() + 1 + payload.len());
        request.extend_from_slice(name.as_bytes());
        request.push(b'\n');
        request.extend_from_slice(payload);

        let (tx, rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = channel();
        let sid_to_use = self.alloc_bidi_stream_id(); // NEW

        struct UnaryHandler {
            req: Vec<u8>,
            buf: Vec<u8>,
            acc: Vec<u8>,
            tx: Sender<Vec<u8>>,
            stream_id: u64, // fixed per RPC
            done: bool,
        }

        impl TransportHandler for UnaryHandler {
            fn on_conn_created(&mut self, _conn: &mut Connection) {}

            fn on_conn_established(&mut self, conn: &mut Connection) {
                debug!("{} connection established", conn.trace_id());
                // Write the request on our dedicated stream; many QUIC stacks allow
                // implicit open on first write with a new stream id.
                let _ =
                    conn.stream_write(self.stream_id, std::mem::take(&mut self.req).into(), true);
            }

            fn on_conn_closed(&mut self, _conn: &mut Connection) {}

            fn on_stream_created(&mut self, _conn: &mut Connection, _stream_id: u64) {}

            fn on_stream_readable(&mut self, conn: &mut Connection, stream_id: u64) {
                if stream_id != self.stream_id {
                    return;
                }
                while let Ok((n, fin)) = conn.stream_read(stream_id, &mut self.buf) {
                    self.acc.extend_from_slice(&self.buf[..n]);
                    if fin && !self.done {
                        let _ = self.tx.send(std::mem::take(&mut self.acc));
                        self.done = true;
                    }
                }
            }

            fn on_stream_writable(&mut self, _conn: &mut Connection, _stream_id: u64) {}
            fn on_stream_closed(&mut self, _conn: &mut Connection, _stream_id: u64) {}
            fn on_new_token(&mut self, _conn: &mut Connection, _token: Vec<u8>) {}
        }

        let handler = UnaryHandler {
            req: request,
            buf: vec![0; MAX_BUF_SIZE],
            acc: Vec::new(),
            tx,
            stream_id: sid_to_use, // NEW
            done: false,
        };

        let mut config = Config::new()?;
        config.set_max_idle_timeout(self.idle_timeout);

        // NEW: TLS from builder (ALPN + insecure toggle)
        let tls_config = TlsConfig::new_client_config(self.alpn.clone(), self.tls_insecure)?;
        config.set_tls_config(tls_config);

        let mut endpoint = Endpoint::new(
            Box::new(config),
            false,
            Box::new(handler),
            self.sender.clone(),
        );

        endpoint.connect(
            self.sock.local_addr(),
            self.connect_to,
            None,
            None,
            None,
            None,
        )?;

        let mut events = mio::Events::with_capacity(1024);
        let deadline = Instant::now() + Duration::from_millis(self.idle_timeout + 2_000);

        loop {
            endpoint.process_connections()?;

            self.poll.poll(&mut events, endpoint.timeout())?;
            for event in events.iter() {
                if event.is_readable() {
                    loop {
                        let (len, local, remote) =
                            match self.sock.recv_from(&mut self.recv_buf, event.token()) {
                                Ok(v) => v,
                                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                                Err(e) => return Err(format!("recv error: {e:?}").into()),
                            };
                        let pkt_info = PacketInfo {
                            src: remote,
                            dst: local,
                            time: Instant::now(),
                        };
                        endpoint.recv(&mut self.recv_buf[..len], &pkt_info)?;
                    }
                }
            }

            endpoint.on_timeout(Instant::now());

            if let Ok(resp) = rx.try_recv() {
                return Ok(resp);
            }
            if Instant::now() >= deadline {
                return Err("rpc deadline exceeded".into());
            }
        }
    }

    /// Streaming RPC: returns a Stream of frames as they arrive.
    ///
    /// Spawns a dedicated OS thread. Channel ends when the thread exits (after FIN/error).
    pub async fn call_stream(
        &mut self,
        name: &str,
        payload: &[u8],
    ) -> Result<impl Stream<Item = Result<Vec<u8>>>> {
        use tokio::sync::mpsc;

        let mut request = Vec::with_capacity(name.len() + 1 + payload.len());
        request.extend_from_slice(name.as_bytes());
        request.push(b'\n');
        request.extend_from_slice(payload);

        let (tx, rx) = mpsc::channel::<Result<Vec<u8>>>(1024);

        let connect_to = self.connect_to;
        let idle_timeout = self.idle_timeout;
        let req = request;
        let is_ipv4 = connect_to.is_ipv4();

        // For streaming, also use a fresh stream id
        let stream_id = self.alloc_bidi_stream_id();
        let alpn = self.alpn.clone();
        let tls_insecure = self.tls_insecure;

        thread::spawn(move || {
            let mut config = match Config::new() {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.blocking_send(Err(format!("config error: {e:?}").into()));
                    return;
                }
            };
            config.set_max_idle_timeout(idle_timeout);

            // TLS: honor builder knobs
            let tls_config = match TlsConfig::new_client_config(alpn, tls_insecure) {
                Ok(t) => t,
                Err(e) => {
                    let _ = tx.blocking_send(Err(format!("tls config error: {e:?}").into()));
                    return;
                }
            };
            config.set_tls_config(tls_config);

            let mut poll = match mio::Poll::new() {
                Ok(p) => p,
                Err(e) => {
                    let _ = tx.blocking_send(Err(format!("poll new error: {e:?}").into()));
                    return;
                }
            };
            let registry = poll.registry();

            let sock_owned = match QuicSocket::new_client_socket(is_ipv4, registry) {
                Ok(s) => Arc::new(s),
                Err(e) => {
                    let _ = tx.blocking_send(Err(format!("socket create error: {e:?}").into()));
                    return;
                }
            };

            let sender: Rc<dyn tquic::PacketSendHandler> = Rc::new(StaticSockSender {
                sock: sock_owned.clone(),
            });

            struct ClientHandler {
                tx: Option<tokio::sync::mpsc::Sender<Result<Vec<u8>>>>, // drop on FIN
                buf: Vec<u8>,
                req: Vec<u8>,
                sid: u64,
            }

            impl TransportHandler for ClientHandler {
                fn on_conn_created(&mut self, _conn: &mut Connection) {}

                fn on_conn_established(&mut self, conn: &mut Connection) {
                    debug!("{} connection established", conn.trace_id());
                    let _ = conn.stream_write(self.sid, std::mem::take(&mut self.req).into(), true);
                }

                fn on_conn_closed(&mut self, _conn: &mut Connection) {}

                fn on_stream_created(&mut self, _conn: &mut Connection, _stream_id: u64) {}

                fn on_stream_readable(&mut self, conn: &mut Connection, stream_id: u64) {
                    if stream_id != self.sid {
                        return;
                    }
                    while let Ok((n, fin)) = conn.stream_read(stream_id, &mut self.buf) {
                        if let Some(ref tx) = self.tx {
                            let _ = tx.blocking_send(Ok(self.buf[..n].to_vec()));
                        }
                        if fin {
                            // Close stream to receiver
                            let _ = self.tx.take();
                            break;
                        }
                    }
                }

                fn on_stream_writable(&mut self, _conn: &mut Connection, _stream_id: u64) {}
                fn on_stream_closed(&mut self, _conn: &mut Connection, _stream_id: u64) {}
                fn on_new_token(&mut self, _conn: &mut Connection, _token: Vec<u8>) {}
            }

            let handler = ClientHandler {
                tx: Some(tx.clone()),
                buf: vec![0; MAX_BUF_SIZE],
                req,
                sid: stream_id,
            };

            let mut endpoint =
                Endpoint::new(Box::new(config), false, Box::new(handler), sender.clone());

            if let Err(e) =
                endpoint.connect(sock_owned.local_addr(), connect_to, None, None, None, None)
            {
                let _ = tx.blocking_send(Err(format!("connect error: {e:?}").into()));
                return;
            }

            let mut events = mio::Events::with_capacity(1024);
            let mut buf = vec![0u8; MAX_BUF_SIZE];

            // Drive I/O until the handler closes the channel (FIN or error)
            loop {
                if let Err(e) = endpoint.process_connections() {
                    let _ = tx.blocking_send(Err(format!("process error: {e:?}").into()));
                    break;
                }

                if let Err(e) = poll.poll(&mut events, endpoint.timeout()) {
                    let _ = tx.blocking_send(Err(format!("poll error: {e:?}").into()));
                    break;
                }

                for event in events.iter() {
                    if event.is_readable() {
                        loop {
                            let (len, local, remote) = match sock_owned
                                .recv_from(&mut buf, event.token())
                            {
                                Ok(v) => v,
                                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                                Err(e) => {
                                    let _ =
                                        tx.blocking_send(Err(format!("recv error: {e:?}").into()));
                                    return;
                                }
                            };
                            let pkt_info = PacketInfo {
                                src: remote,
                                dst: local,
                                time: Instant::now(),
                            };
                            if let Err(e) = endpoint.recv(&mut buf[..len], &pkt_info) {
                                let _ = tx
                                    .blocking_send(Err(
                                        format!("endpoint recv error: {e:?}").into()
                                    ));
                            }
                        }
                    }
                }

                endpoint.on_timeout(Instant::now());
            }
        });

        Ok(ReceiverStream::new(rx))
    }
}

pub struct QuicRpcClientBuilder {
    connect_to: SocketAddr,
    idle_timeout: u64,
    // NEW:
    tls_insecure: bool,
    alpn: Vec<Vec<u8>>,
}

impl QuicRpcClientBuilder {
    pub fn new(connect_to: SocketAddr) -> Self {
        Self {
            connect_to,
            idle_timeout: 5_000,
            tls_insecure: false,         // NEW: default secure
            alpn: vec![b"rpc".to_vec()], // NEW: default ALPN
        }
    }

    /// Timeout in **milliseconds**
    pub fn idle_timeout(mut self, millis: u64) -> Self {
        self.idle_timeout = millis;
        self
    }

    /// NEW: disable certificate/hostname verification (dev-only!)
    pub fn tls_insecure(mut self, insecure: bool) -> Self {
        self.tls_insecure = insecure;
        self
    }

    /// NEW: set ALPN protocols (e.g., vec![b"rpc".to_vec()])
    pub fn alpn(mut self, proto: impl AsRef<[u8]>) -> Self {
        self.alpn = vec![proto.as_ref().to_vec()];
        self
    }

    // Multiple ALPN protos: &["rpc", "h3"], &[b"rpc".as_slice(), b"h3".as_slice()], etc.
    pub fn alpn_many<I, S>(mut self, protos: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<[u8]>,
    {
        self.alpn = protos.into_iter().map(|p| p.as_ref().to_vec()).collect();
        self
    }

    pub fn build(self) -> Result<QuicRpcClient> {
        QuicRpcClient::from_builder(self)
    }
}
