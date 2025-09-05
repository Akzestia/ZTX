use std::net::SocketAddr;
use std::time::Instant;

use bytes::Bytes;
use futures::StreamExt;
use futures::executor::block_on;
use log::{debug, error, info};
use mio::Events;
use mio::event::Event;

use tquic::{
    Config, CongestionControlAlgorithm, Connection, Endpoint, MultipathAlgorithm, PacketInfo,
    TlsConfig, TransportHandler,
};

use crate::handlers::{RpcContext, RpcServer, snapshot_rpcs, snapshot_streams};
use crate::sockets::{QuicSocket, Result, StaticSockSender};

pub const MAX_BUF_SIZE: usize = 65_536;

#[derive(Clone, Debug)]
pub struct ServerSettings {
    pub cert_file: String,
    pub key_file: String,
    pub listen: SocketAddr,
    pub workers: usize,
    pub idle_timeout: u64,
    pub handshake_timeout: u64,
    pub max_concurrent_conns: u32,
    pub max_connection_window: u64,
    pub max_stream_window: u64,
    pub initial_max_data: u64,
    pub initial_max_stream_data_bidi_local: u64,
    pub initial_max_stream_data_bidi_remote: u64,
    pub initial_max_stream_data_uni: u64,
    pub initial_max_streams_bidi: u64,
    pub initial_max_streams_uni: u64,
    pub congestion_control: CongestionControlAlgorithm,
    pub initial_congestion_window: u64,
    pub min_congestion_window: u64,
    pub slow_start_thresh: u64,
    pub initial_rtt: u64,
    pub enable_pacing: bool,
    pub enable_multipath: bool,
    pub multipath_algorithm: MultipathAlgorithm,
    pub enable_retry: bool,
    pub enable_stateless_reset: bool,
    pub cid_len: usize,
    pub anti_amplification_factor: usize,
    pub send_batch_size: usize,
    pub zerortt_buffer_size: usize,
    pub max_undecryptable_packets: usize,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            cert_file: "./cert.crt".into(),
            key_file: "./cert.key".into(),
            listen: "0.0.0.0:4433".parse().unwrap(),
            workers: num_cpus::get(),
            idle_timeout: 30_000,
            handshake_timeout: 10_000,
            max_concurrent_conns: 100_000,
            max_connection_window: 16 * 1024 * 1024,
            max_stream_window: 8 * 1024 * 1024,
            initial_max_data: 10 * 1024 * 1024,
            initial_max_stream_data_bidi_local: 5 * 1024 * 1024,
            initial_max_stream_data_bidi_remote: 2 * 1024 * 1024,
            initial_max_stream_data_uni: 1 * 1024 * 1024,
            initial_max_streams_bidi: 200,
            initial_max_streams_uni: 100,
            congestion_control: CongestionControlAlgorithm::Bbr,
            initial_congestion_window: 10,
            min_congestion_window: 2,
            slow_start_thresh: u64::MAX,
            initial_rtt: 333,
            enable_pacing: true,
            enable_multipath: false,
            multipath_algorithm: MultipathAlgorithm::MinRtt,
            enable_retry: false,
            enable_stateless_reset: true,
            cid_len: 8,
            anti_amplification_factor: 3,
            send_batch_size: 64,
            zerortt_buffer_size: 1000,
            max_undecryptable_packets: 10,
        }
    }
}

impl ServerSettings {
    pub fn builder() -> ServerSettingsBuilder {
        ServerSettingsBuilder::new()
    }
}

#[derive(Clone, Debug)]
pub struct ServerSettingsBuilder {
    inner: ServerSettings,
}

impl ServerSettingsBuilder {
    pub fn new() -> Self {
        Self {
            inner: ServerSettings::default(),
        }
    }

    pub fn cert_file(mut self, path: impl Into<String>) -> Self {
        self.inner.cert_file = path.into();
        self
    }

    pub fn key_file(mut self, path: impl Into<String>) -> Self {
        self.inner.key_file = path.into();
        self
    }

    pub fn listen(mut self, addr: SocketAddr) -> Self {
        self.inner.listen = addr;
        self
    }

