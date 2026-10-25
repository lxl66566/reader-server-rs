// TXT reader: chunked loading + scroll / paginate modes + touch gestures.
// 划词高亮：选区→locator 编码→批注写入；已存高亮按章节窗口裁剪后分段渲染。

import { useNavigate } from '@solidjs/router';
import { VsEdit, VsNote, VsSymbolColor, VsTrash } from 'solid-icons/vs';
import {
  createEffect,
  createMemo,
  createSignal,
  For,
  Index,
  onCleanup,
  onMount,
  Show,
  untrack,
} from 'solid-js';
import toast from 'solid-toast';

import { bookApi, markerApi } from '../api';
import type { Annotation } from '../api/types';
import { LoadErrorPanel } from '../components/LoadErrorPanel';
import { useI18n } from '../i18n';
import { fetchAnnotations, offlineContentLoader } from '../offline/api';
import { fontFamilyCss, useSettings } from '../stores/settings';
import {
  convertChinese,
  convertDirection,
  loadChineseConverter,
} from '../utils/opencc';
import { FloatingCard, type PopAnchor } from './FloatingCard';
import {
  animateScrollBy,
  bindScrollEdges,
  PAGE_SCROLL_RATIO,
  scrollEdges,
  shiftRunningScroll,
  useStripFlip,
  useVerticalTapZones,
} from './gestures';
import {
  charOffsetInParagraph,
  encodeHighlightLocator,
  highlightsInWindow,
  paragraphElementOf,
  parseHighlightLocator,
  segmentParagraph,
  splitParagraphs,
  type HighlightRange,
  type SplitParagraph,
} from './highlights';
import { bindReaderKeys } from './keyboard';
import { measureCharsPerPage, type PageTypography } from './measure';
import { NoteEditorModal } from './NoteEditor';
import PageHint from './PageHint';
import {
  chapterBounds,
  chapterIndexAt,
  codePointLength,
  extendWindowDown,
  extendWindowUp,
  jumpWindow,
  type ScrollWindow,
  sliceByCodePoints,
  TextSource,
  TXT_CHUNK_SIZE,
} from './textSource';
import type { FormatReaderProps } from './types';

const PAGE_INDICATOR_SPACE_PX = 28;
// 分页重测的去抖间隔：拖动滑块时每个 tick 都改排版，停下后才重测
const MEASURE_DEBOUNCE_MS = 200;
// 滚动模式跳转落点：目标段落顶部距视口顶的留白。findParagraphElAtTop
// 的回读容差必须与此对齐，否则跳转落定后首个 onScroll 会把位置读回
// 上一段，每次开合页面进度回退一段并写回服务端
const JUMP_TOP_INSET_PX = 12;
// selectionchange 的去抖间隔：拖选过程中浮层跟随太抖
const SELECTION_DEBOUNCE_MS = 160;
// 摘录上限（码点）：远小于后端 8KB 文本上限
const EXCERPT_MAX_CODE_POINTS = 600;

/** 已捕获的正文选区：全书字符区间 + 浮层锚点 */
interface CapturedSelection {
  anchor: PopAnchor;
  end: number;
  start: number;
}

interface ScrollAnchor {
  delta: number;
  pos: number;
}

interface ViewProps {
  annotations: () => readonly Annotation[];
  book: FormatReaderProps['book'];
  displayText: (text: string) => string;
  /** tap 命中已有高亮时返回 true（由点按处理打开操作层） */
  interceptMarkTap: (
    target: EventTarget | null,
    event?: MouseEvent | TouchEvent,
  ) => boolean;
  onTapCenter: () => void;
  readingWidth: () => number;
  source: TextSource;
  typography: () => PageTypography;
}

