// Small shared UI primitives. Kept deliberately plain.

import { VsClose, VsLoading } from 'solid-icons/vs';
import {
  createEffect,
  onCleanup,
  Show,
  type JSX,
  type ParentComponent,
} from 'solid-js';

type ButtonVariant = 'danger' | 'ghost' | 'primary';

const BUTTON_STYLES: Record<ButtonVariant, string> = {
  danger:
    'bg-transparent text-red-700 dark:text-red-400 border border-red-300 hover:bg-red-50 dark:hover:bg-red-950',
  ghost: 'bg-transparent text-ink border border-edge hover:bg-surface',
  primary: 'bg-ink text-page hover:opacity-85 border border-ink',
};

export const Button: ParentComponent<{
  class?: string;
  disabled?: boolean;
  onClick?: () => void;
  type?: 'button' | 'submit';
  variant?: ButtonVariant;
}> = props => (
  <button
    class={`inline-flex cursor-pointer items-center justify-center gap-1 rounded px-3 py-1.5 text-sm transition-opacity disabled:cursor-not-allowed disabled:opacity-50 ${BUTTON_STYLES[props.variant ?? 'primary']} ${props.class ?? ''}`}
    disabled={props.disabled}
    onClick={() => props.onClick?.()}
    type={props.type ?? 'button'}
  >
    {props.children}
  </button>
);

export const Input: ParentComponent<{
  class?: string;
  onInput: (value: string) => void;
  onKeyDown?: (event: KeyboardEvent) => void;
  placeholder?: string;
  type?: string;
  value: string;
}> = props => (
  <input
    class={`border-edge bg-surface text-ink focus:border-accent w-full rounded border px-3 py-1.5 text-sm outline-none ${props.class ?? ''}`}
    onInput={event => {
      props.onInput(event.currentTarget.value);
    }}
    onKeyDown={event => props.onKeyDown?.(event)}
    placeholder={props.placeholder}
    type={props.type ?? 'text'}
    value={props.value}
  />
);

export const Spinner: ParentComponent<{ class?: string }> = props => (
  <VsLoading class={`animate-spin ${props.class ?? 'size-5'}`} />
);

export const EmptyState: ParentComponent<{ text: string }> = props => (
  <div class="text-muted py-12 text-center text-sm">{props.text}</div>
);

export const Toggle: ParentComponent<{
  checked: boolean;
  label?: string;
  onChange: (checked: boolean) => void;
}> = props => (
  <label class="inline-flex cursor-pointer items-center gap-2 text-sm">
    <button
      aria-checked={props.checked}
      class={`relative h-5 w-9 cursor-pointer rounded-full transition-colors ${props.checked ? 'bg-accent' : 'bg-edge'}`}
      onClick={() => {
        props.onChange(!props.checked);
      }}
      role="switch"
      type="button"
    >
      {/* knob needs explicit left-0.5: without it the abs-positioned static
          position follows the button's text-align:center and slides too far right */}
      <span
        class={`bg-surface absolute top-0.5 left-0.5 size-4 rounded-full transition-transform ${props.checked ? 'translate-x-4' : 'translate-x-0'}`}
      />
    </button>
    <Show when={props.label}>{props.label}</Show>
  </label>
);

// Modal rendered inline (no portal needed for this app's scale).
export const Modal: ParentComponent<{
  onClose: () => void;
  open: boolean;
  title: string;
}> = props => {
  const onKey = (event: KeyboardEvent) => {
    if (event.key === 'Escape') props.onClose();
  };
  createEffect(() => {
    if (props.open) {
      document.addEventListener('keydown', onKey);
      onCleanup(() => {
        document.removeEventListener('keydown', onKey);
      });
    }
  });
  return (
    <Show when={props.open}>
      <div
        class="rs-fade-in fixed inset-0 z-50 flex items-end justify-center bg-black/40 sm:items-center"
        onClick={event => {
          if (event.target === event.currentTarget) props.onClose();
        }}
      >
        {/* pb uses max(): .safe-bottom's longhand (index.css loads after uno.css)
            would override p-4's padding-bottom with env()=0 on desktop */}
        <div class="bg-surface w-full max-w-md rounded-t-lg px-4 pt-4 pb-[max(calc(var(--spacing)*4),env(safe-area-inset-bottom))] sm:rounded-lg">
          <div class="mb-3 flex items-center justify-between">
            <h2 class="text-base font-medium">{props.title}</h2>
            <button
              aria-label={props.title}
              class="text-muted hover:text-ink cursor-pointer"
              onClick={() => {
                props.onClose();
              }}
              type="button"
            >
              <VsClose class="size-4" />
            </button>
          </div>
          {props.children}
        </div>
      </div>
    </Show>
  );
};

export const SectionCard: ParentComponent<{
  actions?: JSX.Element;
  class?: string;
  title?: string;
}> = props => (
  <section
    class={`border-edge bg-surface rounded-lg border p-4 ${props.class ?? ''}`}
  >
    <Show when={props.title}>
      <div class="mb-3 flex items-center justify-between">
        <h2 class="text-muted text-sm font-medium">{props.title}</h2>
        {props.actions}
      </div>
    </Show>
    {props.children}
  </section>
);
