pub use zetax_macro::{rpc, rstream};
use zetax_types::handlers::{RpcContext, RpcResult, register_rpc};
use zetax_types::server::{ServerSettings, run_server};
use zetax_types::sockets::Result;

#[rpc("echo")]
pub async fn echo(_ctx: RpcContext, input: Vec<u8>) -> RpcResult<Vec<u8>> {
    Ok(input)
}

#[rpc("hello")]
pub async fn hello(_ctx: RpcContext, input: Vec<u8>) -> RpcResult<Vec<u8>> {
    let s = String::from_utf8_lossy(&input);
    Ok(format!("hello: {s}").into_bytes())
}

fn main() -> Result<()> {
    register_rpc(&RPC_ECHO);
    register_rpc(&RPC_HELLO);

    let settings = ServerSettings::builder()
        .workers(num_cpus::get())
        .listen("0.0.0.0:4433".parse().unwrap())
        .cert_file("cert.crt")
        .key_file("cert.key")
        .request_client_auth(true)
        .alpn(b"h3")
        .build();

    run_server(settings)
}
