class Component extends DCLogic {
  constructor(props) {
    super(props);
    this.state = { query: '', sel: 0, menu: false, menuSel: 0, menuQ: '', toast: null };
    this.kb = false;
    this.timer = null;
    this.qEl = null;
    this.listEl = null;
    this.menuQEl = null;
    this.setQ = (el) => { this.qEl = el; };
    this.setList = (el) => { this.listEl = el; };
    this.setMenuQ = (el) => { this.menuQEl = el; };
  }
  componentWillUnmount() {
    clearTimeout(this.timer);
  }
  componentDidUpdate(prevProps, prevState) {
    try {
      if (prevState.menu !== this.state.menu) {
        const t = this.state.menu ? this.menuQEl : this.qEl;
        if (t) t.focus({ preventScroll: true });
      }
      if (prevState.toast && !this.state.toast && this.qEl) this.qEl.focus({ preventScroll: true });
      if (this.kb && prevState.sel !== this.state.sel) {
        this.kb = false;
        const list = this.listEl;
        const el = list && list.querySelector('.row.sel');
        if (el) {
          const r = el.getBoundingClientRect();
          const lr = list.getBoundingClientRect();
          const scale = lr.height / (list.offsetHeight || 1);
          if (r.bottom > lr.bottom) list.scrollTop += (r.bottom - lr.bottom) / scale + 8;
          else if (r.top < lr.top) list.scrollTop -= (lr.top - r.top) / scale + 8;
        }
      }
    } catch (e) {}
  }
  renderVals() {
    const platform = this.props.platform ?? 'macOS';
    const mac = platform === 'macOS';
    const k = {
      mod: mac ? '⌘' : 'Ctrl',
      alt: mac ? '⌥' : 'Alt',
      shift: mac ? '⇧' : 'Shift',
      ctrl: mac ? '⌃' : (platform === 'Windows' ? 'Win' : 'Super')
    };
    const hotkey = mac ? '⌥ Space' : 'Alt Space';
    const accent = this.props.accent ?? '#C9EE6A';
    const wp = String(this.props.wallpaper ?? 'Graphite').toLowerCase();
    const I = {
      terminal: 'M4 5.5h16v13H4z M8 10l2.5 2L8 14 M12.5 14.5H16',
      prompt: 'M6 8l4 4-4 4 M12 16.5h6',
      code: 'M9 8l-4 4 4 4 M15 8l4 4-4 4 M13.2 5.5l-2.4 13',
      globe: 'M3.5 12a8.5 8.5 0 1 0 17 0a8.5 8.5 0 1 0 -17 0 M3.5 12h17 M12 3.5c2.3 2.4 3.4 5.2 3.4 8.5s-1.1 6.1-3.4 8.5 M12 3.5C9.7 5.9 8.6 8.7 8.6 12s1.1 6.1 3.4 8.5',
      notes: 'M6.5 3.5h8l3.5 3.5v13.5h-11.5z M14.5 3.5V7H18 M9.5 12h5.5 M9.5 15.5h4',
      music: 'M9 17.5V6.5l10-2v11 M4 17.5a2.5 2.5 0 1 0 5 0a2.5 2.5 0 1 0 -5 0 M14 15.5a2.5 2.5 0 1 0 5 0a2.5 2.5 0 1 0 -5 0',
      pen: 'M4.5 19.5l1-4.5L15.5 5l3.5 3.5-10 10z M13 7.5l3.5 3.5',
      chat: 'M4 5.5h16v11H10l-4.5 3.5v-3.5H4z',
      folder: 'M3.5 6.5h6l2 2h9v10h-17z',
      file: 'M5.5 3.5h8.5l4.5 4.5v12.5h-13z M14 3.5V8h4.5',
      cal: 'M4 6h16v14H4z M4 10h16 M8 3.5v4 M16 3.5v4',
      clipboard: 'M9 3.5h6v3H9z M7 5H5v15.5h14V5h-2 M9 12h6 M9 15.5h4',
      layout: 'M3.5 5.5h17v13h-17z M11 5.5v13',
      blocks: 'M4 4h7v7H4z M13 4h7v7h-7z M4 13h7v7H4z M16.5 13.5v6 M13.5 16.5h6',
      moon: 'M19.5 14.5A7.5 7.5 0 1 1 9.5 4.5a6 6 0 0 0 10 10z',
      lock: 'M6 10.5h12V20H6z M8.5 10.5V8a3.5 3.5 0 0 1 7 0v2.5',
      sliders: 'M4 7h9 M17 7h3 M15 5v4 M4 17h3 M11 17h9 M9 15v4',
      calc: 'M6 3.5h12v17H6z M9 7.5h6 M9 12h.01 M12 12h.01 M15 12h.01 M9 16h.01 M12 16h.01 M15 16h.01'
    };
    const A = {
      open: 'M7 17L17 7 M9 7h8v8',
      run: 'M8 5.5v13l10-6.5z',
      newwin: 'M4 6h16v13H4z M4 10h16 M12 13v4 M10 15h4',
      folder: 'M3.5 6.5h6l2 2h9v10h-17z',
      pin: 'M9 4h6l-1 6 3 3H7l3-3z M12 13v7',
      kb: 'M3.5 7h17v10h-17z M7 10.5h.01 M10.5 10.5h.01 M14 10.5h.01 M17 10.5h.01 M8 14h8',
      tag: 'M4 12V4.5h7.5L20 13l-7.5 7.5z M8 8h.01',
      quit: 'M12 3.5a8.5 8.5 0 1 0 0 17a8.5 8.5 0 1 0 0-17z M9 9l6 6 M15 9l-6 6',
      off: 'M12 3.5a8.5 8.5 0 1 0 0 17a8.5 8.5 0 1 0 0-17z M6 6l12 12'
    };
    const appTone = {
      term: ['linear-gradient(180deg, #4a4d55, #1c1e22)', '#c8f5b4'],
      code: ['linear-gradient(180deg, #45a3f5, #1d62c8)', '#ffffff'],
      web: ['linear-gradient(180deg, #ffa24d, #e2530f)', '#ffffff'],
      note: ['linear-gradient(180deg, #a184ff, #5b3bd0)', '#ffffff'],
      music: ['linear-gradient(180deg, #3ddc78, #129245)', '#ffffff'],
      pen: ['linear-gradient(180deg, #ff739f, #cf2d63)', '#ffffff'],
      chat: ['linear-gradient(180deg, #86418b, #4a1a4d)', '#ffffff'],
      folder: ['linear-gradient(180deg, #74b6ff, #2f78de)', '#ffffff'],
      cal: ['linear-gradient(180deg, #ff8070, #d6392a)', '#ffffff']
    };
    const app = (id, title, icon, t) => ({ id, title, sub: '', kind: 'Application', action: 'Open Application', go: 'Opening ' + title, icon: I[icon], bg: appTone[t][0], fg: appTone[t][1], tileCls: 'tile app', icCls: 'ic b', keys: [], alias: '' });
    const cmd = (id, title, sub, icon, keys, alias) => ({ id, title, sub, kind: 'Command', action: 'Run Command', go: 'Running ' + title, icon: I[icon], bg: 'rgba(255,255,255,0.08)', fg: '#e9e9ec', tileCls: 'tile', icCls: 'ic', keys: keys || [], alias: alias || '' });

    const pinnedRaw = [
      app('term', 'Terminal', 'prompt', 'term'),
      app('code', 'Visual Studio Code', 'code', 'code'),
      app('ff', 'Firefox', 'globe', 'web'),
      app('obs', 'Obsidian', 'notes', 'note'),
      app('spot', 'Spotify', 'music', 'music')
    ];
    const suggested = [
      app('figma', 'Figma', 'pen', 'pen'),
      cmd('clip', 'Clipboard History', 'Clipboard', 'clipboard', [k.mod, k.shift, 'V'], 'cb'),
      cmd('left', 'Left Half', 'Window Manager', 'layout', [k.ctrl, k.alt, '←']),
      cmd('files', 'Search Files', 'Files', 'file', [], 'f')
    ];
    const commands = [
      cmd('store', 'Plugin Store', 'Pane', 'blocks', [], 'store'),
      cmd('dark', 'Toggle Dark Mode', 'System', 'moon', []),
      cmd('lock', 'Lock Screen', 'System', 'lock', []),
      cmd('settings', 'Settings', 'Pane', 'sliders', [k.mod, ','])
    ];
    const extra = [
      app('slack', 'Slack', 'chat', 'chat'),
      app('filesapp', 'Files', 'folder', 'folder'),
      app('cal', 'Calendar', 'cal', 'cal'),
      cmd('calc', 'Calculator History', 'Calculator', 'calc', [], '='),
      cmd('snap', 'Snap Layouts', 'Window Manager', 'layout', [k.ctrl, k.alt, 'Space'])
    ];

    const launch = (it, text, keep) => {
      if (!it) return;
      clearTimeout(this.timer);
      this.setState({
        toast: {
          text: text || it.go,
          sub: keep ? 'Pane stays open so you can keep going' : 'Pane hides on ↵ · ' + hotkey + ' brings it back',
          hides: !keep,
          bg: it.bg, fg: it.fg, icon: it.icon, tileCls: it.tileCls, icCls: it.icCls
        },
        menu: false,
        menuQ: '',
        menuSel: 0
      });
      this.timer = setTimeout(() => {
        if (keep) this.setState({ toast: null });
        else this.setState({ toast: null, query: '', sel: 0 });
      }, 1800);
    };

    const pinned = pinnedRaw.map((p, i) => ({ ...p, n: String(i + 1), open: () => launch(p) }));

    const raw = this.state.query;
    const q = raw.trim().toLowerCase();
    let sections;
    if (!q) {
      sections = [
        { label: 'Suggested', note: 'From your recent use', items: suggested },
        { label: 'Commands', note: '', items: commands }
      ];
    } else {
      const all = [...pinnedRaw, ...suggested, ...commands, ...extra];
      const hits = all.filter((it) => it.title.toLowerCase().includes(q) || it.sub.toLowerCase().includes(q) || (it.alias && it.alias.startsWith(q)));
      hits.sort((a, b) => (b.title.toLowerCase().startsWith(q) ? 1 : 0) - (a.title.toLowerCase().startsWith(q) ? 1 : 0));
      if (hits.length) {
        sections = [{ label: 'Results', note: hits.length === 1 ? '1 match' : hits.length + ' matches', items: hits }];
      } else {
        const t = raw.trim();
        sections = [{ label: 'No matches', note: 'Try a fallback', items: [
          { ...cmd('web', 'Search the web for “' + t + '”', 'Default browser', 'globe'), kind: 'Fallback', action: 'Search Web', go: 'Searching the web for “' + t + '”' },
          { ...cmd('find', 'Find plugins for “' + t + '”', 'Plugin Store', 'blocks'), kind: 'Fallback', action: 'Open Store', go: 'Opening the Plugin Store' },
          { ...cmd('script', 'Create Script Command “' + t + '”', 'Scripts', 'terminal'), kind: 'Fallback', action: 'Create Command', go: 'Creating “' + t + '”' }
        ] }];
      }
    }

    const total = sections.reduce((n, s) => n + s.items.length, 0);
    const sel = Math.max(0, Math.min(this.state.sel, total - 1));
    let idx = 0;
    let selected = null;
    const out = sections.map((s) => ({
      label: s.label,
      note: s.note,
      items: s.items.map((it) => {
        const i = idx++;
        const at = q ? it.title.toLowerCase().indexOf(q) : -1;
        if (i === sel) selected = it;
        return {
          ...it,
          pre: at >= 0 ? it.title.slice(0, at) : it.title,
          hit: at >= 0 ? it.title.slice(at, at + q.length) : '',
          post: at >= 0 ? it.title.slice(at + q.length) : '',
          cls: i === sel ? 'row sel' : 'row',
          hasSub: !!it.sub,
          hasAlias: !!it.alias,
          hasKeys: it.keys.length > 0,
          pick: () => { if (sel === i) launch(it); else this.setState({ sel: i }); },
          hover: () => { if (this.state.sel !== i && !this.state.menu) this.setState({ sel: i }); }
        };
      })
    }));

    let actsRaw = [];
    if (selected) {
      const t = selected.title;
      if (selected.kind === 'Application') {
        actsRaw = [
          { label: 'Open Application', icon: A.open, keys: [], primary: true },
          { label: 'Open New Window', icon: A.newwin, keys: [k.mod, '↵'], done: 'Opening a new ' + t + ' window' },
          { label: 'Show in File Manager', icon: A.folder, keys: [k.mod, k.shift, 'F'], done: 'Showing ' + t + ' in File Manager' },
          { sep: true, title: 'Pane' },
          { label: 'Pin to Quick Slot', icon: A.pin, keys: [k.mod, 'P'], done: 'Pinned ' + t, keep: true },
          { label: 'Assign Hotkey…', icon: A.kb, keys: [k.mod, k.shift, 'H'], done: 'Press the new hotkey for ' + t, keep: true },
          { label: 'Add Alias…', icon: A.tag, keys: [k.mod, k.shift, 'A'], done: 'Type an alias for ' + t, keep: true },
          { sep: true, title: '' },
          { label: 'Quit ' + t, icon: A.quit, keys: [k.mod, 'Q'], danger: true, done: 'Quit ' + t, keep: true }
        ];
      } else {
        actsRaw = [
          { label: selected.action, icon: A.run, keys: [], primary: true },
          { sep: true, title: 'Pane' },
          { label: 'Pin to Quick Slot', icon: A.pin, keys: [k.mod, 'P'], done: 'Pinned ' + t, keep: true },
          { label: 'Assign Hotkey…', icon: A.kb, keys: [k.mod, k.shift, 'H'], done: 'Press the new hotkey for ' + t, keep: true },
          { label: 'Add Alias…', icon: A.tag, keys: [k.mod, k.shift, 'A'], done: 'Type an alias for ' + t, keep: true },
          { sep: true, title: '' },
          { label: 'Hide from Results', icon: A.off, keys: [k.mod, 'H'], done: 'Hid ' + t + ' from results', keep: true }
        ];
      }
    }
    const mq = this.state.menuQ.trim().toLowerCase();
    const acts0 = mq ? actsRaw.filter((a) => !a.sep && a.label.toLowerCase().includes(mq)) : actsRaw;
    const actList = acts0.filter((a) => !a.sep);
    const nAct = actList.length;
    const msel = Math.max(0, Math.min(this.state.menuSel, nAct - 1));
    const doAct = (a) => {
      if (!selected || !a) return;
      if (a.primary) launch(selected);
      else launch(selected, a.done, !!a.keep);
    };
    let ai = 0;
    const acts = acts0.map((a) => {
      if (a.sep) return { sep: true, row: false, hasTitle: !!a.title, title: a.title || '' };
      const j = ai++;
      return {
        sep: false,
        row: true,
        label: a.label,
        icon: a.icon,
        keys: a.keys,
        primary: !!a.primary,
        notPrimary: !a.primary,
        cls: 'arow' + (j === msel ? ' sel' : '') + (a.danger ? ' danger' : ''),
        run: () => doAct(a),
        hover: () => { if (this.state.menuSel !== j) this.setState({ menuSel: j }); }
      };
    });

    const toggleMenu = () => this.setState({ menu: !this.state.menu, menuSel: 0, menuQ: '' });
    const onQuery = (e) => this.setState({ query: e.target.value, sel: 0 });
    const onMenuQ = (e) => this.setState({ menuQ: e.target.value, menuSel: 0 });
    const onKey = (e) => {
      const mod = e.metaKey || e.ctrlKey;
      if (mod && (e.key === 'k' || e.key === 'K')) { e.preventDefault(); toggleMenu(); return; }
      if (mod && /^[1-5]$/.test(e.key)) { e.preventDefault(); launch(pinnedRaw[Number(e.key) - 1]); return; }
      if (this.state.menu) {
        if (e.key === 'ArrowDown') { e.preventDefault(); this.setState({ menuSel: Math.min(msel + 1, nAct - 1) }); }
        else if (e.key === 'ArrowUp') { e.preventDefault(); this.setState({ menuSel: Math.max(msel - 1, 0) }); }
        else if (e.key === 'Enter') { e.preventDefault(); doAct(actList[msel]); }
        else if (e.key === 'Escape') { e.preventDefault(); this.setState({ menu: false, menuQ: '' }); }
        return;
      }
      if (e.key === 'ArrowDown') { e.preventDefault(); this.kb = true; this.setState({ sel: Math.min(sel + 1, total - 1) }); }
      else if (e.key === 'ArrowUp') { e.preventDefault(); this.kb = true; this.setState({ sel: Math.max(sel - 1, 0) }); }
      else if (e.key === 'Enter') { e.preventDefault(); launch(selected); }
      else if (e.key === 'Escape') { this.setState({ query: '', sel: 0 }); }
    };

    const toast = this.state.toast;
    const hidden = !!(toast && toast.hides);
    const menu = this.state.menu;

    return {
      k,
      accent,
      wp,
      query: raw,
      onQuery,
      onKey,
      setQ: this.setQ,
      setList: this.setList,
      setMenuQ: this.setMenuQ,
      showPinned: !q,
      pinned,
      sections: out,
      primary: selected ? selected.action : 'Open',
      launchSel: () => launch(selected),
      menu,
      menuOff: !menu,
      menuPressed: menu ? 'true' : 'false',
      actBg: menu ? 'rgba(255,255,255,0.1)' : 'transparent',
      toggleMenu,
      selTitle: selected ? selected.title : '',
      selTile: selected ? { tileCls: selected.tileCls, bg: selected.bg, fg: selected.fg, icon: selected.icon } : { tileCls: 'tile', bg: 'transparent', fg: '#ffffff', icon: '' },
      acts,
      menuEmpty: nAct === 0,
      menuQ: this.state.menuQ,
      onMenuQ,
      hasToast: !!toast,
      toast: toast || { text: '', sub: '', bg: 'transparent', fg: '#ffffff', icon: '', tileCls: 'tile', icCls: 'ic' },
      panelOpacity: hidden ? 0 : 1,
      panelScale: hidden ? 0.98 : 1,
      panelPE: hidden ? 'none' : 'auto'
    };
  }
}
