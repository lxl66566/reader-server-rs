// Reader chrome: top/bottom bars + toc / bookmarks / annotations / settings
// panels. Shown on center tap, hidden while reading.

import { useNavigate } from '@solidjs/router';
import {
  VsArrowLeft,
  VsBookmark,
  VsChecklist,
  VsListOrdered,
  VsNote,
  VsSettingsGear,
} from 'solid-icons/vs';
import { createSignal, For, Show } from 'solid-js';
import toast from 'solid-toast';
import { markerApi } from '../api';
import type { Annotation, BookDetail, Bookmark } from '../api/types';
import {
  ColorControl,
  RangeControl,
  SelectControl,
  SettingRow,
  StepperControl,
} from '../components/settings/controls';
import {
  Button,
  EmptyState,
  Input,
  Modal,
  Spinner,
  Toggle,
} from '../components/ui';
import { useI18n } from '../i18n';
import type { DictKey } from '../i18n/zh-CN';
import { FONT_FAMILY_OPTIONS, useSettings } from '../stores/settings';
import { formatPercent } from '../utils/format';
import type { ReaderNavigator } from './types';

export interface ChromeProps {
  annotations: () => Annotation[] | undefined;
  book: BookDetail;
  bookmarks: () => Bookmark[] | undefined;
  currentPosition: () => null | { locator: string; percent: number };
  navigator: () => null | ReaderNavigator;
  percent: () => number;
  reconnecting: () => boolean;
  refetchAnnotations: () => void;
  refetchBookmarks: () => void;
  visible: () => boolean;
}

type Panel = 'annotations' | 'bookmarks' | 'settings' | 'toc' | null;

const BG_PRESETS = ['#faf9f6', '#f8f1e1', '#201f1b', '#eef1f5'];
const FG_PRESETS = ['#2b2a26', '#4a3f2d', '#e8e6e0', '#5b6b8c'];

const Chrome = (props: ChromeProps) => {
  const { t } = useI18n();
  const navigate = useNavigate();
  const [panel, setPanel] = createSignal<Panel>(null);

  const open = (next: Exclude<Panel, null>) => setPanel(next);
  const jump = (fn: (nav: ReaderNavigator) => void) => {
    const nav = props.navigator();
    if (nav) fn(nav);
    setPanel(null);
  };

  return (
    <>
      {/* reconnecting hint is always visible, even with chrome hidden */}
      <Show when={props.reconnecting()}>
        <div class="safe-top absolute top-0 left-0 z-30 w-full bg-black/60 py-1 text-center text-xs text-white">
          {t('readerReconnecting')}
        </div>
      </Show>

      <Show when={props.visible()}>
        {/* top bar */}
        <div class="safe-top rs-fade-in absolute top-0 left-0 z-20 w-full border-b border-edge bg-surface/95 backdrop-blur">
          <div class="flex h-12 items-center gap-2 px-3">
            <button
              aria-label={t('readerExit')}
              class="flex size-9 cursor-pointer items-center justify-center rounded text-muted hover:bg-base hover:text-ink"
              onClick={() => {
                navigate(`/book/${props.book.book_id}`);
              }}
              type="button"
            >
              <VsArrowLeft class="size-5" />
            </button>
            <div class="min-w-0 flex-1 truncate text-sm font-medium">
              {props.book.title}
            </div>
            <div class="text-xs text-muted tabular-nums">
              {t('progressPercent', {
                percent: formatPercent(props.percent()),
              })}
            </div>
          </div>
        </div>

        {/* bottom bar */}
        <div class="safe-bottom rs-fade-in absolute bottom-0 left-0 z-20 w-full border-t border-edge bg-surface/95 backdrop-blur">
          <div class="flex h-14 items-stretch justify-around px-2">
            <BarButton
              icon={<VsListOrdered class="size-5" />}
              label={t('readerToc')}
              onClick={() => open('toc')}
            />
            <BarButton
              icon={<VsBookmark class="size-5" />}
              label={t('readerBookmarks')}
              onClick={() => open('bookmarks')}
            />
            <BarButton
              icon={<VsNote class="size-5" />}
              label={t('readerAnnotations')}
              onClick={() => open('annotations')}
            />
            <BarButton
              icon={<VsSettingsGear class="size-5" />}
              label={t('readerSettings')}
              onClick={() => open('settings')}
            />
          </div>
        </div>
      </Show>

      <TocPanel
        book={props.book}
        onClose={() => setPanel(null)}
        onJump={position => {
          jump(nav => {
            nav.jumpToChapter(position);
          });
        }}
        open={panel() === 'toc'}
      />
      <BookmarksPanel
        book={props.book}
        bookmarks={props.bookmarks}
        currentPosition={props.currentPosition}
        onClose={() => setPanel(null)}
        onJump={locator => {
          jump(nav => {
            nav.jumpToLocator(locator);
          });
        }}
        open={panel() === 'bookmarks'}
        refetch={props.refetchBookmarks}
      />
      <AnnotationsPanel
        annotations={props.annotations}
        book={props.book}
        currentPosition={props.currentPosition}
        onClose={() => setPanel(null)}
        onJump={locator => {
          jump(nav => {
            nav.jumpToLocator(locator);
          });
        }}
        open={panel() === 'annotations'}
        refetch={props.refetchAnnotations}
      />
      <SettingsPanel
        onClose={() => setPanel(null)}
        open={panel() === 'settings'}
      />
    </>
  );
};

