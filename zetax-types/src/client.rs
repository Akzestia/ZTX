use std::net::SocketAddr;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use futures::Stream;
use futures::channel::mpsc;
use log::debug;
use tquic::{Config, Connection, Endpoint, PacketInfo, TlsConfig, TransportHandler};

use crate::sockets::{QuicSocket, Result, StaticSockSender};

const MAX_BUF_SIZE: usize = 65_536;

pub struct QuicRpcClient {
    connect_to: SocketAddr,
    idle_timeout: u64,
    poll: mio::Poll,
    sock: &'static QuicSocket,
    sender: std::rc::Rc<dyn tquic::PacketSendHandler>,
    recv_buf: Vec<u8>,
}

impl QuicRpcClient {
    pub fn builder(connect_to: SocketAddr) -> QuicRpcClientBuilder {
        QuicRpcClientBuilder::new(connect_to)
    }

    fn from_builder(b: QuicRpcClientBuilder) -> Result<Self> {
        let poll = mio::Poll::new()?;
        let registry = poll.registry();
        let sock_owned = QuicSocket::new_client_socket(b.connect_to.is_ipv4(), registry)?;
        let sock_static: &'static QuicSocket = Box::leak(Box::new(sock_owned));
        let sender: std::rc::Rc<dyn tquic::PacketSendHandler> =
            std::rc::Rc::new(StaticSockSender { sock: sock_static });

        Ok(Self {
            connect_to: b.connect_to,
            idle_timeout: b.idle_timeout,
            poll,
            sock: sock_static,
            sender,
            recv_buf: vec![0u8; MAX_BUF_SIZE],
        })
    }

    /// Unary RPC without payload
    pub fn call(&mut self, name: &str) -> Result<Vec<u8>> {
        self.call_with(name, &[])
    }

    /// Unary RPC with payload
    pub fn call_with(&mut self, name: &str, payload: &[u8]) -> Result<Vec<u8>> {
        let mut request = Vec::new();
        request.extend_from_slice(name.as_bytes());
        request.push(b'\n');
        request.extend_from_slice(payload);

        let (tx, rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = channel();

        struct UnaryHandler {
            req: Vec<u8>,
            buf: Vec<u8>,
            tx: Sender<Vec<u8>>,
        }

        impl TransportHandler for UnaryHandler {
            fn on_conn_created(&mut self, _conn: &mut Connection) {}
            fn on_conn_established(&mut self, conn: &mut Connection) {
                debug!("{} connection established", conn.trace_id());
                let _ = conn.stream_write(0, self.req.clone().into(), true);
            }
            fn on_conn_closed(&mut self, _conn: &mut Connection) {}
            fn on_stream_created(&mut self, _conn: &mut Connection, _stream_id: u64) {}
            fn on_stream_readable(&mut self, conn: &mut Connection, stream_id: u64) {
                if let Ok((n, _fin)) = conn.stream_read(stream_id, &mut self.buf) {
                    let _ = self.tx.send(self.buf[..n].to_vec());
                }
            }
            fn on_stream_writable(&mut self, _conn: &mut Connection, _stream_id: u64) {}
            fn on_stream_closed(&mut self, _conn: &mut Connection, _stream_id: u64) {}
            fn on_new_token(&mut self, _conn: &mut Connection, _token: Vec<u8>) {}
        }

        let handler = UnaryHandler {
            req: request,
            buf: vec![0; MAX_BUF_SIZE],
            tx,
        };

        let mut config = Config::new()?;
        config.set_max_idle_timeout(self.idle_timeout);
        let tls_config = TlsConfig::new_client_config(vec![b"rpc".to_vec()], false)?;
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

            // ✅ Check if response arrived
            if let Ok(resp) = rx.try_recv() {
                return Ok(resp);
            }
        }
    }

