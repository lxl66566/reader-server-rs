// EPUB reader: epub.js rendition with theme sync, swipe/tap gestures and
// locator = EpubCFI, percent via generated locations.

import { useNavigate } from '@solidjs/router';
import ePub, { type Book, type Rendition } from 'epubjs';
import {
  createEffect,
  createSignal,
  on,
  onCleanup,
  onMount,
  Show,
} from 'solid-js';

import { bookApi } from '../api';
import { LoadErrorPanel } from '../components/LoadErrorPanel';
import { Spinner } from '../components/ui';
import { fetchBookFile } from '../offline/api';
import { fontFamilyCss, useSettings } from '../stores/settings';
import {
  type ConvertDirection,
  convertChinese,
  convertDirection,
  loadChineseConverter,
} from '../utils/opencc';
import {
  estimatePercent,
  LOCATIONS_TIMEOUT_MS,
  settlesWithin,
} from './epubProgress';
import { sanitizeEpubArchive } from './epubSanitize';
import {
  type TapBounds,
  animateScrollBy,
  bindScrollEdges,
  scrollEdges,
  attachTapSwipeGestures,
  isTextEntryTarget,
  PAGE_SCROLL_RATIO,
  tapZone,
  tapZoneVertical,
} from './gestures';
import PageHint from './PageHint';
import { stripEpubParagraphIndent } from './paragraph';
import type { DeepLink, FormatReaderProps } from './types';

// epub.js 的 Spine 类型未声明 spineItems（章节数组），运行时存在；
// 收窄类型以取得章节数，供降级 percent 估算用
const spineLengthOf = (b: Book): number =>
  (b.spine as Book['spine'] & { spineItems?: unknown[] }).spineItems?.length ??
  0;

// 阅读主题强制覆盖的文本元素：书自带的直接规则（如 p { color:#000 !important }）
// 会击败 body 上的颜色继承，必须以同等 !important 逐元素压制才能跟随用户主题
const TEXT_COLOR_SELECTOR =
  'body, p, li, dd, dt, blockquote, div, span, a, h1, h2, h3, h4, h5, h6, ' +
  'td, th, pre, figcaption, strong, em, b, i, small, code, time';

/** hex 颜色是否为暗色（加权亮度 < 0.5）；非 hex 格式按暗色处理 */
function isDarkColor(hex: string): boolean {
  const m = /^#([0-9a-f]{6})$/i.exec(hex);
  if (!m) return true;
  const n = Number.parseInt(m[1] ?? '000000', 16);
  const lum =
    (0.299 * ((n >> 16) & 0xff) +
      0.587 * ((n >> 8) & 0xff) +
      0.114 * (n & 0xff)) /
    255;
  return lum < 0.5;
}

