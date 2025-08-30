# ZTX

ZTX - a simple, fast QUIC framework, built using [TQUIC](https://tquic.net/).

# Usage

# Setting up server

```rs
fn main() -> Result<()> {
    let mut settings = ServerSettings::default();
    settings.listen = "0.0.0.0:4433".parse().unwrap();
    // Optional: set max amount of workers
    settings.workers = num_cpus::get();

    run_server(settings)
}
```

# Register RPC (Server side)

```rs
// Define echo RPC
#[rpc("echo")]
pub async fn echo(_ctx: RpcContext, input: Vec<u8>) -> RpcResult<Vec<u8>> {
    Ok(input)
}

// Register RPC
fn main() -> Result<()> {
    // U must register ALL of your defined RPCs beforee run_server()
    register_rpc(&RPC_ECHO);

    // Server setup
    // ...
    run_server(settings)
}

```

# Setting up client

```rs
#[tokio::main]
async fn main() -> Result<()> {
    let server: SocketAddr = "127.0.0.1:4433".parse().unwrap();
    // Optional: log connection
    info!("connecting to {}", server);

    let mut client = QuicRpcClient::builder(server).idle_timeout(5_000).build()?;
}
```

# Calling RPC (Client side)

```rs
// RPC with payload
let resp = client.call_with("echo", b"hello world")?;
println!("echo => {}", String::from_utf8_lossy(&resp));
// RPC without payload
let _ = client.call("ping")?;
```

# Docs

More info available in [docs](docs) folder


# License [MIT](LICENSE)
