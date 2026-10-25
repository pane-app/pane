import re,sys,html,glob,os
sys.stdout.reconfigure(encoding='utf-8')
pat=re.compile(sys.argv[1],re.I)
seen=set()
files=sorted(glob.glob(sys.argv[2]),key=lambda p:[int(x) for x in re.findall(r'\d+',os.path.basename(p))])
for path in files:
    t=open(path,encoding='utf-8',errors='replace').read()
    t=re.sub(r'<script.*?</script>|<style.*?</style>','',t,flags=re.S)
    t=html.unescape(re.sub(r'<[^>]+>','\n',t))
    lines=[l.strip() for l in t.split('\n') if l.strip()]
    for i,l in enumerate(lines):
        if pat.search(l) and l not in seen and 'newsletter' not in l and 'product updates and news' not in l:
            seen.add(l)
            ctx=' | '.join(lines[max(0,i-1):i+2])
            print(f'[{os.path.basename(path)}] {ctx}')