const EpubReader = (props: FormatReaderProps) => {
  const { settings } = useSettings();
  const navigate = useNavigate();
  let container: HTMLDivElement | undefined;
  let rootRef: HTMLDivElement | undefined;

  let book: Book | null = null;
  let rendition: null | Rendition = null;
  // 缓存首次下载的书籍文件：切换翻页模式重建 rendition 时复用，
  // 避免每次切换都重新下载整本 epub（可达几十 MB）
  let cachedData: ArrayBuffer | null = null;
  const [ready, setReady] = createSignal(false);
  const [error, setError] = createSignal(false);
  // 视口是否停在当前章边界 + 当前 spine 下标（边界提示显隐用）
  const [atEdge, setAtEdge] = createSignal({ bottom: false, top: false });
  const [sectionIndex, setSectionIndex] = createSignal<null | number>(null);
  let pendingLocator: null | string = null;
  let locationsReady = false;
  // locations 超时/失败后的降级模式：percent 按 spine 章节中点估算
  let percentFallback = false;
  let lastCfi: null | string = null;
  let deepLinkConsumed = false;
  let detachScrollEdge: (() => void) | null = null;

  onMount(() => {
    void runSetup();
    onCleanup(() => {
      setReady(false);
      detachScrollEdge?.();
      detachScrollEdge = null;
      book?.destroy();
      book = null;
      rendition = null;
    });
  });

  // 主加载链路的统一入口：失败时进入错误态而不是永远停在加载态。
  // startAt：重建（切换翻页模式）时传入跳转前的当前位置，避免回退到旧进度
  async function runSetup(startAt?: string): Promise<void> {
    setError(false);
    detachScrollEdge?.();
    detachScrollEdge = null;
    setAtEdge({ bottom: false, top: false });
    setSectionIndex(null);
    locationsReady = false; // 重建后 locations 需重新生成
    percentFallback = false;
    try {
      await setup(startAt);
    } catch (err) {
      console.error('Failed to load epub book', err);
      // 销毁半成品实例，让重试从干净状态开始
      book?.destroy();
      book = null;
      rendition = null;
      setReady(false);
      setError(true);
    }
  }

  // 深链解析：定位直接用 locator；章节经后端换算为 spine 下标后取 href
  async function resolveDeepLink(link: DeepLink): Promise<null | string> {
    if (link.kind === 'locator') return link.locator;
    try {
      const res = await bookApi.jumpToChapter(
        props.book.book_id,
        link.chapterId,
      );
      const section = book?.spine.get(res.position);
      return section ? section.href : null;
    } catch {
      return null; // 解析失败回退到保存的进度
    }
  }

  async function setup(startAt?: string): Promise<void> {
    if (!container) return;
    // 修复部分电子书（XHTML 允许、HTML 解析器不认）自闭合的 raw-text 标签，
    // 否则章节正文会被 script 标签吞掉，整章渲染为空白
    cachedData ??= await sanitizeEpubArchive(
      // 断网时回退显式缓存的整本文件
      await fetchBookFile(props.book.book_id),
    );
    // 每次重建都切一份副本给 epub.js，避免其实例污染共享缓存
    book = ePub(cachedData.slice(0));
    await book.ready;

    // 首次加载消费深链（章节/定位），优先于保存的进度
    let start = startAt ?? null;
    if (!deepLinkConsumed && props.deepLink) {
      deepLinkConsumed = true;
      start = (await resolveDeepLink(props.deepLink)) ?? start;
    }
    const savedCfi =
      props.initialLocator?.includes('epubcfi') === true
        ? props.initialLocator
        : null;
    const target = start ?? savedCfi;
    // 无任何 cfi 起点（首次加载且未存进度）时，locations 生成后按 percent 落位
    const honorBarePercent = target === null;

    const scrolled = settings.pagination_mode === 'scroll';
    rendition = book.renderTo(container, {
      flow: scrolled ? 'scrolled-doc' : 'paginated',
      height: '100%',
      // manager default：滚动模式下也只渲染一章，滚动发生在 manager 容器上，
      // 章节边界由翻页手势硬切换（continuous 会把后续章节拼接在下方）
      manager: 'default',
      width: '100%',
    });

    registerTheme();
    registerContentHooks();

    rendition.on(
      'relocated',
      (location: { start: { cfi: string; index: number } }) => {
        lastCfi = location.start.cfi;
        setSectionIndex(location.start.index);
        reportPosition();
      },
    );

    if (target) {
      try {
        await rendition.display(target);
      } catch {
        // 保存的 cfi 可能已过期（如书籍重新处理过）：回退到起始位置
        await rendition.display();
      }
    } else {
      await rendition.display();
    }

    if (scrolled) watchScrollEdge();
    setReady(true);
    if (pendingLocator) {
      await displaySafe(pendingLocator);
      pendingLocator = null;
    }

    // locations give us percent mapping; generate with a timeout so that
    // huge books (慢或失败) degrade to spine-based estimates instead of
    // blocking progress reporting forever
    generateLocationsWithFallback(book, honorBarePercent);

    reportPosition();
  }

  /**
   * locations 生成（带超时降级）。一次性判定：超时/失败即转入 spine
   * 估算且本会话不再升级回精确换算，避免 percent 中途跳变；期间
   * rendition 重建（book 实例更换）则本次结果作废。
   */
  function generateLocationsWithFallback(
    target: Book,
    honorBarePercent: boolean,
  ): void {
    void settlesWithin(
      target.locations.generate(1600),
      LOCATIONS_TIMEOUT_MS,
    ).then(ok => {
      if (book !== target) return;
      if (ok) {
        locationsReady = true;
        // honor a bare percent progress when no cfi was saved
        if (honorBarePercent && props.book.percent > 0) {
          const cfi = target.locations.cfiFromPercentage(props.book.percent);
          void displaySafe(cfi);
        }
      } else {
        // 生成超时/失败：降级为 spine 估算，心跳不中断（精度退化到章级）；
        // 无法换算 cfi，honorBarePercent 的按 percent 落位只能放弃
        percentFallback = true;
      }
      reportPosition();
    });
  }

  async function displaySafe(target: string): Promise<boolean> {
    if (!rendition) return false;
    try {
      await rendition.display(target);
      return true;
    } catch {
      // stale locator (e.g. from another epub build): stay where we are
      return false;
    }
  }

  function reportPosition(): void {
    if (!lastCfi || !book) return;
    let percent: number;
    if (locationsReady) {
      try {
        percent = book.locations.percentageFromCfi(lastCfi);
      } catch {
        return; // cfi 超出 locations 范围：本次无法给出可靠 percent
      }
    } else if (percentFallback) {
      // 降级：locations 不可用，按当前章 spine 中点估算
      const idx = sectionIndex();
      if (idx === null) return;
      percent = estimatePercent(idx, spineLengthOf(book));
    } else {
      // locations 仍在生成：跳过本次上报（心跳暂停由 Reader 处理），
      // 避免把页面加载时的旧 percent 写进跨设备同步
      return;
    }
    props.onPosition(lastCfi, Math.min(1, Math.max(0, percent)));
  }

  // --- theming: mirror ReadingSettings into the iframe ---
  function registerTheme(): void {
    if (!rendition) return;
    // scrolled 模式下宿主容器全宽（滚动条贴屏幕右缘），正文列宽
    // 改由 iframe body 的 max-width 限制并居中
    const scrolled = settings.pagination_mode === 'scroll';
    rendition.themes.register('custom', {
      body: {
        background: 'transparent',
        'font-family': fontFamilyCss(settings.font_family) || 'inherit',
        padding: '0 16px',
        ...(scrolled
          ? {
              // !important：书籍自带样式常重置 body margin，需强制覆盖
              margin: '0 auto !important',
              'max-width': `${settings.reading_width}em !important`,
            }
          : {}),
      },
      // 书籍自带样式常见 !important 黑字/白底（p{color:#000}、h3{background:#f5f5f5}），
      // 与阅读主题叠加后在暗色背景下不可读：文字色逐元素压制、
      // 背景色全量透明（img/image 除外），让正文完全跟随用户设置
      'body *:not(img):not(image)': {
        'background-color': 'transparent !important',
      },
      // iframe 文档 color-scheme 缺省为 light：html/body 透明时 Chrome 会
      // 透出白色 root canvas，暗色阅读背景下表现为正文区域整块发白
      html: {
        'color-scheme': isDarkColor(settings.background_color)
          ? 'dark'
          : 'light',
      },
      p: {
        'line-height': `${settings.line_height} !important`,
        'margin-bottom': `${settings.paragraph_spacing}em !important`,
        'text-indent': `${settings.text_indent}em !important`,
      },
      [TEXT_COLOR_SELECTOR]: {
        color: `${settings.text_color} !important`,
      },
    });
    rendition.themes.select('custom');
    rendition.themes.override('font-size', `${settings.font_size}px`, true);
    rendition.themes.override(
      'letter-spacing',
      `${settings.letter_spacing}px`,
      true,
    );
  }

  // re-apply when settings change (color/font/size/width)
  createEffect(
    on(
      () => [
        settings.text_color,
        settings.font_family,
        settings.font_size,
        settings.line_height,
        settings.letter_spacing,
        settings.paragraph_spacing,
        settings.text_indent,
        settings.reading_width,
        settings.pagination_mode,
      ],
      () => {
        if (rendition) registerTheme();
      },
      { defer: true },
    ),
  );

  // --- content hooks: gestures inside the iframe + optional conversion ---
  function registerContentHooks(): void {
    if (!rendition) return;
    rendition.hooks.content.register((contents: { document: Document }) => {
      // 主题对 p 强制 text-indent，剥离书源自带的段首字符缩进避免叠加
      stripEpubParagraphIndent(contents.document);
      void loadChineseConverter(
        convertDirection(settings.chinese_conversion),
      ).then(() => {
        if (!rendition) return;
        // 应用时重新读当前设置，避免加载期间切换方向后应用了过期方向
        convertTree(
          contents.document,
          convertDirection(settings.chinese_conversion),
        );
      });
      // iframe 内的事件不会冒泡到父窗口：转发交互信号给宿主的活跃检测，
      // 否则分页模式点按翻页 60s 后会被判为不活跃、心跳停发
      const forwardActivity = () => props.onActivity?.();
      for (const event of ['pointerdown', 'touchstart', 'keydown'] as const) {
        contents.document.addEventListener(event, forwardActivity, {
          passive: true,
        });
      }
      attachGestures(contents.document);
    });
  }

  // 切换简繁：对已渲染的 iframe 文档就地还原原文再按新方向转换，
  // 不销毁 rendition，避免 clear()+start() 造成的白屏闪烁
  createEffect(
    on(
      () => settings.chinese_conversion,
      mode => {
        if (!rendition || !ready()) return;
        const direction = convertDirection(mode);
        void loadChineseConverter(direction).then(() => {
          for (const view of rendition?.views() ?? []) {
            // epub.js 的 View 类型未暴露 contents，但运行时存在
            const doc = (view as { contents?: { document?: Document } })
              .contents?.document;
            if (doc?.body) convertTree(doc, direction);
          }
        });
      },
      { defer: true },
    ),
  );

  // 每个文档首次转换前缓存原文文本节点：来回切换时始终从原文出发，
  // 避免 s2t/t2s 叠加转换造成文字劣化
  const originalTexts = new WeakMap<Document, Map<Text, string>>();

  function convertTree(
    doc: Document,
    direction: ConvertDirection | null,
  ): void {
    let originals = originalTexts.get(doc);
    if (!originals) {
      originals = new Map();
      originalTexts.set(doc, originals);
    }
    const walker = doc.createTreeWalker(doc.body, NodeFilter.SHOW_TEXT);
    let node = walker.nextNode();
    while (node) {
      const textNode = node as Text;
      if (!originals.has(textNode))
        originals.set(textNode, textNode.textContent);
      textNode.textContent = convertChinese(
        originals.get(textNode) ?? '',
        direction,
      );
      node = walker.nextNode();
    }
  }

  // --- gestures: swipe flips a page (paginate mode), tap zones flip/toggle ---
  function attachGestures(doc: Document): void {
    attachTapSwipeGestures(doc, {
      bounds: () => hostBoundsIn(doc),
      onFlip: dir => {
        void flip(dir);
      },
      onTapCenter: () => {
        props.onTapCenter();
      },
      paginate: () => settings.pagination_mode === 'paginate',
      scroll:
        settings.pagination_mode === 'scroll'
          ? {
              bounds: () => hostVerticalBoundsIn(doc),
              onPageScroll: pageScroll,
            }
          : undefined,
    });
  }

  // epub.js 会平移 iframe（分页模式翻列、滚动模式文档流），iframe 内事件的
  // clientX/Y 处于 iframe 坐标系；分区判定前须把宿主边界换算进同一坐标系，
  // 否则翻过一页后所有 tap 都会落入"右区"
  function hostBoundsIn(doc: Document): null | TapBounds {
    const host = container?.getBoundingClientRect();
    if (!host) return null;
    const frame = doc.defaultView?.frameElement?.getBoundingClientRect();
    if (!frame) return { left: host.left, right: host.right };
    return { left: host.left - frame.left, right: host.right - frame.left };
  }

  function hostVerticalBoundsIn(
    doc: Document,
  ): null | { bottom: number; top: number } {
    const host = container?.getBoundingClientRect();
    if (!host) return null;
    const frame = doc.defaultView?.frameElement?.getBoundingClientRect();
    if (!frame) return { bottom: host.bottom, top: host.top };
    return { bottom: host.bottom - frame.top, top: host.top - frame.top };
  }

  // 滚动模式 tap 翻页：贴边即切章，否则章内滚动 90% 视口。
  // - 贴边判定读容器实时状态而非 atEdge 信号——信号经 rAF 异步更新，
  //   切章后立刻点击会读到旧值而把切章误判成零位移滚动；
  // - 不用 hasPrev/NextChapter 门控——它依赖 relocated 维护的章节下标，
  //   新章节渲染完到 relocated 触发之间有窗口期，期间 tap 会被误判成
  //   章内滚动（零位移，点击无效）。spine 首尾的 next/prev 是 no-op，
  //   章界由 epub.js 兜底
  function pageScroll(dir: -1 | 1): void {
    const el = scrolledContainer();
    if (!el) return;
    const edges = scrollEdges(el);
    const edgeReached = dir > 0 ? edges.bottom : edges.top;
    if (edgeReached) {
      void flip(dir);
      return;
    }
    animateScrollBy(el, dir * el.clientHeight * PAGE_SCROLL_RATIO);
  }

  // iframe 之外的宿主区域（页边距、短章上下方）没有 iframe 手势，点击走
  // 同一套分区语义，避免出现"点了没反应"的死区
  const onHostTap = (event: MouseEvent) => {
    const rect = rootRef?.getBoundingClientRect();
    if (!rect) return;
    if (settings.pagination_mode === 'paginate') {
      const zone = tapZone(event.clientX, {
        left: rect.left,
        right: rect.right,
      });
      if (zone === 'left') {
        void flip(-1);
        return;
      }
      if (zone === 'right') {
        void flip(1);
        return;
      }
      props.onTapCenter();
      return;
    }
    const zone = tapZoneVertical(event.clientY, {
      bottom: rect.bottom,
      top: rect.top,
    });
    if (zone === 'top') {
      pageScroll(-1);
      return;
    }
    if (zone === 'bottom') {
      pageScroll(1);
      return;
    }
    props.onTapCenter();
  };

  // epub.js 运行时在 rendition 上挂载 manager 实例（类型未声明），
  // 收窄到滚动翻页所需的容器字段
  const scrolledContainer = (): HTMLDivElement | undefined =>
    (
      rendition as
        | null
        | undefined
        | { manager?: { container?: HTMLDivElement } }
    )?.manager?.container;

  // 跟踪 manager 容器贴边状态，维护章节边界提示的显隐（仅滚动模式）
  function watchScrollEdge(): void {
    detachScrollEdge?.();
    detachScrollEdge = null;
    const el = scrolledContainer();
    if (!el) return;
    // epub.js 不约束横向溢出（书内宽元素/封面图），滚动容器会出现横向
    // 滚动条；滚动模式只有纵向文档流，直接隐藏
    el.style.overflowX = 'hidden';
    detachScrollEdge = bindScrollEdges(el, setAtEdge);
  }

  // 边界提示只在存在相邻章节时显示，避免误导
  const hasNextChapter = () => {
    const idx = sectionIndex();
    return idx !== null && book !== null && idx < book.spine.last().index;
  };
  const hasPrevChapter = () => {
    const idx = sectionIndex();
    return idx !== null && idx > 0;
  };

  // rendition.next/prev 在章节渲染出错时会 reject：就地捕获记日志，
  // 停留当前页；手势与键盘路径共用，均不留未捕获的 rejection
  async function flip(dir: -1 | 1): Promise<void> {
    if (!rendition) return;
    try {
      if (dir > 0) await rendition.next();
      else await rendition.prev();
    } catch (err) {
      console.error('epub flip failed', err);
    }
  }

  // switching paginate/scroll needs a fresh rendition (flow is a creation option)
  createEffect(
    on(
      () => settings.pagination_mode,
      () => {
        if (!book) return; // initial setup reads the mode directly
        const current = lastCfi;
        book.destroy();
        book = null;
        rendition = null;
        setReady(false);
        // 以当前位置作为重建起点，避免 display(initialLocator) 短暂回退旧进度
        void runSetup(current ?? undefined);
      },
      { defer: true },
    ),
  );

  // desktop keyboard
  onMount(() => {
    const onKey = (event: KeyboardEvent) => {
      // 弹窗输入框内的方向键不翻页
      if (isTextEntryTarget(event.target)) return;
      if (settings.pagination_mode === 'paginate') {
        if (event.key === 'ArrowLeft') void flip(-1);
        if (event.key === 'ArrowRight') void flip(1);
      } else if (event.key === 'ArrowDown') {
        event.preventDefault();
        pageScroll(1);
      } else if (event.key === 'ArrowUp') {
        event.preventDefault();
        pageScroll(-1);
      }
    };
    window.addEventListener('keydown', onKey);
    onCleanup(() => {
      window.removeEventListener('keydown', onKey);
    });
  });

  props.registerNavigator({
    jumpToChapter: position => {
      const section = book?.spine.get(position);
      return section ? displaySafe(section.href) : Promise.resolve(false);
    },
    jumpToLocator: locator => {
      if (ready()) return displaySafe(locator);
      // 尚未就绪：暂存到 setup 完成后执行（消费路径触发 relocated → onPosition）
      pendingLocator = locator;
      return Promise.resolve(true);
    },
  });

  return (
    <div class="relative h-full w-full" onClick={onHostTap} ref={rootRef}>
      <div
        class="mx-auto h-full w-full"
        ref={container}
        style={{
          // scrolled 模式滚动发生在容器内，全宽让滚动条贴屏幕右缘；
          // 列宽由 iframe body 限制（见 registerTheme）
          'max-width':
            settings.pagination_mode === 'scroll'
              ? undefined
              : `${settings.reading_width}em`,
        }}
      />
      <Show when={settings.pagination_mode === 'scroll'}>
        <PageHint
          direction="down"
          visible={atEdge().bottom && hasNextChapter()}
        />
        <PageHint direction="up" visible={atEdge().top && hasPrevChapter()} />
      </Show>
      <Show when={!ready() && !error()}>
        <div class="absolute inset-0 flex items-center justify-center">
          <Spinner />
        </div>
      </Show>
      <Show when={error()}>
        <div class="absolute inset-0 flex items-center justify-center">
          <LoadErrorPanel
            onBack={() => {
              navigate(`/book/${props.book.book_id}`);
            }}
            onRetry={() => {
              void runSetup();
            }}
          />
        </div>
      </Show>
    </div>
  );
};

export default EpubReader;
