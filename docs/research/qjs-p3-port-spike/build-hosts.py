"""Build the bounded P3 host and scratch componentizer without global env changes."""
from pathlib import Path
import os, json, subprocess

HERE=Path(__file__).resolve().parent
ROOT=Path(json.loads((HERE.parent/'p3-native-toolchain-local.json').read_text())['root'])
builds=[
    ('host',HERE.parent/'p3-only-host/Cargo.toml',ROOT/'host-target',[],HERE.parent/'p3-only-host/build.log'),
    ('componentizer',ROOT/'qjs-port/Cargo.toml',ROOT/'componentizer-target',
     ['-p','componentize-qjs','--example','p3_build'],HERE/'build-componentizer.log'),
]
for name,manifest,target,extra,logpath in builds:
    with logpath.open('w') as log:
        p=subprocess.run(['cargo','build','--manifest-path',str(manifest),*extra],
            env={**os.environ,'CARGO_TARGET_DIR':str(target),'CARGO_PROFILE_DEV_DEBUG':'0'},
            stdout=log,stderr=subprocess.STDOUT)
    print(name,p.returncode)
    if p.returncode:
        print(logpath.read_text()[-6000:])
        raise SystemExit(p.returncode)
