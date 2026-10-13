// 章节边界提示：滚到章首/章尾时浮现的小标签，纯展示（pointer-events none，
// 不拦截点击），切章行为由阅读容器的点击分区处理。仅滚动模式挂载。
import { VsArrowDown, VsArrowUp } from 'solid-icons/vs';
import { Show } from 'solid-js';

import { useI18n } from '../i18n';

const PageHint = (props: { direction: 'down' | 'up'; visible: boolean }) => {
  const { t } = useI18n();
  return (
    <Show when={props.visible}>
      <div
        class={`pointer-events-none absolute inset-x-0 z-10 flex justify-center ${
          props.direction === 'down' ? 'bottom-1' : 'top-1'
        }`}
      >
        <div class="flex items-center gap-1 rounded-full bg-black/40 px-2.5 py-1 text-xs text-white opacity-70">
          <Show
            fallback={<VsArrowUp class="size-3.5" />}
            when={props.direction === 'down'}
          >
            <VsArrowDown class="size-3.5" />
          </Show>
          <span>
            {props.direction === 'down'
              ? t('readerNextChapter')
              : t('readerPrevChapter')}
          </span>
        </div>
      </div>
    </Show>
  );
};

export default PageHint;
