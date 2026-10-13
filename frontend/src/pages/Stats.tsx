// Reading stats page: total time, streaks and a pure-div 30-day bar chart.

import { VsFlame, VsTarget, VsWatch } from 'solid-icons/vs';
import {
  createResource,
  createSignal,
  For,
  Show,
  type JSX,
  type ParentComponent,
} from 'solid-js';

import { ApiError, readingApi } from '../api';
import type { ReadingStats } from '../api/types';
import { Button, EmptyState, SectionCard, Spinner } from '../components/ui';
import { useI18n } from '../i18n';
import { useRequireAuth } from '../stores/auth';
import { formatDuration } from '../utils/format';

const Stats = () => {
  useRequireAuth();
  const { t } = useI18n();
  const [statsError, setStatsError] = createSignal<null | string>(null);
  const [stats, { refetch }] = createResource(async () => {
    try {
      const data = await readingApi.stats();
      setStatsError(null);
      return data;
    } catch (error) {
      setStatsError(
        error instanceof ApiError ? error.message : t('errorLoadFailed'),
      );
      throw error;
    }
  });

  return (
    <div class="space-y-4">
      <h1 class="text-lg font-medium">{t('statsTitle')}</h1>
      <Show
        fallback={
          <StateBlock
            error={statsError()}
            onRetry={() => {
              void refetch();
            }}
          />
        }
        when={stats()}
      >
        {data => (
          <>
            <div class="grid grid-cols-1 gap-4 sm:grid-cols-3">
              <StatCard
                icon={<VsWatch class="size-5" />}
                label={t('statsTotalTime')}
                value={formatDuration(data().total_reading_seconds, t)}
              />
              <StatCard
                icon={<VsFlame class="size-5" />}
                label={t('statsCurrentStreak')}
                value={t('statsDays', { days: data().current_streak_days })}
              />
              <StatCard
                icon={<VsTarget class="size-5" />}
                label={t('statsLongestStreak')}
                value={t('statsDays', { days: data().longest_streak_days })}
              />
            </div>
            <SectionCard title={t('statsRecent30')}>
              <Show
                fallback={<EmptyState text={t('statsEmpty')} />}
                when={data().recent_days.length > 0}
              >
                <BarChart days={data().recent_days} />
              </Show>
            </SectionCard>
          </>
        )}
      </Show>
    </div>
  );
};

const StatCard: ParentComponent<{
  icon: JSX.Element;
  label: string;
  value: string;
}> = props => (
  <div class="border-edge bg-surface rounded-lg border p-4">
    <div class="text-muted flex items-center gap-2 text-sm">
      <span class="text-accent">{props.icon}</span>
      {props.label}
    </div>
    <div class="mt-2 text-2xl font-medium tabular-nums">{props.value}</div>
  </div>
);

// Bar heights scale against the busiest day; tooltip carries date + duration.
const BarChart: ParentComponent<{
  days: ReadingStats['recent_days'];
}> = props => {
  const { locale, t } = useI18n();
  const max = () => Math.max(1, ...props.days.map(day => day.reading_seconds));
  return (
    <div class="flex h-32 items-end gap-px">
      <For each={props.days}>
        {day => (
          <div
            class="bg-accent min-w-0 flex-1 rounded-t opacity-85 transition-opacity hover:opacity-100"
            style={{ height: `${(day.reading_seconds / max()) * 100}%` }}
            title={`${formatShortDate(day.date, locale())}: ${formatDuration(day.reading_seconds, t)}`}
          />
        )}
      </For>
    </div>
  );
};

const StateBlock: ParentComponent<{
  error: null | string;
  onRetry: () => void;
}> = props => {
  const { t } = useI18n();
  return (
    <div class="flex flex-col items-center gap-3 py-12">
      <Show fallback={<Spinner class="size-6" />} when={props.error}>
        <p class="text-muted text-sm">{props.error}</p>
        <Button onClick={props.onRetry} variant="ghost">
          {t('retry')}
        </Button>
      </Show>
    </div>
  );
};

function formatShortDate(dateStr: string, locale: string): string {
  // Local noon keeps the date stable across timezones.
  const date = new Date(`${dateStr}T00:00:00`);
  if (Number.isNaN(date.getTime())) return dateStr;
  return date.toLocaleDateString(locale, { day: 'numeric', month: 'numeric' });
}

export default Stats;
