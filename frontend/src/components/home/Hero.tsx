// Landing hero shown to logged-out visitors: features + login/register CTA.

import { A } from '@solidjs/router';
import { VsClockface, VsGlobe, VsSettingsGear, VsSync } from 'solid-icons/vs';
import { For, type Component } from 'solid-js';
import { useI18n } from '../../i18n';
import { Button } from '../ui';

interface Feature {
  icon: Component<{ class?: string }>;
  text: string;
  title: string;
}

export const Hero = () => {
  const { t } = useI18n();
  const features: Feature[] = [
    {
      icon: VsSync,
      text: t('heroFeature1Desc'),
      title: t('heroFeature1Title'),
    },
    {
      icon: VsClockface,
      text: t('heroFeature2Desc'),
      title: t('heroFeature2Title'),
    },
    {
      icon: VsGlobe,
      text: t('heroFeature3Desc'),
      title: t('heroFeature3Title'),
    },
    {
      icon: VsSettingsGear,
      text: t('heroFeature4Desc'),
      title: t('heroFeature4Title'),
    },
  ];
  return (
    <div class="mx-auto max-w-2xl py-8 sm:py-14">
      <h1 class="text-2xl font-semibold text-ink sm:text-3xl">
        {t('heroTitle')}
      </h1>
      <p class="mt-3 text-sm text-muted sm:text-base">{t('heroSubtitle')}</p>
      <div class="mt-8 grid grid-cols-1 gap-3 sm:grid-cols-2">
        <For each={features}>
          {feature => (
            <div class="rounded-lg border border-edge bg-surface p-4">
              <div class="flex items-center gap-2">
                <feature.icon class="size-5 text-accent" />
                <h3 class="text-sm font-medium text-ink">{feature.title}</h3>
              </div>
              <p class="mt-2 text-sm text-muted">{feature.text}</p>
            </div>
          )}
        </For>
      </div>
      <div class="mt-8 flex flex-col items-center gap-3">
        <A href="/register">
          <Button>{t('getStarted')}</Button>
        </A>
        <A class="text-sm text-muted hover:text-ink" href="/login">
          {t('goLogin')}
        </A>
      </div>
    </div>
  );
};
