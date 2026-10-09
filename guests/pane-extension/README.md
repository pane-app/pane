# pane-extension

Write [Pane](https://github.com/pane-app/pane) extensions in Rust. An
extension is a WebAssembly component implementing Pane's `pane:extension`
contract; this crate is its bindings, the host functions (feedback, the
window, data, preferences, operations, system programs, web requests and
the rest), the extension UI builder and the print and log macros. It is
`no_std`, so the component imports only WASI 0.3 interfaces, and it supplies
the allocator and a panic handler.

You need only rustup's stable toolchain and the `wasm32-wasip2` target
(`rustup target add wasm32-wasip2`). Until the crate is published, depend on
it by `path`, at `guests/pane-extension` in a checkout of Pane's repository.

```toml
[package]
name = "hello-rust"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
pane-extension = "0.1"
```

```rust
#![no_std]

use pane_extension::alloc::{string::String, vec::Vec};
use pane_extension::feedback::{Toast, show_toast};
use pane_extension::{Command, CustomView, FieldValue, FormError, Item, List, NoCustomView};

struct Hello;
pane_extension::export!(Hello);

impl Command for Hello {
    type CustomView = NoCustomView;

    async fn render() -> Result<List, String> {
        Ok(List::new("Hello").item(Item::new("hello", "Say hello").on_action(|| async {
            show_toast(Toast::success("Hello from Rust"));
            Ok(())
        })))
    }

    async fn submit_form(_item_id: String, _values: Vec<FieldValue>) -> Result<String, FormError> {
        Err(FormError {
            field: None,
            message: "this command has no forms".into(),
        })
    }

    async fn open_view(_item_id: String) -> Result<CustomView, String> {
        Err("this command has no custom views".into())
    }
}
```

Build it with `cargo build --release --target wasm32-wasip2`, and name the
component in the package's `pane.json`:

```json
{
  "manifestVersion": 1,
  "title": "Hello Rust",
  "version": "0.1.0",
  "apiVersion": "0.1",
  "commands": [
    {
      "id": "hello",
      "title": "Hello Rust",
      "component": "target/wasm32-wasip2/release/hello_rust.wasm"
    }
  ]
}
```

## Versions

The crate's version follows the extension API it targets: 0.1.x builds
components for API 0.1 (`"apiVersion": "0.1"`, the WIT package
`pane:extension@0.1.0`), writing version 1 of the list tree. A new API
version is a new minor version of the crate before 1.0, and a new major
version from 1.0.

## More

The guide to writing commands, with an example of every interface, is
[`guests/README.md`](https://github.com/pane-app/pane/blob/main/guests/README.md)
in Pane's repository; the contract itself is the WIT in this crate's `wit/`
folder.

Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT),
at your option.
