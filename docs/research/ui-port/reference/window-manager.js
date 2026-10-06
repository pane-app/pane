class Component extends DCLogic {
  constructor(props) {
    super(props);
    this.state = { sel: 8 };
  }
  renderVals() {
    const platform = this.props.platform ?? 'macOS';
    const mac = platform === 'macOS';
    const k = {
      mod: mac ? '⌘' : 'Ctrl',
      alt: mac ? '⌥' : 'Alt',
      ctrl: mac ? '⌃' : (platform === 'Windows' ? 'Win' : 'Super')
    };
    const accent = this.props.accent ?? '#C9EE6A';
    const wp = String(this.props.wallpaper ?? 'Graphite').toLowerCase();
    const L = [
      ['Left Half', 0, 0, 50, 100, '←'],
      ['Right Half', 50, 0, 50, 100, '→'],
      ['Top Half', 0, 0, 100, 50, '↑'],
      ['Bottom Half', 0, 50, 100, 50, '↓'],
      ['Maximize', 0, 0, 100, 100, '↵'],
      ['Left Third', 0, 0, 33.333, 100, 'D'],
      ['Center Third', 33.333, 0, 33.334, 100, 'F'],
      ['Right Third', 66.667, 0, 33.333, 100, 'G'],
      ['Left Two-Thirds', 0, 0, 66.667, 100, 'E'],
      ['Center', 20, 12.5, 60, 75, 'C']
    ];
    const sel = this.state.sel;
    const layouts = L.map((l, i) => ({
      name: l[0], x: l[1], y: l[2], w: l[3], h: l[4],
      pressed: i === sel ? 'true' : 'false',
      bg: i === sel ? 'rgba(255,255,255,0.1)' : 'rgba(255,255,255,0.04)',
      ring: i === sel ? accent : 'rgba(255,255,255,0.05)',
      fill: i === sel ? accent : 'rgba(255,255,255,0.5)',
      tcol: i === sel ? '#ffffff' : '#a3a4a9',
      pick: () => { if (this.state.sel !== i) this.setState({ sel: i }); }
    }));
    const c = L[sel];
    const g = {
      name: c[0], x: c[1], y: c[2], w: c[3], h: c[4],
      size: Math.round(1440 * c[3] / 100) + ' × ' + Math.round(900 * c[4] / 100),
      keys: [k.ctrl, k.alt, c[5]]
    };
    const onKey = (e) => {
      const map = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -5, ArrowDown: 5 };
      if (map[e.key] !== undefined) {
        e.preventDefault();
        const n = Math.max(0, Math.min(L.length - 1, sel + map[e.key]));
        this.setState({ sel: n });
        try {
          const btns = e.currentTarget.querySelectorAll('button');
          if (btns[n]) btns[n].focus({ preventScroll: true });
        } catch (err) {}
      }
    };
    const P = '#c6b0ff', B = '#8fc3ff', O = '#ffc285', G = '#86deaf', N = '#3a3b41', M = '#2c2d32';
    const code = [
      { indent: 0, segs: [{ w: 44, c: P }, { w: 60, c: N }, { w: 30, c: P }, { w: 120, c: G }] },
      { indent: 0, segs: [{ w: 44, c: P }, { w: 90, c: N }, { w: 30, c: P }, { w: 110, c: G }] },
      { indent: 0, segs: [] },
      { indent: 0, segs: [{ w: 60, c: P }, { w: 70, c: P }, { w: 110, c: B }, { w: 60, c: N }] },
      { indent: 24, segs: [{ w: 40, c: P }, { w: 60, c: N }, { w: 90, c: B }, { w: 40, c: M }] },
      { indent: 24, segs: [{ w: 34, c: P }, { w: 40, c: N }, { w: 150, c: M }] },
      { indent: 48, segs: [{ w: 70, c: N }, { w: 30, c: O }] },
      { indent: 48, segs: [{ w: 60, c: N }, { w: 36, c: O }] },
      { indent: 48, segs: [{ w: 80, c: N }, { w: 80, c: G }] },
      { indent: 24, segs: [{ w: 20, c: N }] },
      { indent: 24, segs: [{ w: 50, c: P }, { w: 130, c: B }, { w: 70, c: M }] },
      { indent: 0, segs: [{ w: 14, c: N }] },
      { indent: 0, segs: [] },
      { indent: 0, segs: [{ w: 160, c: M }, { w: 90, c: M }] }
    ];
    return {
      k,
      accent,
      wp,
      isMac: mac,
      notMac: !mac,
      layouts,
      g,
      onKey,
      code
    };
  }
}
