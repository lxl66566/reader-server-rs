// Inline tag editor: chip list with add-on-Enter and per-chip removal.
import { VsClose } from 'solid-icons/vs';
import { createEffect, createSignal, For, type Component } from 'solid-js';
import toast from 'solid-toast';

import { useI18n } from '../../i18n';
import { Input } from '../ui';

export const TagEditor: Component<{
  onChange: (tags: string[]) => Promise<void>;
  tags: string[];
}> = props => {
  const { t } = useI18n();
  const [items, setItems] = createSignal<string[]>([]);
  const [draft, setDraft] = createSignal('');

  // Follow the server-provided list (e.g. after a refetch).
  createEffect(() => {
    setItems(props.tags);
  });

  const commit = (next: string[]) => {
    setItems(next);
    void props.onChange(next).catch(() => toast.error(t('errorGeneric')));
  };

  const addTag = () => {
    const tag = draft().trim();
    if (!tag) return;
    setDraft('');
    if (items().includes(tag)) return;
    commit([...items(), tag]);
  };

  const removeTag = (tag: string) => {
    commit(items().filter(item => item !== tag));
  };

  return (
    <div class="flex flex-wrap items-center gap-2">
      <For each={items()}>
        {tag => (
          <button
            aria-label={`${t('delete')}: ${tag}`}
            class="border-edge bg-page hover:border-accent flex min-h-10 cursor-pointer items-center gap-1 rounded-full border px-3 text-sm transition-colors"
            onClick={() => {
              removeTag(tag);
            }}
            type="button"
          >
            {tag}
            <VsClose class="text-muted size-3.5" />
          </button>
        )}
      </For>
      <Input
        class="min-h-10 flex-1"
        onInput={setDraft}
        onKeyDown={event => {
          if (event.key === 'Enter') {
            event.preventDefault();
            addTag();
          }
        }}
        placeholder={t('addTagPlaceholder')}
        value={draft()}
      />
    </div>
  );
};
