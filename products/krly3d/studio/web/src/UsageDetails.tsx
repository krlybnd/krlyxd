import type { Source, Usage, UsageCall } from './client';

// Snapshots include preceding calls, also across HITL resumes. The latest value
// for an id replaces the earlier value; repeated snapshots are never added.
export function mergeUsage(previous: Usage | undefined, snapshot: Usage | undefined): Usage | undefined {
  if (!snapshot) return previous;
  return { calls: [...new Map([...(previous?.calls ?? []), ...snapshot.calls].map(call => [call.id, call])).values()] };
}

const number = new Intl.NumberFormat('hu-HU');
const dollars = new Intl.NumberFormat('hu-HU', { minimumFractionDigits: 2, maximumFractionDigits: 10 });
function usd(value: number) {
  return `${value > 0 && value < 1e-10 ? value.toExponential(3) : dollars.format(value)} USD`;
}
function knownCost(call: UsageCall) {
  return (call.costKind === 'reported' || call.costKind === 'estimated') && Number.isFinite(call.costUsd) && call.costUsd >= 0;
}
const costLabels = { reported: 'szolgáltató által jelentett', estimated: 'becsült', unknown: 'ismeretlen' };
const stages: Record<string, string> = { cad_review: 'Formaellenőrzés', cad_repair: 'Tervjavítás', expert: 'Szakértő', research: 'Kutatás', image_search: 'Képkeresés', plan: 'Tervezés', understand: 'Értelmezés', vision: 'Képelemzés' };

export function UsageDetails({ usage }: { usage?: Usage }) {
  const calls = mergeUsage(undefined, usage)?.calls;
  const known = calls?.filter(knownCost) ?? [];
  const total = known.reduce((sum, call) => sum + call.costUsd, 0);
  const unknown = (calls?.length ?? 0) - known.length;
  const estimated = known.some(call => call.costKind === 'estimated');
  const reported = known.some(call => call.costKind === 'reported');
  const costLabel = estimated && reported ? 'jelentett és becsült' : estimated ? 'becsült' : 'szolgáltató által jelentett';
  const knownTokens = calls?.filter(call => call.tokensKnown) ?? [];
  const summaryCost = calls?.length ? !known.length ? ' · költség ismeretlen' : ` · ${unknown ? 'legalább ' : estimated ? '≈ ' : ''}${usd(total)}` : '';
  return <details className="usage-details">
    <summary>Használat és költség{calls ? ` · ${calls.length} AI-hívás` : ' · nincs adat'}{summaryCost}</summary>
    {!calls ? <p>A használat és a költség még nem ismert.</p> : !calls.length ? <p>Ebben a műveletben 0 AI-hívás. Ez nem a teljes alkalmazás költsége.</p> : <>
      <p className="usage-total"><strong>Kérés összesen (a folytatásokkal): </strong>{!known.length ? 'Ismeretlen költség' : `${unknown ? 'Részleges költség: ' : ''}${usd(total)} · ${costLabel}`}{unknown > 0 && ` · ${unknown} hívás költsége ismeretlen`}</p>
      <p>{knownTokens.length ? `${number.format(knownTokens.reduce((sum, call) => sum + call.totalTokens, 0))} token${knownTokens.length < calls.length ? ' (részleges; további tokenszámok ismeretlenek)' : ' összesen'}` : 'A tokenszám ismeretlen.'} · {number.format(calls.reduce((sum, call) => sum + call.searchCalls, 0))} keresés</p>
      <ul className="usage-calls">{calls.map(call => <li key={call.id}>
        <strong>{stages[call.stage] || call.stage} · {call.model || 'Ismeretlen modell'}</strong>
        <span>{knownCost(call) ? `${usd(call.costUsd)} · ${costLabels[call.costKind]}` : 'Ismeretlen költség'}</span>
        <span>{call.tokensKnown ? `Bemenet: ${number.format(call.inputTokens)} · kimenet: ${number.format(call.outputTokens)} · gyorsítótár: ${number.format(call.cachedTokens)} · összesen: ${number.format(call.totalTokens)} token` : 'Tokenszám ismeretlen (bemenet, kimenet, gyorsítótár, összesen).'}</span>
        <span>Keresések: {number.format(call.searchCalls)}</span>
      </li>)}</ul>
    </>}
  </details>;
}

function safeUrl(value: string): string | undefined {
  try {
    const url = new URL(value);
    if (url.protocol === 'https:' || url.protocol === 'http:') return url.href;
  } catch { /* Invalid and non-web sources remain plain text. */ }
}

export function Sources({ sources }: { sources: Source[] }) {
  if (!sources.length) return null;
  return <div className="response-sources"><strong>Források</strong><ul>{sources.map(source => {
    const url = safeUrl(source.url);
    return <li key={source.url}>{url ? <a href={url} target="_blank" rel="noopener noreferrer">{source.title || source.url}</a> : <span>{source.title || 'Nem megnyitható forrás'}</span>}</li>;
  })}</ul></div>;
}
