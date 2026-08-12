// App shell: top navigation for non-reader pages.

import { A, useNavigate } from '@solidjs/router';
import { VsAccount, VsBook, VsGraph, VsSettings } from 'solid-icons/vs';
import { Show, type ParentComponent } from 'solid-js';
import toast from 'solid-toast';
import { useI18n } from '../i18n';
import { useAuth } from '../stores/auth';

export const Layout: ParentComponent = props => {
  const { t } = useI18n();
  const auth = useAuth();
  const navigate = useNavigate();

  const logout = () => {
    auth.logout();
    toast.success(t('logout'));
    navigate('/');
  };

  return (
    <div class="flex min-h-full flex-col">
      <header class="safe-top sticky top-0 z-40 border-b border-edge bg-surface">
        <div class="mx-auto flex h-12 max-w-4xl items-center gap-4 px-4">
          <A class="flex items-center gap-1.5 text-base font-semibold" href="/">
            <VsBook class="size-5 text-accent" />
            {t('appName')}
          </A>
          <nav class="ml-auto flex items-center gap-1 text-sm">
            <Show when={auth.isLoggedIn()}>
              <NavLink
                href="/"
                icon={<VsBook class="size-4" />}
                label={t('navHome')}
              />
              <NavLink
                href="/stats"
                icon={<VsGraph class="size-4" />}
                label={t('navStats')}
              />
              <NavLink
                href="/settings"
                icon={<VsSettings class="size-4" />}
                label={t('navSettings')}
              />
              <button
                class="flex cursor-pointer items-center gap-1 rounded px-2 py-1 text-muted hover:bg-base hover:text-ink"
                onClick={logout}
                type="button"
              >
                <VsAccount class="size-4" />
                <span class="hidden sm:inline">{t('logout')}</span>
              </button>
            </Show>
            <Show when={!auth.isLoggedIn()}>
              <A
                class="rounded border border-edge px-3 py-1 hover:bg-base"
                href="/login"
              >
                {t('login')}
              </A>
            </Show>
          </nav>
        </div>
      </header>
      <main class="mx-auto w-full max-w-4xl flex-1 px-4 py-6">
        {props.children}
      </main>
    </div>
  );
};

const NavLink: ParentComponent<{
  href: string;
  icon: import('solid-js').JSX.Element;
  label: string;
}> = props => (
  <A
    activeClass="text-ink"
    class="flex items-center gap-1 rounded px-2 py-1 text-muted hover:bg-base hover:text-ink"
    href={props.href}
  >
    {props.icon}
    <span class="hidden sm:inline">{props.label}</span>
  </A>
);