    pub fn workers(mut self, workers: usize) -> Self {
        self.inner.workers = workers;
        self
    }

    pub fn idle_timeout(mut self, timeout: u64) -> Self {
        self.inner.idle_timeout = timeout;
        self
    }

    pub fn handshake_timeout(mut self, timeout: u64) -> Self {
        self.inner.handshake_timeout = timeout;
        self
    }

    pub fn max_concurrent_conns(mut self, val: u32) -> Self {
        self.inner.max_concurrent_conns = val;
        self
    }

    pub fn max_connection_window(mut self, val: u64) -> Self {
        self.inner.max_connection_window = val;
        self
    }

    pub fn max_stream_window(mut self, val: u64) -> Self {
        self.inner.max_stream_window = val;
        self
    }

    pub fn initial_max_data(mut self, val: u64) -> Self {
        self.inner.initial_max_data = val;
        self
    }

    pub fn initial_max_stream_data_bidi_local(mut self, val: u64) -> Self {
        self.inner.initial_max_stream_data_bidi_local = val;
        self
    }

    pub fn initial_max_stream_data_bidi_remote(mut self, val: u64) -> Self {
        self.inner.initial_max_stream_data_bidi_remote = val;
        self
    }

    pub fn initial_max_stream_data_uni(mut self, val: u64) -> Self {
        self.inner.initial_max_stream_data_uni = val;
        self
    }

    pub fn initial_max_streams_bidi(mut self, val: u64) -> Self {
        self.inner.initial_max_streams_bidi = val;
        self
    }

    pub fn initial_max_streams_uni(mut self, val: u64) -> Self {
        self.inner.initial_max_streams_uni = val;
        self
    }

    pub fn congestion_control(mut self, algo: CongestionControlAlgorithm) -> Self {
        self.inner.congestion_control = algo;
        self
    }

    pub fn initial_congestion_window(mut self, val: u64) -> Self {
        self.inner.initial_congestion_window = val;
        self
    }

    pub fn min_congestion_window(mut self, val: u64) -> Self {
        self.inner.min_congestion_window = val;
        self
    }

    pub fn slow_start_thresh(mut self, val: u64) -> Self {
        self.inner.slow_start_thresh = val;
        self
    }

    pub fn initial_rtt(mut self, val: u64) -> Self {
        self.inner.initial_rtt = val;
        self
    }

    pub fn enable_pacing(mut self, enable: bool) -> Self {
        self.inner.enable_pacing = enable;
        self
    }

    pub fn enable_multipath(mut self, enable: bool) -> Self {
        self.inner.enable_multipath = enable;
        self
    }

    pub fn multipath_algorithm(mut self, algo: MultipathAlgorithm) -> Self {
        self.inner.multipath_algorithm = algo;
        self
    }

    pub fn enable_retry(mut self, enable: bool) -> Self {
        self.inner.enable_retry = enable;
        self
    }

    pub fn enable_stateless_reset(mut self, enable: bool) -> Self {
        self.inner.enable_stateless_reset = enable;
        self
    }

    pub fn cid_len(mut self, len: usize) -> Self {
        self.inner.cid_len = len;
        self
    }

    pub fn anti_amplification_factor(mut self, val: usize) -> Self {
        self.inner.anti_amplification_factor = val;
        self
    }

    pub fn send_batch_size(mut self, val: usize) -> Self {
        self.inner.send_batch_size = val;
        self
    }

    pub fn zerortt_buffer_size(mut self, val: usize) -> Self {
        self.inner.zerortt_buffer_size = val;
        self
    }

    pub fn max_undecryptable_packets(mut self, val: usize) -> Self {
        self.inner.max_undecryptable_packets = val;
        self
    }

    pub fn build(self) -> ServerSettings {
        self.inner
    }
}
struct ServerHandler {
    buf: Vec<u8>,
    rpc: RpcServer,
}

impl ServerHandler {
    pub fn new() -> Result<Self> {
        let mut rpc = RpcServer::new();
        for r in snapshot_rpcs() {
            rpc.sign(r);
        }
        for s in snapshot_streams() {
            rpc.sign_stream(s);
        }
        Ok(Self {
            buf: vec![0; MAX_BUF_SIZE],
            rpc,
        })
    }
}

