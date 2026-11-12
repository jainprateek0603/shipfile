//! The pipeline never panics, whatever the input. A stable-toolchain complement to the
//! `cargo-fuzz` targets in `fuzz/`, so every CI run covers it.

use proptest::prelude::*;
use shipfile_core::MemProject;

const ORDERS: &str = include_str!("../../../examples/orders/shipfile.yaml");

fn project() -> MemProject {
    MemProject::new()
        .with_file("api/package.json", "{}")
        .with_file("worker/pyproject.toml", "[project]\n")
        .with_file("Dockerfile", "FROM scratch\n")
}

fn run(source: &str) {
    let project = project();
    for env in [None, Some("prod"), Some("nope")] {
        if let Ok(loaded) = shipfile_core::load(source, env, &project) {
            for name in loaded.model.services.keys() {
                let _ = loaded.model.contract(name);
            }
        }
    }
}

fn text(pattern: &str) -> impl Strategy<Value = String> {
    proptest::string::string_regex(pattern).expect("valid regex")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn arbitrary_text(source in text(r"\PC{0,200}")) {
        run(&source);
    }

    #[test]
    fn yaml_shaped_text(source in text(r#"([a-z_]{1,8}: ?(\[|\{|-|[a-z0-9./]{0,6}|"|')?\n {0,6}){0,20}"#)) {
        run(&source);
    }

    /// The example spec with one byte range removed or replaced: mostly broken in small ways.
    #[test]
    fn mutated_example(start in 0..ORDERS.len(), len in 0usize..40, insert in text(r#"[ :\-\[\]{}a-z0-9\n\t"']{0,6}"#)) {
        let mut start = start;
        while !ORDERS.is_char_boundary(start) { start -= 1; }
        let mut end = (start + len).min(ORDERS.len());
        while !ORDERS.is_char_boundary(end) { end -= 1; }
        let source = format!("{}{insert}{}", &ORDERS[..start], &ORDERS[end..]);
        run(&source);
    }
}

#[test]
fn example_loads() {
    let loaded = shipfile_core::load(ORDERS, Some("prod"), &project()).expect("example is valid");
    assert!(loaded.model.app.protect, "prod is protected by default");
}