export default Chrome;

// empty-string labels fall back to the percent display (label is optional)
const bookmarkTitle = (
  bookmark: Bookmark,
  t: (key: DictKey, params?: Record<string, number | string>) => string,
): string =>
  bookmark.label && bookmark.label.length > 0
    ? bookmark.label
    : t('progressPercent', { percent: formatPercent(bookmark.percent) });

const BarButton = (buttonProps: {
  icon: import('solid-js').JSX.Element;
  label: string;
  onClick: () => void;
}) => (
  <button
    class="flex min-w-16 cursor-pointer flex-col items-center justify-center gap-0.5 rounded px-2 text-muted hover:bg-base hover:text-ink"
    onClick={() => {
      buttonProps.onClick();
    }}
    type="button"
  >
    {buttonProps.icon}
    <span class="text-xs">{buttonProps.label}</span>
  </button>
);

// ---------------------------------------------------------------------------
// toc
// ---------------------------------------------------------------------------

const TocPanel = (panelProps: {
  book: BookDetail;
  onClose: () => void;
  onJump: (position: number) => void;
  open: boolean;
}) => {
  const { t } = useI18n();
  return (
    <Modal
      onClose={panelProps.onClose}
      open={panelProps.open}
      title={t('readerToc')}
    >
      <Show
        fallback={<EmptyState text={t('noChapters')} />}
        when={panelProps.book.chapters.length > 0}
      >
        <ul class="max-h-[60vh] overflow-y-auto">
          <For each={panelProps.book.chapters}>
            {chapter => (
              <li>
                <button
                  class="w-full cursor-pointer rounded px-2 py-2.5 text-left text-sm hover:bg-base"
                  onClick={() => {
                    panelProps.onJump(chapter.position);
                  }}
                  type="button"
                >
                  {chapter.title}
                </button>
              </li>
            )}
          </For>
        </ul>
      </Show>
    </Modal>
  );
};

// ---------------------------------------------------------------------------
// bookmarks
// ---------------------------------------------------------------------------

