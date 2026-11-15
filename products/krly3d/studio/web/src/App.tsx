import { domIds } from '../../ssot/domIds';
import { useCallback, useEffect, useMemo, useReducer, useRef, useState, type ChangeEvent, type FormEvent } from 'react';
import { ArrowDown, ArrowDownToLine, ArrowRight, ArrowUp, AudioLines, Box, Check, CheckCheck, ChevronDown, ChevronRight, CircleAlert, Command, ImagePlus, LoaderCircle, MessageSquare, Mic, Paperclip, RefreshCw, Settings2, Square, Terminal, X } from 'lucide-react';
import { health, upload, run, transcribe, download, type CadState, type Health, type StudioEvent, type RunInput, type ExpertQuestionData, type Usage, type Source } from './client';
import { TypedCadPanel } from './TypedCadPanel';
import { ExpertQuestion } from './ExpertQuestion';
import { UsageDetails, Sources, mergeUsage } from './UsageDetails';
import { startRecording, type Recording } from './voice';
import { Viewport } from './Viewport';
import { initialModelState, modelReducer, prepareModel } from './model';
import type { StudioMesh } from './scene';

type RunMode = 'chat' | 'build' | 'demo' | 'complex' | 'character';
interface Attachment { key: string; file: File; url: string }
interface RunRecord {
  id: string;
  prompt: string;
  mode: RunMode;
  attachments: { name: string; url: string }[];
  events: StudioEvent[];
  state: 'running' | 'waiting' | 'completed' | 'error' | 'cancelled';
  question?: ExpertQuestionData;
  usage?: Usage;
  sources: Source[];
  error?: string;
}
interface Operation {
  input: RunInput;
  attachments: RunRecord['attachments'];
  usage?: Usage;
  sources: Source[];
}
interface PendingQuestion { recordId: string; question: ExpertQuestionData; operation: Operation }

const errorText = (cause: unknown) => cause instanceof Error ? cause.message : 'Ismeretlen hiba történt.';
const format = new Intl.NumberFormat('hu-HU', { maximumFractionDigits: 2 });
const isAbort = (cause: unknown) => cause instanceof Error && cause.name === 'AbortError';
const EMPTY_MESHES: StudioMesh[] = [];

function voiceError(message: string): string {
  // The helper supplies localized, sanitized errors; keep its actionable detail.
  if (/[^\x00-\x7F]/.test(message)) return message;
  if (/permission was denied/i.test(message)) return 'A mikrofonengedélyt elutasítottad. Engedélyezd a mikrofont a böngészőben, majd próbáld újra.';
  if (/no microphone/i.test(message)) return 'Nem található mikrofon.';
  if (/busy or unavailable/i.test(message)) return 'A mikrofon foglalt vagy nem érhető el.';
  if (/timed out/i.test(message)) return 'A mikrofonengedély kérése lejárt. Kattints újra a mikrofonra.';
  if (/size limit/i.test(message)) return 'A hangfelvétel túl nagy. Készíts rövidebb felvételt.';
  if (/empty/i.test(message)) return 'A hangfelvétel üres. Beszélj a mikrofonba a leállítás előtt.';
  if (/disconnected/i.test(message)) return 'A mikrofon kapcsolata megszakadt. Próbáld újra a felvételt.';
  if (/browser|format|https/i.test(message)) return 'A hangrögzítéshez támogatott böngésző és HTTPS vagy localhost szükséges.';
  return 'Nem sikerült rögzíteni a hangot. Ellenőrizd a mikrofont, majd próbáld újra.';
}

const phases: Record<string, string> = {
  accepted: 'KÉRÉS FOGADVA', understand: 'ÉRTELMEZÉS', plan: 'MŰVELETI ÖSSZEFOGLALÓ',
  geometry: 'GEOMETRIAI ESZKÖZ', started: 'NATÍV FELDOLGOZÁS', evidence: 'KONTÚR',
  coarse_preview: 'KORAI HÁLÓ', editable_proposal: 'B-REP MODELL', validated: 'ELLENŐRZÖTT MODELL', preview: 'ELŐNÉZET',
  validation: 'ELLENŐRZÉS', completed: 'BEFEJEZVE', failed: 'HIBA',
  construction_step: 'ÉPÍTÉSI LÉPÉS', sampling: 'TÉRFOGAT MINTAVÉTELEZÉSE',
  field_mesh: 'TÉRBELI HÁLÓ', final_mesh: 'TÉRBELI MESH', printing: 'STL ÉS ELLENŐRZÉS', print_validation: 'NYOMTATÁSI JELENTÉS',
  cad_review: 'FORMA ÉS MÉRETEK', cad_validate: 'TERVELLENŐRZÉS', cad_repair: 'TERVJAVÍTÁS', cad_failed: 'ELUTASÍTOTT TERV', expert: 'SZAKÉRTŐ', research: 'KUTATÁS', image_search: 'KÉPKERESÉS', question: 'PONTOSÍTÓ KÉRDÉS', usage: 'HASZNÁLAT',
};

