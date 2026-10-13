// 加载失败的兜底面板：提示 + 返回/重试（与 BookDetail 页的错误处理模式一致）。

import { VsWarning } from 'solid-icons/vs';

import { useI18n } from '../i18n';
import { Button } from './ui';

export const LoadErrorPanel = (props: {
  onBack: () => void;
  onRetry: () => void;
}) => {
  const { t } = useI18n();
  return (
    <div class="flex flex-col items-center gap-4 py-16">
      <VsWarning class="text-muted size-8" />
      <p class="text-muted text-sm">{t('errorLoadFailed')}</p>
      <div class="flex gap-2">
        <Button onClick={props.onBack} type="button" variant="ghost">
          {t('back')}
        </Button>
        <Button onClick={props.onRetry} type="button">
          {t('retry')}
        </Button>
      </div>
    </div>
  );
};
