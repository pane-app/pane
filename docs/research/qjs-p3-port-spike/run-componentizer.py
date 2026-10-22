from pathlib import Path
import subprocess, os, json, time, sys

HERE=Path(__file__).resolve().parent
meta=json.loads((HERE.parent/'p3-native-toolchain-local.json').read_text())
root,sdk=Path(meta['root']),Path(meta['sdk'])
js=Path(json.loads((HERE.parent/'wasi03-js-local.json').read_text())['root'])
capabilities='--capabilities' in sys.argv
cmd=[root/'componentizer-target/debug/examples/p3_build.exe',
     HERE/'wit' if capabilities else HERE.parent/'wasi03-js-spike/wit',
     root/'capabilities.mjs' if capabilities else js/'search.mjs',
     root/'qjs-runtime-target/wasm32-wasip3/release/componentize_qjs_runtime.wasm',
     root/('qjs-p3-capabilities.wasm' if capabilities else 'qjs-p3-search.wasm')]
start=time.monotonic()
p=subprocess.run([str(x) for x in cmd],env={**os.environ,
    'QJS_P3_LIBC':str(sdk/'share/wasi-sysroot/lib/wasm32-wasip3/libc.so')},
    capture_output=True,text=True,timeout=120)
result={'exit_code':p.returncode,'seconds':time.monotonic()-start,'stdout':p.stdout,'stderr':p.stderr}
(HERE/('capabilities-componentize.json' if capabilities else 'componentize-custom-result.json')).write_text(json.dumps(result,indent=2))
print(json.dumps(result))
raise SystemExit(p.returncode)
