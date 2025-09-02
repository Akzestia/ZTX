use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Mutex, OnceLock};

use futures::Stream;

use crate::sockets::Result;

pub type RpcResult<T> = Result<T>;

#[derive(Debug, Default, Clone)]
pub struct RpcContext {}

/// Unary RPC future
pub type RpcFut = Pin<Box<dyn Future<Output = RpcResult<Vec<u8>>> + Send + 'static>>;

pub struct RpcBox;
impl RpcBox {
    pub fn boxed<F>(f: F) -> RpcFut
    where
        F: Future<Output = RpcResult<Vec<u8>>> + Send + 'static,
    {
        Box::pin(f)
    }
}

/// Unary RPC trait
pub trait Rpc: Sync {
    fn name(&self) -> &'static str;
    fn call(&self, ctx: RpcContext, input: Vec<u8>) -> RpcFut;
}

/// Streaming RPC future
pub type RpcStreamFut =
    Pin<Box<dyn Future<Output = Pin<Box<dyn Stream<Item = RpcResult<Vec<u8>>> + Send>>> + Send>>;

pub struct RpcStreamBox;
impl RpcStreamBox {
    pub fn boxed<F, S>(f: F) -> RpcStreamFut
    where
        F: Future<Output = S> + Send + 'static,
        S: Stream<Item = RpcResult<Vec<u8>>> + Send + 'static,
    {
        Box::pin(async move {
            Box::pin(f.await) as Pin<Box<dyn Stream<Item = RpcResult<Vec<u8>>> + Send>>
        })
    }
}

/// Streaming RPC trait
pub trait RpcStream: Sync {
    fn name(&self) -> &'static str;
    fn call_stream(&self, ctx: RpcContext, input: Vec<u8>) -> RpcStreamFut;
}

/// Server registry
pub struct RpcServer {
    unary: HashMap<&'static str, &'static dyn Rpc>,
    streams: HashMap<&'static str, &'static dyn RpcStream>,
}

impl RpcServer {
    pub fn new() -> Self {
        Self {
            unary: HashMap::new(),
            streams: HashMap::new(),
        }
    }

    pub fn sign(&mut self, r: &'static dyn Rpc) -> &mut Self {
        self.unary.insert(r.name(), r);
        self
    }

    pub fn sign_stream(&mut self, r: &'static dyn RpcStream) -> &mut Self {
        self.streams.insert(r.name(), r);
        self
    }

    pub fn handle(&self, name: &str, ctx: RpcContext, input: Vec<u8>) -> Option<RpcFut> {
        self.unary.get(name).map(|r| r.call(ctx, input))
    }

    pub fn handle_stream(
        &self,
        name: &str,
        ctx: RpcContext,
        input: Vec<u8>,
    ) -> Option<RpcStreamFut> {
        self.streams.get(name).map(|r| r.call_stream(ctx, input))
    }
}

// ------------ Global registration ------------

static GLOBAL_RPCS: OnceLock<Mutex<Vec<&'static dyn Rpc>>> = OnceLock::new();
static GLOBAL_STREAMS: OnceLock<Mutex<Vec<&'static dyn RpcStream>>> = OnceLock::new();

fn global_vec() -> &'static Mutex<Vec<&'static dyn Rpc>> {
    GLOBAL_RPCS.get_or_init(|| Mutex::new(Vec::new()))
}

fn global_stream_vec() -> &'static Mutex<Vec<&'static dyn RpcStream>> {
    GLOBAL_STREAMS.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn register_rpc(r: &'static dyn Rpc) {
    let v = global_vec();
    let mut g = v.lock().unwrap();
    let r_ptr = r as *const dyn Rpc as *const ();
    if !g.iter().any(|&p| {
        let p_ptr = p as *const dyn Rpc as *const ();
        p_ptr == r_ptr
    }) {
        g.push(r);
    }
}

pub fn register_stream(r: &'static dyn RpcStream) {
    let v = global_stream_vec();
    let mut g = v.lock().unwrap();
    let r_ptr = r as *const dyn RpcStream as *const ();
    if !g.iter().any(|&p| {
        let p_ptr = p as *const dyn RpcStream as *const ();
        p_ptr == r_ptr
    }) {
        g.push(r);
    }
}
pub fn snapshot_rpcs() -> Vec<&'static dyn Rpc> {
    let g = global_vec().lock().unwrap();
    g.clone()
}

pub fn snapshot_streams() -> Vec<&'static dyn RpcStream> {
    let g = global_stream_vec().lock().unwrap();
    g.clone()
}
