//! Compiles the shared session protocol definitions into Rust types.
//!
//! The `.proto` files are the single source of truth shared with the Go
//! `skated` server (in `server-go/proto/skatev1`). We reuse that directory
//! rather than duplicating schemas, so the two implementations cannot drift.
//!
//! `protoc` must be available. If it is not on PATH, set `PROTOC` to the
//! binary path, or point `PROTOC` at a protoc that lives outside PATH.

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The proto sources live beside the Go server at the repository root,
    // three directories up from this crate:
    // crates/skate-proto -> crates -> skate-3-rust-engine -> <repo>/server-go.
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let proto_dir = manifest
        .join("..")
        .join("..")
        .join("..")
        .join("server-go")
        .join("proto");

    if !proto_dir.is_dir() {
        panic!(
            "shared proto directory not found at {}; expected the Go server's \
             proto/skatev1 sources to be present in the repository",
            proto_dir.display()
        );
    }

    let protos = [
        proto_dir.join("skatev1/common.proto"),
        proto_dir.join("skatev1/control.proto"),
        proto_dir.join("skatev1/gameplay.proto"),
        proto_dir.join("skatev1/envelope.proto"),
    ];
    for proto in &protos {
        // Rebuild when any schema changes.
        println!("cargo:rerun-if-changed={}", proto.display());
    }
    println!("cargo:rerun-if-changed={}", proto_dir.display());

    let mut config = prost_build::Config::new();
    config.compile_protos(&protos, &[proto_dir])?;

    Ok(())
}
