const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

// These tests cover the current beta.7 behavior. Older tests below this directory
// retain assertions for the UI at the time they were originally recorded.
const selection = [
  'test-locale-catalogues.cjs',
  'test-preferences.cjs',
  'test-library-topics.cjs',
  'test-vault-all-kinds.cjs',
  'test-beta5.cjs',
  'test-resume-forget.cjs',
  'test-engine-action-layout.cjs',
  'test-beta4.cjs',
  'test-engine-ui.cjs',
  'test-app-updates.cjs',
];

const startedAt = new Date();
const stamp = startedAt.toISOString().replace(/[:.]/g, '-');
const outputDirectory = process.env.UM_VERIFICATION_OUTPUT_DIRECTORY
  ? path.resolve(process.env.UM_VERIFICATION_OUTPUT_DIRECTORY)
  : path.join(__dirname, 'current', `beta7-${stamp}`);
fs.mkdirSync(outputDirectory, { recursive: true });
const requested = process.argv.slice(2);
const tests = requested.length ? requested : selection;
for (const test of tests) {
  if (!selection.includes(test)) {
    throw new Error(`Unknown beta.7 test: ${test}. Select from ${selection.join(', ')}`);
  }
}

const results = [];
for (const test of tests) {
  console.log(`RUN ${test}`);
  const result = spawnSync(process.execPath, [path.join(__dirname, test)], {
    cwd: path.join(__dirname, '..', 'source'),
    env: { ...process.env, UM_VERIFICATION_OUTPUT_DIRECTORY: outputDirectory },
    encoding: 'utf8',
    timeout: 180_000,
    maxBuffer: 16 * 1024 * 1024,
    windowsHide: true,
  });
  const output = (result.stdout || '') + (result.stderr || '')
    + (result.error ? `\n${result.error.stack}\n` : '');
  const log = test.replace(/\.cjs$/, '.log');
  fs.writeFileSync(path.join(outputDirectory, log), output);
  if (output) process.stdout.write(output);
  const passed = result.status === 0 && !result.error;
  results.push({ test, passed, exitCode: result.status, signal: result.signal, log });
  console.log(`${passed ? 'PASS' : 'FAIL'} ${test}`);
}

const report = {
  version: '0.2.0-beta.7',
  startedAt: startedAt.toISOString(),
  finishedAt: new Date().toISOString(),
  node: process.version,
  outputDirectory,
  passed: results.every(result => result.passed),
  results,
};
fs.writeFileSync(path.join(outputDirectory, 'regression-report.json'), JSON.stringify(report, null, 2) + '\n');
console.log(`Report: ${path.join(outputDirectory, 'regression-report.json')}`);
process.exitCode = report.passed ? 0 : 1;
