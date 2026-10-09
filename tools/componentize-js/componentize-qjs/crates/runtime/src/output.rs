//! Standard output and standard error for JavaScript, which QuickJS leaves
//! out: a native `__paneWrite(stream, text)` that writes `text` to standard
//! output (`stream` 1) or standard error (2) through the WASI 0.3 libc. The
//! embedder's JavaScript builds `console` on it and removes the global.

use std::io::Write;

use rquickjs::{Ctx, Function};

fn write(stream: i32, text: String) {
    // Nothing is reported back: output that cannot be written is lost, as
    // with any program's.
    if stream == 2 {
        let mut err = std::io::stderr().lock();
        let _ = err.write_all(text.as_bytes());
        let _ = err.flush();
    } else {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(text.as_bytes());
        let _ = out.flush();
    }
}

/// Defines `__paneWrite` on the global object.
pub(crate) fn install(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.globals().set(
        "__paneWrite",
        Function::new(ctx.clone(), write)?.with_name("__paneWrite")?,
    )
}
