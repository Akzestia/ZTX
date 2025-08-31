fn main() {
    let mut cfg = prost_build::Config::new();

    capnpc::CompilerCommand::new()
        .src_prefix("schema")
        .file("schema/calc.capnp")
        .run()
        .expect("schema compiler command");
}
