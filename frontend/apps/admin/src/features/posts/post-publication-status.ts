export function postPublicationStatus(status: {
    current_revision_id: string | null;
    published_revision_id: string | null;
}): { label: string; tone: 'ok' | 'info' | 'mute' } {
    if (!status.published_revision_id) return { label: '草稿', tone: 'mute' };
    if (status.current_revision_id !== status.published_revision_id) {
        return { label: '已上线 · 有新草稿', tone: 'info' };
    }
    return { label: '已上线', tone: 'ok' };
}
