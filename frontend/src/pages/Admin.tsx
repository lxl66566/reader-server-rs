// /admin — fullscreen admin console (no Layout): auth gate (setup/login) then
// a tabbed dashboard. Admin tokens are a separate namespace from user tokens.

import { VsSignOut } from 'solid-icons/vs';
import {
  createEffect,
  createSignal,
  Show,
  type Component,
  type ParentComponent,
} from 'solid-js';
import toast from 'solid-toast';

import { adminApi, authApi, getToken, setToken } from '../api';
import {
  AdminsTab,
  AuditLogsTab,
  InviteCodesTab,
  SettingsTab,
  UsersTab,
} from '../components/admin';
import { Button, Input, Spinner } from '../components/ui';
import { useI18n } from '../i18n';
import { toastError } from '../utils/toast';

type AdminTab = 'admins' | 'invite' | 'logs' | 'settings' | 'users';
type Phase = 'checking' | 'dashboard' | 'login' | 'setup';

const Admin: Component = () => {
  const { t } = useI18n();
  const [phase, setPhase] = createSignal<Phase>(
    getToken('admin') ? 'dashboard' : 'checking',
  );
  const [tab, setTab] = createSignal<AdminTab>('invite');
  const [username, setUsername] = createSignal('');
  const [password, setPassword] = createSignal('');
  const [confirmPassword, setConfirmPassword] = createSignal('');
  const [busy, setBusy] = createSignal(false);

  // resolve the pre-auth phase: which form (setup vs login) to show
  createEffect(() => {
    if (phase() !== 'checking') return;
    void adminApi
      .checkSetup()
      .then(data => setPhase(data.is_setup ? 'login' : 'setup'))
      .catch(() => setPhase('login'));
  });

  // any 401 from dashboard requests clears the token and returns to login
  const backToLogin = () => {
    setToken('admin', null);
    setPassword('');
    setConfirmPassword('');
    setPhase('login');
  };

  const authenticate = async () => {
    if (phase() === 'setup' && password() !== confirmPassword()) {
      toast.error(t('passwordMismatch'));
      return;
    }
    setBusy(true);
    try {
      const res =
        phase() === 'setup'
          ? await authApi.adminSetup(username(), password())
          : await authApi.adminLogin(username(), password());
      setToken('admin', res.admin_token);
      setUsername('');
      setPassword('');
      setConfirmPassword('');
      setPhase('dashboard');
    } catch (err) {
      toastError(err, t);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div class="flex min-h-full flex-col">
      <Show
        fallback={
          <div class="flex flex-1 items-center justify-center p-4">
            <div class="border-edge bg-surface w-full max-w-sm rounded-lg border p-6">
              <Show
                fallback={
                  <>
                    <h1 class="text-lg font-semibold">
                      {phase() === 'setup'
                        ? t('adminSetupTitle')
                        : t('adminLoginTitle')}
                    </h1>
                    <Show when={phase() === 'setup'}>
                      <p class="text-muted mt-1 text-sm">
                        {t('adminSetupDesc')}
                      </p>
                    </Show>
                    <form
                      class="mt-4 space-y-3"
                      onSubmit={event => {
                        event.preventDefault();
                        void authenticate();
                      }}
                    >
                      <FormField label={t('username')}>
                        <Input onInput={setUsername} value={username()} />
                      </FormField>
                      <FormField label={t('password')}>
                        <Input
                          onInput={setPassword}
                          type="password"
                          value={password()}
                        />
                      </FormField>
                      <Show when={phase() === 'setup'}>
                        <FormField label={t('confirmPassword')}>
                          <Input
                            onInput={setConfirmPassword}
                            type="password"
                            value={confirmPassword()}
                          />
                        </FormField>
                      </Show>
                      <Button
                        class="w-full"
                        disabled={busy() || !username() || !password()}
                        type="submit"
                      >
                        {busy() ? <Spinner class="size-4" /> : null}
                        {phase() === 'setup' ? t('confirm') : t('login')}
                      </Button>
                    </form>
                  </>
                }
                when={phase() === 'checking'}
              >
                <div class="flex justify-center py-10">
                  <Spinner class="size-6" />
                </div>
              </Show>
            </div>
          </div>
        }
        when={phase() === 'dashboard'}
      >
        <header class="safe-top border-edge bg-surface sticky top-0 z-40 border-b">
          <div class="mx-auto flex h-12 max-w-4xl items-center justify-between px-4">
            <h1 class="text-base font-semibold">{t('adminDashboard')}</h1>
            <Button onClick={backToLogin} variant="ghost">
              <VsSignOut class="size-4" />
              {t('logout')}
            </Button>
          </div>
        </header>
        <nav class="mx-auto flex w-full max-w-4xl gap-1 overflow-x-auto px-4 pt-4 text-sm">
          <TabButton
            active={tab() === 'invite'}
            label={t('adminTabInvite')}
            onClick={() => setTab('invite')}
          />
          <TabButton
            active={tab() === 'users'}
            label={t('adminTabUsers')}
            onClick={() => setTab('users')}
          />
          <TabButton
            active={tab() === 'admins'}
            label={t('adminTabAdmins')}
            onClick={() => setTab('admins')}
          />
          <TabButton
            active={tab() === 'logs'}
            label={t('adminTabLogs')}
            onClick={() => setTab('logs')}
          />
          <TabButton
            active={tab() === 'settings'}
            label={t('adminTabSettings')}
            onClick={() => setTab('settings')}
          />
        </nav>
        <main class="mx-auto w-full max-w-4xl flex-1 px-4 py-4">
          <Show when={tab() === 'invite'}>
            <InviteCodesTab onUnauthorized={backToLogin} />
          </Show>
          <Show when={tab() === 'users'}>
            <UsersTab onUnauthorized={backToLogin} />
          </Show>
          <Show when={tab() === 'admins'}>
            <AdminsTab onUnauthorized={backToLogin} />
          </Show>
          <Show when={tab() === 'logs'}>
            <AuditLogsTab onUnauthorized={backToLogin} />
          </Show>
          <Show when={tab() === 'settings'}>
            <SettingsTab onUnauthorized={backToLogin} />
          </Show>
        </main>
      </Show>
    </div>
  );
};

const TabButton: Component<{
  active: boolean;
  label: string;
  onClick: () => void;
}> = props => (
  <button
    class={`shrink-0 cursor-pointer rounded-t px-3 py-1.5 text-sm ${
      props.active
        ? 'border-accent text-ink border-b-2'
        : 'text-muted hover:text-ink'
    }`}
    onClick={() => {
      props.onClick();
    }}
    type="button"
  >
    {props.label}
  </button>
);

const FormField: ParentComponent<{ label: string }> = props => (
  <label class="flex flex-col gap-1 text-sm">
    <span class="text-muted">{props.label}</span>
    {props.children}
  </label>
);

export default Admin;
