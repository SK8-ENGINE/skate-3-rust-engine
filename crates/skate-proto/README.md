# skate-proto

Generated Rust bindings for the authoritative session protocol, produced by
`build.rs` with [prost] from the **shared** `.proto` sources in
`server-go/proto/skatev1`.

Both the Rust engine and the Go `skated` server compile the same schema, so the
two implementations cannot drift.

## Layout

```
crates/skate-proto/
  build.rs              compiles ../..//server-go/proto/skatev1/*.proto
  src/lib.rs            exposes the generated `v1` module
  examples/session_smoke.rs   manual client against a running skated
  tests/two_clients.rs        automated two-client relay test
```

## Building

`build.rs` needs `protoc`. It honours the `PROTOC` environment variable, so on
Windows:

```powershell
$env:PROTOC="C:\path\to\protoc.exe"
cargo build -p skate-proto
```

## Usage

```rust
use prost::Message;
use skate_proto::{PROTOCOL_VERSION, v1};

let hello = v1::Envelope {
    protocol_version: PROTOCOL_VERSION,
    message: Some(v1::envelope::Message::Hello(v1::Hello { /* ... */ })),
};
let mut bytes = Vec::new();
hello.encode(&mut bytes)?;
```

The types mirror the proto package `skate.v1`: `Envelope` plus the control
(`Hello`, `Welcome`, `Reject`, `Ping`, `Pong`, `Roster`, `Goodbye`) and gameplay
(`Snapshot`, `BodySnapshot`, `PoseSnapshot`, `ApplicationRecord`) messages.

[prost]: https://github.com/tokio-rs/prost