    /// Streaming RPC: returns a Stream of frames as they arrive
    pub async fn call_stream(
        &mut self,
        name: &str,
        payload: &[u8],
    ) -> Result<impl Stream<Item = Result<Vec<u8>>>> {
        let mut request = Vec::new();
        request.extend_from_slice(name.as_bytes());
        request.push(b'\n');
        request.extend_from_slice(payload);

        let (tx, rx) = mpsc::unbounded::<Result<Vec<u8>>>();

        let connect_to = self.connect_to;
        let idle_timeout = self.idle_timeout;
        let req = request.clone();
        let is_ipv4 = connect_to.is_ipv4();

        tokio::spawn(async move {
            let mut config = match Config::new() {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.unbounded_send(Err(format!("config error: {e:?}").into()));
                    return;
                }
            };
            config.set_max_idle_timeout(idle_timeout);
            let tls_config = match TlsConfig::new_client_config(vec![b"rpc".to_vec()], false) {
                Ok(t) => t,
                Err(e) => {
                    let _ = tx.unbounded_send(Err(format!("tls config error: {e:?}").into()));
                    return;
                }
            };
            config.set_tls_config(tls_config);

            let mut poll = match mio::Poll::new() {
                Ok(p) => p,
                Err(e) => {
                    let _ = tx.unbounded_send(Err(format!("poll new error: {e:?}").into()));
                    return;
                }
            };
            let registry = poll.registry();

            let sock_owned = match QuicSocket::new_client_socket(is_ipv4, registry) {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.unbounded_send(Err(format!("socket create error: {e:?}").into()));
                    return;
                }
            };
            let sock_static: &'static QuicSocket = Box::leak(Box::new(sock_owned));
            let sender: std::rc::Rc<dyn tquic::PacketSendHandler> =
                std::rc::Rc::new(StaticSockSender { sock: sock_static });

            struct ClientHandler {
                tx: futures::channel::mpsc::UnboundedSender<Result<Vec<u8>>>,
                buf: Vec<u8>,
                req: Vec<u8>,
            }

            impl TransportHandler for ClientHandler {
                fn on_conn_created(&mut self, _conn: &mut Connection) {}
                fn on_conn_established(&mut self, conn: &mut Connection) {
                    debug!("{} connection established", conn.trace_id());
                    let _ = conn.stream_write(0, self.req.clone().into(), true);
                }
                fn on_conn_closed(&mut self, _conn: &mut Connection) {}
                fn on_stream_created(&mut self, _conn: &mut Connection, _stream_id: u64) {}
                fn on_stream_readable(&mut self, conn: &mut Connection, stream_id: u64) {
                    while let Ok((n, fin)) = conn.stream_read(stream_id, &mut self.buf) {
                        let frame = self.buf[..n].to_vec();
                        let _ = self.tx.unbounded_send(Ok(frame));
                        if fin {
                            let _ = self.tx.unbounded_send(Ok(b"END\n".to_vec()));
                        }
                    }
                }
                fn on_stream_writable(&mut self, _conn: &mut Connection, _stream_id: u64) {}
                fn on_stream_closed(&mut self, _conn: &mut Connection, _stream_id: u64) {}
                fn on_new_token(&mut self, _conn: &mut Connection, _token: Vec<u8>) {}
            }

            let handler = ClientHandler {
                tx: tx.clone(),
                buf: vec![0; MAX_BUF_SIZE],
                req,
            };

            let mut endpoint =
                Endpoint::new(Box::new(config), false, Box::new(handler), sender.clone());

            if let Err(e) =
                endpoint.connect(sock_static.local_addr(), connect_to, None, None, None, None)
            {
                let _ = tx.unbounded_send(Err(format!("connect error: {e:?}").into()));
                return;
            }

            let mut events = mio::Events::with_capacity(1024);
            let mut buf = vec![0u8; MAX_BUF_SIZE];

            loop {
                if let Err(e) = endpoint.process_connections() {
                    let _ = tx.unbounded_send(Err(format!("process error: {e:?}").into()));
                    break;
                }

                if let Err(e) = poll.poll(&mut events, endpoint.timeout()) {
                    let _ = tx.unbounded_send(Err(format!("poll error: {e:?}").into()));
                    break;
                }

                for event in events.iter() {
                    if event.is_readable() {
                        loop {
                            let (len, local, remote) = match sock_static
                                .recv_from(&mut buf, event.token())
                            {
                                Ok(v) => v,
                                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                                Err(e) => {
                                    let _ =
                                        tx.unbounded_send(Err(format!("recv error: {e:?}").into()));
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
                                    .unbounded_send(Err(
                                        format!("endpoint recv error: {e:?}").into()
                                    ));
                            }
                        }
                    }
                }

                endpoint.on_timeout(Instant::now());
            }
        });

        Ok(rx)
    }
}

pub struct QuicRpcClientBuilder {
    connect_to: SocketAddr,
    idle_timeout: u64,
}

impl QuicRpcClientBuilder {
    pub fn new(connect_to: SocketAddr) -> Self {
        Self {
            connect_to,
            idle_timeout: 5_000,
        }
    }

    pub fn idle_timeout(mut self, micros: u64) -> Self {
        self.idle_timeout = micros;
        self
    }

    pub fn build(self) -> Result<QuicRpcClient> {
        QuicRpcClient::from_builder(self)
    }
}