const TxtReader = (props: FormatReaderProps) => {
  const { t } = useI18n();
  const { settings } = useSettings();
  // 断网时 chunk 读取回退 IDB 缓存；在线读取写穿缓存
  const source = new TextSource(
    props.book.book_id,
    props.book.total_chars,
    offlineContentLoader(props.book.book_id),
  );

  // char offset at the top of the viewport (scroll) or page start (paginate)
  const [offset, setOffset] = createSignal(initialOffset());
  const [ready, setReady] = createSignal(false);
  // bridge for navigator jumps while in scroll mode
  const [scrollJump, setScrollJump] = createSignal<null | number>(null);
  // 滚动模式跳转的完成回调：ScrollView 执行完 jumpTo 后 resolve
  let scrollJumpDone: ((ok: boolean) => void) | null = null;

  // --- 划词高亮状态 ---
  let rootEl: HTMLDivElement | undefined;
  const [annotations, setAnnotations] = createSignal<readonly Annotation[]>([]);
  // annotationsVersion 由 shell 在本端写入与 ws 推送时 bump
  createEffect(() => {
    props.annotationsVersion();
    void fetchAnnotations(props.book.book_id)
      .then(setAnnotations)
      .catch((err: unknown) => {
        console.error('Failed to load annotations', err);
      });
  });

  const [selection, setSelection] = createSignal<CapturedSelection | null>(
    null,
  );
  // 点按已有高亮弹出的操作层
  const [markMenu, setMarkMenu] = createSignal<null | {
    anchor: PopAnchor;
    annotation: Annotation;
  }>(null);
  // 划词写笔记 / 编辑已有笔记的弹窗数据
  const [noteDraft, setNoteDraft] = createSignal<null | {
    end: number;
    excerpt: string;
    start: number;
  }>(null);
  const [editTarget, setEditTarget] = createSignal<Annotation | null>(null);
  const [busy, setBusy] = createSignal(false);

  function initialOffset(): number {
    const fromLocator = Number.parseInt(props.initialLocator ?? '', 10);
    if (Number.isFinite(fromLocator) && fromLocator >= 0) {
      return Math.min(fromLocator, props.book.total_chars);
    }
    return Math.floor(props.book.percent * props.book.total_chars);
  }

  // --- script conversion: t2s / s2t / none per setting (offsets stay on the original) ---
  // 转换器加载完成后 bump 版本号，触发文本重算
  const [converterVersion, setConverterVersion] = createSignal(0);
  createEffect(() => {
    const direction = convertDirection(settings.chinese_conversion);
    void loadChineseConverter(direction).then(() =>
      setConverterVersion(v => v + 1),
    );
  });
  const displayText = (text: string) => {
    converterVersion();
    return convertChinese(text, convertDirection(settings.chinese_conversion));
  };

  const typography = createMemo((): PageTypography => {
    const family = fontFamilyCss(settings.font_family);
    return {
      fontFamily: family,
      fontSize: settings.font_size,
      letterSpacing: settings.letter_spacing,
      lineHeight: settings.line_height,
      paragraphSpacing: settings.paragraph_spacing,
      textIndent: settings.text_indent,
    };
  });

  const reportPosition = (at: number) => {
    const percent =
      props.book.total_chars > 0 ? at / props.book.total_chars : 0;
    props.onPosition(String(at), Math.min(1, Math.max(0, percent)));
  };

  // 返回值表示跳转是否成功完成（供 Reader 挂起/恢复心跳回写）
  const jumpToOffset = (at: number): Promise<boolean> => {
    const clamped = Math.min(Math.max(0, at), props.book.total_chars);
    if (settings.pagination_mode === 'paginate') {
      setOffset(clamped);
      return Promise.resolve(true);
    }
    // 滚动模式经信号桥接到 ScrollView，等待其串行执行完毕
    return new Promise(resolve => {
      scrollJumpDone?.(false); // 被新请求覆盖的旧跳转直接判失败
      scrollJumpDone = resolve;
      setScrollJump(clamped);
    });
  };

  props.registerNavigator({
    currentChapterPosition: () => offset(),
    jumpToChapter: position => jumpToOffset(position),
    jumpToLocator: locator => {
      // 高亮 locator 落到选区起点；书签/进度仍为纯数字偏移
      const highlight = parseHighlightLocator(locator);
      if (highlight) return jumpToOffset(highlight.start);
      const at = Number.parseInt(locator, 10);
      return Number.isFinite(at) ? jumpToOffset(at) : Promise.resolve(false);
    },
  });

  // 深链（章节/定位）在视图就绪后经正常跳转路径应用，
  // 保证与滚动模式的 init() 串行，避免并发竞争 scrollTop
  let deepLinkDone = false;
  createEffect(() => {
    if (!ready() || deepLinkDone) return;
    deepLinkDone = true;
    const link = props.deepLink;
    if (!link) return;
    if (link.kind === 'locator') {
      const at = Number.parseInt(link.locator, 10);
      if (Number.isFinite(at)) void jumpToOffset(at);
    } else {
      void (async () => {
        try {
          const res = await bookApi.jumpToChapter(
            props.book.book_id,
            link.chapterId,
          );
          void jumpToOffset(res.position);
        } catch {
          // 章节解析失败：停留在初始位置
        }
      })();
    }
  });

  // report once on ready and on every offset change
  createEffect(() => {
    if (ready()) reportPosition(offset());
  });

  // 位置变化（滚动/翻页）后浮层锚点失效：统一收起
  createEffect(() => {
    void offset();
    setSelection(null);
    setMarkMenu(null);
  });

  const closePopovers = () => {
    setSelection(null);
    setMarkMenu(null);
  };

  // --- 选区捕获：selectionchange 里校验「正文内的非空选区」并换算偏移 ---
  let selectionTimer: ReturnType<typeof setTimeout> | undefined;
  onMount(() => {
    const onSelectionChange = () => {
      clearTimeout(selectionTimer);
      selectionTimer = setTimeout(() => {
        // 笔记弹窗打开期间的选择变化（输入框光标等）不参与划词
        if (noteDraft() || editTarget()) return;
        captureSelection();
      }, SELECTION_DEBOUNCE_MS);
    };
    document.addEventListener('selectionchange', onSelectionChange);
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      clearSelection();
      setMarkMenu(null);
    };
    window.addEventListener('keydown', onKeyDown);
    onCleanup(() => {
      clearTimeout(selectionTimer);
      document.removeEventListener('selectionchange', onSelectionChange);
      window.removeEventListener('keydown', onKeyDown);
    });
  });

  const clearSelection = () => {
    window.getSelection()?.removeAllRanges();
    setSelection(null);
  };

  // DOM 选区边界（段落元素 + 文本节点 + 码元偏移）→ 全书码点偏移
  const domOffsetToBook = (
    p: HTMLElement,
    node: Node,
    unitOffset: number,
  ): null | number => {
    const pos = Number(p.dataset.pos);
    const indent = Number(p.dataset.indent);
    const within = charOffsetInParagraph(p, node, unitOffset);
    if (!Number.isFinite(pos) || !Number.isFinite(indent) || within === null) {
      return null;
    }
    return pos + indent + within;
  };

  const popAnchorOf = (rect: DOMRect): null | PopAnchor => {
    const rootRect = rootEl?.getBoundingClientRect();
    if (!rootRect) return null;
    return {
      bottom: rect.bottom - rootRect.top,
      left: rect.left + rect.width / 2 - rootRect.left,
      top: rect.top - rootRect.top,
    };
  };

  const captureSelection = () => {
    const sel = document.getSelection();
    const range =
      sel && sel.rangeCount > 0 && !sel.isCollapsed ? sel.getRangeAt(0) : null;
    if (!range) {
      setSelection(null);
      return;
    }
    // 出现新选区时收起高亮操作层（两者互斥）
    setMarkMenu(null);
    const startP = paragraphElementOf(range.startContainer);
    const endP = paragraphElementOf(range.endContainer);
    const start = startP
      ? domOffsetToBook(startP, range.startContainer, range.startOffset)
      : null;
    const end = endP
      ? domOffsetToBook(endP, range.endContainer, range.endOffset)
      : null;
    if (start === null || end === null || end <= start) {
      setSelection(null);
      return;
    }
    const anchor = popAnchorOf(range.getBoundingClientRect());
    if (!anchor) {
      setSelection(null);
      return;
    }
    setSelection({ anchor, end, start });
  };

  // tap 命中已有高亮：打开操作层（两条手势路径都经此消费 tap）
  const interceptMarkTap = (
    target: EventTarget | null,
    _event?: MouseEvent | TouchEvent,
  ): boolean => {
    const mark =
      target instanceof Element
        ? target.closest<HTMLElement>('mark[data-annotation-id]')
        : null;
    if (!mark) return false;
    const id = Number(mark.dataset.annotationId);
    const annotation = annotations().find(a => a.id === id);
    if (!annotation) return false;
    clearSelection();
    const anchor = popAnchorOf(mark.getBoundingClientRect());
    if (anchor) setMarkMenu({ anchor, annotation });
    return true;
  };

  // --- 批注写入 ---
  const percentAt = (at: number): number =>
    props.book.total_chars > 0
      ? Math.min(1, Math.max(0, at / props.book.total_chars))
      : 0;

  const locatorFor = (start: number, end: number): string =>
    encodeHighlightLocator({
      chapter: chapterIndexAt(props.book.chapters, start),
      end,
      start,
    });

  // 摘录取原文（渲染的是简繁转换后文本，但转换近似等长，摘录仅供阅读）
  const excerptOf = (sel: { end: number; start: number }): string =>
    sliceByCodePoints(
      source.slice(sel.start, sel.end),
      0,
      EXCERPT_MAX_CODE_POINTS,
    );

  const createHighlight = async (): Promise<void> => {
    const sel = selection();
    if (!sel || busy()) return;
    setBusy(true);
    try {
      await markerApi.addAnnotation(props.book.book_id, {
        end_locator: String(sel.end),
        end_percent: percentAt(sel.end),
        excerpt: excerptOf(sel) || undefined,
        kind: 'highlight',
        start_locator: locatorFor(sel.start, sel.end),
        start_percent: percentAt(sel.start),
      });
      props.onAnnotationsChanged?.();
      toast.success(t('readerHighlightAdded'));
    } catch (error) {
      toast.error(error instanceof Error ? error.message : t('errorGeneric'));
    } finally {
      setBusy(false);
      clearSelection();
    }
  };

  const createNote = async (content: string): Promise<void> => {
    const sel = noteDraft();
    if (!sel || busy()) return;
    setBusy(true);
    try {
      await markerApi.addAnnotation(props.book.book_id, {
        content,
        end_locator: String(sel.end),
        end_percent: percentAt(sel.end),
        excerpt: sel.excerpt || undefined,
        kind: 'note',
        start_locator: locatorFor(sel.start, sel.end),
        start_percent: percentAt(sel.start),
      });
      setNoteDraft(null);
      props.onAnnotationsChanged?.();
    } catch (error) {
      toast.error(error instanceof Error ? error.message : t('errorGeneric'));
    } finally {
      setBusy(false);
    }
  };

  const saveNoteEdit = async (content: string): Promise<void> => {
    const target = editTarget();
    if (!target || busy()) return;
    setBusy(true);
    try {
      await markerApi.updateAnnotation(props.book.book_id, target.id, {
        content,
      });
      setEditTarget(null);
      setMarkMenu(null);
      props.onAnnotationsChanged?.();
      toast.success(t('readerNoteSaved'));
    } catch (error) {
      toast.error(error instanceof Error ? error.message : t('errorGeneric'));
    } finally {
      setBusy(false);
    }
  };

  const removeHighlight = async (id: number): Promise<void> => {
    if (busy()) return;
    setBusy(true);
    try {
      await markerApi.removeAnnotation(props.book.book_id, id);
      setMarkMenu(null);
      props.onAnnotationsChanged?.();
    } catch (error) {
      toast.error(error instanceof Error ? error.message : t('errorGeneric'));
    } finally {
      setBusy(false);
    }
  };

  const viewProps = (): ViewProps => ({
    annotations,
    book: props.book,
    displayText,
    interceptMarkTap,
    onTapCenter: () => {
      closePopovers();
      props.onTapCenter();
    },
    readingWidth: () => settings.reading_width,
    source,
    typography,
  });

  return (
    <div class="relative h-full w-full" ref={rootEl}>
      <Show
        fallback={
          <ScrollView
            {...viewProps()}
            initialOffset={offset()}
            jumpRequest={scrollJump}
            onInit={() => setReady(true)}
            onJumpHandled={jump => {
              const done = scrollJumpDone;
              scrollJumpDone = null;
              setScrollJump(null); // 清空信号：相同目标的再次跳转仍可触发
              // jump 链上已 settle（enqueueJump 保证不 reject），直接桥接结果
              void jump.then(done ?? (() => undefined));
            }}
            onOffsetChange={setOffset}
          />
        }
        when={settings.pagination_mode === 'paginate'}
      >
        <PaginateView
          {...viewProps()}
          offset={offset()}
          onInit={() => setReady(true)}
          onOffsetChange={setOffset}
        />
      </Show>

      {/* 划词操作浮层 */}
      <Show when={selection()}>
        {sel => (
          <FloatingCard anchor={sel().anchor}>
            <div class="flex items-center">
              <button
                class="hover:bg-page flex cursor-pointer items-center gap-1 rounded px-2 py-1.5 text-xs"
                onClick={() => void createHighlight()}
                onMouseDown={event => {
                  event.preventDefault();
                }}
                type="button"
              >
                <VsSymbolColor class="size-4" />
                {t('readerHighlight')}
              </button>
              <button
                class="hover:bg-page flex cursor-pointer items-center gap-1 rounded px-2 py-1.5 text-xs"
                onClick={() => {
                  const current = sel();
                  setNoteDraft({
                    end: current.end,
                    excerpt: excerptOf(current),
                    start: current.start,
                  });
                  clearSelection();
                }}
                onMouseDown={event => {
                  event.preventDefault();
                }}
                type="button"
              >
                <VsNote class="size-4" />
                {t('addNote')}
              </button>
            </div>
          </FloatingCard>
        )}
      </Show>

      {/* 已有高亮的操作层 */}
      <Show when={markMenu()}>
        {menu => (
          <FloatingCard anchor={menu().anchor}>
            <Show when={menu().annotation.excerpt}>
              <p class="border-edge text-muted mb-1.5 max-w-64 truncate border-l-2 pl-1.5 text-xs">
                {menu().annotation.excerpt}
              </p>
            </Show>
            <Show when={menu().annotation.content}>
              <p class="mb-1.5 max-w-64 text-sm">{menu().annotation.content}</p>
            </Show>
            <div class="flex items-center justify-end gap-1">
              <button
                class="hover:bg-page flex cursor-pointer items-center gap-1 rounded px-2 py-1.5 text-xs"
                onClick={() => {
                  setEditTarget(menu().annotation);
                }}
                type="button"
              >
                <VsEdit class="size-3.5" />
                {menu().annotation.content === null
                  ? t('addNote')
                  : t('readerEditNote')}
              </button>
              <button
                class="flex cursor-pointer items-center gap-1 rounded px-2 py-1.5 text-xs text-red-700 hover:bg-red-50 dark:text-red-400 dark:hover:bg-red-950"
                disabled={busy()}
                onClick={() => void removeHighlight(menu().annotation.id)}
                type="button"
              >
                <VsTrash class="size-3.5" />
                {t('delete')}
              </button>
            </div>
          </FloatingCard>
        )}
      </Show>

      <NoteEditorModal
        busy={busy()}
        initial=""
        onClose={() => setNoteDraft(null)}
        onSave={content => void createNote(content)}
        open={noteDraft() !== null}
        title={t('addNote')}
      />
      <NoteEditorModal
        busy={busy()}
        initial={editTarget()?.content ?? ''}
        onClose={() => setEditTarget(null)}
        onSave={content => void saveNoteEdit(content)}
        open={editTarget() !== null}
        title={
          editTarget()?.content === null ? t('addNote') : t('readerEditNote')
        }
      />
    </div>
  );
};

