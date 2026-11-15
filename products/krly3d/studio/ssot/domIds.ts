/** Canonical, type-derived DOM contract. Names and paths come only from these keys. */
export const domScheme = {
  studio: {
    modelTree: { feature: { select: {}, details: {} } },
    typedCad: { session: {}, state: { changes: {} }, source: {} },
  },
} as const;

declare const featureIdBrand: unique symbol;
export type FeatureId = string & { readonly [featureIdBrand]: true };
/** Validate the API identifier before it can become a repeated DOM instance key. */
export function featureId(value: string): FeatureId {
  if (!/^[a-z][a-z0-9_-]{0,31}$/.test(value)) throw new Error(`Invalid CAD feature identifier: ${value}`);
  return value as FeatureId;
}
export interface DomScope {
  readonly id: string;
  readonly key?: string;
}
export interface DomNode {
  readonly scopes: readonly DomScope[];
  readonly attrs: { readonly 'data-domid': string; readonly 'data-domkey'?: string };
}
type Children<S> = { readonly [K in keyof S]: K extends 'feature'
  ? { readonly key: (key: FeatureId) => Tree<S[K]> }
  : Tree<S[K]> };
export type Tree<S> = DomNode & Children<S>;

function derive<S extends object>(scheme: S, parents: readonly DomScope[] = []): Children<S> {
  return Object.fromEntries(Object.entries(scheme).map(([name, children]) => {
    const id = [...parents.map(scope => scope.id.split('.').at(-1)), name].join('.');
    const node = (key?: string) => {
      const scopes = [...parents, { id, ...(key === undefined ? {} : { key }) }];
      return { scopes, attrs: { 'data-domid': id, ...(key === undefined ? {} : { 'data-domkey': key }) }, ...derive(children, scopes) };
    };
    return [name, name === 'feature' ? { key: (key: FeatureId) => node(key) } : node()];
  })) as unknown as Children<S>;
}
export const domIds = derive(domScheme);
