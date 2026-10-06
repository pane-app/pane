class Component extends DCLogic {
  renderVals() {
    const platform = this.props.platform ?? 'macOS';
    const mac = platform === 'macOS';
    const k = { mod: mac ? '⌘' : 'Ctrl' };
    const accent = this.props.accent ?? '#C9EE6A';
    const P = {
      clipboard: { label: 'Reads clipboard', icon: 'M9 3.5h6v3H9z M7 5H5v15.5h14V5h-2' },
      network: { label: 'Network access', icon: 'M3.5 12a8.5 8.5 0 1 0 17 0a8.5 8.5 0 1 0 -17 0 M3.5 12h17 M12 3.5c2.3 2.4 3.4 5.2 3.4 8.5s-1.1 6.1-3.4 8.5 M12 3.5C9.7 5.9 8.6 8.7 8.6 12s1.1 6.1 3.4 8.5' },
      keychain: { label: 'Secure storage', icon: 'M14.5 4a5.5 5.5 0 1 0 0 11a5.5 5.5 0 1 0 0-11z M10.6 13.4L4 20 M6.5 17.5l2 2' },
      screen: { label: 'Reads screen pixels', icon: 'M3.5 5h17v11h-17z M9 20h6 M12 16v4' },
      system: { label: 'Runs local commands', icon: 'M4 5.5h16v13H4z M8 10l2.5 2L8 14 M12.5 14.5H16' }
    };
    const raw = [
      { name: 'JSON Tools', author: '@mira', desc: 'Format, validate and query JSON straight from your clipboard.', size: '24 KB', tone: ['#402a1c', '#f5b88a'], icon: 'M9 4.5c-2 0-2.5 1-2.5 2.5v2.5c0 1.2-.8 2-2 2 1.2 0 2 .8 2 2V16c0 1.5.5 2.5 2.5 2.5 M15 4.5c2 0 2.5 1 2.5 2.5v2.5c0 1.2.8 2 2 2-1.2 0-2 .8-2 2V16c0 1.5-.5 2.5-2.5 2.5', perms: ['clipboard'], action: 'Open' },
      { name: 'Git Pulls', author: '@jonas', desc: 'Review, filter and check out pull requests from the keyboard.', size: '61 KB', tone: ['#173352', '#8fc3ff'], icon: 'M4 6a2 2 0 1 0 4 0a2 2 0 1 0 -4 0 M4 18a2 2 0 1 0 4 0a2 2 0 1 0 -4 0 M16 18a2 2 0 1 0 4 0a2 2 0 1 0 -4 0 M6 8v8 M18 16v-6a3 3 0 0 0-3-3h-3 M14 5l-2 2 2 2', perms: ['network', 'keychain'], action: 'Install', sel: true },
      { name: 'World Clock', author: '@aiko', desc: 'Compare cities at a glance — type “3pm Lagos in Oslo”.', size: '18 KB', tone: ['#163b3f', '#86d9e0'], icon: 'M3.5 12a8.5 8.5 0 1 0 17 0a8.5 8.5 0 1 0 -17 0 M12 7.5V12l3 2', perms: [], action: 'Install' },
      { name: 'Color Picker', author: '@femi', desc: 'Pick any pixel on screen and copy it as HEX, RGB or OKLCH.', size: '40 KB', tone: ['#45202d', '#ff9fbd'], icon: 'M16 3.5l4.5 4.5-2.5 2.5-4.5-4.5z M13.5 6L5 14.5V19h4.5L18 10.5', perms: ['screen', 'clipboard'], action: 'Update' },
      { name: 'Docker', author: '@rhea', desc: 'Start, stop and tail logs of your local containers.', size: '52 KB', tone: ['#2b3542', '#a9c6e8'], icon: 'M12 3.5l7.5 4.3v8.4L12 20.5l-7.5-4.3V7.8z M12 12l7.5-4.2 M12 12L4.5 7.8 M12 12v8.5', perms: ['system'], action: 'Install' },
      { name: 'Kill Process', author: '@tomasz', desc: 'Find what is eating your CPU and end it.', size: '12 KB', tone: ['#4a2e17', '#ffc285'], icon: 'M3 12h4l3-7 4 14 3-7h4', perms: ['system'], action: 'Open' },
      { name: 'Translate', author: '@lin', desc: 'Translate selected text with the provider you choose.', size: '29 KB', tone: ['#33254f', '#c6b0ff'], icon: 'M4 5.5h9 M8.5 4v1.5 M6 5.5c.5 3 2.5 5.5 5.5 7 M11 5.5c-.5 3-2.5 5.5-6 7.5 M12.5 20l4-9 4 9 M14 17h5', perms: ['network', 'clipboard'], action: 'Install' }
    ];
    const plugins = raw.map((p) => ({
      name: p.name,
      author: p.author,
      desc: p.desc,
      size: p.size,
      bg: p.tone[0],
      fg: p.tone[1],
      icon: p.icon,
      perms: p.perms.map((x) => P[x]),
      action: p.action,
      pillCls: p.action === 'Open' ? 'pill ghost' : 'pill',
      cls: p.sel ? 'prow sel' : 'prow'
    }));
    return { k, accent, plugins };
  }
}
