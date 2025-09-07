# ZTX

[![Crates.io](https://img.shields.io/crates/v/cql_lsp.svg)](https://crates.io/crates/ztx)
[![Crates.io](https://img.shields.io/crates/v/cql_lsp.svg)](https://crates.io/crates/ztx_types)
[![Crates.io](https://img.shields.io/crates/v/cql_lsp.svg)](https://crates.io/crates/ztx_macro)
[![Crates.io](https://img.shields.io/crates/v/cql_lsp.svg)](https://crates.io/crates/ztx_io)

ZTX - a simple, blazingly fast RoQ (RPC over QUIC) framework, built using [TQUIC](https://tquic.net/).

# Performance

ZTX can be as fast as **4x** the speed of gRPC, but you should note that ZTX contains
a wide range of specific optimizations, so creating a fair benchmark is a bit complicated

# Usage

> [!TIP]
> See [examples](https://github.com/Akzestia/ZTX-Examples) for basic ztx client/server implementation

# Setting up server

```rs
fn main() -> Result<()> {
    register_rpc(&RPC_ECHO);
    register_rpc(&RPC_HELLO);

    let settings = ServerSettings::builder()
        .workers(num_cpus::get())
        .listen("0.0.0.0:4433".parse().unwrap())
        .cert_file("cert.crt")
        .key_file("cert.key")
        .build();

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
    // You must register ALL of your defined RPCs before run_server()
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
