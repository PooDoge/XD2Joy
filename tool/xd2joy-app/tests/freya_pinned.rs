//! Freya 0.5 is still in release candidates, and Cargo treats `^0.5.0-rc.8` as allowing any later
//! rc. A plain `cargo update` would quietly move the toolkit's inner crates to a newer rc than the
//! one this app was written against, so this fails until someone moves every one of them on purpose.

const PINNED: &str = "0.5.0-rc.8";
/// Crates published from the Freya repository that are not named `freya*`.
const ALSO_FREYA: [&str; 3] = ["torin", "ragnarok", "pathgraph"];

#[test]
fn every_freya_crate_is_on_the_pinned_rc() -> Result<(), Box<dyn std::error::Error>> {
    let lock = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../Cargo.lock"))?;
    let mut name = "";
    let mut wrong = Vec::new();
    let mut seen = 0;
    for line in lock.lines() {
        if let Some(n) = line.strip_prefix("name = ") {
            name = n.trim_matches('"');
        } else if let Some(v) = line.strip_prefix("version = ") {
            let version = v.trim_matches('"');
            if name.starts_with("freya") && !name.starts_with("freya-skia") || ALSO_FREYA.contains(&name) {
                seen += 1;
                if version != PINNED {
                    wrong.push(format!("{name} {version}"));
                }
            }
        }
    }
    assert!(seen > 5, "found only {seen} Freya crates in Cargo.lock; did the lock file move?");
    assert!(wrong.is_empty(), "these Freya crates are not {PINNED}: {wrong:?}");
    Ok(())
}
