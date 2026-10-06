class Component extends DCLogic {
  renderVals() {
    const accent = this.props.accent ?? '#C9EE6A';
    return { accent, accentUpper: String(accent).toUpperCase() };
  }
}
