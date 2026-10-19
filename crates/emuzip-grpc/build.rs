fn main() {
    let fds = protox::compile(["emulator_controller.proto", "snapshot_service.proto", "emulator_stats.proto"], ["proto"]).expect("protos");
    tonic_build::configure().build_server(false).compile_fds(fds).expect("tonic-build");
    println!("cargo:rerun-if-changed=proto");
}
