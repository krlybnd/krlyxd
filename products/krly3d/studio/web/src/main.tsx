import React from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import './styles.css';

class AppBoundary extends React.Component<{ children: React.ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() { return { failed: true }; }
  render() {
    if (this.state.failed) return <main className="fatal-error"><h1>A Studio nem tudott elindulni.</h1><p>Töltsd újra az oldalt a munkatér helyreállításához.</p><button className="button primary" onClick={() => window.location.reload()}>Újratöltés</button></main>;
    return this.props.children;
  }
}

createRoot(document.getElementById('root')!).render(<React.StrictMode><AppBoundary><App /></AppBoundary></React.StrictMode>);
