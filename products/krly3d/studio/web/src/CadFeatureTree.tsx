import { useState } from 'react';
import { domIds, featureId, type DomNode } from '../../ssot/domIds';
import { ChevronDown, Layers2, RotateCcw } from 'lucide-react';
import type { CadFeature, Construction } from '../../gen/krly/studio/v1/studio_pb.ts';
import { arcSweep, featureLabels } from './cadConstruction';
const number = new Intl.NumberFormat('hu-HU', { maximumFractionDigits: 3 });
const fmt = (value: number) => number.format(value);
const names: Record<string, string> = { size: 'Méret', center: 'Középpont', radius: 'Sugár', height: 'Magasság', depth: 'Mélység', z: 'Síkeltolás', patternRadius: 'Osztókör sugara', count: 'Darabszám', startAngle: 'Kezdőszög' };
function FeatureDetails({ feature, domNode }: { feature: CadFeature; domNode: DomNode }) {
  let from = feature.sketch?.start ?? [];
  return <div {...domNode.attrs} className="tree-feature-details" role="group" aria-label={`${feature.title} részletei`}>
    {feature.inputs.length > 0 && <div className="tree-detail">Előzmény: {feature.inputs.join(' → ')}</div>}
    {feature.parameters.map(parameter => <div className="tree-detail" key={parameter.name}>{names[parameter.name] ?? parameter.name}: {parameter.text === 'through_all' ? 'Átmenő · teljes testen' : parameter.text || parameter.values.map(fmt).join(' × ')} {parameter.unit === 'deg' ? '°' : parameter.name === 'count' && parameter.unit === '' ? 'db' : parameter.unit}</div>)}
    {['extrude', 'extrude_profile', 'pocket'].includes(feature.kind) && <div className="tree-detail">Irány: {feature.kind === 'pocket' ? '−Z' : '+Z'}</div>}
    {feature.selections.length > 0 && <div className="tree-detail">Élkijelölés: {feature.selections.join(', ')}</div>}
    {feature.sketch && <>
      <div className="tree-detail">XY vázlatsík · Z = {fmt(feature.sketch.offset)} mm</div>
      <div className="tree-detail">Kezdőpont: ({feature.sketch.start.map(fmt).join(', ')}) mm</div>
      {feature.sketch.curves.map((curve, index) => {
        const start = from; from = curve.to;
        return <div className="tree-detail curve-detail" key={index}>{index + 1}. {curve.kind === 'arc' ? `Ív · R ${fmt(Math.hypot(start[0] - curve.center[0], start[1] - curve.center[1]))} mm · ${fmt(Math.abs(arcSweep(start, curve.to, curve.center, curve.clockwise)) * 180 / Math.PI)}° · ${curve.clockwise ? 'óramutató szerint' : 'óramutatóval ellentétes'}` : `Szakasz · ${fmt(Math.hypot(curve.to[0] - start[0], curve.to[1] - start[1]))} mm`}<br />({start.map(fmt).join(', ')}) → ({curve.to.map(fmt).join(', ')}) mm{curve.kind === 'arc' && <><br />Középpont: ({curve.center.map(fmt).join(', ')}) mm</>}</div>;
      })}
      {(from[0] !== feature.sketch.start[0] || from[1] !== feature.sketch.start[1]) && <div className="tree-detail">Záró szakasz · {fmt(Math.hypot(from[0] - feature.sketch.start[0], from[1] - feature.sketch.start[1]))} mm · vissza a kezdőpontba</div>}
      <div className="tree-detail">Analitikus görbeadatok · az ívek képernyős rajza közelítés.</div>
    </>}
  </div>;
}
export function CadFeatureTree({ construction, selected, active, playing, selectedReference, onReference, onSelect, onReplay }: { construction: Construction; selected: string | null; active: string | null; playing: boolean; selectedReference: string | null; onReference: (reference: string | null) => void; onSelect: (id: string) => void; onReplay: () => void }) {
  const [originExpanded, setOriginExpanded] = useState(true);
  const [stepsExpanded, setStepsExpanded] = useState(true);
  return <div {...domIds.studio.modelTree.attrs} className="construction-tree generic-feature-tree glass" role="tree" aria-label="CAD modellfa">
    <div className="construction-title"><Layers2 size={13} /><span>MODELLFA</span><span className="construction-units">mm · Z ↑</span><button className="construction-replay" onClick={onReplay} aria-label="Modellépítés újrajátszása"><RotateCcw size={12} /></button></div>
    <details className="tree-folder" open={originExpanded} onToggle={event => setOriginExpanded(event.currentTarget.open)}>
      <summary className={`tree-branch tree-disclosure ${originExpanded ? '' : 'is-collapsed'}`} role="treeitem" aria-expanded={originExpanded}><ChevronDown size={12} /><span>Origó</span></summary>
      <div className="tree-folder-children" role="group" aria-label="Origó referenciái">
        {(['X', 'Y', 'Z', 'origin', 'XY', 'XZ', 'YZ'] as const).map(reference => <button type="button" key={reference} className={`tree-item tree-reference ${selectedReference === reference ? 'is-selected' : ''}`} role="treeitem" aria-selected={selectedReference === reference} onClick={() => onReference(selectedReference === reference ? null : reference)}>
          <span className={`tree-glyph ${reference === 'origin' ? 'origin-glyph' : reference.length === 1 ? `axis-glyph axis-${reference.toLowerCase()}` : 'plane-glyph'}`} /><span>{reference === 'origin' ? 'Origópont · (0, 0, 0)' : reference.length === 1 ? `${reference} tengely` : `${reference} sík${reference === 'XY' ? ' · z = 0' : ''}`}</span>
        </button>)}
      </div>
    </details>
    <details className="tree-folder steps-folder" open={stepsExpanded} onToggle={event => setStepsExpanded(event.currentTarget.open)}>
      <summary className={`tree-branch tree-disclosure ${stepsExpanded ? '' : 'is-collapsed'}`} role="treeitem" aria-expanded={stepsExpanded}><ChevronDown size={12} /><span>Lépések</span></summary>
      <div className="tree-folder-children" role="group" aria-label="CAD lépések">
    {construction.features.map((feature, index) => {
      const featureDom = domIds.studio.modelTree.feature.key(featureId(feature.id));
      return <div {...featureDom.attrs} key={feature.id}>
      <button {...featureDom.select.attrs} type="button" role="treeitem" aria-selected={selected === feature.id} aria-current={active === feature.id ? 'step' : undefined} aria-expanded={selected === feature.id} className={`tree-item generic-feature-row ${selected === feature.id ? 'is-selected' : ''} ${playing && active === feature.id ? 'is-building-step' : ''}`} onClick={() => onSelect(feature.id)}>
        <span className={`tree-glyph ${feature.kind === 'sketch' ? 'sketch-glyph' : 'extrude-glyph'}`} /><span>{index + 1}. {featureLabels[feature.kind] ?? feature.kind} · {feature.title}<small>{feature.id}</small></span>
      </button>
      {selected === feature.id && <FeatureDetails feature={feature} domNode={featureDom.details} />}
    </div>; })}
      </div>
    </details>
  </div>;
}
