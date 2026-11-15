import { Code, ConnectError } from '@connectrpc/connect';
import { useEffect, useState } from 'react';
import { domIds } from '../../ssot/domIds';
import { cad, type CadInput, type CadState } from './client';

const STORAGE = 'krly3d-studio-cad-sessions';
const validSession = (id: unknown): id is string =>
  typeof id === 'string' && /^[0-9a-f]{8}-[0-9a-f-]{27}$/.test(id);

function remembered(): string[] {
  try {
    const found: unknown = JSON.parse(localStorage.getItem(STORAGE) ?? '[]');
    if (Array.isArray(found)) {
      const ids = [...new Set(found.filter(validSession))].slice(0, 8);
      if (ids.length) return ids;
    }
  } catch { /* Private or unavailable browser storage. */ }
  return [crypto.randomUUID()];
}

export function TypedCadPanel({ onState }: { onState: (state: CadState | null) => void }) {
  const [sessions, setSessions] = useState(remembered);
  const [sessionId, setSessionId] = useState(() => sessions[0]);
  const [current, setCurrent] = useState<CadState | null>(null);
  const [source, setSource] = useState('');
  const [working, setWorking] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    try { localStorage.setItem(STORAGE, JSON.stringify(sessions)); } catch { /* Session still works. */ }
  }, [sessions]);
  useEffect(() => {
    const controller = new AbortController();
    setCurrent(null); onState(null); setError('');
    void cad({ sessionId, action: 'show' }, controller.signal).then(state => {
      if (!controller.signal.aborted) { setCurrent(state); onState(state); }
    }).catch(cause => {
      if (controller.signal.aborted || (cause instanceof ConnectError && cause.code === Code.NotFound)) return;
      setError(cause instanceof Error ? cause.message : 'Az előzmény nem nyitható meg.');
    });
    return () => controller.abort();
  }, [sessionId, onState]);

  async function act(action: CadInput['action']) {
    if (working) return;
    setWorking(true); setError('');
    try {
      const state = await cad({
        sessionId, action,
        source: action === 'start' || action === 'review' ? source : '',
        modelId: current?.modelId ?? '', baseRevision: current?.revision ?? 0,
      });
      setCurrent(state); onState(state);
      if (action === 'start' || action === 'review') setSource('');
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'A CAD-művelet sikertelen.');
    } finally {
      setWorking(false);
    }
  }

  function newModel() {
    const id = crypto.randomUUID();
    setSessions(previous => [id, ...previous].slice(0, 8));
    setSessionId(id);
    setSource('');
  }

  return <div {...domIds.studio.typedCad.attrs} className="typed-cad-panel">
    <div className="typed-cad-session">
      <label>Munkamenet
        <select {...domIds.studio.typedCad.session.attrs} value={sessionId} onChange={event => setSessionId(event.target.value)} disabled={working}>
          {sessions.map(id => <option key={id} value={id}>{id === sessionId && current ? current.modelId : id.slice(0, 8)}</option>)}
        </select>
      </label>
      <button type="button" onClick={newModel} disabled={working}>Új modell</button>
    </div>
    <p className="typed-cad-explainer">Típusos matematikai szerződésből natív CAD-terv. A review a meglévő modell feltételeit módosítja; az elfogadott állapot helyben megmarad.</p>
    {current && <section {...domIds.studio.typedCad.state.attrs} className="typed-cad-state" aria-label="Elfogadott CAD-revízió">
      <div className="typed-cad-heading"><strong>{current.modelId}</strong><span>r{current.revision} · {current.acceptedEntries} elfogadott állapot</span></div>
      <p>{current.status === 'tested' ? 'A megadott feltételek mért próbái teljesültek.' : current.status} Teljes alakazonosság nincs bizonyítva.</p>
      {current.previewIssue && <p className="inline-error" role="status">Az elfogadott modell megmaradt, de az előnézet nem készült el: {current.previewIssue}</p>}
      <div className="typed-cad-actions"><button type="button" onClick={() => void act('undo')} disabled={working || current.revision === 0}>Visszavonás</button><button type="button" onClick={() => void act('redo')} disabled={working || current.activeEntry + 1 >= current.acceptedEntries}>Újra alkalmazás</button></div>
      {current.changes.length > 0 && <details {...domIds.studio.typedCad.state.changes.attrs} open><summary>Változások ({current.changes.length})</summary><ul>{current.changes.map(item => <li key={`${item.kind}:${item.id}`}><code>{item.id}</code> · {item.kind}</li>)}</ul></details>}
      <details><summary>Építési terv ({current.plan.length})</summary><ol>{current.plan.map(step => <li key={step.id}><code>{step.id}</code> · {step.operation}({step.inputs.join(', ')})</li>)}</ol></details>
      <details><summary>Ellenőrzések ({current.checks.length})</summary><ul>{current.checks.map(check => <li key={check.id}><code>{check.id}</code> · {check.status}{check.observed ? ` · ${check.observed}` : ''}</li>)}</ul></details>
    </section>}
    <form className="typed-cad-form" onSubmit={event => { event.preventDefault(); void act(current ? 'review' : 'start'); }}>
      <label htmlFor="typed-cad-source">{current ? 'Javítás · #cad_review' : 'Alapmodell · #cad_spec és #cad_target'}</label>
      <textarea {...domIds.studio.typedCad.source.attrs} id="typed-cad-source" value={source} onChange={event => setSource(event.target.value)} maxLength={65_536} rows={10} spellCheck={false} placeholder={current ? '#cad_review(...)' : '#cad_spec(...)\n#cad_target(...)'} disabled={working} />
      <button type="submit" disabled={working || !source.trim()}>{working ? 'Ellenőrzés…' : current ? 'Javítás ellenőrzése és elfogadása' : 'Modell ellenőrzése és létrehozása'}</button>
    </form>
    {error && <p className="inline-error" role="alert">{error}</p>}
  </div>;
}