export default TxtReader;

// ---------------------------------------------------------------------------
// shared helpers
// ---------------------------------------------------------------------------

function contentStyle(typo: PageTypography): Record<string, string> {
  return {
    'font-size': `${typo.fontSize}px`,
    'letter-spacing': `${typo.letterSpacing}px`,
    'line-height': String(typo.lineHeight),
    ...(typo.fontFamily ? { 'font-family': typo.fontFamily } : {}),
  };
}

// 段落正文：按高亮分段渲染，高亮段包 mark（点按与重绘都靠 data-annotation-id）
// 简繁转换为近似等长的字级映射，展示偏移直接对齐原文偏移（段首缩进由 indent 补偿）
const ParagraphText = (props: {
  displayText: (text: string) => string;
  highlights: () => HighlightRange[];
  paragraph: SplitParagraph;
}) => {
  const display = () => props.displayText(props.paragraph.text);
  const segments = createMemo(() => {
    const start = props.paragraph.offset + props.paragraph.indent;
    return segmentParagraph(
      start,
      start + codePointLength(display()),
      props.highlights(),
    );
  });
  return (
    <Show fallback=" " when={segments().length > 0}>
      <Index each={segments()}>
        {seg => {
          const text = () => {
            const start = props.paragraph.offset + props.paragraph.indent;
            return sliceByCodePoints(
              display(),
              seg().start - start,
              seg().end - start,
            );
          };
          return (
            <Show fallback={text()} when={seg().highlighted}>
              <mark
                class="bg-highlight rounded-[2px] text-inherit"
                data-annotation-id={seg().id}
              >
                {text()}
              </mark>
            </Show>
          );
        }}
      </Index>
    </Show>
  );
};

