// 阅读器内浮层卡片：TxtReader 与 EpubReader 共用（iframe 内坐标需先行换算）。

import type { JSX } from 'solid-js';

/** 浮层锚点：相对阅读器根元素的定位（由视口坐标换算而来） */
export interface PopAnchor {
  bottom: number;
  left: number;
  top: number;
}

// 优先显示在锚点上方，贴顶时翻到锚点下方
export const FloatingCard = (props: {
  anchor: PopAnchor;
  children: JSX.Element;
}) => (
  <div
    class="bg-surface border-edge rs-fade-in absolute z-30 flex flex-col rounded-lg border p-1.5 shadow-lg"
    style={{
      left: `clamp(80px, ${props.anchor.left}px, calc(100% - 80px))`,
      top:
        props.anchor.top > 56
          ? `${props.anchor.top - 8}px`
          : `${props.anchor.bottom + 8}px`,
      transform: `translate(-50%, ${props.anchor.top > 56 ? '-100%' : '0'})`,
    }}
  >
    {props.children}
  </div>
);
