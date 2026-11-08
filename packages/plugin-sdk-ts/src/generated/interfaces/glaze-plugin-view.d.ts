/** @module Interface glaze:plugin/view@0.1.0 **/
export type Tone = import('./glaze-plugin-types.js').Tone;
export type TextRole = import('./glaze-plugin-types.js').TextRole;
export type Align = import('./glaze-plugin-types.js').Align;
export interface Segment {
  fraction: number,
  color: Tone,
}
export interface Series {
  label: string,
  values: Float32Array,
  color?: Tone,
}
export interface SelectOption {
  label: string,
  value: string,
}
export interface Text {
  text: string,
  role: TextRole,
  color?: Tone,
  align: Align,
}
export interface Icon {
  icon: string,
  size?: number,
  color?: Tone,
}
export interface Button {
  label: string,
  action: string,
  icon?: string,
  primary: boolean,
}
export interface IconButton {
  icon: string,
  label: string,
  action: string,
  enabled: boolean,
}
export interface Select {
  label: string,
  action: string,
  options: Array<SelectOption>,
  selected?: string,
}
export interface Field {
  placeholder: string,
  action: string,
}
export interface ListRow {
  title: string,
  detail?: string,
  icon?: string,
  trailing?: string,
  action?: string,
}
export interface Stat {
  label: string,
  value: string,
  detail?: string,
}
export interface Metric {
  value: string,
  label: string,
  color?: Tone,
  width?: number,
}
export interface Ring {
  fraction: number,
  label?: string,
  color?: Tone,
}
export interface LimitBar {
  left: number,
  pace?: number,
  color?: Tone,
  badge?: string,
  label?: string,
  reset?: string,
}
export interface Sparkline {
  values: Float32Array,
  color?: Tone,
}
/**
 * `x-labels`: first, middle, last.
 */
export interface AreaChart {
  series: Array<Series>,
  xLabels: Array<string>,
  unit?: string,
}
export interface Card {
  title?: string,
}
export interface Empty {
  icon?: string,
  title: string,
  detail?: string,
}
export interface Grid {
  columns: number,
  gap?: number,
}
/**
 * The kit's spring for `animated` nodes: changes to the child animate with it.
 */
export interface Spring {
  stiffness: number,
  damping: number,
}
/**
 * Canvas drawing commands, in kit points from the canvas's top left; drawn natively.
 */
export interface Point {
  x: number,
  y: number,
}
export interface Rect {
  x: number,
  y: number,
  w: number,
  h: number,
}
export type Paint = PaintFill | PaintStroke;
export interface PaintFill {
  tag: 'fill',
  val: Tone,
}
export interface PaintStroke {
  tag: 'stroke',
  val: [Tone, number],
}
export type Draw = DrawRect | DrawCircle | DrawLine | DrawPath | DrawText | DrawImage;
export interface DrawRect {
  tag: 'rect',
  val: [Rect, number, Paint],
}
export interface DrawCircle {
  tag: 'circle',
  val: [Point, number, Paint],
}
export interface DrawLine {
  tag: 'line',
  val: [Point, Point, Tone, number],
}
/**
 * SVG path data (`M L C Q Z`).
 */
export interface DrawPath {
  tag: 'path',
  val: [string, Paint],
}
export interface DrawText {
  tag: 'text',
  val: [Point, string, TextRole, Tone],
}
/**
 * An image the plugin registered with `ui.put-image`.
 */
export interface DrawImage {
  tag: 'image',
  val: [Rect, number],
}
export interface Canvas {
  height: number,
  commands: Array<Draw>,
}
/**
 * `gap` for stack/row/list.
 */
export type NodeKind = NodeKindStack | NodeKindRow | NodeKindList | NodeKindGrid | NodeKindCard | NodeKindText | NodeKindIcon | NodeKindButton | NodeKindIconButton | NodeKindSelect | NodeKindField | NodeKindListRow | NodeKindStat | NodeKindMetric | NodeKindRing | NodeKindLimitBar | NodeKindSegmentedBar | NodeKindSparkline | NodeKindAreaChart | NodeKindEmpty | NodeKindSpacer | NodeKindAnimated | NodeKindCanvas;
export interface NodeKindStack {
  tag: 'stack',
  val: number | undefined,
}
export interface NodeKindRow {
  tag: 'row',
  val: number | undefined,
}
export interface NodeKindList {
  tag: 'list',
  val: number | undefined,
}
export interface NodeKindGrid {
  tag: 'grid',
  val: Grid,
}
export interface NodeKindCard {
  tag: 'card',
  val: Card,
}
export interface NodeKindText {
  tag: 'text',
  val: Text,
}
export interface NodeKindIcon {
  tag: 'icon',
  val: Icon,
}
export interface NodeKindButton {
  tag: 'button',
  val: Button,
}
export interface NodeKindIconButton {
  tag: 'icon-button',
  val: IconButton,
}
export interface NodeKindSelect {
  tag: 'select',
  val: Select,
}
export interface NodeKindField {
  tag: 'field',
  val: Field,
}
export interface NodeKindListRow {
  tag: 'list-row',
  val: ListRow,
}
export interface NodeKindStat {
  tag: 'stat',
  val: Stat,
}
export interface NodeKindMetric {
  tag: 'metric',
  val: Metric,
}
export interface NodeKindRing {
  tag: 'ring',
  val: Ring,
}
export interface NodeKindLimitBar {
  tag: 'limit-bar',
  val: LimitBar,
}
export interface NodeKindSegmentedBar {
  tag: 'segmented-bar',
  val: Array<Segment>,
}
export interface NodeKindSparkline {
  tag: 'sparkline',
  val: Sparkline,
}
export interface NodeKindAreaChart {
  tag: 'area-chart',
  val: AreaChart,
}
export interface NodeKindEmpty {
  tag: 'empty',
  val: Empty,
}
export interface NodeKindSpacer {
  tag: 'spacer',
}
export interface NodeKindAnimated {
  tag: 'animated',
  val: Spring,
}
export interface NodeKindCanvas {
  tag: 'canvas',
  val: Canvas,
}
/**
 * Followed by `children` direct children, each a whole subtree.
 */
export interface Node {
  kind: NodeKind,
  children: number,
}
/**
 * One face's body. The shell adds the chrome (header, footer, refresh button).
 */
export interface Face {
  nodes: Array<Node>,
  footer?: string,
  refresh: boolean,
}
