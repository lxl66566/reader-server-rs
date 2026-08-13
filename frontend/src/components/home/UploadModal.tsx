// Upload book form: file picker + title/author + public toggle.

import { createEffect, createSignal, type ParentComponent } from 'solid-js';
import toast from 'solid-toast';

import { ApiError, bookApi } from '../../api';
import { useI18n } from '../../i18n';
import { Button, Input, Modal, Toggle } from '../ui';

export const UploadModal: ParentComponent<{
  onClose: () => void;
  onUploaded: () => void;
  open: boolean;
}> = props => {
  const { t } = useI18n();
  const [author, setAuthor] = createSignal('');
  const [file, setFile] = createSignal<File | null>(null);
  const [isPublic, setIsPublic] = createSignal(false);
  const [title, setTitle] = createSignal('');
  const [uploading, setUploading] = createSignal(false);

  // Reset the form whenever the modal is reopened.
  createEffect(() => {
    if (props.open) {
      setAuthor('');
      setFile(null);
      setIsPublic(false);
      setTitle('');
    }
  });

  const onFileChange = (event: Event) => {
    const input = event.currentTarget as HTMLInputElement;
    const next = input.files?.[0] ?? null;
    setFile(next);
    if (next && title() === '') {
      setTitle(next.name.replace(/\.[^.]+$/, ''));
    }
  };

  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    if (!file() || uploading()) return;
    void doUpload();
  };

  const doUpload = async () => {
    setUploading(true);
    try {
      const f = file();
      if (!f) return;
      const form = new FormData();
      form.append('file', f);
      form.append('title', title().trim());
      if (author().trim()) form.append('author', author().trim());
      form.append('is_public', isPublic() ? 'true' : 'false');
      await bookApi.upload(form);
      toast.success(t('uploadSuccess'));
      props.onUploaded();
      props.onClose();
    } catch (error) {
      if (error instanceof ApiError && error.status === 429) {
        toast.error(t('errorRateLimit'));
      } else if (error instanceof Error) {
        toast.error(error.message);
      } else {
        toast.error(t('errorGeneric'));
      }
    } finally {
      setUploading(false);
    }
  };

  return (
    <Modal
      onClose={() => {
        props.onClose();
      }}
      open={props.open}
      title={t('uploadTitle')}
    >
      <form class="flex flex-col gap-3" onSubmit={submit}>
        <label class="border-edge bg-base text-muted hover:bg-surface block cursor-pointer rounded border px-3 py-2 text-sm">
          <span class="block truncate">
            {file() ? file()!.name : t('uploadFile')}
          </span>
          <input
            accept=".txt,.epub,.pdf"
            class="hidden"
            onChange={onFileChange}
            type="file"
          />
        </label>
        <label class="block text-sm">
          <span class="text-muted mb-1 block">{t('titleLabel')}</span>
          <Input class="w-full" onInput={setTitle} value={title()} />
        </label>
        <label class="block text-sm">
          <span class="text-muted mb-1 block">{t('authorLabel')}</span>
          <Input class="w-full" onInput={setAuthor} value={author()} />
        </label>
        <div class="flex items-center justify-between">
          <span class="text-sm">{t('isPublicLabel')}</span>
          <Toggle checked={isPublic()} onChange={setIsPublic} />
        </div>
        <p class="text-muted text-xs">{t('uploadFileHint')}</p>
        <div class="mt-1 flex gap-2">
          <Button
            class="flex-1"
            onClick={() => {
              props.onClose();
            }}
            variant="ghost"
          >
            {t('cancel')}
          </Button>
          <Button
            class="flex-1"
            disabled={!file() || !title().trim() || uploading()}
            type="submit"
          >
            {uploading() ? t('uploading') : t('uploadSubmit')}
          </Button>
        </div>
      </form>
    </Modal>
  );
};
