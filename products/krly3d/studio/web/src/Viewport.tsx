import { useEffect, useRef, useState } from 'react';
import { Box, ChevronDown, Expand, Grid2X2, Layers2, LoaderCircle, MousePointer2, Pause, Play, Rotate3D, RotateCcw, SkipForward, Triangle, Upload, X } from 'lucide-react';
import * as THREE from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import { validateMesh, type CameraView, type DisplayMode, type SceneStats, type StudioMesh } from './scene';
import { createRenderGeometry } from './renderGeometry';
import type { ModelSnapshot } from './model';
import { RevealTimeline, type RevealFrame } from './reveal';
import { CadFeatureTree } from './CadFeatureTree';
import { expandCadStages, sketchDisplayPoints, type CadPlaybackStage } from './cadConstruction';

type PlaybackStage = CadPlaybackStage;
interface PlaybackInfo extends RevealFrame {
  label: string;
  index: number;
  count: number;
  available: boolean;
  reduced: boolean;
  sketch?: boolean;
}

interface Props {
  meshes: StudioMesh[];
  stages: ModelSnapshot[];
  construction?: ModelSnapshot['construction'];
  generation: string | null;
  preview: boolean;
  notice: string | null;
  busy: boolean;
  connected: boolean;
  onDemo: () => void;
  onComplexDemo: () => void;
  onCharacterDemo: () => void;
  characterConnected: boolean;
  units: string;
  onImport: () => void;
}

interface ViewportController {
  replace: (meshes: StudioMesh[]) => void;
  sync: (meshes: StudioMesh[], stages: PlaybackStage[], generation: string | null, failed: boolean, busy: boolean) => void;
  replay: () => void;
  togglePlayback: () => void;
  skip: () => void;
  view: (view: CameraView) => void;
  fit: () => void;
  mode: (mode: DisplayMode) => void;
  grid: (visible: boolean) => void;
  selectCadFeature: (id: string) => void;
  selectCadReference: (reference: string | null) => void;
  construction: (data: ModelSnapshot['construction'], feature: string | null, plane: string | null) => void;
}

const emptyStats: SceneStats = { objects: 0, triangles: 0, vertices: 0, dimensions: null };
const number = new Intl.NumberFormat('hu-HU', { maximumFractionDigits: 2 });
const format = (value: number) => value !== 0 && Math.abs(value) < 0.01 ? value.toExponential(1).replace('.', ',') : number.format(value);

// Derivative antialiasing keeps the XY lines legible at oblique camera angles.
const gridVertex = `
  varying vec3 world;
  void main() {
    vec4 p = modelMatrix * vec4(position, 1.0);
    world = p.xyz;
    gl_Position = projectionMatrix * viewMatrix * p;
  }
`;
const gridFragment = `
  varying vec3 world;
  uniform float spacing;
  uniform float radius;
  uniform vec2 center;
  float line(vec2 p, float stepSize) {
    vec2 coord = p / stepSize;
    vec2 width = max(fwidth(coord), vec2(0.00001));
    vec2 d = abs(fract(coord - 0.5) - 0.5) / width;
    return 1.0 - min(min(d.x, d.y), 1.0);
  }
  void main() {
    float minor = line(world.xy, spacing);
    float major = line(world.xy, spacing * 5.0);
    float fade = 1.0 - smoothstep(radius * 0.25, radius, length(world.xy - center));
    vec2 axisWidth = max(fwidth(world.xy), vec2(0.00001));
    float xAxis = 1.0 - min(abs(world.y) / axisWidth.y, 1.0);
    float yAxis = 1.0 - min(abs(world.x) / axisWidth.x, 1.0);
    vec3 color = mix(vec3(0.39, 0.46, 0.48), vec3(0.40, 0.67, 0.65), major * 0.35);
    color = mix(color, vec3(0.72, 0.40, 0.39), xAxis * 0.52);
    color = mix(color, vec3(0.40, 0.64, 0.53), yAxis * 0.52);
    float alpha = max(max(minor * 0.17, major * 0.30), max(xAxis, yAxis) * 0.40) * fade;
    gl_FragColor = vec4(color, alpha);
  }
`;

