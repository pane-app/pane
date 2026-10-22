"""Bounded functional/import checks. Debug-host timing is not a performance benchmark."""
from pathlib import Path
import hashlib, json, subprocess, sys, time

HERE=Path(__file__).resolve().parent
ROOT=Path(json.loads((HERE.parent/'p3-native-toolchain-local.json').read_text())['root'])
TOOLS=json.loads((HERE.parent/'wasi03-js-local.json').read_text())['wasm_tools']
CLI=json.loads((HERE.parent/'wasm-spike-local.json').read_text())['exe']
HOST=ROOT/'host-target/debug/p3-only-host.exe'
host_only='--host-only' in sys.argv

def run(command):
    p=subprocess.run([str(s) for s in command],capture_output=True,text=True,timeout=120)
    r={'exit_code':p.returncode,'stdout':p.stdout,'stderr':p.stderr}
    assert p.returncode==0,r
    return r

def check(value):
    assert value['invalidDataRejected'] is True,value
    assert int(value['elapsedNs'])>=10_000_000,value
    assert [x['id'] for x in value['items']]==['calculator'],value

results=[]
if host_only:
    wasm=ROOT/'qjs-p3-capabilities.wasm'
    r=run([HOST,wasm,'queries','io',ROOT/'std-fixture','20'])
    values=[json.loads(s) for s in r['stdout'].splitlines()]
    assert len(values)==20
    for i,value in enumerate(values):
        check(value)
        assert value['invocations']==i+1,value
        assert value['fileText']=='p3 standard library fixture\n' and value['fileError'] is None,value
        assert value['completion']=={'tag':'ok'},value
        assert 0<=value['random']<1 and abs(value['dateMs']-time.time()*1000)<60_000,value
    assert values[-1]['dateMs']>values[0]['dateMs']
    results.append({'case':'20 calls, one instance, P3-only host',**r})
    # New process/instance starts with new transient state. This is not hot reload.
    r=run([HOST,wasm,'query','missing',ROOT/'std-fixture'])
    value=json.loads(r['stdout']);check(value)
    assert value['invocations']==1 and value['fileErrorPayload']=={'tag':'no-entry'},value
    results.append({'case':'missing file, fresh instance, P3-only host',**r})
    r=run([HOST,wasm,'query','io'])
    value=json.loads(r['stdout']);check(value)
    assert value['fileError']=='Error: no-preopens',value
    results.append({'case':'no filesystem grant, P3-only host',**r})
    filename='host-results.json'
else:
    artifacts=[]
    for name in ['search','capabilities']:
        wasm=ROOT/f'qjs-p3-{name}.wasm'
        run([TOOLS,'validate','--features','all',wasm])
        wit=subprocess.check_output([TOOLS,'component','wit',str(wasm)],text=True)
        (HERE/f'{name}-component.wit').write_text(wit)
        world=wit.split('world root {',1)[1].split('\n}',1)[0]
        imports=[line.strip() for line in world.splitlines() if line.strip().startswith('import ')]
        assert imports and all(line.endswith('@0.3.0;') for line in imports),imports
        assert 'export query: async func' in world
        artifacts.append({'name':name,'bytes':wasm.stat().st_size,'sha256':hashlib.sha256(wasm.read_bytes()).hexdigest(),'imports':imports})
    for query,ids in [('calclator',['calculator']),('', ['calculator','applications','quicklinks']),('zzzzzz',[])]:
        r=run([CLI,'run','-S','p3=y','--invoke',f'query({json.dumps(query)})',ROOT/'qjs-p3-search.wasm'])
        value=json.loads(json.loads(r['stdout']))
        assert [x['id'] for x in value['items']]==ids,value
        assert value['invalidDataRejected'] and int(value['elapsedNs'])>=10_000_000,value
        results.append({'case':query,**r})
    filename='validation-results.json'
report={'pass':True,'cases':results}
if not host_only: report['artifacts']=artifacts
(HERE/filename).write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({'pass':True,'cases':len(results),'report':str(HERE/filename)}))
