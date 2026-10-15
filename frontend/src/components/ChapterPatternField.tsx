// txt 章节切分正则输入：空 = 内置默认规则，用 new RegExp 做轻量即时校验（服务端 fancy-regex 为准）
// 附带切分演示：内置正则与切分算法复刻 src/parser/chapter.rs，供用户直观理解正则效果
import { createMemo, For, Show, type ParentComponent } from 'solid-js';

import { useI18n } from '../i18n';
import { Input } from './ui';

/**
 * 与 src/parser/chapter.rs 的 CHAPTER_RE 保持一致（二者语法当前兼容）。
 * Rust 侧行首为 [\u3000\s]*，全角空格已被 \s 覆盖，此处等价简化为 \s*
 */
const BUILTIN_CHAPTER_RE =
  /^\s*(序章|序言|卷首语|扉页|楔子|正文(?!完|结)|终章|后记|尾声|番外|第?\s{0,4}[\d〇零一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟]+\s{0,4}(?:章|节(?!课)|卷|集(?![合和])|部(?![分赛游])|篇(?!张))).{0,30}$/;

const SAMPLE_TEXT = [
  '楔子',
  '雨夜，老宅的门铃忽然响了。',
  '第一章 故人',
  '来人自称是我失散多年的兄长，手里提着一盏旧灯笼。',
  '第二章 灯谜',
  '他说，要想知道真相，先解开灯谜。',
].join('\n');

interface DemoLine {
  readonly chapter: null | number;
  readonly text: string;
}

/** 轻量校验：空串视为有效（使用默认规则） */
export function isValidChapterPattern(pattern: string): boolean {
  const trimmed = pattern.trim();
  if (!trimmed) return true;
  try {
    // 能构造即可；test 消费实例本身，避免闲置变量
    new RegExp(trimmed).test('');
    return true;
  } catch {
    return false;
  }
}

/** 复刻后端切分语义：逐行 trim 后 is_match，首行非空恒为首章 */
function splitSample(content: string, re: RegExp): readonly DemoLine[] {
  const lines: DemoLine[] = [];
  let isFirstLine = true;
  let chapterNo = 0;
  for (const raw of content.split('\n')) {
    const text = raw.trim();
    const isChapter = isFirstLine ? text !== '' : re.test(text);
    isFirstLine = false;
    if (isChapter) chapterNo += 1;
    lines.push({ chapter: isChapter ? chapterNo : null, text });
  }
  return lines;
}

export const ChapterPatternField: ParentComponent<{
  onInput: (value: string) => void;
  value: string;
}> = props => {
  const { t } = useI18n();

  // 当前生效的演示正则：非法时为 null；空串回退内置规则（与后端 trim 语义一致）
  const demoRe = createMemo<null | RegExp>(() => {
    const pattern = props.value.trim();
    if (!pattern) return BUILTIN_CHAPTER_RE;
    try {
      return new RegExp(pattern);
    } catch {
      return null;
    }
  });
  const invalid = createMemo(() => demoRe() === null);
  const demoLines = createMemo<readonly DemoLine[]>(() => {
    const re = demoRe();
    return re ? splitSample(SAMPLE_TEXT, re) : [];
  });
  const chapterCount = createMemo(
    () => demoLines().filter(line => line.chapter !== null).length,
  );

  return (
    <div class="block text-sm">
      <label class="block">
        <span class="text-muted mb-1 block">{t('chapterPatternLabel')}</span>
        <Input
          onInput={props.onInput}
          placeholder={t('chapterPatternPlaceholder')}
          value={props.value}
        />
      </label>
      <Show when={invalid()}>
        <span class="mt-1 block text-xs text-red-600 dark:text-red-400">
          {t('chapterPatternInvalid')}
        </span>
      </Show>
      <details class="mt-1">
        <summary class="text-muted hover:text-ink w-fit cursor-pointer text-xs select-none">
          {t('chapterPatternDemoSummary')}
        </summary>
        <div class="border-edge bg-page mt-2 rounded border p-2 text-xs">
          <p class="text-muted leading-5">{t('chapterPatternDemoHint')}</p>
          <Show when={demoRe()}>
            <div class="mt-2 grid grid-cols-[1.25rem_minmax(0,1fr)] gap-x-2 gap-y-0.5">
              <For each={demoLines()}>
                {line => (
                  <>
                    <span
                      class="text-accent text-right tabular-nums"
                      classList={{ invisible: line.chapter === null }}
                    >
                      {line.chapter ?? ''}
                    </span>
                    <span
                      class="truncate"
                      classList={{ 'text-muted': line.chapter === null }}
                    >
                      {line.text}
                    </span>
                  </>
                )}
              </For>
            </div>
            <p class="text-muted mt-2">
              {t('chapterPatternDemoCount', { n: chapterCount() })}
            </p>
          </Show>
        </div>
      </details>
    </div>
  );
};