// 滚动模式中单个 chunk 的段落列表。getParagraphs 对文本未变化的 chunk
// 返回同一数组引用，配合 <For> 的 key 让窗口滑动时未变化的 chunk 不重渲染
const ChunkParagraphs = (props: {
  chunkEls: Map<number, HTMLElement[]>;
  displayText: (text: string) => string;
  getParagraphs: (index: number) => SplitParagraph[];
  highlights: () => HighlightRange[];
  index: number;
  typo: () => PageTypography;
}) => {
  const paragraphs = createMemo(() => props.getParagraphs(props.index));
  const els: HTMLElement[] = [];
  props.chunkEls.set(props.index, els);
  onCleanup(() => {
    if (props.chunkEls.get(props.index) === els) {
      props.chunkEls.delete(props.index);
    }
  });
  return (
    <Index each={paragraphs()}>
      {para => (
        <p
          data-indent={para().indent}
          data-pos={para().offset}
          ref={el => {
            els.push(el);
            onCleanup(() => {
              const i = els.indexOf(el);
              if (i >= 0) els.splice(i, 1);
            });
          }}
          style={{
            'contain-intrinsic-size': `auto ${props.typo().fontSize * props.typo().lineHeight}px`,
            'content-visibility': 'auto',
            margin: `0 0 ${props.typo().paragraphSpacing}em`,
            'text-indent': `${props.typo().textIndent}em`,
            'white-space': 'pre-wrap',
            'word-break': 'break-word',
          }}
        >
          <ParagraphText
            displayText={props.displayText}
            highlights={props.highlights}
            paragraph={para()}
          />
        </p>
      )}
    </Index>
  );
};

