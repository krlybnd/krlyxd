import { domIds, featureId } from './domIds';
const feature = domIds.studio.modelTree.feature.key(featureId('outline'));
void feature.select.attrs;
// @ts-expect-error A typo is not a DOM point.
void feature.selet;
// @ts-expect-error Only validated API identifiers are instance keys.
domIds.studio.modelTree.feature.key('outline');
// @ts-expect-error A feature child is not a child of the model tree.
void domIds.studio.modelTree.select;
