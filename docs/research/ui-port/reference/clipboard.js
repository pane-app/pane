class Component extends DCLogic {
  constructor(props) {
    super(props);
    this.state = { query: '', tab: 'All', selId: 'code', paused: false };
    this.kb = false;
  }
  componentDidUpdate(prevProps, prevState) {
    if (this.kb && prevState.selId !== this.state.selId) {
      this.kb = false;
      try {
        const el = document.querySelector('.row.sel');
        const list = el && el.closest('.list');
        if (el && list) {
          const r = el.getBoundingClientRect();
          const lr = list.getBoundingClientRect();
          const scale = lr.height / (list.offsetHeight || 1);
          if (r.bottom > lr.bottom) list.scrollTop += (r.bottom - lr.bottom) / scale + 8;
          else if (r.top < lr.top) list.scrollTop -= (lr.top - r.top) / scale + 8;
        }
      } catch (e) {}
    }
  }
  renderVals() {
    const platform = this.props.platform ?? 'macOS';
    const mac = platform === 'macOS';
    const k = { mod: mac ? '⌘' : 'Ctrl', shift: mac ? '⇧' : 'Shift' };
    const accent = this.props.accent ?? '#C9EE6A';
    const wp = String(this.props.wallpaper ?? 'Graphite').toLowerCase();
    const I = {
      terminal: 'M4 5.5h16v13H4z M8 10l2.5 2L8 14 M12.5 14.5H16',
      code: 'M9 8l-4 4 4 4 M15 8l4 4-4 4 M13.2 5.5l-2.4 13',
      link: 'M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1 M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1',
      image: 'M3.5 5h17v14h-17z M3.5 16l5-5 4 4 3-3 5 5 M15.5 8.5h.01',
      text: 'M5 6.5h14 M5 11h14 M5 15.5h9',
      mail: 'M3.5 6h17v12h-17z M4 6.5l8 6 8-6'
    };
    const tone = {
      term: ['#2a2c30', '#e6e7ea'],
      code: ['#173352', '#8fc3ff'],
      web: ['#4a2e17', '#ffc285'],
      chat: ['#163b3f', '#86d9e0'],
      folder: ['#2b3542', '#a9c6e8'],
      sys: ['rgba(255,255,255,0.08)', '#e9e9ec']
    };
    const code = 'const pane = createPane({\n  blur: 44,\n  tint: 0.7,\n  accent: "#C9EE6A",\n})';
    const chars = (s) => s.length + ' characters';
    const all = [
      { id: 'ssh', group: 'Pinned', type: 'Text', kind: 'text', title: 'ssh deploy@10.0.4.12', body: 'ssh deploy@10.0.4.12', icon: I.terminal, tone: 'term', app: 'Terminal', time: 'Mon', when: 'Monday, 09:12' },
      { id: 'code', group: 'Today', type: 'Text', kind: 'code', title: 'const pane = createPane({', body: code, icon: I.code, tone: 'code', app: 'Visual Studio Code', time: '14:02', when: 'Today, 14:02', size: '5 lines · ' + chars(code) },
      { id: 'lime', group: 'Today', type: 'Color', kind: 'color', title: '#C9EE6A', hex: '#C9EE6A', rgb: 'rgb(201 238 106)', hsl: 'hsl(77 79% 67%)', app: 'Color Picker', time: '13:48', when: 'Today, 13:48' },
      { id: 'link', group: 'Today', type: 'Link', kind: 'link', title: 'example.com/plugins/manifest', url: 'https://example.com/plugins/manifest', domain: 'example.com', icon: I.link, tone: 'web', app: 'Firefox', time: '13:31', when: 'Today, 13:31' },
      { id: 'shot', group: 'Today', type: 'Image', kind: 'image', title: 'Screenshot 2880 × 1800', icon: I.image, tone: 'folder', app: 'Screenshot', time: '12:10', when: 'Today, 12:10', dims: '2880 × 1800' },
      { id: 'standup', group: 'Today', type: 'Text', kind: 'text', title: 'Standup moved to 10:30 tomorrow', body: 'Standup moved to 10:30 tomorrow — same room, bring the launcher demo.', icon: I.text, tone: 'chat', app: 'Slack', time: '11:04', when: 'Today, 11:04' },
      { id: 'mail', group: 'Yesterday', type: 'Text', kind: 'text', title: 'hello@example.com', body: 'hello@example.com', icon: I.mail, tone: 'sys', app: 'Mail', time: '17:22', when: 'Yesterday, 17:22' },
      { id: 'ice', group: 'Yesterday', type: 'Color', kind: 'color', title: '#8FD3FF', hex: '#8FD3FF', rgb: 'rgb(143 211 255)', hsl: 'hsl(204 100% 78%)', app: 'Figma', time: '16:40', when: 'Yesterday, 16:40' }
    ];
    const copiedLine = (it) => {
      const w = it.when;
      const rel = /^(Today|Yesterday)/.test(w) ? w.charAt(0).toLowerCase() + w.slice(1) : 'on ' + w;
      return 'Copied ' + rel + ' from ' + it.app;
    };

    const tabNames = ['All', 'Text', 'Links', 'Images', 'Colors'];
    const tabType = { Text: 'Text', Links: 'Link', Images: 'Image', Colors: 'Color' };
    const tab = this.state.tab;
    const q = this.state.query.trim().toLowerCase();
    const visible = all.filter((it) => (tab === 'All' || it.type === tabType[tab]) && (!q || (it.title + ' ' + (it.body || '') + ' ' + it.app).toLowerCase().includes(q)));
    const ids = visible.map((it) => it.id);
    const selId = ids.includes(this.state.selId) ? this.state.selId : (ids[0] || null);
    const cur0 = visible.find((it) => it.id === selId) || null;

    const groups = ['Pinned', 'Today', 'Yesterday'];
    const sections = groups.map((g) => ({
      label: g,
      items: visible.filter((it) => it.group === g).map((it) => ({
        id: it.id,
        title: it.title,
        time: it.time,
        hex: it.hex || '',
        isColor: it.kind === 'color',
        notColor: it.kind !== 'color',
        icon: it.icon || '',
        bg: it.tone ? tone[it.tone][0] : '',
        fg: it.tone ? tone[it.tone][1] : '',
        cls: it.id === selId ? 'row sel' : 'row',
        pick: () => this.setState({ selId: it.id })
      }))
    })).filter((s) => s.items.length > 0);

    const tabs = tabNames.map((t) => ({ label: t, cls: t === tab ? 'tab on' : 'tab', pick: () => this.setState({ tab: t }) }));

    const onKey = (e) => {
      const i = ids.indexOf(selId);
      if (e.key === 'ArrowDown' && i < ids.length - 1) { e.preventDefault(); this.kb = true; this.setState({ selId: ids[i + 1] }); }
      else if (e.key === 'ArrowUp' && i > 0) { e.preventDefault(); this.kb = true; this.setState({ selId: ids[i - 1] }); }
      else if (e.key === 'Escape') { this.setState({ query: '' }); }
    };

    const paused = this.state.paused;
    const cur = cur0 || {};
    const kind = cur0 ? cur0.kind : '';

    return {
      k,
      accent,
      wp,
      query: this.state.query,
      placeholder: 'Search ' + all.length + ' clips…',
      onQuery: (e) => this.setState({ query: e.target.value }),
      onKey,
      tabs,
      sections,
      isEmpty: visible.length === 0,
      hasCur: !!cur0,
      cur,
      copied: cur0 ? copiedLine(cur0) : 'Nothing selected',
      is: { code: kind === 'code', text: kind === 'text', color: kind === 'color', link: kind === 'link', image: kind === 'image' },
      paused: paused ? 'true' : 'false',
      pauseLabel: paused ? 'Resume' : 'Pause',
      pauseIcon: paused ? 'M8 5.5v13l10-6.5z' : 'M9 5.5v13 M15 5.5v13',
      pauseColor: paused ? accent : '#d9dadd',
      togglePause: () => this.setState({ paused: !paused })
    };
  }
}
