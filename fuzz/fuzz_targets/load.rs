//! The whole pipeline (parse, schema, semantic checks, overlay, resolve) never panics.
#![no_main]

use libfuzzer_sys::fuzz_target;
use shipfile_core::MemProject;

fuzz_target!(|data: &[u8]| {
    let Ok(source) = std::str::from_utf8(data) else { return };
    let project = MemProject::new().with_file("api/package.json", "{}").with_file("Dockerfile", "FROM scratch\n");
    for env in [None, Some("prod"), Some("dev")] {
        if let Ok(loaded) = shipfile_core::load(source, env, &project) {
            for name in loaded.model.services.keys() {
                let _ = loaded.model.contract(name);
            }
        }
    }
});