impl TransportHandler for ServerHandler {
    fn on_conn_created(&mut self, conn: &mut Connection) {
        debug!("{} connection created", conn.trace_id());
    }

    fn on_conn_established(&mut self, conn: &mut Connection) {
        debug!("{} connection established", conn.trace_id());
    }

    fn on_conn_closed(&mut self, conn: &mut Connection) {
        debug!("{} connection closed", conn.trace_id());
    }

    fn on_stream_created(&mut self, conn: &mut Connection, stream_id: u64) {
        debug!("{} stream {} created", conn.trace_id(), stream_id);
    }

    fn on_stream_readable(&mut self, conn: &mut Connection, stream_id: u64) {
        while let Ok((n, fin)) = conn.stream_read(stream_id, &mut self.buf) {
            if !fin {
                continue;
            }

            let req = &self.buf[..n];
            let mut parts = req.splitn(2, |b| *b == b'\n');
            let name = parts.next().unwrap_or(&[]);
            let payload = parts.next().unwrap_or(&[]);
            let rpc_name = std::str::from_utf8(name).unwrap_or("");

            let ctx = RpcContext::default();

            // Unary RPC
            if let Some(fut) = self.rpc.handle(rpc_name, ctx.clone(), payload.to_vec()) {
                let resp = match block_on(fut) {
                    Ok(bytes) => {
                        let mut out = b"OK\n".to_vec();
                        out.extend_from_slice(&bytes);
                        out
                    }
                    Err(e) => {
                        let mut out = b"ERR\n".to_vec();
                        out.extend_from_slice(e.to_string().as_bytes());
                        out
                    }
                };
                let _ = conn.stream_write(stream_id, Bytes::from(resp), true);
                return;
            }

            // Streaming RPC
            if let Some(stream_fut) = self.rpc.handle_stream(rpc_name, ctx, payload.to_vec()) {
                let mut stream = block_on(stream_fut);
                while let Some(item) = block_on(stream.as_mut().next()) {
                    match item {
                        Ok(bytes) => {
                            let mut out = b"CHUNK\n".to_vec();
                            out.extend_from_slice(&bytes);
                            let _ = conn.stream_write(stream_id, Bytes::from(out), false);
                        }
                        Err(e) => {
                            let mut out = b"ERR\n".to_vec();
                            out.extend_from_slice(e.to_string().as_bytes());
                            let _ = conn.stream_write(stream_id, Bytes::from(out), false);
                        }
                    }
                }
                let _ = conn.stream_write(stream_id, Bytes::from_static(b"END\n"), true);
                return;
            }

            // Unknown RPC
            let mut out = b"ERR\nunknown rpc: ".to_vec();
            out.extend_from_slice(rpc_name.as_bytes());
            let _ = conn.stream_write(stream_id, Bytes::from(out), true);
        }
    }

    fn on_stream_writable(&mut self, conn: &mut Connection, stream_id: u64) {
        debug!("{} stream {} writable", conn.trace_id(), stream_id);
    }

    fn on_stream_closed(&mut self, conn: &mut Connection, stream_id: u64) {
        debug!("{} stream {} closed", conn.trace_id(), stream_id);
    }

    fn on_new_token(&mut self, conn: &mut Connection, token: Vec<u8>) {
        debug!("{} new token {:?}", conn.trace_id(), token);
    }
}

pub struct Worker {
    endpoint: Endpoint,
    poll: mio::Poll,
    sock: &'static QuicSocket,
    recv_buf: Vec<u8>,
}

