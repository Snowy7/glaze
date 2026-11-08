import type { Align as WitAlign, TextRole, Tone as WitTone } from './generated/interfaces/glaze-plugin-types.js';
import type { Face as WitFace, Node, NodeKind } from './generated/interfaces/glaze-plugin-view.js';

export type Role = TextRole;
export const Role = {
  LargeTitle: 'large-title', Title1: 'title1', Title2: 'title2', Title3: 'title3',
  Headline: 'headline', Body: 'body', Callout: 'callout', Subheadline: 'subheadline',
  Footnote: 'footnote', Caption: 'caption',
} as const satisfies Record<string, Role>;

export type Tone = WitTone;
export const Tone = {
  Label: { tag: 'label' }, Secondary: { tag: 'secondary' }, Tertiary: { tag: 'tertiary' },
  Accent: { tag: 'accent' }, Warning: { tag: 'warning' }, Destructive: { tag: 'destructive' },
  Positive: { tag: 'positive' }, Hex(value: string): Tone { return { tag: 'hex', val: value }; },
} as const;
export type Align = WitAlign;
export const Align = { Start: 'start', Center: 'center', End: 'end' } as const;

/** A kit tree. These builders have the same arguments/defaults as Rust's View builders.
 * Use the constructor with a generated NodeKind for the remaining Rust enum variants. */
export class View {
  kind: NodeKind;
  children: readonly View[];
  constructor(kind: NodeKind, children: readonly View[] = []) {
    this.kind = kind;
    this.children = children;
  }

  static text(text: string, role: Role): View {
    return new View({ tag: 'text', val: { text, role, color: undefined, align: 'start' } });
  }
  static tinted(text: string, role: Role, color: Tone): View {
    return new View({ tag: 'text', val: { text, role, color, align: 'start' } });
  }
  static stack(children: Iterable<View>): View {
    return new View({ tag: 'stack', val: undefined }, Array.from(children));
  }
  static row(children: Iterable<View>): View {
    return new View({ tag: 'row', val: undefined }, Array.from(children));
  }
  static empty(icon: string, title: string, detail: string): View {
    return new View({ tag: 'empty', val: { icon, title, detail } });
  }
  static button(label: string, action: string): View {
    return new View({ tag: 'button', val: { label, action, icon: undefined, primary: false } });
  }
  face(): Face {
    return new Face(this);
  }
}

/** The shell adds the header and chrome. Mutate footer/refresh just like Rust's Face. */
export class Face {
  footer: string | undefined;
  refresh = false;
  body: View;
  constructor(body: View) { this.body = body; }
}

/** WIT uses pre-order nodes and DIRECT child counts, rather than subtree sizes. */
export function flatten(face: Face): WitFace {
  const nodes: Node[] = [];
  const pending = [face.body];
  while (pending.length > 0) {
    if (nodes.length === 4096) throw new RangeError('A face may contain at most 4096 nodes');
    const view = pending.pop()!;
    nodes.push({ kind: view.kind, children: view.children.length });
    for (let index = view.children.length - 1; index >= 0; index--) pending.push(view.children[index]!);
  }
  return { nodes, footer: face.footer, refresh: face.refresh };
}
