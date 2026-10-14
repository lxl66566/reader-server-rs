// Upload book form: file picker + title/author + public toggle.

import {
  createEffect,
  createSignal,
  Show,
  type ParentComponent,
} from 'solid-js';
import toast from 'solid-toast';

import { bookApi } from '../../api';
import { useI18n } from '../../i18n';
import { toastError } from '../../utils/toast';
import {
  ChapterPatternField,
  isValidChapterPattern,
} from '../ChapterPatternField';
import { Button, Input, Modal, Toggle } from '../ui';

export const UploadModal: ParentComponent<{
  onClose: () => void;
  onUploaded: () => void;
  open: boolean;
}> = props => {
  const { t } = useI18n();
  const [author, setAuthor] = createSignal('');
  const [chapterPattern, setChapterPattern] = createSignal('');
  const [file, setFile] = createSignal<File | null>(null);
  const [isPublic, setIsPublic] = createSignal(false);
  const [title, setTitle] = createSignal('');
  const [uploading, setUploading] = createSignal(false);

  // 章节 regex 仅对 txt 有意义；服务端按内容识别格式，这里用扩展名近似
  const isTxt = () => file()?.name.toLowerCase().endsWith('.txt') ?? false;

  // Reset the form whenever the modal is reopened.
  createEffect(() => {
    if (props.open) {
      setAuthor('');
      setChapterPattern('');
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

  // 上传控制器：上传中「取消」按钮 abort；关闭弹窗同样中止后台请求
  let uploadController: AbortController | null = null;

  const doUpload = async () => {
    setUploading(true);
    uploadController = new AbortController();
    try {
      const f = file();
      if (!f) return;
      const form = new FormData();
      form.append('file', f);
      form.append('title', title().trim());
      if (author().trim()) form.append('author', author().trim());
      const pattern = chapterPattern().trim();
      if (isTxt() && pattern) form.append('chapter_pattern', pattern);
      form.append('is_public', isPublic() ? 'true' : 'false');
      await bookApi.upload(form, uploadController.signal);
      toast.success(t('uploadSuccess'));
      props.onUploaded();
      props.onClose();
    } catch (error) {
      // 主动取消不弹错误
      if (!uploadController.signal.aborted) toastError(error, t);
    } finally {
      setUploading(false);
      uploadController = null;
    }
  };

  return (
    <Modal
      onClose={() => {
        // 上传中禁止关闭：弹窗状态由上传结果或取消按钮驱动
        if (uploading()) return;
        props.onClose();
      }}
      open={props.open}
      title={t('uploadTitle')}
    >
      <form class="flex flex-col gap-3" onSubmit={submit}>
        <label class="border-edge bg-page text-muted hover:bg-surface block cursor-pointer rounded border px-3 py-2 text-sm">
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
          <Input onInput={setTitle} value={title()} />
        </label>
        <label class="block text-sm">
          <span class="text-muted mb-1 block">{t('authorLabel')}</span>
          <Input onInput={setAuthor} value={author()} />
        </label>
        <Show when={isTxt()}>
          <ChapterPatternField
            onInput={setChapterPattern}
            value={chapterPattern()}
          />
        </Show>
        <div class="flex items-center justify-between">
          <span class="text-sm">{t('isPublicLabel')}</span>
          <Toggle checked={isPublic()} onChange={setIsPublic} />
        </div>
        <p class="text-muted text-xs">{t('uploadFileHint')}</p>
        <div class="mt-1 flex gap-2">
          <Button
            class="flex-1"
            onClick={() => {
              if (uploading()) uploadController?.abort();
              else props.onClose();
            }}
            variant="ghost"
          >
            {uploading() ? t('uploadCancel') : t('cancel')}
          </Button>
          <Button
            class="flex-1"
            disabled={
              !file() ||
              !title().trim() ||
              uploading() ||
              (isTxt() && !isValidChapterPattern(chapterPattern()))
            }
            type="submit"
          >
            {uploading() ? t('uploading') : t('uploadSubmit')}
          </Button>
        </div>
      </form>
    </Modal>
  );
};