impl Worker {
    pub fn new(settings: &ServerSettings) -> Result<Self> {
        let mut config = Config::new()?;
        config.set_max_idle_timeout(settings.idle_timeout);
        config.set_max_handshake_timeout(settings.handshake_timeout);
        config.set_max_concurrent_conns(settings.max_concurrent_conns);
        config.set_max_connection_window(settings.max_connection_window);
        config.set_max_stream_window(settings.max_stream_window);
        config.set_initial_max_data(settings.initial_max_data);
        config.set_initial_max_stream_data_bidi_local(settings.initial_max_stream_data_bidi_local);
        config
            .set_initial_max_stream_data_bidi_remote(settings.initial_max_stream_data_bidi_remote);
        config.set_initial_max_stream_data_uni(settings.initial_max_stream_data_uni);
        config.set_initial_max_streams_bidi(settings.initial_max_streams_bidi);
        config.set_initial_max_streams_uni(settings.initial_max_streams_uni);
        config.set_congestion_control_algorithm(settings.congestion_control);
        config.set_initial_congestion_window(settings.initial_congestion_window);
        config.set_min_congestion_window(settings.min_congestion_window);
        config.set_slow_start_thresh(settings.slow_start_thresh);
        config.set_initial_rtt(settings.initial_rtt);
        config.enable_pacing(settings.enable_pacing);
        config.enable_multipath(settings.enable_multipath);
        config.set_multipath_algorithm(settings.multipath_algorithm);
        config.enable_retry(settings.enable_retry);
        config.enable_stateless_reset(settings.enable_stateless_reset);
        config.set_cid_len(settings.cid_len);
        config.set_anti_amplification_factor(settings.anti_amplification_factor);
        config.set_send_batch_size(settings.send_batch_size);
        config.set_zerortt_buffer_size(settings.zerortt_buffer_size);
        config.set_max_undecryptable_packets(settings.max_undecryptable_packets);

        let alpn = vec![b"rpc".to_vec()];
        let tls_config =
            TlsConfig::new_server_config(&settings.cert_file, &settings.key_file, alpn, true)?;
        config.set_tls_config(tls_config);

        let poll = mio::Poll::new()?;
        let registry = poll.registry();
        let sock_owned = QuicSocket::new_reuseport(&settings.listen, registry, 1)?;
        // Leak socket to satisfy 'static lifetime required by Endpoint
        let sock_static: &'static QuicSocket = Box::leak(Box::new(sock_owned));
        let sender: std::rc::Rc<dyn tquic::PacketSendHandler> =
            std::rc::Rc::new(StaticSockSender { sock: sock_static });

        let handler = ServerHandler::new()?;
        let endpoint = Endpoint::new(Box::new(config), true, Box::new(handler), sender);

        Ok(Self {
            endpoint,
            poll,
            sock: sock_static,
            recv_buf: vec![0u8; MAX_BUF_SIZE],
        })
    }

    pub fn run(mut self) -> Result<()> {
        let mut events = Events::with_capacity(1024);
        loop {
            self.endpoint.process_connections()?;
            self.poll.poll(&mut events, self.endpoint.timeout())?;
            for ev in events.iter() {
                if ev.is_readable() {
                    self.process_read_event(ev)?;
                }
            }
            self.endpoint.on_timeout(Instant::now());
        }
    }

    fn process_read_event(&mut self, event: &Event) -> Result<()> {
        loop {
            let (len, local, remote) = match self.sock.recv_from(&mut self.recv_buf, event.token())
            {
                Ok(v) => v,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    error!("socket recv error: {:?}", e);
                    return Err(e.into());
                }
            };
            let pkt_buf = &mut self.recv_buf[..len];
            let pkt_info = PacketInfo {
                src: remote,
                dst: local,
                time: Instant::now(),
            };
            if let Err(e) = self.endpoint.recv(pkt_buf, &pkt_info) {
                error!("recv failed: {:?}", e);
            }
        }
        Ok(())
    }
}

pub fn run_server(settings: ServerSettings) -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    info!(
        "starting {} workers on {}",
        settings.workers, settings.listen
    );

    let mut handles = Vec::with_capacity(settings.workers);
    for i in 0..settings.workers {
        let s = settings.clone();
        let h = std::thread::Builder::new()
            .name(format!("quic-wkr-{}", i))
            .spawn(move || {
                if let Err(e) = Worker::new(&s).and_then(|w| w.run()) {
                    error!("worker {} exited: {:?}", i, e);
                }
            })?;
        handles.push(h);
    }
    for h in handles {
        let _ = h.join();
    }
    Ok(())
}
