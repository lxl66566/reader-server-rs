// Reusable form controls for the settings page: label rows, steppers, range
// sliders, selects and color swatches. They stay dumb (value in via props,
// changes out via callbacks) so the page owns all store wiring.

import { VsAdd, VsRemove } from 'solid-icons/vs';
import { For, type JSX, type ParentComponent } from 'solid-js';

export const SettingRow: ParentComponent<{
  label: string;
  right: JSX.Element;
}> = props => (
  <div class="flex items-center justify-between gap-3 py-2">
    <span class="text-ink text-sm">{props.label}</span>
    {props.right}
  </div>
);

export const StepperControl: ParentComponent<{
  max: number;
  min: number;
  onChange: (value: number) => void;
  value: number;
}> = props => {
  const decrease = () => {
    props.onChange(Math.max(props.min, props.value - 1));
  };
  const increase = () => {
    props.onChange(Math.min(props.max, props.value + 1));
  };
  return (
    <div class="flex items-center gap-1">
      <button
        class="border-edge text-muted hover:bg-base hover:text-ink flex size-7 cursor-pointer items-center justify-center rounded border"
        onClick={decrease}
        type="button"
      >
        <VsRemove class="size-3.5" />
      </button>
      <span class="w-8 text-center text-sm tabular-nums">{props.value}</span>
      <button
        class="border-edge text-muted hover:bg-base hover:text-ink flex size-7 cursor-pointer items-center justify-center rounded border"
        onClick={increase}
        type="button"
      >
        <VsAdd class="size-3.5" />
      </button>
    </div>
  );
};

export const RangeControl: ParentComponent<{
  max: number;
  min: number;
  onChange: (value: number) => void;
  step: number;
  unit?: string;
  value: number;
}> = props => {
  // Number of decimal places the step implies, used to drop float noise.
  const decimals = () => String(props.step).split('.')[1]?.length ?? 0;
  const display = () => props.value.toFixed(decimals());
  return (
    <div class="flex items-center gap-2">
      <input
        class="accent-accent w-32"
        max={props.max}
        min={props.min}
        onInput={event => {
          const value = Number(event.currentTarget.value);
          props.onChange(Number(value.toFixed(decimals())));
        }}
        step={props.step}
        type="range"
        value={props.value}
      />
      <span class="text-muted w-12 text-right text-sm tabular-nums">
        {display()}
        {props.unit}
      </span>
    </div>
  );
};

export const SelectControl: ParentComponent<{
  onChange: (value: string) => void;
  options: { label: string; value: string }[];
  value: string;
}> = props => (
  <select
    class="border-edge bg-surface text-ink focus:border-accent max-w-44 rounded border px-2 py-1 text-sm outline-none"
    onChange={event => {
      props.onChange(event.currentTarget.value);
    }}
    value={props.value}
  >
    <For each={props.options}>
      {option => <option value={option.value}>{option.label}</option>}
    </For>
  </select>
);

export const ColorControl: ParentComponent<{
  onChange: (value: string) => void;
  presets: string[];
  value: string;
}> = props => {
  // 当前值不在预设中（即自定义色生效）时，高亮取色圆圈
  const isCustom = () => !props.presets.includes(props.value);
  return (
    <div class="flex items-center gap-1.5">
      <For each={props.presets}>
        {color => (
          <button
            aria-label={color}
            class={`size-6 cursor-pointer rounded-full border transition-transform hover:scale-110 ${props.value === color ? 'border-accent' : 'border-edge'}`}
            onClick={() => {
              props.onChange(color);
            }}
            style={{ 'background-color': color }}
            type="button"
          />
        )}
      </For>
      {/* 自定义取色：虚线边框 + 居中加号，与预设色块区分；
          mix-blend-difference 保证加号在任意底色下可见 */}
      <label
        class={`relative size-6 cursor-pointer overflow-hidden rounded-full border border-dashed transition-transform hover:scale-110 ${isCustom() ? 'border-accent' : 'border-edge'}`}
        title={props.value}
      >
        <span
          class="absolute inset-0 block"
          style={{ 'background-color': props.value }}
        />
        <VsAdd class="absolute inset-0 m-auto size-3.5 text-white mix-blend-difference" />
        <input
          aria-label={props.value}
          class="absolute inset-0 cursor-pointer opacity-0"
          onInput={event => {
            props.onChange(event.currentTarget.value);
          }}
          type="color"
          value={props.value}
        />
      </label>
    </div>
  );
};