// 二分查找最后一个满足条件的段落（段落的 pos / offsetTop 均随 DOM 顺序单调）
function lastMatching(
  els: HTMLElement[],
  pred: (el: HTMLElement) => boolean,
): HTMLElement | undefined {
  let lo = 0;
  let hi = els.length - 1;
  let best: HTMLElement | undefined;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    const el = els[mid];
    if (!el) break;
    if (pred(el)) {
      best = el;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return best;
}

// ---------------------------------------------------------------------------
// paginate mode
// ---------------------------------------------------------------------------

const PaginateView = (
  props: ViewProps & {
    offset: number;
    onInit: () => void;
    onOffsetChange: (at: number) => void;
  },
) => {
  const { t } = useI18n();
  const navigate = useNavigate();
  let container: HTMLDivElement | undefined;

  const [size, setSize] = createSignal({ height: 0, width: 0 });
  const [charsPerPage, setCharsPerPage] = createSignal(0);
  const [version, setVersion] = createSignal(0); // bump after chunk loads
  const [loading, setLoading] = createSignal(true);
  const [error, setError] = createSignal(false);

  // 测量代际：只有最新一代的结果允许落地，避免旧参数的重测覆盖新结果
  let measureGen = 0;
  // 排版/尺寸变化的去抖定时器（拖动滑块时每个 tick 都会触发）
  let measureTimer: ReturnType<typeof setTimeout> | undefined;

  onMount(() => {
    const observer = new ResizeObserver(entries => {
      const entry = entries[0];
      if (!entry) return;
      setSize({
        height: Math.floor(entry.contentRect.height),
        width: Math.floor(entry.contentRect.width),
      });
    });
    if (container) observer.observe(container);
    onCleanup(() => {
      observer.disconnect();
    });
  });

  const onLoadError = (err: unknown) => {
    console.error('Failed to load txt content', err);
    setError(true);
  };

  // 主加载链路：加载样本 chunk 并测量每页字数
  const measure = async () => {
    const gen = ++measureGen;
    const { height, width } = size();
    const typo = props.typography();
    // untracked: re-measuring on every page turn would loop forever
    const start = untrack(() => Math.max(0, props.offset - TXT_CHUNK_SIZE));
    await props.source.ensureRange(start, start + 2 * TXT_CHUNK_SIZE);
    // 等待期间已有更新的测量启动，本次结果作废
    if (gen !== measureGen) return;
    const sample = props.displayText(
      props.source.slice(start, start + 2 * TXT_CHUNK_SIZE),
    );
    if (!sample) return;
    const measured = measureCharsPerPage(
      sample,
      typo,
      width - 32,
      height - PAGE_INDICATOR_SPACE_PX,
    );
    // 2% safety: dense-paragraph text measures taller than the sample
    setCharsPerPage(Math.max(50, Math.floor(measured * 0.98)));
    setLoading(false);
    props.onInit();
  };

  const retry = () => {
    setError(false);
    // 成功后 bump version，让分页文本基于已加载的 chunk 重算
    void measure()
      .then(() => setVersion(v => v + 1))
      .catch(onLoadError);
  };

  // measure chars per page whenever typography / viewport changes
  createEffect(() => {
    const { height, width } = size();
    props.typography();
    // tracking: converted text changes how much fits a page
    props.displayText('x');
    if (width < 50 || height < 50) return;
    // 拖动滑块时每个 tick 都会触发，去抖后只测量最后一组参数
    clearTimeout(measureTimer);
    measureTimer = setTimeout(() => {
      void measure().catch(onLoadError);
    }, MEASURE_DEBOUNCE_MS);
  });
  onCleanup(() => {
    clearTimeout(measureTimer);
  });

  const totalPages = createMemo(() => {
    const cpp = charsPerPage();
    if (cpp <= 0) return 1;
    return Math.max(1, Math.ceil(props.book.total_chars / cpp));
  });

  const page = createMemo(() => {
    const cpp = charsPerPage();
    if (cpp <= 0) return 0;
    return Math.min(totalPages() - 1, Math.floor(props.offset / cpp));
  });

  // prev / current / next page 的原文切片（起点偏移供段落定位用）
  const pageSlices = createMemo((): { raw: string; start: number }[] => {
    const cpp = charsPerPage();
    version(); // recompute after loads
    if (cpp <= 0) return [];
    const current = page();
    return [-1, 0, 1].map(delta => {
      const p = current + delta;
      if (p < 0 || p >= totalPages()) return { raw: '', start: 0 };
      const start = p * cpp;
      return {
        raw: props.source.slice(
          start,
          Math.min(start + cpp, props.book.total_chars),
        ),
        start,
      };
    });
  });

  // keep the visible trio loaded
  createEffect(() => {
    const cpp = charsPerPage();
    if (cpp <= 0) return;
    const current = page();
    const start = Math.max(0, (current - 1) * cpp);
    const end = Math.min((current + 2) * cpp, props.book.total_chars);
    void props.source
      .ensureRange(start, end)
      .then(() => setVersion(v => v + 1))
      .catch(onLoadError);
  });

  const canPrev = () => page() > 0;
  const canNext = () => page() < totalPages() - 1;

  // 手势与翻页动画状态（共享 strip 模型）
  const gestures = useStripFlip({
    bounds: () => container?.getBoundingClientRect() ?? null,
    canNext,
    canPrev,
    interceptTap: props.interceptMarkTap,
    onCommit: dir => {
      const cpp = charsPerPage();
      const next = page() + dir;
      props.onOffsetChange(Math.min(next * cpp, props.book.total_chars));
    },
    onTapCenter: () => {
      props.onTapCenter();
    },
  });

  // 键盘切章：相邻章节的起始位置（全书连续分页，章界只是 offset 标记）
  const jumpChapter = (dir: -1 | 1): void => {
    const next = chapterIndexAt(props.book.chapters, props.offset) + dir;
    if (next < 0) return;
    const target = props.book.chapters.at(next);
    if (target) props.onOffsetChange(target.position);
  };

  onMount(() => {
    const detachKeys = bindReaderKeys(document, {
      chapter: jumpChapter,
      page: gestures.flip,
    });
    onCleanup(detachKeys);
  });

  const typo = () => props.typography();

  return (
    <div
      class="relative mx-auto h-full w-full overflow-hidden"
      onClick={gestures.onClick}
      onTouchEnd={gestures.onTouchEnd}
      onTouchMove={gestures.onTouchMove}
      onTouchStart={gestures.onTouchStart}
      ref={container}
      style={{
        'max-width': `${props.readingWidth()}em`,
        'touch-action': 'pan-y',
      }}
    >
      <Show when={!loading() && charsPerPage() > 0}>
        <div
          class="flex h-full"
          classList={{
            'transition-transform duration-200 ease-out':
              gestures.transitioning(),
          }}
          onTransitionEnd={gestures.onTransitionEnd}
          style={{ transform: gestures.stripTransform(), width: '300%' }}
        >
          <Index each={pageSlices()}>
            {slice => (
              <div
                class="h-full w-1/3 overflow-hidden px-4"
                style={contentStyle(typo())}
              >
                <PageContent
                  displayText={props.displayText}
                  highlights={() =>
                    highlightsInWindow(
                      props.annotations(),
                      0,
                      props.book.total_chars,
                    )
                  }
                  page={slice()}
                  typo={typo()}
                />
              </div>
            )}
          </Index>
        </div>
        <div class="absolute bottom-1 left-0 w-full text-center text-xs opacity-50">
          {t('readerPageOf', { page: page() + 1, total: totalPages() })}
        </div>
      </Show>
      <Show when={loading() && !error()}>
        <div class="flex h-full items-center justify-center text-sm opacity-60">
          {t('loading')}
        </div>
      </Show>
      <Show when={error()}>
        <div class="absolute inset-0 flex items-center justify-center">
          <LoadErrorPanel
            onBack={() => {
              navigate(`/book/${props.book.book_id}`);
            }}
            onRetry={retry}
          />
        </div>
      </Show>
    </div>
  );
};

// 单页正文：原文按 '\n' 拆段（保留原文偏移），逐段转换展示并叠加高亮分段
const PageContent = (props: {
  displayText: (text: string) => string;
  highlights: () => HighlightRange[];
  page: { raw: string; start: number };
  typo: PageTypography;
}) => {
  const paragraphs = createMemo(() =>
    splitParagraphs(props.page.raw, props.page.start),
  );
  return (
    <Index each={paragraphs()}>
      {para => (
        <p
          data-indent={para().indent}
          data-pos={para().offset}
          style={{
            margin: `0 0 ${props.typo.paragraphSpacing}em`,
            'text-indent': `${props.typo.textIndent}em`,
            'white-space': 'pre-wrap',
            'word-break': 'break-word',
          }}
        >
          <ParagraphText
            displayText={props.displayText}
            highlights={props.highlights}
            paragraph={para()}
          />
        </p>
      )}
    </Index>
  );
};

// ---------------------------------------------------------------------------
// scroll mode
// ---------------------------------------------------------------------------

const ScrollView = (
  props: ViewProps & {
    initialOffset: number;
    jumpRequest: () => null | number;
    onInit: () => void;
    /** 跳转请求已受理并开始执行；参数在其完成后以成败 resolve */
    onJumpHandled: (jump: Promise<boolean>) => void;
    onOffsetChange: (at: number) => void;
  },
) => {
  const { t } = useI18n();
  const navigate = useNavigate();
  let container: HTMLDivElement | undefined;

  // --- 章节作用域：滚动模式一次只渲染一章（硬切换） ---
  const [chapterIdx, setChapterIdx] = createSignal(
    chapterIndexAt(props.book.chapters, props.initialOffset),
  );
  const chapterRange = () =>
    chapterBounds(props.book.chapters, chapterIdx(), props.book.total_chars);
  // 窗口允许的 chunk 边界（end 为 exclusive 上限）
  const chapterChunkBounds = () => {
    const range = chapterRange();
    return {
      maxEnd: Math.ceil(range.end / TXT_CHUNK_SIZE),
      minStart: Math.floor(range.start / TXT_CHUNK_SIZE),
    };
  };

  // 当前章节内可渲染的高亮（章节区间 → 窗口裁剪）
  const chapterHighlights = createMemo(() => {
    const range = chapterRange();
    return highlightsInWindow(props.annotations(), range.start, range.end);
  });

  // 已加载的 chunk 窗口 [start, end)：双向滑动且有上限（不越出当前章），
  // 避免 DOM 无界增长
  const firstChunk = Math.floor(props.initialOffset / TXT_CHUNK_SIZE);
  const [win, setWin] = createSignal<ScrollWindow>({
    end: firstChunk + 1,
    start: firstChunk,
  });
  const [version, setVersion] = createSignal(0);
  const [loadingMore, setLoadingMore] = createSignal(false);
  const [error, setError] = createSignal(false);
  // 视口是否停在当前章的边界（用于边界提示显隐）
  const [atEdge, setAtEdge] = createSignal({ bottom: false, top: false });
  // 每个 chunk 的段落元素，按 chunk 索引分组；查找时按窗口顺序拼接
  const chunkEls = new Map<number, HTMLElement[]>();

  // 窗口内的 chunk 索引：只依赖窗口本身，chunk 内容变化由各 chunk 组件自行跟踪
  const chunkIndices = createMemo(() => {
    const { end, start } = win();
    const out: number[] = [];
    for (let i = start; i < end; i++) out.push(i);
    return out;
  });

  // 缓存每个 chunk 的段落拆分结果：文本未变时复用同一数组引用，
  // 配合 <For> 的 key 让未变化的 chunk 完全不重渲染
  const paraCache = new Map<
    number,
    { paras: SplitParagraph[]; text: string }
  >();

  const chunkParagraphs = (index: number): SplitParagraph[] => {
    version(); // chunk 数据到达后重算
    const start = index * TXT_CHUNK_SIZE;
    const text = props.source.slice(
      start,
      Math.min(start + TXT_CHUNK_SIZE, props.book.total_chars),
    );
    const cached = paraCache.get(index);
    if (cached && cached.text === text) return cached.paras;
    const { end: chapterEnd, start: chapterStart } = chapterRange();
    // 章节首尾的 chunk 会跨章，范围外的段落直接丢弃
    const paras = splitParagraphs(text, start).filter(
      para => para.offset >= chapterStart && para.offset < chapterEnd,
    );
    paraCache.set(index, { paras, text });
    return paras;
  };

  // 窗口滑动后回收缓存，避免占用随书籍体积增长
  createEffect(() => {
    const { end, start } = win();
    for (const key of paraCache.keys()) {
      if (key < start || key >= end) paraCache.delete(key);
    }
  });

  const ensureWindow = async () => {
    const { end, start } = win();
    await props.source.ensureRange(
      start * TXT_CHUNK_SIZE,
      Math.min(end * TXT_CHUNK_SIZE, props.book.total_chars),
    );
    setVersion(v => v + 1);
  };

  const onLoadError = (err: unknown) => {
    console.error('Failed to load txt content', err);
    setError(true);
  };

  // 跳转与初始化串行执行：并发 jumpTo 会竞争 scrollTop 与窗口状态
  // （如深链与 init() 同时触发时定位互相覆盖）
  let jumpChain: Promise<boolean> = Promise.resolve(false);

  // 跳转/初始化失败统一进错误态；job 永不 reject
  const settle = (err: unknown): boolean => {
    onLoadError(err);
    return false;
  };

  const enqueueJump = (
    at: number,
    land: 'bottom' | 'top' = 'top',
  ): Promise<boolean> => {
    const run = (): Promise<boolean> =>
      jumpTo(at, land).then(() => true, settle);
    // 链上前序失败不影响本次执行
    const job = jumpChain.then(run, run);
    jumpChain = job;
    return job;
  };

  // 主加载链路：失败进入错误态（可重试），而不是永远空白
  const init = async () => {
    await ensureWindow();
    await jumpTo(props.initialOffset);
    props.onInit();
  };

  const runInit = () => {
    jumpChain = init().then(() => true, settle);
  };

  const retry = () => {
    setError(false);
    runInit();
  };

  onMount(() => {
    runInit();
    // 贴边跟踪独立于 scroll 事件：短章不足一屏时从不滚动，也要能出边界提示
    if (container) {
      const detach = bindScrollEdges(container, setAtEdge);
      onCleanup(() => {
        detach();
        cancelAnimationFrame(scrollRaf);
      });
    }
    // 键盘：空格/enter/上下键章内滚动（贴边切章），左右方向键直接切章
    const detachKeys = bindReaderKeys(document, {
      chapter: switchChapter,
      page: pageScroll,
    });
    onCleanup(detachKeys);
  });

  // jumps requested via the navigator (bookmark / chapter / cross-device sync)
  createEffect(() => {
    const at = props.jumpRequest();
    if (at !== null) {
      props.onJumpHandled(enqueueJump(at));
    }
  });

  async function jumpTo(
    at: number,
    land: 'bottom' | 'top' = 'top',
  ): Promise<void> {
    // 跳转目标可能属于另一章：切换章节作用域并重算段落过滤
    const idx = chapterIndexAt(props.book.chapters, at);
    if (idx !== chapterIdx()) {
      setChapterIdx(idx);
      paraCache.clear();
    }
    const chunk = Math.floor(at / TXT_CHUNK_SIZE);
    // 远距离跳转时重置窗口到目标附近，避免窗口被拉得过大；
    // 窗口不越出当前章（end 为 exclusive 上限）
    const next = jumpWindow(win(), chunk, chapterChunkBounds().maxEnd);
    if (next !== win()) setWin(next);
    await ensureWindow();
    await settleLanding(at, land);
    // 落点窗口内容不足一屏（章尾紧贴 chunk 边界、自定义正则切出的短章
    // 等）时容器不可滚动、不产生 scroll 事件，onScroll 按滚动阈值触发的
    // 增量加载永远不会执行：沿衔接阅读的方向主动扩窗，直到可滚动或抵达
    // 本章边界（窗口不再变化 = 整章已渲染）
    while (container && container.scrollHeight <= container.clientHeight) {
      const before = win();
      // 串行扩窗：每轮需等取数与落点定位完成后，依最新布局重新判断
      // oxlint-disable-next-line no-await-in-loop
      await (land === 'bottom' ? loadPrev() : loadNext());
      if (win() === before) break;
      // oxlint-disable-next-line no-await-in-loop
      await settleLanding(at, land);
    }
    // 落点不产生滚动（短章不足一屏、落点与当前滚动位置相同）时 onScroll
    // 不会触发，位置信号停留在旧章：按落点视口直接上报
    const landed = container
      ? findParagraphElAtTop(container.scrollTop)
      : undefined;
    props.onOffsetChange(landed ? Number(landed.dataset.pos) : at);
  }

  // 定位落点：bottom 落到章末衔接阅读顺序，top 落到目标段落顶。
  // 等 scrollTop 落定后再返回，让调用方（心跳挂起/恢复）拿到确定状态
  function settleLanding(at: number, land: 'bottom' | 'top'): Promise<void> {
    return new Promise<void>(resolve => {
      requestAnimationFrame(() => {
        if (container) {
          // content-visibility: auto 把视口外段落折叠成单行占位，直接读
          // offsetTop / scrollHeight 拿到的是塌缩布局的值，恢复位置会
          // 回退数页；scrollIntoView 会展开目标沿途段落并精确计算真实
          // 落点（跳转目标是 cv 元素时视为 relevant to the user）
          if (land === 'bottom') {
            const last = paragraphElsInOrder().at(-1);
            if (last) last.scrollIntoView({ block: 'end' });
            else container.scrollTop = container.scrollHeight;
          } else {
            const target = findParagraphEl(at);
            if (target) {
              target.scrollIntoView({ block: 'start' });
              container.scrollTop = Math.max(
                0,
                container.scrollTop - JUMP_TOP_INSET_PX,
              );
            }
          }
        }
        resolve();
      });
    });
  }

  // 按窗口顺序拼接所有段落的 DOM 元素（各 chunk 组内保持 DOM 顺序）
  function paragraphElsInOrder(): HTMLElement[] {
    const { end, start } = win();
    const out: HTMLElement[] = [];
    for (let i = start; i < end; i++) {
      const els = chunkEls.get(i);
      if (els) out.push(...els);
    }
    return out;
  }

  // 最后一个 offset <= at 的段落
  function findParagraphEl(at: number): HTMLElement | undefined {
    return lastMatching(
      paragraphElsInOrder(),
      el => Number(el.dataset.pos) <= at,
    );
  }

  function findParagraphElAtTop(scrollTop: number): HTMLElement | undefined {
    if (!container) return undefined;
    const base = container.offsetTop;
    return lastMatching(
      paragraphElsInOrder(),
      // 容差与 JUMP_TOP_INSET_PX 对齐：跳转落点（段落顶距视口顶留白
      // 12px）必须能读回该段落本身，否则每次恢复都回退一段
      el => el.offsetTop - base <= scrollTop + JUMP_TOP_INSET_PX,
    );
  }

  let scrollRaf = 0;
  const onScroll = () => {
    cancelAnimationFrame(scrollRaf);
    scrollRaf = requestAnimationFrame(() => {
      if (!container) return;
      const { clientHeight, scrollHeight, scrollTop } = container;
      // report the paragraph at the top of the viewport
      const el = findParagraphElAtTop(scrollTop);
      if (el) props.onOffsetChange(Number(el.dataset.pos));
      // infinite loading in both directions
      if (scrollTop + clientHeight > scrollHeight - 1200) void loadNext();
      if (scrollTop < 600 && win().start > 0) void loadPrev();
    });
  };

  // 视口顶部段落作为锚点：窗口滑动前捕获，滑动后据此恢复滚动位置
  function captureAnchor(): null | ScrollAnchor {
    if (!container) return null;
    const el = findParagraphElAtTop(container.scrollTop);
    if (!el) return null;
    return {
      delta: container.scrollTop - (el.offsetTop - container.offsetTop),
      pos: Number(el.dataset.pos),
    };
  }

  function restoreAnchor(anchor: null | ScrollAnchor): void {
    if (!container || !anchor) return;
    const el = findParagraphEl(anchor.pos);
    if (el) {
      container.scrollTop = Math.max(
        0,
        el.offsetTop - container.offsetTop + anchor.delta,
      );
    }
  }

  // 窗口滑动的原子提交：换窗、锚点补偿与滚动动画坐标平移在同一同步块
  // 内完成，中间无绘制帧，视口内文字保持不动
  function slideWindow(next: ScrollWindow, anchor: null | ScrollAnchor): void {
    if (!container) return;
    const scrollTopBefore = container.scrollTop;
    setWin(next);
    restoreAnchor(anchor);
    // 章内翻页动画每帧按旧坐标绝对写回 scrollTop，补偿平移后需跟随平移
    shiftRunningScroll(container, container.scrollTop - scrollTopBefore);
  }

  async function loadNext(): Promise<void> {
    if (loadingMore() || !container) return;
    const prev = win();
    // 窗口不越出当前章：到章尾 chunk 即停止扩展（硬切章由点击分区触发）
    const { maxEnd } = chapterChunkBounds();
    if (prev.end >= maxEnd) return;
    setLoadingMore(true);
    try {
      // 先取数后换窗：保证提交的原子性（见 slideWindow）；
      // 失败时窗口未动，下一次滚动触发时会重试
      await props.source.ensureRange(
        prev.end * TXT_CHUNK_SIZE,
        Math.min((prev.end + 1) * TXT_CHUNK_SIZE, props.book.total_chars),
      );
    } catch (err) {
      console.error('Failed to load txt chunk', err);
      setLoadingMore(false);
      return;
    }
    // 取数期间窗口被跳转重置：放弃本次滑动
    if (win() !== prev) {
      setLoadingMore(false);
      return;
    }
    // 视口已完全越过顶部 chunk 时才回收它，否则允许窗口暂时超限
    const anchor = captureAnchor();
    const canDropTop =
      anchor !== null && Math.floor(anchor.pos / TXT_CHUNK_SIZE) > prev.start;
    slideWindow(extendWindowDown(prev, maxEnd, canDropTop), anchor);
    setLoadingMore(false);
  }

  async function loadPrev(): Promise<void> {
    if (loadingMore() || !container) return;
    const prev = win();
    // 窗口不越出当前章：到章首 chunk 即停止扩展
    const { minStart } = chapterChunkBounds();
    if (prev.start <= minStart) return;
    setLoadingMore(true);
    try {
      await props.source.ensureRange(
        (prev.start - 1) * TXT_CHUNK_SIZE,
        prev.start * TXT_CHUNK_SIZE,
      );
    } catch (err) {
      console.error('Failed to load txt chunk', err);
      setLoadingMore(false);
      return;
    }
    if (win() !== prev) {
      setLoadingMore(false);
      return;
    }
    // 视口底部未进入末尾 chunk 时才回收它，否则允许窗口暂时超限
    const bottomEl = findParagraphElAtTop(
      container.scrollTop + container.clientHeight,
    );
    const canDropBottom =
      bottomEl !== undefined &&
      Math.floor(Number(bottomEl.dataset.pos) / TXT_CHUNK_SIZE) < prev.end - 1;
    slideWindow(extendWindowUp(prev, minStart, canDropBottom), captureAnchor());
    setLoadingMore(false);
  }

  // --- 点击分区翻页：上/下 1/3 章内滚动 PAGE_SCROLL_RATIO 视口，
  // 章节边界处硬切章；中区打开 chrome（滚动本身交给原生） ---
  function pageScroll(dir: -1 | 1): void {
    if (!container) return;
    const edges = scrollEdges(container);
    if (dir > 0 ? edges.bottom : edges.top) {
      switchChapter(dir);
      return;
    }
    const { clientHeight } = container;
    animateScrollBy(container, dir * clientHeight * PAGE_SCROLL_RATIO);
  }

  // 下一章从头显示，上一章落到章末衔接阅读顺序；首末章越界时保持原章。
  // 经串行跳转链执行：失败进错误态，且不会留下未捕获的 rejection
  function switchChapter(dir: -1 | 1): void {
    const next = chapterIdx() + dir;
    if (next < 0 || next >= props.book.chapters.length) return;
    const range = chapterBounds(
      props.book.chapters,
      next,
      props.book.total_chars,
    );
    void enqueueJump(
      dir > 0 ? range.start : range.end - 1,
      dir > 0 ? 'top' : 'bottom',
    );
  }

  // 边界提示只在存在相邻章节时显示，避免误导
  const hasNextChapter = () => chapterIdx() + 1 < props.book.chapters.length;
  const hasPrevChapter = () =>
    props.book.chapters.length > 0 && chapterIdx() > 0;

  // 点击分区（触摸走 touchend，桌面走 click 门控）：上/下 1/3 章内滚动
  const tapZones = useVerticalTapZones({
    bounds: () => container?.getBoundingClientRect() ?? null,
    interceptTap: props.interceptMarkTap,
    onPageScroll: pageScroll,
    onTapCenter: props.onTapCenter,
  });

  const typo = () => props.typography();

  return (
    <Show
      fallback={
        <div class="flex h-full items-center justify-center">
          <LoadErrorPanel
            onBack={() => {
              navigate(`/book/${props.book.book_id}`);
            }}
            onRetry={retry}
          />
        </div>
      }
      when={!error()}
    >
      <div class="relative h-full w-full">
        {/* 滚动容器全宽让滚动条贴屏幕右缘，列宽由内层限制并居中 */}
        <div
          class="h-full w-full overflow-y-auto"
          onClick={tapZones.onClick}
          onMouseDown={tapZones.onMouseDown}
          onScroll={onScroll}
          onTouchEnd={tapZones.onTouchEnd}
          onTouchStart={tapZones.onTouchStart}
          ref={container}
        >
          <div
            class="mx-auto w-full px-4 py-6"
            style={{
              'max-width': `${props.readingWidth()}em`,
              ...contentStyle(typo()),
            }}
          >
            <For each={chunkIndices()}>
              {index => (
                <ChunkParagraphs
                  chunkEls={chunkEls}
                  displayText={props.displayText}
                  getParagraphs={chunkParagraphs}
                  highlights={chapterHighlights}
                  index={index}
                  typo={typo}
                />
              )}
            </For>
            <Show when={loadingMore()}>
              <div class="py-4 text-center text-sm opacity-50">
                {t('loading')}
              </div>
            </Show>
          </div>
        </div>
        <PageHint
          direction="down"
          visible={atEdge().bottom && hasNextChapter()}
        />
        <PageHint direction="up" visible={atEdge().top && hasPrevChapter()} />
      </div>
    </Show>
  );
};
