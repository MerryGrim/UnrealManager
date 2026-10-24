/** Pure parsing only. Product validity is established separately by Epic. */
export const REDEMPTION_BATCH_LIMITS = Object.freeze({
    maxKeys: 100,
    maxUtf8Bytes: 64 * 1024,
    maxPhysicalRows: 256,
    maxKeyCharacters: 128,
} as const);

export type RedemptionBatchIssueCode =
    | 'empty'
    | 'inputTooLarge'
    | 'tooManyRows'
    | 'tooManyKeys'
    | 'keyTooLong'
    | 'invalidCharacters';

/** Issues deliberately contain no input text or key fragments. */
export type RedemptionBatchIssue = {
    code: RedemptionBatchIssueCode;
    line?: number;
    limit?: number;
};

export type RedemptionBatchKey = {
    /** Sensitive, in-memory input. Do not persist, log or include in diagnostics. */
    key: string;
    line: number;
    masked: string;
};

export type RedemptionBatchResult = {
    ok: true;
    keys: RedemptionBatchKey[];
    physicalRows: number;
    blankRows: number;
    duplicateLines: number[];
} | {
    ok: false;
    issues: RedemptionBatchIssue[];
};

const trimAsciiEdges = (value: string) => value.replace(/^[\t\n\v\f\r ]+|[\t\n\v\f\r ]+$/g, '');

/** Never reveal a complete short key, including through a suffix. */
export function maskRedemptionKey(value: string): string {
    const key = trimAsciiEdges(value);
    if (key.length <= 4) return '****';
    const segments = key.split('-');
    if (segments.length > 1 && segments.every(Boolean)) return segments[segments.length - 1].slice(-8);
    return `**** ${key.slice(-4)}`;
}

/**
 * One key per physical line. No grouping, minimum length, checksum, case or
 * hyphen changes are assumed. A failed parse returns no usable key list.
 * The caller must reject an oversized File before calling File.text().
 */
export function parseRedemptionBatch(text: string): RedemptionBatchResult {
    const limits = REDEMPTION_BATCH_LIMITS;
    // UTF-8 cannot be shorter than the UTF-16 code-unit count. Bound the input
    // before creating an encoded buffer, then enforce the exact UTF-8 limit.
    if (text.length > limits.maxUtf8Bytes || new TextEncoder().encode(text).byteLength > limits.maxUtf8Bytes) {
        return { ok: false, issues: [{ code: 'inputTooLarge', limit: limits.maxUtf8Bytes }] };
    }

    const lines = text.replace(/^\uFEFF/, '').split(/\r\n|\n|\r/);
    // A final line terminator ends the last physical row; it is not a new row.
    if (lines.length > 1 && lines[lines.length - 1] === '') lines.pop();
    if (lines.length > limits.maxPhysicalRows) {
        return { ok: false, issues: [{ code: 'tooManyRows', limit: limits.maxPhysicalRows }] };
    }

    const keys: RedemptionBatchKey[] = [];
    const issues: RedemptionBatchIssue[] = [];
    const duplicateLines: number[] = [];
    const seen = new Set<string>();
    let blankRows = 0;

    for (const [index, raw] of lines.entries()) {
        const key = trimAsciiEdges(raw);
        const line = index + 1;
        if (!key) {
            blankRows += 1;
            continue;
        }
        if (key.length > limits.maxKeyCharacters) {
            issues.push({ code: 'keyTooLong', line, limit: limits.maxKeyCharacters });
            continue;
        }
        if (!/^[A-Za-z0-9-]+$/.test(key) || !/[A-Za-z0-9]/.test(key)) {
            issues.push({ code: 'invalidCharacters', line });
            continue;
        }
        const comparison = key.toUpperCase();
        if (seen.has(comparison)) {
            duplicateLines.push(line);
            continue;
        }
        seen.add(comparison);
        keys.push({ key, line, masked: maskRedemptionKey(key) });
    }

    if (keys.length > limits.maxKeys) issues.push({ code: 'tooManyKeys', limit: limits.maxKeys });
    if (issues.length) return { ok: false, issues };
    if (!keys.length) return { ok: false, issues: [{ code: 'empty' }] };
    return { ok: true, keys, physicalRows: lines.length, blankRows, duplicateLines };
}
