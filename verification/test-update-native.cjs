#!/usr/bin/env node
// Opt-in signed native subprocess fixtures. Defaults use tiny synthetic apps;
// --fixture-app runs an isolated full Tauri app with a real WebView/ready ACK.
// Existing accounts, installed applications and published releases are untouched.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const cp = require('node:child_process');
const assert = require('node:assert/strict');
const root = path.resolve(__dirname, '..');
const option = (name) => { const at = process.argv.indexOf(name); return at < 0 ? null : process.argv[at + 1]; };
const testExecutable = option('--test-executable');
if (!testExecutable || !fs.existsSync(testExecutable)) throw new Error('Pass --test-executable <cargo test --lib unit-test EXE>.');
const tools = process.env.UM_TOOLS_ROOT || path.resolve(root, '../../Tools');
const powershell = option('--powershell') || process.env.UM_POWERSHELL || 'pwsh.exe';
const compiler = option('--inno-compiler') || path.join(tools, 'InnoSetup7', 'ISCC.exe');
const providedApp = option('--fixture-app');
const providedSetup = option('--fixture-setup');
const providedPortable = option('--fixture-portable-directory');
const actualApp = Boolean(providedApp || providedPortable);
const installerKind = providedSetup ? 'provided' : option('--installer-kind') || 'mock';
assert(['mock', 'inno', 'provided'].includes(installerKind), '--installer-kind must be mock or inno');
if (providedSetup) assert(providedPortable, '--fixture-setup requires --fixture-portable-directory with all five matching files');
const csc = option('--csharp-compiler') || (process.env.WINDIR && path.join(process.env.WINDIR, 'Microsoft.NET', 'Framework64', 'v4.0.30319', 'csc.exe'));
const privateKey = option('--private-key') || path.join(root, '.release-secrets', 'update-key.dpapi');
for (const file of [privateKey, ...(!actualApp || installerKind === 'mock' ? [csc] : []), ...(installerKind === 'inno' ? [compiler] : []), ...(providedSetup ? [providedSetup] : []), ...(providedApp ? [providedApp] : [])]) if (!file || !fs.existsSync(file)) throw new Error(`Required fixture input is missing: ${file}`);
const output = path.resolve(option('--output-directory') || path.join(__dirname, 'current', `native-update-${new Date().toISOString().replaceAll(':', '-')}-${crypto.randomUUID().slice(0, 8)}`));
fs.mkdirSync(output, {recursive: false});
const currentVersion = JSON.parse(fs.readFileSync(path.join(root, 'source', 'package.json'))).version;
const versionParts = /^(\d+)\.(\d+)\.(\d+)(?:-beta\.(\d+))?$/.exec(currentVersion);
if (!versionParts) throw new Error('Pass a source SemVer with an optional beta.N prerelease before running these fixtures.');
const version = option('--fixture-version') || (versionParts[4] ? `${versionParts[1]}.${versionParts[2]}.${versionParts[3]}-beta.${Number(versionParts[4]) + 1}` : `${versionParts[1]}.${versionParts[2]}.${Number(versionParts[3]) + 1}`);
assert(/^\d+\.\d+\.\d+(?:-beta\.\d+)?$/.test(version), 'Invalid fixture version');
const files = ['unreal-manager.exe', 'README.txt', 'LICENSE', 'THIRD_PARTY_NOTICES.md', 'THIRD-PARTY-LICENSES.txt'];
const entry = ['--ignored', '--exact', 'app_update_apply::tests::native_subprocess_entry', '--nocapture'];
const sha = (file) => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const writeJson = (file, value) => fs.writeFileSync(file, JSON.stringify(value, null, 2));
const delay = (ms) => new Promise(resolve => setTimeout(resolve, ms));
async function until(check, message, milliseconds = 10000) {
  const started = Date.now();
  while (Date.now() - started < milliseconds) { const result = check(); if (result) return result; await delay(20); }
  throw new Error(message);
}
function run(executable, args, cwd = output) {
  const result = cp.spawnSync(executable, args, {cwd, windowsHide: true, encoding: 'utf8', env: {...process.env, TEMP: output, TMP: output}, maxBuffer: 4 * 1024 * 1024});
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${path.basename(executable)} exited ${result.status}: ${result.stderr || result.stdout}`);
  return result.stdout;
}
function child(executable, args, environment, log) {
  const stdout = fs.openSync(path.join(output, `${log}.stdout.log`), 'wx');
  const stderr = fs.openSync(path.join(output, `${log}.stderr.log`), 'wx');
  const process = cp.spawn(executable, args, {windowsHide: true, cwd: path.dirname(executable), env: environment, stdio: ['ignore', stdout, stderr]});
  fs.closeSync(stdout); fs.closeSync(stderr);
  process.on('error', error => { process.fixtureError = error; });
  return process;
}
async function exit(process, timeout = 30000) {
  return until(() => { if (process.fixtureError) throw process.fixtureError; return process.exitCode !== null || process.signalCode !== null ? {code: process.exitCode, signal: process.signalCode} : null; }, `Owned fixture process ${process.pid} did not exit`, timeout);
}
function ownStop(process) { if (process && process.exitCode === null && process.signalCode === null) process.kill(); }
const report = {schema: 1, startedAt: new Date().toISOString(), purpose: 'Signed helper subprocesses with synthetic files; no real account or existing installation', installerKind, actualTauriApp: actualApp, installedBoundary: installerKind === 'mock' ? 'Real process with Inno-compatible arguments and synthetic file replacement; does not establish full Inno installation' : 'Isolated Inno fixture, not the production installed application', version, output, cases: [], passed: false};

async function main() {
  if (actualApp) {
    const fixtureBinary = providedApp || path.join(providedPortable, files[0]);
    assert(fs.readFileSync(fixtureBinary).includes(Buffer.from('UM_FULL_UPDATE_FIXTURE_DATA')), 'Full fixture app must require its isolated data-root environment variable');
    const existing = JSON.parse(run(powershell, ['-NoProfile', '-NonInteractive', '-Command', `ConvertTo-Json -InputObject @(Get-Process -Name unreal-manager -ErrorAction SilentlyContinue | Select-Object Id,ProcessName) -Compress`]) || '[]');
    assert.deepEqual(existing, [], 'Refusing to run full fixtures while an existing Unreal Manager process is running');
  }
  // Copy only signing-tool metadata and change its isolated version. The
  // production source, public key, and DPAPI-protected private key stay intact.
  const source = path.join(output, 'source'); fs.mkdirSync(path.join(source, 'src-tauri'), {recursive: true});
  for (const name of ['PREPARE-UPDATE.ps1', 'package.json', 'package-lock.json']) fs.copyFileSync(path.join(root, 'source', name), path.join(source, name));
  for (const name of ['Cargo.toml', 'tauri.conf.json', 'update-public-key.json']) fs.copyFileSync(path.join(root, 'source/src-tauri', name), path.join(source, 'src-tauri', name));
  for (const name of ['package.json', 'package-lock.json', 'src-tauri/tauri.conf.json']) {
    const file = path.join(source, name); const data = JSON.parse(fs.readFileSync(file)); data.version = version;
    if (data.packages?.['']) data.packages[''].version = version;
    writeJson(file, data);
  }
  const cargo = path.join(source, 'src-tauri/Cargo.toml'); fs.writeFileSync(cargo, fs.readFileSync(cargo, 'utf8').replace(/^version\s*=\s*"[^"]+"/m, `version = "${version}"`));
  const release = path.join(output, 'signed-fixture'); const portable = path.join(release, 'UnrealManager-Portable'); fs.mkdirSync(portable, {recursive: true});
  if (providedPortable) for (const name of files) fs.copyFileSync(path.join(providedPortable, name), path.join(portable, name));
  const program = path.join(output, 'FixtureApp.cs');
  fs.writeFileSync(program, `using System; using System.IO; using System.Reflection; using System.Text; using System.Threading;
[assembly: AssemblyTitle("Unreal Manager")]
[assembly: AssemblyProduct("Unreal Manager")]
[assembly: AssemblyVersion("0.2.0.0")]
[assembly: AssemblyFileVersion("0.2.0.0")]
[assembly: AssemblyInformationalVersion("${version}")]
class FixtureApp { static int Main(string[] args) {
  if (args.Length != 2 || args[0] != "--app-update-restarted") return 0;
  if (Environment.GetEnvironmentVariable("UM_UPDATE_FIXTURE_ACK") == "exit") return 42;
  string directory = Environment.GetEnvironmentVariable("UM_UPDATE_FIXTURE_DIRECTORY");
  string temporary = Path.Combine(directory, "healthy-fixture.tmp");
  File.WriteAllText(temporary, Environment.GetEnvironmentVariable("UM_UPDATE_FIXTURE_HEALTHY"), new UTF8Encoding(false));
  File.Move(temporary, Path.Combine(directory, "healthy.json"));
  File.WriteAllText(Path.Combine(directory, "new-started"), args[1]);
  Thread.Sleep(1000); return 0;
} }
`);
  if (providedApp) fs.copyFileSync(providedApp, path.join(portable, files[0]));
  else if (!actualApp) run(csc, ['/nologo', '/platform:x64', '/target:winexe', `/out:${path.join(portable, files[0])}`, program]);
  if (!providedPortable) for (const name of files.slice(1)) fs.writeFileSync(path.join(portable, name), `Synthetic updated ${name}; contains no real user data.\n`);
  const setupName = `UnrealManager-${version}-Windows-x64-Setup.exe`;
  const iss = path.join(output, 'fixture.iss');
  fs.writeFileSync(iss, `[Setup]
AppId=UnrealManagerUpdaterIsolatedFixture
AppName=Unreal Manager
AppVersion=${version}
VersionInfoProductTextVersion=${version}
VersionInfoVersion=0.2.0.0
DefaultDirName={tmp}\\UnrealManagerUpdaterIsolatedFixture
UsePreviousAppDir=no
AppendDefaultDirName=no
CreateAppDir=yes
Uninstallable=no
CreateUninstallRegKey=no
PrivilegesRequired=lowest
CloseApplications=no
RestartApplications=no
DisableProgramGroupPage=yes
DisableDirPage=yes
OutputDir=${release}
OutputBaseFilename=${setupName.slice(0, -4)}
Compression=lzma2
SolidCompression=yes
SetupLogging=no
[Files]
${files.map(name => `Source: "${path.join(portable, name)}"; DestDir: "{app}"; Flags: ignoreversion`).join('\n')}
`);
  if (installerKind === 'provided') fs.copyFileSync(providedSetup, path.join(release, setupName));
  else if (installerKind === 'inno') run(compiler, ['/Q', iss]);
  else {
    const setupSource = path.join(output, 'FixtureSetup.cs');
    fs.writeFileSync(setupSource, `using System; using System.IO; using System.Reflection; using System.Text; using System.Threading; using System.Diagnostics;
[assembly: AssemblyTitle("Unreal Manager")]
[assembly: AssemblyProduct("Unreal Manager")]
[assembly: AssemblyVersion("0.2.0.0")]
[assembly: AssemblyFileVersion("0.2.0.0")]
[assembly: AssemblyInformationalVersion("${version}")]
class FixtureSetup { static int Main(string[] args) { try { return Run(args); } catch(Exception error) { File.WriteAllText(Path.Combine(Environment.GetEnvironmentVariable("UM_UPDATE_FIXTURE_DIRECTORY"), "installer-fixture-error.txt"), error.ToString()); return 99; } }
static int Run(string[] args) {
  string fixture = Environment.GetEnvironmentVariable("UM_UPDATE_FIXTURE_DIRECTORY");
  if (args.Length == 1 && args[0] == "--fixture-descendant") {
    File.WriteAllText(Path.Combine(fixture, "descendant-ready"), Process.GetCurrentProcess().Id.ToString());
    for (int attempt = 0; attempt < 1200 && !File.Exists(Path.Combine(fixture, "stop-installer-fixture")); attempt++) Thread.Sleep(50);
    return 0;
  }
  string destination = null;
  foreach (string arg in args) { if (arg.StartsWith("/DIR=", StringComparison.Ordinal)) destination = arg.Substring(5); }
  if (destination == null || args.Length != 5) return 9;
  foreach (string expected in new string[] {"/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-"}) if (Array.IndexOf(args, expected) < 0) return 10;
  if (Environment.GetEnvironmentVariable("UM_UPDATE_FIXTURE_CRASH_PROBE") == "yes") {
    ProcessStartInfo start = new ProcessStartInfo(Environment.GetEnvironmentVariable("UM_UPDATE_FIXTURE_SETUP_FILE"), "--fixture-descendant"); start.UseShellExecute = false; start.CreateNoWindow = true; start.WorkingDirectory = fixture; Process.Start(start);
    File.WriteAllText(Path.Combine(fixture, "installer-ready"), Process.GetCurrentProcess().Id.ToString());
    for (int attempt = 0; attempt < 1200 && !File.Exists(Path.Combine(fixture, "stop-installer-fixture")); attempt++) Thread.Sleep(50);
    return 77;
  }
  string source = Environment.GetEnvironmentVariable("UM_UPDATE_FIXTURE_PAYLOAD_DIRECTORY");
  foreach (string name in new string[] {${files.map(name => `"${name}"`).join(', ')}}) {
    for (int attempt = 0; ; attempt++) { try { File.Copy(Path.Combine(source, name), Path.Combine(destination, name), true); break; } catch (IOException) { if (attempt >= 99) throw; Thread.Sleep(50); } }
  }
  File.WriteAllLines(Path.Combine(Environment.GetEnvironmentVariable("UM_UPDATE_FIXTURE_DIRECTORY"), "installer-arguments.txt"), args, new UTF8Encoding(false));
  return Environment.GetEnvironmentVariable("UM_UPDATE_FIXTURE_INSTALL_FAIL") == "yes" ? 6 : 0;
} }
`);
    run(csc, ['/nologo', '/platform:x64', '/target:winexe', `/out:${path.join(release, setupName)}`, setupSource]);
  }
  run(powershell, ['-NoProfile', '-NonInteractive', '-File', path.join(source, 'PREPARE-UPDATE.ps1'), '-ReleaseDirectory', release, '-PrivateKeyPath', privateKey]);
  const manifest = JSON.parse(fs.readFileSync(path.join(release, 'update-manifest.json')));
  assert.equal(manifest.version, version); assert.equal(manifest.appSha256, sha(path.join(portable, files[0])));
  report.signedFixture = {appSha256: manifest.appSha256, installedSha256: manifest.installed.sha256, portableSha256: manifest.portable.sha256};
  const closeScript = path.join(output, 'close-owned-fixture.ps1');
  fs.writeFileSync(closeScript, `param([Parameter(Mandatory)][string]$Executable,[Parameter(Mandatory)][string]$StartedAfterUtc)
$ErrorActionPreference='Stop'
function Normal([string]$Value) { if($Value.StartsWith('\\\\?\\')){$Value=$Value.Substring(4)};[IO.Path]::GetFullPath($Value) }
$target=Normal $Executable; $after=[DateTime]::Parse($StartedAfterUtc).ToUniversalTime(); $records=@()
foreach($item in @(Get-CimInstance Win32_Process -Filter "Name='unreal-manager.exe'")) {
  if(!$item.ExecutablePath -or !(Normal $item.ExecutablePath).Equals($target,[StringComparison]::OrdinalIgnoreCase) -or $item.CreationDate.ToUniversalTime() -lt $after){continue}
  $owned=Get-Process -Id $item.ProcessId; $null=$owned.Handle; $created=$owned.StartTime.ToFileTimeUtc()
  if([Math]::Abs($created-$item.CreationDate.ToFileTimeUtc()) -ge 10){throw 'Owned fixture process creation identity changed'}
  $all=@(Get-CimInstance Win32_Process);$ids=[Collections.Generic.HashSet[uint32]]::new();$null=$ids.Add([uint32]$item.ProcessId)
  do{$changed=$false;foreach($candidate in $all){if($ids.Contains([uint32]$candidate.ParentProcessId) -and $ids.Add([uint32]$candidate.ProcessId)){$changed=$true}}}while($changed)
  $browsers=@($all | Where-Object {$ids.Contains([uint32]$_.ProcessId) -and $_.Name -eq 'msedgewebview2.exe' -and $_.CommandLine -notmatch ' --type='} | ForEach-Object {@{pid=$_.ProcessId;parentPid=$_.ParentProcessId;image=$_.ExecutablePath;commandLine=$_.CommandLine;disableGpu=$_.CommandLine -match '(?:^|\\s)--disable-gpu(?:\\s|$)';disableGpuCompositing=$_.CommandLine -match '(?:^|\\s)--disable-gpu-compositing(?:\\s|$)'}})
  $closed=$false
  for($attempt=0;$attempt -lt 30 -and !$owned.HasExited -and !$closed;$attempt++){ $owned.Refresh();$closed=$owned.CloseMainWindow();if(!$closed){Start-Sleep -Milliseconds 100} }
  $forced=$false
  if(!$owned.WaitForExit(5000)){
    $again=Get-CimInstance Win32_Process -Filter "ProcessId=$($item.ProcessId)"
    if(!$again -or !$again.ExecutablePath -or !(Normal $again.ExecutablePath).Equals($target,[StringComparison]::OrdinalIgnoreCase) -or [Math]::Abs($again.CreationDate.ToFileTimeUtc()-$created) -ge 10){throw 'Refusing to stop an unverified process'}
    $owned.Kill();$forced=$true;if(!$owned.WaitForExit(5000)){throw 'Owned fixture process did not stop'}
  }
  $records+=@{pid=$item.ProcessId;image=$item.ExecutablePath;creation=$created.ToString();mainWindowClose=$closed;forced=$forced;exitCode=$owned.ExitCode;webviewBrowsers=$browsers;gpuEvidenceBoundary='Observed process configuration only; GPU activity was not measured'}
}
ConvertTo-Json -InputObject @($records) -Depth 8 -Compress
`);

  for (const scenario of [
    {name: 'portable-commit', mode: 'portable', ack: true},
    {name: 'portable-native-exit-rolls-back', mode: 'portable', ack: false},
    {name: 'installed-commit', mode: 'installed', ack: true},
    {name: 'installed-native-exit-rolls-back', mode: 'installed', ack: false},
    ...(installerKind === 'mock' ? [{name: 'installed-process-failure-rolls-back', mode: 'installed', ack: true, installFails: true}] : []),
    ...(installerKind === 'mock' ? [{name: 'helper-crash-kills-installer-tree-and-preserves-recovery', mode: 'installed', ack: false, crashProbe: true}] : []),
    {name: 'tampered-portable-refused-before-parent-exit', mode: 'portable', ack: true, tampered: true},
  ].filter(scenario => (!actualApp || scenario.name === 'portable-commit' || scenario.name === 'installed-commit' || scenario.tampered) && (!option('--case') || scenario.name === option('--case')))) {
    const item = {name: scenario.name, mode: scenario.mode, passed: false}; report.cases.push(item);
    const directory = path.join(output, scenario.name); const app = path.join(directory, 'application folder'); const job = path.join(directory, 'job');
    fs.mkdirSync(app, {recursive: true}); fs.mkdirSync(job);
    fs.mkdirSync(path.join(app, 'Data/Account'), {recursive: true}); fs.mkdirSync(path.join(app, 'Data/Tasks'), {recursive: true});
    fs.mkdirSync(path.join(app, 'Data/Settings'), {recursive: true}); fs.writeFileSync(path.join(app, 'Data/Settings/window.json'), '"exit"'); writeJson(path.join(app, 'Data/Settings/app-updates.json'), {enabled: false, channel: 'beta', lastAttempt: 0});
    fs.writeFileSync(path.join(app, 'Data/Account/session.fixture'), 'fictional preserved account'); fs.writeFileSync(path.join(app, 'Data/Tasks/task.fixture'), 'fictional preserved paused task'); fs.writeFileSync(path.join(app, 'user-project.fixture'), 'preserved project');
    if (scenario.mode === 'portable') fs.writeFileSync(path.join(app, 'portable.marker'), 'original mode marker');
    const original = path.join(app, files[0]); fs.copyFileSync(testExecutable, original); fs.copyFileSync(testExecutable, path.join(job, files[0]));
    for (const name of files.slice(1)) fs.writeFileSync(path.join(app, name), `Synthetic old ${name}.\n`);
    const oldHashes = Object.fromEntries(files.map(name => [name, sha(path.join(app, name))]));
    fs.copyFileSync(path.join(release, 'update-manifest.json'), path.join(job, 'manifest.json')); fs.copyFileSync(path.join(release, 'update-manifest.sig'), path.join(job, 'manifest.sig'));
    const payload = manifest[scenario.mode]; fs.copyFileSync(path.join(release, payload.name), path.join(job, payload.name));
    if (scenario.tampered) fs.appendFileSync(path.join(job, payload.name), 'tamper');
    const id = crypto.randomUUID();
    const environment = {...process.env, TEMP: output, TMP: output, UM_UPDATE_FIXTURE_DIRECTORY: job, UM_FULL_UPDATE_FIXTURE_DATA: path.join(app, 'Data'), UM_UPDATE_FIXTURE_MODE: 'parent', UM_UPDATE_FIXTURE_ACK: scenario.ack ? 'ready' : 'exit', UM_UPDATE_FIXTURE_HEALTHY: JSON.stringify({id, version, appSha256: manifest.appSha256}), UM_UPDATE_FIXTURE_PAYLOAD_DIRECTORY: portable, UM_UPDATE_FIXTURE_INSTALL_FAIL: scenario.installFails ? 'yes' : 'no', UM_UPDATE_FIXTURE_CRASH_PROBE: scenario.crashProbe ? 'yes' : 'no', UM_UPDATE_FIXTURE_SETUP_FILE: path.join(job, payload.name)};
    const startedAfter = new Date().toISOString();
    let parent, helper, probeGuard;
    try {
      parent = child(original, entry, environment, `${scenario.name}-parent`);
      await until(() => fs.existsSync(path.join(job, 'parent-ready')), 'Fixture parent did not start');
      const created = run(powershell, ['-NoProfile', '-NonInteractive', '-Command', `(Get-Process -Id ${parent.pid}).StartTime.ToFileTimeUtc().ToString()`]).trim();
      // Serialize FILETIME as its decimal integer rather than through a JS
      // Number, which would lose the 100 ns precision used for PID binding.
      const jobData = {schema: 1, id, parentPid: parent.pid, parentCreated: '__FILETIME__', originalExe: original, mode: scenario.mode, manifest: path.join(job, 'manifest.json'), signature: path.join(job, 'manifest.sig'), payload: path.join(job, payload.name)};
      fs.writeFileSync(path.join(job, 'job.json'), JSON.stringify(jobData, null, 2).replace('"__FILETIME__"', created));
      helper = child(path.join(job, files[0]), entry, {...environment, UM_UPDATE_FIXTURE_MODE: 'helper'}, `${scenario.name}-helper`);
      if (scenario.tampered) {
        item.helperExit = await exit(helper); assert.notEqual(item.helperExit.code, 0); assert(!fs.existsSync(path.join(job, 'ready.json'))); assert.equal(parent.exitCode, null);
        for (const name of files) assert.equal(sha(path.join(app, name)), oldHashes[name]);
        fs.writeFileSync(path.join(job, 'parent-exit'), 'exit'); await exit(parent);
      } else {
        const ready = await until(() => fs.existsSync(path.join(job, 'ready.json')) && JSON.parse(fs.readFileSync(path.join(job, 'ready.json'))), 'Native helper did not acknowledge verified readiness');
        assert.equal(ready.id, id); assert.equal(ready.helperPid, helper.pid);
        item.readyBoundToOwnedHelper = true;
        item.helperDescendantsBeforeApply = JSON.parse(run(powershell, ['-NoProfile', '-NonInteractive', '-Command', `@((Get-CimInstance Win32_Process -Filter "ParentProcessId=${helper.pid}") | Select-Object ProcessId,ParentProcessId,Name) | ConvertTo-Json -Compress`]) || '[]');
        assert(!JSON.stringify(item.helperDescendantsBeforeApply).toLowerCase().includes('webview'));
        fs.writeFileSync(path.join(job, 'parent-exit'), 'exit'); await exit(parent);
        if (scenario.crashProbe) {
          const readPid = name => { const file = path.join(job, name); return fs.existsSync(file) && Number(fs.readFileSync(file, 'utf8').trim()); };
          const installerPid = await until(() => readPid('installer-ready'), 'Owned installer did not start');
          const descendantPid = await until(() => readPid('descendant-ready'), 'Owned installer descendant did not start');
          item.ownedInstallerTreePids = [installerPid, descendantPid];
          ownStop(helper); item.helperExit = await exit(helper);
          const alive = JSON.parse(run(powershell, ['-NoProfile', '-NonInteractive', '-Command', `$until=[DateTime]::UtcNow.AddSeconds(5); do { $alive=@(Get-Process -Id ${installerPid},${descendantPid} -ErrorAction SilentlyContinue); if($alive.Count -eq 0){break}; Start-Sleep -Milliseconds 50 } while([DateTime]::UtcNow -lt $until); ConvertTo-Json -InputObject @($alive | Select-Object Id,ProcessName) -Compress`]) || '[]');
          assert.deepEqual(alive, []); item.installerAndDescendantTerminatedByJobClose = true;
          probeGuard = child(original, entry, {...environment, UM_UPDATE_FIXTURE_MODE: 'expect-interrupted-installer'}, `${scenario.name}-startup`);
          assert.equal((await exit(probeGuard)).code, 0); assert(fs.existsSync(path.join(job, 'interrupted-installer-blocked')));
          assert(fs.existsSync(path.join(app, '.um-app-update.json')));
          for (const name of files) { assert.equal(sha(path.join(app, name)), oldHashes[name]); assert.equal(sha(path.join(app, `.um-app-update-${id}`, 'backup', name)), oldHashes[name]); }
          item.interruptedInstallerBlocksAutomaticRecoveryAndRetainsBackups = true;
        } else {
        item.helperExit = await exit(helper);
        const committed = scenario.ack && !scenario.installFails;
        assert.equal(item.helperExit.code, committed ? 0 : 101);
        item.result = JSON.parse(fs.readFileSync(path.join(job, 'result.json')));
        if (scenario.mode === 'installed' && installerKind === 'mock') {
          const arguments = fs.readFileSync(path.join(job, 'installer-arguments.txt'), 'utf8').trim().split(/\r?\n/);
          assert.deepEqual(arguments.slice(0, 4), ['/SILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-']); assert.equal(arguments[4], `/DIR=${app}`);
          item.installerArgumentsAndSameDirectoryVerified = true;
          if (scenario.installFails) assert.match(item.result.error, /installer exited with 6/);
        }
        if (committed) {
          assert.equal(sha(original), manifest.appSha256); for (const name of files) assert.equal(sha(path.join(app, name)), sha(path.join(portable, name)));
          if (actualApp) { assert(fs.existsSync(path.join(job, 'healthy.json'))); item.actualNativeReadyAcknowledgement = JSON.parse(fs.readFileSync(path.join(job, 'healthy.json'))); }
          else assert(fs.existsSync(path.join(job, 'new-started')));
        } else {
          await until(() => fs.existsSync(path.join(job, 'old-restarted')), 'Restored old app was not restarted');
          for (const name of files) assert.equal(sha(path.join(app, name)), oldHashes[name]);
        }
        assert(!fs.existsSync(path.join(app, '.um-app-update.json')));
        assert(!fs.readdirSync(app).some(name => name.startsWith('.um-app-update-')));
        }
      }
      assert.equal(fs.readFileSync(path.join(app, 'Data/Account/session.fixture'), 'utf8'), 'fictional preserved account');
      assert.equal(fs.readFileSync(path.join(app, 'Data/Tasks/task.fixture'), 'utf8'), 'fictional preserved paused task');
      assert.equal(fs.readFileSync(path.join(app, 'user-project.fixture'), 'utf8'), 'preserved project');
      assert.equal(fs.existsSync(path.join(app, 'portable.marker')), scenario.mode === 'portable');
      if (scenario.mode === 'portable') assert.equal(fs.readFileSync(path.join(app, 'portable.marker'), 'utf8'), 'original mode marker');
      item.preservedSyntheticAccountTasksProjectAndMode = true; item.passed = true;
    } finally {
      fs.writeFileSync(path.join(job, 'stop-installer-fixture'), 'stop'); ownStop(probeGuard); ownStop(helper); ownStop(parent);
      if (actualApp) { const cleanup = run(powershell, ['-NoProfile', '-NonInteractive', '-File', closeScript, '-Executable', original, '-StartedAfterUtc', startedAfter]); fs.writeFileSync(path.join(job, 'owned-cleanup.json'), cleanup || '[]', {flag: 'wx'}); item.ownedAppCleanup = JSON.parse(cleanup || '[]'); }
      writeJson(path.join(output, 'native-update-report.json'), report);
    }
  }
  assert(report.cases.length > 0, 'No matching native fixture cases');
  report.passed = report.cases.every(item => item.passed);
}
main().catch(error => {report.error = error.stack; process.exitCode = 1;}).finally(() => {report.finishedAt = new Date().toISOString(); writeJson(path.join(output, 'native-update-report.json'), report); console.log(JSON.stringify({passed: report.passed, cases: report.cases.map(({name, passed}) => ({name, passed})), report: path.join(output, 'native-update-report.json'), error: report.error}, null, 2));});
