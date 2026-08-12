// Login page: username + password form, posts to auth store, toasts errors.

import { A, useNavigate } from '@solidjs/router';
import { createSignal, Show } from 'solid-js';
import toast from 'solid-toast';
import { ApiError } from '../api';
import { Button, Input, Spinner } from '../components/ui';
import { useI18n } from '../i18n';
import type { DictKey } from '../i18n/zh-CN';
import { useAuth } from '../stores/auth';

const Login = () => {
  const { t } = useI18n();
  const auth = useAuth();
  const navigate = useNavigate();
  const [password, setPassword] = createSignal('');
  const [submitting, setSubmitting] = createSignal(false);
  const [username, setUsername] = createSignal('');

  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    if (submitting()) return;
    void doLogin();
  };

  const doLogin = async () => {
    setSubmitting(true);
    try {
      await auth.login(username().trim(), password());
      navigate('/', { replace: true });
    } catch (error) {
      showError(error, t);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div class="mx-auto max-w-sm">
      <form
        class="rounded-lg border border-edge bg-surface p-6"
        onSubmit={submit}
      >
        <h1 class="mb-4 text-lg font-medium">{t('login')}</h1>
        <label class="mb-3 block text-sm">
          <span class="mb-1 block text-muted">{t('username')}</span>
          <Input class="w-full" onInput={setUsername} value={username()} />
        </label>
        <label class="mb-4 block text-sm">
          <span class="mb-1 block text-muted">{t('password')}</span>
          <Input
            class="w-full"
            onInput={setPassword}
            type="password"
            value={password()}
          />
        </label>
        <Button class="w-full" disabled={submitting()} type="submit">
          <Show fallback={t('login')} when={submitting()}>
            <Spinner class="size-4" />
          </Show>
        </Button>
      </form>
      <A
        class="mt-4 block text-center text-sm text-muted hover:text-ink"
        href="/register"
      >
        {t('goRegister')}
      </A>
    </div>
  );
};

function showError(error: unknown, t: (key: DictKey) => string): void {
  if (error instanceof ApiError && error.status === 429) {
    toast.error(t('errorRateLimit'));
  } else if (error instanceof Error) {
    toast.error(error.message);
  } else {
    toast.error(t('errorGeneric'));
  }
}

export default Login;
