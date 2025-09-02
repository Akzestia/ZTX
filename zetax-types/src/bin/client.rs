use std::net::SocketAddr;

use futures::StreamExt;
use log::info;
use zetax_types::client::QuicRpcClient;
use zetax_types::sockets::Result;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let server: SocketAddr = "127.0.0.1:4433".parse().unwrap();
    info!("connecting to {}", server);

    let mut client = QuicRpcClient::builder(server).idle_timeout(5_000).build()?;

    let resp = client.call_with("echo", b"hello world")?;
    println!("echo => {}", String::from_utf8_lossy(&resp));

    let resp2 = client.call_with("hello", b"from-binary")?;
    println!("hello => {}", String::from_utf8_lossy(&resp2));

    let _ = client.call("ping")?;

    Ok(())
}
