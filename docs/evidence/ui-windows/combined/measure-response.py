from pathlib import Path
from PIL import Image, ImageChops, ImageStat
import json
root=Path(__file__).resolve().parent
box=(630,120,730,300)
results={}
for theme in ('dark','light'):
    for mode in ('glass','control'):
        names=[f'glass-{theme}-{b}backdrop' if mode=='glass' else f'control-{theme}-{b}' for b in ('light','dark')]
        paths=[root/n/'00-initial-window.png' for n in names]
        imgs=[Image.open(p).convert('RGB') for p in paths]
        assert imgs[0].size == imgs[1].size
        value=sum(ImageStat.Stat(ImageChops.difference(imgs[0].crop(box),imgs[1].crop(box))).mean)/3
        results[f'{mode}-{theme}']={'mean_absolute_rgb_difference':round(value,6),'files':[p.relative_to(root).as_posix() for p in paths]}
record={'region_left_top_right_bottom':box,'meaning':'Background response establishes transparency only. Blur is established separately by visual review of softened known external edges.','results':results}
(root/'backdrop-response.json').write_text(json.dumps(record,indent=2)+'\n',encoding='utf-8')
print(json.dumps(record,indent=2))

