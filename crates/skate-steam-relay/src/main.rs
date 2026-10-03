//! Optional Steam byte relay. The game never links this SDK or calls Steam Input.
#[cfg(feature = "steam")]
mod directory;
#[cfg(feature = "steam")]
mod imp;

#[cfg(feature = "steam")]
fn main() {
    imp::main();
}

#[cfg(not(feature = "steam"))]
fn main() {
    eprintln!("skate-steam-relay was built without the steam feature");
    std::process::exit(1);
}
