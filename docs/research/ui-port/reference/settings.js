class Component extends DCLogic {
  constructor(props) {
    super(props);
    this.state = { material: 'Glass', accent: null, blur: 44, tint: 70, density: 'Default', pin: true, tip: true };
  }
  renderVals() {
    const s = this.state;
    const accent = s.accent ?? this.props.accent ?? '#C9EE6A';
    const materials = ['Glass', 'Frost', 'Solid'].map((m) => ({
      label: m,
      cls: m === s.material ? 'seg on' : 'seg',
      pressed: m === s.material ? 'true' : 'false',
      pick: () => this.setState({ material: m })
    }));
    const notes = {
      Glass: 'Dark, blurred glass that picks up your wallpaper.',
      Frost: 'Lighter, brighter glass for pale wallpapers.',
      Solid: 'No transparency. Pane switches to this when your OS asks to reduce transparency.'
    };
    const sw = [
      { name: 'Lime', hex: '#C9EE6A' },
      { name: 'Ice', hex: '#8FD3FF' },
      { name: 'Amber', hex: '#FFC46B' },
      { name: 'Lilac', hex: '#C3B2FF' }
    ];
    const swatches = sw.map((x) => {
      const on = x.hex.toLowerCase() === String(accent).toLowerCase();
      return {
        name: x.name,
        hex: x.hex,
        pressed: on ? 'true' : 'false',
        ring: on ? '0 0 0 2px #1a1b1e, 0 0 0 4px ' + x.hex : 'inset 0 0 0 1px rgba(0,0,0,0.2)',
        pick: () => this.setState({ accent: x.hex })
      };
    });
    const densities = ['Compact', 'Default', 'Roomy'].map((d) => ({
      label: d,
      cls: d === s.density ? 'seg on' : 'seg',
      pressed: d === s.density ? 'true' : 'false',
      pick: () => this.setState({ density: d })
    }));
    const rowH = { Compact: 32, Default: 38, Roomy: 46 }[s.density];
    const solid = s.material === 'Solid';
    const frost = s.material === 'Frost';
    const alpha = solid ? 1 : (s.tint / 100) * (frost ? 0.78 : 1);
    const base = frost ? '74,76,84' : '22,23,26';
    const pv = {
      bg: 'linear-gradient(180deg, rgba(255,255,255,0.05), rgba(255,255,255,0) 36%), rgba(' + base + ',' + alpha.toFixed(2) + ')',
      bf: solid ? 'none' : 'blur(' + s.blur + 'px) saturate(160%)'
    };
    const rows = [
      { title: 'Figma', kind: 'Application', icon: 'M4.5 19.5l1-4.5L15.5 5l3.5 3.5-10 10z', tb: 'linear-gradient(180deg, #ff739f, #cf2d63)', tf: '#ffffff', cls: 'tile app' },
      { title: 'Configure Pane', kind: 'Command', icon: 'M4 7h9 M17 7h3 M15 5v4 M4 17h3 M11 17h9 M9 15v4', tb: 'rgba(255,255,255,0.08)', tf: '#e9e9ec', cls: 'tile' },
      { title: 'figma-tokens.json', kind: 'File', icon: 'M5.5 3.5h8.5l4.5 4.5v12.5h-13z', tb: '#2b3542', tf: '#a9c6e8', cls: 'tile' },
      { title: 'Recent Figma Files', kind: 'Command', icon: 'M5.5 3.5h8.5l4.5 4.5v12.5h-13z', tb: 'rgba(255,255,255,0.08)', tf: '#e9e9ec', cls: 'tile' }
    ];
    const pvRows = rows.map((r, i) => ({ ...r, bg: i === 0 ? 'rgba(255,255,255,0.09)' : 'transparent' }));
    return {
      accent,
      materials,
      materialNote: notes[s.material],
      swatches,
      blur: s.blur,
      tint: s.tint,
      onBlur: (e) => this.setState({ blur: Number(e.target.value) }),
      onTint: (e) => this.setState({ tint: Number(e.target.value) }),
      isSolid: solid,
      glassOpacity: solid ? 0.4 : 1,
      densities,
      rowH,
      pv,
      pvRows,
      pinOn: s.pin ? 'true' : 'false',
      pinOnBool: s.pin,
      pinTrack: s.pin ? accent : 'rgba(255,255,255,0.16)',
      pinKnob: s.pin ? 19 : 3,
      togglePin: () => this.setState({ pin: !s.pin }),
      tipOn: s.tip ? 'true' : 'false',
      tipTrack: s.tip ? accent : 'rgba(255,255,255,0.16)',
      tipKnob: s.tip ? 19 : 3,
      toggleTip: () => this.setState({ tip: !s.tip }),
      tipText: s.tip ? 'Tab searches inside a plugin' : ''
    };
  }
}
