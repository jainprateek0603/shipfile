//! `Document::parse` never panics: any input gives a document or a diagnostic.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(source) = std::str::from_utf8(data) {
        if let Ok(doc) = shipfile_core::Document::parse(source) {
            let _ = doc.to_json();
        }
    }
});
