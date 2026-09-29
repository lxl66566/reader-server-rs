// Register page: username + password + confirm + optional invite code.

import { A, useNavigate } from '@solidjs/router';
import { createSignal, Show } from 'solid-js';
import toast from 'solid-toast';

import { Button, Input, Spinner } from '../components/ui';
import { useI18n } from '../i18n';
import { useAuth } from '../stores/auth';
import { toastError } from '../utils/toast';

const Register = () => {
  const { t } = useI18n();
  const auth = useAuth();
  const navigate = useNavigate();
  const [confirmPassword, setConfirmPassword] = createSignal('');
  const [inviteCode, setInviteCode] = createSignal('');
  const [password, setPassword] = createSignal('');
  const [submitting, setSubmitting] = createSignal(false);
  const [username, setUsername] = createSignal('');

  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    if (submitting()) return;
    if (password() !== confirmPassword()) {
      toast.error(t('passwordMismatch'));
      return;
    }
    void doRegister();
  };

  const doRegister = async () => {
    setSubmitting(true);
    try {
      const code = inviteCode().trim();
      await auth.register(username().trim(), password(), code || undefined);
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
        <h1 class="mb-4 text-lg font-medium">{t('register')}</h1>
        <label class="mb-3 block text-sm">
          <span class="text-muted mb-1 block">{t('username')}</span>
          <Input class="w-full" onInput={setUsername} value={username()} />
        </label>
        <label class="mb-3 block text-sm">
          <span class="text-muted mb-1 block">{t('password')}</span>
          <Input
            class="w-full"
            onInput={setPassword}
            type="password"
            value={password()}
          />
        </label>
        <label class="mb-3 block text-sm">
          <span class="text-muted mb-1 block">{t('confirmPassword')}</span>
          <Input
            class="w-full"
            onInput={setConfirmPassword}
            type="password"
            value={confirmPassword()}
          />
        </label>
        <label class="mb-4 block text-sm">
          <span class="text-muted mb-1 block">{t('inviteCodeOptional')}</span>
          <Input class="w-full" onInput={setInviteCode} value={inviteCode()} />
        </label>
        <Button class="w-full" disabled={submitting()} type="submit">
          <Show fallback={t('register')} when={submitting()}>
            <Spinner class="size-4" />
          </Show>
        </Button>
      </form>
      <A
        class="text-muted hover:text-ink mt-4 block text-center text-sm"
        href="/login"
      >
        {t('goLogin')}
      </A>
    </div>
  );
};

export default Register;