export function Viewport({ meshes, stages, construction, generation, preview, notice, busy, connected, onDemo, onComplexDemo, onCharacterDemo, characterConnected, units, onImport }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const controller = useRef<ViewportController | null>(null);
  const [stats, setStats] = useState<SceneStats>(emptyStats);
  const [error, setError] = useState('');
  const [mode, setMode] = useState<DisplayMode>('solid');
  const [grid, setGrid] = useState(true);
  const [selected, setSelected] = useState<string | null>(null);
  const [spacing, setSpacing] = useState(1);
  const [help, setHelp] = useState(false);
  const [axes, setAxes] = useState([[29, 8], [-23, 13], [0, -30]]);
  const [renderedMeshes, setRenderedMeshes] = useState<StudioMesh[]>([]);
  const [selectedFeature, setSelectedFeature] = useState<string | null>('extrude');
  const [selectedPlane, setSelectedPlane] = useState<string | null>(null);
  const [originExpanded, setOriginExpanded] = useState(true);
  const [stepsExpanded, setStepsExpanded] = useState(true);
  const [extrudeExpanded, setExtrudeExpanded] = useState(true);
  const [sketchExpanded, setSketchExpanded] = useState(true);
  const [constructionPlayback, setConstructionPlayback] = useState<{ phase: 'idle' | 'sketch' | 'extruding' | 'complete'; progress: number }>({ phase: 'idle', progress: 0 });
  const [playback, setPlayback] = useState<PlaybackInfo>({ status: 'complete', shown: 0, total: 0, label: '', index: 0, count: 0, available: false, reduced: false });
  const [playbackFeatureId, setPlaybackFeatureId] = useState<string | null>(null);
  const [visibleSketch, setVisibleSketch] = useState<{ id: string; z: number; points: number } | null>(null);
  const imageConstruction = construction?.version === 1 ? construction : undefined;
  const cadConstruction = construction?.version === 2 ? construction : undefined;
  const cadFeatureStages = stages.filter(stage => /^\d+\/\d+ · /u.test(stage.name)).slice(-8);

  useEffect(() => {
    const container = host.current;
    if (!container) return;
    let renderer: THREE.WebGLRenderer;
    try {
      renderer = new THREE.WebGLRenderer({ antialias: true, alpha: false, powerPreference: 'high-performance' });
    } catch {
      setError('A 3D nézethez WebGL 2 szükséges. Engedélyezd a böngésző hardveres gyorsítását, majd töltsd újra az oldalt.');
      return;
    }
    const scene = new THREE.Scene();
    scene.background = new THREE.Color('#20252a');
    const camera = new THREE.PerspectiveCamera(38, 1, 0.01, 100000);
    camera.up.set(0, 0, 1);
    camera.position.set(12, -16, 12);
    const controls = new OrbitControls(camera, renderer.domElement);
    controls.enableDamping = true;
    controls.dampingFactor = 0.12;
    controls.screenSpacePanning = true;
    controls.minDistance = 0.0001;
    controls.maxDistance = 1e8;
    controls.target.set(0, 0, 0);
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    renderer.outputColorSpace = THREE.SRGBColorSpace;
    renderer.toneMapping = THREE.ACESFilmicToneMapping;
    renderer.toneMappingExposure = 1.18;
    renderer.shadowMap.enabled = true;
    renderer.shadowMap.type = THREE.PCFShadowMap;
    renderer.domElement.tabIndex = 0;
    renderer.domElement.setAttribute('aria-label', '3D munkatér. Egérrel forgatható, jobb gombbal mozgatható, görgetéssel nagyítható. F: illesztés, 1: izometrikus, 2: felül-, 3: elölnézet. Nyilak: forgatás, Shift és nyilak: mozgatás.');
    renderer.domElement.setAttribute('role', 'img');
    container.appendChild(renderer.domElement);

    const hemisphere = new THREE.HemisphereLight('#dceaf3', '#5b625d', 2.0);
    hemisphere.position.set(0, 0, 1);
    const sun = new THREE.DirectionalLight('#fff2dc', 3.4);
    sun.position.set(-9, -7, 16);
    sun.castShadow = true;
    sun.shadow.mapSize.set(2048, 2048);
    sun.shadow.normalBias = 0.035;
    sun.shadow.bias = -0.0001;
    const fill = new THREE.DirectionalLight('#9bbfd1', 0.9);
    fill.position.set(6, 8, 5);
    scene.add(hemisphere, sun, sun.target, fill);

    const models = new THREE.Group();
    scene.add(models);
    const constructionPreview = new THREE.Group();
    scene.add(constructionPreview);
    const guides = new THREE.Group();
    scene.add(guides);
    const ground = new THREE.Mesh(new THREE.PlaneGeometry(1, 1), new THREE.ShadowMaterial({ opacity: 0.19 }));
    ground.receiveShadow = true;
    ground.position.z = -0.002;
    ground.scale.setScalar(2000);
    scene.add(ground);
    const gridMaterial = new THREE.ShaderMaterial({
      vertexShader: gridVertex,
      fragmentShader: gridFragment,
      transparent: true,
      depthWrite: false,
      uniforms: { spacing: { value: 1 }, radius: { value: 80 }, center: { value: new THREE.Vector2() } },
    });
    const gridPlane = new THREE.Mesh(new THREE.PlaneGeometry(1, 1), gridMaterial);
    gridPlane.scale.setScalar(2000);
    gridPlane.position.z = -0.001;
    scene.add(gridPlane);
    let frame = 0;
    let constructionFrame = 0;
    let constructionRun = 0;
    let disposed = false;
    let activeMode: DisplayMode = 'solid';
    let selectedId: string | null = null;
    let lastSpacing = 0;
    let lastAxes = '';
    let sceneRadius = 4;
    const bounds = new THREE.Box3();
    const center = new THREE.Vector3();
    const inverse = new THREE.Quaternion();
    const timeline = new RevealTimeline();
    const motion = window.matchMedia('(prefers-reduced-motion: reduce)');
    let reduced = motion.matches;
    let surfaceVisible = container.clientWidth > 0 && container.clientHeight > 0;
    let loadedData: StudioMesh[] | null = null;
    let latestData: StudioMesh[] = [];
    let history: PlaybackStage[] = [];
    let confirmedHistory: PlaybackStage[] = [];
    let confirmedData: StudioMesh[] | null = null;
    let pending: PlaybackStage[] = [];
    let currentStage: PlaybackStage | null = null;
    let activeGeneration: string | null = null;
    let blocked = false;
    let backendBusy = false;
    let manualPause = false;
    let autoPause = false;
    let skipRemaining = false;
    let forceConstructionMotion = false;
    let seen = new Set<string>();
    let lastPublish = -Infinity;
    let lastStatus = '';

    const triangleCount = (data: StudioMesh[]) => data.reduce((sum, item) => sum + (item.indices?.length ?? item.positions.length / 3) / 3, 0);
    function fallback(): PlaybackStage {
      return { key: 'current', name: latestData[0]?.name || 'Aktuális modell', phase: 'editable_proposal', meshes: latestData };
    }
    function publish(now: number, force = false) {
      const raw = timeline.read(now);
      const state = manualPause && pending.length ? { ...raw, status: 'paused' as const } : raw;
      const available = !blocked && latestData.length > 0 && (!backendBusy || history.length > 0);
      const key = `${state.status}:${history.length}:${available}:${reduced}`;
      if (!force && key === lastStatus && now - lastPublish < 90) return;
      lastPublish = now; lastStatus = key;
      const index = currentStage ? history.findIndex(stage => stage.key === currentStage!.key) + 1 : 0;
      setPlayback({ ...state, label: currentStage?.name || latestData[0]?.name || '', index: Math.max(1, index), count: Math.max(1, history.length), available, reduced, sketch: Boolean(currentStage?.sketchFeatureId) });
    }
    function updateVisibility() {
      const now = performance.now();
      if (document.hidden || !surfaceVisible) {
        if (timeline.read(now).status === 'playing') { timeline.pause(now); autoPause = true; }
      } else if (autoPause && !manualPause) {
        autoPause = false; timeline.resume(now); invalidate();
      }
      publish(now, true);
    }
    function cancelConstructionPreview() {
      constructionRun++;
      if (constructionFrame) cancelAnimationFrame(constructionFrame);
      constructionFrame = 0;
      clearModels(constructionPreview);
      models.visible = true;
    }
    function playConstruction(stage: PlaybackStage, animate: boolean, forceMotion = false) {
      const data = stage.construction;
      cancelConstructionPreview();
      if (!data || !animate || (reduced && !forceMotion)) {
        models.visible = true;
        if (loadedData !== stage.meshes) controller.current?.replace(stage.meshes);
        setConstructionPlayback({ phase: 'complete', progress: 1 });
        setSelectedFeature('extrude');
        updateConstruction(data, 'extrude', null);
        return;
      }
      try {
        const outer = data.sketchLoops.find(loop => !loop.hole);
        if (!outer || outer.pointsXy.length < 6) throw new Error('A kihúzás előnézetéhez hiányzik a zárt külső kontúr.');
        const shape = new THREE.Shape();
        shape.moveTo(outer.pointsXy[0], outer.pointsXy[1]);
        for (let index = 2; index + 1 < outer.pointsXy.length; index += 2) shape.lineTo(outer.pointsXy[index], outer.pointsXy[index + 1]);
        shape.closePath();
        for (const loop of data.sketchLoops.filter(item => item.hole)) {
          if (loop.pointsXy.length < 6) continue;
          const hole = new THREE.Path();
          hole.moveTo(loop.pointsXy[0], loop.pointsXy[1]);
          for (let index = 2; index + 1 < loop.pointsXy.length; index += 2) hole.lineTo(loop.pointsXy[index], loop.pointsXy[index + 1]);
          hole.closePath();
          shape.holes.push(hole);
        }
        const geometry = new THREE.ExtrudeGeometry(shape, { depth: data.extrusionDistance, bevelEnabled: false, curveSegments: 1, steps: 1 });
        const solid = new THREE.Mesh(geometry, new THREE.MeshStandardMaterial({
          color: '#11615b', emissive: '#00cdb5', emissiveIntensity: 1.65, metalness: 0.12, roughness: 0.3,
          transparent: true, opacity: 0.72, depthWrite: false, side: THREE.DoubleSide,
        }));
        solid.position.z = data.sketchOffset;
        solid.scale.z = 0;
        const edgeGeometry = new THREE.EdgesGeometry(geometry, 22);
        const halo = new THREE.LineSegments(edgeGeometry, new THREE.LineBasicMaterial({ color: '#00d9c5', transparent: true, opacity: 0.30, depthTest: false, toneMapped: false }));
        halo.position.z = data.sketchOffset;
        const edge = new THREE.LineSegments(edgeGeometry, new THREE.LineBasicMaterial({ color: '#a9fff0', transparent: true, opacity: 0.94, depthTest: false, toneMapped: false }));
        edge.position.z = data.sketchOffset;
        constructionPreview.add(solid, halo, edge);
        models.visible = false;
        updateConstruction(data, 'sketch', null);
        setSelectedFeature('sketch');
        setSelectedPlane(null);
        setConstructionPlayback({ phase: 'sketch', progress: 0 });
        const run = constructionRun;
        const started = performance.now();
        const sketchDuration = 130;
        const extrusionDuration = 300;
        let lastUi = 0;
        let enteredExtrude = false;
        const animateFrame = (now: number) => {
          if (disposed || run !== constructionRun) return;
          const elapsed = now - started;
          if (elapsed < sketchDuration) {
            constructionFrame = requestAnimationFrame(animateFrame);
            invalidate();
            return;
          }
          const raw = Math.min(1, (elapsed - sketchDuration) / extrusionDuration);
          const progress = raw * raw * (3 - 2 * raw);
          solid.scale.z = Math.max(progress, 0.0001);
          if (!enteredExtrude) {
            enteredExtrude = true;
            setSelectedFeature('extrude');
          }
          if (now - lastUi > 55 || raw >= 1) {
            setConstructionPlayback({ phase: 'extruding', progress });
            lastUi = now;
          }
          if (raw >= 1) {
            models.visible = true;
            if (loadedData !== stage.meshes) controller.current?.replace(stage.meshes);
            const fadeStarted = now;
            const fadeDuration = 150;
            const fadeToNative = (fadeNow: number) => {
              if (disposed || run !== constructionRun) return;
              const fade = Math.min(1, (fadeNow - fadeStarted) / fadeDuration);
              (solid.material as THREE.MeshStandardMaterial).opacity = 0.72 * (1 - fade);
              (halo.material as THREE.LineBasicMaterial).opacity = 0.30 * (1 - fade);
              (edge.material as THREE.LineBasicMaterial).opacity = 0.94 * (1 - fade);
              if (fade >= 1) {
                constructionFrame = 0;
                clearModels(constructionPreview);
                setConstructionPlayback({ phase: 'complete', progress: 1 });
                forceConstructionMotion = false;
                updateConstruction(data, 'extrude', null);
                invalidate();
                if (pending.length && !manualPause && !skipRemaining) playNext(fadeNow);
                return;
              }
              constructionFrame = requestAnimationFrame(fadeToNative);
              invalidate();
            };
            constructionFrame = requestAnimationFrame(fadeToNative);
            return;
          }
          constructionFrame = requestAnimationFrame(animateFrame);
          invalidate();
        };
        constructionFrame = requestAnimationFrame(animateFrame);
      } catch {
        models.visible = true;
        clearModels(constructionPreview);
        if (loadedData !== stage.meshes) controller.current?.replace(stage.meshes);
        setConstructionPlayback({ phase: 'complete', progress: 1 });
        setSelectedFeature('extrude');
        updateConstruction(data, 'extrude', null);
      }
    }
    function showStage(stage: PlaybackStage, animate: boolean, now = performance.now()) {
      currentStage = stage;
      cancelConstructionPreview();
      if (stage.construction?.version === 1 && stage.phase === 'editable_proposal') playConstruction(stage, animate, forceConstructionMotion);
      else {
        if (loadedData !== stage.meshes) controller.current?.replace(stage.meshes);
        setConstructionPlayback({ phase: 'idle', progress: 0 });
      }
      setPlaybackFeatureId(stage.featureId ?? null);
      if (stage.construction?.version === 2) {
        setSelectedFeature(stage.featureId ?? null);
        setSelectedPlane(null);
        updateConstruction(stage.construction, stage.sketchFeatureId ?? null, null);
      } else setVisibleSketch(null);
      now = performance.now();
      const intermediate = stage.phase !== 'editable_proposal';
      if (stage.sketchFeatureId) {
        timeline.start(0, now, 850, animate && !reduced, 0, true);
      } else {
        const image = stage.construction?.version === 1;
        timeline.start(triangleCount(stage.meshes), now, image ? 0 : intermediate ? 650 : history.length > 1 ? 2000 : 3000, animate && !reduced && !image, intermediate ? 150 : 0);
      }
      applyMode(); updateVisibility(); publish(now, true); invalidate();
    }
    function playNext(now = performance.now()) {
      const stage = pending.shift();
      if (stage) showStage(stage, true, now);
    }
    function showLatest() {
      pending = []; manualPause = false; autoPause = false;
      forceConstructionMotion = false;
      showStage(history.at(-1) ?? fallback(), false);
    }
    function updateDrawRange(state: RevealFrame) {
      // A sketch display stage leaves the preceding native solid fully visible.
      let remaining = currentStage?.sketchFeatureId ? triangleCount(currentStage.meshes) : state.shown;
      models.children.forEach(object => {
        if (object instanceof THREE.Mesh && object.userData.role === 'surface') {
          const count = Math.min(remaining, object.userData.triangles as number);
          object.geometry.setDrawRange(0, count * 3);
          remaining -= count;
        }
        if (object.userData.role === 'reveal-wire') object.visible = !currentStage?.sketchFeatureId && state.status !== 'complete';
        if (object.userData.role === 'feature-wire') object.visible = Boolean(currentStage?.sketchFeatureId) || state.status === 'complete';
      });
    }

    function draw(now: number) {
      frame = 0;
      if (disposed || document.hidden || !surfaceVisible) return;
      controls.update();
      const reveal = timeline.read(now);
      updateDrawRange(reveal);
      const distance = camera.position.distanceTo(controls.target);
      const visibleHeight = 2 * distance * Math.tan(THREE.MathUtils.degToRad(camera.fov / 2));
      const wanted = visibleHeight / 24;
      const power = 10 ** Math.floor(Math.log10(Math.max(wanted, 1e-9)));
      const ratio = wanted / power;
      const step = (ratio < 2 ? 1 : ratio < 5 ? 2 : 5) * power;
      gridMaterial.uniforms.spacing.value = step;
      gridMaterial.uniforms.radius.value = Math.max(visibleHeight * 2.4, sceneRadius * 2);
      gridMaterial.uniforms.center.value.set(controls.target.x, controls.target.y);
      gridPlane.position.x = controls.target.x;
      gridPlane.position.y = controls.target.y;
      gridPlane.scale.setScalar(Math.max(visibleHeight * 8, sceneRadius * 8));
      ground.position.x = controls.target.x;
      ground.position.y = controls.target.y;
      ground.scale.copy(gridPlane.scale);
      if (step !== lastSpacing) { lastSpacing = step; setSpacing(step); }
      inverse.copy(camera.quaternion).invert();
      const projected = [new THREE.Vector3(1, 0, 0), new THREE.Vector3(0, 1, 0), new THREE.Vector3(0, 0, 1)]
        .map(axis => { axis.applyQuaternion(inverse); return [Math.round(axis.x * 28), Math.round(-axis.y * 28)]; });
      const key = JSON.stringify(projected);
      if (lastAxes !== key) { lastAxes = key; setAxes(projected); }
      renderer.render(scene, camera);
      publish(now);
      if (reveal.status === 'playing') invalidate();
      else if (reveal.status === 'complete' && pending.length && !manualPause && !constructionFrame) playNext(now);
    }
    function invalidate() {
      if (!frame && !disposed && !document.hidden && surfaceVisible) frame = requestAnimationFrame(draw);
    }
    controls.addEventListener('change', invalidate);

    function applyMode() {
      models.traverse(object => {
        if (object instanceof THREE.Mesh && object.userData.role === 'surface') {
          object.visible = activeMode === 'solid';
          (object.material as THREE.MeshStandardMaterial).emissive.set(object.userData.id === selectedId ? '#143a3a' : '#000000');
        } else if (object instanceof THREE.Mesh && object.userData.role === 'reveal-wire') {
          (object.material as THREE.MeshBasicMaterial).opacity = activeMode === 'wire' ? 0.65 : 0.22;
        } else if (object instanceof THREE.LineSegments) {
          const material = object.material as THREE.LineBasicMaterial;
          material.opacity = activeMode === 'wire' ? 0.7 : object.userData.id === selectedId ? 0.7 : 0.15;
          material.color.set(object.userData.id === selectedId ? '#80e6da' : '#9dcac7');
        }
      });
      invalidate();
    }
    function fit() {
      const target = bounds.isEmpty() ? new THREE.Vector3() : bounds.getCenter(new THREE.Vector3());
      const direction = camera.position.clone().sub(controls.target).normalize();
      if (!direction.lengthSq()) direction.set(1, -1, 0.8).normalize();
      const limitingFov = Math.min(THREE.MathUtils.degToRad(camera.fov), 2 * Math.atan(Math.tan(THREE.MathUtils.degToRad(camera.fov / 2)) * camera.aspect));
      const distance = sceneRadius / Math.sin(limitingFov / 2) * 1.3;
      camera.position.copy(target).addScaledVector(direction, distance);
      controls.target.copy(target);
      camera.near = Math.max(sceneRadius / 10000, 1e-7);
      camera.far = Math.max(sceneRadius * 100000, 1000);
      camera.updateProjectionMatrix();
      controls.update();
      invalidate();
    }
    function view(next: CameraView) {
      const distance = camera.position.distanceTo(controls.target);
      const direction = next === 'top' ? new THREE.Vector3(0, -0.00001, 1) : next === 'front' ? new THREE.Vector3(0, -1, 0.00001) : new THREE.Vector3(1, -1.4, 1);
      camera.position.copy(controls.target).addScaledVector(direction.normalize(), distance);
      camera.up.set(0, 0, 1);
      controls.update();
      invalidate();
    }
    function clearModels(group = models) {
      const geometries = new Set<THREE.BufferGeometry>();
      group.traverse(object => {
        if (object instanceof THREE.Mesh || object instanceof THREE.Line) {
          geometries.add(object.geometry);
          const materials = Array.isArray(object.material) ? object.material : [object.material];
          materials.forEach(material => material.dispose());
        }
      });
      geometries.forEach(geometry => geometry.dispose());
      group.clear();
    }
    function updateConstruction(data: ModelSnapshot['construction'], feature: string | null, plane: string | null) {
      clearModels(guides);
      const radius = Math.max(sceneRadius, 1);
      setVisibleSketch(null);
      if (data?.version === 1 && feature === 'sketch') {
        for (const loop of data.sketchLoops) {
          const points: THREE.Vector3[] = [];
          for (let index = 0; index + 1 < loop.pointsXy.length; index += 2) {
            points.push(new THREE.Vector3(loop.pointsXy[index], loop.pointsXy[index + 1], Math.max(radius * 0.0004, 1e-5)));
          }
          if (points.length > 2) {
            points.push(points[0].clone());
            const geometry = new THREE.BufferGeometry().setFromPoints(points);
            guides.add(new THREE.Line(geometry, new THREE.LineBasicMaterial({ color: loop.hole ? '#d7a888' : '#8de3d2', depthTest: false, transparent: true, opacity: 0.95 })));
          }
        }
      }
      if (data?.version === 2) {
        const selected = data.features.find(item => item.id === feature);
        if (selected?.sketch) {
          const display = sketchDisplayPoints(selected.sketch);
          const points = display.map(point => new THREE.Vector3(...point));
          const geometry = new THREE.BufferGeometry().setFromPoints(points);
          const line = new THREE.Line(geometry, new THREE.LineBasicMaterial({ color: '#8de3d2', depthTest: false, transparent: true, opacity: 1 }));
          line.renderOrder = 10;
          guides.add(line);
          setVisibleSketch({ id: selected.id, z: selected.sketch.offset, points: points.length });
          if (!loadedData?.length) {
            bounds.setFromPoints(points); bounds.getCenter(center);
            sceneRadius = Math.max(bounds.getSize(new THREE.Vector3()).length() / 2, 1);
            fit();
          }
        }
      }
      if (plane) {
        if (plane === 'origin') {
          const point = new THREE.Mesh(new THREE.SphereGeometry(radius * 0.035, 16, 12), new THREE.MeshBasicMaterial({ color: '#fff0a6', toneMapped: false }));
          guides.add(point);
        }
        const axis = plane === 'X' ? { direction: new THREE.Vector3(1, 0, 0), color: '#ff807b' }
          : plane === 'Y' ? { direction: new THREE.Vector3(0, 1, 0), color: '#76e5a9' }
            : plane === 'Z' ? { direction: new THREE.Vector3(0, 0, 1), color: '#7abfff' } : null;
        if (axis) {
          const length = radius * 2.8;
          const arrow = new THREE.ArrowHelper(axis.direction, new THREE.Vector3(), length, axis.color, radius * 0.2, radius * 0.15);
          const negative = new THREE.Line(new THREE.BufferGeometry().setFromPoints([axis.direction.clone().multiplyScalar(-length), new THREE.Vector3()]), new THREE.LineBasicMaterial({ color: axis.color, transparent: true, opacity: 0.68, depthTest: false }));
          guides.add(arrow, negative);
        }
        if (plane === 'XY' || plane === 'XZ' || plane === 'YZ') {
        const size = radius * 2.5;
        const surface = new THREE.Mesh(new THREE.PlaneGeometry(size, size), new THREE.MeshBasicMaterial({ color: plane === 'XY' ? '#75bfc0' : plane === 'XZ' ? '#c89b83' : '#a3acda', transparent: true, opacity: 0.10, side: THREE.DoubleSide, depthWrite: false }));
        const edge = new THREE.LineSegments(new THREE.EdgesGeometry(surface.geometry), new THREE.LineBasicMaterial({ color: plane === 'XY' ? '#75bfc0' : plane === 'XZ' ? '#c89b83' : '#a3acda', transparent: true, opacity: 0.75, depthTest: false }));
        if (plane === 'XZ') { surface.rotation.x = Math.PI / 2; edge.rotation.x = Math.PI / 2; }
        if (plane === 'YZ') { surface.rotation.y = Math.PI / 2; edge.rotation.y = Math.PI / 2; }
        const lift = radius * 0.001;
        if (plane === 'XY') { surface.position.z = lift; edge.position.z = lift; }
        if (plane === 'XZ') { surface.position.y = lift; edge.position.y = lift; }
        if (plane === 'YZ') { surface.position.x = lift; edge.position.x = lift; }
        guides.add(surface, edge);
        }
      }
      invalidate();
    }
    controller.current = {
      replace(data) {
        try { data.forEach(validateMesh); } catch (cause) {
          setError(cause instanceof Error ? cause.message : 'Nem sikerült megjeleníteni a geometriát.');
          return;
        }
        const wasEmpty = models.children.length === 0;
        const previousRadius = sceneRadius;
        const previousCenter = bounds.isEmpty() ? null : bounds.getCenter(new THREE.Vector3());
        const previousIds = new Set(models.children.map(object => object.userData.id));
        const changedModel = data.some(item => !previousIds.has(item.id));
        const replacement = new THREE.Group();
        const nextBounds = new THREE.Box3();
        let triangles = 0;
        let vertices = 0;
        try { data.forEach(item => {
          const geometry = createRenderGeometry(item);
          const material = new THREE.MeshStandardMaterial({ color: item.color || '#a5b8bc', metalness: 0.28, roughness: 0.36, side: THREE.DoubleSide, polygonOffset: true, polygonOffsetFactor: 1, polygonOffsetUnits: 1 });
          const model = new THREE.Mesh(geometry.surface, material);
          model.castShadow = true;
          model.receiveShadow = true;
          model.userData = { id: item.id, name: item.name, role: 'surface', triangles: (item.indices?.length ?? item.positions.length / 3) / 3 };
          const wire = new THREE.LineSegments(geometry.edges, new THREE.LineBasicMaterial({ color: '#9dcac7', transparent: true, opacity: 0.15 }));
          wire.userData = { id: item.id, name: item.name, role: 'feature-wire' };
          // The renderer applies the same drawRange to these actual triangle
          // edges (wireframe's index multiplier is handled internally by Three).
          const revealWire = new THREE.Mesh(geometry.surface, new THREE.MeshBasicMaterial({ color: '#9dcfc7', wireframe: true, transparent: true, opacity: activeMode === 'wire' ? 0.65 : 0.22, depthWrite: false }));
          revealWire.userData = { id: item.id, name: item.name, role: 'reveal-wire' };
          replacement.add(model, wire, revealWire);
          nextBounds.expandByObject(model);
          vertices += item.positions.length / 3;
          triangles += (item.indices?.length ?? item.positions.length / 3) / 3;
        }); } catch (cause) {
          clearModels(replacement);
          setError(cause instanceof Error ? cause.message : 'Nem sikerült megjeleníteni a geometriát.');
          return;
        }
        clearModels();
        if (replacement.children.length) models.add(...replacement.children);
        bounds.copy(nextBounds);
        loadedData = data; setRenderedMeshes(data);
        const size = bounds.isEmpty() ? null : bounds.getSize(new THREE.Vector3());
        sceneRadius = size ? Math.max(size.length() / 2, 0.0001) : 4;
        if (!bounds.isEmpty()) bounds.getCenter(center); else center.set(0, 0, 0);
        sun.target.position.copy(center);
        sun.position.copy(center).add(new THREE.Vector3(-1.5, -1, 2.8).multiplyScalar(sceneRadius));
        const shadowCamera = sun.shadow.camera;
        shadowCamera.left = shadowCamera.bottom = -sceneRadius * 2;
        shadowCamera.right = shadowCamera.top = sceneRadius * 2;
        shadowCamera.near = Math.max(sceneRadius * 0.001, 0.00001);
        shadowCamera.far = sceneRadius * 10;
        shadowCamera.updateProjectionMatrix();
        sun.shadow.normalBias = sceneRadius * 0.004;
        const floor = bounds.isEmpty() ? 0 : Math.min(0, bounds.min.z);
        ground.position.z = floor - sceneRadius * 0.001;
        gridPlane.position.z = floor - sceneRadius * 0.0005;
        setStats({ objects: data.length, vertices, triangles, dimensions: size ? [size.x, size.y, size.z] : null });
        if (selectedId && !data.some(item => item.id === selectedId)) { selectedId = null; setSelected(null); }
        setError('');
        applyMode();
        const changedBounds = sceneRadius > previousRadius * 2 || sceneRadius < previousRadius / 2 || (previousCenter && previousCenter.distanceTo(center) > sceneRadius);
        if (data.length && (wasEmpty || changedModel || changedBounds)) fit();
      },
      sync(data, incoming, runGeneration, failed, isBusy) {
        const now = performance.now();
        latestData = data; backendBusy = isBusy; blocked = failed;
        if (runGeneration !== activeGeneration || failed) {
          activeGeneration = runGeneration;
          pending = []; history = data === confirmedData ? [...confirmedHistory] : [];
          seen = new Set(history.map(stage => stage.key)); currentStage = history.at(-1) ?? null;
          setPlaybackFeatureId(currentStage?.featureId ?? null); setVisibleSketch(null); clearModels(guides);
          manualPause = false; autoPause = false; skipRemaining = failed; forceConstructionMotion = false;
          if (loadedData !== data) controller.current?.replace(data);
          timeline.start(triangleCount(data), now, 0, false);
        }
        if (failed) { publish(now, true); invalidate(); return; }
        if (incoming.length) {
          try { history = expandCadStages(incoming.slice(0, 8)); }
          catch (cause) { setError(cause instanceof Error ? cause.message : 'Érvénytelen CAD modellfa.'); return; }
          const keys = new Set(history.map(stage => stage.key));
          pending = pending.filter(stage => keys.has(stage.key));
          for (const stage of history) if (!seen.has(stage.key)) { seen.add(stage.key); pending.push(stage); }
          if ((reduced && !forceConstructionMotion) || skipRemaining) showLatest();
          else if (timeline.read(now).status === 'complete' && pending.length && !manualPause && !constructionFrame) playNext(now);
        } else if (loadedData !== data) {
          controller.current?.replace(data);
          timeline.start(triangleCount(data), now, 0, false);
        }
        if (!isBusy && history.length && history.at(-1)?.meshes === data) {
          confirmedHistory = [...history]; confirmedData = data;
        }
        publish(now, true); invalidate();
      },
      replay() {
        if (blocked || !latestData.length) return;
        if (!history.length) history = [fallback()];
        forceConstructionMotion = true;
        pending = [...history]; manualPause = false; autoPause = false; skipRemaining = false;
        playNext();
      },
      togglePlayback() {
        const now = performance.now();
        if (manualPause || timeline.read(now).status === 'paused') {
          manualPause = false; autoPause = false;
          if (timeline.read(now).status === 'complete' && pending.length) playNext(now);
          else timeline.resume(now);
          updateVisibility();
        } else { timeline.pause(now); manualPause = true; }
        publish(now, true); invalidate();
      },
      skip() { skipRemaining = true; showLatest(); },
      view, fit,
      mode(next) { activeMode = next; applyMode(); },
      grid(visible) { gridPlane.visible = visible; invalidate(); },
      selectCadFeature(id) {
        const stage = history.find(item => item.featureId === id);
        if (!stage) return;
        pending = []; manualPause = false; autoPause = false; skipRemaining = true;
        showStage(stage, false);
        // A legacy combined profile/extrusion may expose its analytic profile too.
        updateConstruction(stage.construction, id, null);
      },
      selectCadReference(reference) {
        pending = []; manualPause = false; autoPause = false; skipRemaining = true;
        timeline.skip();
        updateConstruction(undefined, null, reference);
        publish(performance.now(), true);
      },
      construction: updateConstruction,
    };
    let measured = false;
    const observer = new ResizeObserver(() => {
      if (disposed) return;
      const { width, height } = container.getBoundingClientRect();
      surfaceVisible = width > 0 && height > 0;
      updateVisibility();
      if (!width || !height) return;
      camera.aspect = width / height;
      camera.updateProjectionMatrix();
      renderer.setSize(width, height);
      if (!measured && !bounds.isEmpty()) fit();
      measured = true;
      invalidate();
    });
    observer.observe(container);
    const raycaster = new THREE.Raycaster();
    let pointerStart = { x: 0, y: 0 };
    function pointerDown(event: PointerEvent) { pointerStart = { x: event.clientX, y: event.clientY }; }
    function pointerUp(event: PointerEvent) {
      if (event.button !== 0 || Math.hypot(event.clientX - pointerStart.x, event.clientY - pointerStart.y) > 4) return;
      const rect = renderer.domElement.getBoundingClientRect();
      raycaster.setFromCamera(new THREE.Vector2(((event.clientX - rect.left) / rect.width) * 2 - 1, -((event.clientY - rect.top) / rect.height) * 2 + 1), camera);
      const hit = raycaster.intersectObjects(models.children.filter(object => object.userData.role === 'surface'))[0];
      selectedId = hit?.object.userData.id ?? null;
      setSelected(selectedId);
      applyMode();
    }
    function keydown(event: KeyboardEvent) {
      if (event.ctrlKey || event.metaKey || event.altKey) return;
      if (event.key.toLowerCase() === 'f') { event.preventDefault(); fit(); }
      const views: Record<string, CameraView> = { '1': 'isometric', '2': 'top', '3': 'front' };
      if (views[event.key]) { event.preventDefault(); view(views[event.key]); }
      if (event.key.startsWith('Arrow')) {
        event.preventDefault();
        const offset = camera.position.clone().sub(controls.target);
        if (event.shiftKey) {
          const right = new THREE.Vector3(1, 0, 0).applyQuaternion(camera.quaternion);
          const up = new THREE.Vector3(0, 1, 0).applyQuaternion(camera.quaternion);
          const movement = new THREE.Vector3().addScaledVector(right, event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0).addScaledVector(up, event.key === 'ArrowUp' ? 1 : event.key === 'ArrowDown' ? -1 : 0).multiplyScalar(offset.length() * 0.04);
          camera.position.add(movement); controls.target.add(movement);
        } else {
          const spherical = new THREE.Spherical().setFromVector3(new THREE.Vector3(offset.x, offset.z, -offset.y));
          spherical.theta += event.key === 'ArrowLeft' ? -0.1 : event.key === 'ArrowRight' ? 0.1 : 0;
          spherical.phi += event.key === 'ArrowUp' ? -0.1 : event.key === 'ArrowDown' ? 0.1 : 0;
          spherical.makeSafe();
          const result = new THREE.Vector3().setFromSpherical(spherical);
          camera.position.copy(controls.target).add(new THREE.Vector3(result.x, -result.z, result.y));
        }
        controls.update(); invalidate();
      }
      if (event.key === '+' || event.key === '-' || event.key === '=') {
        event.preventDefault();
        camera.position.sub(controls.target).multiplyScalar(event.key === '-' ? 1.15 : 1 / 1.15).add(controls.target);
        controls.update(); invalidate();
      }
    }
    function contextLost(event: Event) {
      event.preventDefault();
      setError('A grafikus kapcsolat megszakadt. A böngésző megpróbálja helyreállítani.');
    }
    function contextRestored() { setError(''); invalidate(); }
    function motionChanged() {
      reduced = motion.matches;
      if (reduced && !forceConstructionMotion) { skipRemaining = true; showLatest(); }
      publish(performance.now(), true);
    }
    motion.addEventListener('change', motionChanged);
    document.addEventListener('visibilitychange', updateVisibility);
    renderer.domElement.addEventListener('pointerdown', pointerDown);
    renderer.domElement.addEventListener('pointerup', pointerUp);
    renderer.domElement.addEventListener('keydown', keydown);
    renderer.domElement.addEventListener('webglcontextlost', contextLost);
    renderer.domElement.addEventListener('webglcontextrestored', contextRestored);
    invalidate();
    return () => {
      disposed = true;
      timeline.skip(); pending = []; history = [];
      if (constructionFrame) cancelAnimationFrame(constructionFrame);
      cancelAnimationFrame(frame);
      motion.removeEventListener('change', motionChanged);
      document.removeEventListener('visibilitychange', updateVisibility);
      observer.disconnect();
      controls.removeEventListener('change', invalidate);
      controls.dispose();
      renderer.domElement.removeEventListener('pointerdown', pointerDown);
      renderer.domElement.removeEventListener('pointerup', pointerUp);
      renderer.domElement.removeEventListener('keydown', keydown);
      renderer.domElement.removeEventListener('webglcontextlost', contextLost);
      renderer.domElement.removeEventListener('webglcontextrestored', contextRestored);
      clearModels();
      clearModels(constructionPreview);
      ground.geometry.dispose(); (ground.material as THREE.Material).dispose();
      gridPlane.geometry.dispose(); gridMaterial.dispose();
      sun.shadow.dispose();
      renderer.dispose();
      renderer.forceContextLoss();
      renderer.domElement.remove();
      controller.current = null;
    };
  }, []);

  useEffect(() => { controller.current?.sync(meshes, stages, generation, Boolean(notice), busy); }, [meshes, stages, generation, notice, busy]);
  useEffect(() => { if (construction?.version !== 2) controller.current?.construction(construction, selectedFeature, selectedPlane); }, [construction, selectedFeature, selectedPlane]);
  const changeMode = (next: DisplayMode) => { setMode(next); controller.current?.mode(next); };
  const toggleGrid = () => { setGrid(!grid); controller.current?.grid(!grid); };
  const selectedMesh = renderedMeshes.find(mesh => mesh.id === selected);

  return (
    <section className="viewport" aria-label="Modell és 3D munkatér">
      <div className="viewport-canvas" ref={host} />
      <div className="viewport-top">
        <div className="scene-label"><span className="eyebrow">MUNKATÉR</span><span>{error ? 'Megjelenítési hiba' : stats.objects ? preview ? 'Előzetes modell · véglegesítésre vár' : 'Elkészült modell' : 'Új jelenet'}</span></div>
        <div className="viewport-top-actions">
          {stats.objects > 0 && <button className="icon-button glass" onClick={onCharacterDemo} title="Térbeli figura · stilizált Pikachu demó" aria-label="Térbeli figura újraépítése" disabled={busy || !characterConnected}><Box size={17} /></button>}
          <button className="icon-button glass" onClick={onImport} title="Kép importálása" aria-label="Kép importálása" disabled={busy}><Upload size={17} /></button>
          <button className="icon-button glass" onClick={() => setHelp(!help)} aria-expanded={help} aria-label="Navigációs súgó" title="Navigációs súgó"><MousePointer2 size={17} /></button>
        </div>
      </div>
      <div className="view-tools glass" role="toolbar" aria-label="Nézetbeállítások">
        <button className="icon-button" onClick={() => controller.current?.fit()} title="Modell illesztése (F)" aria-label="Modell illesztése"><Expand size={18} /></button>
        <span className="tool-divider" />
        <button className="view-button" onClick={() => controller.current?.view('isometric')} title="Izometrikus nézet (1)">ISO</button>
        <button className="view-button" onClick={() => controller.current?.view('top')} title="Felülnézet (2)">FELÜL</button>
        <button className="view-button" onClick={() => controller.current?.view('front')} title="Elölnézet (3)">ELÖL</button>
        <span className="tool-divider" />
        <button className={`icon-button ${mode === 'solid' ? 'is-active' : ''}`} onClick={() => changeMode('solid')} title="Felületek" aria-label="Felületek" aria-pressed={mode === 'solid'}><Box size={18} /></button>
        <button className={`icon-button ${mode === 'wire' ? 'is-active' : ''}`} onClick={() => changeMode('wire')} title="Drótváz" aria-label="Drótváz" aria-pressed={mode === 'wire'}><Layers2 size={18} /></button>
        <button className={`icon-button ${grid ? 'is-active' : ''}`} onClick={toggleGrid} title="Talajrács" aria-label="Talajrács" aria-pressed={grid}><Grid2X2 size={18} /></button>
      </div>
      {!stats.objects && !error && !cadConstruction && (
        <div className="stage-empty">
          <div className="stage-empty-icon"><Box size={25} strokeWidth={1.2} /></div>
          <h1>Szövegből a térig.</h1>
          <p>Írd le az alkatrészt,<br />és építsd fel lépésenként.</p>
          <div className="stage-empty-actions">
            <button className="button primary" onClick={onDemo} disabled={busy || !connected}>{busy ? <LoaderCircle size={15} className="spin" /> : <Box size={15} />}Demó betöltése</button>
            <button className="button secondary" onClick={onComplexDemo} disabled={busy || !connected}><Layers2 size={15} />Összetett demó</button>
            <button className="button secondary" onClick={onCharacterDemo} disabled={busy || !characterConnected} title="Rögzített paraméteres Pikachu-demó · mm · AI nélkül"><Box size={15} />Térbeli figura</button>
            <button className="button secondary" onClick={onImport} disabled={busy || !connected}><Upload size={15} />Importálás</button>
          </div>
          <span className="stage-empty-note">Valós geometria. Közvetlen visszajelzés.</span>
        </div>
      )}
      {error && <div className="viewport-error" role="alert"><Triangle size={19} /><p>{error}</p></div>}
      {notice && <div className="scene-notice glass" role="status"><Triangle size={14} /><p>{notice}</p></div>}
      {busy && <div className={`scene-working glass ${playback.available ? 'above-playback' : ''}`} role="status"><LoaderCircle size={14} className="spin" />Művelet folyamatban…</div>}
      {imageConstruction && construction && <div className={`construction-tree glass ${constructionPlayback.phase === 'extruding' ? 'is-building' : ''}`} role="tree" aria-label="B-rep műveleti fa">
        <div className="construction-title"><Layers2 size={13} /><span>MODELLFA</span><span className="construction-units">{units === 'egység' ? 'saját egység' : units}</span><button className="construction-replay" onClick={() => controller.current?.replay()} title="A modellépítés újrajátszása" aria-label="Modellépítés újrajátszása"><RotateCcw size={12} /></button></div>
        {constructionPlayback.phase !== 'idle' && <div className="construction-progress" role="progressbar" aria-label="Kihúzás előrehaladása" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(constructionPlayback.progress * 100)}>
          <div className="construction-progress-heading"><span>{constructionPlayback.phase === 'sketch' ? 'Vázlat kontúrja' : constructionPlayback.phase === 'extruding' ? 'Kihúzás · +Z' : 'Natív háló kész'}</span><span>{Math.round(constructionPlayback.progress * 100)}%</span></div>
          <div className="construction-progress-track"><span style={{ width: `${constructionPlayback.progress * 100}%` }} /></div>
        </div>}
        <details className="tree-folder" open={originExpanded} onToggle={event => setOriginExpanded(event.currentTarget.open)}>
          <summary className={`tree-branch tree-disclosure ${originExpanded ? '' : 'is-collapsed'}`} role="treeitem" aria-expanded={originExpanded}><ChevronDown size={12} /><span>Origó</span></summary>
          <div className="tree-folder-children" role="group">
            {(['X', 'Y', 'Z'] as const).map(axis => <button key={axis} className={`tree-item tree-reference ${selectedPlane === axis ? 'is-selected' : ''}`} role="treeitem" aria-selected={selectedPlane === axis} title={`${axis} Axis`} onClick={() => { setSelectedPlane(selectedPlane === axis ? null : axis); setSelectedFeature(null); }}><span className={`tree-glyph axis-glyph axis-${axis.toLowerCase()}`} /><span>{axis} tengely</span></button>)}
            <button className={`tree-item tree-reference ${selectedPlane === 'origin' ? 'is-selected' : ''}`} role="treeitem" aria-selected={selectedPlane === 'origin'} title="Origin Point" onClick={() => { setSelectedPlane(selectedPlane === 'origin' ? null : 'origin'); setSelectedFeature(null); }}><span className="tree-glyph origin-glyph" /><span>Origópont · (0, 0, 0)</span></button>
            {(['XY', 'XZ', 'YZ'] as const).map(plane => <button key={plane} className={`tree-item tree-plane ${selectedPlane === plane ? 'is-selected' : ''}`} role="treeitem" aria-selected={selectedPlane === plane} title={`${plane} Plane`} onClick={() => { setSelectedPlane(selectedPlane === plane ? null : plane); setSelectedFeature(null); }}><span className="tree-glyph plane-glyph" /><span>{plane} sík{plane === 'XY' ? ' · z = 0' : ''}</span></button>)}
          </div>
        </details>
        <details className="tree-folder steps-folder" open={stepsExpanded} onToggle={event => setStepsExpanded(event.currentTarget.open)}>
          <summary className={`tree-branch tree-disclosure ${stepsExpanded ? '' : 'is-collapsed'}`} role="treeitem" aria-expanded={stepsExpanded}><ChevronDown size={12} /><span>Lépések</span></summary>
          <div className="tree-folder-children" role="group">
            <details className="tree-feature-group" open={extrudeExpanded} onToggle={event => setExtrudeExpanded(event.currentTarget.open)}>
              <summary className={`tree-item tree-disclosure ${selectedFeature === 'extrude' ? 'is-selected' : ''} ${constructionPlayback.phase === 'extruding' ? 'is-building-step' : ''} ${extrudeExpanded ? '' : 'is-collapsed'}`} role="treeitem" aria-selected={selectedFeature === 'extrude'} aria-expanded={extrudeExpanded} onClick={() => { setSelectedFeature('extrude'); setSelectedPlane(null); }}><ChevronDown size={11} /><span className="tree-glyph extrude-glyph" /><span>Kihúzás1 · +Z {constructionPlayback.phase === 'extruding' ? format(construction.extrusionDistance * constructionPlayback.progress) : format(construction.extrusionDistance)} / {format(construction.extrusionDistance)} {units}</span></summary>
              <div className="tree-feature-child" role="group">
                <details className="tree-sketch-group" open={sketchExpanded} onToggle={event => setSketchExpanded(event.currentTarget.open)}>
                  <summary className={`tree-item tree-disclosure ${selectedFeature === 'sketch' ? 'is-selected' : ''} ${constructionPlayback.phase === 'sketch' ? 'is-building-step' : ''} ${sketchExpanded ? '' : 'is-collapsed'}`} role="treeitem" aria-selected={selectedFeature === 'sketch'} aria-expanded={sketchExpanded} onClick={() => { setSelectedFeature('sketch'); setSelectedPlane(null); }}><ChevronDown size={10} /><span className="tree-glyph sketch-glyph" /><span>Vázlat1 · {construction.sketchPlane}, z = {format(construction.sketchOffset)}</span></summary>
                  <div className="tree-sketch-details" role="group">{construction.sketchLoops.map((loop, index) => <div className="tree-detail" key={`loop-${index}`}>{loop.hole ? 'Furat' : 'Külső kontúr'} · {loop.pointsXy.length / 2} pont</div>)}</div>
                </details>
              </div>
            </details>
          </div>
        </details>
        <div className="tree-origin-note">Forrásközép → XY-origó: ({format(construction.sourceCenter[0])}, {format(construction.sourceCenter[1])}) {units}<br />Modellorigó: (0, 0, 0) · sík: z = 0</div>
      </div>}
      {cadConstruction && <CadFeatureTree construction={cadConstruction} selected={selectedFeature} active={playbackFeatureId} playing={playback.status === 'playing'} selectedReference={selectedPlane} onReference={reference => { setSelectedPlane(reference); setSelectedFeature(null); controller.current?.selectCadReference(reference); }} onSelect={id => { setSelectedFeature(id); setSelectedPlane(null); controller.current?.selectCadFeature(id); }} onReplay={() => controller.current?.replay()} />}
      {visibleSketch && <div className="cad-sketch-notice glass" role="status" aria-label="Látható vázlatprofil">Profil: {visibleSketch.id} · XY, Z = {format(visibleSketch.z)} mm · {visibleSketch.points} megjelenítési pont · csak görbék</div>}
      {!construction && cadFeatureStages.length > 0 && <div className={`construction-tree generic-feature-tree glass ${busy ? 'is-building' : ''}`} role="tree" aria-label="CAD modellfa">
        <div className="construction-title"><Layers2 size={13} /><span>MODELLFA</span><span className="construction-units">mm · jobbkezes UCS</span></div>
        <details className="tree-folder generic-tree-root" open={originExpanded} onToggle={event => setOriginExpanded(event.currentTarget.open)}>
          <summary className={`tree-branch tree-disclosure ${originExpanded ? '' : 'is-collapsed'}`} role="treeitem" aria-expanded={originExpanded}><ChevronDown size={12} /><span>Origó · (0, 0, 0)</span></summary>
          <div className="tree-folder-children" role="group" aria-label="Origótengelyek">{['X tengely', 'Y tengely', 'Z tengely · felfelé'].map((label, index) => <div className="tree-item tree-reference" role="treeitem" key={label}><span className={`tree-glyph axis-glyph axis-${['x', 'y', 'z'][index]}`} /><span>{label}</span></div>)}</div>
        </details>
        <details className="tree-folder steps-folder" open={stepsExpanded} onToggle={event => setStepsExpanded(event.currentTarget.open)}>
          <summary className={`tree-branch tree-disclosure ${stepsExpanded ? '' : 'is-collapsed'}`} role="treeitem" aria-expanded={stepsExpanded}><ChevronDown size={12} /><span>Lépések</span><span className="construction-units">{cadFeatureStages.length}</span></summary>
          <div className="tree-folder-children" role="group">{cadFeatureStages.map((stage, index) => <div className={`tree-item generic-feature-row ${busy && index === cadFeatureStages.length - 1 ? 'is-building-step' : ''}`} role="treeitem" key={stage.key}><span className="tree-glyph extrude-glyph" /><span>{stage.name.replace(/^\d+\/\d+ · /u, '')}</span></div>)}</div>
        </details>
      </div>}
      {playback.available && !imageConstruction && <div className={`draw-playback glass ${cadConstruction || cadFeatureStages.length ? 'cad-playback' : ''}`} role="group" aria-label="Kirajzolás vezérlése">
        <div className="draw-heading"><span>{playback.status === 'paused' ? 'Kirajzolás szünetel' : playback.status === 'playing' ? 'Kirajzolás' : 'Kirajzolás kész'}</span><span className="draw-count" role="progressbar" aria-label="Kirajzolt háromszögek" aria-valuemin={0} aria-valuemax={playback.total} aria-valuenow={playback.shown}>{playback.sketch ? 'Vázlatprofil · görbék' : `${format(playback.shown)} / ${format(playback.total)} háromszög`}</span></div>
        <div className="draw-stage" title={playback.label}>{playback.count > 1 && `${playback.index}/${playback.count} · `}{playback.label}</div>
        <div className="draw-controls"><button className="replay-button" onClick={() => controller.current?.replay()} disabled={playback.reduced} title={playback.reduced ? 'A csökkentett mozgás be van kapcsolva' : 'Az érkezett geometriai lépések újrajátszása'}><RotateCcw size={12} />Kirajzolás újra</button><span />{playback.status !== 'complete' && <button className="icon-button" onClick={() => controller.current?.togglePlayback()} aria-label={playback.status === 'paused' ? 'Kirajzolás folytatása' : 'Kirajzolás szüneteltetése'} title={playback.status === 'paused' ? 'Folytatás' : 'Szünet'}>{playback.status === 'paused' ? <Play size={13} /> : <Pause size={13} />}</button>}<button className="icon-button" onClick={() => controller.current?.skip()} disabled={playback.status === 'complete' && playback.index === playback.count} aria-label="Kirajzolás átugrása: legfrissebb teljes háló" title="Legfrissebb teljes háló"><SkipForward size={14} /></button></div>
        <div className="draw-track" aria-hidden="true"><span style={{ width: `${playback.total ? 100 * playback.shown / playback.total : 100}%` }} /></div>
      </div>}
      {selectedMesh && <div className="selection-info glass"><Box size={14} /><span>{selectedMesh.name}</span><span className="muted">{format(selectedMesh.positions.length / 3)} csúcs</span></div>}
      {help && <aside className="navigation-help glass"><div><strong>Navigáció</strong><button className="icon-button" onClick={() => setHelp(false)} aria-label="Súgó bezárása"><X size={14} /></button></div><p>Bal gomb · forgatás<br />Jobb gomb / Shift + húzás · mozgatás<br />Görgetés / csippentés · nagyítás</p><p><kbd>F</kbd> illesztés <kbd>1</kbd> ISO <kbd>2</kbd> felül <kbd>3</kbd> elöl</p><p className="muted">Fókuszált nézetben: nyilak · forgatás, Shift + nyilak · mozgatás, + / − · nagyítás</p></aside>}
      <div className="axis-gizmo" aria-label="Koordinátatengelyek: X, Y, Z felfelé">
        <svg width="92" height="92" viewBox="0 0 92 92" aria-hidden="true">{axes.map(([x, y], i) => <g key={i} className={`axis-${i}`}><line x1="46" y1="46" x2={46 + x} y2={46 + y} /><circle cx={46 + x} cy={46 + y} r="8" /><text x={46 + x} y={46 + y + 3}>{['X', 'Y', 'Z'][i]}</text></g>)}<circle cx="46" cy="46" r="2.5" fill="#b1c4ca" /></svg>
      </div>
      <footer className="viewport-status">
        <div><Rotate3D size={13} /><span>Z ↑</span><span className="status-divider" /><span>XY · {format(spacing)} {units}</span></div>
        <div className="geometry-stats"><span>{format(stats.objects)} test</span><span>{format(stats.triangles)} háromszög</span>{stats.dimensions && <span className="dimensions">{stats.dimensions.map(format).join(' × ')} {units}</span>}</div>
      </footer>
    </section>
  );
}
