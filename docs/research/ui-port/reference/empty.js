class Component extends DCLogic {
  constructor(props) {
    super(props);
    this.state = { query: 'kubectx' };
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
    return {
      k,
      accent: this.props.accent ?? '#C9EE6A',
      wp: String(this.props.wallpaper ?? 'Graphite').toLowerCase(),
      query: this.state.query,
      onQuery: (e) => this.setState({ query: e.target.value })
    };
  }
}