function Execution({ record }: { record: RunRecord }) {
  const [expanded, setExpanded] = useState(true);
  const events = record.events.filter(event => event.kind !== 'message' && event.kind !== 'artifact');
  if (!events.length && record.state !== 'running' && !record.error) return null;
  const running = record.state === 'running';
  const latest = events.at(-1);
  return <div className={`execution ${record.state === 'error' ? 'execution-error' : ''}`}>
    <button className="execution-summary" onClick={() => setExpanded(!expanded)} aria-expanded={expanded}>
      {running ? <LoaderCircle size={14} className="spin" /> : record.state === 'waiting' ? <MessageSquare size={14} /> : record.state === 'error' ? <CircleAlert size={14} /> : record.state === 'cancelled' ? <Square size={12} /> : <CheckCheck size={15} />}
      <span>{running ? 'Végrehajtás folyamatban' : record.state === 'waiting' ? 'Válaszra vár' : record.state === 'error' ? 'A végrehajtás megszakadt' : record.state === 'cancelled' ? 'Leállítva' : 'Végrehajtás befejezve'}</span>
      {latest && <span className="execution-time">{format.format(latest.elapsedMs / 1000)} s</span>}
      <ChevronDown size={14} className={expanded ? 'chevron-open' : ''} />
    </button>
    {expanded && <div className="execution-events" aria-label="Megfigyelt végrehajtási események">
      {!events.length && running && <p className="execution-wait">Várakozás a szolgáltatás visszajelzésére…</p>}
      {events.map(event => <div className={`execution-event event-${event.kind}`} key={`${event.runId}-${event.seq}`}>
        <span className="event-marker">{event.kind === 'error' ? <X size={11} /> : event.kind === 'completed' ? <Check size={11} /> : event.kind === 'mesh' ? <Box size={11} /> : <Terminal size={11} />}</span>
        <div><span className="event-phase">{phases[event.phase] || event.phase}</span><p>{event.text || (event.kind === 'mesh' ? 'A geometria frissült.' : event.kind === 'completed' ? 'Kész.' : 'Állapotfrissítés')}</p></div>
      </div>)}
      {record.error && <p className="inline-error" role="alert">{record.error}</p>}
    </div>}
  </div>;
}

function ArtifactButton({ artifact, ready }: { artifact: NonNullable<StudioEvent['artifact']>; ready: boolean }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const abort = useRef<AbortController | null>(null);
  useEffect(() => () => abort.current?.abort(), []);
  async function save() {
    if (!ready || busy) return;
    setBusy(true); setError('');
    abort.current = new AbortController();
    try {
      const result = await download(artifact.id, abort.current.signal);
      const bytes = new Uint8Array(result.bytes);
      const url = URL.createObjectURL(new Blob([bytes], { type: result.mime }));
      const link = document.createElement('a');
      link.href = url; link.download = result.name; link.click();
      // Give the browser time to start the download before releasing its URL.
      setTimeout(() => URL.revokeObjectURL(url), 1000);
    } catch (cause) { if (!isAbort(cause)) setError(errorText(cause)); }
    finally { if (!abort.current.signal.aborted) setBusy(false); }
  }
  return <div><button className="artifact" onClick={() => void save()} disabled={busy || !ready} title={ready ? 'Elkészült fájl letöltése' : 'Előzetes fájl, az építés még nem fejeződött be sikeresen.'}>{busy ? <LoaderCircle size={16} className="spin" /> : <ArrowDownToLine size={16} />}<span>{artifact.name}<small>{format.format(artifact.size / 1024)} KB{!ready && ' · nem végleges'}</small></span><ChevronRight size={14} /></button>{error && <p className="inline-error" role="alert">{error}</p>}</div>;
}

