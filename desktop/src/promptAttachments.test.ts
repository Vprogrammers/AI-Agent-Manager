import { describe, expect, it } from 'vitest';
import { attachmentKind, MAX_ATTACHMENTS, validateAttachment, type PromptAttachment } from './promptAttachments';

const image: PromptAttachment = { name: 'photo.png', path: '/tmp/photo.png', size: 1024, kind: 'image' };

describe('prompt attachments', () => {
  it('recognizes images and supported documents', () => {
    expect(attachmentKind('photo.WEBP')).toBe('image');
    expect(attachmentKind('code.kt')).toBe('document');
    expect(attachmentKind('archive.exe')).toBeNull();
  });
  it('accepts a supported attachment', () => {
    expect(validateAttachment(image, [])).toBeNull();
  });
  it('rejects duplicates, oversize and empty files', () => {
    expect(validateAttachment(image, [image])).toMatch(/already/);
    expect(validateAttachment({ ...image, size: 0 }, [])).toMatch(/between/);
    expect(validateAttachment({ ...image, size: 21 * 1024 * 1024 }, [])).toMatch(/between/);
  });
  it('enforces attachment count and extension', () => {
    expect(validateAttachment(image, Array.from({ length: MAX_ATTACHMENTS }, (_, i) => ({ ...image, path: String(i) })))).toMatch(/Maximum/);
    expect(validateAttachment({ ...image, name: 'bad.exe' }, [])).toMatch(/Unsupported/);
  });
});
