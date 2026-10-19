#!/usr/bin/env node
// Read-only release check. Reports labels/counts, never matched private values.
const fs = require('node:fs');
const path = require('node:path');
const cp = require('node:child_process');
const assert = require('node:assert/strict');

const root = path.resolve(__dirname, '..');
const maximumFileBytes = 256 * 1024 * 1024;
const localDirectories = new Set(['data', 'webview2', 'ebwebview', '.release-secrets']);

function patternsFor(workspace, environment = process.env, publicIdentities = []) {
  const paths = path.win32.isAbsolute(workspace) ? path.win32 : path;
  const projects = paths.dirname(workspace);
  const shared = paths.basename(projects).toLowerCase() === 'projects' ? paths.dirname(projects) : workspace;
  const values = [
    ['workspace', workspace],
    ['workspaceParent', shared],
    ['profile', environment.USERPROFILE],
    ['profile', environment.HOME],
    ['toolRoot', paths.join(workspace, 'tools')],
    ['toolRoot', paths.join(shared, 'Tools')],
    ...['UM_TOOLS_ROOT', 'CARGO_HOME', 'RUSTUP_HOME', 'CARGO_TARGET_DIR', 'CODEX_HOME'].map(name => ['toolRoot', environment[name]]),
  ];
  const profileName = location => location ? (path.win32.isAbsolute(location) ? path.win32 : path).basename(location) : '';
  const usernames = new Set([environment.USERNAME, environment.USER, profileName(environment.USERPROFILE), profileName(environment.HOME)]);
  for (const username of usernames) if (username && !/^(root|user|default|administrator|admin|runner|system)$/i.test(username)) values.push(['username', username]);
  const unique = new Map();
  for (const [category, raw] of values) {
    if (!raw || (category !== 'username' && !path.isAbsolute(raw) && !path.win32.isAbsolute(raw))) continue;
    const text = String(raw).replace(/[\\/]+$/, '');
    if (!text || (category !== 'username' && text === path.parse(text).root.replace(/[\\/]+$/, ''))) continue;
    const variants = category === 'username' ? [text] : [text.replaceAll('\\', '/'), ...[1, 2, 4, 8].map(n => text.replaceAll('/', '\\').replaceAll('\\', '\\'.repeat(n)))];
    for (const value of variants) {
      const id = `${category}:${value.toLowerCase()}`;
      if (!unique.has(id)) unique.set(id, {category, value, publicIdentity: category === 'username' && publicIdentities.some(identity => identity.toLowerCase() === value.toLowerCase())});
    }
  }
  return [...unique.values()].flatMap(({category, value, publicIdentity}) => [
    {category, publicIdentity, encoding: 'utf8', needle: lowerAscii(Buffer.from(value, 'utf8'))},
    {category, publicIdentity, encoding: 'utf16le', needle: lowerAscii(Buffer.from(value, 'utf16le'))},
  ]);
}
function publicAuthorIdentities() {
  const names = [];
  try {
    const tauri = JSON.parse(fs.readFileSync(path.join(root, 'source/src-tauri/tauri.conf.json'), 'utf8'));
    if (typeof tauri.bundle?.publisher === 'string') names.push(tauri.bundle.publisher);
    const cargo = fs.readFileSync(path.join(root, 'source/src-tauri/Cargo.toml'), 'utf8');
    const authors = cargo.match(/^authors\s*=\s*(\[[^\]\r\n]*\])/m);
    if (authors) names.push(...JSON.parse(authors[1]).map(name => name.replace(/\s*<[^>]+>$/, '').trim()));
  } catch { /* An unreadable declaration grants no author exclusion. */ }
  return names.filter(name => typeof name === 'string' && name);
}
function lowerAscii(bytes) {
  const output = Buffer.from(bytes);
  for (let i = 0; i < output.length; i++) if (output[i] >= 65 && output[i] <= 90) output[i] += 32;
  return output;
}
function isWord(code) { return code >= 48 && code <= 57 || code >= 65 && code <= 90 || code >= 97 && code <= 122 || code === 95; }
function publicUrlRanges(bytes, encoding, alignment) {
  const text = encoding === 'utf8' ? bytes.toString('latin1') : bytes.subarray(alignment).toString('utf16le');
  const width = encoding === 'utf8' ? 1 : 2;
  const ranges = [];
  for (const match of text.matchAll(/https?:\/\/[^\s<>"'\x00]+/gi)) {
    try {
      const url = new URL(match[0]);
      if (!url.username && !url.password && url.hostname && !/^(localhost|127\.|\[::1\])/i.test(url.hostname)) ranges.push([match.index * width + alignment, (match.index + match[0].length) * width + alignment]);
    } catch { /* A malformed URL does not excuse a possible private value. */ }
  }
  return ranges;
}
function scanBytes(bytes, patterns) {
  const folded = lowerAscii(bytes);
  const counts = {};
  const offsets = [];
  const urls = new Map();
  const seen = new Set();
  let ignoredPublicUrlUsernames = 0;
  let ignoredPublicAttributionUsernames = 0;
  for (const pattern of patterns) {
    let at = -1;
    while ((at = folded.indexOf(pattern.needle, at + 1)) !== -1) {
      const alignment = pattern.encoding === 'utf8' ? 0 : at % 2;
      const encoding = pattern.encoding === 'utf8' ? 'utf8' : alignment ? 'utf16leOdd' : 'utf16leEven';
      const id = `${pattern.category}:${encoding}:${at}`;
      if (seen.has(id)) continue;
      seen.add(id);
      if (pattern.category === 'username') {
        const width = pattern.encoding === 'utf8' ? 1 : 2;
        const read = index => index < 0 || index + width > bytes.length ? -1 : width === 1 ? bytes[index] : bytes.readUInt16LE(index);
        if (isWord(read(at - width)) || isWord(read(at + pattern.needle.length))) continue;
        const key = `${pattern.encoding}:${alignment}`;
        if (!urls.has(key)) urls.set(key, publicUrlRanges(bytes, pattern.encoding, alignment));
        if (urls.get(key).some(([start, end]) => at >= start && at + pattern.needle.length <= end)) { ignoredPublicUrlUsernames++; continue; }
        // An intentionally public author/publisher may equal the Windows
        // account name. Keep attribution, but never excuse a path component.
        const pathSeparator = code => code === 47 || code === 92 || code === 58;
        if (pattern.publicIdentity && !pathSeparator(read(at-width)) && !pathSeparator(read(at+pattern.needle.length))) { ignoredPublicAttributionUsernames++; continue; }
      }
      counts[pattern.category] ??= {utf8: 0, utf16leEven: 0, utf16leOdd: 0};
      counts[pattern.category][encoding]++;
      offsets.push(at);
    }
  }
  return {counts, ignoredPublicUrlUsernames, ignoredPublicAttributionUsernames, offsets: [...new Set(offsets)].sort((a,b) => a-b)};
}
function excludedSource(relative) {
  return /^(?:Releases|Data|Temp|tools|\.release-secrets)(?:\/|$)/i.test(relative) || /^verification\/current(?:\/|$)/i.test(relative);
}
function sourceInventory() {
  const result = cp.spawnSync('git', ['-C', root, 'ls-files', '--cached', '--others', '--exclude-standard', '-z'], {encoding: 'utf8', windowsHide: true, maxBuffer: 16 * 1024 * 1024});
  if (result.error || result.status !== 0) throw new Error('Cannot read the public Git working-tree inventory.');
  const files = [...new Set(result.stdout.split('\0').filter(Boolean))].sort();
  return {files: files.filter(file => !excludedSource(file.replaceAll('\\', '/'))), excluded: files.filter(file => excludedSource(file.replaceAll('\\', '/'))).length};
}
function imageMetadata(bytes) {
  if (bytes.length < 12 || bytes.toString('ascii', 0, 4) !== 'RIFF' || bytes.toString('ascii', 8, 12) !== 'WEBP') return undefined;
  const chunks = [];
  for (let offset = 12; offset + 8 <= bytes.length;) {
    const type = bytes.toString('ascii', offset, offset + 4);
    const length = bytes.readUInt32LE(offset + 4);
    if (['EXIF', 'XMP ', 'ICCP'].includes(type)) chunks.push(type.trim());
    if (offset + 8 + length > bytes.length) break;
    offset += 8 + length + length % 2;
  }
  return chunks;
}
function audit(options) {
  const patterns = patternsFor(root, process.env, publicAuthorIdentities());
  const report = {schema: 1, passed: false, source: {filesScanned: 0, excludedLocalFiles: 0}, artifacts: {filesScanned: 0}, counts: {}, findings: [], issues: [], webpMetadata: [], ignoredPublicUrlUsernames: 0, ignoredPublicAttributionUsernames: 0,
    patternCategories: [...new Set(patterns.map(pattern => pattern.category))], usernameAliasesChecked: patterns.filter(pattern => pattern.category === 'username' && pattern.encoding === 'utf8').length,
    limits: 'Checks current Git-tracked and non-ignored new files plus explicitly selected artifact bytes. Counts can overlap across categories. UTF-16LE is checked at both byte alignments. Compressed payload contents and text drawn into image pixels are not decoded; zero matches do not prove absence of every possible personal identifier.'};
  function file(absolute, label, group) {
    let stat;
    try { stat = fs.lstatSync(absolute); } catch { report.issues.push({file: label, reason: 'missing or unreadable input'}); return; }
    if (!stat.isFile() || stat.isSymbolicLink()) { report.issues.push({file: label, reason: 'expected a regular file without links'}); return; }
    if (stat.size > maximumFileBytes) { report.issues.push({file: label, reason: 'input exceeds the 256 MiB scan limit'}); return; }
    const bytes = fs.readFileSync(absolute);
    const scanned = scanBytes(bytes, patterns);
    report[group].filesScanned++;
    report.ignoredPublicUrlUsernames += scanned.ignoredPublicUrlUsernames;
    report.ignoredPublicAttributionUsernames += scanned.ignoredPublicAttributionUsernames;
    for (const [category, value] of Object.entries(scanned.counts)) {
      report.counts[category] ??= {utf8: 0, utf16leEven: 0, utf16leOdd: 0};
      for (const encoding of Object.keys(value)) report.counts[category][encoding] += value[encoding];
    }
    if (scanned.offsets.length) {
      const finding = {file: label, scope: group, counts: scanned.counts};
      if (/\.(?:md|txt|json|[cm]?js|ts|tsx|rs|ps1|py|toml|ya?ml|html|css|svg|cmd)$/.test(label)) {
        const positions = new Set(scanned.offsets);
        let line = 1; const lines = new Set();
        for (let index = 0; index < bytes.length; index++) { if (positions.has(index)) lines.add(line); if (bytes[index] === 10) line++; }
        finding.lines = [...lines];
      }
      report.findings.push(finding);
    }
    const metadata = imageMetadata(bytes);
    if (metadata) report.webpMetadata.push({file: label, chunks: metadata});
  }
  const inventory = sourceInventory();
  report.source.excludedLocalFiles = inventory.excluded;
  for (const relative of inventory.files) file(path.join(root, relative), relative.replaceAll('\\', '/'), 'source');
  if (options.exe) file(path.resolve(options.exe), `EXE/${path.basename(options.exe)}`, 'artifacts');
  if (options.portable) {
    const base = path.resolve(options.portable);
    function walk(folder, prefix) {
      let entries;
      try { entries = fs.readdirSync(folder, {withFileTypes: true}); } catch { report.issues.push({file: prefix, reason: 'cannot read portable directory'}); return; }
      for (const entry of entries) {
        const relative = `${prefix}/${entry.name}`;
        if (entry.isSymbolicLink()) { report.issues.push({file: relative, reason: 'portable entry is a link'}); continue; }
        if (localDirectories.has(entry.name.toLowerCase())) { report.issues.push({file: relative, reason: 'local account/data directory must not be distributed; contents were not read'}); continue; }
        if (entry.isDirectory()) walk(path.join(folder, entry.name), relative);
        else file(path.join(folder, entry.name), relative, 'artifacts');
      }
    }
    walk(base, 'portable');
  }
  report.passed = report.findings.length === 0 && report.issues.length === 0;
  return report;
}
function selfTest() {
  const patterns = patternsFor('X:\\Fixture\\Project', {USERPROFILE: 'X:\\Fixture\\Users\\SamplePerson', USERNAME: 'SamplePerson'});
  const username = patterns.filter(p => p.category === 'username');
  const sample = 'X:\\Fixture\\Project';
  const workspacePatterns = patterns.filter(p => p.category === 'workspace');
  assert.equal(scanBytes(Buffer.from(sample), workspacePatterns).counts.workspace.utf8, 1);
  assert.equal(scanBytes(Buffer.from(sample.toUpperCase()), workspacePatterns).counts.workspace.utf8, 1);
  assert.equal(scanBytes(Buffer.from(sample.replaceAll('\\', '/')), workspacePatterns).counts.workspace.utf8, 1);
  assert.equal(scanBytes(Buffer.from(JSON.stringify(sample)), workspacePatterns).counts.workspace.utf8, 1);
  assert.equal(scanBytes(Buffer.from(sample, 'utf16le'), workspacePatterns).counts.workspace.utf16leEven, 1);
  assert.equal(scanBytes(Buffer.concat([Buffer.from([1]), Buffer.from(sample, 'utf16le')]), workspacePatterns).counts.workspace.utf16leOdd, 1);
  assert.equal(scanBytes(Buffer.from('SamplePerson'), username).counts.username.utf8, 1);
  assert.equal(scanBytes(Buffer.from('OtherSamplePersonSuffix'), username).offsets.length, 0);
  for (const encoding of ['utf8', 'utf16le']) {
    const checked = scanBytes(Buffer.from('https://github.com/SamplePerson/project', encoding), username);
    assert.equal(checked.offsets.length, 0); assert.equal(checked.ignoredPublicUrlUsernames, 1);
  }
  const publicOwner = patternsFor('X:\\Fixture\\Project', {USERNAME: 'SamplePerson'}, ['SamplePerson']).filter(p => p.category === 'username');
  assert.equal(scanBytes(Buffer.from('Copyright SamplePerson'), publicOwner).ignoredPublicAttributionUsernames, 1);
  assert.equal(scanBytes(Buffer.from('X:\\Users\\SamplePerson\\file'), publicOwner).counts.username.utf8, 1);
  assert(excludedSource('verification/current/private.log'));
  assert(!excludedSource('verification/check-public-privacy.cjs'));
  console.log('PASS privacy scanner: paths/case/slashes/JSON escaping, UTF-8, even/odd UTF-16LE, username boundaries, public URL/author exclusions with path detection and local source exclusions.');
}
function main() {
  const options = {};
  const args = process.argv.slice(2);
  if (args.length === 1 && args[0] === '--self-test') return selfTest();
  for (let i = 0; i < args.length; i++) {
    if (!['--exe', '--portable', '--output'].includes(args[i]) || !args[i+1] || args[i+1].startsWith('--')) throw new Error('Usage: node verification/check-public-privacy.cjs [--exe <file>] [--portable <directory>] [--output <new-report.json>] or --self-test');
    options[args[i].slice(2)] = args[++i];
  }
  const report = audit(options);
  if (options.output) fs.writeFileSync(path.resolve(options.output), JSON.stringify(report, null, 2) + '\n', {flag: 'wx'});
  console.log(`${report.passed ? 'PASS' : 'FAIL'} privacy check: ${report.source.filesScanned} public files, ${report.artifacts.filesScanned} artifact files, ${report.findings.length} files with possible private values, ${report.issues.length} input issues.`);
  console.log(JSON.stringify({patternCategories: report.patternCategories, usernameAliasesChecked: report.usernameAliasesChecked, counts: report.counts, findings: report.findings, issues: report.issues, webpMetadata: report.webpMetadata, ignoredPublicUrlUsernames: report.ignoredPublicUrlUsernames, ignoredPublicAttributionUsernames: report.ignoredPublicAttributionUsernames}, null, 2));
  process.exitCode = report.passed ? 0 : 1;
}
if (require.main === module) {
  try { main(); } catch { console.error('Privacy check could not complete. Verify arguments, Git inventory, readable inputs and a new report output path.'); process.exitCode = 2; }
}
module.exports = {patternsFor, scanBytes, excludedSource, imageMetadata};