const BookmarksPanel = (panelProps: {
  book: BookDetail;
  bookmarks: () => Bookmark[] | undefined;
  currentPosition: ChromeProps['currentPosition'];
  onClose: () => void;
  onJump: (locator: string) => void;
  open: boolean;
  refetch: () => void;
}) => {
  const { t } = useI18n();
  const [label, setLabel] = createSignal('');
  const [busy, setBusy] = createSignal(false);

  const add = async () => {
    const pos = panelProps.currentPosition();
    if (!pos || busy()) return;
    setBusy(true);
    try {
      await markerApi.addBookmark(
        panelProps.book.book_id,
        pos.locator,
        pos.percent,
        label() || undefined,
      );
      setLabel('');
      panelProps.refetch();
      toast.success(t('readerBookmarkAdded'));
    } catch (error) {
      toast.error(error instanceof Error ? error.message : t('errorGeneric'));
    } finally {
      setBusy(false);
    }
  };

  const remove = async (id: number) => {
    try {
      await markerApi.removeBookmark(panelProps.book.book_id, id);
      panelProps.refetch();
    } catch (error) {
      toast.error(error instanceof Error ? error.message : t('errorGeneric'));
    }
  };

  return (
    <Modal
      onClose={panelProps.onClose}
      open={panelProps.open}
      title={t('readerBookmarks')}
    >
      <div class="mb-3 flex gap-2">
        <Input
          class="flex-1"
          onInput={setLabel}
          placeholder={t('addBookmarkLabel')}
          value={label()}
        />
        <Button disabled={busy()} onClick={() => void add()}>
          {t('addBookmark')}
        </Button>
      </div>
      <Show
        fallback={
          <div class="flex justify-center py-6">
            <Spinner />
          </div>
        }
        when={panelProps.bookmarks()}
      >
        {list => (
          <Show
            fallback={<EmptyState text={t('noBookmarks')} />}
            when={list().length > 0}
          >
            <ul class="max-h-[50vh] overflow-y-auto">
              <For each={list()}>
                {bookmark => (
                  <li class="flex items-center gap-2">
                    <button
                      class="min-w-0 flex-1 cursor-pointer truncate rounded px-2 py-2.5 text-left text-sm hover:bg-base"
                      onClick={() => {
                        panelProps.onJump(bookmark.locator);
                      }}
                      type="button"
                    >
                      {bookmarkTitle(bookmark, t)}
                    </button>
                    <button
                      aria-label={t('delete')}
                      class="cursor-pointer rounded px-2 py-1 text-xs text-muted hover:text-red-700"
                      onClick={() => void remove(bookmark.id)}
                      type="button"
                    >
                      {t('delete')}
                    </button>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        )}
      </Show>
    </Modal>
  );
};

// ---------------------------------------------------------------------------
// annotations
// ---------------------------------------------------------------------------

const AnnotationsPanel = (panelProps: {
  annotations: () => Annotation[] | undefined;
  book: BookDetail;
  currentPosition: ChromeProps['currentPosition'];
  onClose: () => void;
  onJump: (locator: string) => void;
  open: boolean;
  refetch: () => void;
}) => {
  const { t } = useI18n();
  const [draft, setDraft] = createSignal('');
  const [busy, setBusy] = createSignal(false);

  const add = async () => {
    const pos = panelProps.currentPosition();
    const content = draft().trim();
    if (!pos || !content || busy()) return;
    setBusy(true);
    try {
      await markerApi.addAnnotation(panelProps.book.book_id, {
        content,
        kind: 'note',
        start_locator: pos.locator,
        start_percent: pos.percent,
      });
      setDraft('');
      panelProps.refetch();
    } catch (error) {
      toast.error(error instanceof Error ? error.message : t('errorGeneric'));
    } finally {
      setBusy(false);
    }
  };

  const remove = async (id: number) => {
    try {
      await markerApi.removeAnnotation(panelProps.book.book_id, id);
      panelProps.refetch();
    } catch (error) {
      toast.error(error instanceof Error ? error.message : t('errorGeneric'));
    }
  };

  return (
    <Modal
      onClose={panelProps.onClose}
      open={panelProps.open}
      title={t('readerAnnotations')}
    >
      <div class="mb-3">
        <textarea
          class="h-20 w-full rounded border border-edge bg-surface px-3 py-2 text-sm text-ink outline-none focus:border-accent"
          onInput={event => setDraft(event.currentTarget.value)}
          placeholder={t('readerNotePlaceholder')}
          value={draft()}
        />
        <div class="mt-2 flex justify-end">
          <Button
            disabled={busy() || !draft().trim()}
            onClick={() => void add()}
          >
            {t('readerSaveNote')}
          </Button>
        </div>
      </div>
      <Show
        fallback={
          <div class="flex justify-center py-6">
            <Spinner />
          </div>
        }
        when={panelProps.annotations()}
      >
        {list => (
          <Show
            fallback={<EmptyState text={t('noAnnotations')} />}
            when={list().length > 0}
          >
            <ul class="max-h-[50vh] space-y-2 overflow-y-auto">
              <For each={list()}>
                {annotation => (
                  <li class="rounded border border-edge p-2">
                    <div class="flex items-center justify-between text-xs text-muted">
                      <span>
                        {annotation.kind === 'note'
                          ? t('kindNote')
                          : t('kindHighlight')}
                        {' · '}
                        {t('progressPercent', {
                          percent: formatPercent(annotation.start_percent),
                        })}
                      </span>
                      <span class="flex gap-2">
                        <button
                          class="cursor-pointer hover:text-ink"
                          onClick={() => {
                            panelProps.onJump(annotation.start_locator);
                          }}
                          type="button"
                        >
                          <VsChecklist class="size-4" />
                        </button>
                        <button
                          aria-label={t('delete')}
                          class="cursor-pointer hover:text-red-700"
                          onClick={() => void remove(annotation.id)}
                          type="button"
                        >
                          {t('delete')}
                        </button>
                      </span>
                    </div>
                    <Show when={annotation.excerpt}>
                      <p class="mt-1 border-l-2 border-edge pl-2 text-xs text-muted">
                        {annotation.excerpt}
                      </p>
                    </Show>
                    <Show when={annotation.content}>
                      <p class="mt-1 text-sm">{annotation.content}</p>
                    </Show>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        )}
      </Show>
    </Modal>
  );
};

// ---------------------------------------------------------------------------
// settings
// ---------------------------------------------------------------------------

const SettingsPanel = (panelProps: { onClose: () => void; open: boolean }) => {
  const { t } = useI18n();
  const { settings, update } = useSettings();

  const fontLabel = (option: string): string => {
    const keys: Record<string, DictKey> = {
      default: 'fontDefault',
      sans: 'fontSans',
      serif: 'fontSerif',
    };
    return t(keys[option] ?? 'fontDefault');
  };

  return (
    <Modal
      onClose={panelProps.onClose}
      open={panelProps.open}
      title={t('readerSettings')}
    >
      <div class="max-h-[60vh] overflow-y-auto pr-1">
        <SettingRow
          label={t('settingsFontSize')}
          right={
            <StepperControl
              max={28}
              min={12}
              onChange={value => {
                update({ font_size: value });
              }}
              value={settings.font_size}
            />
          }
        />
        <SettingRow
          label={t('settingsLineHeight')}
          right={
            <RangeControl
              max={2.4}
              min={1.2}
              onChange={value => {
                update({ line_height: value });
              }}
              step={0.1}
              value={settings.line_height}
            />
          }
        />
        <SettingRow
          label={t('settingsLetterSpacing')}
          right={
            <RangeControl
              max={4}
              min={0}
              onChange={value => {
                update({ letter_spacing: value });
              }}
              step={0.5}
              unit="px"
              value={settings.letter_spacing}
            />
          }
        />
        <SettingRow
          label={t('settingsParagraphSpacing')}
          right={
            <RangeControl
              max={2}
              min={0}
              onChange={value => {
                update({ paragraph_spacing: value });
              }}
              step={0.1}
              unit="em"
              value={settings.paragraph_spacing}
            />
          }
        />
        <SettingRow
          label={t('settingsReadingWidth')}
          right={
            <RangeControl
              max={60}
              min={20}
              onChange={value => {
                update({ reading_width: value });
              }}
              step={1}
              unit="em"
              value={settings.reading_width}
            />
          }
        />
        <SettingRow
          label={t('settingsTextIndent')}
          right={
            <SelectControl
              onChange={value => {
                update({ text_indent: Number(value) });
              }}
              options={[
                { label: t('indentNone'), value: '0' },
                { label: t('indentOne'), value: '1' },
                { label: t('indentTwo'), value: '2' },
              ]}
              value={String(settings.text_indent)}
            />
          }
        />
        <SettingRow
          label={t('settingsFontFamily')}
          right={
            <SelectControl
              onChange={value => {
                update({ font_family: value });
              }}
              options={FONT_FAMILY_OPTIONS.map(option => ({
                label: fontLabel(option),
                value: option,
              }))}
              value={settings.font_family}
            />
          }
        />
        <SettingRow
          label={t('settingsBackground')}
          right={
            <ColorControl
              onChange={value => {
                update({ background_color: value });
              }}
              presets={BG_PRESETS}
              value={settings.background_color}
            />
          }
        />
        <SettingRow
          label={t('settingsTextColor')}
          right={
            <ColorControl
              onChange={value => {
                update({ text_color: value });
              }}
              presets={FG_PRESETS}
              value={settings.text_color}
            />
          }
        />
        <SettingRow
          label={t('settingsSimplified')}
          right={
            <Toggle
              checked={settings.simplified_chinese}
              onChange={checked => {
                update({ simplified_chinese: checked });
              }}
            />
          }
        />
        <SettingRow
          label={t('settingsPaginationMode')}
          right={
            <SelectControl
              onChange={value => {
                update({
                  pagination_mode: value === 'scroll' ? 'scroll' : 'paginate',
                });
              }}
              options={[
                { label: t('modeScroll'), value: 'scroll' },
                { label: t('modePaginate'), value: 'paginate' },
              ]}
              value={settings.pagination_mode}
            />
          }
        />
      </div>
    </Modal>
  );
};
