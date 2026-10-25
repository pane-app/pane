import re,sys,html
sys.stdout.reconfigure(encoding='utf-8')
for path in sys.argv[2:]:
    t=open(path,encoding='utf-8',errors='replace').read()
    t=re.sub(r'<script.*?</script>|<style.*?</style>','',t,flags=re.S)
    t=html.unescape(re.sub(r'<[^>]+>','\n',t))
    lines=[l.strip() for l in t.split('\n') if l.strip()]
    pat=re.compile(sys.argv[1],re.I)
    cur=''
    for l in lines:
        if re.match(r'^v?\d+\.\d+(\.\d+)?$',l) or re.search(r'(January|February|March|April|May|June|July|August|September|October|November|December) \d+, 20\d\d',l):
            cur=l if not cur.endswith(l) else cur
            hdr=l
        if pat.search(l):
            print(f'[{path}] {hdr if "hdr" in dir() else ""} :: {l}')
