export const MAX_ATTACHMENTS = 8;
export const MAX_ATTACHMENT_BYTES = 20 * 1024 * 1024;

export type PromptAttachment = {
  name: string;
  path: string;
  size: number;
  kind: 'image' | 'document';
};

const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'webp']);
const DOCUMENT_EXTENSIONS = new Set([
  'pdf', 'txt', 'md', 'json', 'yaml', 'yml', 'xml', 'csv',
  'kt', 'kts', 'swift', 'ts', 'tsx', 'js', 'jsx', 'py',
  'rs', 'java', 'go', 'html', 'css', 'sh', 'toml',
]);

export function attachmentKind(name: string): PromptAttachment['kind'] | null {
  const extension = name.split('.').pop()?.toLowerCase() ?? '';
  if (IMAGE_EXTENSIONS.has(extension)) return 'image';
  if (DOCUMENT_EXTENSIONS.has(extension)) return 'document';
  return null;
}

export function validateAttachment(
  attachment: PromptAttachment,
  existing: readonly PromptAttachment[],
): string | null {
  if (!attachment.path || !attachment.name) return 'Attachment must have a name and path.';
  if (existing.length >= MAX_ATTACHMENTS) return `Maximum ${MAX_ATTACHMENTS} attachments.`;
  if (attachment.size <= 0 || attachment.size > MAX_ATTACHMENT_BYTES)
    return 'Attachment must be between 1 byte and 20 MiB.';
  const kind = attachmentKind(attachment.name);
  if (!kind || kind !== attachment.kind) return 'Unsupported attachment format.';
  if (existing.some(item => item.path === attachment.path)) return 'File already attached.';
  return null;
}
