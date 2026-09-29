// Login page: username + password form, posts to auth store, toasts errors.

import { A, useNavigate } from '@solidjs/router';
import { createSignal, Show } from 'solid-js';

import { Button, Input, Spinner } from '../components/ui';
import { useI18n } from '../i18n';
import { useAuth } from '../stores/auth';
import { toastError } from '../utils/toast';

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
      toastError(error, t);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div class="mx-auto max-w-sm">
      <form
        class="border-edge bg-surface rounded-lg border p-6"
        onSubmit={submit}
      >
        <h1 class="mb-4 text-lg font-medium">{t('login')}</h1>
        <label class="mb-3 block text-sm">
          <span class="text-muted mb-1 block">{t('username')}</span>
          <Input class="w-full" onInput={setUsername} value={username()} />
        </label>
        <label class="mb-4 block text-sm">
          <span class="text-muted mb-1 block">{t('password')}</span>
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
        class="text-muted hover:text-ink mt-4 block text-center text-sm"
        href="/register"
      >
        {t('goRegister')}
      </A>
    </div>
  );
};

export default Login;
