"""Copy the audited checkout and remove the now-obsolete preview1 adapter reset."""
from pathlib import Path
import json, shutil, subprocess, tempfile

HERE = Path(__file__).resolve().parent
meta = json.loads((HERE.parent/'p3-native-toolchain-local.json').read_text())
root = Path(meta['root'])
source = Path(tempfile.gettempdir())/'kyoko-qjs-p3-audit'
pin = 'e563c6d6ae50b087980414015663ca9c948c09bb'
assert subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip() == pin
dest = root/'qjs-port'
if not dest.exists():
    shutil.copytree(source,dest,ignore=shutil.ignore_patterns('.git'))
p=dest/'crates/runtime/src/lib.rs';s=p.read_text()
s=s.replace('        abi::reset_adapter_state();\n','')
p.write_text(s)
p=dest/'crates/runtime/src/abi.rs';s=p.read_text()
s=s.replace('''// WASI adapter state reset that is used during Wizer pre-initialization
#[link(wasm_import_module = "wasi_snapshot_preview1")]
unsafe extern "C" {
    #[link_name = "reset_adapter_state"]
    pub(crate) fn reset_adapter_state();
}

''','')
old = '''        #[link_name = "[context-get-0]"]
        pub(crate) fn context_get() -> u32;

        #[link_name = "[context-set-0]"]
        pub(crate) fn context_set(value: u32);

'''
if old in s:
    s=s.replace(old, '')
    anchor='mod async_builtins {\n'
    s=s.replace(anchor, anchor+'''    // SDK 34 libc owns canonical context slots for stack/TLS bookkeeping.
    // Its task hooks preserve this thread-local slot across async suspension.
    std::thread_local! {
        static QJS_TASK_CONTEXT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }

    pub(crate) unsafe fn context_get() -> u32 {
        QJS_TASK_CONTEXT.with(|slot| slot.get())
    }

    pub(crate) unsafe fn context_set(value: u32) {
        QJS_TASK_CONTEXT.with(|slot| slot.set(value));
    }

''', 1)
p.write_text(s)
# The libc preopen/descriptor reset stays intact. This is not a trap stub.
assert '__wasilibc_reset_preopens' in s
print(dest)
