//! A friendly crash report instead of a raw Rust backtrace.

use std::backtrace::Backtrace;
use std::io::Write;
use std::panic::PanicHookInfo;

const ISSUES: &str = "https://github.com/jainprateek0603/shipfile/issues/new";

pub(crate) fn install() {
    // Developers asking for a backtrace get the default hook.
    if std::env::var_os("RUST_BACKTRACE").is_some() {
        return;
    }
    std::panic::set_hook(Box::new(report));
}

fn report(info: &PanicHookInfo<'_>) {
    let message = message(info);
    let trace = Backtrace::force_capture();
    let file = std::env::temp_dir().join(format!("shipfile-crash-{}.txt", std::process::id()));
    let saved = std::fs::write(&file, format!("{}\n{message}\n\n{trace}", about())).is_ok();
    let mut err = std::io::stderr().lock();
    let _ = writeln!(err, "\nshipfile crashed. This is a bug in shipfile, not in your setup.\n");
    let _ = writeln!(err, "  {message}");
    let _ = writeln!(err, "  {}\n", about());
    if saved {
        let _ = writeln!(err, "Please report it at {ISSUES} and attach {}", file.display());
    } else {
        let _ = writeln!(err, "Please report it at {ISSUES} (re-run with RUST_BACKTRACE=1 for a backtrace)");
    }
}

fn about() -> String {
    format!("shipfile {} on {} {}", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH)
}

fn message(info: &PanicHookInfo<'_>) -> String {
    let payload = info
        .payload()
        .downcast_ref::<&str>()
        .map(ToString::to_string)
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_owned());
    match info.location() {
        Some(at) => format!("{payload} (at {}:{})", at.file(), at.line()),
        None => payload,
    }
}
