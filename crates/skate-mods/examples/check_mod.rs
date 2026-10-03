fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: check_mod <package-folder-or-zip>");
    match skate_mods::validate_package(std::path::Path::new(&path)) {
        Ok(m) => {
            println!("OK {} api={} entry={}", m.id, m.api, m.entry);
        }
        Err(e) => {
            eprintln!("FAIL: {e}");
            std::process::exit(1);
        }
    }
}
