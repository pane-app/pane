import re,sys,html
sys.stdout.reconfigure(encoding='utf-8')
pat=re.compile(sys.argv[1],re.I); n=int(sys.argv[2])
for path in sys.argv[3:]:
    t=open(path,encoding='utf-8',errors='replace').read()
    t=re.sub(r'<script.*?</script>|<style.*?</style>','',t,flags=re.S)
    t=html.unescape(re.sub(r'<[^>]+>','\n',t))
    lines=[l.strip() for l in t.split('\n') if l.strip()]
    hdr=''
    for i,l in enumerate(lines):
        if re.search(r'(January|February|March|April|May|June|July|August|September|October|November|December) \d+, 20\d\d',l): hdr=lines[i-1]+' '+l if i else l
        if pat.search(l):
            print(f'--- [{path}] {hdr}'); print(' | '.join(lines[max(0,i-2):i+n]))
