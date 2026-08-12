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
    <span class="text-sm text-ink">{props.label}</span>
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
        class="flex size-7 cursor-pointer items-center justify-center rounded border border-edge text-muted hover:bg-base hover:text-ink"
        onClick={decrease}
        type="button"
      >
        <VsRemove class="size-3.5" />
      </button>
      <span class="w-8 text-center text-sm tabular-nums">{props.value}</span>
      <button
        class="flex size-7 cursor-pointer items-center justify-center rounded border border-edge text-muted hover:bg-base hover:text-ink"
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
        class="w-32 accent-accent"
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
      <span class="w-12 text-right text-sm tabular-nums text-muted">
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
    class="max-w-44 rounded border border-edge bg-surface px-2 py-1 text-sm text-ink outline-none focus:border-accent"
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
}> = props => (
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
    <label
      class="relative size-6 cursor-pointer overflow-hidden rounded-full border border-edge"
      title={props.value}
    >
      <span
        class="absolute inset-0 block"
        style={{ 'background-color': props.value }}
      />
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
