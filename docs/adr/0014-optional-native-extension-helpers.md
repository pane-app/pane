# Allow optional native helpers for WASI extensions

Accepted in Q33: after comparing Pi, Raycast and Zed, the user said "hmm ok then" to optional prebuilt native helpers invoked through our SDK/host API. Keep WASI 0.3 as the extension interface requirement while allowing platform-specific helpers for libraries and OS functionality unavailable in the guest; supported packages provide appropriate binaries without normal-user compiler setup.

The host owns managed execution, process input/output, cancellation and cleanup on disable/reload; arbitrary detached descendants or external side effects remain outside a universal cleanup guarantee. Native binaries add packaging and active-process costs, and bundled versus managed-download details remain open. This does not restore native Rust executables or Node as separate default extension entry points. See [precedent research](../research/native-helper-precedents-q33.md).