export function App() {
  const [sessionId] = useState(() => crypto.randomUUID());
  const [connection, setConnection] = useState<Health | null>(null);
  const [checking, setChecking] = useState(true);
  const [connectionError, setConnectionError] = useState('');
  const [text, setText] = useState('');
  const [mode, setMode] = useState<'chat' | 'build'>('chat');
  const [records, setRecords] = useState<RunRecord[]>([]);
  const [pendingQuestion, setPendingQuestion] = useState<PendingQuestion | null>(null);
  const pendingRef = useRef<PendingQuestion | null>(null);
  const [active, setActive] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [model, dispatchModel] = useReducer(modelReducer, initialModelState);
  const [typedMode, setTypedMode] = useState(false);
  const [cadState, setCadState] = useState<CadState | null>(null);
  const typedMeshes = useMemo<StudioMesh[]>(() => cadState?.mesh ? [{
    id: `cad:${cadState.modelId}:${cadState.activeEntry}`,
    name: cadState.modelId,
    positions: cadState.mesh.positions,
    indices: cadState.mesh.indices,
  }] : EMPTY_MESHES, [cadState]);
  const displayedModel = model.preview ?? model.confirmed;
  const mesh = typedMode ? cadState?.mesh ?? null : displayedModel?.mesh ?? null;
  const sceneMeshes = typedMode ? typedMeshes : displayedModel?.meshes ?? EMPTY_MESHES;
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [sourceImage, setSourceImage] = useState<{ name: string; url: string } | null>(null);
  const [error, setError] = useState('');
  const [dragging, setDragging] = useState(false);
  const [settings, setSettings] = useState(false);
  const [height, setHeight] = useState('3');
  const [pixelUnit, setPixelUnit] = useState('1');
  const [tolerance, setTolerance] = useState('0.05');
  const [mobileTab, setMobileTab] = useState<'model' | 'chat'>('model');
  const [showScroll, setShowScroll] = useState(false);
  const [voiceState, setVoiceState] = useState<'idle' | 'starting' | 'recording' | 'processing'>('idle');
  const input = useRef<HTMLTextAreaElement>(null);
  const picker = useRef<HTMLInputElement>(null);
  const scroll = useRef<HTMLDivElement>(null);
  const stickBottom = useRef(true);
  const runAbort = useRef<AbortController | null>(null);
  const healthAbort = useRef<AbortController | null>(null);
  const voiceAbort = useRef<AbortController | null>(null);
  const objectUrls = useRef(new Set<string>());
  const dragDepth = useRef(0);
  const running = useRef(false);
  const recorder = useRef<Recording | null>(null);
  const voiceCaptureAbort = useRef<AbortController | null>(null);

  const checkHealth = useCallback(async () => {
    healthAbort.current?.abort();
    const abort = new AbortController(); healthAbort.current = abort;
    setChecking(true); setConnectionError('');
    try {
      const result = await health(abort.signal);
      if (!abort.signal.aborted) setConnection(result);
    } catch (cause) {
      if (!abort.signal.aborted) { setConnection(null); setConnectionError(errorText(cause)); }
    } finally { if (!abort.signal.aborted) setChecking(false); }
  }, []);

  useEffect(() => {
    void checkHealth();
    return () => {
      healthAbort.current?.abort(); runAbort.current?.abort(); voiceAbort.current?.abort();
      voiceCaptureAbort.current?.abort(); recorder.current?.cancel();
      objectUrls.current.forEach(url => URL.revokeObjectURL(url));
      objectUrls.current.clear();
    };
  }, [checkHealth]);

  useEffect(() => {
    if (stickBottom.current) scroll.current?.scrollTo({ top: scroll.current.scrollHeight, behavior: 'instant' });
  }, [records, active]);
  useEffect(() => {
    if (!input.current) return;
    input.current.style.height = 'auto';
    input.current.style.height = `${Math.min(input.current.scrollHeight, 180)}px`;
  }, [text]);

  function addFiles(files: FileList | File[]) {
    if (running.current) return;
    const images = Array.from(files).filter(file => /^image\/(png|jpeg)$/.test(file.type));
    if (images.length !== files.length) setError('PNG vagy JPEG képet csatolj.');
    else if (images.length > 1) setError('Egyszerre egy rajz építhető. Az első képet csatoltam.');
    else setError('');
    if (!images.length) return;
    if (images[0].size > 16 * 1024 * 1024) { setError('A kép legfeljebb 16 MiB méretű lehet.'); return; }
    const next = images.slice(0, 1).map(file => {
      const url = URL.createObjectURL(file); objectUrls.current.add(url);
      return { key: crypto.randomUUID(), file, url };
    });
    setAttachments(current => {
      current.forEach(item => { URL.revokeObjectURL(item.url); objectUrls.current.delete(item.url); });
      return next;
    });
    if (images.length) setMode('build');
    setMobileTab('chat');
  }
  function removeAttachment(key: string) {
    setAttachments(current => current.filter(item => {
      if (item.key !== key) return true;
      URL.revokeObjectURL(item.url); objectUrls.current.delete(item.url); return false;
    }));
  }
  function updateRecord(id: string, update: (record: RunRecord) => RunRecord) {
    setRecords(current => current.map(record => record.id === id ? update(record) : record));
  }
  function changePending(pending: PendingQuestion | null) {
    pendingRef.current = pending;
    setPendingQuestion(pending);
  }
  async function execute(requestedMode: RunMode, answer?: { questionId: string; answerIds: string[] }) {
    if (running.current || !connection) return;
    const resume = answer ? pendingRef.current : null;
    if (answer && (!resume || resume.question.id !== answer.questionId)) return;
    const selected = resume?.question.options.filter(option => answer?.answerIds.includes(option.id)) ?? [];
    if (answer && !selected.length) return;
    if (resume) requestedMode = resume.operation.input.mode;
    if (voiceState !== 'idle') { setError('Előbb fejezd be a hangfelvételt és annak átírását.'); return; }
    if (requestedMode === 'chat' && !connection.chat) { setError('A beszélgetéshez beállított nyelvi modell szükséges. Képből az Építés módban AI nélkül is készülhet geometria.'); return; }
    if (requestedMode !== 'chat' && !(requestedMode === 'character' ? connection.nativeCharacter : connection.native)) { setError('A natív geometriai szolgáltatás jelenleg nem érhető el.'); return; }
    if (!resume && requestedMode === 'build' && !attachments.length && !sourceImage && !text.trim()) { setError('Írd le röviden a formát, vagy csatolj egy felülnézeti rajzot.'); return; }
    if (!resume && requestedMode === 'build' && !attachments.length && !sourceImage && !connection.chat) { setError('Kép nélküli építéshez beállított AI-kapcsolat szükséges. Csatolj egy felülnézeti rajzot, vagy állítsd be az AI-kapcsolatot.'); return; }
    if (!resume && requestedMode === 'chat' && !text.trim() && !attachments.length) return;
    const values = resume ? [resume.operation.input.height, resume.operation.input.pixelUnit, resume.operation.input.tolerance] : requestedMode === 'character' ? [8, .025, .05] : [Number(height), Number(pixelUnit), Number(tolerance)];
    if (values.some(value => !Number.isFinite(value) || value <= 0)) { setError('A magasság, képpontlépték és tolerancia pozitív szám legyen.'); setSettings(true); return; }
    const id = crypto.randomUUID();
    const prompt = resume ? `Válasz: ${selected.map(option => option.label).join('; ')}` : text.trim();
    const isDemo = requestedMode === 'demo' || requestedMode === 'complex' || requestedMode === 'character';
    const sentAttachments = isDemo || resume ? [] : [...attachments];
    const abort = new AbortController(); runAbort.current = abort;
    running.current = true; setActive(true); setError(''); stickBottom.current = true;
    const record: RunRecord = { id, prompt: isDemo ? requestedMode === 'character' ? 'Stilizált Pikachu · térbeli mesh' : requestedMode === 'complex' ? 'Összetett demó betöltése' : 'Demó betöltése' : prompt || `${sourceImage && !sentAttachments.length ? 'Modell újraépítése' : '3D modell építése a csatolt képből'} · magasság: ${height}, lépték: ${pixelUnit}, tolerancia: ${tolerance}`, mode: requestedMode, attachments: sentAttachments.map(item => {
      const url = URL.createObjectURL(item.file); objectUrls.current.add(url);
      return { name: item.file.name, url };
    }), events: [], state: 'running', usage: resume?.operation.usage, sources: resume?.operation.sources ?? [] };
    const operation: Operation = resume?.operation ?? {
      input: { sessionId, text: isDemo ? '' : prompt, attachmentIds: [], mode: requestedMode, height: values[0], pixelUnit: values[1], tolerance: values[2] },
      attachments: record.attachments, sources: [],
    };
    // An ordinary request supersedes the old form immediately. A resume keeps
    // its pending id until a successful question/completion, including on abort.
    if (!resume) changePending(null);
    setRecords(current => [...current, record]);
    dispatchModel({ type: 'begin', runId: id });
    let terminal: 'completed' | 'error' | 'question' | null = null;
    let nextQuestion: ExpertQuestionData | undefined;
    let failure: string | undefined;
    let affectsGeometry = requestedMode !== 'chat' && !(requestedMode === 'build' && !attachments.length && !sourceImage);
    let latestMeshPhase: string | null = null;
    try {
      const attachmentIds: string[] = [];
      if (sentAttachments.length) setUploading(true);
      for (const item of sentAttachments) {
        const result = await upload(item.file, abort.signal); attachmentIds.push(result.id);
      }
      setUploading(false);
      if (!resume) operation.input.attachmentIds = attachmentIds;
      const request: RunInput = resume ? { ...operation.input, text: '', attachmentIds: [], questionId: resume.question.id, answerIds: selected.map(option => option.id) } : operation.input;
      for await (const event of run(request, abort.signal)) {
        if (abort.signal.aborted) break;
        operation.usage = mergeUsage(operation.usage, event.usage);
        operation.sources = [...new Map([...operation.sources, ...event.sources].map(source => [source.url, source])).values()];
        const { usage, sources } = operation;
        updateRecord(id, current => ({ ...current, events: [...current.events, event], usage, sources }));
        if (event.kind === 'question') {
          if (!event.question?.id || !event.question.options.length) throw new Error('A szolgáltatás érvénytelen kérdést küldött.');
          nextQuestion = event.question;
          terminal = 'question';
          break;
        }
        if (event.phase === 'geometry' || event.mesh) affectsGeometry = true;
        if (event.mesh) {
          dispatchModel({ type: 'preview', runId: id, snapshot: prepareModel(event.mesh, id, event.text || 'Geometriai lépés', { key: `${id}:${event.seq}`, phase: event.phase }, event.construction) });
          latestMeshPhase = event.phase;
        }
        if (event.kind === 'completed' && affectsGeometry &&
          !(requestedMode === 'character'
            ? latestMeshPhase === 'final_mesh'
            : latestMeshPhase === 'editable_proposal' || latestMeshPhase === 'validated')) {
          throw new Error('A szolgáltatás nem küldött végleges geometriát. A korai háló nem végleges modell.');
        }
        if (event.kind === 'error' || event.kind === 'completed') { terminal = event.kind; break; }
      }
    } catch (cause) {
      if (isAbort(cause)) abort.abort();
      else failure = errorText(cause);
    } finally {
      // Completion and suspension are terminal even if the transport closes
      // immediately afterward. Only completion commits provisional geometry.
      const outcome = terminal === 'completed' ? 'completed' : terminal === 'question' ? 'waiting' : abort.signal.aborted ? 'cancelled' : 'error';
      if (outcome === 'completed') {
        changePending(null);
        dispatchModel({ type: 'complete', runId: id });
        if (operation.attachments[0]) setSourceImage(operation.attachments[0]);
      } else if (outcome === 'waiting' && nextQuestion) {
        changePending({ recordId: id, question: nextQuestion, operation });
        dispatchModel({ type: 'rollback', runId: id, reason: 'cancelled', affectsGeometry: false });
      } else dispatchModel({ type: 'rollback', runId: id, reason: outcome === 'cancelled' ? 'cancelled' : 'error', affectsGeometry });
      if ((outcome === 'completed' || outcome === 'waiting') && !isDemo && !resume) {
        setText(''); setAttachments([]);
        sentAttachments.forEach(item => { URL.revokeObjectURL(item.url); objectUrls.current.delete(item.url); });
      }
      updateRecord(id, current => ({ ...current, state: outcome, question: nextQuestion, error: outcome === 'error' ? failure ?? (terminal === 'error' ? undefined : 'Az adatfolyam befejező esemény nélkül lezárult.') : undefined }));
      running.current = false; setActive(false); setUploading(false);
      if (runAbort.current === abort) runAbort.current = null;
    }
  }

  async function toggleVoice() {
    if (voiceState === 'starting') { voiceCaptureAbort.current?.abort(); setVoiceState('idle'); return; }
    if (voiceState === 'recording') {
      const capture = recorder.current;
      if (!capture) return;
      setVoiceState('processing');
      const abort = new AbortController(); voiceAbort.current = abort;
      try {
        const blob = await capture.stop();
        const value = await transcribe(blob, abort.signal);
        if (!abort.signal.aborted) { setText(current => `${current}${current ? '\n' : ''}${value}`); input.current?.focus(); }
      } catch (cause) {
        if (!abort.signal.aborted && !isAbort(cause)) setError(errorText(cause).match(/^[\x00-\x7F]*$/) ? voiceError(errorText(cause)) : errorText(cause));
      } finally {
        recorder.current = null;
        if (!abort.signal.aborted) setVoiceState('idle');
      }
      return;
    }
    if (voiceState !== 'idle' || !connection?.speech) return;
    const abort = new AbortController(); voiceCaptureAbort.current = abort;
    setVoiceState('starting'); setError('');
    try {
      const capture = await startRecording(message => {
        if (abort.signal.aborted || voiceCaptureAbort.current !== abort) return;
        if (/korlátot/i.test(message)) setError('A felvétel elérte a 60 másodperces korlátot. A mikrofon gombjával kérheted az átírását.');
        else { setError(voiceError(message)); setVoiceState('idle'); recorder.current = null; }
      }, { signal: abort.signal });
      if (abort.signal.aborted) { capture.cancel(); return; }
      recorder.current = capture; setVoiceState('recording');
    } catch (cause) { if (!abort.signal.aborted && !isAbort(cause)) { setError(voiceError(errorText(cause))); setVoiceState('idle'); } }
  }

  function onSubmit(event: FormEvent) { event.preventDefault(); void execute(mode); }
  function onPick(event: ChangeEvent<HTMLInputElement>) { if (event.target.files) addFiles(event.target.files); event.target.value = ''; }
  const backendReady = Boolean(connection);
  const canRun = !active && backendReady && voiceState === 'idle' && (mode === 'build' ? Boolean(connection?.native && (attachments.length || sourceImage || (connection.chat && text.trim()))) : Boolean(connection?.chat && (text.trim() || attachments.length)));
  const jumpToBottom = () => { stickBottom.current = true; scroll.current?.scrollTo({ top: scroll.current.scrollHeight, behavior: 'smooth' }); };

  return <div {...domIds.studio.attrs} className="studio-shell">
    <a className="skip-link" href={typedMode ? '#typed-cad-source' : '#composer'}>Ugrás a szerkesztőhöz</a>
    <header className="app-header">
      <a className="brand" href="/" aria-label="KRLY Studio kezdőlap"><span className="brand-mark"><Box size={22} strokeWidth={1.3} /></span><span>KRLY</span><span className="brand-slash">/</span><span className="brand-studio">Studio</span></a>
      <div className="document-label"><span className="document-dot" />{typedMode ? cadState ? `${cadState.modelId} · r${cadState.revision}` : 'Új CAD-modell' : model.preview ? 'Előzetes modell' : mesh ? 'Elkészült modell' : 'Névtelen munkatér'}</div>
      <div className="connection"><span className={`connection-dot ${checking ? 'checking' : backendReady ? 'online' : 'offline'}`} /><span>{checking ? 'Kapcsolódás…' : backendReady ? 'Szolgáltatás elérhető' : 'Nincs kapcsolat'}</span><button className="icon-button" onClick={() => void checkHealth()} disabled={checking || active} aria-label="Kapcsolat újraellenőrzése" title="Kapcsolat újraellenőrzése"><RefreshCw size={13} className={checking ? 'spin' : ''} /></button></div>
    </header>
    <nav className="mobile-tabs" aria-label="Munkaterületek"><button className={mobileTab === 'model' ? 'is-active' : ''} onClick={() => setMobileTab('model')}><Box size={15} />Modell</button><button className={mobileTab === 'chat' ? 'is-active' : ''} onClick={() => setMobileTab('chat')}><MessageSquare size={15} />Beszélgetés{active && <LoaderCircle size={12} className="spin" />}</button></nav>
    <main className={`workspace mobile-${mobileTab}`}>
      <Viewport meshes={sceneMeshes} stages={typedMode ? [] : model.stages} construction={typedMode ? undefined : model.preview?.construction ?? model.confirmed?.construction} generation={typedMode ? cadState ? `cad:${cadState.modelId}:${cadState.activeEntry}` : null : model.generation} preview={typedMode ? false : Boolean(model.preview)} notice={typedMode ? null : model.notice} busy={active} connected={Boolean(connection?.native)} onDemo={() => { setTypedMode(false); void execute('demo'); }} onComplexDemo={() => { setTypedMode(false); void execute('complex'); }} onCharacterDemo={() => { setTypedMode(false); void execute('character'); }} characterConnected={Boolean(connection?.nativeCharacter)} units={mesh?.units === 'mm' ? 'mm' : mesh?.units === 'm' ? 'm' : 'egység'} onImport={() => { setTypedMode(false); setMobileTab('chat'); picker.current?.click(); }} />
      <aside className="chat-panel" aria-label="Beszélgetés és végrehajtás">
        <header className="chat-header"><div className="chat-title"><MessageSquare size={17} strokeWidth={1.5} /><h2>{typedMode ? 'Típusos CAD' : 'Beszélgetés'}</h2></div><button className="text-button" type="button" onClick={() => setTypedMode(!typedMode)} disabled={active}>{typedMode ? 'Beszélgetés' : 'Típusos CAD'}</button></header>
        {typedMode ? <TypedCadPanel onState={setCadState} /> : <>
        {!checking && !backendReady && <div className="service-notice" role="alert"><CircleAlert size={16} /><div><strong>A háttérszolgáltatás nem érhető el.</strong><p>{connectionError || 'Ellenőrizd, hogy a Studio szolgáltatás fut-e.'}</p><button className="text-button" onClick={() => void checkHealth()}><RefreshCw size={12} />Újrapróbálás</button></div></div>}
        {connection && (!connection.chat || !connection.native) && <div className="capability-notice"><CircleAlert size={14} /><span>{!connection.native ? 'A natív geometriai motor nem érhető el.' : 'AI nincs beállítva. Képből az Építés móddal dolgozhatsz.'}{connection.message && <details><summary>Részletek</summary><p>{connection.message}</p></details>}</span></div>}
        <div className="chat-scroll" ref={scroll} onScroll={() => { const el = scroll.current; if (!el) return; const near = el.scrollHeight - el.scrollTop - el.clientHeight < 80; stickBottom.current = near; setShowScroll(!near); }}>
          {!records.length && <div className="chat-welcome"><div className="welcome-symbol"><Command size={21} strokeWidth={1.2} /></div><span className="eyebrow">SZÖVEG → NATÍV CAD</span><h2>Mit alkossunk?</h2><p>Írd le az alkatrészt. A szabványos méreteket előbb szöveges forrásból ellenőrizzük, majd kép nélkül építjük fel a natív feature-lépéseket. Kép csak külön kontúrkihúzáshoz kell.</p><div className="welcome-actions"><button onClick={() => void execute('character')} disabled={active || !connection?.nativeCharacter}><Box size={17} /><span>Térbeli figura<small>Stilizált Pikachu · térbeli mesh · mm</small></span><ArrowRight size={15} /></button><button onClick={() => { setMode('build'); input.current?.focus(); }} disabled={active || !connection?.native}><ImagePlus size={17} /><span>Alkatrész leírása vagy rajz<small>Szövegből CAD · kép opcionális</small></span><ArrowRight size={15} /></button><button onClick={() => { setMode('chat'); setText('Milyen geometriai műveleteket tudsz elvégezni?'); input.current?.focus(); }}><MessageSquare size={17} /><span>Beszéljük át<small>Geometriai kérdések és tervezés</small></span><ArrowRight size={15} /></button></div><p className="welcome-footnote">A lépések a szolgáltatás tényleges visszajelzései.<br />CAD-recept és geometriapreview milliméterben.</p></div>}
          <div className="conversation" role="log" aria-label="Üzenetek és műveletek" aria-live="polite" aria-relevant="additions text">
            {records.map(record => <article className="run-record" key={record.id}>
              <div className="user-message"><span className="message-author">TE<span>{record.mode === 'character' ? 'Térbeli figura · demó' : record.mode === 'build' ? 'Építés' : record.mode === 'complex' ? 'Összetett demó' : record.mode === 'demo' ? 'Demó' : 'Beszélgetés'}</span></span>{record.attachments.length > 0 && <div className="message-images">{record.attachments.map((item, index) => <a key={index} href={item.url} target="_blank" rel="noreferrer" title={item.name}><img src={item.url} alt={item.name} /></a>)}</div>}<p>{record.prompt}</p></div>
              <div className="run-response"><div className="response-mark"><Box size={14} strokeWidth={1.4} /><span>STUDIO</span></div><Execution record={record} />{record.events.filter(event => event.kind === 'message' && event.text).map(event => <div className="assistant-message" key={`${event.runId}-${event.seq}`}><p>{event.text}</p></div>)}{record.events.filter(event => event.kind === 'artifact' && event.artifact).map(event => <ArtifactButton key={`${event.runId}-${event.seq}`} artifact={event.artifact!} ready={record.state === 'completed' || (record.state === 'error' && event.artifact!.name === 'print-report.json')} />)}
                {record.question && <ExpertQuestion question={record.question} active={pendingQuestion?.recordId === record.id} busy={active || voiceState !== 'idle' || !connection?.chat} onAnswer={answerIds => void execute('chat', { questionId: record.question!.id, answerIds })} />}
                <Sources sources={record.sources} />
                <UsageDetails usage={record.usage} />
              </div>
            </article>)}
          </div>
        </div>
        {showScroll && <button className="scroll-bottom icon-button glass" onClick={jumpToBottom} aria-label="Ugrás a legújabb eseményhez"><ArrowDown size={16} /></button>}
        {mesh && <div className="model-summary"><Box size={13} />{model.preview ? <><span className="warning-text">Előzetes geometria</span><span>Véglegesítésre vár</span></> : <><span>{format.format(mesh.volume)} {mesh.units === 'mm' ? 'mm³' : mesh.units === 'm' ? 'm³' : 'egység³'}</span><span>{format.format(mesh.area)} {mesh.units === 'mm' ? 'mm²' : mesh.units === 'm' ? 'm²' : 'egység²'}</span><span className={mesh.openEdges ? 'warning-text' : ''}>{mesh.openEdges ? `${mesh.openEdges} nyitott él` : 'Zárt háló'}</span></>}</div>}
        <div className="composer-area">
          <div className="composer-options"><div className="mode-switch" role="group" aria-label="Végrehajtás módja"><button className={mode === 'chat' ? 'is-active' : ''} onClick={() => setMode('chat')} disabled={active} aria-pressed={mode === 'chat'}>Beszélgetés</button><button className={mode === 'build' ? 'is-active' : ''} onClick={() => setMode('build')} disabled={active} aria-pressed={mode === 'build'}>Építés</button></div><button className={`icon-button ${settings ? 'is-active' : ''}`} onClick={() => setSettings(!settings)} aria-label="Építési beállítások" title="Építési beállítások" aria-expanded={settings}><Settings2 size={16} /></button></div>
          {mode === 'build' && sourceImage && !attachments.length && <button className="source-image" onClick={() => picker.current?.click()} disabled={active} title="Forráskép cseréje"><ImagePlus size={12} /><span>Újraépítés: {sourceImage.name}</span><span>Csere</span></button>}
          {settings && <div className="build-settings"><div><strong>Geometriai beállítások</strong><span>Saját modellegység · nincs feltételezett fizikai méret</span></div><div className="settings-fields"><label>Magasság<input type="number" min="0.000001" step="any" value={height} onChange={event => setHeight(event.target.value)} disabled={active} /><small>modellegység</small></label><label>Képpontlépték<input type="number" min="0.000001" step="any" value={pixelUnit} onChange={event => setPixelUnit(event.target.value)} disabled={active} /><small>egység / px</small></label><label>Tolerancia<input type="number" min="0.000001" step="any" value={tolerance} onChange={event => setTolerance(event.target.value)} disabled={active} /><small>modellegység</small></label></div></div>}
          {error && <div className="composer-error" role="alert"><CircleAlert size={14} /><span>{error}</span><button className="icon-button" onClick={() => setError('')} aria-label="Hibaüzenet bezárása"><X size={13} /></button></div>}
          <form className={`composer ${dragging ? 'is-dragging' : ''} ${voiceState === 'recording' ? 'is-recording' : ''}`} onSubmit={onSubmit}
            onDragEnter={event => { event.preventDefault(); if (event.dataTransfer.types.includes('Files')) { dragDepth.current++; setDragging(true); } }}
            onDragOver={event => { event.preventDefault(); event.dataTransfer.dropEffect = active ? 'none' : 'copy'; }}
            onDragLeave={event => { event.preventDefault(); if (--dragDepth.current <= 0) { dragDepth.current = 0; setDragging(false); } }}
            onDrop={event => { event.preventDefault(); dragDepth.current = 0; setDragging(false); if (event.dataTransfer.files.length) addFiles(event.dataTransfer.files); }}>
            {dragging && <div className="drop-overlay"><ImagePlus size={26} /><span>Engedd el a képet a csatoláshoz</span></div>}
            {attachments.length > 0 && <div className="attachment-list">{attachments.map(item => <div className="attachment-preview" key={item.key}><img src={item.url} alt={item.file.name} /><span title={item.file.name}>{item.file.name}</span><button type="button" disabled={active} onClick={() => removeAttachment(item.key)} aria-label={`${item.file.name} eltávolítása`}><X size={12} /></button></div>)}</div>}
            <label className="sr-only" htmlFor="composer">Üzenet a Studiónak</label><textarea id="composer" ref={input} value={text} onChange={event => setText(event.target.value)} maxLength={8000} placeholder={mode === 'build' ? sourceImage && !attachments.length ? 'Újraépítés vagy írd le a következő alkatrészt…' : 'Írd le az alkatrészt; kép csak rajz-kihúzáshoz kell…' : 'Kérdezz a rajzról vagy a modellről…'} rows={2} disabled={active || voiceState === 'processing'} onPaste={event => { const files = event.clipboardData.files; if (files.length) { event.preventDefault(); addFiles(files); } }} onKeyDown={event => { if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) { event.preventDefault(); if (canRun) void execute(mode); } }} />
            <div className="composer-bar">
              <div className="composer-tools">
                <button type="button" className="icon-button" disabled={active} onClick={() => picker.current?.click()} title="Kép csatolása" aria-label="Kép csatolása"><Paperclip size={18} /></button>
                <button type="button" className={`icon-button voice-button ${voiceState === 'recording' ? 'is-listening' : ''}`}
                  onClick={() => void toggleVoice()} disabled={active || !connection?.speech || voiceState === 'processing'}
                  title={!connection?.speech ? 'A hangátírás nincs beállítva' : voiceState === 'starting' ? 'Mikrofonkérés megszakítása' : voiceState === 'recording' ? 'Hangfelvétel leállítása' : 'Üzenet diktálása'}
                  aria-label={voiceState === 'starting' ? 'Mikrofonkérés megszakítása' : voiceState === 'recording' ? 'Hangfelvétel leállítása' : 'Üzenet diktálása'} aria-pressed={voiceState === 'recording'}>
                  {voiceState === 'processing' || voiceState === 'starting' ? <LoaderCircle size={18} className="spin" /> : voiceState === 'recording' ? <AudioLines size={18} /> : <Mic size={18} />}
                </button>
                <span className="composer-hint" role="status">{uploading ? 'Kép feltöltése…' : voiceState === 'starting' ? 'Mikrofon engedélyezése…' : voiceState === 'recording' ? 'Felvétel…' : voiceState === 'processing' ? 'Hang átírása…' : mode === 'build' ? attachments.length || sourceImage ? 'Csatolt képből · AI nélkül' : connection?.chat ? 'Szövegből natív CAD · kép opcionális' : 'Kép nélküli építéshez AI szükséges' : connection?.chat ? `${connection.model || 'AI'} · beállítva` : 'AI nincs beállítva'}</span>
              </div>
              {active ? <button type="button" className="send-button stop-button" onClick={() => runAbort.current?.abort()} aria-label="Végrehajtás leállítása" title="Leállítás"><Square size={14} fill="currentColor" /></button> : <button type="submit" className="send-button" disabled={!canRun} aria-label={mode === 'build' ? 'Modell építése' : 'Üzenet küldése'} title={mode === 'build' ? 'Modell építése' : 'Küldés (Enter)'}>{mode === 'build' ? <Box size={18} /> : <ArrowUp size={20} />}</button>}
            </div>
          </form>
          <div className="composer-footer"><span>Kép húzható ide · Shift + Enter: új sor</span><span>KRLY 3D</span></div>
        </div>
        </>}
      </aside>
    </main>
    <input ref={picker} className="sr-only" type="file" accept="image/png,image/jpeg" onChange={onPick} aria-label="Kép kiválasztása" />
  </div>;
}
