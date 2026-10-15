// 全局离线提示条：navigator 离线时展示（阅读器内与其他页面共用）。

import { Show, type Component } from 'solid-js';

import { useI18n } from '../i18n';
import { createOnlineSignal } from '../offline/net';

// 模块级单例信号：各实例共享同一 online/offline 监听
const online = createOnlineSignal();

export const isOnline = online;

export const OfflineBanner: Component<{ class?: string }> = props => {
  const { t } = useI18n();
  return (
    <Show when={!online()}>
      <div
        class={`bg-fg/70 text-bg w-full py-1 text-center text-xs ${props.class ?? ''}`}
      >
        {t('offlineBanner')}
      </div>
    </Show>
  );
};
